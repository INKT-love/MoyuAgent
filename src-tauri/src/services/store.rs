//! Plugin and skill store: catalog parsing, safe package extraction and package inspection.
//!
//! This module has no Tauri dependency so it can be tested on its own. Permission summaries are
//! derived from the downloaded package itself, never from what the catalog claims.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use url::Url;

pub const MAX_CATALOG_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PACKAGE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_FILES: usize = 1000;
const MAX_ITEMS: usize = 500;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Plugin,
    Skill,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogItem {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    pub description: String,
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size: Option<u64>,
}

pub fn valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub fn valid_plugin_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit())
        && id.len() <= 63
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub fn valid_id(kind: Kind, id: &str) -> bool {
    match kind {
        Kind::Plugin => valid_plugin_id(id),
        Kind::Skill => valid_skill_name(id),
    }
}

/// HTTPS everywhere; plain HTTP only for loopback so a local catalog can be tested.
pub fn allowed_url(url: &Url) -> bool {
    match url.scheme() {
        "https" => url.host_str().is_some() && url.username().is_empty(),
        "http" => matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")),
        _ => false,
    }
}

pub fn parse_catalog_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value.trim()).map_err(|_| "商店地址不是有效的网址".to_owned())?;
    if !allowed_url(&url) {
        return Err("商店地址必须使用 https（本机测试可用 http://127.0.0.1）".to_owned());
    }
    Ok(url)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn clean_text(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_control() || *ch == ' ')
        .take(limit)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Parses a catalog. Invalid entries are skipped instead of failing the whole catalog.
pub fn parse_catalog(text: &str, base: &Url) -> Result<Vec<CatalogItem>, String> {
    let value: Value = serde_json::from_str(text).map_err(|_| "商店目录不是有效的 JSON".to_owned())?;
    let entries = value
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| "商店目录缺少 items 列表".to_owned())?;
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for entry in entries.iter().take(MAX_ITEMS) {
        let Ok(mut item) = serde_json::from_value::<CatalogItem>(entry.clone()) else {
            continue;
        };
        if !valid_id(item.kind, &item.id) || !seen.insert((item.kind, item.id.clone())) {
            continue;
        }
        let digest = item.sha256.trim().to_ascii_lowercase();
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            continue;
        }
        let Ok(resolved) = base.join(item.url.trim()) else {
            continue;
        };
        if !allowed_url(&resolved) {
            continue;
        }
        item.sha256 = digest;
        item.url = resolved.to_string();
        item.name = clean_text(&item.name, 80);
        item.description = clean_text(&item.description, 400);
        item.version = clean_text(&item.version, 32);
        item.author = clean_text(&item.author, 80);
        item.tags = item.tags.iter().map(|tag| clean_text(tag, 24)).filter(|tag| !tag.is_empty()).take(8).collect();
        if item.name.is_empty() || item.version.is_empty() {
            continue;
        }
        items.push(item);
    }
    Ok(items)
}

/// Dotted numeric versions compare numerically; anything else only counts as "different".
pub fn is_newer(installed: &str, available: &str) -> bool {
    fn parts(value: &str) -> Option<Vec<u64>> {
        value
            .trim_start_matches('v')
            .split('.')
            .map(|part| part.split(['-', '+']).next().unwrap_or("").parse().ok())
            .collect()
    }
    match (parts(installed), parts(available)) {
        (Some(mut left), Some(mut right)) => {
            let length = left.len().max(right.len());
            left.resize(length, 0);
            right.resize(length, 0);
            right > left
        }
        _ => installed != available,
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Capability {
    pub risk: Risk,
    pub title: String,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    pub version: String,
    pub capabilities: Vec<Capability>,
}

fn capability(risk: Risk, title: &str, detail: &str) -> Capability {
    Capability { risk, title: title.to_owned(), detail: detail.to_owned() }
}

fn read_limited(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > MAX_FILE_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// Frontmatter value for a single-line `key: value` entry.
fn frontmatter(text: &str, key: &str) -> Option<String> {
    let body = text.trim_start_matches('\u{feff}').strip_prefix("---")?;
    let end = body.find("\n---")?;
    body[..end].lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name.trim() == key).then(|| value.trim().trim_matches(['"', '\'']).to_owned())
    })
}

fn inspect_skill(dir: &Path) -> Result<Inspection, String> {
    let text = read_limited(&dir.join("SKILL.md")).ok_or_else(|| "技能包缺少 SKILL.md".to_owned())?;
    let name = frontmatter(&text, "name").unwrap_or_default();
    let description = frontmatter(&text, "description").unwrap_or_default();
    if !valid_skill_name(&name) {
        return Err("SKILL.md 的 name 不符合规范（小写字母、数字和单个短横线）".to_owned());
    }
    if description.is_empty() || description.chars().count() > 1024 {
        return Err("SKILL.md 需要 1 到 1024 字的 description".to_owned());
    }
    let mut capabilities = vec![capability(
        Risk::Low,
        "给 Agent 增加说明",
        "技能内容只在 Agent 需要时被读取，并按你当前的权限模式执行。",
    )];
    let mut scripts = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&current) else { continue };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Some(ext) = path.extension().and_then(|value| value.to_str()) {
                if matches!(ext.to_ascii_lowercase().as_str(), "sh" | "ps1" | "bat" | "cmd" | "py" | "js" | "mjs" | "ts" | "exe") {
                    scripts.push(path.strip_prefix(dir).unwrap_or(&path).to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    if !scripts.is_empty() {
        scripts.sort();
        capabilities.push(capability(
            Risk::Medium,
            "附带可执行脚本",
            &format!("Agent 可能运行这些文件：{}", scripts.iter().take(6).cloned().collect::<Vec<_>>().join("、")),
        ));
    }
    Ok(Inspection { id: name.clone(), kind: Kind::Skill, name, version: String::new(), capabilities })
}

fn inspect_plugin(dir: &Path) -> Result<Inspection, String> {
    let raw = read_limited(&dir.join("plugin.json")).ok_or_else(|| "插件包缺少 plugin.json".to_owned())?;
    let value: Value = serde_json::from_str(&raw).map_err(|_| "plugin.json 不是有效的 JSON".to_owned())?;
    let id = value.get("id").and_then(Value::as_str).unwrap_or("");
    if !valid_plugin_id(id) {
        return Err("plugin.json 的 id 只能含小写字母、数字和短横线".to_owned());
    }
    let name = value.get("name").and_then(Value::as_str).map(str::trim).filter(|name| !name.is_empty());
    let name = name.ok_or_else(|| "plugin.json 缺少 name".to_owned())?.to_owned();
    let version = value.get("version").and_then(Value::as_str).unwrap_or("0.1.0").trim().to_owned();
    let mut capabilities = Vec::new();
    if dir.join("backend.wasm").is_file() {
        capabilities.push(capability(
            Risk::High,
            "拦截后端命令",
            "WASM 后端会看到每次界面到后端的调用参数（包括登录邮箱和密码），可以拒绝或替换结果。它不能访问磁盘和网络。",
        ));
    }
    if ["index.mjs", "index.js"].iter().any(|file| dir.join(file).is_file()) {
        capabilities.push(capability(
            Risk::High,
            "在 OpenCode 里运行代码",
            "插件脚本会随 Agent 引擎启动，能读写文件、联网并使用 Agent 的 API 密钥环境。",
        ));
    }
    if let Some(patch) = read_limited(&dir.join("opencode.patch.json")) {
        let keys: Vec<String> = serde_json::from_str::<Value>(&patch)
            .ok()
            .and_then(|value| value.as_object().map(|object| object.keys().cloned().collect()))
            .unwrap_or_default();
        let detail = if keys.is_empty() { "补丁无法解析，安装时会被拒绝。".to_owned() } else { format!("会改动 Agent 配置：{}。不能改模型和线路。", keys.join("、")) };
        let risk = if keys.iter().any(|key| matches!(key.as_str(), "mcp" | "permission" | "plugin" | "command")) { Risk::High } else { Risk::Medium };
        capabilities.push(capability(risk, "修改 Agent 配置", &detail));
    }
    if dir.join("AGENTS.md").is_file() {
        capabilities.push(capability(Risk::Medium, "追加 Agent 提示词", "AGENTS.md 的内容会加入每次对话的系统提示。"));
    }
    let has_mixin = value.get("mixins").and_then(Value::as_array).is_some_and(|items| !items.is_empty()) || dir.join("ui.html").is_file();
    if has_mixin {
        let scripted = value.get("mixins").and_then(Value::as_array).is_some_and(|items| items.iter().any(|item| item.get("script").is_some()));
        capabilities.push(capability(
            if scripted { Risk::High } else { Risk::Medium },
            if scripted { "在界面里运行脚本" } else { "在界面里加入内容" },
            if scripted { "界面脚本能读取并修改当前页面上的内容。" } else { "页面片段跑在沙箱里，可调用你开放给插件的应用命令。" },
        ));
    }
    if capabilities.is_empty() {
        capabilities.push(capability(Risk::Low, "没有额外能力", "这个插件只包含基本信息。"));
    }
    Ok(Inspection { id: id.to_owned(), kind: Kind::Plugin, name, version, capabilities })
}

/// Inspects an extracted package and checks it is the item the catalog promised.
pub fn inspect_dir(kind: Kind, dir: &Path, expected_id: &str) -> Result<Inspection, String> {
    let mut found = match kind {
        Kind::Plugin => inspect_plugin(dir)?,
        Kind::Skill => inspect_skill(dir)?,
    };
    if found.id != expected_id {
        return Err(format!("安装包里的名称（{}）和目录里的（{expected_id}）不一致", found.id));
    }
    found.capabilities.sort_by_key(|item| std::cmp::Reverse(item.risk as u8));
    Ok(found)
}

const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const RESERVED_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "lpt1", "lpt2", "lpt3",
];

fn safe_component(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or("").to_ascii_lowercase();
    !part.is_empty()
        && part != "."
        && part != ".."
        && part != ".git"
        && !part.contains([':', '\\', '<', '>', '"', '|', '?', '*'])
        && !part.ends_with(['.', ' '])
        && !RESERVED_NAMES.contains(&stem.as_str())
}

/// Validated relative path for a zip entry name, or an error naming the offending entry.
fn entry_path(name: &str) -> Result<PathBuf, String> {
    let trimmed = name.trim_end_matches('/');
    if trimmed.starts_with('/') || trimmed.is_empty() {
        return Err(format!("安装包里有不安全的路径：{name}"));
    }
    let mut path = PathBuf::new();
    for part in trimmed.split('/') {
        if !safe_component(part) {
            return Err(format!("安装包里有不安全的路径：{name}"));
        }
        path.push(part);
    }
    Ok(path)
}

/// Extracts a zip into `destination` (which must not exist yet or be empty).
/// Rejects traversal, absolute paths, symlinks, odd file names and oversized archives.
/// A single shared top-level folder is removed so both zip layouts are accepted.
pub fn extract_zip(bytes: &[u8], destination: &Path) -> Result<usize, String> {
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err("安装包超过大小限制".to_owned());
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "安装包不是有效的 zip".to_owned())?;
    if archive.len() > MAX_FILES {
        return Err("安装包里的文件过多".to_owned());
    }
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|_| "安装包已损坏".to_owned())?;
        let is_link = entry.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000);
        if is_link {
            return Err(format!("安装包里不允许符号链接：{}", entry.name()));
        }
        entries.push((index, entry_path(entry.name())?, entry.is_dir()));
    }
    let strip = entries
        .iter()
        .map(|(_, path, _)| path.components().next())
        .collect::<Vec<_>>()
        .windows(2)
        .all(|pair| pair[0] == pair[1])
        && entries.iter().all(|(_, path, is_dir)| path.components().count() > 1 || *is_dir);
    std::fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    let mut total = 0u64;
    let mut files = 0usize;
    for (index, path, is_dir) in entries {
        let relative: PathBuf = if strip { path.components().skip(1).collect() } else { path };
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = destination.join(&relative);
        if is_dir {
            std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut entry = archive.by_index(index).map_err(|_| "安装包已损坏".to_owned())?;
        let mut data = Vec::new();
        entry
            .by_ref()
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut data)
            .map_err(|_| "安装包已损坏".to_owned())?;
        total += data.len() as u64;
        if data.len() as u64 > MAX_FILE_BYTES || total > MAX_TOTAL_BYTES {
            return Err("安装包解压后超过大小限制".to_owned());
        }
        std::fs::write(&target, data).map_err(|error| error.to_string())?;
        files += 1;
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn base() -> Url {
        Url::parse("https://store.example.com/moyu/index.json").unwrap()
    }

    fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, body) in files {
            writer.start_file(*name, options).unwrap();
            writer.write_all(body.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn urls_must_be_https_or_loopback_http() {
        assert!(allowed_url(&Url::parse("https://a.example/x").unwrap()));
        assert!(allowed_url(&Url::parse("http://127.0.0.1:8080/x").unwrap()));
        assert!(!allowed_url(&Url::parse("http://a.example/x").unwrap()));
        assert!(!allowed_url(&Url::parse("file:///etc/passwd").unwrap()));
        assert!(!allowed_url(&Url::parse("https://user:pw@a.example/x").unwrap()));
        assert!(parse_catalog_url("ftp://a.example/index.json").is_err());
    }

    #[test]
    fn catalog_skips_bad_entries_and_resolves_relative_urls() {
        let digest = "A".repeat(64);
        let text = serde_json::json!({ "items": [
            { "id": "concise", "kind": "plugin", "name": "简洁", "description": "d", "version": "1.0.0", "url": "pkg/concise.zip", "sha256": digest },
            { "id": "Bad_ID", "kind": "plugin", "name": "x", "description": "", "version": "1", "url": "a.zip", "sha256": digest },
            { "id": "git-release", "kind": "skill", "name": "发布", "description": "d", "version": "2", "url": "http://evil.example/a.zip", "sha256": digest },
            { "id": "git-release", "kind": "skill", "name": "发布", "description": "d", "version": "2", "url": "https://cdn.example/a.zip", "sha256": "short" },
            { "id": "concise", "kind": "plugin", "name": "重复", "description": "d", "version": "1", "url": "a.zip", "sha256": digest },
            { "id": "ok-skill", "kind": "skill", "name": "ok", "description": "d", "version": "1", "url": "https://cdn.example/a.zip", "sha256": digest }
        ]})
        .to_string();
        let items = parse_catalog(&text, &base()).unwrap();
        let ids: Vec<_> = items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, ["concise", "ok-skill"]);
        assert_eq!(items[0].url, "https://store.example.com/moyu/pkg/concise.zip");
        assert_eq!(items[0].sha256, "a".repeat(64));
        assert!(parse_catalog("{}", &base()).is_err());
        assert!(parse_catalog("nope", &base()).is_err());
    }

    #[test]
    fn newer_versions_compare_numerically() {
        assert!(is_newer("0.9.0", "0.10.0"));
        assert!(is_newer("1.0", "1.0.1"));
        assert!(!is_newer("1.2.0", "1.2"));
        assert!(!is_newer("2.0.0", "1.9.9"));
        assert!(is_newer("beta", "gamma"));
        assert!(!is_newer("beta", "beta"));
    }

    #[test]
    fn extract_strips_single_top_folder() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = zip_of(&[("pack/plugin.json", "{}"), ("pack/sub/a.txt", "a")]);
        let count = extract_zip(&bytes, &dir.path().join("out")).unwrap();
        assert_eq!(count, 2);
        assert!(dir.path().join("out/plugin.json").is_file());
        assert!(dir.path().join("out/sub/a.txt").is_file());
    }

    #[test]
    fn extract_keeps_flat_layout() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = zip_of(&[("plugin.json", "{}"), ("ui.html", "x")]);
        extract_zip(&bytes, &dir.path().join("out")).unwrap();
        assert!(dir.path().join("out/plugin.json").is_file());
    }

    #[test]
    fn extract_rejects_unsafe_entries() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["../evil.txt", "a/../../evil.txt", "/abs.txt", "C:/x.txt", "a\\b.txt", ".git/config", "nul.txt", "dots.."] {
            let bytes = zip_of(&[("plugin.json", "{}"), (name, "x")]);
            let result = extract_zip(&bytes, &dir.path().join("out"));
            assert!(result.is_err(), "{name} must be rejected");
        }
        assert!(!dir.path().join("evil.txt").exists());
    }

    #[test]
    fn extract_rejects_symlinks_and_bad_archives() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer.add_symlink("link", "/etc/passwd", options).unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        assert!(extract_zip(&bytes, &dir.path().join("a")).is_err());
        assert!(extract_zip(b"not a zip", &dir.path().join("b")).is_err());
    }

    #[test]
    fn extract_enforces_file_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        let big = "x".repeat(MAX_FILE_BYTES as usize + 1);
        let bytes = zip_of(&[("plugin.json", "{}"), ("big.bin", &big)]);
        assert!(extract_zip(&bytes, &dir.path().join("out")).is_err());
    }

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn plugin_inspection_reports_real_capabilities() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "plugin.json", r#"{"id":"probe","name":"探针","version":"1.2.0"}"#);
        write(dir.path(), "backend.wasm", "\0asm");
        write(dir.path(), "AGENTS.md", "hi");
        write(dir.path(), "opencode.patch.json", r#"{"mcp":{}}"#);
        let found = inspect_dir(Kind::Plugin, dir.path(), "probe").unwrap();
        assert_eq!(found.version, "1.2.0");
        let titles: Vec<_> = found.capabilities.iter().map(|item| item.title.as_str()).collect();
        assert!(titles.contains(&"拦截后端命令") && titles.contains(&"追加 Agent 提示词"));
        assert_eq!(found.capabilities[0].risk, Risk::High, "highest risk is listed first");
        assert!(inspect_dir(Kind::Plugin, dir.path(), "other").is_err(), "id must match the catalog");
    }

    #[test]
    fn plugin_without_extras_is_low_risk() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "plugin.json", r#"{"id":"plain","name":"Plain"}"#);
        let found = inspect_dir(Kind::Plugin, dir.path(), "plain").unwrap();
        assert_eq!(found.capabilities.len(), 1);
        assert_eq!(found.capabilities[0].risk, Risk::Low);
        write(dir.path(), "plugin.json", r#"{"id":"Bad Id","name":"x"}"#);
        assert!(inspect_dir(Kind::Plugin, dir.path(), "plain").is_err());
    }

    #[test]
    fn skill_inspection_validates_frontmatter_and_flags_scripts() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "SKILL.md", "---\nname: git-release\ndescription: Create releases\n---\nbody");
        let plain = inspect_dir(Kind::Skill, dir.path(), "git-release").unwrap();
        assert_eq!(plain.capabilities.len(), 1);
        std::fs::create_dir(dir.path().join("scripts")).unwrap();
        write(&dir.path().join("scripts"), "run.sh", "echo");
        let scripted = inspect_dir(Kind::Skill, dir.path(), "git-release").unwrap();
        assert_eq!(scripted.capabilities[0].risk, Risk::Medium);
        assert!(scripted.capabilities[0].detail.contains("scripts/run.sh"));
        write(dir.path(), "SKILL.md", "---\nname: Bad Name\ndescription: x\n---\n");
        assert!(inspect_dir(Kind::Skill, dir.path(), "git-release").is_err());
        write(dir.path(), "SKILL.md", "no frontmatter");
        assert!(inspect_dir(Kind::Skill, dir.path(), "git-release").is_err());
    }
}
