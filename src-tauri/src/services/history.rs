use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "conversations.json";
const STORE_KEY: &str = "histories";
const MAX_CONVERSATIONS: usize = 80;
const MAX_MESSAGES: usize = 200;
const MAX_ID: usize = 80;
const MAX_TITLE: usize = 120;
const MAX_DRAFT: usize = 64 * 1024;
const MAX_TEXT: usize = 64 * 1024;
const MAX_ERROR: usize = 2000;
const MAX_SESSION: usize = 160;
const MAX_WORKSPACE: usize = 1024;
const MAX_STORE_CHARS: usize = 1_500_000;

#[derive(Clone, Copy)]
pub enum SanitizeMode {
    Persist,
    Restore,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySnapshot {
    #[serde(default)]
    pub active_id: String,
    #[serde(default)]
    pub conversations: Vec<ConversationRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRecord {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub messages: Vec<MessageRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub workspace: String,
    #[serde(default)]
    pub draft: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRecord {
    pub id: String,
    pub role: String,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Histories {
    #[serde(default)]
    users: HashMap<String, HistorySnapshot>,
}

pub async fn load(app: &AppHandle, user_id: i64) -> Result<HistorySnapshot, String> {
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        let store = open(&app)?;
        let histories = read_histories(&store)?;
        Ok(sanitize_snapshot(
            histories
                .users
                .get(&user_id.to_string())
                .cloned()
                .unwrap_or_default(),
            SanitizeMode::Restore,
        ))
    })
    .await
    .map_err(|error| error.to_string())?
}

pub async fn save(app: &AppHandle, user_id: i64, snapshot: HistorySnapshot) -> Result<(), String> {
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        let store = open(&app)?;
        let mut histories = read_histories(&store)?;
        histories.users.insert(
            user_id.to_string(),
            sanitize_snapshot(snapshot, SanitizeMode::Persist),
        );
        store.set(
            STORE_KEY,
            serde_json::to_value(&histories).map_err(|error| error.to_string())?,
        );
        store
            .save()
            .map_err(|error| format!("Cannot save conversations: {error}"))
    })
    .await
    .map_err(|error| error.to_string())?
}

fn open(app: &AppHandle) -> Result<std::sync::Arc<tauri_plugin_store::Store<tauri::Wry>>, String> {
    app.store_builder(STORE_FILE)
        .disable_auto_save()
        .build()
        .map_err(|error| format!("Cannot open conversations: {error}"))
}

fn read_histories(store: &tauri_plugin_store::Store<tauri::Wry>) -> Result<Histories, String> {
    match store.get(STORE_KEY) {
        None => Ok(Histories::default()),
        Some(value) => Ok(serde_json::from_value(value).unwrap_or_default()),
    }
}

pub fn sanitize_snapshot(snapshot: HistorySnapshot, mode: SanitizeMode) -> HistorySnapshot {
    let mut seen = HashSet::new();
    let mut conversations = Vec::new();
    for conversation in snapshot.conversations {
        let Some(record) = sanitize_conversation(conversation, mode) else {
            continue;
        };
        if !seen.insert(record.id.clone()) {
            continue;
        }
        conversations.push(record);
    }
    let active_id = if valid_id(&snapshot.active_id) && seen.contains(&snapshot.active_id) {
        snapshot.active_id
    } else {
        conversations
            .iter()
            .find(|item| !item.archived)
            .or_else(|| conversations.first())
            .map(|item| item.id.clone())
            .unwrap_or_default()
    };
    conversations.retain(|item| {
        item.id == active_id
            || !item.messages.is_empty()
            || item.pinned
            || (item.id == active_id && !item.draft.is_empty())
    });
    cap_conversations(&mut conversations, &active_id);
    HistorySnapshot {
        active_id,
        conversations,
    }
}

fn sanitize_conversation(
    conversation: ConversationRecord,
    mode: SanitizeMode,
) -> Option<ConversationRecord> {
    if !valid_id(&conversation.id) {
        return None;
    }
    let mut seen = HashSet::new();
    let mut messages = Vec::new();
    for message in conversation.messages {
        let Some(record) = sanitize_message(message, mode) else {
            continue;
        };
        if !seen.insert(record.id.clone()) {
            continue;
        }
        messages.push(record);
    }
    if messages.len() > MAX_MESSAGES {
        messages.drain(..messages.len() - MAX_MESSAGES);
    }
    Some(ConversationRecord {
        id: conversation.id,
        title: clip(&conversation.title, MAX_TITLE),
        messages,
        session_id: conversation
            .session_id
            .filter(|value| !value.is_empty())
            .map(|value| clip(&value, MAX_SESSION)),
        workspace: clip(&conversation.workspace, MAX_WORKSPACE),
        draft: clip(&conversation.draft, MAX_DRAFT),
        pinned: conversation.pinned && !conversation.archived,
        archived: conversation.archived,
    })
}

fn sanitize_message(message: MessageRecord, mode: SanitizeMode) -> Option<MessageRecord> {
    if !valid_id(&message.id) {
        return None;
    }
    let role = match message.role.as_str() {
        "user" | "assistant" => message.role,
        _ => return None,
    };
    let mut state = match message.state.as_deref() {
        Some("streaming" | "completed" | "failed" | "cancelled") => message.state,
        Some(_) if role == "assistant" && !message.text.is_empty() => Some("completed".to_owned()),
        _ => None,
    };
    if matches!(mode, SanitizeMode::Restore) && state.as_deref() == Some("streaming") {
        state = Some("cancelled".to_owned());
    }
    if role == "user" {
        state = None;
    }
    Some(MessageRecord {
        id: message.id,
        role,
        text: clip(&message.text, MAX_TEXT),
        state,
        error: message
            .error
            .filter(|value| !value.is_empty())
            .map(|value| clip(&value, MAX_ERROR)),
    })
}

fn cap_conversations(conversations: &mut Vec<ConversationRecord>, active_id: &str) {
    let mut chars = conversations.iter().map(conversation_chars).sum::<usize>();
    let over_count = |items: &[ConversationRecord]| items.len() > MAX_CONVERSATIONS;
    let over_chars = |total: usize| total > MAX_STORE_CHARS;
    let mut index = conversations.len();
    while index > 0 && (over_count(conversations) || over_chars(chars)) {
        index -= 1;
        if conversations[index].id == active_id || conversations[index].pinned {
            continue;
        }
        if conversations[index].archived || over_count(conversations) || over_chars(chars) {
            chars = chars.saturating_sub(conversation_chars(&conversations[index]));
            conversations.remove(index);
        }
    }
    index = conversations.len();
    while index > 0 && (over_count(conversations) || over_chars(chars)) {
        index -= 1;
        if conversations[index].id == active_id {
            continue;
        }
        chars = chars.saturating_sub(conversation_chars(&conversations[index]));
        conversations.remove(index);
    }
}

fn conversation_chars(conversation: &ConversationRecord) -> usize {
    conversation.title.chars().count()
        + conversation.draft.chars().count()
        + conversation
            .messages
            .iter()
            .map(|message| {
                message.text.chars().count()
                    + message
                        .error
                        .as_ref()
                        .map(|value| value.chars().count())
                        .unwrap_or(0)
            })
            .sum::<usize>()
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn clip(value: &str, max: usize) -> String {
    match value.chars().count() <= max {
        true => value.to_owned(),
        false => value.chars().take(max).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: &str, role: &str, text: &str, state: Option<&str>) -> MessageRecord {
        MessageRecord {
            id: id.into(),
            role: role.into(),
            text: text.into(),
            state: state.map(str::to_owned),
            error: None,
        }
    }

    fn conversation(id: &str, messages: Vec<MessageRecord>) -> ConversationRecord {
        ConversationRecord {
            id: id.into(),
            title: id.into(),
            messages,
            session_id: None,
            workspace: r"D:\workspace".into(),
            draft: String::new(),
            pinned: false,
            archived: false,
        }
    }

    #[test]
    fn restore_marks_interrupted_streams_cancelled() {
        let snapshot = sanitize_snapshot(
            HistorySnapshot {
                active_id: "c1".into(),
                conversations: vec![conversation(
                    "c1",
                    vec![
                        message("u1", "user", "hello", None),
                        message("a1", "assistant", "partial", Some("streaming")),
                    ],
                )],
            },
            SanitizeMode::Restore,
        );
        assert_eq!(
            snapshot.conversations[0].messages[1].state.as_deref(),
            Some("cancelled")
        );
    }

    #[test]
    fn persist_keeps_in_progress_streams() {
        let snapshot = sanitize_snapshot(
            HistorySnapshot {
                active_id: "c1".into(),
                conversations: vec![conversation(
                    "c1",
                    vec![message("a1", "assistant", "partial", Some("streaming"))],
                )],
            },
            SanitizeMode::Persist,
        );
        assert_eq!(
            snapshot.conversations[0].messages[0].state.as_deref(),
            Some("streaming")
        );
    }

    #[test]
    fn empty_drafts_are_dropped_except_the_active_conversation() {
        let snapshot = sanitize_snapshot(
            HistorySnapshot {
                active_id: "keep".into(),
                conversations: vec![
                    conversation("keep", vec![]),
                    conversation("drop", vec![]),
                    conversation("saved", vec![message("u1", "user", "hi", None)]),
                ],
            },
            SanitizeMode::Persist,
        );
        let ids: Vec<_> = snapshot
            .conversations
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(ids, ["keep", "saved"]);
        assert_eq!(snapshot.active_id, "keep");
    }

    #[test]
    fn users_are_isolated_in_the_store_map() {
        let mut histories = Histories::default();
        histories.users.insert(
            "1".into(),
            HistorySnapshot {
                active_id: "mine".into(),
                conversations: vec![conversation(
                    "mine",
                    vec![message("u1", "user", "account one", None)],
                )],
            },
        );
        histories.users.insert(
            "2".into(),
            HistorySnapshot {
                active_id: "theirs".into(),
                conversations: vec![conversation(
                    "theirs",
                    vec![message("u2", "user", "account two", None)],
                )],
            },
        );
        assert_eq!(
            histories.users.get("1").unwrap().conversations[0].messages[0].text,
            "account one"
        );
        assert_eq!(
            histories.users.get("2").unwrap().conversations[0].messages[0].text,
            "account two"
        );
        histories
            .users
            .insert("2".into(), HistorySnapshot::default());
        assert_eq!(histories.users.get("1").unwrap().active_id, "mine");
        assert!(histories.users.get("2").unwrap().conversations.is_empty());
    }

    #[test]
    fn invalid_roles_and_duplicate_ids_are_removed() {
        let snapshot = sanitize_snapshot(
            HistorySnapshot {
                active_id: "c1".into(),
                conversations: vec![
                    conversation(
                        "c1",
                        vec![
                            message("u1", "user", "ok", None),
                            message("bad", "system", "nope", None),
                            message("u1", "user", "duplicate", None),
                        ],
                    ),
                    conversation("c1", vec![message("u2", "user", "dup conversation", None)]),
                ],
            },
            SanitizeMode::Persist,
        );
        assert_eq!(snapshot.conversations.len(), 1);
        assert_eq!(snapshot.conversations[0].messages.len(), 1);
        assert_eq!(snapshot.conversations[0].messages[0].text, "ok");
    }

    #[test]
    fn conversation_cap_keeps_pinned_and_active() {
        let mut conversations = vec![conversation(
            "active",
            vec![message("u0", "user", "now", None)],
        )];
        conversations.extend((1..=MAX_CONVERSATIONS).map(|index| {
            let mut item = conversation(
                &format!("old-{index}"),
                vec![message("u", "user", "older", None)],
            );
            item.pinned = index == 1;
            item
        }));
        let snapshot = sanitize_snapshot(
            HistorySnapshot {
                active_id: "active".into(),
                conversations,
            },
            SanitizeMode::Persist,
        );
        assert!(snapshot.conversations.len() <= MAX_CONVERSATIONS);
        assert!(snapshot
            .conversations
            .iter()
            .any(|item| item.id == "active"));
        assert!(snapshot.conversations.iter().any(|item| item.id == "old-1"));
    }

    #[test]
    fn message_cap_keeps_the_newest_turns() {
        let messages = (0..=MAX_MESSAGES)
            .map(|index| message(&format!("m{index}"), "user", &format!("turn {index}"), None))
            .collect();
        let snapshot = sanitize_snapshot(
            HistorySnapshot {
                active_id: "c1".into(),
                conversations: vec![conversation("c1", messages)],
            },
            SanitizeMode::Persist,
        );
        assert_eq!(snapshot.conversations[0].messages.len(), MAX_MESSAGES);
        assert_eq!(snapshot.conversations[0].messages[0].text, "turn 1");
        assert_eq!(
            snapshot.conversations[0].messages.last().unwrap().text,
            format!("turn {MAX_MESSAGES}")
        );
    }
}
