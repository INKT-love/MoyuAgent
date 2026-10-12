use super::config::{self, Preferences};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::Path;
use tauri::AppHandle;
use tauri_plugin_secure_store::{delete_secret, get_secret, set_secret};

const MAX_SETTINGS_BYTES: usize = 16 * 1024;
const MAX_SECRET_BYTES: usize = 8 * 1024;
const MAX_KEYS: usize = 32;
const SPM_ID: &str = "spm";
const SPM_SECRET: &str = "apiKey";
const SPM_ENV: &str = "MOYU_PLUGIN_SPM_API_KEY";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginApplied {
    pub persisted: bool,
    pub instructions: bool,
    pub mcp: bool,
    pub proxy: bool,
    pub details: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSettingsView {
    pub id: String,
    pub values: Value,
    pub secrets: HashMap<String, bool>,
    pub applied: PluginApplied,
}

pub fn state_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(config::app_directory(app)?
        .join("harness")
        .join("plugin-state"))
}

pub fn secret_name(user_id: i64, plugin_id: &str, key: &str) -> String {
    format!("plugin-{user_id}-{plugin_id}-{key}")
}

pub fn valid_settings_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic())
        && key.len() <= 40
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn is_secret_key(key: &str) -> bool {
    matches!(
        key,
        "apiKey" | "api_key" | "token" | "secret" | "password" | "authorization"
    )
}

pub fn sanitize_values(value: Value) -> Result<Value, String> {
    let mut object = match value {
        Value::Object(object) => object,
        Value::Null => Map::new(),
        _ => return Err("Plugin settings must be an object".into()),
    };
    if object.len() > MAX_KEYS {
        return Err("Plugin settings exceeded the size limit".into());
    }
    object.retain(|key, _| valid_settings_key(key) && !is_secret_key(key));
    let sanitized = Value::Object(object);
    let encoded =
        serde_json::to_vec(&sanitized).map_err(|_| "Plugin settings must be an object")?;
    if encoded.len() > MAX_SETTINGS_BYTES {
        return Err("Plugin settings exceeded the size limit".into());
    }
    Ok(sanitized)
}

pub fn sanitize_secrets(value: Option<Value>) -> Result<Vec<(String, Option<String>)>, String> {
    let Some(Value::Object(object)) = value else {
        return Ok(Vec::new());
    };
    if object.len() > 8 {
        return Err("Plugin settings exceeded the size limit".into());
    }
    let mut secrets = Vec::new();
    for (key, item) in object {
        if !valid_settings_key(&key) {
            return Err("Invalid plugin secret name".into());
        }
        match item {
            Value::Null => secrets.push((key, None)),
            Value::String(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    secrets.push((key, None));
                } else if trimmed.len() > MAX_SECRET_BYTES {
                    return Err("Plugin settings exceeded the size limit".into());
                } else {
                    secrets.push((key, Some(trimmed.to_owned())));
                }
            }
            _ => return Err("Plugin secrets must be strings".into()),
        }
    }
    Ok(secrets)
}

pub fn settings_for<'a>(preferences: &'a Preferences, user_id: i64, plugin_id: &str) -> Value {
    preferences
        .plugin_settings
        .get(&user_id.to_string())
        .and_then(|plugins| plugins.get(plugin_id))
        .cloned()
        .unwrap_or_else(|| json!({}))
}

pub fn secret_names_for<'a>(
    preferences: &'a Preferences,
    user_id: i64,
    plugin_id: &str,
) -> Vec<String> {
    preferences
        .plugin_secret_names
        .get(&user_id.to_string())
        .and_then(|plugins| plugins.get(plugin_id))
        .cloned()
        .unwrap_or_default()
}

pub fn put_settings(
    preferences: &mut Preferences,
    user_id: i64,
    plugin_id: &str,
    values: Value,
    secret_names: Vec<String>,
) {
    let user = user_id.to_string();
    preferences
        .plugin_settings
        .entry(user.clone())
        .or_default()
        .insert(plugin_id.to_owned(), values);
    preferences
        .plugin_secret_names
        .entry(user)
        .or_default()
        .insert(plugin_id.to_owned(), secret_names);
}

pub fn remove_plugin(preferences: &mut Preferences, plugin_id: &str) {
    for plugins in preferences.plugin_settings.values_mut() {
        plugins.remove(plugin_id);
    }
    for plugins in preferences.plugin_secret_names.values_mut() {
        plugins.remove(plugin_id);
    }
}

pub fn collect(
    preferences: &Preferences,
    user_id: i64,
) -> (
    Vec<String>,
    HashMap<String, Value>,
    HashMap<String, Vec<String>>,
) {
    let enabled = preferences.enabled_plugins.clone();
    let mut settings = HashMap::new();
    let mut names = HashMap::new();
    for id in &enabled {
        settings.insert(id.clone(), settings_for(preferences, user_id, id));
        names.insert(id.clone(), secret_names_for(preferences, user_id, id));
    }
    (enabled, settings, names)
}

pub fn apply_spm_mcp(config: &mut Value, settings: &Value, has_key: bool) -> bool {
    let enabled = settings
        .get("mcpEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if enabled && has_key {
        let mcp = config
            .as_object_mut()
            .expect("OpenCode config is an object")
            .entry("mcp")
            .or_insert_with(|| json!({}));
        mcp["spm"] = json!({
            "type": "remote",
            "url": "https://api.spmos.ai/mcp",
            "enabled": true,
            "oauth": false,
            "headers": {
                "Authorization": format!("Bearer {{env:{SPM_ENV}}}")
            }
        });
        true
    } else {
        remove_spm_mcp(config);
        false
    }
}

fn remove_spm_mcp(config: &mut Value) {
    let Some(root) = config.as_object_mut() else {
        return;
    };
    let Some(mcp) = root.get_mut("mcp").and_then(Value::as_object_mut) else {
        return;
    };
    mcp.remove("spm");
    if mcp.is_empty() {
        root.remove("mcp");
    }
}

pub fn spm_runtime_instructions(settings: &Value) -> String {
    let mut lines = Vec::new();
    if let Some(depth) = settings
        .get("recallDepth")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| matches!(*value, "fast" | "auto" | "deep"))
    {
        lines.push(format!("recall 的 depth 用 {depth}。"));
    }
    if let Some(partition) = settings
        .get("userPartition")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 120)
    {
        lines.push(format!(
            "所有 remember / recall / read 都传 user_partition `{partition}`。"
        ));
    }
    lines.join("\n")
}

pub fn extra_env(
    enabled: &[String],
    settings: &HashMap<String, Value>,
    secrets: &HashMap<String, String>,
    state_dir: &Path,
) -> Vec<(String, String)> {
    let mut env = vec![(
        "MOYU_PLUGIN_STATE_DIR".into(),
        state_dir.to_string_lossy().into_owned(),
    )];
    let mut items: Vec<_> = settings.iter().collect();
    items.sort_by(|left, right| left.0.cmp(right.0));
    let revision = serde_json::to_string(&items).unwrap_or_default();
    env.push((
        "MOYU_PLUGIN_REVISION".into(),
        format!("{:x}", fxhash(&revision)),
    ));
    if enabled.iter().any(|id| id == SPM_ID) {
        if let Some(key) = secrets.get(&format!("{SPM_ID}.{SPM_SECRET}")) {
            if !key.is_empty() {
                env.push((SPM_ENV.into(), key.clone()));
            }
        }
    }
    env
}

fn fxhash(value: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

pub async fn write_state_files(
    directory: &Path,
    enabled: &[String],
    settings: &HashMap<String, Value>,
) -> Result<(), String> {
    tokio::fs::create_dir_all(directory)
        .await
        .map_err(|error| error.to_string())?;
    for id in enabled {
        let values = settings.get(id).cloned().unwrap_or_else(|| json!({}));
        let path = directory.join(format!("{id}.json"));
        tokio::fs::write(
            &path,
            serde_json::to_vec_pretty(&values).map_err(|error| error.to_string())?,
        )
        .await
        .map_err(|error| format!("Cannot write plugin settings: {error}"))?;
    }
    Ok(())
}

pub const SUBAGENTS_ID: &str = "subagents";
const SUBAGENT_ROLES: &[&str] = &["researcher", "code-reviewer"];
const MAX_AUTO_MODELS: usize = 5;
const PROVIDER_ID: &str = "sub2api";

fn valid_model_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 120
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b':' | b'/'))
}

fn model_slug(id: &str) -> String {
    let mut slug = String::new();
    for ch in id.chars().map(|ch| ch.to_ascii_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}

/// Makes `id` usable by sub-agents by cloning the parent model's definition
/// (same reasoning options) under the new id. Providers cannot be set by plugin overlays,
/// so this is the only place sub-agent models get registered.
fn register_model(config: &mut Value, parent: &str, id: &str) -> bool {
    let Some(models) = config
        .pointer_mut(&format!("/provider/{PROVIDER_ID}/models"))
        .and_then(Value::as_object_mut)
    else {
        return false;
    };
    if models.contains_key(id) {
        return true;
    }
    let mut entry = models.get(parent).cloned().unwrap_or_else(|| json!({}));
    entry["name"] = json!(id);
    models.insert(id.to_owned(), entry);
    true
}

/// Applies the "子 Agent" plugin's model choice.
/// - follow (default): sub-agents define no model, so OpenCode runs them on the parent's model.
/// - fixed: every sub-agent runs on the model the user picked.
/// - auto: adds per-model variants (`researcher--<model>`) so the parent model picks one itself.
pub fn apply_subagent_models(
    config: &mut Value,
    instructions: &mut String,
    enabled: &[String],
    settings: &HashMap<String, Value>,
    available: &[String],
) {
    if !enabled.iter().any(|id| id == SUBAGENTS_ID) {
        return;
    }
    let values = settings
        .get(SUBAGENTS_ID)
        .cloned()
        .unwrap_or_else(|| json!({}));
    let Some(parent) = config
        .get("model")
        .and_then(Value::as_str)
        .and_then(|model| model.strip_prefix(&format!("{PROVIDER_ID}/")))
        .map(str::to_owned)
    else {
        return;
    };
    let roles: Vec<&str> = SUBAGENT_ROLES
        .iter()
        .copied()
        .filter(|role| config.pointer(&format!("/agent/{role}")).is_some())
        .collect();
    if roles.is_empty() {
        return;
    }
    match values.get("modelMode").and_then(Value::as_str) {
        Some("fixed") => {
            let Some(model) = values
                .get("model")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|model| {
                    valid_model_id(model)
                        && (available.is_empty() || available.iter().any(|item| item == model))
                })
            else {
                return;
            };
            if model != parent && !register_model(config, &parent, model) {
                return;
            }
            for role in roles {
                config["agent"][role]["model"] = json!(format!("{PROVIDER_ID}/{model}"));
            }
        }
        Some("auto") => {
            let mut chosen: Vec<(String, String)> = Vec::new();
            for id in available {
                if id == &parent || !valid_model_id(id) {
                    continue;
                }
                let slug = model_slug(id);
                if slug.is_empty() || chosen.iter().any(|(existing, _)| existing == &slug) {
                    continue;
                }
                chosen.push((slug, id.clone()));
                if chosen.len() >= MAX_AUTO_MODELS {
                    break;
                }
            }
            let mut listed = Vec::new();
            for role in &roles {
                let base = config["agent"][*role].clone();
                let description = base
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                for (slug, id) in &chosen {
                    if !register_model(config, &parent, id) {
                        continue;
                    }
                    let name = format!("{role}--{slug}");
                    let mut variant = base.clone();
                    variant["model"] = json!(format!("{PROVIDER_ID}/{id}"));
                    variant["description"] = json!(format!("{description}(使用模型 {id})"));
                    config["agent"][name.as_str()] = variant;
                    listed.push(format!("- `{name}`:{id}"));
                }
            }
            if listed.is_empty() {
                return;
            }
            if !instructions.is_empty() {
                instructions.push_str("\n\n");
            }
            instructions.push_str(&format!(
                "### 子 Agent 模型选择\n\n默认的 `researcher` 与 `code-reviewer` 使用当前对话的模型 `{parent}`。\n需要换模型时,改用下面带模型后缀的变体,根据任务难度和模型名称自己判断:\n{}",
                listed.join("\n")
            ));
        }
        _ => {}
    }
}

pub fn apply_runtime(
    config: &mut Value,
    instructions: &mut String,
    enabled: &[String],
    settings: &HashMap<String, Value>,
    has_spm_key: bool,
) -> PluginApplied {
    let spm_enabled = enabled.iter().any(|id| id == SPM_ID);
    let spm_values = settings.get(SPM_ID).cloned().unwrap_or_else(|| json!({}));
    let mcp = if spm_enabled {
        let applied = apply_spm_mcp(config, &spm_values, has_spm_key);
        if applied {
            if !instructions.contains("## SPM 记忆") {
                if !instructions.is_empty() {
                    instructions.push_str("\n\n");
                }
                instructions.push_str("## SPM 记忆");
            }
            let extra = spm_runtime_instructions(&spm_values);
            if !extra.is_empty() {
                instructions.push_str("\n\n");
                instructions.push_str(&extra);
            }
        }
        applied
    } else {
        remove_spm_mcp(config);
        false
    };
    let proxy = spm_enabled
        && spm_values
            .get("proxyEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    PluginApplied {
        persisted: true,
        instructions: instructions.contains("## SPM 记忆") || instructions.contains("## 简洁模式"),
        mcp,
        proxy,
        details: Vec::new(),
    }
}

pub fn inspect_applied(
    plugin_id: &str,
    enabled: bool,
    values: &Value,
    has_secret: bool,
    config: Option<&Value>,
    instructions: &str,
    state: Option<&Value>,
) -> PluginApplied {
    let mut details = Vec::new();
    let persisted = state.is_some_and(|saved| saved == values) || !enabled;
    if enabled {
        details.push("插件已启用".into());
    } else {
        details.push("插件未启用，设置会保存，启用后写入引擎".into());
    }
    let instructions_hit = match plugin_id {
        "spm" => instructions.contains("## SPM 记忆"),
        "concise" => instructions.contains("## 简洁模式"),
        "reviewer" => instructions.contains("## 审查模式"),
        SUBAGENTS_ID => instructions.contains("## 子 Agent"),
        _ => false,
    };
    if plugin_id == SUBAGENTS_ID && enabled {
        let mode = values.get("modelMode").and_then(Value::as_str);
        let effective = config
            .and_then(|value| value.pointer("/agent/researcher/model"))
            .and_then(Value::as_str);
        let variants = config
            .and_then(|value| value.get("agent"))
            .and_then(Value::as_object)
            .map(|agents| agents.keys().filter(|name| name.contains("--")).count())
            .unwrap_or(0);
        details.push(match (mode, effective) {
            (Some("fixed"), Some(model)) => format!("子 Agent 固定使用 {model}"),
            (Some("fixed"), None) => "固定模型未生效：还没选择模型，或该模型不在当前分组里".into(),
            (Some("auto"), _) if variants > 0 => {
                format!("子 Agent 模型由主模型自行选择（{variants} 个变体）")
            }
            (Some("auto"), _) => "自动选择未生效：当前分组没有可替换的模型".into(),
            _ => "子 Agent 沿用当前对话的模型".into(),
        });
    }
    if instructions_hit {
        details.push("提示词已写入 harness-instructions.md".into());
    }
    let mcp = plugin_id == SPM_ID
        && config
            .and_then(|value| value.pointer("/mcp/spm/enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
    if plugin_id == SPM_ID {
        if mcp {
            details.push("阶段一 MCP 已写入 opencode.json".into());
        } else if values
            .get("mcpEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            if !has_secret {
                details.push("阶段一未生效：还没有保存 SPM 密钥".into());
            } else if !enabled {
                details.push("阶段一未写入引擎：先启用插件".into());
            } else if config.is_none() {
                details.push("阶段一未写入引擎：先在设置里配置模型".into());
            } else {
                details.push("阶段一 MCP 尚未出现在 opencode.json".into());
            }
        }
        let header_ok = config
            .and_then(|value| value.pointer("/mcp/spm/headers/Authorization"))
            .and_then(Value::as_str)
            .is_some_and(|header| header.contains(&format!("{{env:{SPM_ENV}}}")));
        if mcp && header_ok {
            details.push("SPM 密钥走环境变量，没有写入配置文件".into());
        }
    }
    let proxy = plugin_id == SPM_ID
        && enabled
        && values
            .get("proxyEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        && state
            .and_then(|value| value.get("proxyEnabled"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
    if plugin_id == SPM_ID {
        if proxy {
            details.push("阶段二已写入插件状态：长会话会裁剪旧轮次，需要时用 recall 取回".into());
        } else if values
            .get("proxyEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            details.push("阶段二已保存，启用插件并发送一次任务后生效".into());
        }
    }
    if persisted && enabled {
        details.push("插件设置已落盘，可再次读取核对".into());
    }
    PluginApplied {
        persisted: persisted || state.is_none(),
        instructions: instructions_hit,
        mcp,
        proxy,
        details,
    }
}

pub async fn load_secrets(
    user_id: i64,
    enabled: &[String],
    names: &HashMap<String, Vec<String>>,
) -> HashMap<String, String> {
    let mut secrets = HashMap::new();
    for id in enabled {
        let Some(keys) = names.get(id) else {
            continue;
        };
        for key in keys {
            if let Ok(Some(value)) = get_secret(&secret_name(user_id, id, key)).await {
                if !value.is_empty() {
                    secrets.insert(format!("{id}.{key}"), value);
                }
            }
        }
    }
    secrets
}

pub async fn secret_presence(
    user_id: i64,
    plugin_id: &str,
    names: &[String],
) -> HashMap<String, bool> {
    let mut present = HashMap::new();
    for key in names {
        let exists = get_secret(&secret_name(user_id, plugin_id, key))
            .await
            .ok()
            .flatten()
            .is_some_and(|value| !value.is_empty());
        present.insert(key.clone(), exists);
    }
    present
}

pub async fn write_secrets(
    user_id: i64,
    plugin_id: &str,
    secrets: Vec<(String, Option<String>)>,
) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for (key, value) in secrets {
        let name = secret_name(user_id, plugin_id, &key);
        match value {
            Some(secret) => {
                set_secret(&name, secret).await?;
                names.push(key);
            }
            None => {
                delete_secret(&name).await?;
            }
        }
    }
    Ok(names)
}

pub async fn delete_secrets(user_id: i64, plugin_id: &str, names: &[String]) {
    for key in names {
        let _ = delete_secret(&secret_name(user_id, plugin_id, key)).await;
    }
}

pub async fn read_json_file(path: &Path) -> Option<Value> {
    let bytes = tokio::fs::read(path).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub async fn read_text_file(path: &Path) -> String {
    tokio::fs::read_to_string(path).await.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn settings_keys_and_secrets_are_constrained() {
        assert!(valid_settings_key("mcpEnabled"));
        assert!(valid_settings_key("apiKey"));
        assert!(!valid_settings_key("api-key"));
        assert!(!valid_settings_key(""));
        let cleaned = sanitize_values(json!({
            "mcpEnabled": true,
            "apiKey": "spm_live_secret",
            "nested-bad": 1
        }))
        .unwrap();
        assert_eq!(cleaned["mcpEnabled"], true);
        assert!(cleaned.get("apiKey").is_none());
        assert!(sanitize_values(json!([])).is_err());
    }

    #[test]
    fn spm_mcp_overlay_uses_env_interpolation() {
        let mut config = json!({ "plugin": [] });
        assert!(apply_spm_mcp(
            &mut config,
            &json!({ "mcpEnabled": true }),
            true
        ));
        assert_eq!(config["mcp"]["spm"]["type"], "remote");
        assert_eq!(
            config["mcp"]["spm"]["headers"]["Authorization"],
            "Bearer {env:MOYU_PLUGIN_SPM_API_KEY}"
        );
        assert!(!apply_spm_mcp(
            &mut config,
            &json!({ "mcpEnabled": true }),
            false
        ));
        assert!(config.get("mcp").is_none());
    }

    #[test]
    fn inspect_reports_missing_key_and_disabled_plugin() {
        let applied = inspect_applied(
            "spm",
            true,
            &json!({ "mcpEnabled": true, "proxyEnabled": true }),
            false,
            None,
            "",
            None,
        );
        assert!(!applied.mcp);
        assert!(applied
            .details
            .iter()
            .any(|line| line.contains("还没有保存 SPM 密钥")));
    }

    #[test]
    fn inspect_confirms_written_mcp_and_proxy_state() {
        let values = json!({ "mcpEnabled": true, "proxyEnabled": true });
        let config = json!({
            "mcp": {
                "spm": {
                    "enabled": true,
                    "headers": { "Authorization": "Bearer {env:MOYU_PLUGIN_SPM_API_KEY}" }
                }
            }
        });
        let applied = inspect_applied(
            "spm",
            true,
            &values,
            true,
            Some(&config),
            "## SPM 记忆\nrecall",
            Some(&values),
        );
        assert!(applied.mcp);
        assert!(applied.proxy);
        assert!(applied.instructions);
        assert!(applied
            .details
            .iter()
            .any(|line| line.contains("阶段一 MCP 已写入")));
    }

    fn subagent_base_config() -> Value {
        json!({
            "model": "sub2api/gpt-5.2",
            "provider": { "sub2api": { "models": {
                "gpt-5.2": { "name": "gpt-5.2", "options": { "reasoningEffort": "high" } }
            }}},
            "agent": {
                "researcher": { "description": "调研", "mode": "subagent" },
                "code-reviewer": { "description": "审查", "mode": "subagent" }
            }
        })
    }

    fn subagent_settings(values: Value) -> HashMap<String, Value> {
        HashMap::from([("subagents".to_owned(), values)])
    }

    #[test]
    fn subagents_follow_parent_model_by_default() {
        let mut config = subagent_base_config();
        let before = config.clone();
        let mut instructions = String::new();
        let enabled = vec!["subagents".to_owned()];
        let available = vec!["gpt-5.2".to_owned(), "gpt-5.2-mini".to_owned()];
        for settings in [
            subagent_settings(json!({})),
            subagent_settings(json!({ "modelMode": "follow" })),
            subagent_settings(json!({ "modelMode": "bogus" })),
        ] {
            apply_subagent_models(
                &mut config,
                &mut instructions,
                &enabled,
                &settings,
                &available,
            );
            assert_eq!(config, before);
            assert!(instructions.is_empty());
        }
    }

    #[test]
    fn subagents_fixed_model_is_registered_with_parent_options() {
        let mut config = subagent_base_config();
        let mut instructions = String::new();
        apply_subagent_models(
            &mut config,
            &mut instructions,
            &["subagents".into()],
            &subagent_settings(json!({ "modelMode": "fixed", "model": "gpt-5.2-mini" })),
            &["gpt-5.2".into(), "gpt-5.2-mini".into()],
        );
        assert_eq!(
            config["agent"]["researcher"]["model"],
            "sub2api/gpt-5.2-mini"
        );
        assert_eq!(
            config["agent"]["code-reviewer"]["model"],
            "sub2api/gpt-5.2-mini"
        );
        let registered = &config["provider"]["sub2api"]["models"]["gpt-5.2-mini"];
        assert_eq!(registered["name"], "gpt-5.2-mini");
        assert_eq!(registered["options"]["reasoningEffort"], "high");
        assert_eq!(config["model"], "sub2api/gpt-5.2");
    }

    #[test]
    fn subagents_fixed_model_rejects_unknown_or_invalid_ids() {
        for model in ["not-in-group", "bad id; rm", ""] {
            let mut config = subagent_base_config();
            let before = config.clone();
            apply_subagent_models(
                &mut config,
                &mut String::new(),
                &["subagents".into()],
                &subagent_settings(json!({ "modelMode": "fixed", "model": model })),
                &["gpt-5.2".into(), "gpt-5.2-mini".into()],
            );
            assert_eq!(config, before, "model {model:?} must be ignored");
        }
    }

    #[test]
    fn subagents_ignore_settings_when_plugin_is_disabled() {
        let mut config = subagent_base_config();
        let before = config.clone();
        apply_subagent_models(
            &mut config,
            &mut String::new(),
            &[],
            &subagent_settings(json!({ "modelMode": "fixed", "model": "gpt-5.2-mini" })),
            &["gpt-5.2-mini".into()],
        );
        assert_eq!(config, before);
    }

    #[test]
    fn subagents_auto_mode_adds_variants_and_guidance() {
        let mut config = subagent_base_config();
        let mut instructions = String::from("## 子 Agent\n\n正文");
        let available: Vec<String> = [
            "gpt-5.2",
            "gpt-5.2-mini",
            "claude-sonnet-4-6",
            "x1",
            "x2",
            "x3",
            "x4",
        ]
        .iter()
        .map(|id| id.to_string())
        .collect();
        apply_subagent_models(
            &mut config,
            &mut instructions,
            &["subagents".into()],
            &subagent_settings(json!({ "modelMode": "auto" })),
            &available,
        );
        // The parent model is skipped and the list is capped at five models.
        let agents = config["agent"].as_object().unwrap();
        assert_eq!(agents.keys().filter(|name| name.contains("--")).count(), 10);
        assert!(agents.get("researcher").unwrap().get("model").is_none());
        let variant = &config["agent"]["researcher--gpt-5-2-mini"];
        assert_eq!(variant["model"], "sub2api/gpt-5.2-mini");
        assert_eq!(variant["mode"], "subagent");
        assert!(variant["description"]
            .as_str()
            .unwrap()
            .contains("gpt-5.2-mini"));
        assert!(config["provider"]["sub2api"]["models"]
            .get("claude-sonnet-4-6")
            .is_some());
        assert!(instructions.contains("`code-reviewer--claude-sonnet-4-6`"));
        assert!(instructions.contains("当前对话的模型 `gpt-5.2`"));
        assert!(!instructions.contains("x4"));
    }

    #[test]
    fn subagents_auto_mode_without_alternatives_changes_nothing() {
        let mut config = subagent_base_config();
        let before = config.clone();
        let mut instructions = String::new();
        apply_subagent_models(
            &mut config,
            &mut instructions,
            &["subagents".into()],
            &subagent_settings(json!({ "modelMode": "auto" })),
            &["gpt-5.2".into()],
        );
        assert_eq!(config, before);
        assert!(instructions.is_empty());
    }

    #[test]
    fn inspect_reports_effective_subagent_model() {
        let config = json!({ "agent": { "researcher": { "model": "sub2api/gpt-5.2-mini" } } });
        let applied = inspect_applied(
            "subagents",
            true,
            &json!({ "modelMode": "fixed", "model": "gpt-5.2-mini" }),
            false,
            Some(&config),
            "## 子 Agent",
            None,
        );
        assert!(applied.instructions);
        assert!(applied
            .details
            .iter()
            .any(|line| line.contains("固定使用 sub2api/gpt-5.2-mini")));
        let missing = inspect_applied(
            "subagents",
            true,
            &json!({ "modelMode": "fixed" }),
            false,
            Some(&json!({ "agent": { "researcher": {} } })),
            "",
            None,
        );
        assert!(missing.details.iter().any(|line| line.contains("未生效")));
    }

    #[test]
    fn apply_runtime_appends_partition_instructions() {
        let mut config = json!({ "plugin": [] });
        let mut instructions = String::new();
        let mut settings = HashMap::new();
        settings.insert(
            "spm".into(),
            json!({
                "mcpEnabled": true,
                "proxyEnabled": true,
                "recallDepth": "fast",
                "userPartition": "user-123"
            }),
        );
        let applied = apply_runtime(
            &mut config,
            &mut instructions,
            &["spm".into()],
            &settings,
            true,
        );
        assert!(applied.mcp);
        assert!(instructions.contains("user_partition `user-123`"));
        assert!(instructions.contains("depth 用 fast"));
    }
}
