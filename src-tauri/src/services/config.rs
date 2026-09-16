use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSummary {
    pub group_id: i64,
    pub group_name: String,
    pub platform: String,
    pub model: String,
    pub config_path: String,
    #[serde(rename = "workspace")]
    pub working_directory: String,
    pub user_id: i64,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    #[serde(default)]
    pub endpoint_index: usize,
    pub config: Option<ConfigSummary>,
}

pub async fn load(app: &AppHandle) -> Result<Preferences, String> {
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        let store = app
            .store_builder("settings.json")
            .disable_auto_save()
            .build()
            .map_err(|e| format!("Cannot open settings: {e}"))?;
        store
            .get("preferences")
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| format!("Invalid settings: {e}"))
            .map(|p| p.unwrap_or_default())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn save(app: &AppHandle, preferences: Preferences) -> Result<(), String> {
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        let store = app
            .store_builder("settings.json")
            .disable_auto_save()
            .build()
            .map_err(|e| format!("Cannot open settings: {e}"))?;
        store.set(
            "preferences",
            serde_json::to_value(preferences).map_err(|e| e.to_string())?,
        );
        store
            .save()
            .map_err(|e| format!("Cannot save settings: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

pub fn app_directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

pub async fn write_bridge(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app_directory(app)?;
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|e| e.to_string())?;
    let path = directory.join("stream-bridge.mjs");
    tokio::fs::write(&path, include_str!("../../resources/stream-bridge.mjs"))
        .await
        .map_err(|e| format!("Cannot install stream bridge: {e}"))?;
    Ok(path)
}

pub fn opencode_config(
    config: &ConfigSummary,
    base_url: &str,
    bridge_path: &Path,
) -> Result<Value, String> {
    let (npm, base_url) = match config.platform.as_str() {
        "anthropic" | "claude" => ("@ai-sdk/anthropic", format!("{base_url}/v1")),
        "openai" => ("@ai-sdk/openai", format!("{base_url}/v1")),
        other => {
            return Err(format!(
                "Unsupported Sub2API group platform: {other}. Choose an Anthropic or OpenAI group."
            ))
        }
    };
    let bridge_url = url::Url::from_file_path(bridge_path)
        .map_err(|_| "Invalid stream bridge path".to_owned())?;
    Ok(json!({
        "$schema": "https://opencode.ai/config.json",
        "plugin": [bridge_url.as_str()],
        "enabled_providers": ["sub2api"],
        "provider": {
            "sub2api": {
                "npm": npm,
                "name": "Sub2API",
                "options": {"baseURL": base_url, "apiKey": "{env:SUB2API_API_KEY}", "timeout": 120000},
                "models": {config.model.clone(): {"name": config.model, "limit": {"context": 128000, "output": 16000}}}
            }
        },
        "model": format!("sub2api/{}", config.model),
        "share": "disabled",
        "autoupdate": false,
        "permission": {"*": "allow", "external_directory": "deny"}
    }))
}

pub async fn write_opencode_config(
    app: &AppHandle,
    config: &ConfigSummary,
    base_url: &str,
) -> Result<Value, String> {
    let bridge = write_bridge(app).await?;
    let content = opencode_config(config, base_url, &bridge)?;
    tokio::fs::write(
        &config.config_path,
        serde_json::to_vec_pretty(&content).map_err(|e| e.to_string())?,
    )
    .await
    .map_err(|e| format!("Cannot write OpenCode configuration: {e}"))?;
    Ok(content)
}

pub async fn working_directory(
    app: &AppHandle,
    requested: Option<String>,
) -> Result<PathBuf, String> {
    let path = match requested.filter(|p| !p.trim().is_empty()) {
        Some(value) => PathBuf::from(value.trim()),
        None => {
            let path = app_directory(app)?.join("workspace");
            tokio::fs::create_dir_all(&path)
                .await
                .map_err(|e| e.to_string())?;
            path
        }
    };
    if !path.is_absolute()
        || !tokio::fs::metadata(&path)
            .await
            .map_err(|e| format!("Cannot open workspace: {e}"))?
            .is_dir()
    {
        return Err("Workspace must be an existing absolute directory".to_owned());
    }
    tokio::fs::canonicalize(path)
        .await
        .map_err(|e| e.to_string())
}

pub fn sidecar_path(app: &AppHandle) -> Result<PathBuf, String> {
    let filename = if cfg!(windows) {
        "opencode.exe"
    } else {
        "opencode"
    };
    if cfg!(debug_assertions) {
        let suffix = if cfg!(windows) { ".exe" } else { "" };
        return Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join(format!(
                "opencode-{}{suffix}",
                env!("TAURI_ENV_TARGET_TRIPLE")
            )));
    }
    if cfg!(target_os = "macos") {
        return app
            .path()
            .resource_dir()
            .map(|p| p.join("../MacOS").join(filename))
            .map_err(|e| e.to_string());
    }
    std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .map(|p| p.join(filename))
        .ok_or_else(|| "Cannot resolve sidecar directory".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_config_uses_secret_reference_and_selected_endpoint() {
        let config = ConfigSummary {
            group_id: 1,
            group_name: "test".into(),
            platform: "anthropic".into(),
            model: "claude-sonnet-4-6".into(),
            config_path: String::new(),
            working_directory: String::new(),
            user_id: 1,
        };
        let path = std::env::temp_dir().join("stream-bridge.mjs");
        let value = opencode_config(&config, "https://inkaicf.flymiku.top", &path).unwrap();
        assert_eq!(
            value["provider"]["sub2api"]["options"]["apiKey"],
            "{env:SUB2API_API_KEY}"
        );
        assert_eq!(
            value["provider"]["sub2api"]["options"]["baseURL"],
            "https://inkaicf.flymiku.top/v1"
        );
    }
}
