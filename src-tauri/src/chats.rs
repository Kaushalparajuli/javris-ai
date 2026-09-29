//! Saved conversations.
//!
//! Each chat is one JSON file under <research root>/chats, so a conversation
//! survives quitting the app and the user can keep several going at once.

use crate::settings;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::AppHandle;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMsg {
    pub id: u32,
    /// "you", "jarvis", "tool" or "notice".
    pub who: String,
    pub text: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Chat {
    pub id: String,
    pub title: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub messages: Vec<ChatMsg>,
}

impl Default for Chat {
    fn default() -> Self {
        Self { id: String::new(), title: String::new(), created_at: 0, updated_at: 0, messages: vec![] }
    }
}

/// What the sidebar needs, without dragging every transcript into memory.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSummary {
    id: String,
    title: String,
    created_at: u64,
    updated_at: u64,
    count: usize,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn chats_dir(app: &AppHandle) -> PathBuf {
    settings::research_root(app).join("chats")
}

/// Ids are generated here, but they arrive back from the UI as plain strings,
/// so only ever trust a safe filename.
fn safe_id(id: &str) -> Option<String> {
    let ok = !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    ok.then(|| id.to_string())
}

fn chat_file(app: &AppHandle, id: &str) -> Option<PathBuf> {
    Some(chats_dir(app).join(format!("{}.json", safe_id(id)?)))
}

fn read_chat(path: &PathBuf) -> Option<Chat> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Every saved conversation in full, for search.
pub(crate) fn all_chats(app: &AppHandle) -> Vec<Chat> {
    std::fs::read_dir(chats_dir(app))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| read_chat(&e.path()))
        .collect()
}

#[tauri::command]
pub fn list_chats(app: AppHandle) -> Vec<ChatSummary> {
    let mut out: Vec<ChatSummary> = std::fs::read_dir(chats_dir(&app))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| read_chat(&e.path()))
        .map(|c| ChatSummary {
            id: c.id,
            title: c.title,
            created_at: c.created_at,
            updated_at: c.updated_at,
            count: c.messages.len(),
        })
        .collect();
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    out
}

#[tauri::command]
pub fn load_chat(app: AppHandle, id: String) -> Option<Chat> {
    read_chat(&chat_file(&app, &id)?)
}

/// Start a new, empty conversation and return it.
#[tauri::command]
pub fn new_chat(app: AppHandle) -> Result<Chat, String> {
    let now = now_ms();
    let dir = chats_dir(&app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Same millisecond twice is unlikely but cheap to rule out.
    let mut id = format!("c{now}");
    let mut n = 1;
    while dir.join(format!("{id}.json")).exists() {
        id = format!("c{now}-{n}");
        n += 1;
    }
    let chat = Chat { id, title: String::new(), created_at: now, updated_at: now, messages: vec![] };
    write(&app, &chat)?;
    Ok(chat)
}

fn write(app: &AppHandle, chat: &Chat) -> Result<(), String> {
    let path = chat_file(app, &chat.id).ok_or("Bad chat id")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(chat).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

/// Store the transcript. `created_at` is kept from the existing file so the
/// conversation's age doesn't reset on every save.
#[tauri::command]
pub fn save_chat(app: AppHandle, id: String, title: String, messages: Vec<ChatMsg>) -> Result<Chat, String> {
    let path = chat_file(&app, &id).ok_or("Bad chat id")?;
    let now = now_ms();
    let created_at = read_chat(&path).map(|c| c.created_at).unwrap_or(now);
    let chat = Chat { id, title, created_at, updated_at: now, messages };
    write(&app, &chat)?;
    Ok(chat)
}

#[tauri::command]
pub fn rename_chat(app: AppHandle, id: String, title: String) -> Result<(), String> {
    let path = chat_file(&app, &id).ok_or("Bad chat id")?;
    let mut chat = read_chat(&path).ok_or("No such chat")?;
    chat.title = title;
    chat.updated_at = now_ms();
    write(&app, &chat)
}

#[tauri::command]
pub fn delete_chat(app: AppHandle, id: String) -> Result<(), String> {
    let path = chat_file(&app, &id).ok_or("Bad chat id")?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
