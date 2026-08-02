//! Local multi-session persistence (JSON under the app data dir).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::chat::{Message, Role};

const APP_DIR: &str = "uk.nandi.chat";
const SESSIONS_SUBDIR: &str = "sessions";
const TITLE_MAX: usize = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSession {
    pub id: String,
    pub title: String,
    pub updated_at_ms: u64,
    pub provider_id: String,
    pub model_id: String,
    pub messages: Vec<Message>,
}

impl SavedSession {
    pub fn age_label(&self) -> String {
        format_age(self.updated_at_ms)
    }
}

/// Resolve the sessions directory (creates it when possible).
pub fn sessions_dir() -> Option<PathBuf> {
    let candidates = session_dir_candidates();
    for dir in &candidates {
        if fs::create_dir_all(dir).is_ok() {
            return Some(dir.clone());
        }
    }
    candidates.into_iter().next()
}

fn session_dir_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();

    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        let p = PathBuf::from(xdg);
        if !p.as_os_str().is_empty() {
            out.push(p.join(APP_DIR).join(SESSIONS_SUBDIR));
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        out.push(
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join(APP_DIR)
                .join(SESSIONS_SUBDIR),
        );
    }

    // Android NativeActivity / adb-friendly fallbacks
    out.push(PathBuf::from(format!(
        "/data/data/{APP_DIR}/files/{SESSIONS_SUBDIR}"
    )));
    out.push(PathBuf::from(format!(
        "/data/local/tmp/{APP_DIR}/{SESSIONS_SUBDIR}"
    )));

    out
}

/// Newest-first listing of saved sessions.
pub fn list_sessions() -> Result<Vec<SavedSession>, String> {
    let Some(dir) = sessions_dir() else {
        return Err("no writable sessions directory".into());
    };
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let entries = fs::read_dir(&dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
    let mut sessions = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(session) = load_session_path(&path) {
            if !session.messages.is_empty() {
                sessions.push(session);
            }
        }
    }
    sessions.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    Ok(sessions)
}

pub fn load_session(id: &str) -> Result<SavedSession, String> {
    let path = session_path(id)?;
    load_session_path(&path)
}

fn load_session_path(path: &Path) -> Result<SavedSession, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
}

pub fn save_session(session: &SavedSession) -> Result<(), String> {
    let path = session_path(&session.id)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(session).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, &path).map_err(|e| format!("rename {}: {e}", path.display()))?;
    Ok(())
}

fn session_path(id: &str) -> Result<PathBuf, String> {
    let id = id.trim();
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err("invalid session id".into());
    }
    let dir = sessions_dir().ok_or_else(|| "no writable sessions directory".to_string())?;
    Ok(dir.join(format!("{id}.json")))
}

/// Allocate a new session id (millis since epoch).
pub fn new_session_id() -> String {
    now_ms().to_string()
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Title from the first user message.
pub fn title_from_messages(messages: &[Message]) -> String {
    let raw = messages
        .iter()
        .find(|m| m.role == Role::User)
        .map(|m| m.content.as_str())
        .unwrap_or("Untitled")
        .trim();
    if raw.is_empty() {
        return "Untitled".into();
    }
    let collapsed: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= TITLE_MAX {
        collapsed
    } else {
        let t: String = collapsed.chars().take(TITLE_MAX.saturating_sub(1)).collect();
        format!("{t}…")
    }
}

fn format_age(updated_at_ms: u64) -> String {
    let now = now_ms();
    let age = Duration::from_millis(now.saturating_sub(updated_at_ms));
    let secs = age.as_secs();
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{Message, Role};
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn title_truncates() {
        let messages = vec![Message {
            role: Role::User,
            content: "hello   world  ".into(),
        }];
        assert_eq!(title_from_messages(&messages), "hello world");
    }

    #[test]
    fn save_load_roundtrip() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "uk.nandi.chat-test-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("XDG_DATA_HOME", &dir);

        let id = new_session_id();
        let session = SavedSession {
            id: id.clone(),
            title: "hi".into(),
            updated_at_ms: now_ms(),
            provider_id: "openrouter".into(),
            model_id: "test-model".into(),
            messages: vec![
                Message {
                    role: Role::User,
                    content: "hi".into(),
                },
                Message {
                    role: Role::Assistant,
                    content: "hello".into(),
                },
            ],
        };
        save_session(&session).expect("save");
        let loaded = load_session(&id).expect("load");
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.model_id, "test-model");
        let listed = list_sessions().expect("list");
        assert!(listed.iter().any(|s| s.id == id));

        let _ = fs::remove_dir_all(&dir);
        std::env::remove_var("XDG_DATA_HOME");
    }
}
