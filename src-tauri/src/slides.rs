//! Slide decks: Codex writes a small script with pptxgenjs that builds deck.pptx in the task's
//! folder, and Jarvis can then upload the deck to Google Drive, where it becomes Google Slides.
//!
//! Codex works sandboxed in the task folder with no network, so it can't install anything there.
//! Jarvis installs one pinned copy of pptxgenjs in `~/Jarvis/.slides-kit/` (once, with npm) and
//! links its node_modules into each deck's folder, so a script there can `require("pptxgenjs")`.
//! The script runs on the private Node the video kit installs, or else the Node on this computer.

use crate::google;
use crate::settings;
use crate::tasks::{self, Status, Task};
use crate::videokit;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::process::Command;

/// The library version the prompt and the tests were written against.
const PPTXGENJS: &str = "pptxgenjs@4.0.1";
pub(crate) const DECK_FILE: &str = "deck.pptx";
/// Where an upload's Google link is kept, next to the deck.
const GOOGLE_FILE: &str = "google.json";
const PPTX_MIME: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation";
const SLIDES_MIME: &str = "application/vnd.google-apps.presentation";
const UPLOAD_URL: &str = "https://www.googleapis.com/upload/drive/v3/files";
/// Above this, Google asks for a resumable upload instead of a single multipart request.
const MULTIPART_LIMIT: usize = 5 * 1024 * 1024;
const RECONNECT: &str = "Reconnect Google Drive in Settings → Apps to allow uploading slides.";

/// Only one install of the library at a time, even if two decks are asked for at once.
static INSTALLING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// ---------- the toolbox ----------

fn kit_dir(app: &AppHandle) -> PathBuf {
    settings::home(app).join("Jarvis").join(".slides-kit")
}

fn kit_modules(app: &AppHandle) -> PathBuf {
    kit_dir(app).join("node_modules")
}

/// The pinned version is the one installed.
fn kit_ready(app: &AppHandle) -> bool {
    let wanted = PPTXGENJS.split('@').nth(1).unwrap_or("");
    std::fs::read_to_string(kit_modules(app).join("pptxgenjs").join("package.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .is_some_and(|v| v["version"] == wanted)
}

/// The Node that runs deck scripts: the video kit's own copy first, then one on this computer.
fn find_node(app: &AppHandle) -> Option<PathBuf> {
    let private = videokit::Runtime::for_app(app).node_path();
    if private.is_file() {
        return Some(private);
    }
    let node = if cfg!(windows) { "node.exe" } else { "node" };
    std::env::split_paths(&tasks::child_path(app)).map(|d| d.join(&node)).find(|p| p.is_file())
}

/// How to run npm: the video kit's npm with its own Node, or the npm on this computer.
fn npm_command(app: &AppHandle, node: &Path) -> Option<Command> {
    let bundled = node.parent().and_then(Path::parent).map(|d| d.join("lib").join("node_modules").join("npm").join("bin").join("npm-cli.js"));
    if let Some(cli) = bundled.filter(|p| p.is_file()) {
        let mut cmd = Command::new(node);
        cmd.arg(cli);
        return Some(cmd);
    }
    tasks::find_npm(app).map(Command::new)
}

/// Install pptxgenjs into the slides kit, unless the pinned version is already there.
async fn ensure_kit(app: &AppHandle, node: &Path) -> Result<(), String> {
    let _one = INSTALLING.lock().await;
    if kit_ready(app) {
        return Ok(());
    }
    let dir = kit_dir(app);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    let manifest = dir.join("package.json");
    if !manifest.exists() {
        std::fs::write(&manifest, "{\"name\":\"jarvis-slides-kit\",\"private\":true}").map_err(|e| e.to_string())?;
    }
    let mut cmd = npm_command(app, node).ok_or("Couldn't find npm to install the slide library. Set up video in Jarvis's Set up window, or install Node.js from nodejs.org.")?;
    let path = match node.parent() {
        Some(bin) => format!("{}{}{}", bin.display(), if cfg!(windows) { ";" } else { ":" }, tasks::child_path(app)),
        None => tasks::child_path(app),
    };
    cmd.args(["install", "--no-fund", "--no-audit", "--loglevel=error", "--save-exact", "--prefix"])
        .arg(&dir)
        .arg(PPTXGENJS)
        .current_dir(&dir)
        .env("PATH", path)
        .env("npm_config_update_notifier", "false")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let out = tokio::time::timeout(Duration::from_secs(300), cmd.output())
        .await
        .map_err(|_| "Installing the slide library took longer than five minutes and was stopped.".to_string())?
        .map_err(|e| format!("Couldn't run npm: {e}"))?;
    if !out.status.success() || !kit_ready(app) {
        let text = format!("{}{}", String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout));
        return Err(format!("Couldn't install the slide library: {}", tasks::truncate(&text, 240)));
    }
    Ok(())
}

/// Make the kit's node_modules available in the deck folder: a link where possible, a copy otherwise.
fn link_modules(app: &AppHandle, dir: &Path) -> Result<(), String> {
    let target = kit_modules(app);
    let link = dir.join("node_modules");
    if link.exists() {
        return Ok(());
    }
    #[cfg(unix)]
    if std::os::unix::fs::symlink(&target, &link).is_ok() {
        return Ok(());
    }
    copy_dir(&target, &link).map_err(|e| format!("Couldn't give the deck folder the slide library: {e}"))
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

// ---------- the worker's brief ----------

fn slides_prompt(s: &settings::Settings, title: &str, brief: &str, count: u32, node: &Path, sources: &[String], images: &[String], research: bool) -> String {
    let who = if s.user_name.trim().is_empty() { "The user".to_string() } else { s.user_name.trim().to_string() };
    let sources_rule = if sources.is_empty() {
        String::new()
    } else {
        format!(
            "Source material is in this folder: {}. Read it fully first. Every number, name and claim on the slides must come from it \
             (or from your own research, if allowed below), never from memory.\n",
            sources.join(", ")
        )
    };
    let images_rule = if images.is_empty() {
        "- Images: there are none to use. Don't add pictures from the web or from remote URLs; use shapes, charts and type instead.".to_string()
    } else {
        format!(
            "- Images: you may use these files in this folder: {}. Look at each one first and use it only where it fits. \
             Use no other images, and never remote URLs.",
            images.join(", ")
        )
    };
    let facts = if research {
        "- Research on the web where the deck needs facts, figures or recent events, and list the sources on a final 'Sources' slide."
    } else {
        "- Don't browse the web. Where a real figure is needed and the sources don't have it, write a clear placeholder such as [add figure] instead of inventing one."
    };
    format!(
        "You are the slide designer for Jarvis, a voice assistant. {who} asked for a deck of {count} slides titled \"{title}\":\n\n\
         \"{brief}\"\n\n\
         {sources_rule}\
         How to build it:\n\
         - The pptxgenjs library ({PPTXGENJS}) is already installed in ./node_modules. Don't run npm and don't download anything.\n\
         - Write build-deck.js in the current directory. It starts with `const pptxgen = require(\"pptxgenjs\");`, sets \
           `pptx.layout = \"LAYOUT_16x9\"`, builds every slide and ends with `pptx.writeFile({{ fileName: \"{DECK_FILE}\" }})`.\n\
         - Run it with exactly this command: \"{node}\" build-deck.js\n\
         - If it fails, fix build-deck.js and run it again until {DECK_FILE} is written. Keep build-deck.js: later changes are made by editing it.\n\
         Design rules:\n\
         - Exactly {count} slides, the first a title slide with the deck's title and a one-line subtitle.\n\
         - One clean, consistent design: two or three colours, one font family, the same margins, title position and sizes on every slide. \
           Generous white space; nothing may run off the slide or overlap.\n\
         - At most 6 bullets per slide, each a short phrase, not a sentence. Prefer one clear message per slide, said in its title.\n\
         - Speaker notes on every slide with slide.addNotes(...): two to four sentences the presenter can say.\n\
         - When there is data, show it with pptxgenjs charts (slide.addChart with pptx.ChartType.bar, line, pie or doughnut) or a table, \
           using the real numbers exactly as the sources give them, with units.\n\
         {images_rule}\n\
         {facts}\n\
         - Work only inside the current directory. Do not touch files anywhere else.\n\
         - Your FINAL message is read aloud: one or two plain spoken sentences about the deck. No markdown, no file names, no paths.",
        node = node.display()
    )
}

fn slides_edit_prompt(change: &str, node: &Path) -> String {
    format!(
        "The user wants this change to the deck: \"{change}\"\n\n\
         Edit build-deck.js in the current directory with precise edits, keep everything the change doesn't touch exactly as it is, \
         and run it again with: \"{node}\" build-deck.js so that {DECK_FILE} is rewritten. The pptxgenjs library is in ./node_modules; \
         don't run npm or download anything. Work only inside the current directory. \
         Your FINAL message is read aloud: one or two plain spoken sentences about what changed. No markdown, no paths.",
        node = node.display()
    )
}

// ---------- starting and editing ----------

/// Copy the main text of an earlier task (a report or a document) into the deck's folder.
fn copy_source(app: &AppHandle, id: u32, dir: &Path) -> Result<String, String> {
    let task = tasks::get_task(app, id).ok_or(format!("There is no task #{id} to build the deck from."))?;
    let file = ["document.md", "report.md"].iter().map(|f| Path::new(&task.dir).join(f)).find(|p| p.is_file()).ok_or(format!("Task #{id} has no report or document to build the deck from."))?;
    let name = format!("source-{id}.md");
    std::fs::copy(&file, dir.join(&name)).map_err(|e| format!("Couldn't copy the source material: {e}"))?;
    Ok(name)
}

/// Copy files the user attached (logos, photos) into assets/. Only files Jarvis imported as
/// attachments are allowed, the same rule as for images.
fn copy_attachments(app: &AppHandle, paths: &[String], dir: &Path) -> Vec<String> {
    let Some(root) = settings::research_root(app).join("attachments").canonicalize().ok() else { return vec![] };
    let mut names = vec![];
    for p in paths {
        let Ok(src) = Path::new(p).canonicalize() else { continue };
        if !src.starts_with(&root) || !src.is_file() {
            continue;
        }
        let Some(name) = src.file_name() else { continue };
        let assets = dir.join("assets");
        let _ = std::fs::create_dir_all(&assets);
        if std::fs::copy(&src, assets.join(name)).is_ok() {
            names.push(format!("assets/{}", name.to_string_lossy()));
        }
    }
    names
}

/// Start a deck: install the library if needed, set up the folder and hand Codex the brief.
#[tauri::command]
pub async fn start_slides(
    app: AppHandle,
    title: String,
    brief: String,
    slides: Option<u32>,
    source_task: Option<u32>,
    attachments: Option<Vec<String>>,
    research: Option<bool>,
    chat_id: Option<String>,
) -> Result<Task, String> {
    let s = settings::load(&app);
    let codex = tasks::find_codex(&app, &s).ok_or("Codex CLI was not found. Open Settings to install it.")?;
    let node = find_node(&app).ok_or("Slides need Node.js. Set up video in Jarvis's Set up window (it brings its own Node), or install Node.js from nodejs.org.")?;
    ensure_kit(&app, &node).await?;

    let count = slides.unwrap_or(8).clamp(1, 40);
    let title = if title.trim().is_empty() { tasks::truncate(&brief, 60) } else { title.trim().to_string() };
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let dir = settings::research_root(&app).join("slides").join(format!("{stamp}-{}", tasks::slug(&title)));
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    link_modules(&app, &dir)?;
    let sources = match source_task {
        Some(id) => vec![copy_source(&app, id, &dir)?],
        None => vec![],
    };
    let images = copy_attachments(&app, &attachments.unwrap_or_default(), &dir);
    let research = research.unwrap_or(false);
    let depth = if research { "deep" } else { "quick" };

    let task = tasks::new_task(&app, "slides", &title, &brief, depth, Some(dir.clone()), None, chat_id.as_deref().unwrap_or(""))?;
    let mut args = tasks::base_args(&s, &dir, depth, research);
    args.push(slides_prompt(&s, &title, &brief, count, &node, &sources, &images, research));
    tasks::save_index(&app);
    let _ = app.emit("task-update", &task);
    tauri::async_runtime::spawn(tasks::run_codex(app.clone(), task.id, args, dir, codex));
    Ok(task)
}

/// The first task of a deck: edits share its folder and carry its title.
fn root_of(app: &AppHandle, id: u32) -> Result<Task, String> {
    let mut task = tasks::get_task(app, id).ok_or(format!("There is no deck #{id}."))?;
    while let Some(parent) = task.parent_id.and_then(|p| tasks::get_task(app, p)) {
        task = parent;
    }
    if task.kind != "slides" {
        return Err(format!("#{id} isn't a slide deck."));
    }
    Ok(task)
}

/// Change a deck: Codex edits build-deck.js and builds deck.pptx again.
#[tauri::command]
pub async fn slides_edit(app: AppHandle, id: u32, instructions: String, chat_id: Option<String>) -> Result<Task, String> {
    let s = settings::load(&app);
    let codex = tasks::find_codex(&app, &s).ok_or("Codex CLI was not found. Open Settings to install it.")?;
    let node = find_node(&app).ok_or("Slides need Node.js. Set up video in Jarvis's Set up window, or install Node.js from nodejs.org.")?;
    ensure_kit(&app, &node).await?;
    let root = root_of(&app, id)?;
    let dir = PathBuf::from(&root.dir);
    link_modules(&app, &dir)?;
    let chat = chat_id.filter(|c| !c.is_empty()).unwrap_or_else(|| root.chat_id.clone());
    let task = tasks::new_task(&app, "slides", &format!("Edit: {}", tasks::truncate(&instructions, 50)), &instructions, "quick", Some(dir.clone()), Some(root.id), &chat)?;
    let mut args = tasks::base_args(&s, &dir, "quick", false);
    args.push(slides_edit_prompt(&instructions, &node));
    tasks::save_index(&app);
    let _ = app.emit("task-update", &task);
    tauri::async_runtime::spawn(tasks::run_codex(app.clone(), task.id, args, dir, codex));
    Ok(task)
}

/// Called by tasks::finish for a slides task: a run that didn't leave a deck failed, and a
/// finished run's summary says where the deck is.
pub(crate) fn settle(task: &mut Task) {
    let deck = Path::new(&task.dir).join(DECK_FILE);
    let built = std::fs::metadata(&deck).map(|m| m.len() > 0).unwrap_or(false);
    if task.status != Status::Done {
        return;
    }
    if !built {
        task.status = Status::Failed;
        task.error = format!("Codex finished but didn't save {DECK_FILE}.");
    } else if !task.summary.contains(DECK_FILE) {
        task.summary = format!("{} The deck is saved as {DECK_FILE}.", task.summary.trim()).trim().to_string();
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlidesInfo {
    /// Absolute path of deck.pptx.
    deck: String,
    /// deck.pptx exists.
    ready: bool,
    /// The Google Slides link from the last upload, or empty.
    link: String,
}

/// Where a deck is and whether it has been uploaded, for the side panel.
#[tauri::command]
pub fn slides_info(app: AppHandle, id: u32) -> Result<SlidesInfo, String> {
    let root = root_of(&app, id)?;
    let dir = Path::new(&root.dir);
    let deck = dir.join(DECK_FILE);
    let link = std::fs::read_to_string(dir.join(GOOGLE_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["link"].as_str().map(String::from))
        .unwrap_or_default();
    Ok(SlidesInfo { ready: deck.is_file(), deck: deck.display().to_string(), link })
}

/// Open a finished deck in the Mac's default app for .pptx (usually Keynote).
#[tauri::command]
pub fn slides_open(app: AppHandle, id: u32) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let root = root_of(&app, id)?;
    let deck = Path::new(&root.dir).join(DECK_FILE);
    if !deck.is_file() {
        return Err("This deck isn't finished yet.".into());
    }
    app.opener().open_path(deck.display().to_string(), None::<&str>).map_err(|e| e.to_string())
}

// ---------- uploading to Google Slides ----------

/// `path` as a file inside Jarvis's research folder, or an error. Nothing else is ever uploaded.
pub(crate) fn inside_research_root(app: &AppHandle, path: &Path) -> Result<PathBuf, String> {
    let root = settings::research_root(app).canonicalize().map_err(|_| "Jarvis's research folder doesn't exist yet.".to_string())?;
    let file = path.canonicalize().map_err(|_| "That file doesn't exist.".to_string())?;
    if !file.starts_with(&root) || !file.is_file() {
        return Err("Jarvis only uploads its own files.".into());
    }
    Ok(file)
}

/// A multipart/related body: the file's metadata as JSON, then its bytes.
fn multipart_body(boundary: &str, metadata: &Value, bytes: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(bytes.len() + 512);
    body.extend_from_slice(format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata}\r\n").as_bytes());
    body.extend_from_slice(format!("--{boundary}\r\nContent-Type: {PPTX_MIME}\r\n\r\n").as_bytes());
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

/// A boundary that can't appear in the file: random, and checked against the bytes anyway.
fn boundary_for(bytes: &[u8]) -> String {
    loop {
        let mut raw = [0u8; 12];
        let _ = getrandom::fill(&mut raw);
        let b = format!("jarvis-{}", raw.iter().map(|x| format!("{x:02x}")).collect::<String>());
        if !bytes.windows(b.len()).any(|w| w == b.as_bytes()) {
            return b;
        }
    }
}

/// Google's answer to a failed upload, in plain words. Missing permission means the Drive sign-in
/// predates the upload scope.
async fn upload_failure(res: reqwest::Response) -> String {
    let status = res.status();
    let e = google::failure(res).await;
    let l = e.to_lowercase();
    if l.contains("insufficient") || (status == reqwest::StatusCode::FORBIDDEN && l.contains("scope")) {
        format!("{e} {RECONNECT}")
    } else {
        google::with_scope_hint(e, "Drive")
    }
}

fn upload_client() -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(600)).build().unwrap_or_default()
}

async fn upload(token: &str, metadata: &Value, bytes: Vec<u8>) -> Result<Value, String> {
    let client = upload_client();
    let res = if bytes.len() <= MULTIPART_LIMIT {
        let boundary = boundary_for(&bytes);
        client
            .post(google::url_with(UPLOAD_URL, &[("uploadType", "multipart"), ("fields", "id,webViewLink")]))
            .bearer_auth(token)
            .header(reqwest::header::CONTENT_TYPE, format!("multipart/related; boundary={boundary}"))
            .body(multipart_body(&boundary, metadata, &bytes))
            .send()
            .await
    } else {
        // Large decks: ask for an upload address, then send the bytes there.
        let start = client
            .post(google::url_with(UPLOAD_URL, &[("uploadType", "resumable"), ("fields", "id,webViewLink")]))
            .bearer_auth(token)
            .header("X-Upload-Content-Type", PPTX_MIME)
            .header("X-Upload-Content-Length", bytes.len().to_string())
            .json(metadata)
            .send()
            .await
            .map_err(|e| format!("Couldn't reach Google: {e}"))?;
        if !start.status().is_success() {
            return Err(upload_failure(start).await);
        }
        let location = start
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(String::from)
            .ok_or("Google didn't give an upload address.")?;
        client.put(location).header(reqwest::header::CONTENT_TYPE, PPTX_MIME).body(bytes).send().await
    }
    .map_err(|e| format!("Couldn't reach Google: {e}"))?;
    if !res.status().is_success() {
        return Err(upload_failure(res).await);
    }
    res.json().await.map_err(|e| format!("Google's answer couldn't be read: {e}"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Uploaded {
    id: String,
    link: String,
}

/// Upload a finished deck to Google Drive as Google Slides. Returns the new file and its link.
/// The UI asks the user first, since this sends the deck to Google.
#[tauri::command]
pub async fn slides_upload(app: AppHandle, task_id: u32) -> Result<Uploaded, String> {
    let root = root_of(&app, task_id)?;
    let deck = Path::new(&root.dir).join(DECK_FILE);
    if !deck.is_file() {
        return Err("This deck isn't finished yet: there's no deck.pptx to upload.".into());
    }
    let deck = inside_research_root(&app, &deck)?;
    let bytes = std::fs::read(&deck).map_err(|e| format!("Couldn't read the deck: {e}"))?;
    let token = google::access_token(&app, "drive").await?;
    let metadata = json!({ "name": root.title, "mimeType": SLIDES_MIME });
    let v = upload(&token, &metadata, bytes).await?;
    let id = v["id"].as_str().unwrap_or("").to_string();
    if id.is_empty() {
        return Err("Google didn't say where the deck went.".into());
    }
    let link = v["webViewLink"].as_str().map(String::from).unwrap_or_else(|| format!("https://docs.google.com/presentation/d/{id}/edit"));
    let _ = std::fs::write(Path::new(&root.dir).join(GOOGLE_FILE), json!({ "id": id, "link": link, "uploadedAt": tasks::now_ms() }).to_string());
    Ok(Uploaded { id, link })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(dir: &Path, status: Status, summary: &str) -> Task {
        serde_json::from_value(json!({
            "id": 1, "title": "Deck", "request": "", "depth": "quick", "status": status, "steps": [], "summary": summary,
            "error": "", "dir": dir.display().to_string(), "threadId": "", "parentId": null, "startedAt": 0, "finishedAt": null,
            "searches": 0, "sources": 0, "kind": "slides"
        }))
        .unwrap()
    }

    #[test]
    fn a_finished_deck_names_its_file_and_a_missing_one_fails() {
        let dir = std::env::temp_dir().join(format!("jarvis-slides-{}", tasks::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut t = task(&dir, Status::Done, "Ten slides on Acme.");
        settle(&mut t);
        assert!(t.status == Status::Failed && t.error.contains("deck.pptx"));

        std::fs::write(dir.join(DECK_FILE), b"PK").unwrap();
        let mut t = task(&dir, Status::Done, "Ten slides on Acme.");
        settle(&mut t);
        assert!(t.status == Status::Done);
        assert_eq!(t.summary, "Ten slides on Acme. The deck is saved as deck.pptx.");

        let mut cancelled = task(&dir, Status::Cancelled, "");
        settle(&mut cancelled);
        assert!(cancelled.status == Status::Cancelled && cancelled.summary.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_upload_body_is_metadata_then_the_deck() {
        let bytes = b"PK\x03\x04deck";
        let b = boundary_for(bytes);
        let body = multipart_body(&b, &json!({ "name": "Q3", "mimeType": SLIDES_MIME }), bytes);
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with(&format!("--{b}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{{")));
        assert!(text.contains("\"mimeType\":\"application/vnd.google-apps.presentation\""));
        assert!(text.contains(&format!("--{b}\r\nContent-Type: {PPTX_MIME}\r\n\r\nPK")));
        assert!(text.ends_with(&format!("deck\r\n--{b}--\r\n")));
        assert_ne!(boundary_for(bytes), b, "each upload gets its own boundary");
    }

    #[test]
    fn the_prompt_pins_the_library_the_node_and_the_folder() {
        let s = settings::Settings::default();
        let p = slides_prompt(&s, "Investor update", "Q3 numbers", 10, Path::new("/x/node/bin/node"), &["source-4.md".into()], &[], false);
        for part in ["Exactly 10 slides", "\"/x/node/bin/node\" build-deck.js", "LAYOUT_16x9", "At most 6 bullets", "addNotes", "addChart", "source-4.md", "Don't run npm", "Work only inside the current directory", "remote URLs"] {
            assert!(p.contains(part), "missing {part}");
        }
        assert!(p.contains("Don't browse the web"));
        let with_images = slides_prompt(&s, "T", "B", 8, Path::new("node"), &[], &["assets/logo.png".into()], true);
        assert!(with_images.contains("assets/logo.png") && with_images.contains("Research on the web"));
    }
}
