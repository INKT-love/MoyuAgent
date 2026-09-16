use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde_json::Value;

const BASE_URLS: [&str; 2] = ["https://inktandwkx.top", "https://inkaicf.flymiku.top"];
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
pub const UNAUTHORIZED: &str = "Authentication expired or credentials were rejected (HTTP 401)";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    pub index: usize,
    pub name: String,
    pub base_url: String,
}

pub fn endpoints() -> Vec<Endpoint> {
    BASE_URLS
        .iter()
        .enumerate()
        .map(|(index, url)| Endpoint {
            index,
            name: if index == 0 { "Primary" } else { "Backup (CF)" }.to_owned(),
            base_url: (*url).to_owned(),
        })
        .collect()
}

pub struct ApiClient {
    client: reqwest::Client,
    current: AtomicUsize,
    base_urls: [&'static str; 2],
}

impl ApiClient {
    pub fn new(index: usize) -> Result<Self, String> {
        Self::with_urls(index, BASE_URLS)
    }

    fn with_urls(index: usize, base_urls: [&'static str; 2]) -> Result<Self, String> {
        if index >= base_urls.len() {
            return Err("Invalid API endpoint index".to_owned());
        }
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(30))
            // Never forward authentication or login bodies to a redirected host.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "Cannot initialize the API client".to_owned())?;
        Ok(Self {
            client,
            current: AtomicUsize::new(index),
            base_urls,
        })
    }

    pub fn get_current_base_url(&self) -> &'static str {
        self.base_urls[self.current.load(Ordering::Acquire)]
    }

    pub fn endpoint(&self) -> Endpoint {
        let index = self.current.load(Ordering::Acquire);
        Endpoint {
            index,
            name: if index == 0 { "Primary" } else { "Backup (CF)" }.to_owned(),
            base_url: self.base_urls[index].to_owned(),
        }
    }

    pub fn switch_endpoint(&self, index: Option<usize>) -> Result<Endpoint, String> {
        match index {
            Some(index) if index < self.base_urls.len() => {
                self.current.store(index, Ordering::Release)
            }
            Some(_) => return Err("Invalid API endpoint index".to_owned()),
            None => {
                self.current.fetch_xor(1, Ordering::AcqRel);
            }
        }
        Ok(self.endpoint())
    }

    pub async fn request_json(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
        idempotency_key: Option<&str>,
    ) -> Result<Value, String> {
        if !path.starts_with('/') || path.starts_with("//") || path.contains(['\\', '#']) {
            return Err("Invalid API request path".to_owned());
        }
        let first = self.current.load(Ordering::Acquire);
        for attempt in 0..2 {
            let index = (first + attempt) % self.base_urls.len();
            let mut request = self
                .client
                .request(method.clone(), format!("{}{}", self.base_urls[index], path));
            if let Some(token) = token {
                request = request.bearer_auth(token);
            }
            if let Some(body) = body.as_ref() {
                request = request.json(body);
            }
            if let Some(key) = idempotency_key {
                request = request.header("Idempotency-Key", key);
            }
            match request.send().await {
                Ok(mut response) => {
                    let status = response.status();
                    if attempt == 0 && matches!(status.as_u16(), 502..=504) {
                        self.failover(first);
                        continue;
                    }
                    if !status.is_success() {
                        return Err(safe_http_error(status));
                    }
                    if response
                        .content_length()
                        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
                    {
                        return Err("API response exceeded the size limit".to_owned());
                    }
                    let mut bytes = Vec::new();
                    let mut read_failed = false;
                    loop {
                        match response.chunk().await {
                            Ok(Some(chunk)) => {
                                if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                                    return Err("API response exceeded the size limit".to_owned());
                                }
                                bytes.extend_from_slice(&chunk);
                            }
                            Ok(None) => break,
                            Err(_) => {
                                read_failed = true;
                                break;
                            }
                        }
                    }
                    if read_failed {
                        if attempt == 0 {
                            self.failover(first);
                            continue;
                        }
                        return Err(
                            "The API connection closed before the response was complete".to_owned()
                        );
                    }
                    let value: Value = serde_json::from_slice(&bytes)
                        .map_err(|_| "The API returned an invalid JSON response".to_owned())?;
                    if let Some(code) = value.get("code") {
                        if code.as_i64() != Some(0) && code.as_str() != Some("0") {
                            return Err("The API rejected the request; check account permissions and settings".to_owned());
                        }
                        return value.get("data").cloned().ok_or_else(|| {
                            "The API response is missing its data field".to_owned()
                        });
                    }
                    return Ok(value);
                }
                Err(error) => {
                    if attempt == 0
                        && (error.is_connect() || error.is_timeout() || error.is_request())
                    {
                        self.failover(first);
                        continue;
                    }
                    return Err(
                        "Cannot reach the API endpoints; check the network connection".to_owned(),
                    );
                }
            }
        }
        Err("Both API endpoints are unavailable".to_owned())
    }

    fn failover(&self, failed: usize) {
        // A request that started earlier must not override a manual switch.
        let _ =
            self.current
                .compare_exchange(failed, 1 - failed, Ordering::AcqRel, Ordering::Acquire);
    }
}

fn safe_http_error(status: StatusCode) -> String {
    match status.as_u16() {
        400 => "The API rejected the request parameters (HTTP 400)".to_owned(),
        401 => UNAUTHORIZED.to_owned(),
        403 => "The account does not have permission for this operation (HTTP 403)".to_owned(),
        404 => "The requested API is unavailable on this server version (HTTP 404)".to_owned(),
        409 => "The requested resource already exists (HTTP 409)".to_owned(),
        429 => "The API rate limit was reached; try again later (HTTP 429)".to_owned(),
        _ => format!("The API request failed (HTTP {})", status.as_u16()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    async fn server(
        status: u16,
        body: &'static str,
    ) -> (&'static str, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Box::leak(format!("http://{}", listener.local_addr().unwrap()).into_boxed_str());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            loop {
                let mut bytes = [0; 4096];
                let count = socket.read(&mut bytes).await.unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&bytes[..count]);
                if let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n")
                {
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    if request.len() >= header_end + 4 + length {
                        break;
                    }
                }
                assert!(request.len() < 8192, "test request exceeded limit");
            }
            let request = String::from_utf8_lossy(&request).into_owned();
            let response = format!("HTTP/1.1 {status} Status\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/json\r\n\r\n{body}", body.len());
            socket.write_all(response.as_bytes()).await.unwrap();
            request
        });
        (url, task)
    }

    #[tokio::test]
    async fn retries_gateway_failure_once_and_preserves_idempotency() {
        let (primary, first) = server(503, "{}").await;
        let (backup, second) = server(200, r#"{"code":0,"data":{"id":42}}"#).await;
        let api = ApiClient::with_urls(0, [primary, backup]).unwrap();
        let result = api
            .request_json(
                Method::POST,
                "/api/v1/keys",
                None,
                Some(serde_json::json!({"name":"test"})),
                Some("fixed-request-id"),
            )
            .await
            .unwrap();
        assert_eq!(result["id"], 42);
        assert_eq!(api.endpoint().index, 1);
        assert!(first
            .await
            .unwrap()
            .to_ascii_lowercase()
            .contains("idempotency-key: fixed-request-id"));
        assert!(second
            .await
            .unwrap()
            .to_ascii_lowercase()
            .contains("idempotency-key: fixed-request-id"));
    }

    #[tokio::test]
    async fn unauthorized_does_not_failover_or_expose_server_body() {
        let (primary, first) =
            server(401, r#"{"code":"UNAUTHORIZED","message":"secret-value"}"#).await;
        let api = ApiClient::with_urls(0, [primary, "http://127.0.0.1:1"]).unwrap();
        let error = api
            .request_json(Method::GET, "/api/v1/auth/me", None, None, None)
            .await
            .unwrap_err();
        assert_eq!(error, UNAUTHORIZED);
        assert!(!error.contains("secret-value"));
        assert_eq!(api.endpoint().index, 0);
        first.await.unwrap();
    }

    #[tokio::test]
    async fn connection_failure_uses_backup() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let unavailable =
            Box::leak(format!("http://{}", listener.local_addr().unwrap()).into_boxed_str());
        drop(listener);
        let (backup, task) = server(200, r#"{"code":0,"data":[]}"#).await;
        let api = ApiClient::with_urls(0, [unavailable, backup]).unwrap();
        assert!(api
            .request_json(Method::GET, "/groups", None, None, None)
            .await
            .unwrap()
            .is_array());
        assert_eq!(api.endpoint().index, 1);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn two_unavailable_endpoints_stop_after_one_retry() {
        let (primary, first) = server(503, "{}").await;
        let (backup, second) = server(503, "{}").await;
        let api = ApiClient::with_urls(0, [primary, backup]).unwrap();
        let error = api
            .request_json(Method::GET, "/api/v1/groups/available", None, None, None)
            .await
            .unwrap_err();
        assert!(error.contains("HTTP 503"));
        first.await.unwrap();
        second.await.unwrap();
    }
}
