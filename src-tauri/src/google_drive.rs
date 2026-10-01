//! Google Drive: find and read the user's files, and save Jarvis's own work there.
//!
//! Reading a file returns its text only (Docs, Sheets as CSV, Slides, plain text); Jarvis never
//! downloads binaries. Saving turns one of Jarvis's own Markdown or text files into a Google Doc
//! (Google doesn't allow the Drive write scope together with YouTube's, so this goes through the
//! Docs permission). Only files inside Jarvis's research folder can be saved, so a
//! prompt-injected path can't send anything else off the computer.

use crate::google::{api, api_text, url_with, with_scope_hint};
use crate::google_docs;
use crate::settings;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

const FILES: &str = "https://www.googleapis.com/drive/v3/files";
const FIELDS: &str = "id,name,mimeType,modifiedTime,webViewLink,owners(displayName)";
/// Text returned from one file, in bytes.
const MAX_TEXT: usize = 60_000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveFile {
    id: String,
    name: String,
    /// "doc", "sheet", "slides", "folder", "pdf" or "file".
    kind: String,
    modified: String,
    owner: String,
    link: String,
}

fn kind_of(mime: &str) -> &'static str {
    match mime {
        "application/vnd.google-apps.document" => "doc",
        "application/vnd.google-apps.spreadsheet" => "sheet",
        "application/vnd.google-apps.presentation" => "slides",
        "application/vnd.google-apps.folder" => "folder",
        "application/pdf" => "pdf",
        _ => "file",
    }
}

fn file_from(v: &Value) -> DriveFile {
    DriveFile {
        id: v["id"].as_str().unwrap_or("").into(),
        name: v["name"].as_str().unwrap_or("(untitled)").into(),
        kind: kind_of(v["mimeType"].as_str().unwrap_or("")).into(),
        modified: v["modifiedTime"].as_str().unwrap_or("").into(),
        owner: v["owners"][0]["displayName"].as_str().unwrap_or("").into(),
        link: v["webViewLink"].as_str().unwrap_or("").into(),
    }
}

/// A value for use inside a Drive query string literal.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// The Drive query for words typed or spoken by the user: matches names and contents.
fn search_query(text: &str) -> String {
    let t = text.trim();
    if t.is_empty() {
        return "trashed = false".into();
    }
    format!("(name contains {q} or fullText contains {q}) and trashed = false", q = quote(t))
}

/// Search Drive by words in the name or contents; newest first.
#[tauri::command]
pub async fn drive_search(app: AppHandle, query: String, max: Option<u32>) -> Result<Vec<DriveFile>, String> {
    let max = max.unwrap_or(10).clamp(1, 50).to_string();
    let q = search_query(&query);
    let url = url_with(FILES, &[("q", &q), ("pageSize", &max), ("orderBy", "modifiedTime desc"), ("fields", &format!("files({FIELDS})"))]);
    let v = api(&app, reqwest::Method::GET, &url, None).await.map_err(|e| with_scope_hint(e, "Drive"))?;
    Ok(v["files"].as_array().map(|a| a.iter().map(file_from).collect()).unwrap_or_default())
}

#[tauri::command]
pub async fn drive_get(app: AppHandle, id: String) -> Result<DriveFile, String> {
    let url = url_with(&format!("{FILES}/{}", enc(&id)), &[("fields", FIELDS)]);
    let v = api(&app, reqwest::Method::GET, &url, None).await.map_err(|e| with_scope_hint(e, "Drive"))?;
    Ok(file_from(&v))
}

/// File ids are opaque, but they end up in a URL path, so keep anything odd out of it.
fn enc(id: &str) -> String {
    id.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')).collect()
}

#[derive(Serialize)]
pub struct FileText {
    name: String,
    text: String,
    truncated: bool,
}

/// The text of a file: Docs and Slides as plain text, Sheets as CSV (first tab), text files as is.
#[tauri::command]
pub async fn drive_read(app: AppHandle, id: String) -> Result<FileText, String> {
    let id = enc(&id);
    let meta = api(&app, reqwest::Method::GET, &url_with(&format!("{FILES}/{id}"), &[("fields", "name,mimeType")]), None)
        .await
        .map_err(|e| with_scope_hint(e, "Drive"))?;
    let mime = meta["mimeType"].as_str().unwrap_or("");
    let name = meta["name"].as_str().unwrap_or("").to_string();
    let url = match mime {
        "application/vnd.google-apps.document" | "application/vnd.google-apps.presentation" => url_with(&format!("{FILES}/{id}/export"), &[("mimeType", "text/plain")]),
        "application/vnd.google-apps.spreadsheet" => url_with(&format!("{FILES}/{id}/export"), &[("mimeType", "text/csv")]),
        m if m.starts_with("text/") || matches!(m, "application/json" | "application/xml") => url_with(&format!("{FILES}/{id}"), &[("alt", "media")]),
        _ => return Err(format!("\"{name}\" isn't a file Jarvis can read as text ({mime}). Open it from its link instead.")),
    };
    let (text, truncated) = api_text(&app, &url, MAX_TEXT).await.map_err(|e| with_scope_hint(e, "Drive"))?;
    Ok(FileText { name, text, truncated })
}

/// `path` as a file inside Jarvis's research folder, or an error.
fn inside_research_root(app: &AppHandle, path: &str) -> Result<PathBuf, String> {
    let root = settings::research_root(app).canonicalize().map_err(|_| "Jarvis's research folder doesn't exist yet.".to_string())?;
    let file = Path::new(path).canonicalize().map_err(|_| "That file doesn't exist.".to_string())?;
    if !file.starts_with(&root) || !file.is_file() || file.file_name().is_some_and(|n| n == "audit.jsonl") {
        return Err("Jarvis only saves its own documents and reports to Drive.".into());
    }
    Ok(file)
}

/// Save a Markdown or text file from Jarvis's research folder as a new Google Doc.
#[tauri::command]
pub async fn drive_upload(app: AppHandle, path: String, name: Option<String>) -> Result<DriveFile, String> {
    let file = inside_research_root(&app, &path)?;
    if !matches!(file.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref(), Some("md" | "markdown" | "txt")) {
        return Err("Only Jarvis's documents and reports (Markdown or text) can be saved to Google.".into());
    }
    let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let title = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| file.file_stem().and_then(|n| n.to_str()).unwrap_or("Jarvis document").to_string());
    let d = google_docs::create(&app, title, text).await?;
    Ok(DriveFile { id: d.id, name: d.title, kind: "doc".into(), modified: String::new(), owner: String::new(), link: d.link })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_escape_quotes() {
        assert_eq!(search_query("Q3 deck"), "(name contains 'Q3 deck' or fullText contains 'Q3 deck') and trashed = false");
        assert!(search_query("it's a \\ test").contains("'it\\'s a \\\\ test'"));
        assert_eq!(search_query("  "), "trashed = false");
    }

    #[test]
    fn ids_cannot_escape_the_path() {
        assert_eq!(enc("1aB-_c/../x?y"), "1aB-_cxy");
    }

    #[test]
    fn kinds_and_mimes() {
        assert_eq!(kind_of("application/vnd.google-apps.spreadsheet"), "sheet");
        assert_eq!(kind_of("image/png"), "file");
    }
}
