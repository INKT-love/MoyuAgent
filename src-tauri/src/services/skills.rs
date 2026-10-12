//! Installed skills live in one global folder that OpenCode reads through `skills.paths`.
//! Disabling a skill keeps its files and adds a `permission.skill` deny rule instead.

use super::config;
use super::store;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tauri::AppHandle;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub enabled: bool,
}

pub fn skills_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config::app_directory(app)?.join("skills"))
}

/// Registers the skills folder and hides skills the user turned off.
pub fn apply_to_config(content: &mut Value, root: &Path, disabled: &[String]) {
    content["skills"] = json!({ "paths": [root.to_string_lossy()] });
    if !content.get("permission").is_some_and(Value::is_object) {
        content["permission"] = json!({});
    }
    let mut rules = serde_json::Map::new();
    for name in disabled.iter().filter(|name| store::valid_skill_name(name)) {
        rules.insert(name.clone(), json!("deny"));
    }
    if !rules.is_empty() {
        content["permission"]["skill"] = Value::Object(rules);
    }
}

fn frontmatter_value(text: &str, key: &str) -> Option<String> {
    let body = text.trim_start_matches('\u{feff}').strip_prefix("---")?;
    let end = body.find("\n---")?;
    body[..end].lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name.trim() == key).then(|| value.trim().trim_matches(['"', '\'']).to_owned())
    })
}

pub async fn list(root: &Path, disabled: &[String]) -> Vec<SkillInfo> {
    let Ok(mut entries) = tokio::fs::read_dir(root).await else {
        return Vec::new();
    };
    let mut skills = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !store::valid_skill_name(&name) {
            continue;
        }
        let Ok(text) = tokio::fs::read_to_string(entry.path().join("SKILL.md")).await else {
            continue;
        };
        skills.push(SkillInfo {
            description: frontmatter_value(&text, "description").unwrap_or_default(),
            enabled: !disabled.contains(&name),
            name,
        });
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    skills
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_registers_root_and_denies_disabled_skills() {
        let mut content = json!({ "permission": { "*": "allow", "skill": { "x": "allow" } } });
        apply_to_config(&mut content, Path::new("/data/skills"), &["git-release".into(), "Bad Name".into()]);
        assert_eq!(content["skills"]["paths"][0], "/data/skills");
        assert_eq!(content["permission"]["*"], "allow");
        assert_eq!(content["permission"]["skill"], json!({ "git-release": "deny" }));
    }

    #[test]
    fn config_without_disabled_skills_leaves_permissions_alone() {
        let mut content = json!({});
        apply_to_config(&mut content, Path::new("/s"), &[]);
        assert!(content["permission"].as_object().unwrap().is_empty());
    }

    #[tokio::test]
    async fn list_reads_valid_skills_only() {
        let dir = tempfile::tempdir().unwrap();
        for (folder, body) in [
            ("git-release", "---\nname: git-release\ndescription: Make releases\n---\n"),
            ("Bad_Name", "---\nname: x\ndescription: y\n---\n"),
        ] {
            std::fs::create_dir(dir.path().join(folder)).unwrap();
            std::fs::write(dir.path().join(folder).join("SKILL.md"), body).unwrap();
        }
        std::fs::create_dir(dir.path().join("no-file")).unwrap();
        let skills = list(dir.path(), &["git-release".into()]).await;
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].description, "Make releases");
        assert!(!skills[0].enabled);
    }
}
