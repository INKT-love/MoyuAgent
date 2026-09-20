use super::auth::{Group, GroupModel};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};
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
    #[serde(default = "default_reasoning_effort")]
    pub reasoning_effort: String,
    #[serde(default = "default_permission_mode")]
    pub permission_mode: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    #[serde(default)]
    pub endpoint_index: usize,
    pub config: Option<ConfigSummary>,
    #[serde(default)]
    pub recent_workspaces: Vec<String>,
    #[serde(default)]
    pub pinned_workspaces: Vec<String>,
    #[serde(default)]
    pub workspace_labels: HashMap<String, String>,
    #[serde(default)]
    pub enabled_plugins: Vec<String>,
    #[serde(default)]
    pub catalog: Option<Catalog>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceActionResult {
    pub recent_workspaces: Vec<String>,
    pub pinned_workspaces: Vec<String>,
    pub workspace_labels: HashMap<String, String>,
    pub switch_to: Option<String>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    #[serde(default)]
    pub user_id: i64,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub models: HashMap<String, Vec<GroupModel>>,
}

impl Preferences {
    pub fn groups_for(&self, user_id: i64) -> Option<&[Group]> {
        self.catalog
            .as_ref()
            .filter(|catalog| catalog.user_id == user_id)
            .map(|catalog| catalog.groups.as_slice())
    }

    pub fn models_for(&self, user_id: i64, group_id: i64) -> Option<&[GroupModel]> {
        self.catalog
            .as_ref()
            .filter(|catalog| catalog.user_id == user_id)?
            .models
            .get(&group_id.to_string())
            .map(Vec::as_slice)
    }

    pub fn set_groups(&mut self, user_id: i64, groups: Vec<Group>) {
        let ids: HashSet<String> = groups.iter().map(|group| group.id.to_string()).collect();
        match self.catalog.as_mut() {
            Some(catalog) if catalog.user_id == user_id => {
                catalog.groups = groups;
                catalog.models.retain(|key, _| ids.contains(key));
            }
            _ => {
                self.catalog = Some(Catalog {
                    user_id,
                    groups,
                    models: HashMap::new(),
                });
            }
        }
    }

    pub fn set_models(&mut self, user_id: i64, group_id: i64, models: Vec<GroupModel>) {
        let Some(catalog) = self
            .catalog
            .as_mut()
            .filter(|catalog| catalog.user_id == user_id)
        else {
            return;
        };
        catalog.models.insert(group_id.to_string(), models);
    }
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

pub fn default_model_for(platform: &str) -> String {
    match platform.to_ascii_lowercase().as_str() {
        "openai" => "gpt-5.2".into(),
        "anthropic" | "claude" | "" => "claude-sonnet-4-6".into(),
        _ => String::new(),
    }
}

pub fn default_reasoning_effort() -> String {
    "high".into()
}

pub fn normalize_reasoning_effort(value: Option<&str>) -> String {
    match value.map(|item| item.trim().to_ascii_lowercase()).as_deref() {
        Some("low") => "low".into(),
        Some("medium") => "medium".into(),
        Some("high") => "high".into(),
        Some("xhigh") => "xhigh".into(),
        _ => default_reasoning_effort(),
    }
}

pub fn default_permission_mode() -> String {
    "assist".into()
}

pub fn normalize_permission_mode(value: Option<&str>) -> String {
    match value.map(|item| item.trim().to_ascii_lowercase()).as_deref() {
        Some("ask") => "ask".into(),
        Some("full") => "full".into(),
        Some("assist") => "assist".into(),
        _ => default_permission_mode(),
    }
}

pub fn permission_config(mode: &str) -> Value {
    match normalize_permission_mode(Some(mode)).as_str() {
        "ask" => json!({
            "*": "allow",
            "webfetch": "deny",
            "websearch": "deny",
            "external_directory": "deny"
        }),
        "full" => json!({ "*": "allow" }),
        _ => json!({ "*": "allow", "external_directory": "deny" }),
    }
}

pub fn opencode_config(
    config: &ConfigSummary,
    base_url: &str,
    bridge_path: &Path,
) -> Result<Value, String> {
    let npm = match config.platform.to_ascii_lowercase().as_str() {
        "anthropic" | "claude" => "@ai-sdk/anthropic",
        _ => "@ai-sdk/openai",
    };
    let base_url = format!("{base_url}/v1");
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
                "models": {config.model.clone(): {
                    "name": config.model,
                    "limit": {"context": 128000, "output": 16000},
                    "options": {"reasoningEffort": normalize_reasoning_effort(Some(&config.reasoning_effort))},
                    "variants": {
                        "low": {"reasoningEffort": "low"},
                        "medium": {"reasoningEffort": "medium"},
                        "high": {"reasoningEffort": "high"},
                        "xhigh": {"reasoningEffort": "xhigh"}
                    }
                }}
            }
        },
        "model": format!("sub2api/{}", config.model),
        "share": "disabled",
        "autoupdate": false,
        "permission": permission_config(&config.permission_mode)
    }))
}

pub async fn write_opencode_config(
    app: &AppHandle,
    config: &ConfigSummary,
    base_url: &str,
    enabled_plugins: &[String],
) -> Result<Value, String> {
    let bridge = write_bridge(app).await?;
    let mut content = opencode_config(config, base_url, &bridge)?;
    super::harness::apply_to_opencode(app, &mut content, enabled_plugins).await?;
    tokio::fs::write(
        &config.config_path,
        serde_json::to_vec_pretty(&content).map_err(|e| e.to_string())?,
    )
    .await
    .map_err(|e| format!("Cannot write OpenCode configuration: {e}"))?;
    Ok(content)
}

pub fn remember_workspace(preferences: &mut Preferences, path: String) {
    preferences
        .recent_workspaces
        .retain(|item| !workspace_eq(item, &path));
    preferences.recent_workspaces.insert(0, path);
    preferences.recent_workspaces.truncate(8);
}

pub fn workspace_key(value: &str) -> String {
    let mut path = value.replace('\\', "/");
    while path.len() > 1 && path.ends_with('/') {
        path.pop();
    }
    if cfg!(windows) {
        path.make_ascii_lowercase();
    }
    path
}

pub fn workspace_eq(left: &str, right: &str) -> bool {
    !left.is_empty() && !right.is_empty() && workspace_key(left) == workspace_key(right)
}

pub fn pin_workspace(preferences: &mut Preferences, path: &str, pinned: bool) {
    preferences
        .pinned_workspaces
        .retain(|item| !workspace_eq(item, path));
    if pinned {
        preferences.pinned_workspaces.insert(0, path.to_owned());
        preferences.pinned_workspaces.truncate(16);
        if !preferences
            .recent_workspaces
            .iter()
            .any(|item| workspace_eq(item, path))
        {
            remember_workspace(preferences, path.to_owned());
        }
    }
}

pub fn rename_workspace(preferences: &mut Preferences, path: &str, name: &str) {
    preferences
        .workspace_labels
        .retain(|key, _| !workspace_eq(key, path));
    let name: String = name
        .chars()
        .filter(|char| *char != '\n' && *char != '\r')
        .take(80)
        .collect::<String>()
        .trim()
        .to_owned();
    if !name.is_empty() {
        preferences
            .workspace_labels
            .insert(path.to_owned(), name);
    }
}

pub fn forget_workspace(preferences: &mut Preferences, path: &str) -> bool {
    let was_current = preferences
        .config
        .as_ref()
        .is_some_and(|config| workspace_eq(&config.working_directory, path));
    preferences
        .recent_workspaces
        .retain(|item| !workspace_eq(item, path));
    preferences
        .pinned_workspaces
        .retain(|item| !workspace_eq(item, path));
    preferences
        .workspace_labels
        .retain(|key, _| !workspace_eq(key, path));
    was_current
}

pub fn next_workspace(preferences: &Preferences) -> Option<String> {
    preferences
        .pinned_workspaces
        .first()
        .cloned()
        .or_else(|| preferences.recent_workspaces.first().cloned())
}

pub fn workspace_action_result(
    preferences: &Preferences,
    switch_to: Option<String>,
) -> WorkspaceActionResult {
    WorkspaceActionResult {
        recent_workspaces: preferences.recent_workspaces.clone(),
        pinned_workspaces: preferences.pinned_workspaces.clone(),
        workspace_labels: preferences.workspace_labels.clone(),
        switch_to,
    }
}

pub fn worktree_destination(repo: &Path) -> PathBuf {
    let stem = repo
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "workspace".into());
    let parent = repo.parent().filter(|path| !path.as_os_str().is_empty());
    let directory = parent.unwrap_or(repo);
    let prefix = format!("{stem}-worktree");
    let candidate = directory.join(&prefix);
    if !candidate.exists() {
        return candidate;
    }
    for index in 2..100 {
        let candidate = directory.join(format!("{prefix}-{index}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    directory.join(format!("{prefix}-{}", uuid::Uuid::new_v4().as_simple()))
}

pub async fn create_git_worktree(repo: &Path) -> Result<PathBuf, String> {
    let repo = tokio::fs::canonicalize(repo)
        .await
        .map_err(|error| format!("Cannot open workspace: {error}"))?;
    let inside = git_output(&repo, &["rev-parse", "--is-inside-work-tree"]).await?;
    if inside.trim() != "true" {
        return Err("This folder is not a Git repository".into());
    }
    let root = PathBuf::from(
        git_output(&repo, &["rev-parse", "--show-toplevel"])
            .await?
            .trim(),
    );
    let dest = worktree_destination(&root);
    let dest_text = dest
        .to_str()
        .ok_or_else(|| "Invalid worktree path".to_owned())?;
    let stem = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("ws");
    let safe: String = stem
        .chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() {
                char
            } else {
                '-'
            }
        })
        .collect();
    let branch = format!(
        "moyu-wt-{safe}-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    git_output(
        &root,
        &["worktree", "add", "-b", &branch, dest_text, "HEAD"],
    )
    .await?;
    Ok(dest)
}

async fn git_output(directory: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = tokio::process::Command::new("git");
    command
        .args(args)
        .current_dir(directory)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "Git is not available. Install Git before creating a worktree.".to_owned()
        } else {
            error.to_string()
        }
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("Git worktree failed");
        return Err(message.chars().take(240).collect());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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
            reasoning_effort: "xhigh".into(),
            permission_mode: "assist".into(),
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
        assert_eq!(value["provider"]["sub2api"]["npm"], "@ai-sdk/anthropic");
        let model = &value["provider"]["sub2api"]["models"]["claude-sonnet-4-6"];
        assert_eq!(model["options"]["reasoningEffort"], "xhigh");
        assert_eq!(model["variants"]["low"]["reasoningEffort"], "low");
        assert_eq!(model["variants"]["xhigh"]["reasoningEffort"], "xhigh");
        assert_eq!(value["permission"]["*"], "allow");
        assert_eq!(value["permission"]["external_directory"], "deny");
        assert!(value["permission"].get("webfetch").is_none());
    }

    #[test]
    fn generated_config_uses_openai_compatible_sdk_for_other_platforms() {
        let config = ConfigSummary {
            group_id: 8,
            group_name: "Gemini".into(),
            platform: "gemini".into(),
            model: "gemini-2.5-pro".into(),
            config_path: String::new(),
            working_directory: String::new(),
            user_id: 1,
            reasoning_effort: "high".into(),
            permission_mode: "full".into(),
        };
        let path = std::env::temp_dir().join("stream-bridge.mjs");
        let value = opencode_config(&config, "https://api.inktandwkx.top", &path).unwrap();
        assert_eq!(value["provider"]["sub2api"]["npm"], "@ai-sdk/openai");
        assert_eq!(
            value["provider"]["sub2api"]["options"]["baseURL"],
            "https://api.inktandwkx.top/v1"
        );
        assert_eq!(value["permission"], json!({ "*": "allow" }));
    }

    #[test]
    fn catalog_is_scoped_to_the_signed_in_user() {
        let mut preferences = Preferences::default();
        preferences.set_groups(
            2,
            vec![Group {
                id: 37,
                name: "限时福利".into(),
                platform: "openai".into(),
            }],
        );
        preferences.set_models(
            2,
            37,
            vec![GroupModel {
                id: "gpt-5.6-sol".into(),
                name: "GPT-5.6 Sol".into(),
            }],
        );
        assert_eq!(preferences.groups_for(2).unwrap()[0].id, 37);
        assert_eq!(preferences.models_for(2, 37).unwrap()[0].id, "gpt-5.6-sol");
        assert!(preferences.groups_for(3).is_none());
        assert!(preferences.models_for(3, 37).is_none());
        preferences.set_groups(3, vec![]);
        assert!(preferences.groups_for(2).is_none());
        assert_eq!(preferences.groups_for(3).unwrap().len(), 0);
    }

    #[test]
    fn refreshing_groups_prunes_models_for_removed_groups() {
        let mut preferences = Preferences::default();
        preferences.set_groups(
            1,
            vec![
                Group {
                    id: 8,
                    name: "Gemini".into(),
                    platform: "gemini".into(),
                },
                Group {
                    id: 9,
                    name: "Kimi".into(),
                    platform: "kimi".into(),
                },
            ],
        );
        preferences.set_models(
            1,
            8,
            vec![GroupModel {
                id: "gemini-2.5-pro".into(),
                name: "Gemini 2.5 Pro".into(),
            }],
        );
        preferences.set_models(
            1,
            9,
            vec![GroupModel {
                id: "kimi-k2".into(),
                name: "Kimi K2".into(),
            }],
        );
        preferences.set_groups(
            1,
            vec![Group {
                id: 8,
                name: "Gemini".into(),
                platform: "gemini".into(),
            }],
        );
        assert_eq!(
            preferences.models_for(1, 8).unwrap()[0].id,
            "gemini-2.5-pro"
        );
        assert!(preferences.models_for(1, 9).is_none());
    }

    #[test]
    fn set_models_does_not_create_an_empty_groups_catalog() {
        let mut preferences = Preferences::default();
        preferences.set_models(
            1,
            8,
            vec![GroupModel {
                id: "gemini-2.5-pro".into(),
                name: "Gemini 2.5 Pro".into(),
            }],
        );
        assert!(preferences.groups_for(1).is_none());
        assert!(preferences.models_for(1, 8).is_none());
    }

    #[test]
    fn default_model_covers_openai_compatible_platforms() {
        assert_eq!(default_model_for("openai"), "gpt-5.2");
        assert_eq!(default_model_for("anthropic"), "claude-sonnet-4-6");
        assert_eq!(default_model_for("gemini"), "");
        assert_eq!(default_model_for("grok"), "");
    }

    #[test]
    fn remember_workspace_keeps_newest_unique_paths() {
        let mut preferences = Preferences::default();
        remember_workspace(&mut preferences, r"D:\alpha".into());
        remember_workspace(&mut preferences, r"D:\beta".into());
        remember_workspace(&mut preferences, r"D:\alpha".into());
        assert_eq!(
            preferences.recent_workspaces,
            [r"D:\alpha".to_owned(), r"D:\beta".to_owned()]
        );
    }

    #[test]
    fn workspace_eq_normalizes_separators_and_trailing_slash() {
        assert_eq!(workspace_key(r"D:\alpha"), workspace_key("D:/alpha/"));
        assert!(workspace_eq(r"D:\alpha", "D:/alpha/"));
        assert!(!workspace_eq("", "D:/alpha"));
        assert!(!workspace_eq("D:/alpha", ""));
    }

    #[test]
    fn pin_workspace_moves_to_front_and_unpins() {
        let mut preferences = Preferences::default();
        pin_workspace(&mut preferences, r"D:\a", true);
        pin_workspace(&mut preferences, r"D:\b", true);
        pin_workspace(&mut preferences, r"D:\a", true);
        assert_eq!(
            preferences.pinned_workspaces,
            [r"D:\a".to_owned(), r"D:\b".to_owned()]
        );
        assert!(
            preferences
                .recent_workspaces
                .iter()
                .any(|path| path == r"D:\a")
        );
        assert!(
            preferences
                .recent_workspaces
                .iter()
                .any(|path| path == r"D:\b")
        );
        pin_workspace(&mut preferences, r"D:\a", false);
        assert_eq!(preferences.pinned_workspaces, [r"D:\b".to_owned()]);
    }

    #[test]
    fn rename_workspace_stores_trimmed_label_and_clears_empty() {
        let mut preferences = Preferences::default();
        rename_workspace(&mut preferences, r"D:\alpha", "  Alpha\n");
        assert_eq!(
            preferences.workspace_labels.get(r"D:\alpha").unwrap(),
            "Alpha"
        );
        rename_workspace(&mut preferences, "D:/alpha/", "");
        assert!(preferences.workspace_labels.is_empty());
    }

    #[test]
    fn forget_workspace_drops_lists_and_reports_current() {
        let mut preferences = Preferences::default();
        remember_workspace(&mut preferences, r"D:\alpha".into());
        pin_workspace(&mut preferences, r"D:\alpha", true);
        rename_workspace(&mut preferences, r"D:\alpha", "Alpha");
        preferences.config = Some(ConfigSummary {
            group_id: 1,
            group_name: "test".into(),
            platform: "anthropic".into(),
            model: "claude-sonnet-4-6".into(),
            config_path: String::new(),
            working_directory: r"D:\alpha".into(),
            user_id: 1,
            reasoning_effort: "high".into(),
            permission_mode: "assist".into(),
        });
        assert!(forget_workspace(&mut preferences, "D:/alpha"));
        assert!(preferences.recent_workspaces.is_empty());
        assert!(preferences.pinned_workspaces.is_empty());
        assert!(preferences.workspace_labels.is_empty());
        assert!(next_workspace(&preferences).is_none());
    }

    #[test]
    fn missing_reasoning_effort_defaults_to_high() {
        let config: ConfigSummary = serde_json::from_value(json!({
            "groupId": 1,
            "groupName": "test",
            "platform": "openai",
            "model": "gpt-5.6-sol",
            "configPath": "",
            "workspace": "",
            "userId": 1
        }))
        .unwrap();
        assert_eq!(config.reasoning_effort, "high");
        assert_eq!(normalize_reasoning_effort(Some("XHIGH")), "xhigh");
        assert_eq!(normalize_reasoning_effort(Some("max")), "high");
        assert_eq!(normalize_reasoning_effort(None), "high");
        assert_eq!(config.permission_mode, "assist");
        assert_eq!(normalize_permission_mode(Some("FULL")), "full");
        assert_eq!(normalize_permission_mode(Some("yolo")), "assist");
        assert_eq!(
            permission_config("ask")["webfetch"],
            json!("deny")
        );
    }

    #[test]
    fn next_workspace_prefers_pinned() {
        let mut preferences = Preferences::default();
        remember_workspace(&mut preferences, r"D:\recent".into());
        pin_workspace(&mut preferences, r"D:\pinned", true);
        assert_eq!(next_workspace(&preferences).as_deref(), Some(r"D:\pinned"));
    }

    #[test]
    fn worktree_destination_uses_sibling_folder() {
        let root = std::env::temp_dir().join(format!(
            "moyu-wt-src-{}",
            uuid::Uuid::new_v4().as_simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let dest = worktree_destination(&root);
        assert_eq!(
            dest,
            root.parent()
                .unwrap()
                .join(format!("{}-worktree", root.file_name().unwrap().to_string_lossy()))
        );
        assert!(!dest.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
