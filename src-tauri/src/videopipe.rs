//! Making a video from a brief, in stages. The code worker (Codex) only ever writes files: its sandbox
//! can't start a browser, a local server or the network. Jarvis does everything else between stages.
//!
//!   1. PLAN     the worker writes STORYBOARD.md and requests.json (what media it wants)
//!   2. APPROVE  the person reads the storyboard and approves it or asks for changes
//!   3. MEDIA    Jarvis fetches and makes what was asked for: catalog blocks, music, sound effects,
//!               photos, icons, logos, a Gemini voiceover, generated pictures, cut-outs
//!   4. COMPOSE  the worker writes index.html from the storyboard and the finished media
//!   5. CHECK    Jarvis runs HyperFrames' checker; the worker fixes what it finds (up to twice)
//!   6. DRAFT    Jarvis renders a draft MP4; a final render is a separate step
//!
//! Progress goes to the app as `video-stage` events: {folder, stage, message, data}.

use crate::preview;
use crate::settings;
use crate::tasks::{self, Status, Task};
use crate::videokit::{self, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::process::Command;
use tokio::sync::oneshot;

/// HyperFrames skills every video worker gets. The first is Jarvis's own and wins where they differ.
const CORE_SKILLS: [&str; 9] = [
    "video-design",
    "hyperframes",
    "hyperframes-core",
    "hyperframes-animation",
    "hyperframes-creative",
    "hyperframes-audio",
    "hyperframes-keyframes",
    "hyperframes-registry",
    "media-use",
];
/// Ready-made workflows for particular kinds of video; one is added to the core set.
const WORKFLOWS: [&str; 9] = ["product-launch-video", "faceless-explainer", "slideshow", "music-to-video", "embedded-captions", "talking-head-recut", "pr-to-video", "motion-graphics", "general-video"];
const MAX_PLAN_ROUNDS: u32 = 3;
const MAX_FIX_ROUNDS: u32 = 2;

static RUNNING: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static STOPPED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static ANSWERS: Mutex<Option<HashMap<String, oneshot::Sender<Answer>>>> = Mutex::new(None);

#[derive(Debug)]
struct Answer {
    approve: bool,
    feedback: String,
}

fn with_set<R>(set: &Mutex<Option<HashSet<String>>>, f: impl FnOnce(&mut HashSet<String>) -> R) -> R {
    let mut g = set.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(HashSet::new))
}

fn stage(app: &AppHandle, folder: &str, stage: &str, message: impl Into<String>, data: Value) {
    let _ = app.emit("video-stage", json!({ "folder": folder, "stage": stage, "message": message.into(), "data": data }));
}

fn stopped(folder: &str) -> bool {
    with_set(&STOPPED, |s| s.contains(folder))
}

// ---------- requests the worker writes ----------

#[derive(Deserialize, Default, Debug)]
#[serde(default)]
pub struct Requests {
    /// Catalog blocks to install (transitions, overlays, charts, effects).
    pub blocks: Vec<String>,
    pub media: Vec<MediaRequest>,
    /// Pictures to generate. `sheet: true` asks for many objects on a plain background, cut out afterwards.
    pub images: Vec<ImageRequest>,
    pub voiceover: Option<VoiceRequest>,
}

#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct MediaRequest {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub intent: String,
}

#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct ImageRequest {
    pub id: String,
    pub prompt: String,
    pub sheet: bool,
}

#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct VoiceRequest {
    pub text: String,
    pub style: String,
    pub voice: String,
    pub language: String,
}

/// An id a worker may name a file after.
fn clean_id(id: &str) -> Option<String> {
    let id = id.trim();
    (!id.is_empty() && id.len() <= 40 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).then(|| id.to_string())
}

fn clean_block(name: &str) -> Option<String> {
    let n = name.trim();
    (!n.is_empty() && n.len() <= 60 && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')).then(|| n.to_string())
}

fn parse_requests(text: &str) -> Result<Requests, String> {
    let r: Requests = serde_json::from_str(text).map_err(|e| format!("requests.json isn't valid: {e}"))?;
    if r.blocks.len() > 20 || r.media.len() > 30 || r.images.len() > 8 {
        return Err("requests.json asks for too much: at most 20 blocks, 30 media items and 8 generated pictures.".into());
    }
    Ok(r)
}

// ---------- the worker stages ----------

fn skills_for(workflow: &str) -> Vec<String> {
    let mut list: Vec<String> = CORE_SKILLS.iter().map(|s| s.to_string()).collect();
    if WORKFLOWS.contains(&workflow) {
        list.push(workflow.to_string());
    }
    list
}

fn plan_request(title: &str, brief: &str, meta: &videokit::VideoMeta, workflow: &str, feedback: &str) -> String {
    let flow = if WORKFLOWS.contains(&workflow) { format!("\nThis is a \"{workflow}\" video: follow know-how/{workflow}/SKILL.md for how that kind of video is planned and paced.\n") } else { String::new() };
    let again = if feedback.trim().is_empty() { String::new() } else { format!("\nThe person read your storyboard and asked for changes. Update STORYBOARD.md and requests.json for them, keeping what they didn't mention:\n\"{}\"\n", feedback.trim()) };
    format!(
        "Stage 1 of 3: PLAN. Do not write index.html yet.\n\
         The video is called \"{title}\" ({format}, {w}x{h}, {secs} seconds; see video.json). What the person asked for:\n\"{brief}\"\n{flow}{again}\n\
         Write two files in this folder:\n\
         1. STORYBOARD.md: the goal and viewer, the design direction (palette, two fonts), then each scene with its start time, length, the one idea, the exact on-screen text, \
            the visual, the narration line, and which assets it uses. Scenes add up to {secs} seconds. Then the full narration script if there is a voiceover.\n\
         2. requests.json: everything Jarvis must fetch or make for you before you compose, as JSON:\n\
         {{\n  \"blocks\": [\"catalog-block-name\"],\n  \"media\": [ {{\"id\": \"music\", \"type\": \"bgm|sfx|image|icon|logo\", \"intent\": \"what you need, in plain words\"}} ],\n  \"images\": [ {{\"id\": \"sheet1\", \"prompt\": \"…\", \"sheet\": true}} ],\n  \"voiceover\": {{\"text\": \"the whole narration\", \"style\": \"warm, friendly\", \"language\": \"en\"}}\n}}\n\
         Leave out any key you don't need (a silent video has no voiceover). `blocks` are names from the HyperFrames catalog (see know-how/hyperframes-registry/SKILL.md and registry-index.json if present); \
         `logo` intents are a domain such as example.com; `images` are for pictures that must be made (a product, an illustration, a sheet of separate objects on a plain background to be cut out). \
         Ask only for what the storyboard really uses, and keep each intent specific. Photos, music and sound effects come from HeyGen's catalog when the person has connected it.\n\
         Speech in files the person gave you (a talking video, an interview) is transcribed by Jarvis after you finish this stage, into assets/<file>.words.json with the exact time of every word. \
         So plan captions and highlights around that: do NOT wait for a transcript and do not ask for one in requests.json.",
        format = meta.format,
        w = meta.width,
        h = meta.height,
        secs = meta.seconds
    )
}

/// The video must run at least as long as its narration, plus a breath at each end.
/// Returns the (possibly longer) video and a sentence for the composer when it was stretched.
fn fit_length(dir: &Path, meta: &videokit::VideoMeta, items: &[Item]) -> (videokit::VideoMeta, String) {
    let Some(voice) = items.iter().find(|i| i.ok && i.kind == "voiceover").and_then(|i| i.duration) else { return (meta.clone(), String::new()) };
    let need = ((voice + 1.2).ceil() as u32).min(180);
    if need <= meta.seconds {
        return (meta.clone(), String::new());
    }
    let mut longer = meta.clone();
    longer.seconds = need;
    if let Ok(json) = serde_json::to_string_pretty(&longer) {
        let _ = std::fs::write(dir.join("video.json"), json);
    }
    // The starting file still has the old length on its root element.
    if let Ok(html) = std::fs::read_to_string(dir.join("index.html")) {
        let old = format!("data-start=\"0\" data-duration=\"{}\" data-width", meta.seconds);
        if html.contains(&old) {
            let _ = std::fs::write(dir.join("index.html"), html.replacen(&old, &format!("data-start=\"0\" data-duration=\"{need}\" data-width"), 1));
        }
    }
    let note = format!(
        "The finished narration runs {voice:.1} seconds, longer than the {} seconds planned, so the video is now {need} seconds long (video.json and the root data-duration say so). \
         Stretch the scene timings in the storyboard to fit the narration, keeping their order and proportions, and keep each scene's caption in step with the voice.",
        meta.seconds
    );
    (longer, note)
}

fn compose_request(meta: &videokit::VideoMeta, manifest_note: &str, length_note: &str) -> String {
    format!(
        "Stage 2 of 3: COMPOSE. The storyboard in STORYBOARD.md was approved. Write the video now: index.html (plus files under compositions/ if you use sub-compositions).\n\
         Canvas {w}x{h}, {secs} seconds (video.json). Start from know-how/video-design/references/video-skeleton.html.\n\
         Jarvis has fetched and made the media. assets/manifest.json lists every file with its id, kind, duration and credit; use those files exactly as they are, by relative path. \
         {manifest_note} {length_note}\n\
         If a requested item failed (ok: false), design without it instead of inventing a file. Catalog blocks you asked for are installed under compositions/ (see \"blocks\" in the manifest for each one's usage snippet).\n\
         If assets/voiceover.words.json exists, time the captions from its words. Mix any music quietly under the voice.\n\
         When you are done run ./hf lint and fix every error. Do not render.",
        w = meta.width,
        h = meta.height,
        secs = meta.seconds
    )
}

fn fix_request(problems: &[String]) -> String {
    format!(
        "Jarvis ran HyperFrames' checker on index.html and it found these problems. Fix every one at its cause, keep the design and timing otherwise as they are, then run ./hf lint:\n{}\n\
         Errors (marked ✗) must be fixed; ignore warnings called nested_structure_needs_subcomposition.",
        problems.iter().map(|p| format!("- {p}")).collect::<Vec<_>>().join("\n")
    )
}

/// Run one worker stage and wait for it.
async fn run_stage(app: &AppHandle, folder: &str, title: &str, request: String, workflow: &str, chat: &str) -> Result<Task, String> {
    let task = tasks::start_code_task(app.clone(), title.to_string(), request, folder.to_string(), None, Some(chat.to_string()), Some("video".into()), Some(skills_for(workflow)))?;
    stage(app, folder, "task", title.to_string(), json!({ "taskId": task.id }));
    loop {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        if stopped(folder) {
            let _ = tasks::cancel_task(tauri::Manager::state::<tasks::TaskStore>(app), task.id);
            return Err("Stopped.".into());
        }
        let Some(t) = tasks::get_task(app, task.id) else { return Err("The worker's task disappeared.".into()) };
        match t.status {
            Status::Running => continue,
            Status::Done => return Ok(t),
            Status::Cancelled => return Err("Stopped.".into()),
            Status::Failed => return Err(if t.error.is_empty() { "The worker stopped without finishing.".into() } else { t.error }),
        }
    }
}

// ---------- the media stage ----------

#[derive(Serialize, Clone, Default)]
#[serde(default)]
struct Item {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    ok: bool,
    file: String,
    duration: Option<f64>,
    note: String,
    credit: String,
}

/// Word timings spread over the narration by length, for captions when no transcription model is
/// installed: each word gets time in proportion to its letters, with a short pause after punctuation.
fn estimate_words(text: &str, total: f64) -> Vec<Value> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() || total <= 0.0 {
        return vec![];
    }
    let weight = |w: &str| w.chars().filter(|c| c.is_alphanumeric()).count().max(1) as f64 + if w.ends_with(['.', '!', '?']) { 3.0 } else if w.ends_with([',', ';', ':']) { 1.5 } else { 0.0 };
    let sum: f64 = words.iter().map(|w| weight(w)).sum();
    let mut t = 0.0f64;
    words
        .iter()
        .map(|w| {
            let len = total * weight(w) / sum;
            let spoken = len * if weight(w) > w.chars().filter(|c| c.is_alphanumeric()).count().max(1) as f64 { 0.8 } else { 1.0 };
            let v = json!({ "text": w, "start": (t * 100.0).round() / 100.0, "end": ((t + spoken) * 100.0).round() / 100.0 });
            t += len;
            v
        })
        .collect()
}

/// Heard words with the script's own spelling: when both have the same number of words, the script's text
/// (names spelled right) gets the heard timings; otherwise the heard words are used as they are.
fn align_words(script: &str, heard: Vec<Value>) -> Vec<Value> {
    let words: Vec<&str> = script.split_whitespace().collect();
    if words.len() != heard.len() {
        return heard;
    }
    words.iter().zip(heard).map(|(w, h)| json!({ "text": w, "start": h["start"], "end": h["end"] })).collect()
}

async fn hf_json(rt: &Runtime, dir: &Path, args: &[&str]) -> Result<Value, String> {
    let (ok, text) = videokit::run_hf_public(rt, dir, args).await?;
    let line = text.lines().rev().find(|l| l.trim_start().starts_with('{')).unwrap_or("");
    let v: Value = serde_json::from_str(line.trim()).map_err(|_| tasks::truncate(&text, 200))?;
    if ok && v["ok"] != json!(false) {
        Ok(v)
    } else {
        Err(v["error"].as_str().map(String::from).unwrap_or_else(|| tasks::truncate(&text, 200)))
    }
}

async fn resolve_media(rt: &Runtime, dir: &Path, m: &MediaRequest) -> Item {
    let mut item = Item { id: m.id.clone(), kind: m.kind.clone(), ..Default::default() };
    if !["bgm", "sfx", "image", "icon", "logo"].contains(&m.kind.as_str()) {
        item.note = format!("Jarvis can't fetch \"{}\".", m.kind);
        return item;
    }
    match hf_json(rt, dir, &["media-use", "resolve", "--type", &m.kind, "--intent", &m.intent, "--project", ".", "--json"]).await {
        Ok(v) => {
            let from = v["path"].as_str().unwrap_or("");
            let ext = Path::new(from).extension().and_then(|e| e.to_str()).unwrap_or("bin");
            let to = format!("assets/{}.{ext}", m.id);
            if !from.is_empty() && std::fs::copy(dir.join(from), dir.join(&to)).is_ok() {
                item.ok = true;
                item.file = to;
                item.duration = v["duration"].as_f64();
                item.credit = v["provenance"]["provider"].as_str().map(String::from).unwrap_or_default();
                item.note = v["description"].as_str().unwrap_or("").to_string();
            } else {
                item.note = "The file wasn't saved.".into();
            }
        }
        Err(e) => item.note = e,
    }
    item
}

/// A generated picture: a worker makes it with Codex's image generation, then it is copied into assets/.
async fn generate_image(app: &AppHandle, dir: &Path, meta: &videokit::VideoMeta, req: &ImageRequest, chat: &str) -> Vec<Item> {
    let mut item = Item { id: req.id.clone(), kind: "image".into(), ..Default::default() };
    let aspect = match meta.format.as_str() {
        "portrait" => "tall",
        "square" => "square",
        _ => "wide",
    };
    let prompt = if req.sheet {
        format!("{}\n\nLay out each object separately on one sheet, with generous empty space between every object so none touch or overlap, on a perfectly flat, plain pure white background with no shadows, no floor, no gradient and no text.", req.prompt.trim())
    } else {
        req.prompt.trim().to_string()
    };
    let started = tasks::start_image(app.clone(), format!("Picture for the video: {}", req.id), prompt, Some(1), Some(if req.sheet { "square".into() } else { aspect.into() }), None, None, Some(chat.to_string()));
    let task = match started {
        Ok(t) => t,
        Err(e) => {
            item.note = e;
            return vec![item];
        }
    };
    let done = loop {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        match tasks::get_task(app, task.id) {
            Some(t) if t.status == Status::Running => continue,
            Some(t) => break t,
            None => break task,
        }
    };
    let Some(src) = done.images.first().filter(|_| done.status == Status::Done) else {
        item.note = if done.error.is_empty() { "No picture was made.".into() } else { done.error };
        return vec![item];
    };
    let to = format!("assets/{}.png", req.id);
    if std::fs::copy(src, dir.join(&to)).is_err() {
        item.note = "The picture wasn't saved.".into();
        return vec![item];
    }
    item.ok = true;
    item.file = to.clone();
    item.credit = "generated".into();
    if !req.sheet {
        return vec![item];
    }
    // A sheet of separate objects on white: cut each one out into its own transparent picture.
    match crate::cutout::split_sheet(&dir.join(&to), &dir.join("assets"), &req.id) {
        Ok(parts) if !parts.is_empty() => {
            let mut out = vec![item];
            for (i, p) in parts.iter().enumerate() {
                out.push(Item { id: format!("{}-{}", req.id, i + 1), kind: "cutout".into(), ok: true, file: format!("assets/{p}"), credit: "generated, cut out".into(), note: format!("object {} of {}", i + 1, parts.len()), ..Default::default() });
            }
            out
        }
        Ok(_) => {
            item.note = "No separate objects were found on the sheet.".into();
            vec![item]
        }
        Err(e) => {
            item.note = e;
            vec![item]
        }
    }
}

async fn make_voiceover(app: &AppHandle, rt: &Runtime, dir: &Path, v: &VoiceRequest) -> Item {
    let mut item = Item { id: "voiceover".into(), kind: "voiceover".into(), ..Default::default() };
    let settings = settings::load(app);
    let mut fell_back = String::new();
    match crate::voices::speak(app, v.text.trim()).await {
        Some(Ok(sp)) => {
            let to = format!("assets/voiceover.{}", sp.ext);
            if std::fs::write(dir.join(&to), &sp.audio).is_err() {
                item.note = "The voiceover wasn't saved.".into();
                return item;
            }
            // The length is where the last word ends, plus the breath after it.
            let secs = sp.words.last().and_then(|w| w["end"].as_f64()).map(|e| e + 0.2).unwrap_or_else(|| (v.text.split_whitespace().count() as f64 * 0.4).max(1.0));
            let words = if sp.words.is_empty() { estimate_words(&v.text, secs) } else { sp.words };
            let _ = std::fs::write(dir.join("assets/voiceover.words.json"), serde_json::to_string_pretty(&json!({ "duration": secs, "estimated": false, "words": words })).unwrap_or_default());
            item.ok = true;
            item.file = to;
            item.duration = Some(secs);
            item.credit = "ElevenLabs speech".into();
            item.note = "Word timings for captions are in assets/voiceover.words.json.".into();
            return item;
        }
        Some(Err(e)) if settings.gemini_api_key.trim().is_empty() => {
            item.note = e;
            return item;
        }
        Some(Err(e)) => fell_back = format!("{e} Gemini's voice was used instead. "),
        None => {}
    }
    let key = settings.gemini_api_key;
    if key.trim().is_empty() {
        item.note = "No voice is set up (no Gemini key and no ElevenLabs key), so there is no voiceover.".into();
        return item;
    }
    let engine = rt.skills().join("media-use/audio/scripts/audio.mjs");
    if !engine.is_file() {
        item.note = "The audio engine isn't unpacked.".into();
        return item;
    }
    let request = json!({
        "provider": "gemini",
        "voice": if !v.voice.trim().is_empty() { v.voice.trim() } else if settings.narration_gemini_voice.trim().is_empty() { "Kore" } else { settings.narration_gemini_voice.trim() },
        "lang": if v.language.trim().is_empty() { "en" } else { v.language.trim() },
        "style": if v.style.trim().is_empty() { "warm, clear, unhurried" } else { v.style.trim() },
        "lines": [{ "id": "vo", "text": v.text.trim() }],
        "bgm": { "mode": "none" }
    });
    let _ = std::fs::write(dir.join("audio_request.json"), request.to_string());
    let mut cmd = Command::new(rt.node_path());
    cmd.arg(&engine).args(["--request", "audio_request.json", "--out", "audio_meta.json"]).current_dir(dir).env_clear().envs(rt.env_pairs()).env("GEMINI_API_KEY", key.trim()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let out = match tokio::time::timeout(Duration::from_secs(180), cmd.output()).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => {
            item.note = format!("The audio engine didn't start: {e}");
            return item;
        }
        Err(_) => {
            item.note = "The voiceover took too long.".into();
            return item;
        }
    };
    let _ = std::fs::remove_file(dir.join("audio_request.json"));
    let meta: Value = std::fs::read_to_string(dir.join("audio_meta.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
    let _ = std::fs::remove_file(dir.join("audio_meta.json"));
    let voice = &meta["voices"][0];
    let (Some(path), Some(secs)) = (voice["path"].as_str(), voice["duration_s"].as_f64()) else {
        item.note = format!("The voiceover wasn't made: {}", tasks::truncate(&String::from_utf8_lossy(&out.stderr), 160));
        return item;
    };
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("wav");
    let to = format!("assets/voiceover.{ext}");
    if std::fs::copy(dir.join(path), dir.join(&to)).is_err() {
        item.note = "The voiceover wasn't saved.".into();
        return item;
    }
    // Exact word times when the speech model is installed; otherwise spread over the length of the narration.
    let (words, estimated) = match voice["words"].as_array() {
        Some(w) if !w.is_empty() => (w.clone(), false),
        _ => match videokit::transcribe_words(rt, dir, &to).await {
            Some(heard) => (align_words(&v.text, heard), false),
            None => (estimate_words(&v.text, secs), true),
        },
    };
    let _ = std::fs::write(dir.join("assets/voiceover.words.json"), serde_json::to_string_pretty(&json!({ "duration": secs, "estimated": estimated, "words": words })).unwrap_or_default());
    item.ok = true;
    item.file = to;
    item.duration = Some(secs);
    item.credit = "Gemini speech".into();
    item.note = format!("{fell_back}Word timings for captions are in assets/voiceover.words.json.");
    item
}

/// Spoken files the person gave (talking video, interview audio…): the words with their times, so captions
/// and highlights can be placed where each word is really said. Installs the speech model first if needed.
async fn transcribe_given(app: &AppHandle, rt: &Runtime, dir: &Path, folder: &str) -> Vec<Item> {
    let ours = |name: &str| name.starts_with("voiceover") || name.starts_with("sfx") || name.starts_with("bgm");
    let mut found: Vec<String> = vec![];
    for entry in std::fs::read_dir(dir.join("assets")).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let ext = Path::new(&name).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let stem = Path::new(&name).file_stem().and_then(|e| e.to_str()).unwrap_or("").to_string();
        if ["mp4", "mov", "m4v", "webm", "mp3", "wav", "m4a", "aac", "ogg"].contains(&ext.as_str()) && !ours(&name) && !dir.join(format!("assets/{stem}.words.json")).is_file() {
            found.push(name);
        }
    }
    found.sort();
    if found.is_empty() {
        return vec![];
    }
    if !rt.captions_ready() {
        stage(app, folder, "media", "Installing the speech model so the words can be timed (once, about 700 MB)…", Value::Null);
        if let Err(e) = videokit::captions_install(app.clone()).await {
            return found.into_iter().map(|n| Item { id: format!("{n}-words"), kind: "transcript".into(), note: e.clone(), ..Default::default() }).collect();
        }
    }
    let rt = Runtime::for_app(app);
    let mut items = vec![];
    for name in found {
        stage(app, folder, "media", format!("Listening to {name}…"), Value::Null);
        let rel = format!("assets/{name}");
        let mut item = Item { id: format!("{name}-words"), kind: "transcript".into(), ..Default::default() };
        match videokit::transcribe_words(&rt, dir, &rel).await {
            Some(words) => {
                let out = format!("assets/{}.words.json", Path::new(&name).file_stem().and_then(|e| e.to_str()).unwrap_or("media"));
                let end = words.last().and_then(|w| w["end"].as_f64()).unwrap_or(0.0);
                let _ = std::fs::write(dir.join(&out), serde_json::to_string_pretty(&json!({ "source": rel, "duration": end, "estimated": false, "words": words })).unwrap_or_default());
                item.ok = true;
                item.file = out;
                item.note = format!("{} words with exact start and end times, spoken in {rel}. Use them for captions and highlights.", words.len());
            }
            None => item.note = format!("No speech was found in {rel} (or it couldn't be transcribed)."),
        }
        items.push(item);
    }
    items
}

/// Fetch and make everything in requests.json. Writes assets/manifest.json and returns the items.
async fn fulfill(app: &AppHandle, rt: &Runtime, dir: &Path, folder: &str, meta: &videokit::VideoMeta, reqs: &Requests, chat: &str) -> Vec<Item> {
    let _ = std::fs::create_dir_all(dir.join("assets"));
    let mut items: Vec<Item> = vec![];
    let mut blocks: Vec<Value> = vec![];
    items.extend(transcribe_given(app, rt, dir, folder).await);
    for name in reqs.blocks.iter().filter_map(|b| clean_block(b)) {
        stage(app, folder, "media", format!("Installing the “{name}” effect…"), Value::Null);
        match hf_json(rt, dir, &["add", &name, "--dir", ".", "--no-clipboard", "--json"]).await {
            Ok(v) => blocks.push(json!({ "name": name, "ok": true, "result": v })),
            Err(e) => blocks.push(json!({ "name": name, "ok": false, "error": e })),
        }
    }
    for m in &reqs.media {
        let Some(id) = clean_id(&m.id) else { continue };
        let m = MediaRequest { id, ..m.clone() };
        stage(app, folder, "media", format!("Finding {}: {}", m.kind, tasks::truncate(&m.intent, 60)), Value::Null);
        let item = resolve_media(rt, dir, &m).await;
        // A photo that the catalog couldn't give is made instead.
        if !item.ok && m.kind == "image" {
            stage(app, folder, "media", format!("Making a picture: {}", tasks::truncate(&m.intent, 60)), Value::Null);
            items.extend(generate_image(app, dir, meta, &ImageRequest { id: m.id.clone(), prompt: m.intent.clone(), sheet: false }, chat).await);
        } else {
            items.push(item);
        }
        if stopped(folder) {
            break;
        }
    }
    for im in &reqs.images {
        let Some(id) = clean_id(&im.id) else { continue };
        stage(app, folder, "media", format!("Making {}…", if im.sheet { "a sheet of objects" } else { "a picture" }), Value::Null);
        items.extend(generate_image(app, dir, meta, &ImageRequest { id, ..im.clone() }, chat).await);
        if stopped(folder) {
            break;
        }
    }
    if let Some(v) = reqs.voiceover.as_ref().filter(|v| !v.text.trim().is_empty()) {
        stage(app, folder, "media", "Recording the voiceover…", Value::Null);
        items.push(make_voiceover(app, rt, dir, v).await);
    }
    let manifest = json!({ "items": items, "blocks": blocks });
    let _ = std::fs::write(dir.join("assets/manifest.json"), serde_json::to_string_pretty(&manifest).unwrap_or_default());
    items
}

fn manifest_note(items: &[Item]) -> String {
    let ok = items.iter().filter(|i| i.ok).count();
    let failed: Vec<&str> = items.iter().filter(|i| !i.ok).map(|i| i.id.as_str()).collect();
    if failed.is_empty() {
        format!("All {ok} items are ready.")
    } else {
        format!("{ok} items are ready; these could not be made: {}.", failed.join(", "))
    }
}

// ---------- the whole thing ----------

pub struct Spec {
    pub folder: String,
    pub title: String,
    pub brief: String,
    pub workflow: String,
    pub chat: String,
    /// Skip the approval step (tests, and "just make it").
    pub auto_approve: bool,
}

async fn wait_for_answer(folder: &str) -> Answer {
    let (tx, rx) = oneshot::channel();
    ANSWERS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(folder.to_string(), tx);
    rx.await.unwrap_or(Answer { approve: false, feedback: String::new() })
}

/// The person's answer to a storyboard: approve it, or say what to change.
#[tauri::command]
pub fn video_answer(folder: String, approve: bool, feedback: Option<String>) -> Result<(), String> {
    let tx = ANSWERS.lock().unwrap_or_else(|e| e.into_inner()).as_mut().and_then(|m| m.remove(&folder));
    match tx {
        Some(tx) => tx.send(Answer { approve, feedback: feedback.unwrap_or_default() }).map_err(|_| "That storyboard isn't waiting any more.".to_string()),
        None => Err("There's no storyboard waiting for an answer.".into()),
    }
}

/// Stop a video that is being made.
#[tauri::command]
pub fn video_stop(folder: String) {
    with_set(&STOPPED, |s| s.insert(folder.clone()));
    if let Some(tx) = ANSWERS.lock().unwrap_or_else(|e| e.into_inner()).as_mut().and_then(|m| m.remove(&folder)) {
        let _ = tx.send(Answer { approve: false, feedback: String::new() });
    }
    // Only this video's render or check; another video being made carries on.
    videokit::video_cancel(Some(folder));
}

/// Make a video from a brief. The project folder is made first (video_new). Runs in the background.
#[tauri::command]
pub async fn video_make(app: AppHandle, folder: String, title: String, brief: String, workflow: Option<String>, chat_id: Option<String>, auto_approve: Option<bool>) -> Result<(), String> {
    let rt = Runtime::for_app(&app);
    if !rt.ready() {
        return Err("The video tools aren't set up yet. Open Set up and press Set up video.".into());
    }
    let (dir, _) = preview::project_dir(&app, &folder)?;
    if !dir.join("video.json").is_file() {
        return Err("That folder isn't a video project.".into());
    }
    if with_set(&RUNNING, |s| !s.insert(folder.clone())) {
        return Err("This video is already being made.".into());
    }
    with_set(&STOPPED, |s| s.remove(&folder));
    let spec = Spec { folder: folder.clone(), title, brief, workflow: workflow.unwrap_or_default(), chat: chat_id.unwrap_or_default(), auto_approve: auto_approve.unwrap_or(false) };
    tauri::async_runtime::spawn(async move {
        let result = produce(&app, &rt, &dir, &spec).await;
        with_set(&RUNNING, |s| s.remove(&spec.folder));
        match result {
            Ok(note) => stage(&app, &spec.folder, "done", note, Value::Null),
            Err(e) => stage(&app, &spec.folder, "error", e, Value::Null),
        }
    });
    Ok(())
}

async fn produce(app: &AppHandle, rt: &Runtime, dir: &Path, spec: &Spec) -> Result<String, String> {
    let folder = spec.folder.as_str();
    videokit::ensure_content(app).await?;
    let meta: videokit::VideoMeta = std::fs::read_to_string(dir.join("video.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).ok_or("video.json is missing.")?;
    // The catalog's names, so the worker can pick effects that exist.
    let _ = std::fs::copy(rt.content().join("registry/registry.json"), dir.join("registry-index.json"));

    // 1-2. Plan, and the person's approval.
    let mut feedback = String::new();
    let mut reqs = Requests::default();
    for round in 1..=MAX_PLAN_ROUNDS {
        stage(app, folder, "plan", if round == 1 { "Planning the video…".to_string() } else { "Updating the storyboard…".to_string() }, Value::Null);
        run_stage(app, folder, &format!("Storyboard: {}", spec.title), plan_request(&spec.title, &spec.brief, &meta, &spec.workflow, &feedback), &spec.workflow, &spec.chat).await?;
        let board = std::fs::read_to_string(dir.join("STORYBOARD.md")).map_err(|_| "The worker didn't write a storyboard.".to_string())?;
        reqs = match std::fs::read_to_string(dir.join("requests.json")) {
            Ok(t) => parse_requests(&t)?,
            Err(_) => Requests::default(),
        };
        if spec.auto_approve {
            break;
        }
        stage(app, folder, "storyboard", "The storyboard is ready. Read it and approve it, or say what to change.", json!({ "markdown": board, "round": round }));
        let answer = wait_for_answer(folder).await;
        if stopped(folder) {
            return Err("Stopped.".into());
        }
        if answer.approve {
            break;
        }
        if round == MAX_PLAN_ROUNDS {
            return Err("The storyboard still isn't right after three tries. Start again with a clearer brief.".into());
        }
        feedback = answer.feedback;
    }

    // 3. Media.
    stage(app, folder, "media", "Getting the media ready…", Value::Null);
    let items = fulfill(app, rt, dir, folder, &meta, &reqs, &spec.chat).await;
    if stopped(folder) {
        return Err("Stopped.".into());
    }
    let (meta, length_note) = fit_length(dir, &meta, &items);

    // 4. Compose.
    stage(app, folder, "compose", "Building the video…", Value::Null);
    run_stage(app, folder, &format!("Compose: {}", spec.title), compose_request(&meta, &manifest_note(&items), &length_note), &spec.workflow, &spec.chat).await?;
    checked_draft(app, rt, dir, folder, &spec.workflow, &spec.chat).await
}

/// Check the video, have the worker fix what the checker finds, then render a draft.
async fn checked_draft(app: &AppHandle, rt: &Runtime, dir: &Path, folder: &str, workflow: &str, chat: &str) -> Result<String, String> {
    let mut last = videokit::CheckReport { ok: false, problems: vec![], text: String::new() };
    for round in 0..=MAX_FIX_ROUNDS {
        stage(app, folder, "check", if round == 0 { "Checking the video…".to_string() } else { "Checking the fixes…".to_string() }, Value::Null);
        last = videokit::check_project_public(rt, dir).await?;
        if last.ok {
            break;
        }
        if round == MAX_FIX_ROUNDS || stopped(folder) {
            break;
        }
        stage(app, folder, "fix", format!("Fixing {} problem{}…", last.problems.len().max(1), if last.problems.len() == 1 { "" } else { "s" }), json!({ "problems": last.problems }));
        let problems = if last.problems.is_empty() { vec![tasks::truncate(&last.text, 400)] } else { last.problems.clone() };
        run_stage(app, folder, "Fix the video", fix_request(&problems), workflow, chat).await?;
    }
    if !last.ok {
        let why = last.problems.first().cloned().unwrap_or_else(|| "the checker could not run it".into());
        return Err(format!("The video still has a problem Jarvis couldn't fix: {why}"));
    }
    let mut done = render_draft(app, rt, dir, folder).await?;
    // A second look at the real frames; one round of fixes if the art director finds something.
    let mut review_note = String::new();
    if !stopped(folder) {
        match visual_review(app, rt, dir, folder).await {
            Ok(problems) if !problems.is_empty() => {
                stage(app, folder, "fix", format!("The art director found {} thing{} to fix…", problems.len(), if problems.len() == 1 { "" } else { "s" }), json!({ "problems": problems }));
                let request = format!(
                    "Jarvis rendered a draft and looked at its frames. The art director found these layout problems:\n{}\n\
                     Fix each at its cause in index.html (move, resize or retime the element, never delete the content), keep everything else as it is, then run ./hf lint.",
                    problems.iter().map(|p| format!("- {p}")).collect::<Vec<_>>().join("\n")
                );
                if run_stage(app, folder, "Polish the video", request, workflow, chat).await.is_ok() {
                    stage(app, folder, "check", "Checking the polish…", Value::Null);
                    if videokit::check_project_public(rt, dir).await.map(|r| r.ok).unwrap_or(false) {
                        done = render_draft(app, rt, dir, folder).await?;
                        review_note = format!(" The art director's {} note{} {} applied.", problems.len(), if problems.len() == 1 { "" } else { "s" }, if problems.len() == 1 { "was" } else { "were" });
                    } else {
                        review_note = " The art director's fixes didn't pass the checker, so the first draft was kept.".into();
                        let _ = std::fs::write(dir.join("review-notes.md"), format!("# Left unfixed\n\n{}\n", problems.join("\n")));
                    }
                }
            }
            Ok(_) => review_note = " The art director found nothing to fix.".into(),
            Err(_) => {}
        }
    }
    Ok(format!("The draft is ready ({} KB, rendered in {}s).{review_note}", done.size / 1024, done.seconds_taken))
}

async fn render_draft(app: &AppHandle, rt: &Runtime, dir: &Path, folder: &str) -> Result<videokit::RenderDone, String> {
    stage(app, folder, "render", "Rendering a draft…", Value::Null);
    let handle = app.clone();
    let key = folder.to_string();
    let render = videokit::render_public(rt, dir, "draft", move |percent, message| {
        let _ = handle.emit("video-render", json!({ "folder": key, "percent": percent, "message": message }));
    });
    tokio::pin!(render);
    // Stop works while it waits for another video's render too (a running render is killed by video_stop).
    loop {
        tokio::select! {
            done = &mut render => return done,
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                if stopped(folder) {
                    return Err("Stopped.".into());
                }
            }
        }
    }
}

/// Look at the draft's frames. Quietly does nothing without a Gemini key or if the look fails.
async fn visual_review(app: &AppHandle, rt: &Runtime, dir: &Path, folder: &str) -> Result<Vec<String>, String> {
    let key = settings::load(app).gemini_api_key;
    if key.trim().is_empty() {
        return Err("no key".into());
    }
    stage(app, folder, "check", "The art director is looking at the draft…", Value::Null);
    let meta = videokit::video_info(app.clone(), folder.to_string()).map(|i| i.meta).map_err(|e| e.to_string())?;
    let frames = crate::videoreview::frames(&rt.ffmpeg(), &dir.join("renders/draft.mp4"), meta.seconds as f64, &std::env::temp_dir().join(format!("jarvis-review-{}", std::process::id()))).await;
    crate::videoreview::review(crate::videoreview::gemini_base(), key.trim(), &meta.title, &frames).await
}

/// Change a finished video: the worker edits it, the checker looks, and a new draft is rendered.
#[tauri::command]
pub async fn video_edit(app: AppHandle, folder: String, instructions: String, region: Option<String>, chat_id: Option<String>) -> Result<(), String> {
    let rt = Runtime::for_app(&app);
    let (dir, _) = preview::project_dir(&app, &folder)?;
    if !dir.join("index.html").is_file() {
        return Err("This video has no composition yet.".into());
    }
    if instructions.trim().is_empty() {
        return Err("Say what to change.".into());
    }
    if with_set(&RUNNING, |s| !s.insert(folder.clone())) {
        return Err("This video is being worked on. Wait for it to finish, or stop it first.".into());
    }
    with_set(&STOPPED, |s| s.remove(&folder));
    let chat = chat_id.unwrap_or_default();
    let pointed = region.filter(|r| !r.trim().is_empty()).map(|r| format!("\nThe person pointed at this part of the picture: {}.", r.trim())).unwrap_or_default();
    let request = format!(
        "{}\n\nThis is a change to the existing video in this folder, not a new one: read STORYBOARD.md and index.html, make precise edits, and keep everything the change doesn't touch as it is. \
         Media can't be fetched for a change: use only the files already in assets/.{pointed} Run ./hf lint when you are done.",
        instructions.trim()
    );
    tauri::async_runtime::spawn(async move {
        let result = async {
            videokit::ensure_content(&app).await?;
            stage(&app, &folder, "compose", "Changing the video…", Value::Null);
            run_stage(&app, &folder, &format!("Change: {}", tasks::truncate(&instructions, 50)), request, "", &chat).await?;
            checked_draft(&app, &rt, &dir, &folder, "", &chat).await
        }
        .await;
        with_set(&RUNNING, |s| s.remove(&folder));
        match result {
            Ok(note) => stage(&app, &folder, "done", note, Value::Null),
            Err(e) => stage(&app, &folder, "error", e, Value::Null),
        }
    });
    Ok(())
}

/// Developer builds only: `JARVIS_E2E_VIDEO=<brief.json>` makes the app run the whole video flow by itself
/// at start-up (setting up the toolbox if needed, approving its own storyboard) and log every stage to
/// `JARVIS_E2E_LOG`. It exists to test the real app process, with real settings, Codex and tools.
#[cfg(debug_assertions)]
pub fn e2e_hook(app: AppHandle) {
    use std::io::Write;
    use tauri::Listener;
    let Some(spec_path) = std::env::var_os("JARVIS_E2E_VIDEO") else { return };
    let log_path = std::env::var_os("JARVIS_E2E_LOG").map(std::path::PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("jarvis-e2e-video.log"));
    let log = move |line: String| {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
            let _ = writeln!(f, "[{}] {}", chrono::Local::now().format("%H:%M:%S"), line);
        }
    };
    for name in ["video-setup", "video-stage", "video-render", "heygen-login"] {
        let log = log.clone();
        app.listen(name, move |e| log(format!("{name} {}", e.payload())));
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(12)).await;
        let spec: Value = match std::fs::read_to_string(&spec_path).ok().and_then(|t| serde_json::from_str(&t).ok()) {
            Some(v) => v,
            None => return log("E2E: couldn't read the brief file".into()),
        };
        // One brief, or {"runs": [brief, brief…]} made one after the other.
        let runs: Vec<Value> = spec["runs"].as_array().cloned().unwrap_or_else(|| vec![spec.clone()]);
        for run in runs {
            e2e_run(&app, &run, &log).await;
        }
        log("E2E: all runs finished".into());
    });
}

#[cfg(debug_assertions)]
fn clean_rel(p: &str) -> bool {
    !p.is_empty() && !p.starts_with('/') && !p.split('/').any(|c| c == "..")
}

#[cfg(debug_assertions)]
async fn e2e_run(app: &AppHandle, spec: &Value, log: &(impl Fn(String) + Clone + Send + Sync + 'static)) {
    use tauri::Listener;
    let text = |k: &str, d: &str| spec[k].as_str().unwrap_or(d).to_string();
    log(format!("E2E: starting \"{}\"", text("title", "Test video")));
    if !Runtime::for_app(app).ready() {
        log("E2E: setting up the video tools".into());
        if let Err(e) = videokit::video_setup(app.clone()).await {
            return log(format!("E2E: setup failed: {e}"));
        }
    }
    if spec["heygen_install"].as_bool() == Some(true) {
        match videokit::heygen_install(app.clone()).await {
            Ok(s) => log(format!("E2E: HeyGen installed={} signed_in={} ({})", s.installed, s.signed_in, s.message)),
            Err(e) => log(format!("E2E: HeyGen install failed: {e}")),
        }
    }
    let ws = match crate::workspaces::create_project(app.clone(), text("title", "Test video")) {
        Ok(w) => w,
        Err(e) => return log(format!("E2E: project failed: {e}")),
    };
    let folder = match crate::workspaces::project_new_site(app.clone(), ws.slug.clone(), text("title", "Test video")) {
        Ok(f) => f,
        Err(e) => return log(format!("E2E: folder failed: {e}")),
    };
    if let Err(e) = videokit::video_new(app.clone(), folder.clone(), text("title", "Test video"), Some(text("format", "landscape")), spec["seconds"].as_u64().map(|s| s as u32)) {
        return log(format!("E2E: video_new failed: {e}"));
    }
    log(format!("E2E: folder {folder}"));
    // Footage or audio the person "gave": copied into the folder before the plan.
    for f in spec["files"].as_array().into_iter().flatten() {
        let (from, to) = (f["from"].as_str().unwrap_or(""), f["to"].as_str().unwrap_or(""));
        let dest = Path::new(&folder).join(to);
        if clean_rel(to) {
            let _ = std::fs::create_dir_all(dest.parent().unwrap_or(Path::new(&folder)));
            log(format!("E2E: given file {to}: {:?}", std::fs::copy(from, &dest).map(|n| n)));
        }
    }
    // Resolves when this video reaches "done" (after the optional change) or "error".
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let change = spec["edit"].as_str().map(String::from);
    let (handle, edit_folder, log2) = (app.clone(), folder.clone(), log.clone());
    let asked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let id = app.listen("video-stage", move |e| {
        let v: Value = serde_json::from_str(e.payload()).unwrap_or(Value::Null);
        if v["folder"] != json!(edit_folder) {
            return;
        }
        match v["stage"].as_str() {
            Some("error") => {
                let _ = tx.send("error".into());
            }
            Some("done") => match &change {
                Some(change) if !asked.swap(true, std::sync::atomic::Ordering::SeqCst) => {
                    let (handle, folder, change, log) = (handle.clone(), edit_folder.clone(), change.clone(), log2.clone());
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(3)).await;
                        log(format!("E2E: asking for a change: {change}"));
                        if let Err(e) = video_edit(handle, folder, change, None, None).await {
                            log(format!("E2E: video_edit failed: {e}"));
                        }
                    });
                }
                _ => {
                    let _ = tx.send("done".into());
                }
            },
            _ => {}
        }
    });
    if let Err(e) = video_make(app.clone(), folder.clone(), text("title", "Test video"), text("brief", ""), Some(text("workflow", "")), None, Some(true)).await {
        log(format!("E2E: video_make failed: {e}"));
    }
    let mut rx = rx;
    let outcome = tokio::time::timeout(Duration::from_secs(40 * 60), rx.recv()).await;
    log(format!("E2E: \"{}\" ended: {:?}", text("title", "Test video"), outcome.ok().flatten()));
    app.unlisten(id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_read_and_kept_small() {
        let r = parse_requests(r#"{"blocks":["flash-through-white"],"media":[{"id":"music","type":"bgm","intent":"warm acoustic"}],"images":[{"id":"sheet1","prompt":"five breads","sheet":true}],"voiceover":{"text":"Hello.","style":"warm"}}"#).unwrap();
        assert_eq!((r.blocks.len(), r.media[0].kind.as_str(), r.images[0].sheet), (1, "bgm", true));
        assert_eq!(r.voiceover.unwrap().text, "Hello.");
        assert!(parse_requests("{}").unwrap().media.is_empty(), "every key is optional");
        assert!(parse_requests("not json").is_err());
        let many = format!("{{\"blocks\":[{}]}}", (0..25).map(|i| format!("\"b{i}\"")).collect::<Vec<_>>().join(","));
        assert!(parse_requests(&many).is_err());
    }

    #[test]
    fn names_from_a_worker_are_never_trusted_as_paths() {
        assert_eq!(clean_id("hero_1"), Some("hero_1".into()));
        assert_eq!(clean_id("../../etc/passwd"), None);
        assert_eq!(clean_id("a b"), None);
        assert_eq!(clean_id(""), None);
        assert_eq!(clean_block("flash-through-white"), Some("flash-through-white".into()));
        assert_eq!(clean_block("Flash; rm -rf /"), None);
        assert_eq!(clean_block("--force"), Some("--force".into()), "dashes are allowed in names");
    }

    #[test]
    fn caption_timings_follow_the_narration_length() {
        let w = estimate_words("Warm bread, every morning. Baked at four.", 6.0);
        assert_eq!(w.len(), 7);
        assert_eq!(w[0]["text"], "Warm");
        assert_eq!(w[0]["start"], 0.0);
        let last_end = w[6]["end"].as_f64().unwrap();
        assert!(last_end > 4.8 && last_end <= 6.0, "{last_end}");
        let starts: Vec<f64> = w.iter().map(|x| x["start"].as_f64().unwrap()).collect();
        assert!(starts.windows(2).all(|p| p[1] > p[0]), "words come in order");
        assert!(w[2]["start"].as_f64().unwrap() - w[1]["end"].as_f64().unwrap() > 0.05, "a pause follows a comma");
        assert!(estimate_words("", 5.0).is_empty() && estimate_words("hi", 0.0).is_empty());
    }

    #[test]
    fn heard_words_take_the_scripts_spelling_when_they_line_up() {
        let heard = vec![json!({"text": "Lacamari", "start": 0.3, "end": 0.9}), json!({"text": "brings", "start": 1.0, "end": 1.4})];
        let aligned = align_words("Lakhamari brings", heard.clone());
        assert_eq!((aligned[0]["text"].as_str(), aligned[0]["start"].as_f64()), (Some("Lakhamari"), Some(0.3)));
        assert_eq!(align_words("Lakhamari brings a little crunch", heard.clone()).len(), 2, "a different count keeps what was heard");
        assert_eq!(align_words("Lakhamari brings a little crunch", heard)[0]["text"], "Lacamari");
    }

    #[test]
    fn a_video_grows_to_fit_its_narration() {
        let dir = std::env::temp_dir().join(format!("jarvis-fit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let meta = videokit::VideoMeta { title: "t".into(), format: "portrait".into(), width: 1080, height: 1920, seconds: 20 };
        std::fs::write(dir.join("index.html"), "<div id=\"root\" data-composition-id=\"main\" data-start=\"0\" data-duration=\"20\" data-width=\"1080\">").unwrap();
        let voice = |secs: f64| Item { id: "voiceover".into(), kind: "voiceover".into(), ok: true, duration: Some(secs), ..Default::default() };
        let (same, note) = fit_length(&dir, &meta, &[voice(18.0)]);
        assert_eq!((same.seconds, note.as_str()), (20, ""), "a narration that fits changes nothing");
        assert_eq!(fit_length(&dir, &meta, &[]).0.seconds, 20, "no narration, no change");
        let (longer, note) = fit_length(&dir, &meta, &[voice(22.76)]);
        assert_eq!(longer.seconds, 24, "22.76s of speech plus a breath, rounded up");
        assert!(note.contains("22.8 seconds") && note.contains("24 seconds"));
        assert!(std::fs::read_to_string(dir.join("index.html")).unwrap().contains("data-duration=\"24\""));
        assert!(std::fs::read_to_string(dir.join("video.json")).unwrap().contains("\"seconds\": 24"));
        assert_eq!(fit_length(&dir, &meta, &[voice(400.0)]).0.seconds, 180, "never longer than three minutes");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_worker_gets_the_core_skills_and_the_workflow_that_fits() {
        let s = skills_for("slideshow");
        assert_eq!(s[0], "video-design");
        assert!(s.contains(&"media-use".to_string()) && s.contains(&"hyperframes-registry".to_string()) && s.last().unwrap() == "slideshow");
        assert_eq!(skills_for("rm -rf").len(), CORE_SKILLS.len(), "an unknown workflow adds nothing");
        let meta = videokit::VideoMeta { title: "t".into(), format: "portrait".into(), width: 1080, height: 1920, seconds: 30 };
        let plan = plan_request("Bakery reel", "A reel for a bakery", &meta, "product-launch-video", "");
        assert!(plan.contains("PLAN") && plan.contains("1080x1920") && plan.contains("30 seconds") && plan.contains("requests.json") && plan.contains("know-how/product-launch-video/SKILL.md"));
        assert!(!plan.contains("asked for changes"));
        assert!(plan_request("t", "b", &meta, "", "make it shorter").contains("make it shorter"));
        let compose = compose_request(&meta, "All 3 items are ready.", "");
        assert!(compose.contains("COMPOSE") && compose.contains("assets/manifest.json") && compose.contains("./hf lint") && compose.contains("Do not render"));
        assert!(fix_request(&["✗ gsap_x: bad".into()]).contains("- ✗ gsap_x: bad"));
    }
}
