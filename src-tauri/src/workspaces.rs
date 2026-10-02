//! Projects (called workspaces in code): "Fikra", "GSoft", "Personal". Each has its own folder under
//! <research root>/projects/<slug>/ holding workspace.json (what it is, where its code lives, how to
//! check it) and room for notes. Memories are tagged by workspace, and code fixes default to the
//! workspace's project folder. The active workspace is kept in settings.

use crate::settings;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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
    /// Standing instructions: how Jarvis should work in every chat in this project.
    pub instructions: String,
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

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFile {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    /// Seconds since 1970.
    pub modified: u64,
}

const MAX_FILE: u64 = 50 * 1024 * 1024;

/// The project's files folder, for a project that exists and has one.
fn files_dir(app: &AppHandle, slug: &str) -> Result<PathBuf, String> {
    let ws = load(app, slug).ok_or("There's no project by that name.")?;
    if ws.folder.is_empty() {
        return Err("This project has no folder.".into());
    }
    let dir = PathBuf::from(&ws.folder);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Only plain file names: no folders, no dots-first, no path tricks.
fn safe_name(name: &str) -> Option<&str> {
    let ok = !name.is_empty() && name.len() <= 200 && !name.starts_with('.') && !name.contains('/') && !name.contains('\\') && !name.contains("..");
    ok.then_some(name)
}

pub(crate) fn list_files(app: &AppHandle, slug: &str) -> Vec<ProjectFile> {
    let Ok(dir) = files_dir(app, slug) else { return vec![] };
    let mut out: Vec<ProjectFile> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let meta = e.metadata().ok()?;
            if name.starts_with('.') {
                return None;
            }
            let modified = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
            Some(ProjectFile { name, is_dir: meta.is_dir(), size: if meta.is_dir() { 0 } else { meta.len() }, modified })
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

#[tauri::command]
pub fn project_files(app: AppHandle, slug: String) -> Vec<ProjectFile> {
    list_files(&app, &slug)
}

/// Copy files into the project. A name that's taken gets a number before its extension.
#[tauri::command]
pub fn project_add_files(app: AppHandle, slug: String, paths: Vec<String>) -> Result<Vec<ProjectFile>, String> {
    let dir = files_dir(&app, &slug)?;
    for p in paths {
        let from = PathBuf::from(&p);
        let meta = std::fs::metadata(&from).map_err(|_| format!("Can't read {p}."))?;
        if !meta.is_file() {
            return Err(format!("{p} isn't a file."));
        }
        if meta.len() > MAX_FILE {
            return Err(format!("{} is over 50 MB.", from.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(p)));
        }
        let name = from.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let name = safe_name(&name).ok_or("That file name can't be used.")?.to_string();
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
            _ => (name.clone(), String::new()),
        };
        let mut to = dir.join(&name);
        let mut n = 2;
        while to.exists() {
            to = dir.join(format!("{stem} {n}{ext}"));
            n += 1;
        }
        std::fs::copy(&from, &to).map_err(|e| e.to_string())?;
    }
    Ok(list_files(&app, &slug))
}

#[tauri::command]
pub fn project_remove_file(app: AppHandle, slug: String, name: String) -> Result<Vec<ProjectFile>, String> {
    let dir = files_dir(&app, &slug)?;
    let name = safe_name(&name).ok_or("That file name can't be used.")?;
    match std::fs::remove_file(dir.join(name)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    Ok(list_files(&app, &slug))
}

/// The websites in a project: folders that hold a web page. (A site built straight into the project
/// folder isn't listed; it's the project itself.)
pub(crate) fn list_sites(app: &AppHandle, slug: &str) -> Vec<String> {
    let Ok(dir) = files_dir(app, slug) else { return vec![] };
    sites_in(&dir)
}

fn sites_in(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.metadata().map(|m| m.is_dir()).unwrap_or(false))
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter(|e| std::fs::read_dir(e.path()).into_iter().flatten().flatten().any(|f| f.file_name().to_string_lossy().ends_with(".html")))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    out.sort();
    out
}

/// A folder name for a new site that nothing in `dir` uses yet.
fn free_site_name(dir: &Path, name: &str) -> String {
    let base = {
        let s = slug(name);
        if s.is_empty() { "website".to_string() } else { s }
    };
    let mut folder = base.clone();
    let mut n = 2;
    while dir.join(&folder).exists() {
        folder = format!("{base}-{n}");
        n += 1;
    }
    folder
}

#[tauri::command]
pub fn project_sites(app: AppHandle, slug: String) -> Vec<String> {
    list_sites(&app, &slug)
}

/// A new, empty folder for a website inside the project, named after it ("Modern IT Company" becomes
/// modern-it-company; a taken name gets -2, -3…). Earlier sites are never reused. Returns the folder.
#[tauri::command]
pub fn project_new_site(app: AppHandle, slug: String, name: String) -> Result<String, String> {
    let dir = files_dir(&app, &slug)?;
    let path = dir.join(free_site_name(&dir, &name));
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// Copy a document Jarvis wrote or opened into the project (or into one of its site folders, if
/// `subdir` names one) as `brief.md`, so the code worker can build from it. A taken name gets a
/// number. Returns the file's name.
#[tauri::command]
pub fn project_import_document(app: AppHandle, slug: String, task_id: u32, subdir: Option<String>) -> Result<String, String> {
    let mut dir = files_dir(&app, &slug)?;
    if let Some(sub) = subdir.filter(|s| !s.is_empty()) {
        dir = dir.join(safe_name(&sub).ok_or("That folder name can't be used.")?);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    let task = crate::tasks::get_task(&app, task_id).ok_or("There's no such document.")?;
    let from = PathBuf::from(&task.dir).join(crate::tasks::DOCUMENT_FILE);
    let mut name = "brief.md".to_string();
    let mut n = 2;
    while dir.join(&name).exists() {
        name = format!("brief {n}.md");
        n += 1;
    }
    std::fs::copy(&from, dir.join(&name)).map_err(|_| "That document has no content yet.".to_string())?;
    Ok(name)
}

/// A project file's text, for Jarvis to read. Binary files and huge ones are refused.
#[tauri::command]
pub fn project_read_file(app: AppHandle, slug: String, name: String) -> Result<String, String> {
    let dir = files_dir(&app, &slug)?;
    let name = safe_name(&name).ok_or("That file name can't be used.")?;
    let bytes = std::fs::read(dir.join(name)).map_err(|_| format!("There's no file called {name} in this project."))?;
    let text = String::from_utf8(bytes).map_err(|_| format!("{name} isn't a text file, so it can't be read here."))?;
    Ok(text.chars().take(30_000).collect())
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
        return Err("Give the project a name.".into());
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

/// A new project made from just a name: its folder is created under the research folder
/// (projects/<slug>/files), so nobody has to pick one. A taken name gets a number after it.
#[tauri::command]
pub fn create_project(app: AppHandle, name: String) -> Result<Workspace, String> {
    let name = name.trim().to_string();
    let base = slug(&name);
    if base.is_empty() {
        return Err("Give the project a name using letters or numbers.".into());
    }
    let mut slug = base.clone();
    let mut n = 2;
    while root(&app).join(&slug).exists() {
        slug = format!("{base}-{n}");
        n += 1;
    }
    let folder = root(&app).join(&slug).join("files");
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    save_workspace(
        app,
        Workspace { slug, name, folder: folder.display().to_string(), ..Default::default() },
    )
}

/// Remove a workspace's settings. Its folder on disk, the memories tagged with it and its chats
/// stay; the chats move out of the project.
#[tauri::command]
pub fn delete_workspace(app: AppHandle, slug: String) -> Result<(), String> {
    if load(&app, &slug).is_none() {
        return Ok(());
    }
    let _ = std::fs::remove_file(file(&app, &slug));
    crate::chats::unfile(&app, &slug);
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
        return Err("There's no project by that name.".into());
    }
    set_active(&app, &slug)?;
    Ok(active(&app))
}

#[cfg(test)]
mod tests {
    use super::{free_site_name, safe_name, sites_in, slug};

    #[test]
    fn names_become_folder_names() {
        assert_eq!(slug("Fikra Ventures"), "fikra-ventures");
        assert_eq!(slug("  G-Soft!! "), "g-soft");
        assert_eq!(slug("../etc"), "etc");
        assert_eq!(slug("!!!"), "");
    }

    #[test]
    fn a_new_site_never_reuses_an_existing_folder() {
        let tmp = std::env::temp_dir().join(format!("jarvis-sites-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        assert_eq!(free_site_name(&tmp, "Modern IT Company website"), "modern-it-company-website");
        std::fs::create_dir_all(tmp.join("modern-it-company-website")).unwrap();
        assert_eq!(free_site_name(&tmp, "Modern IT Company website"), "modern-it-company-website-2");
        std::fs::create_dir_all(tmp.join("modern-it-company-website-2")).unwrap();
        assert_eq!(free_site_name(&tmp, "Modern IT Company website"), "modern-it-company-website-3");
        assert_eq!(free_site_name(&tmp, "!!!"), "website");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn only_folders_with_a_web_page_count_as_sites() {
        let tmp = std::env::temp_dir().join(format!("jarvis-sitelist-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        for d in ["happy-panda", "it-company", "css", "assets", ".hidden"] {
            std::fs::create_dir_all(tmp.join(d)).unwrap();
        }
        std::fs::write(tmp.join("happy-panda/index.html"), "x").unwrap();
        std::fs::write(tmp.join("it-company/home.html"), "x").unwrap();
        std::fs::write(tmp.join("css/styles.css"), "x").unwrap();
        std::fs::write(tmp.join(".hidden/index.html"), "x").unwrap();
        std::fs::write(tmp.join("index.html"), "the project's own site").unwrap();
        assert_eq!(sites_in(&tmp), vec!["happy-panda", "it-company"]);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn file_names_cannot_escape_the_folder() {
        assert_eq!(safe_name("notes.md"), Some("notes.md"));
        assert_eq!(safe_name("../secrets"), None);
        assert_eq!(safe_name("a/b.txt"), None);
        assert_eq!(safe_name(".env"), None);
        assert_eq!(safe_name(""), None);
    }
}
