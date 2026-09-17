use super::config;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager};
use url::Url;

const BLOCKED_OVERLAY_KEYS: &[&str] = &["provider", "model", "$schema"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub enabled: bool,
    pub bundled: bool,
    pub has_module: bool,
    pub has_instructions: bool,
    pub has_ui: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginUi {
    pub id: String,
    pub name: String,
    pub html: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginMixin {
    pub plugin_id: String,
    pub plugin_name: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub select: Option<String>,
    pub at: String,
    pub priority: i32,
    pub html: Option<String>,
    pub script: Option<String>,
}

#[derive(Clone, Debug)]
pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub directory: PathBuf,
    pub instructions: Option<String>,
    pub overlay: Option<Value>,
    pub module: Option<PathBuf>,
    pub ui: Option<PathBuf>,
    pub mixins: Vec<PluginMixin>,
}

pub fn valid_plugin_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit())
        && id.len() <= 63
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub fn parse_manifest(value: &Value) -> Result<PluginManifest, String> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "plugin.json is missing id".to_owned())?;
    if !valid_plugin_id(id) {
        return Err("Plugin id must be lowercase letters, digits and dashes".to_owned());
    }
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "plugin.json is missing name".to_owned())?;
    Ok(PluginManifest {
        id: id.to_owned(),
        name: name.to_owned(),
        description: value
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned(),
        version: value
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("0.1.0")
            .trim()
            .to_owned(),
    })
}

pub fn plugins_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config::app_directory(app)?.join("harness").join("plugins"))
}

pub fn instructions_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config::app_directory(app)?.join("harness-instructions.md"))
}

pub fn bundled_plugins_root(app: &AppHandle) -> Option<PathBuf> {
    for candidate in ["harness", "resources/harness"] {
        if let Ok(path) = app.path().resolve(candidate, BaseDirectory::Resource) {
            if path.is_dir() {
                return Some(path);
            }
        }
    }
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/harness");
    source.is_dir().then_some(source)
}

pub async fn seed_bundled(app: &AppHandle) -> Result<(), String> {
    let destination = plugins_dir(app)?;
    tokio::fs::create_dir_all(&destination)
        .await
        .map_err(|error| error.to_string())?;
    let Some(source) = bundled_plugins_root(app) else {
        return Ok(());
    };
    let mut entries = tokio::fs::read_dir(&source)
        .await
        .map_err(|error| error.to_string())?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| error.to_string())?
    {
        if !entry
            .file_type()
            .await
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            continue;
        }
        let name = entry.file_name();
        let id = name.to_string_lossy();
        if !valid_plugin_id(&id) {
            continue;
        }
        copy_dir(&entry.path(), &destination.join(name)).await?;
    }
    Ok(())
}

pub async fn write_loader(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = config::app_directory(app)?;
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|error| error.to_string())?;
    let path = directory.join("harness-loader.mjs");
    tokio::fs::write(&path, include_str!("../../resources/harness-loader.mjs"))
        .await
        .map_err(|error| format!("Cannot install harness loader: {error}"))?;
    Ok(path)
}

pub async fn list_plugins(app: &AppHandle, enabled: &[String]) -> Result<Vec<PluginInfo>, String> {
    seed_bundled(app).await?;
    let bundled_root = bundled_plugins_root(app);
    let root = plugins_dir(app)?;
    let mut plugins = Vec::new();
    let mut entries = match tokio::fs::read_dir(&root).await {
        Ok(entries) => entries,
        Err(_) => return Ok(plugins),
    };
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| error.to_string())?
    {
        if !entry
            .file_type()
            .await
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            continue;
        }
        let directory = entry.path();
        let Ok(loaded) = load_plugin(&directory).await else {
            continue;
        };
        let id = loaded.manifest.id.clone();
        plugins.push(PluginInfo {
            enabled: enabled.iter().any(|item| item == &id),
            bundled: bundled_root
                .as_ref()
                .is_some_and(|root| root.join(&id).join("plugin.json").exists()),
            has_module: loaded.module.is_some(),
            has_instructions: loaded.instructions.is_some(),
            has_ui: loaded.ui.is_some()
                || loaded.mixins.iter().any(|mixin| mixin.html.is_some()),
            id,
            name: loaded.manifest.name,
            description: loaded.manifest.description,
            version: loaded.manifest.version,
        });
    }
    plugins.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(plugins)
}

pub async fn load_enabled(
    app: &AppHandle,
    enabled: &[String],
) -> Result<Vec<LoadedPlugin>, String> {
    seed_bundled(app).await?;
    let root = plugins_dir(app)?;
    let mut loaded = Vec::new();
    for id in enabled {
        if !valid_plugin_id(id) {
            continue;
        }
        let directory = root.join(id);
        if let Ok(plugin) = load_plugin(&directory).await {
            loaded.push(plugin);
        }
    }
    Ok(loaded)
}

pub async fn load_plugin(directory: &Path) -> Result<LoadedPlugin, String> {
    let raw = tokio::fs::read_to_string(directory.join("plugin.json"))
        .await
        .map_err(|_| "Missing plugin.json".to_owned())?;
    let value: Value =
        serde_json::from_str(&raw).map_err(|_| "plugin.json is not valid JSON".to_owned())?;
    let manifest = parse_manifest(&value)?;
    let instructions = match tokio::fs::read_to_string(directory.join("AGENTS.md")).await {
        Ok(text) if !text.trim().is_empty() => Some(text),
        _ => None,
    };
    let overlay = match tokio::fs::read_to_string(directory.join("opencode.patch.json")).await {
        Ok(text) => Some(
            serde_json::from_str(&text)
                .map_err(|_| "opencode.patch.json is not valid JSON".to_owned())?,
        ),
        Err(_) => None,
    };
    let module = ["index.mjs", "index.js"]
        .into_iter()
        .map(|name| directory.join(name))
        .find(|path| path.exists());
    let ui = directory.join("ui.html");
    let mixins = resolve_mixins(directory, &manifest, &value).await?;
    Ok(LoadedPlugin {
        manifest,
        directory: directory.to_path_buf(),
        instructions,
        overlay,
        module,
        ui: ui.exists().then_some(ui),
        mixins,
    })
}

fn valid_mixin_target(target: &str) -> bool {
    let mut chars = target.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_lowercase())
        && target.len() <= 80
        && target
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.')
}

fn valid_mixin_select(select: &str) -> bool {
    let trimmed = select.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 120
        && !trimmed.contains('<')
        && !trimmed.to_ascii_lowercase().contains("javascript")
}

fn valid_mixin_file(name: &str, ext: &str) -> bool {
    Path::new(name).file_name().and_then(|value| value.to_str()) == Some(name)
        && name.ends_with(ext)
        && !name.contains("..")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

async fn read_mixin_file(directory: &Path, name: &str, ext: &str) -> Result<String, String> {
    if !valid_mixin_file(name, ext) {
        return Err("Mixin file name is invalid".to_owned());
    }
    let text = tokio::fs::read_to_string(directory.join(name))
        .await
        .map_err(|_| format!("Missing mixin file {name}"))?;
    if text.len() > MAX_UI_BYTES {
        return Err("Mixin file exceeded the size limit".to_owned());
    }
    Ok(text)
}

async fn resolve_mixins(
    directory: &Path,
    manifest: &PluginManifest,
    value: &Value,
) -> Result<Vec<PluginMixin>, String> {
    let mut mixins = Vec::new();
    if let Some(entries) = value.get("mixins").and_then(Value::as_array) {
        for entry in entries {
            let target = entry
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_owned();
            if !target.is_empty() && !valid_mixin_target(&target) {
                return Err("Mixin target is invalid".to_owned());
            }
            let select = entry
                .get("select")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            if let Some(value) = &select {
                if !valid_mixin_select(value) {
                    return Err("Mixin select is invalid".to_owned());
                }
            }
            let at = entry
                .get("at")
                .and_then(Value::as_str)
                .unwrap_or("RETURN")
                .to_ascii_uppercase();
            if !matches!(at.as_str(), "HEAD" | "RETURN" | "TAIL" | "WRAP") {
                return Err("Mixin at must be HEAD, RETURN, TAIL or WRAP".to_owned());
            }
            let html = match entry.get("html").and_then(Value::as_str) {
                Some(name) => Some(read_mixin_file(directory, name, ".html").await?),
                None => None,
            };
            let script = match entry.get("script").and_then(Value::as_str) {
                Some(name) => Some(read_mixin_file(directory, name, ".js").await?),
                None => None,
            };
            if html.is_none() && script.is_none() {
                return Err("Mixin needs html or script".to_owned());
            }
            mixins.push(PluginMixin {
                plugin_id: manifest.id.clone(),
                plugin_name: manifest.name.clone(),
                target,
                select,
                at,
                priority: entry
                    .get("priority")
                    .and_then(Value::as_i64)
                    .unwrap_or(1000) as i32,
                html,
                script,
            });
        }
    }
    if mixins.is_empty() && directory.join("ui.html").exists() {
        mixins.push(PluginMixin {
            plugin_id: manifest.id.clone(),
            plugin_name: manifest.name.clone(),
            target: String::new(),
            select: None,
            at: "RETURN".into(),
            priority: 1000,
            html: Some(read_mixin_file(directory, "ui.html", ".html").await?),
            script: None,
        });
    }
    Ok(mixins)
}

pub async fn list_plugin_mixins(
    app: &AppHandle,
    enabled: &[String],
) -> Result<Vec<PluginMixin>, String> {
    let plugins = load_enabled(app, enabled).await?;
    Ok(plugins.into_iter().flat_map(|plugin| plugin.mixins).collect())
}

const MAX_UI_BYTES: usize = 256 * 1024;

pub async fn list_plugin_uis(app: &AppHandle, enabled: &[String]) -> Result<Vec<PluginUi>, String> {
    let plugins = load_enabled(app, enabled).await?;
    let mut uis = Vec::new();
    for plugin in plugins {
        let Some(path) = plugin.ui else {
            continue;
        };
        let html = tokio::fs::read_to_string(&path)
            .await
            .map_err(|error| error.to_string())?;
        if html.len() > MAX_UI_BYTES {
            return Err("Plugin UI exceeded the size limit".to_owned());
        }
        uis.push(PluginUi {
            id: plugin.manifest.id,
            name: plugin.manifest.name,
            html,
        });
    }
    Ok(uis)
}

pub async fn install_from_directory(app: &AppHandle, source: &Path) -> Result<String, String> {
    let plugin = load_plugin(source).await?;
    let destination = plugins_dir(app)?.join(&plugin.manifest.id);
    if destination.exists() {
        tokio::fs::remove_dir_all(&destination)
            .await
            .map_err(|error| error.to_string())?;
    }
    copy_dir(source, &destination).await?;
    Ok(plugin.manifest.id)
}

pub async fn uninstall(app: &AppHandle, id: &str) -> Result<(), String> {
    if !valid_plugin_id(id) {
        return Err("Invalid plugin id".to_owned());
    }
    let directory = plugins_dir(app)?.join(id);
    if directory.exists() {
        tokio::fs::remove_dir_all(&directory)
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub async fn apply_to_opencode(
    app: &AppHandle,
    config: &mut Value,
    enabled: &[String],
) -> Result<PathBuf, String> {
    let loader = write_loader(app).await?;
    let plugins = load_enabled(app, enabled).await?;
    let mut plugin_urls = config
        .get("plugin")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    plugin_urls.push(json!(file_url(&loader)?));
    let mut instructions = Vec::new();
    for plugin in &plugins {
        if let Some(module) = &plugin.module {
            plugin_urls.push(json!(file_url(module)?));
        }
        if let Some(text) = &plugin.instructions {
            instructions.push(format!("## {}\n\n{}", plugin.manifest.name, text.trim()));
        }
        if let Some(overlay) = &plugin.overlay {
            merge_overlay(config, overlay, &plugin.directory)?;
        }
    }
    config["plugin"] = Value::Array(plugin_urls);
    let path = instructions_path(app)?;
    tokio::fs::write(&path, instructions.join("\n\n"))
        .await
        .map_err(|error| error.to_string())?;
    Ok(path)
}

pub fn merge_overlay(config: &mut Value, overlay: &Value, plugin_dir: &Path) -> Result<(), String> {
    let Some(object) = overlay.as_object() else {
        return Ok(());
    };
    for (key, value) in object {
        if BLOCKED_OVERLAY_KEYS.contains(&key.as_str()) {
            continue;
        }
        if key == "plugin" {
            let extras = value
                .as_array()
                .ok_or_else(|| "plugin overlay must be an array".to_owned())?;
            let plugins = config
                .as_object_mut()
                .ok_or_else(|| "OpenCode config must be an object".to_owned())?
                .entry("plugin")
                .or_insert_with(|| json!([]));
            let list = plugins
                .as_array_mut()
                .ok_or_else(|| "plugin list is invalid".to_owned())?;
            for extra in extras {
                let relative = extra
                    .as_str()
                    .ok_or_else(|| "plugin overlay entries must be paths".to_owned())?;
                if relative.contains("..") {
                    return Err("Plugin overlay path cannot contain ..".to_owned());
                }
                let path = plugin_dir.join(relative.trim_start_matches("./"));
                list.push(json!(file_url(&path)?));
            }
            continue;
        }
        merge_value(
            config
                .as_object_mut()
                .ok_or_else(|| "OpenCode config must be an object".to_owned())?
                .entry(key.clone())
                .or_insert(Value::Null),
            value,
        );
    }
    Ok(())
}

fn merge_value(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                if BLOCKED_OVERLAY_KEYS.contains(&key.as_str()) {
                    continue;
                }
                merge_value(base.entry(key.clone()).or_insert(Value::Null), value);
            }
        }
        (base, overlay) => *base = overlay.clone(),
    }
}

fn file_url(path: &Path) -> Result<String, String> {
    Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| "Cannot convert plugin path to a file URL".to_owned())
}

async fn copy_dir(source: &Path, destination: &Path) -> Result<(), String> {
    tokio::fs::create_dir_all(destination)
        .await
        .map_err(|error| error.to_string())?;
    let mut entries = tokio::fs::read_dir(source)
        .await
        .map_err(|error| error.to_string())?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| error.to_string())?
    {
        let name = entry.file_name();
        if name == "node_modules" || name == ".git" {
            continue;
        }
        let from = entry.path();
        let to = destination.join(&name);
        if entry
            .file_type()
            .await
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            Box::pin(copy_dir(&from, &to)).await?;
        } else {
            tokio::fs::copy(&from, &to)
                .await
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_plugin_ids() {
        assert!(valid_plugin_id("concise"));
        assert!(valid_plugin_id("review-2"));
        assert!(!valid_plugin_id("Concise"));
        assert!(!valid_plugin_id("../secret"));
        assert!(!valid_plugin_id(""));
    }

    #[test]
    fn parse_manifest_requires_id_and_name() {
        let manifest = parse_manifest(&json!({
            "id": "concise",
            "name": "简洁模式",
            "description": "short",
            "version": "0.1.0"
        }))
        .unwrap();
        assert_eq!(manifest.id, "concise");
        assert!(parse_manifest(&json!({ "id": "concise" })).is_err());
    }

    #[test]
    fn overlay_cannot_replace_provider_or_model() {
        let mut config = json!({
            "model": "sub2api/keep",
            "provider": { "sub2api": { "apiKey": "secret" } },
            "plugin": ["bridge"]
        });
        merge_overlay(
            &mut config,
            &json!({
                "model": "hijack",
                "provider": { "evil": true },
                "agent": { "build": { "description": "review" } }
            }),
            Path::new("/tmp/plugin"),
        )
        .unwrap();
        assert_eq!(config["model"], "sub2api/keep");
        assert_eq!(config["provider"]["sub2api"]["apiKey"], "secret");
        assert_eq!(config["agent"]["build"]["description"], "review");
    }

    #[test]
    fn mixin_targets_and_files_are_constrained() {
        assert!(valid_mixin_target("ui.main"));
        assert!(valid_mixin_target("ui.composer.toolbar"));
        assert!(valid_mixin_target("ui.settings"));
        assert!(valid_mixin_target("chat.send"));
        assert!(!valid_mixin_target(""));
        assert!(!valid_mixin_target("UI.Main"));
        assert!(!valid_mixin_target("ui/main"));
        assert!(!valid_mixin_target("../etc"));
        assert!(valid_mixin_file("badge.html", ".html"));
        assert!(valid_mixin_file("send.js", ".js"));
        assert!(!valid_mixin_file("../x.html", ".html"));
        assert!(!valid_mixin_file("foo.js", ".html"));
        assert!(!valid_mixin_file("sub/badge.html", ".html"));
        assert!(valid_mixin_select(".composer-context"));
        assert!(valid_mixin_select(".settings-page"));
        assert!(!valid_mixin_select(""));
        assert!(!valid_mixin_select("<script>"));
    }

    #[test]
    fn bundled_harness_ships_lab_settings_mixin() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/harness/lab/plugin.json");
        let value: Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(value["id"], "lab");
        assert_eq!(value["mixins"][0]["select"], ".settings-page");
    }

    #[tokio::test]
    async fn resolve_mixins_reads_html_and_rejects_bad_targets() {
        let directory = tempfile::tempdir().unwrap();
        tokio::fs::write(directory.path().join("badge.html"), "<span>ok</span>")
            .await
            .unwrap();
        tokio::fs::write(directory.path().join("ui.html"), "<p>game</p>")
            .await
            .unwrap();
        let manifest = PluginManifest {
            id: "snake".into(),
            name: "贪吃蛇".into(),
            description: String::new(),
            version: "0.1.0".into(),
        };
        let mixins = resolve_mixins(
            directory.path(),
            &manifest,
            &json!({
                "mixins": [{
                    "select": ".composer-context",
                    "at": "RETURN",
                    "html": "badge.html"
                }]
            }),
        )
        .await
        .unwrap();
        assert_eq!(mixins.len(), 1);
        assert_eq!(mixins[0].select.as_deref(), Some(".composer-context"));
        assert_eq!(mixins[0].html.as_deref(), Some("<span>ok</span>"));

        let fallback = resolve_mixins(directory.path(), &manifest, &json!({}))
            .await
            .unwrap();
        assert!(fallback[0].target.is_empty());
        assert_eq!(fallback[0].html.as_deref(), Some("<p>game</p>"));

        let error = resolve_mixins(
            directory.path(),
            &manifest,
            &json!({ "mixins": [{ "target": "../secret", "html": "badge.html" }] }),
        )
        .await
        .unwrap_err();
        assert!(error.contains("invalid"));
    }
}
