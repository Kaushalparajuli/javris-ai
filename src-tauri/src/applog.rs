//! Small append-only diagnostics log at ~/Jarvis/logs/<name>.log (rotated at 1 MB).

use crate::settings;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::AppHandle;

#[tauri::command]
pub fn app_log(app: AppHandle, name: String, line: String) {
    let safe: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    let dir = settings::research_root(&app).join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{}.log", if safe.is_empty() { "app" } else { &safe }));
    if std::fs::metadata(&path).map(|m| m.len() > 1_000_000).unwrap_or(false) {
        let _ = std::fs::rename(&path, path.with_extension("log.1"));
    }
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{secs} {}", line.replace('\n', " "));
    }
}
