//! The approval record. Every action that went through an approval (approved, declined, blocked,
//! or allowed automatically by a setting) is appended to <research root>/audit.jsonl, one JSON
//! object per line, so there's always a trail of what Jarvis did on the user's behalf.
//!
//! The decisions themselves happen in the UI (src/lib/approvals.ts), where the tools run.

use crate::settings;
use serde_json::{json, Value};
use std::io::Write;
use tauri::AppHandle;

#[tauri::command]
pub fn audit_log(app: AppHandle, entry: Value) -> Result<(), String> {
    let path = settings::research_root(&app).join("audit.jsonl");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let at = chrono::Local::now().to_rfc3339();
    let line = match entry {
        Value::Object(mut m) => {
            m.insert("at".into(), json!(at));
            Value::Object(m)
        }
        other => json!({ "at": at, "entry": other }),
    };
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| e.to_string())?;
    writeln!(f, "{line}").map_err(|e| e.to_string())
}

/// Bring Jarvis forward when an approval is waiting and it isn't in front (it often sits in the menu bar).
#[tauri::command]
pub fn notify_approval(app: AppHandle, title: String, body: String) {
    use tauri::Manager;
    use tauri_plugin_notification::NotificationExt;
    let in_front = app
        .get_webview_window("main")
        .map(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false))
        .unwrap_or(false);
    if !in_front {
        // The approval card lives in the window, so bring it forward as well as notifying.
        crate::show_window(&app);
        let _ = app.notification().builder().title(title).body(body).show();
    }
}
