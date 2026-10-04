//! Meeting notes on disk. Recording and transcribing happen in the front end; this keeps the
//! transcript safe as it grows (a chunk at a time, so a crash or quit loses almost nothing) and
//! writes the notes when the meeting ends. Everything lives in <research root>/meetings/<date>-<title>/.

use crate::settings;
use serde::Serialize;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use tauri::AppHandle;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Meeting {
    pub dir: String,
}

fn safe_dir(app: &AppHandle, dir: &str) -> Result<PathBuf, String> {
    inside(&settings::research_root(app).join("meetings"), dir)
}

/// `dir` if it is a folder directly inside `root`. A last part of `..` or `.` would point
/// somewhere else (`meetings/..` is the research folder), so only a plain name counts.
fn inside(root: &Path, dir: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(dir);
    let plain = path.components().all(|c| !matches!(c, Component::ParentDir | Component::CurDir));
    if !plain || path.parent() != Some(root) || !path.is_dir() {
        return Err("That isn't one of Jarvis's meeting folders.".into());
    }
    Ok(path)
}

fn folder_name(title: &str, stamp: &str) -> String {
    let slug = crate::workspaces::slug(title);
    if slug.is_empty() {
        stamp.to_string()
    } else {
        format!("{stamp}-{slug}")
    }
}

/// Make a new folder for the meeting. Two meetings with the same title in the same minute get
/// separate folders (-2, -3…), so one never writes over the other's transcript.
fn begin_in(root: &Path, title: &str, stamp: &str, started: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(root).map_err(|e| format!("Couldn't create the meeting folder: {e}"))?;
    let base = folder_name(title, stamp);
    let mut n = 1;
    let dir = loop {
        let dir = root.join(if n == 1 { base.clone() } else { format!("{base}-{n}") });
        // create_dir (not create_dir_all) fails if the folder is already there, even if another
        // meeting made it a moment ago.
        match std::fs::create_dir(&dir) {
            Ok(()) => break dir,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 1000 => n += 1,
            Err(e) => return Err(format!("Couldn't create the meeting folder: {e}")),
        }
    };
    let name = if title.trim().is_empty() { "Meeting" } else { title.trim() };
    std::fs::write(dir.join("transcript.md"), format!("# {name}\n\nStarted {started}\n\n")).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Start a meeting folder with a transcript header.
#[tauri::command]
pub fn meeting_begin(app: AppHandle, title: String) -> Result<Meeting, String> {
    let now = chrono::Local::now();
    let root = settings::research_root(&app).join("meetings");
    let dir = begin_in(&root, &title, &now.format("%Y-%m-%d-%H%M").to_string(), &now.format("%A %d %B %Y, %H:%M").to_string())?;
    Ok(Meeting { dir: dir.display().to_string() })
}

/// Add a stretch of transcript.
#[tauri::command]
pub fn meeting_append(app: AppHandle, dir: String, text: String) -> Result<(), String> {
    let dir = safe_dir(&app, &dir)?;
    let mut f = std::fs::OpenOptions::new().append(true).open(dir.join("transcript.md")).map_err(|e| e.to_string())?;
    writeln!(f, "{}\n", text.trim()).map_err(|e| e.to_string())
}

/// Write the finished notes (summary, decisions, action items).
#[tauri::command]
pub fn meeting_finish(app: AppHandle, dir: String, notes: String) -> Result<String, String> {
    let dir = safe_dir(&app, &dir)?;
    std::fs::write(dir.join("notes.md"), notes).map_err(|e| e.to_string())?;
    Ok(dir.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_meeting_folder_is_named_and_started() {
        let root = std::env::temp_dir().join(format!("jarvis-meetings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = begin_in(&root, "Fikra pricing call", "2026-10-01-1500", "Thursday 01 October 2026, 15:00").unwrap();
        assert!(dir.ends_with("2026-10-01-1500-fikra-pricing-call"));
        let t = std::fs::read_to_string(dir.join("transcript.md")).unwrap();
        assert!(t.starts_with("# Fikra pricing call"));
        assert_eq!(folder_name("", "2026-10-01-1500"), "2026-10-01-1500");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn same_title_in_the_same_minute_gets_its_own_folder() {
        let root = std::env::temp_dir().join(format!("jarvis-meetings-twice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let first = begin_in(&root, "Standup", "2026-10-01-0900", "x").unwrap();
        meeting_text(&first, "first meeting's words");
        let second = begin_in(&root, "Standup", "2026-10-01-0900", "x").unwrap();
        let third = begin_in(&root, "Standup", "2026-10-01-0900", "x").unwrap();
        assert!(second.ends_with("2026-10-01-0900-standup-2"));
        assert!(third.ends_with("2026-10-01-0900-standup-3"));
        let t = std::fs::read_to_string(first.join("transcript.md")).unwrap();
        assert!(t.contains("first meeting's words"), "the first transcript was kept");
        let _ = std::fs::remove_dir_all(&root);
    }

    fn meeting_text(dir: &Path, text: &str) {
        let mut f = std::fs::OpenOptions::new().append(true).open(dir.join("transcript.md")).unwrap();
        writeln!(f, "{text}").unwrap();
    }

    #[test]
    fn only_folders_right_inside_meetings_are_accepted() {
        let root = std::env::temp_dir().join(format!("jarvis-meetings-safe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = begin_in(&root, "Call", "2026-10-01-1500", "x").unwrap();
        assert_eq!(inside(&root, &dir.display().to_string()).unwrap(), dir);
        let up = format!("{}/..", root.display());
        assert!(inside(&root, &up).is_err(), "meetings/.. is the research folder");
        assert!(inside(&root, &format!("{}/.", root.display())).is_err());
        assert!(inside(&root, &format!("{}/../meetings/x", root.display())).is_err());
        assert!(inside(&root, &root.join("missing").display().to_string()).is_err());
        assert!(inside(&root, "/tmp").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
