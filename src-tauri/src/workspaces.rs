//! Project workspaces: "Fikra", "GSoft", "Personal". Each has its own folder under
//! <research root>/projects/<slug>/ holding workspace.json (what it is, where its code lives, how to
//! check it) and room for notes. Memories are tagged by workspace, and code fixes default to the
//! workspace's project folder. The active workspace is kept in settings.

use crate::settings;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::AppHandle;

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Workspace {
    /// Folder name under projects/, lowercase with dashes. Set from `name` when saving.
    pub slug: String,
    pub name: String,
    /// What this project is, in a sentence or two. Goes into Jarvis's instructions.
    pub description: String,
    /// The folder on this Mac where the project's code lives, if it has some.
    pub folder: String,
    /// Commands that check the project (tests, type check, build), run after code changes.
    pub verify: Vec<String>,
    /// A web address to open and look at after a change, for web projects.
    pub url: String,
    pub created: u64,
}

pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_matches('-').chars().take(40).collect()
}

fn root(app: &AppHandle) -> PathBuf {
    settings::research_root(app).join("projects")
}

fn file(app: &AppHandle, slug: &str) -> PathBuf {
    root(app).join(slug).join("workspace.json")
}

pub fn load(app: &AppHandle, slug: &str) -> Option<Workspace> {
    if slug.is_empty() || slug.contains('/') || slug.contains("..") {
        return None;
    }
    serde_json::from_str(&std::fs::read_to_string(file(app, slug)).ok()?).ok()
}

/// The workspace Jarvis is working in now, if any.
pub fn active(app: &AppHandle) -> Option<Workspace> {
    load(app, &settings::load(app).active_workspace)
}

#[tauri::command]
pub fn list_workspaces(app: AppHandle) -> Vec<Workspace> {
    let mut out: Vec<Workspace> = std::fs::read_dir(root(&app))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| load(&app, &e.file_name().to_string_lossy()))
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

#[tauri::command]
pub fn get_active_workspace(app: AppHandle) -> Option<Workspace> {
    active(&app)
}

#[tauri::command]
pub fn save_workspace(app: AppHandle, mut workspace: Workspace) -> Result<Workspace, String> {
    workspace.name = workspace.name.trim().to_string();
    if workspace.name.is_empty() {
        return Err("Give the workspace a name.".into());
    }
    if workspace.slug.is_empty() {
        workspace.slug = slug(&workspace.name);
    }
    if workspace.slug.is_empty() {
        return Err("Use letters or numbers in the name.".into());
    }
    workspace.folder = workspace.folder.trim().to_string();
    if !workspace.folder.is_empty() {
        // A project folder must be a real, narrow one, the same rule code tasks use.
        workspace.folder = crate::tasks::check_project(&app, &workspace.folder)?.display().to_string();
    }
    workspace.verify = workspace.verify.iter().map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect();
    if workspace.created == 0 {
        workspace.created = crate::tasks::now_ms();
    }
    let path = file(&app, &workspace.slug);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, serde_json::to_string_pretty(&workspace).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(workspace)
}

/// Remove a workspace's settings. Its folder on disk, and the memories tagged with it, stay.
#[tauri::command]
pub fn delete_workspace(app: AppHandle, slug: String) -> Result<(), String> {
    if load(&app, &slug).is_none() {
        return Ok(());
    }
    let _ = std::fs::remove_file(file(&app, &slug));
    if settings::load(&app).active_workspace == slug {
        set_active(&app, "")?;
    }
    Ok(())
}

fn set_active(app: &AppHandle, slug: &str) -> Result<(), String> {
    let mut s = settings::load(app);
    s.active_workspace = slug.to_string();
    settings::store(app, &s)
}

/// Switch workspace. Empty `slug` means none.
#[tauri::command]
pub fn set_active_workspace(app: AppHandle, slug: String) -> Result<Option<Workspace>, String> {
    if !slug.is_empty() && load(&app, &slug).is_none() {
        return Err("There's no workspace by that name.".into());
    }
    set_active(&app, &slug)?;
    Ok(active(&app))
}

#[cfg(test)]
mod tests {
    use super::slug;

    #[test]
    fn names_become_folder_names() {
        assert_eq!(slug("Fikra Ventures"), "fikra-ventures");
        assert_eq!(slug("  G-Soft!! "), "g-soft");
        assert_eq!(slug("../etc"), "etc");
        assert_eq!(slug("!!!"), "");
    }
}
