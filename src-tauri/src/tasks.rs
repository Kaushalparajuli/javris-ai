//! Research tasks run by the Codex CLI (`codex exec --json`).
//!
//! Each task gets its own folder under ~/Jarvis/research. Codex writes report.md there,
//! and its final message (the spoken summary) goes to summary.txt. Progress events from
//! Codex's JSON stream are turned into readable steps and sent to the UI as `task-update`.

use crate::settings::{self, Settings};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::oneshot;

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub done: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: u32,
    pub title: String,
    pub request: String,
    pub depth: String,
    pub status: Status,
    pub steps: Vec<Step>,
    pub summary: String,
    pub error: String,
    pub dir: String,
    pub thread_id: String,
    pub parent_id: Option<u32>,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub searches: u32,
    pub sources: u32,
    /// "research" or "image".
    #[serde(default = "research_kind")]
    pub kind: String,
    /// Absolute paths of images this task produced.
    #[serde(default)]
    pub images: Vec<String>,
    /// Reference files (logos, photos) given to the worker.
    #[serde(default)]
    pub refs: Vec<String>,
}

fn research_kind() -> String {
    "research".into()
}

const IMAGE_EXTS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

/// Images in `dir` written at or after `since_ms`, oldest first.
fn images_since(dir: &Path, since_ms: u64) -> Vec<String> {
    let mut found: Vec<(u64, String)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let ext = path.extension()?.to_string_lossy().to_lowercase();
            if !IMAGE_EXTS.contains(&ext.as_str()) {
                return None;
            }
            let modified = e.metadata().ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64;
            (modified + 2000 >= since_ms).then(|| (modified, path.display().to_string()))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, p)| p).collect()
}

#[derive(Default)]
pub struct TaskStore {
    tasks: Mutex<HashMap<u32, Task>>,
    cancels: Mutex<HashMap<u32, oneshot::Sender<()>>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexStatus {
    found: bool,
    path: String,
    version: String,
    logged_in: bool,
    message: String,
}

const MAX_STEPS: usize = 40;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn truncate(s: &str, n: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= n {
        s
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= 48 {
            break;
        }
    }
    out.trim_matches('-').to_string()
}

fn index_file(app: &AppHandle) -> PathBuf {
    settings::research_root(app).join("tasks.json")
}

fn save_index(app: &AppHandle) {
    let store = app.state::<TaskStore>();
    let mut list: Vec<Task> = store.tasks.lock().unwrap().values().cloned().collect();
    list.sort_by_key(|t| t.id);
    let path = index_file(app);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(&list) {
        let _ = std::fs::write(path, json);
    }
}

/// Load earlier tasks at startup. Anything still marked running was cut off when the app quit.
pub fn load_index(app: &AppHandle) {
    let Ok(text) = std::fs::read_to_string(index_file(app)) else { return };
    let Ok(list) = serde_json::from_str::<Vec<Task>>(&text) else { return };
    let store = app.state::<TaskStore>();
    let mut tasks = store.tasks.lock().unwrap();
    for mut t in list {
        if t.status == Status::Running {
            t.status = Status::Failed;
            t.error = "Jarvis was closed while this task was running.".into();
        }
        tasks.insert(t.id, t);
    }
}

fn emit_update(app: &AppHandle, id: u32) {
    let task = app.state::<TaskStore>().tasks.lock().unwrap().get(&id).cloned();
    if let Some(t) = task {
        let _ = app.emit("task-update", t);
    }
}

fn find_codex(app: &AppHandle, s: &Settings) -> Option<PathBuf> {
    if !s.codex_path.trim().is_empty() {
        let p = PathBuf::from(s.codex_path.trim());
        return p.exists().then_some(p);
    }
    let home = settings::home(app);
    let mut candidates = vec![
        // Private copy installed by Jarvis setup; used first because a global npm install can break.
        home.join("Jarvis/.codex-cli/node_modules/.bin/codex"),
        PathBuf::from("/opt/homebrew/bin/codex"),
        PathBuf::from("/usr/local/bin/codex"),
        home.join(".local/bin/codex"),
        home.join(".npm-global/bin/codex"),
        home.join(".volta/bin/codex"),
        home.join(".bun/bin/codex"),
    ];
    if let Ok(path) = std::env::var("PATH") {
        candidates.extend(path.split(':').map(|d| Path::new(d).join("codex")));
    }
    candidates.into_iter().find(|p| p.exists())
}

/// Apps launched from Finder get a minimal PATH, but the npm build of codex needs `node`.
fn child_path(app: &AppHandle) -> String {
    let home = settings::home(app);
    format!(
        "/opt/homebrew/bin:/usr/local/bin:{h}/.local/bin:{h}/.volta/bin:{h}/.bun/bin:/usr/bin:/bin:/usr/sbin:/sbin:{rest}",
        h = home.display(),
        rest = std::env::var("PATH").unwrap_or_default()
    )
}

fn research_prompt(s: &Settings, request: &str, depth: &str) -> String {
    let who = if s.user_name.trim().is_empty() { "The user".to_string() } else { s.user_name.trim().to_string() };
    let depth_rule = if depth == "quick" {
        "QUICK: a few targeted searches, prefer authoritative sources, keep the report short (under 400 words)."
    } else {
        "DEEP: search widely, open and read primary sources, cross-check important claims, note disagreements between sources."
    };
    format!(
        "You are the research worker for Jarvis, a voice assistant. {who} asked for this research:\n\n\
         \"{request}\"\n\n\
         Depth: {depth_rule}\n\n\
         Rules:\n\
         - Work only inside the current directory. Do not touch files anywhere else.\n\
         - Use web search for anything time-sensitive. Never invent facts, numbers or sources.\n\
         - Write the full result to report.md in the current directory: a title, a 2-3 sentence answer first, \
           then findings under headings, a comparison table when it helps, and a final '## Sources' list of \
           full URLs. Cite sources inline as [1], [2].\n\
         - Your FINAL message is read aloud by a voice assistant. Make it 2-4 plain spoken sentences with the key \
           answer: no markdown, no lists, no URLs, no citations."
    )
}

fn aspect_rule(aspect: &str) -> &'static str {
    match aspect {
        "landscape" => "Aspect ratio: landscape, about 3:2.",
        "portrait" => "Aspect ratio: portrait, about 2:3.",
        "wide" => "Aspect ratio: wide, 16:9 (banners, hero images, slides).",
        "tall" => "Aspect ratio: tall, 9:16 (phone wallpapers, stories).",
        _ => "Aspect ratio: square, 1:1.",
    }
}

fn refs_rule(refs: &[String], notes: &str) -> String {
    if refs.is_empty() {
        return String::new();
    }
    let names: Vec<String> = refs.iter().filter_map(|r| Path::new(r).file_name().map(|f| format!("refs/{}", f.to_string_lossy()))).collect();
    let how = if notes.trim().is_empty() { "Use them as visual references." } else { notes.trim() };
    format!(
        "Reference files provided by the user: {}.\n\
         - Look at every reference with your image viewing tool before generating.\n\
         - How to use them: {how}\n\
         - If a logo is provided, reproduce it faithfully (same shapes, colors and lettering) or place the provided file itself; never invent a different logo.\n",
        names.join(", ")
    )
}

fn image_prompt(request: &str, count: u32, aspect: &str, refs: &[String], notes: &str) -> String {
    let what = if count == 1 { "one image".to_string() } else { format!("{count} different images") };
    let aspect = aspect_rule(aspect);
    let refs = refs_rule(refs, notes);
    format!(
        "You are the image worker for Jarvis, a voice assistant. Create {what} for this brief:\n\n\
         \"{request}\"\n\n\
         {aspect}\n\
         {refs}\n\
         Rules:\n\
         - Any text in the image must match the brief exactly, spelled correctly.\n\
         - Use your built-in image generation. Do not draw the image with code, SVG or scripts.\n\
         - Save each final image in the current directory as a PNG named image-1.png, image-2.png and so on, \
           using the next unused number. Never overwrite an existing image.\n\
         - Work only inside the current directory.\n\
         - Your FINAL message is read aloud: one or two plain sentences describing what you made. \
           No markdown, no file names or paths."
    )
}

fn image_followup_prompt(feedback: &str) -> String {
    format!(
        "The user wants changes to the image: \"{feedback}\"\n\n\
         Make a new version with your built-in image generation and save it as the next unused image-N.png \
         in the current directory. Keep the earlier images. \
         Your FINAL message is read aloud: one or two plain sentences about what changed. No markdown, no paths."
    )
}

fn followup_prompt(question: &str) -> String {
    format!(
        "Follow-up question from the user about this research: \"{question}\"\n\n\
         Answer it, researching more if needed. Append a section '## Follow-up: {question}' to report.md \
         (keep the existing content and add any new sources to the Sources list). \
         Your FINAL message is read aloud: 2-4 plain spoken sentences, no markdown, no URLs."
    )
}

fn count_sources(report: &str) -> u32 {
    let mut seen = HashSet::new();
    for word in report.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == '<' || c == '>') {
        if let Some(pos) = word.find("http") {
            let url = word[pos..].trim_end_matches(|c: char| ".,;:]\"'".contains(c));
            if url.starts_with("http://") || url.starts_with("https://") {
                seen.insert(url.to_string());
            }
        }
    }
    seen.len() as u32
}

/// Turn one line of `codex exec --json` output into task state.
fn apply_event(task: &mut Task, v: &Value, last_message: &mut String) {
    let kind = v["type"].as_str().unwrap_or("");
    match kind {
        "thread.started" => {
            if let Some(id) = v["thread_id"].as_str() {
                task.thread_id = id.to_string();
            }
        }
        "item.started" | "item.updated" | "item.completed" => {
            let item = &v["item"];
            let done = kind == "item.completed";
            let item_id = item["id"].as_str().unwrap_or("").to_string();
            let s = |k: &str| item[k].as_str().unwrap_or("").to_string();
            let (label, detail) = match item["type"].as_str().unwrap_or("") {
                "web_search" => {
                    let q = item["query"].as_str().or_else(|| item["action"]["query"].as_str()).unwrap_or("");
                    if kind == "item.started" || (done && !task.steps.iter().any(|x| x.id == item_id)) {
                        task.searches += 1;
                    }
                    ("Searching the web".to_string(), truncate(q, 70))
                }
                "command_execution" => ("Running a command".into(), truncate(&s("command"), 70)),
                "file_change" => {
                    let files: Vec<String> = item["changes"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|c| c["path"].as_str())
                                .map(|p| Path::new(p).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default())
                                .collect()
                        })
                        .unwrap_or_default();
                    ("Writing the report".into(), files.join(", "))
                }
                "mcp_tool_call" => ("Using a tool".into(), format!("{} {}", s("server"), s("tool")).trim().to_string()),
                "reasoning" => ("Thinking".into(), truncate(&s("text"), 90)),
                "todo_list" => {
                    let items = item["items"].as_array().map(|a| a.len()).unwrap_or(0);
                    let finished = item["items"]
                        .as_array()
                        .map(|a| a.iter().filter(|i| i["completed"].as_bool() == Some(true)).count())
                        .unwrap_or(0);
                    ("Planning".into(), format!("{finished} of {items} steps"))
                }
                "agent_message" => {
                    if done {
                        *last_message = s("text");
                    }
                    ("Drafting the answer".into(), String::new())
                }
                "error" => {
                    task.error = s("message");
                    return;
                }
                _ => return,
            };
            if let Some(step) = task.steps.iter_mut().find(|x| !item_id.is_empty() && x.id == item_id) {
                step.label = label;
                if !detail.is_empty() {
                    step.detail = detail;
                }
                step.done = step.done || done;
            } else {
                // A new step means earlier ones have moved on.
                for st in task.steps.iter_mut() {
                    st.done = true;
                }
                task.steps.push(Step { id: item_id, label, detail, done });
                if task.steps.len() > MAX_STEPS {
                    task.steps.remove(0);
                }
            }
        }
        "turn.failed" => {
            task.error = v["error"]["message"].as_str().unwrap_or("Codex reported a failure.").to_string();
        }
        "error" => {
            task.error = v["message"].as_str().unwrap_or("Codex reported an error.").to_string();
        }
        _ => {}
    }
}

fn explain_failure(stderr: &str, fallback: &str) -> String {
    let lower = stderr.to_lowercase();
    if lower.contains("login") || lower.contains("not logged in") || lower.contains("unauthorized") || lower.contains("401") {
        return "Codex is not logged in. Run `codex login` in Terminal, then try again.".into();
    }
    if lower.contains("rate limit") || lower.contains("usage limit") {
        return "Codex hit your plan's usage limit. Try again later.".into();
    }
    if !fallback.is_empty() {
        return fallback.to_string();
    }
    let tail: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = tail[tail.len().saturating_sub(3)..].join(" ");
    if tail.is_empty() { "Codex stopped without an answer.".into() } else { truncate(&tail, 300) }
}

/// Spawn codex and follow its JSON stream until it exits. Runs in the background.
async fn run_codex(app: AppHandle, id: u32, args: Vec<String>, dir: PathBuf, codex: PathBuf) {
    let summary_file = dir.join("summary.txt");
    let _ = std::fs::remove_file(&summary_file);

    let mut cmd = Command::new(&codex);
    cmd.args(&args)
        .current_dir(&dir)
        .env("PATH", child_path(&app))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            finish(&app, id, Status::Failed, String::new(), format!("Could not start Codex at {}: {e}", codex.display()));
            return;
        }
    };

    let (cancel_tx, mut cancel_rx) = oneshot::channel();
    app.state::<TaskStore>().cancels.lock().unwrap().insert(id, cancel_tx);

    let stderr_buf = Arc::new(Mutex::new(String::new()));
    if let Some(mut err) = child.stderr.take() {
        let buf = stderr_buf.clone();
        tauri::async_runtime::spawn(async move {
            let mut s = String::new();
            let _ = err.read_to_string(&mut s).await;
            *buf.lock().unwrap() = s;
        });
    }

    let mut last_message = String::new();
    let mut cancelled = false;
    if let Some(out) = child.stdout.take() {
        let mut lines = BufReader::new(out).lines();
        loop {
            tokio::select! {
                line = lines.next_line() => match line {
                    Ok(Some(line)) => {
                        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                        {
                            let store = app.state::<TaskStore>();
                            let mut tasks = store.tasks.lock().unwrap();
                            if let Some(t) = tasks.get_mut(&id) {
                                apply_event(t, &v, &mut last_message);
                            }
                        }
                        emit_update(&app, id);
                    }
                    _ => break,
                },
                _ = &mut cancel_rx => {
                    let _ = child.kill().await;
                    cancelled = true;
                    break;
                }
            }
        }
    }

    let exit = child.wait().await;
    app.state::<TaskStore>().cancels.lock().unwrap().remove(&id);
    // Give the stderr reader a moment to finish.
    tokio::time::sleep(Duration::from_millis(100)).await;

    if cancelled {
        finish(&app, id, Status::Cancelled, String::new(), "Cancelled.".into());
        return;
    }

    let summary = std::fs::read_to_string(&summary_file)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| last_message.trim().to_string());
    let ok = exit.map(|s| s.success()).unwrap_or(false);

    if ok && !summary.is_empty() {
        finish(&app, id, Status::Done, summary, String::new());
    } else {
        let current_error = app
            .state::<TaskStore>()
            .tasks
            .lock()
            .unwrap()
            .get(&id)
            .map(|t| t.error.clone())
            .unwrap_or_default();
        let stderr = stderr_buf.lock().unwrap().clone();
        finish(&app, id, Status::Failed, summary, explain_failure(&stderr, &current_error));
    }
}

fn finish(app: &AppHandle, id: u32, status: Status, summary: String, error: String) {
    let task = {
        let store = app.state::<TaskStore>();
        let mut tasks = store.tasks.lock().unwrap();
        let Some(t) = tasks.get_mut(&id) else { return };
        t.status = status;
        t.summary = summary;
        t.error = error;
        t.finished_at = Some(now_ms());
        for st in t.steps.iter_mut() {
            st.done = true;
        }
        if let Ok(report) = std::fs::read_to_string(Path::new(&t.dir).join("report.md")) {
            t.sources = count_sources(&report);
        }
        if t.kind == "image" {
            t.images = images_since(Path::new(&t.dir), t.started_at);
            if t.status == Status::Done && t.images.is_empty() {
                t.status = Status::Failed;
                t.error = "Codex finished but didn't save an image.".into();
            }
        }
        t.clone()
    };
    save_index(app);
    let _ = app.emit("task-update", &task);
    let _ = app.emit("task-finished", &task);
}

fn reasoning_for(s: &Settings, depth: &str) -> Option<String> {
    match s.codex_reasoning.trim() {
        "" | "auto" => Some(if depth == "quick" { "low".into() } else { "high".into() }),
        "default" => None,
        level => Some(level.to_string()),
    }
}

fn base_args(s: &Settings, dir: &Path, depth: &str, web: bool) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "exec".into(),
        "--json".into(),
        "--skip-git-repo-check".into(),
        "--sandbox".into(),
        "workspace-write".into(),
        "-C".into(),
        dir.display().to_string(),
        "-o".into(),
        dir.join("summary.txt").display().to_string(),
    ];
    if web && s.web_search {
        args.push("-c".into());
        args.push("web_search=\"live\"".into());
    }
    if !s.codex_model.trim().is_empty() {
        args.push("-m".into());
        args.push(s.codex_model.trim().into());
    }
    if let Some(level) = reasoning_for(s, depth) {
        args.push("-c".into());
        args.push(format!("model_reasoning_effort=\"{level}\""));
    }
    args
}

fn new_task(app: &AppHandle, kind: &str, title: &str, request: &str, depth: &str, dir: Option<PathBuf>, parent: Option<u32>) -> Result<Task, String> {
    let store = app.state::<TaskStore>();
    let mut tasks = store.tasks.lock().unwrap();
    let id = tasks.keys().max().copied().unwrap_or(0) + 1;
    let dir = match dir {
        Some(d) => d,
        None => settings::research_root(app)
            .join(if kind == "image" { "images" } else { "research" })
            .join(format!("{id:04}-{}", slug(title))),
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let task = Task {
        id,
        title: title.trim().to_string(),
        request: request.trim().to_string(),
        depth: if depth == "quick" { "quick".into() } else { "deep".into() },
        status: Status::Running,
        summary: String::new(),
        error: String::new(),
        dir: dir.display().to_string(),
        thread_id: String::new(),
        parent_id: parent,
        started_at: now_ms(),
        finished_at: None,
        searches: 0,
        sources: 0,
        kind: kind.to_string(),
        images: vec![],
        refs: vec![],
        steps: if kind == "image" {
            vec![Step { id: "generate".into(), label: "Generating the image".into(), detail: "usually 30–90 seconds".into(), done: false }]
        } else {
            vec![]
        },
    };
    tasks.insert(id, task.clone());
    Ok(task)
}

#[tauri::command]
pub fn list_tasks(store: State<'_, TaskStore>) -> Vec<Task> {
    let mut list: Vec<Task> = store.tasks.lock().unwrap().values().cloned().collect();
    list.sort_by(|a, b| b.id.cmp(&a.id));
    list
}

#[tauri::command]
pub fn start_task(app: AppHandle, title: String, request: String, depth: String) -> Result<Task, String> {
    let s = settings::load(&app);
    let codex = find_codex(&app, &s).ok_or("Codex CLI was not found. Install it with `npm install -g @openai/codex` or set its path in Settings.")?;
    let title = if title.trim().is_empty() { truncate(&request, 60) } else { title };
    let task = new_task(&app, "research", &title, &request, &depth, None, None)?;
    let dir = PathBuf::from(&task.dir);
    let mut args = base_args(&s, &dir, &task.depth, true);
    args.push(research_prompt(&s, &request, &task.depth));
    save_index(&app);
    let _ = app.emit("task-update", &task);
    tauri::async_runtime::spawn(run_codex(app.clone(), task.id, args, dir, codex));
    Ok(task)
}

#[tauri::command]
pub fn followup_task(app: AppHandle, id: u32, question: String) -> Result<Task, String> {
    let s = settings::load(&app);
    let codex = find_codex(&app, &s).ok_or("Codex CLI was not found.")?;
    let parent = app.state::<TaskStore>().tasks.lock().unwrap().get(&id).cloned().ok_or(format!("There is no task #{id}."))?;
    if parent.thread_id.is_empty() {
        return Err(format!("Task #{id} has no Codex session to continue. Start a new research task instead."));
    }
    let dir = PathBuf::from(&parent.dir);
    let image = parent.kind == "image";
    let title = if image { format!("Edit: {}", truncate(&question, 50)) } else { format!("Follow-up: {}", truncate(&question, 50)) };
    let task = new_task(&app, &parent.kind, &title, &question, &parent.depth, Some(dir.clone()), Some(id))?;
    let mut args = base_args(&s, &dir, &parent.depth, !image);
    args.push("resume".into());
    args.push(parent.thread_id.clone());
    args.push(if image { image_followup_prompt(&question) } else { followup_prompt(&question) });
    save_index(&app);
    let _ = app.emit("task-update", &task);
    tauri::async_runtime::spawn(run_codex(app.clone(), task.id, args, dir, codex));
    Ok(task)
}

#[tauri::command]
pub fn start_image(
    app: AppHandle,
    title: String,
    prompt: String,
    count: Option<u32>,
    aspect: Option<String>,
    refs: Option<Vec<String>>,
    ref_notes: Option<String>,
) -> Result<Task, String> {
    let s = settings::load(&app);
    let codex = find_codex(&app, &s).ok_or("Codex CLI was not found. Set its path in Settings.")?;
    let count = count.unwrap_or(1).clamp(1, 4);
    let title = if title.trim().is_empty() { truncate(&prompt, 60) } else { title };
    let task = new_task(&app, "image", &title, &prompt, "quick", None, None)?;
    let dir = PathBuf::from(&task.dir);

    // Copy reference files next to the task so Codex (sandboxed to this folder) can read them.
    let attach_root = settings::research_root(&app).join("attachments").canonicalize().ok();
    let mut copied = vec![];
    for r in refs.unwrap_or_default() {
        let Ok(src) = Path::new(&r).canonicalize() else { continue };
        if attach_root.as_ref().map_or(true, |root| !src.starts_with(root)) {
            continue;
        }
        let refs_dir = dir.join("refs");
        let _ = std::fs::create_dir_all(&refs_dir);
        if let Some(name) = src.file_name() {
            let dest = refs_dir.join(name);
            if std::fs::copy(&src, &dest).is_ok() {
                copied.push(dest.display().to_string());
            }
        }
    }
    if !copied.is_empty() {
        if let Some(t) = app.state::<TaskStore>().tasks.lock().unwrap().get_mut(&task.id) {
            t.refs = copied.clone();
        }
    }
    let task = app.state::<TaskStore>().tasks.lock().unwrap().get(&task.id).cloned().unwrap_or(task);

    let mut args = base_args(&s, &dir, "quick", false);
    args.push(image_prompt(&prompt, count, aspect.as_deref().unwrap_or("square"), &copied, ref_notes.as_deref().unwrap_or("")));
    save_index(&app);
    let _ = app.emit("task-update", &task);
    tauri::async_runtime::spawn(run_codex(app.clone(), task.id, args, dir, codex));
    Ok(task)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    name: String,
    path: String,
}

/// Copy files the user attached (drag and drop or the paperclip) into ~/Jarvis/attachments.
#[tauri::command]
pub fn import_attachments(app: AppHandle, paths: Vec<String>) -> Result<Vec<Attachment>, String> {
    let dir = settings::research_root(&app).join("attachments");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut out = vec![];
    let mut skipped = vec![];
    for p in paths {
        let src = Path::new(&p);
        let ext = src.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let name = src.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        if !IMAGE_EXTS.contains(&ext.as_str()) || name.is_empty() {
            skipped.push(name);
            continue;
        }
        let dest = dir.join(format!("{}-{}", now_ms(), name.replace(' ', "_")));
        std::fs::copy(src, &dest).map_err(|e| format!("Could not copy {name}: {e}"))?;
        out.push(Attachment { name, path: dest.display().to_string() });
    }
    if out.is_empty() && !skipped.is_empty() {
        return Err(format!("Only PNG, JPG and WebP images can be attached ({} skipped).", skipped.join(", ")));
    }
    Ok(out)
}

/// Image file as a data URL, limited to files inside the Jarvis folder.
#[tauri::command]
pub fn read_image(app: AppHandle, path: String) -> Result<String, String> {
    use base64::Engine;
    let root = settings::research_root(&app).canonicalize().map_err(|e| e.to_string())?;
    let file = Path::new(&path).canonicalize().map_err(|_| "Image not found.".to_string())?;
    if !file.starts_with(&root) {
        return Err("That file is outside the Jarvis folder.".into());
    }
    let ext = file.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mime = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    };
    let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
    Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[tauri::command]
pub fn reveal_path(app: AppHandle, path: String) -> Result<(), String> {
    app.opener().reveal_item_in_dir(path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cancel_task(store: State<'_, TaskStore>, id: u32) -> Result<(), String> {
    match store.cancels.lock().unwrap().remove(&id) {
        Some(tx) => {
            let _ = tx.send(());
            Ok(())
        }
        None => Err(format!("Task #{id} is not running.")),
    }
}

#[tauri::command]
pub fn read_report(store: State<'_, TaskStore>, id: u32) -> Result<String, String> {
    let dir = store.tasks.lock().unwrap().get(&id).map(|t| t.dir.clone()).ok_or(format!("There is no task #{id}."))?;
    std::fs::read_to_string(Path::new(&dir).join("report.md")).map_err(|_| "This task has no report yet.".to_string())
}

#[tauri::command]
pub fn reveal_task(app: AppHandle, id: u32) -> Result<(), String> {
    let dir = app.state::<TaskStore>().tasks.lock().unwrap().get(&id).map(|t| t.dir.clone()).ok_or(format!("There is no task #{id}."))?;
    let report = Path::new(&dir).join("report.md");
    let target = if report.exists() { report } else { PathBuf::from(dir) };
    app.opener().reveal_item_in_dir(target).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn append_note(app: AppHandle, text: String) -> Result<String, String> {
    use std::io::Write;
    let root = settings::research_root(&app);
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let path = root.join("notes.md");
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| e.to_string())?;
    let secs = now_ms() / 1000;
    writeln!(f, "- {} <!-- {secs} -->", text.trim().replace('\n', " ")).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn read_notes(app: AppHandle) -> String {
    std::fs::read_to_string(settings::research_root(&app).join("notes.md")).unwrap_or_default()
}

#[tauri::command]
pub async fn codex_status(app: AppHandle) -> CodexStatus {
    let s = settings::load(&app);
    let Some(path) = find_codex(&app, &s) else {
        return CodexStatus {
            found: false,
            path: String::new(),
            version: String::new(),
            logged_in: false,
            message: "Codex CLI not found. Install it with `npm install -g @openai/codex`.".into(),
        };
    };
    let run = |args: &'static [&'static str]| {
        let mut c = Command::new(&path);
        c.args(args).env("PATH", child_path(&app)).stdin(Stdio::null()).kill_on_drop(true);
        async move { tokio::time::timeout(Duration::from_secs(15), c.output()).await }
    };
    let version = match run(&["--version"]).await {
        Ok(Ok(o)) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Ok(Ok(o)) => {
            return CodexStatus {
                found: true,
                path: path.display().to_string(),
                version: String::new(),
                logged_in: false,
                message: format!("Codex is installed but fails to run: {}", truncate(&String::from_utf8_lossy(&o.stderr), 200)),
            }
        }
        _ => {
            return CodexStatus {
                found: true,
                path: path.display().to_string(),
                version: String::new(),
                logged_in: false,
                message: "Codex is installed but did not start. Reinstall it: npm install -g @openai/codex@latest".into(),
            }
        }
    };
    let login = run(&["login", "status"]).await;
    let (logged_in, text) = match login {
        Ok(Ok(o)) => {
            let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            (o.status.success() && !text.to_lowercase().contains("not logged in"), text)
        }
        _ => (false, String::new()),
    };
    CodexStatus {
        found: true,
        path: path.display().to_string(),
        version,
        logged_in,
        message: if logged_in { truncate(&text, 120) } else { "Not logged in. Run `codex login` in Terminal.".into() },
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningLevel {
    effort: String,
    description: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CodexModel {
    slug: String,
    display_name: String,
    description: String,
    default_reasoning: String,
    reasoning_levels: Vec<ReasoningLevel>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexModels {
    models: Vec<CodexModel>,
    /// Model and reasoning level from ~/.codex/config.toml, used when Jarvis doesn't override them.
    default_model: String,
    default_reasoning: String,
    error: String,
}

fn config_value(toml: &str, key: &str) -> String {
    toml.lines()
        .map(str::trim)
        .take_while(|l| !l.starts_with('['))
        .find_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
        })
        .unwrap_or_default()
}

/// Models this Codex login can use, from `codex debug models`.
#[tauri::command]
pub async fn codex_models(app: AppHandle) -> CodexModels {
    let s = settings::load(&app);
    let config = std::fs::read_to_string(settings::home(&app).join(".codex/config.toml")).unwrap_or_default();
    let mut out = CodexModels {
        models: vec![],
        default_model: config_value(&config, "model"),
        default_reasoning: config_value(&config, "model_reasoning_effort"),
        error: String::new(),
    };
    let Some(path) = find_codex(&app, &s) else {
        out.error = "Codex CLI not found.".into();
        return out;
    };
    let mut cmd = Command::new(&path);
    cmd.args(["debug", "models"]).env("PATH", child_path(&app)).stdin(Stdio::null()).kill_on_drop(true);
    let output = match tokio::time::timeout(Duration::from_secs(20), cmd.output()).await {
        Ok(Ok(o)) if o.status.success() => o.stdout,
        _ => {
            out.error = "Could not read the model list from Codex.".into();
            return out;
        }
    };
    let Ok(v) = serde_json::from_slice::<Value>(&output) else {
        out.error = "Codex returned a model list Jarvis could not read.".into();
        return out;
    };
    for m in v["models"].as_array().into_iter().flatten() {
        if m["visibility"].as_str() != Some("list") {
            continue;
        }
        let levels = m["supported_reasoning_levels"]
            .as_array()
            .map(|a| a.iter().filter_map(|l| serde_json::from_value::<ReasoningLevel>(l.clone()).ok()).collect())
            .unwrap_or_default();
        out.models.push(CodexModel {
            slug: m["slug"].as_str().unwrap_or("").to_string(),
            display_name: m["display_name"].as_str().or(m["slug"].as_str()).unwrap_or("").to_string(),
            description: m["description"].as_str().unwrap_or("").to_string(),
            default_reasoning: m["default_reasoning_level"].as_str().unwrap_or("").to_string(),
            reasoning_levels: levels,
        });
    }
    out
}
