//! Saved conversations.
//!
//! Each chat is one JSON file under <research root>/chats, so a conversation
//! survives quitting the app and the user can keep several going at once.
//! A chat can belong to a project (a workspace): opening it switches Jarvis to that project.

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
    /// Slug of the project this chat belongs to, or empty for none.
    pub workspace: String,
    /// Kept at the top of its list.
    pub pinned: bool,
    /// The title was written from the conversation (or by the user), so saving the transcript
    /// must not replace it with the first message again.
    pub title_set: bool,
    /// The user renamed the chat: Jarvis never retitles it.
    pub title_locked: bool,
}

impl Default for Chat {
    fn default() -> Self {
        Self { id: String::new(), title: String::new(), created_at: 0, updated_at: 0, messages: vec![], workspace: String::new(), pinned: false, title_set: false, title_locked: false }
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
    workspace: String,
    pinned: bool,
    title_locked: bool,
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
            workspace: c.workspace,
            pinned: c.pinned,
            title_locked: c.title_locked,
        })
        .collect();
    out.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.updated_at.cmp(&a.updated_at)));
    out
}

#[tauri::command]
pub fn load_chat(app: AppHandle, id: String) -> Option<Chat> {
    read_chat(&chat_file(&app, &id)?)
}

/// An existing project's slug, or empty. Chats never point at a project that isn't there.
fn check_workspace(app: &AppHandle, workspace: Option<String>) -> Result<String, String> {
    let ws = workspace.unwrap_or_default();
    if ws.is_empty() || crate::workspaces::load(app, &ws).is_some() {
        Ok(ws)
    } else {
        Err("There's no project by that name.".into())
    }
}

/// Start a new, empty conversation, in a project if `workspace` names one, and return it.
#[tauri::command]
pub fn new_chat(app: AppHandle, workspace: Option<String>) -> Result<Chat, String> {
    let workspace = check_workspace(&app, workspace)?;
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
    let chat = Chat { id, title: String::new(), created_at: now, updated_at: now, messages: vec![], workspace, pinned: false, title_set: false, title_locked: false };
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

/// Store the transcript. `created_at` and the project are kept from the existing file so the
/// conversation's age doesn't reset on every save and it stays where it was filed.
#[tauri::command]
pub fn save_chat(app: AppHandle, id: String, title: String, messages: Vec<ChatMsg>) -> Result<Chat, String> {
    let path = chat_file(&app, &id).ok_or("Bad chat id")?;
    let now = now_ms();
    let old = read_chat(&path);
    let created_at = old.as_ref().map(|c| c.created_at).unwrap_or(now);
    let old = old.unwrap_or_default();
    // Once the chat has a real title, saving the transcript leaves it alone.
    let title = if old.title_set { old.title.clone() } else { title };
    let chat = Chat { id, title, created_at, updated_at: now, messages, workspace: old.workspace, pinned: old.pinned, title_set: old.title_set, title_locked: old.title_locked };
    write(&app, &chat)?;
    Ok(chat)
}

/// The user renamed the chat: the title sticks, and Jarvis stops retitling it.
#[tauri::command]
pub fn rename_chat(app: AppHandle, id: String, title: String) -> Result<(), String> {
    let path = chat_file(&app, &id).ok_or("Bad chat id")?;
    let mut chat = read_chat(&path).ok_or("No such chat")?;
    let title = title.trim().to_string();
    if title.is_empty() {
        // An empty name hands the title back to Jarvis.
        chat.title_locked = false;
        chat.title_set = false;
    } else {
        chat.title = title;
        chat.title_locked = true;
        chat.title_set = true;
    }
    write(&app, &chat)
}

/// Jarvis named the chat from what it's about. Ignored if the user has renamed it.
/// Doesn't touch `updated_at`, so a new title doesn't reorder the list.
#[tauri::command]
pub fn set_chat_title(app: AppHandle, id: String, title: String) -> Result<(), String> {
    let path = chat_file(&app, &id).ok_or("Bad chat id")?;
    let mut chat = read_chat(&path).ok_or("No such chat")?;
    let title = title.trim().to_string();
    if chat.title_locked || title.is_empty() {
        return Ok(());
    }
    chat.title = title;
    chat.title_set = true;
    write(&app, &chat)
}

/// File a chat under a project, or under none with an empty `workspace`.
#[tauri::command]
pub fn move_chat(app: AppHandle, id: String, workspace: String) -> Result<(), String> {
    let workspace = check_workspace(&app, Some(workspace))?;
    let path = chat_file(&app, &id).ok_or("Bad chat id")?;
    let mut chat = read_chat(&path).ok_or("No such chat")?;
    chat.workspace = workspace;
    write(&app, &chat)
}

#[tauri::command]
pub fn pin_chat(app: AppHandle, id: String, pinned: bool) -> Result<(), String> {
    let path = chat_file(&app, &id).ok_or("Bad chat id")?;
    let mut chat = read_chat(&path).ok_or("No such chat")?;
    chat.pinned = pinned;
    write(&app, &chat)
}

/// A project was deleted: its chats stay, outside any project.
pub(crate) fn unfile(app: &AppHandle, workspace: &str) {
    for mut chat in all_chats(app).into_iter().filter(|c| c.workspace == workspace) {
        chat.workspace.clear();
        let _ = write(app, &chat);
    }
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
