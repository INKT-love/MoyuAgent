//! Network side of the store: catalog download and verified package staging.
//! Nothing here touches the app's real plugin or skill folders; installing is a separate,
//! user-confirmed step that moves a staged directory into place.

use super::store::{
    self, CatalogItem, Inspection, MAX_CATALOG_BYTES, MAX_PACKAGE_BYTES,
};
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;

pub fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(120))
        .user_agent(concat!("MoyuAgent/", env!("CARGO_PKG_VERSION")))
        // Catalogs and packages carry no credentials, so following redirects is safe as long as
        // every hop stays on https (or loopback http) and the chain is short.
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                attempt.error("too many redirects")
            } else if store::allowed_url(attempt.url()) {
                attempt.follow()
            } else {
                attempt.error("redirect to a disallowed address")
            }
        }))
        .build()
        .map_err(|_| "无法初始化下载组件".to_owned())
}

/// Downloads `url` and fails as soon as the body exceeds `limit` bytes.
pub async fn download(client: &reqwest::Client, url: &Url, limit: usize) -> Result<Vec<u8>, String> {
    if !store::allowed_url(url) {
        return Err("下载地址必须使用 https".to_owned());
    }
    let mut response = client
        .get(url.clone())
        .send()
        .await
        .map_err(|_| "无法连接商店，请检查网络或商店地址".to_owned())?;
    if !response.status().is_success() {
        return Err(format!("商店返回了错误（HTTP {}）", response.status().as_u16()));
    }
    if response.content_length().is_some_and(|length| length > limit as u64) {
        return Err("下载内容超过大小限制".to_owned());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "下载中断".to_owned())? {
        if body.len() + chunk.len() > limit {
            return Err("下载内容超过大小限制".to_owned());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub async fn fetch_catalog(client: &reqwest::Client, url: &Url) -> Result<Vec<CatalogItem>, String> {
    let bytes = download(client, url, MAX_CATALOG_BYTES).await?;
    let text = String::from_utf8(bytes).map_err(|_| "商店目录不是有效的文本".to_owned())?;
    store::parse_catalog(&text, url)
}

#[derive(Debug)]
pub struct Staged {
    pub directory: PathBuf,
    pub inspection: Inspection,
}

/// Downloads, verifies the SHA-256 from the catalog, extracts and inspects a package.
/// The directory is removed again on any failure.
pub async fn stage(
    client: &reqwest::Client,
    item: &CatalogItem,
    staging_root: &Path,
) -> Result<Staged, String> {
    let url = Url::parse(&item.url).map_err(|_| "下载地址无效".to_owned())?;
    let bytes = download(client, &url, MAX_PACKAGE_BYTES).await?;
    if store::sha256_hex(&bytes) != item.sha256 {
        return Err("安装包校验失败，内容和商店目录里登记的不一致".to_owned());
    }
    let directory = staging_root.join(uuid::Uuid::new_v4().simple().to_string());
    let (kind, id, expected) = (item.kind, item.id.clone(), directory.clone());
    let outcome = tokio::task::spawn_blocking(move || {
        store::extract_zip(&bytes, &expected)?;
        store::inspect_dir(kind, &expected, &id)
    })
    .await
    .map_err(|_| "解压失败".to_owned())?;
    match outcome {
        Ok(mut inspection) => {
            if inspection.version.is_empty() {
                inspection.version = item.version.clone();
            }
            Ok(Staged { directory, inspection })
        }
        Err(error) => {
            let _ = tokio::fs::remove_dir_all(&directory).await;
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Serves `routes` (path to status, headers, body) on a loopback port.
    async fn serve(routes: Vec<(&'static str, u16, Vec<(String, String)>, Vec<u8>)>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { return };
                let routes = routes.clone();
                tokio::spawn(async move {
                    let mut buffer = vec![0u8; 4096];
                    let read = socket.read(&mut buffer).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
                    let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
                    let found = routes.iter().find(|route| route.0 == path);
                    let (status, headers, body) = match found {
                        Some((_, status, headers, body)) => (*status, headers.clone(), body.clone()),
                        None => (404, Vec::new(), b"missing".to_vec()),
                    };
                    let mut head = format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
                    for (name, value) in headers {
                        head.push_str(&format!("{name}: {value}\r\n"));
                    }
                    head.push_str("\r\n");
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                });
            }
        });
        port
    }

    fn plugin_zip(id: &str) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("plugin.json", options).unwrap();
        writer.write_all(format!(r#"{{"id":"{id}","name":"Demo","version":"1.0.0"}}"#).as_bytes()).unwrap();
        writer.finish().unwrap().into_inner()
    }

    fn item(port: u16, path: &str, bytes: &[u8]) -> CatalogItem {
        CatalogItem {
            id: "demo".into(),
            kind: store::Kind::Plugin,
            name: "Demo".into(),
            description: "d".into(),
            version: "1.0.0".into(),
            author: String::new(),
            tags: Vec::new(),
            url: format!("http://127.0.0.1:{port}{path}"),
            sha256: store::sha256_hex(bytes),
            size: None,
        }
    }

    #[tokio::test]
    async fn stage_downloads_verifies_and_inspects() {
        let bytes = plugin_zip("demo");
        let port = serve(vec![("/demo.zip", 200, Vec::new(), bytes.clone())]).await;
        let root = tempfile::tempdir().unwrap();
        let staged = stage(&client().unwrap(), &item(port, "/demo.zip", &bytes), root.path()).await.unwrap();
        assert_eq!(staged.inspection.id, "demo");
        assert!(staged.directory.join("plugin.json").is_file());
    }

    #[tokio::test]
    async fn stage_rejects_checksum_mismatch_and_leaves_nothing_behind() {
        let bytes = plugin_zip("demo");
        let port = serve(vec![("/demo.zip", 200, Vec::new(), bytes.clone())]).await;
        let root = tempfile::tempdir().unwrap();
        let mut wrong = item(port, "/demo.zip", &bytes);
        wrong.sha256 = "0".repeat(64);
        let error = stage(&client().unwrap(), &wrong, root.path()).await.unwrap_err();
        assert!(error.contains("校验失败"));
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn stage_rejects_package_for_a_different_id() {
        let bytes = plugin_zip("other");
        let port = serve(vec![("/demo.zip", 200, Vec::new(), bytes.clone())]).await;
        let root = tempfile::tempdir().unwrap();
        let error = stage(&client().unwrap(), &item(port, "/demo.zip", &bytes), root.path()).await.unwrap_err();
        assert!(error.contains("不一致"));
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0, "failed staging is cleaned up");
    }

    #[tokio::test]
    async fn download_follows_safe_redirects_only() {
        let port = serve(vec![
            ("/a", 302, vec![("Location".into(), "/b".into())], Vec::new()),
            ("/b", 200, Vec::new(), b"ok".to_vec()),
            ("/evil", 302, vec![("Location".into(), "ftp://example.com/x".into())], Vec::new()),
            ("/loop", 302, vec![("Location".into(), "/loop".into())], Vec::new()),
        ])
        .await;
        let base = format!("http://127.0.0.1:{port}");
        let client = client().unwrap();
        let body = download(&client, &Url::parse(&format!("{base}/a")).unwrap(), 100).await.unwrap();
        assert_eq!(body, b"ok");
        assert!(download(&client, &Url::parse(&format!("{base}/evil")).unwrap(), 100).await.is_err());
        assert!(download(&client, &Url::parse(&format!("{base}/loop")).unwrap(), 100).await.is_err());
    }

    #[tokio::test]
    async fn download_enforces_limit_and_status() {
        let port = serve(vec![("/big", 200, Vec::new(), vec![1u8; 2048])]).await;
        let client = client().unwrap();
        let big = Url::parse(&format!("http://127.0.0.1:{port}/big")).unwrap();
        assert!(download(&client, &big, 1024).await.unwrap_err().contains("大小限制"));
        assert_eq!(download(&client, &big, 4096).await.unwrap().len(), 2048);
        let missing = Url::parse(&format!("http://127.0.0.1:{port}/nope")).unwrap();
        assert!(download(&client, &missing, 1024).await.unwrap_err().contains("404"));
        assert!(download(&client, &Url::parse("http://example.com/x").unwrap(), 10).await.is_err());
    }

    #[tokio::test]
    async fn fetch_catalog_parses_items_relative_to_the_catalog_address() {
        let bytes = plugin_zip("demo");
        let catalog = serde_json::json!({ "items": [{
            "id": "demo", "kind": "plugin", "name": "Demo", "description": "d",
            "version": "1.0.0", "url": "pkg/demo.zip", "sha256": store::sha256_hex(&bytes)
        }]})
        .to_string();
        let port = serve(vec![("/store/index.json", 200, Vec::new(), catalog.into_bytes())]).await;
        let url = Url::parse(&format!("http://127.0.0.1:{port}/store/index.json")).unwrap();
        let items = fetch_catalog(&client().unwrap(), &url).await.unwrap();
        assert_eq!(items[0].url, format!("http://127.0.0.1:{port}/store/pkg/demo.zip"));
    }
}
