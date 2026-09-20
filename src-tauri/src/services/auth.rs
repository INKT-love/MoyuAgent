use std::collections::HashSet;
use std::sync::Arc;

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri_plugin_secure_store::{delete_secret, get_secret, set_secret};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::api_client::{ApiClient, UNAUTHORIZED};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: i64,
    pub name: String,
    pub email: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub platform: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupModel {
    pub id: String,
    pub name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResult {
    pub user: Option<User>,
    pub requires_two_factor: bool,
    pub temp_token: Option<String>,
}

pub struct AuthService {
    api: Arc<ApiClient>,
}

#[derive(Serialize, Deserialize)]
struct PendingKey {
    key: String,
    idempotency: String,
}

impl AuthService {
    pub fn new(api: Arc<ApiClient>) -> Self {
        Self { api }
    }

    pub async fn public_settings(&self) -> Result<Value, String> {
        self.api
            .request_json(Method::GET, "/api/v1/settings/public", None, None, None)
            .await
    }

    pub async fn login(
        &self,
        email: String,
        password: String,
        totp_code: Option<String>,
        temp_token: Option<String>,
    ) -> Result<LoginResult, String> {
        let password = Zeroizing::new(password);
        let (path, body) = if let Some(temp_token) = temp_token {
            let code =
                totp_code.ok_or_else(|| "Enter the six-digit authentication code".to_owned())?;
            if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err("Enter a valid six-digit authentication code".to_owned());
            }
            (
                "/api/v1/auth/login/2fa",
                json!({ "temp_token": temp_token, "totp_code": code }),
            )
        } else {
            if email.trim().is_empty() || password.is_empty() {
                return Err("Email and password are required".to_owned());
            }
            (
                "/api/v1/auth/login",
                json!({ "email": email.trim(), "password": password.as_str() }),
            )
        };
        let payload = self
            .api
            .request_json(Method::POST, path, None, Some(body), None)
            .await?;
        if payload.get("requires_2fa").and_then(Value::as_bool) == Some(true) {
            let temp_token = nonempty_string(&payload, "temp_token")?;
            return Ok(LoginResult {
                user: None,
                requires_two_factor: true,
                temp_token: Some(temp_token),
            });
        }
        let user = parse_user(
            payload
                .get("user")
                .ok_or_else(|| "Login response is missing the user profile".to_owned())?,
        )?;
        self.save_tokens(&payload).await?;
        Ok(LoginResult {
            user: Some(user),
            requires_two_factor: false,
            temp_token: None,
        })
    }

    async fn save_tokens(&self, payload: &Value) -> Result<(), String> {
        let token = nonempty_string(payload, "access_token")?;
        match payload
            .get("refresh_token")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            Some(refresh) => set_secret("auth-refresh", refresh.to_owned()).await?,
            None => delete_secret("auth-refresh").await?,
        }
        set_secret("auth-token", token).await
    }

    async fn authorized(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        idempotency_key: Option<&str>,
    ) -> Result<Value, String> {
        let token = Zeroizing::new(
            get_secret("auth-token")
                .await?
                .ok_or_else(|| "Sign in before continuing".to_owned())?,
        );
        let result = self
            .api
            .request_json(
                method.clone(),
                path,
                Some(token.as_str()),
                body.clone(),
                idempotency_key,
            )
            .await;
        if !matches!(&result, Err(error) if error == UNAUTHORIZED) {
            return result;
        }
        let refresh = match get_secret("auth-refresh").await? {
            Some(refresh) => Zeroizing::new(refresh),
            None => return result,
        };
        let payload = self
            .api
            .request_json(
                Method::POST,
                "/api/v1/auth/refresh",
                None,
                Some(json!({ "refresh_token": refresh.as_str() })),
                None,
            )
            .await?;
        self.save_tokens(&payload).await?;
        let token = Zeroizing::new(nonempty_string(&payload, "access_token")?);
        self.api
            .request_json(method, path, Some(token.as_str()), body, idempotency_key)
            .await
    }

    pub async fn current_user(&self) -> Result<Option<User>, String> {
        if get_secret("auth-token").await?.is_none() {
            return Ok(None);
        }
        let payload = match self
            .authorized(Method::GET, "/api/v1/auth/me", None, None)
            .await
        {
            Ok(payload) => payload,
            Err(error) if error == UNAUTHORIZED => {
                delete_secret("auth-token").await?;
                delete_secret("auth-refresh").await?;
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        Ok(Some(parse_user(&payload)?))
    }

    pub async fn logout(&self) -> Result<(), String> {
        if let Ok(Some(refresh)) = get_secret("auth-refresh").await {
            let refresh = Zeroizing::new(refresh);
            let _ = self
                .api
                .request_json(
                    Method::POST,
                    "/api/v1/auth/logout",
                    None,
                    Some(json!({ "refresh_token": refresh.as_str() })),
                    None,
                )
                .await;
        }
        let access_result = delete_secret("auth-token").await;
        let refresh_result = delete_secret("auth-refresh").await;
        access_result.and(refresh_result)
    }

    pub async fn groups(&self) -> Result<Vec<Group>, String> {
        let payload = self
            .authorized(Method::GET, "/api/v1/groups/available", None, None)
            .await?;
        parse_groups(&payload)
    }

    pub async fn ensure_api_key(&self, group_id: i64, user_id: i64) -> Result<String, String> {
        if group_id <= 0 || user_id <= 0 {
            return Err("A valid account and group are required".to_owned());
        }
        let secret_name = api_key_secret_name(user_id, group_id);
        if let Some(key) = get_secret(&secret_name).await? {
            return Ok(key);
        }
        let pending_name = format!("{secret_name}-pending");
        // Persist the candidate before creating it so an ambiguous network failure is recoverable.
        let pending: PendingKey = match get_secret(&pending_name).await? {
            Some(pending) => serde_json::from_str(&pending)
                .map_err(|_| "The pending API key credential is invalid".to_owned())?,
            None => {
                let pending = PendingKey {
                    key: format!(
                        "sk-moyu-{}{}",
                        Uuid::new_v4().simple(),
                        Uuid::new_v4().simple()
                    ),
                    idempotency: Uuid::new_v4().to_string(),
                };
                let encoded = serde_json::to_string(&pending)
                    .map_err(|_| "Cannot encode the pending API key credential".to_owned())?;
                set_secret(&pending_name, encoded).await?;
                pending
            }
        };
        let candidate = Zeroizing::new(pending.key);
        let name = format!("MoyuAgent-{user_id}-{group_id}");
        let result = self
            .authorized(
                Method::POST,
                "/api/v1/keys",
                Some(json!({
                    "name": name, "group_id": group_id, "custom_key": candidate.as_str()
                })),
                Some(&pending.idempotency),
            )
            .await;
        let key = match result {
            Ok(payload) => {
                if payload.get("group_id").and_then(Value::as_i64) != Some(group_id) {
                    return Err("The created API key is bound to an unexpected group".to_owned());
                }
                nonempty_string(&payload, "key")?
            }
            Err(error) if error.contains("HTTP 409") => self
                .find_created_key(&name, group_id, candidate.as_str())
                .await?
                .ok_or_else(|| {
                    "The API key already exists but could not be verified for this account"
                        .to_owned()
                })?,
            Err(error) => return Err(error),
        };
        set_secret(&secret_name, key.clone()).await?;
        delete_secret(&pending_name).await?;
        Ok(key)
    }

    pub async fn group_models(
        &self,
        group_id: i64,
        user_id: i64,
    ) -> Result<Vec<GroupModel>, String> {
        let key = Zeroizing::new(self.ensure_api_key(group_id, user_id).await?);
        let payload = self
            .api
            .request_json(Method::GET, "/v1/models", Some(key.as_str()), None, None)
            .await?;
        Ok(parse_model_list(&payload))
    }

    async fn find_created_key(
        &self,
        name: &str,
        group_id: i64,
        candidate: &str,
    ) -> Result<Option<String>, String> {
        let query: String = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("search", name)
            .finish();
        for page in 1..=10 {
            let path = format!("/api/v1/keys?page={page}&page_size=100&{query}");
            let payload = self.authorized(Method::GET, &path, None, None).await?;
            let items = payload
                .get("items")
                .and_then(Value::as_array)
                .ok_or_else(|| "The API returned an invalid key list".to_owned())?;
            for item in items {
                if item.get("group_id").and_then(Value::as_i64) == Some(group_id)
                    && item.get("name").and_then(Value::as_str) == Some(name)
                    && item.get("key").and_then(Value::as_str) == Some(candidate)
                    && item.get("status").and_then(Value::as_str) == Some("active")
                {
                    return Ok(Some(candidate.to_owned()));
                }
            }
            if items.len() < 100
                || payload
                    .get("pages")
                    .and_then(Value::as_u64)
                    .is_some_and(|pages| page >= pages)
            {
                break;
            }
        }
        Ok(None)
    }
}

pub fn api_key_secret_name(user_id: i64, group_id: i64) -> String {
    format!("api-key-{user_id}-{group_id}")
}

fn parse_groups(payload: &Value) -> Result<Vec<Group>, String> {
    let values = payload
        .as_array()
        .or_else(|| payload.get("items").and_then(Value::as_array))
        .or_else(|| payload.get("groups").and_then(Value::as_array))
        .ok_or_else(|| "The API returned an invalid group list".to_owned())?;
    Ok(values.iter().filter_map(parse_group).collect())
}

fn parse_group(value: &Value) -> Option<Group> {
    if group_is_inactive(value) {
        return None;
    }
    let id = value.get("id").and_then(|id| {
        id.as_i64()
            .or_else(|| id.as_u64().and_then(|id| i64::try_from(id).ok()))
            .or_else(|| id.as_str().and_then(|id| id.parse().ok()))
    })?;
    if id <= 0 {
        return None;
    }
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())?
        .to_owned();
    let platform = value
        .get("platform")
        .or_else(|| value.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    Some(Group { id, name, platform })
}

fn group_is_inactive(value: &Value) -> bool {
    match value.get("status") {
        Some(Value::String(status)) => {
            status.eq_ignore_ascii_case("inactive") || status.eq_ignore_ascii_case("disabled")
        }
        Some(Value::Number(status)) => status.as_i64() == Some(0),
        _ => false,
    }
}

fn nonempty_string(value: &Value, field: &str) -> Result<String, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("The API response is missing {field}"))
}

const MAX_GROUP_MODELS: usize = 300;

fn valid_model_id(id: &str) -> bool {
    let id = id.trim();
    !id.is_empty()
        && id.len() <= 160
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._:/".contains(&byte))
}

fn model_from_value(value: &Value) -> Option<GroupModel> {
    match value {
        Value::String(id) if valid_model_id(id) => Some(GroupModel {
            id: id.clone(),
            name: id.clone(),
        }),
        Value::Object(_) => {
            let raw = value
                .get("id")
                .and_then(Value::as_str)
                .or_else(|| value.get("name").and_then(Value::as_str))?;
            let id = raw.strip_prefix("models/").unwrap_or(raw).trim();
            if !valid_model_id(id) {
                return None;
            }
            let name = value
                .get("display_name")
                .or_else(|| value.get("displayName"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or(id)
                .to_owned();
            Some(GroupModel {
                id: id.to_owned(),
                name,
            })
        }
        _ => None,
    }
}

fn parse_model_list(value: &Value) -> Vec<GroupModel> {
    let items = value.as_array().map(|items| items.as_slice()).or_else(|| {
        ["data", "models"].into_iter().find_map(|key| {
            value
                .get(key)
                .and_then(Value::as_array)
                .map(|items| items.as_slice())
        })
    });
    let Some(items) = items else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut models = Vec::new();
    for item in items {
        let Some(model) = model_from_value(item) else {
            continue;
        };
        if seen.insert(model.id.clone()) {
            models.push(model);
        }
        if models.len() == MAX_GROUP_MODELS {
            break;
        }
    }
    models
}

fn parse_user(value: &Value) -> Result<User, String> {
    let id = value
        .get("id")
        .and_then(Value::as_i64)
        .filter(|id| *id > 0)
        .ok_or_else(|| "The API returned an invalid user ID".to_owned())?;
    let email = nonempty_string(value, "email")?;
    let name = value
        .get("username")
        .or_else(|| value.get("name"))
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(&email)
        .to_owned();
    Ok(User { id, name, email })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires an unlocked operating system credential vault"]
    async fn operating_system_vault_roundtrip() {
        let name = format!("moyu-verification-{}", Uuid::new_v4());
        set_secret(&name, "local-verification-not-a-real-token".into())
            .await
            .unwrap();
        let loaded = get_secret(&name).await;
        delete_secret(&name).await.unwrap();
        assert_eq!(
            loaded.unwrap().as_deref(),
            Some("local-verification-not-a-real-token")
        );
        assert!(get_secret(&name).await.unwrap().is_none());
    }

    #[test]
    fn parses_sub2api_user_without_display_name() {
        let user =
            parse_user(&json!({ "id": 17, "email": "someone@example.test", "username": null }))
                .unwrap();
        assert_eq!(user.name, "someone@example.test");
        assert_eq!(user.id, 17);
    }

    #[test]
    fn rejects_missing_or_empty_credentials_in_responses() {
        assert!(nonempty_string(&json!({ "access_token": " " }), "access_token").is_err());
        assert!(nonempty_string(&json!({ "token": "unsupported" }), "access_token").is_err());
    }

    #[test]
    fn parses_openai_and_anthropic_model_lists() {
        let openai = parse_model_list(&json!({
            "object": "list",
            "data": [
                { "id": "gpt-5.2", "object": "model" },
                { "id": "gpt-5.2" },
                { "id": "text-embedding-3-large" }
            ]
        }));
        assert_eq!(
            openai
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["gpt-5.2", "text-embedding-3-large"]
        );

        let anthropic = parse_model_list(&json!({
            "data": [
                { "id": "claude-sonnet-4-6", "display_name": "Claude Sonnet 4.6", "type": "model" },
                { "id": "not a model" }
            ]
        }));
        assert_eq!(
            anthropic,
            [GroupModel {
                id: "claude-sonnet-4-6".into(),
                name: "Claude Sonnet 4.6".into(),
            }]
        );
    }

    #[test]
    fn parse_groups_keeps_every_active_sub2api_platform() {
        let groups = parse_groups(&json!([
            { "id": 37, "name": "限时福利", "platform": "openai", "status": "active" },
            { "id": "8", "name": "Gemini", "platform": "gemini", "status": "active" },
            { "id": 9, "name": "Kimi", "type": "kimi", "status": "active" },
            { "id": 10, "name": "Old", "platform": "openai", "status": "inactive" },
            { "id": 0, "name": "Invalid", "platform": "openai", "status": "active" },
            { "name": "Missing id", "platform": "openai", "status": "active" }
        ]))
        .unwrap();
        assert_eq!(
            groups
                .iter()
                .map(|group| (group.id, group.name.as_str(), group.platform.as_str()))
                .collect::<Vec<_>>(),
            [
                (37, "限时福利", "openai"),
                (8, "Gemini", "gemini"),
                (9, "Kimi", "kimi")
            ]
        );
    }

    #[test]
    fn parses_gemini_names_and_plain_id_arrays() {
        let gemini = parse_model_list(&json!({
            "models": [{ "name": "models/gemini-2.5-pro", "displayName": "Gemini 2.5 Pro" }]
        }));
        assert_eq!(
            gemini,
            [GroupModel {
                id: "gemini-2.5-pro".into(),
                name: "Gemini 2.5 Pro".into(),
            }]
        );
        assert_eq!(
            parse_model_list(&json!(["claude-opus-4-6", "claude-opus-4-6"])),
            [GroupModel {
                id: "claude-opus-4-6".into(),
                name: "claude-opus-4-6".into(),
            }]
        );
    }
}
