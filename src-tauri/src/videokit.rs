//! Video kit: making videos with HeyGen's open-source HyperFrames (Apache 2.0). A video is an HTML page
//! with a GSAP timeline; HyperFrames steps through it frame by frame in headless Chrome and encodes
//! an MP4 with FFmpeg.
//!
//! Nobody has to install anything. `video_setup` puts a private copy of everything in
//! `~/Jarvis/.video-runtime/`: Node 22 (checked against nodejs.org's published checksum), HyperFrames,
//! FFmpeg, FFprobe and GSAP. Chrome, Edge or Brave is reused if it's installed, otherwise HyperFrames
//! fetches its own. Every tool is pinned through environment variables, so the user's own Node,
//! FFmpeg and shell setup are never used or changed. Jarvis never runs `hyperframes init`, which
//! writes agent skills into the user's home folder; it writes the project scaffold itself.

use crate::preview;
use crate::settings;
use crate::tasks;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

const NODE_INDEX: &str = "https://nodejs.org/dist/latest-v22.x";
/// What was tested together. HyperFrames changes quickly, so the versions are pinned.
const PINNED: [&str; 4] = ["hyperframes@0.8.112", "ffmpeg-static@5.3.0", "@ffprobe-installer/ffprobe@2.1.2", "gsap@3.15.0"];
/// HyperFrames' 21 agent skills, its catalog index and licence (see scripts/build-hyperframes-skills.mjs).
const SKILLS_ARCHIVE: &[u8] = include_bytes!("../resources/hyperframes-skills.tar.gz");
/// Raise when the archive changes, so the unpacked copy is replaced.
const SKILLS_VERSION: u32 = 1;
const HEYGEN_CDN: &str = "https://static.heygen.ai/cli";

// ---------- where things live ----------

/// The private toolbox. Everything is a path under `base`, so tests can point it anywhere.
#[derive(Clone)]
pub struct Runtime {
    pub base: PathBuf,
}

/// (os, arch) as Node and the FFprobe package name them, or why this computer isn't supported yet.
fn platform() -> Result<(&'static str, &'static str), String> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        other => return Err(format!("Making videos isn't available on {other} yet.")),
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => return Err(format!("Making videos isn't available on {other} processors yet.")),
    };
    Ok((os, arch))
}

impl Runtime {
    pub fn for_app(app: &AppHandle) -> Runtime {
        // Developer builds can point the toolbox somewhere else, to test setup without touching ~/Jarvis.
        if cfg!(debug_assertions) {
            if let Some(p) = std::env::var_os("JARVIS_VIDEO_RT").filter(|p| !p.is_empty()) {
                return Runtime { base: PathBuf::from(p) };
            }
        }
        Runtime { base: settings::home(app).join("Jarvis").join(".video-runtime") }
    }
    fn node_dir(&self) -> PathBuf {
        self.base.join("node")
    }
    fn node(&self) -> PathBuf {
        self.node_dir().join("bin").join("node")
    }
    fn npm_cli(&self) -> PathBuf {
        self.node_dir().join("lib").join("node_modules").join("npm").join("bin").join("npm-cli.js")
    }
    fn pkg(&self) -> PathBuf {
        self.base.join("pkg")
    }
    fn module(&self, rel: &str) -> PathBuf {
        self.pkg().join("node_modules").join(rel)
    }
    fn hf_cli(&self) -> PathBuf {
        self.module("hyperframes/bin/hyperframes.mjs")
    }
    pub(crate) fn ffmpeg(&self) -> PathBuf {
        self.module("ffmpeg-static/ffmpeg")
    }
    fn ffprobe(&self) -> PathBuf {
        let (os, arch) = platform().unwrap_or(("darwin", "arm64"));
        self.module(&format!("@ffprobe-installer/{os}-{arch}/ffprobe"))
    }
    pub fn gsap(&self) -> PathBuf {
        self.module("gsap/dist/gsap.min.js")
    }
    fn browser_note(&self) -> PathBuf {
        self.base.join("browser.txt")
    }
    /// A home folder of the toolbox's own, so its tools (HyperFrames, HeyGen) keep their settings and
    /// sign-ins here and never read or replace the user's.
    fn home(&self) -> PathBuf {
        self.base.join("home")
    }
    /// Where HyperFrames keeps the speech model for exact caption timing (installed on request).
    fn captions_model(&self) -> PathBuf {
        self.home().join(".cache").join("hyperframes").join("parakeet")
    }
    pub fn captions_ready(&self) -> bool {
        std::fs::read_dir(self.captions_model()).map(|mut d| d.next().is_some()).unwrap_or(false)
    }
    fn bin_dir(&self) -> PathBuf {
        self.base.join("bin")
    }
    fn heygen(&self) -> PathBuf {
        self.bin_dir().join("heygen")
    }
    /// HyperFrames' skills, unpacked from the app.
    pub fn content(&self) -> PathBuf {
        self.base.join("hyperframes")
    }
    pub fn skills(&self) -> PathBuf {
        self.content().join("skills")
    }

    pub fn node_path(&self) -> PathBuf {
        self.node()
    }
    pub fn env_pairs(&self) -> Vec<(String, String)> {
        self.env()
    }
    /// The tools, the skills and a browser are all there.
    pub fn ready(&self) -> bool {
        status_of(self).ready
    }

    /// Name the packaged FFmpeg and FFprobe `ffmpeg` and `ffprobe` in the toolbox's bin/ folder: some of
    /// the tools look them up by name on the PATH.
    pub fn link_tools(&self) {
        let _ = std::fs::create_dir_all(self.bin_dir());
        for (name, target) in [("ffmpeg", self.ffmpeg()), ("ffprobe", self.ffprobe())] {
            let link = self.bin_dir().join(name);
            if target.is_file() && link.read_link().ok().as_deref() != Some(target.as_path()) {
                let _ = std::fs::remove_file(&link);
                #[cfg(unix)]
                let _ = std::os::unix::fs::symlink(&target, &link);
            }
        }
    }

    /// Everything but the browser is in place.
    fn installed(&self) -> bool {
        [self.node(), self.hf_cli(), self.ffmpeg(), self.ffprobe(), self.gsap()].iter().all(|p| p.is_file())
    }

    /// The browser to render with: one already on this computer, else the one HyperFrames fetched.
    fn browser(&self) -> Option<PathBuf> {
        tasks::find_chromium().or_else(|| std::fs::read_to_string(self.browser_note()).ok().map(|s| PathBuf::from(s.trim())).filter(|p| p.is_file()))
    }

    /// The environment every HyperFrames command runs in: only these tools, nothing from the user's shell.
    fn env(&self) -> Vec<(String, String)> {
        let mut env = vec![
            ("PATH".to_string(), format!("{}:{}:/usr/bin:/bin:/usr/sbin:/sbin", self.node_dir().join("bin").display(), self.bin_dir().display())),
            ("HOME".into(), self.home().display().to_string()),
            ("HYPERFRAMES_FFMPEG_PATH".into(), self.ffmpeg().display().to_string()),
            ("HYPERFRAMES_FFPROBE_PATH".into(), self.ffprobe().display().to_string()),
            ("HYPERFRAMES_NO_TELEMETRY".into(), "1".into()),
            ("HYPERFRAMES_NO_UPDATE_CHECK".into(), "1".into()),
            ("HYPERFRAMES_SKIP_SKILLS".into(), "1".into()),
            ("HEYGEN_NO_ANALYTICS".into(), "1".into()),
        ];
        if let Some(b) = self.browser() {
            env.push(("HYPERFRAMES_BROWSER_PATH".into(), b.display().to_string()));
        }
        env
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoStatus {
    pub ready: bool,
    /// The speech model for exact caption timing is installed (optional).
    pub captions: bool,
    /// Node, HyperFrames, FFmpeg and FFprobe are installed.
    pub tools: bool,
    pub browser: String,
    pub message: String,
}

fn status_of(rt: &Runtime) -> VideoStatus {
    if let Err(e) = platform() {
        return VideoStatus { ready: false, captions: false, tools: false, browser: String::new(), message: e };
    }
    let tools = rt.installed() && rt.skills().is_dir();
    let browser = rt.browser();
    let message = match (tools, &browser) {
        (true, Some(_)) => "Ready".to_string(),
        (true, None) => "Almost there: Jarvis still needs a browser to draw the frames.".into(),
        _ => "Not set up yet. It's a one-time download of about 300 MB.".into(),
    };
    VideoStatus { ready: tools && browser.is_some(), captions: rt.captions_ready(), tools, browser: browser.map(|b| b.display().to_string()).unwrap_or_default(), message }
}

#[tauri::command]
pub fn video_status(app: AppHandle) -> VideoStatus {
    status_of(&Runtime::for_app(&app))
}

// ---------- setting up ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SetupEvent {
    step: &'static str,
    message: String,
    percent: Option<u8>,
}

static SETTING_UP: AtomicBool = AtomicBool::new(false);

fn say(app: &AppHandle, step: &'static str, message: impl Into<String>, percent: Option<u8>) {
    let _ = app.emit("video-setup", SetupEvent { step, message: message.into(), percent });
}

/// The file name and checksum Node publishes for this platform, from its SHASUMS256.txt.
fn node_archive(shasums: &str, os: &str, arch: &str) -> Option<(String, String)> {
    let suffix = format!("-{os}-{arch}.tar.gz");
    shasums.lines().find_map(|l| {
        let (sum, name) = l.split_once(char::is_whitespace)?;
        let name = name.trim();
        (name.starts_with("node-v") && name.ends_with(&suffix) && sum.len() == 64).then(|| (name.to_string(), sum.to_string()))
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Download `url` to `to`, reporting progress, and return the file's SHA-256.
async fn download(url: &str, to: &Path, mut progress: impl FnMut(u64, Option<u64>)) -> Result<String, String> {
    let mut resp = reqwest::get(url).await.map_err(|e| format!("Couldn't download it: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("The download failed ({}).", resp.status()));
    }
    let total = resp.content_length();
    let mut file = tokio::fs::File::create(to).await.map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut done = 0u64;
    while let Some(chunk) = resp.chunk().await.map_err(|e| format!("The download was interrupted: {e}"))? {
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        done += chunk.len() as u64;
        progress(done, total);
    }
    file.flush().await.map_err(|e| e.to_string())?;
    Ok(hex(&hasher.finalize()))
}

async fn install_node(app: &AppHandle, rt: &Runtime) -> Result<(), String> {
    if rt.node().is_file() && rt.npm_cli().is_file() {
        return Ok(());
    }
    let (os, arch) = platform()?;
    say(app, "node", "Looking up the latest Node 22…", None);
    let sums = reqwest::get(format!("{NODE_INDEX}/SHASUMS256.txt")).await.map_err(|e| format!("Couldn't reach nodejs.org: {e}"))?.text().await.map_err(|e| e.to_string())?;
    let (name, expected) = node_archive(&sums, os, arch).ok_or("nodejs.org doesn't list a build for this computer.")?;
    std::fs::create_dir_all(&rt.base).map_err(|e| e.to_string())?;
    let archive = rt.base.join(&name);
    let mut last: Option<u8> = None;
    let sum = download(&format!("{NODE_INDEX}/{name}"), &archive, |done, total| {
        let pct = total.map(|t| (done * 100 / t.max(1)) as u8);
        // One message per percent, not one per chunk.
        if pct != last {
            last = pct;
            say(app, "node", format!("Downloading Node ({} MB)…", done / 1_048_576), pct);
        }
    })
    .await?;
    if sum != expected {
        let _ = std::fs::remove_file(&archive);
        return Err("The Node download didn't match its published checksum, so it was thrown away. Try again.".into());
    }
    say(app, "node", "Unpacking Node…", None);
    let staging = rt.base.join("node-staging");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let out = Command::new("tar").arg("-xzf").arg(&archive).arg("-C").arg(&staging).output().await.map_err(|e| format!("Couldn't unpack Node: {e}"))?;
    if !out.status.success() {
        return Err(format!("Couldn't unpack Node: {}", tasks::truncate(&String::from_utf8_lossy(&out.stderr), 160)));
    }
    let unpacked = std::fs::read_dir(&staging).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).find(|p| p.is_dir()).ok_or("The Node download was empty.")?;
    let _ = std::fs::remove_dir_all(rt.node_dir());
    std::fs::rename(&unpacked, rt.node_dir()).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_file(&archive);
    // Only `node` and `npm` are used: the rest is a third of the size.
    for rel in ["include", "share", "lib/node_modules/corepack", "bin/corepack"] {
        let p = rt.node_dir().join(rel);
        let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
    }
    Ok(())
}

async fn install_packages(app: &AppHandle, rt: &Runtime) -> Result<(), String> {
    if rt.hf_cli().is_file() && rt.ffmpeg().is_file() && rt.ffprobe().is_file() && rt.gsap().is_file() {
        return Ok(());
    }
    say(app, "packages", "Installing the video tools (HyperFrames, FFmpeg)… this is the slow part.", None);
    std::fs::create_dir_all(rt.pkg()).map_err(|e| e.to_string())?;
    let manifest = rt.pkg().join("package.json");
    if !manifest.exists() {
        std::fs::write(&manifest, "{\"name\":\"jarvis-video-runtime\",\"private\":true}").map_err(|e| e.to_string())?;
    }
    let mut cmd = Command::new(rt.node());
    cmd.arg(rt.npm_cli())
        .args(["install", "--no-fund", "--no-audit", "--loglevel=error", "--prefix"])
        .arg(rt.pkg())
        .args(PINNED)
        .current_dir(&rt.base)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", rt.node_dir().join("bin").display()))
        .env("HOME", rt.home())
        .env("npm_config_cache", rt.base.join("npm-cache"))
        .env("npm_config_update_notifier", "false")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let out = tokio::time::timeout(Duration::from_secs(20 * 60), cmd.output())
        .await
        .map_err(|_| "Installing the video tools took longer than 20 minutes and was stopped.".to_string())?
        .map_err(|e| format!("Couldn't run the installer: {e}"))?;
    if !out.status.success() || !rt.installed() {
        let text = format!("{}{}", String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout));
        return Err(format!("Couldn't install the video tools: {}", tasks::truncate(&text, 240)));
    }
    rt.link_tools();
    Ok(())
}

async fn ensure_browser(app: &AppHandle, rt: &Runtime) -> Result<(), String> {
    if rt.browser().is_some() {
        return Ok(());
    }
    say(app, "browser", "Downloading a browser to draw the frames…", None);
    let (ok, text) = run_hf(rt, &rt.base, &["browser", "ensure"], |_| {}).await?;
    if !ok {
        return Err(format!("Couldn't get a browser: {}", tasks::truncate(&text, 200)));
    }
    let (_, path) = run_hf(rt, &rt.base, &["browser", "path"], |_| {}).await?;
    let found = path.lines().map(str::trim).rev().find(|l| Path::new(l).is_file()).map(String::from);
    match found {
        Some(p) => std::fs::write(rt.browser_note(), p).map_err(|e| e.to_string()),
        None => Err("The browser was downloaded but couldn't be found.".into()),
    }
}

/// Optional: download the speech model (about 700 MB) so captions are timed to the exact words spoken.
#[tauri::command]
pub async fn captions_install(app: AppHandle) -> Result<VideoStatus, String> {
    let rt = Runtime::for_app(&app);
    if !rt.installed() {
        return Err("Set up the video tools first.".into());
    }
    say(&app, "captions", "Downloading the speech model (about 700 MB)…", None);
    let (ok, text) = tokio::time::timeout(Duration::from_secs(20 * 60), run_hf(&rt, &rt.base, &["models", "install", "parakeet", "--json"], |_| {}))
        .await
        .map_err(|_| "The download took longer than 20 minutes and was stopped.".to_string())??;
    let status = status_of(&rt);
    say(&app, "done", if ok { "Captions will be timed to the spoken words.".to_string() } else { tasks::truncate(&text, 200) }, Some(100));
    if ok && status.captions {
        Ok(status)
    } else {
        Err(format!("The speech model couldn't be installed: {}", tasks::truncate(&text, 200)))
    }
}

/// The words of a spoken file with their start and end times, if the speech model is installed.
pub(crate) async fn transcribe_words(rt: &Runtime, dir: &Path, audio_rel: &str) -> Option<Vec<serde_json::Value>> {
    if !rt.captions_ready() {
        return None;
    }
    let (ok, text) = tokio::time::timeout(Duration::from_secs(180), run_hf(rt, dir, &["transcribe", audio_rel, "--json"], |_| {})).await.ok()?.ok()?;
    if !ok {
        return None;
    }
    let line = text.lines().rev().find(|l| l.trim_start().starts_with('{'))?;
    let meta: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let path = meta["transcriptPath"].as_str()?;
    let words: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let words: Vec<serde_json::Value> = words.into_iter().filter(|w| w["text"].is_string() && w["start"].is_number() && w["end"].is_number()).map(|w| serde_json::json!({ "text": w["text"], "start": w["start"], "end": w["end"] })).collect();
    (!words.is_empty()).then_some(words)
}

/// One-time setup. Safe to press again: whatever is already there is kept.
#[tauri::command]
pub async fn video_setup(app: AppHandle) -> Result<VideoStatus, String> {
    platform()?;
    if SETTING_UP.swap(true, Ordering::SeqCst) {
        return Err("Setting up is already running.".into());
    }
    let rt = Runtime::for_app(&app);
    let result = async {
        install_node(&app, &rt).await?;
        install_packages(&app, &rt).await?;
        install_content(&app, &rt).await?;
        ensure_browser(&app, &rt).await?;
        say(&app, "check", "Checking that everything works…", None);
        let probe = Command::new(rt.ffmpeg()).arg("-version").stdin(Stdio::null()).output().await.map_err(|e| format!("FFmpeg won't start: {e}"))?;
        if !probe.status.success() {
            return Err("FFmpeg was installed but won't run on this computer.".to_string());
        }
        Ok(())
    }
    .await;
    SETTING_UP.store(false, Ordering::SeqCst);
    let status = status_of(&rt);
    say(&app, "done", if result.is_ok() { status.message.clone() } else { result.clone().err().unwrap_or_default() }, Some(100));
    result.map(|_| status)
}

// ---------- running HyperFrames ----------

/// Terminal colour codes in the CLI's output would show up as junk in the UI.
fn plain(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            while let Some(&n) = chars.peek() {
                chars.next();
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// "  ████░░  70%  Capturing frame 178/180 (5 workers)" → (70, "Capturing frame 178/180 (5 workers)").
fn parse_progress(line: &str) -> Option<(u8, String)> {
    // Only the progress bar's own lines count: log lines mention percentages too.
    if !line.contains('█') && !line.contains('░') {
        return None;
    }
    let at = line.find('%')?;
    let digits: String = line[..at].chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
    let pct = digits.parse::<u8>().ok().filter(|p| *p <= 100)?;
    Some((pct, line[at + 1..].trim().to_string()))
}

static CURRENT_PID: Mutex<Option<u32>> = Mutex::new(None);

/// Run one HyperFrames command in `dir` with the private toolbox. Calls `on_line` for each line of
/// output (colours removed) and returns whether it succeeded plus the output.
async fn run_hf(rt: &Runtime, dir: &Path, args: &[&str], mut on_line: impl FnMut(&str)) -> Result<(bool, String), String> {
    if !rt.installed() {
        return Err("The video tools aren't set up yet. Open Set up and press Set up video.".into());
    }
    let _ = std::fs::create_dir_all(rt.home());
    rt.link_tools();
    let mut cmd = Command::new(rt.node());
    cmd.arg(rt.hf_cli())
        .args(args)
        .current_dir(dir)
        .env_clear()
        .envs(rt.env())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start HyperFrames: {e}"))?;
    *CURRENT_PID.lock().unwrap() = child.id();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    for pipe in [child.stdout.take().map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Unpin + Send>), child.stderr.take().map(|p| Box::new(p) as _)].into_iter().flatten() {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(pipe).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                let _ = tx.send(l);
            }
        });
    }
    drop(tx);
    let mut text = String::new();
    while let Some(line) = rx.recv().await {
        let line = plain(&line);
        on_line(&line);
        // The renderer's own trace lines are noise to a person.
        if !line.contains("[Render:trace]") && text.len() < 40_000 {
            text.push_str(&line);
            text.push('\n');
        }
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    *CURRENT_PID.lock().unwrap() = None;
    Ok((status.success(), text))
}

/// Unpack HyperFrames' skills (bundled in the app) next to the tools. Skipped when this version is there.
async fn install_content(app: &AppHandle, rt: &Runtime) -> Result<(), String> {
    let marker = rt.content().join(".version");
    if std::fs::read_to_string(&marker).ok().and_then(|v| v.trim().parse::<u32>().ok()) == Some(SKILLS_VERSION) && rt.skills().is_dir() {
        return Ok(());
    }
    say(app, "skills", "Unpacking the video skills…", None);
    std::fs::create_dir_all(&rt.base).map_err(|e| e.to_string())?;
    let tmp = rt.base.join("skills.tar.gz");
    std::fs::write(&tmp, SKILLS_ARCHIVE).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(rt.content());
    let out = Command::new("tar").arg("-xzf").arg(&tmp).arg("-C").arg(&rt.base).output().await.map_err(|e| format!("Couldn't unpack the skills: {e}"))?;
    let _ = std::fs::remove_file(&tmp);
    if !out.status.success() || !rt.skills().is_dir() {
        return Err(format!("Couldn't unpack the skills: {}", tasks::truncate(&String::from_utf8_lossy(&out.stderr), 160)));
    }
    std::fs::write(&marker, SKILLS_VERSION.to_string()).map_err(|e| e.to_string())
}

/// Where a skill lives, for handing it to a worker: the unpacked HyperFrames skills.
pub(crate) fn skill_path(app: &AppHandle, slug: &str) -> Option<PathBuf> {
    if slug.is_empty() || slug.contains('/') || slug.contains("..") {
        return None;
    }
    let p = Runtime::for_app(app).skills().join(slug);
    p.join("SKILL.md").is_file().then_some(p)
}

/// Make sure the skills are unpacked (a project made before they were, or after an app update).
pub(crate) async fn ensure_content(app: &AppHandle) -> Result<(), String> {
    install_content(app, &Runtime::for_app(app)).await
}

// ---------- HeyGen: music, sound effects, photos, icons and voice catalogs ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeygenStatus {
    pub installed: bool,
    pub signed_in: bool,
    pub message: String,
}

async fn heygen_run(rt: &Runtime, args: &[&str], extra_env: &[(&str, &str)], timeout: Duration) -> Result<(bool, String), String> {
    let _ = std::fs::create_dir_all(rt.home());
    let mut cmd = Command::new(rt.heygen());
    cmd.args(args).env_clear().envs(rt.env()).envs(extra_env.iter().copied()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let out = tokio::time::timeout(timeout, cmd.output()).await.map_err(|_| "HeyGen took too long to answer.".to_string())?.map_err(|e| format!("Couldn't start HeyGen: {e}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    Ok((out.status.success(), plain(&text)))
}

async fn heygen_status_of(rt: &Runtime) -> HeygenStatus {
    if !rt.heygen().is_file() {
        return HeygenStatus { installed: false, signed_in: false, message: "Not set up. Music, photos, icons and voices from HeyGen's free catalog need it.".into() };
    }
    match heygen_run(rt, &["auth", "status"], &[], Duration::from_secs(20)).await {
        Ok((true, text)) if !text.contains("\"error\"") => HeygenStatus { installed: true, signed_in: true, message: "Connected".into() },
        Ok(_) => HeygenStatus { installed: true, signed_in: false, message: "Installed. Sign in to HeyGen (free) to use its music, photo, icon and voice catalogs.".into() },
        Err(e) => HeygenStatus { installed: true, signed_in: false, message: e },
    }
}

#[tauri::command]
pub async fn heygen_status(app: AppHandle) -> HeygenStatus {
    heygen_status_of(&Runtime::for_app(&app)).await
}

/// Download HeyGen's command line tool into the toolbox, checking its published checksum, the same
/// steps as HeyGen's own installer script (which would put it in ~/.local/bin).
#[tauri::command]
pub async fn heygen_install(app: AppHandle) -> Result<HeygenStatus, String> {
    let rt = Runtime::for_app(&app);
    if !rt.heygen().is_file() {
        let (os, arch) = platform()?;
        let arch = if arch == "x64" { "amd64" } else { arch };
        say(&app, "heygen", "Looking up the latest HeyGen release…", None);
        let tag = reqwest::get(format!("{HEYGEN_CDN}/stable")).await.map_err(|e| format!("Couldn't reach HeyGen: {e}"))?.text().await.map_err(|e| e.to_string())?.trim().to_string();
        if !tag.starts_with('v') || tag.len() > 20 || !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == 'v' || c == '-') {
            return Err("HeyGen returned a release name Jarvis doesn't recognise.".into());
        }
        let asset = format!("heygen_{tag}_{os}_{arch}.tar.gz");
        let sums = reqwest::get(format!("{HEYGEN_CDN}/releases/{tag}/checksums.txt")).await.map_err(|e| e.to_string())?.text().await.map_err(|e| e.to_string())?;
        let expected = sums.lines().find_map(|l| l.trim().strip_suffix(asset.as_str()).map(|h| h.trim().to_string())).filter(|h| h.len() == 64).ok_or("HeyGen doesn't publish a build for this computer.")?;
        std::fs::create_dir_all(rt.bin_dir()).map_err(|e| e.to_string())?;
        let archive = rt.base.join(&asset);
        let mut last: Option<u8> = None;
        let sum = download(&format!("{HEYGEN_CDN}/releases/{tag}/{asset}"), &archive, |done, total| {
            let pct = total.map(|t| (done * 100 / t.max(1)) as u8);
            if pct != last {
                last = pct;
                say(&app, "heygen", format!("Downloading HeyGen ({} MB)…", done / 1_048_576), pct);
            }
        })
        .await?;
        if sum != expected {
            let _ = std::fs::remove_file(&archive);
            return Err("The HeyGen download didn't match its published checksum, so it was thrown away. Try again.".into());
        }
        let out = Command::new("tar").arg("-xzf").arg(&archive).arg("-C").arg(rt.bin_dir()).arg("heygen").output().await.map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&archive);
        if !out.status.success() || !rt.heygen().is_file() {
            return Err("Couldn't unpack HeyGen.".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(rt.heygen(), std::fs::Permissions::from_mode(0o755));
        }
    }
    Ok(heygen_status_of(&rt).await)
}

/// Sign in to HeyGen in the browser (free). The sign-in page's address goes to the app as `heygen-login`
/// in case the browser doesn't open by itself. Returns when the person has finished or ten minutes pass.
#[tauri::command]
pub async fn heygen_connect(app: AppHandle) -> Result<HeygenStatus, String> {
    let rt = Runtime::for_app(&app);
    if !rt.heygen().is_file() {
        heygen_install(app.clone()).await?;
    }
    let _ = std::fs::create_dir_all(rt.home());
    let mut cmd = Command::new(rt.heygen());
    cmd.args(["auth", "login", "--oauth"]).env_clear().envs(rt.env()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start HeyGen: {e}"))?;
    for pipe in [child.stdout.take().map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Unpin + Send>), child.stderr.take().map(|p| Box::new(p) as _)].into_iter().flatten() {
        let app = app.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(pipe).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                if let Some(url) = plain(&l).split_whitespace().find(|w| w.starts_with("https://")) {
                    let _ = app.emit("heygen-login", url.trim_end_matches(['.', ',']).to_string());
                }
            }
        });
    }
    let _ = tokio::time::timeout(Duration::from_secs(600), child.wait()).await;
    let _ = child.kill().await;
    Ok(heygen_status_of(&rt).await)
}

// ---------- the worker's helper ----------

/// A small script the worker can run as `./hf lint` (static checks only: the worker's sandbox can't start the
/// browser or a local server, so Jarvis itself runs `check`, snapshots and renders). It pins the same tools.
fn write_helper(rt: &Runtime, dir: &Path) -> Result<(), String> {
    let q = |p: &Path| format!("'{}'", p.display().to_string().replace('\'', "'\\''"));
    let mut script = String::from("#!/bin/sh\n# HyperFrames with Jarvis's own tools. Use `./hf lint` where the docs say `npx hyperframes lint`.\n");
    for (k, v) in rt.env() {
        script.push_str(&format!("export {k}={}\n", q(Path::new(&v))));
    }
    script.push_str(&format!("exec {} {} \"$@\"\n", q(&rt.node()), q(&rt.hf_cli())));
    let path = dir.join("hf");
    std::fs::write(&path, script).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Run a HyperFrames command for the pipeline: (succeeded, output).
pub(crate) async fn run_hf_public(rt: &Runtime, dir: &Path, args: &[&str]) -> Result<(bool, String), String> {
    run_hf(rt, dir, args, |_| {}).await
}

pub(crate) async fn check_project_public(rt: &Runtime, dir: &Path) -> Result<CheckReport, String> {
    check_project(rt, dir).await
}

pub(crate) async fn render_public(rt: &Runtime, dir: &Path, quality: &str, progress: impl FnMut(u8, String)) -> Result<RenderDone, String> {
    render_project(rt, dir, quality, progress).await
}

// ---------- projects ----------

#[derive(Serialize, serde::Deserialize, Clone, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct VideoMeta {
    pub title: String,
    /// "landscape", "portrait" or "square".
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub seconds: u32,
}

fn dimensions(format: &str) -> (u32, u32) {
    match format {
        "portrait" => (1080, 1920),
        "square" => (1080, 1080),
        _ => (1920, 1080),
    }
}

/// The starting file: an empty, correctly wired composition. GSAP is a local file, so a render never
/// needs the network.
fn scaffold(meta: &VideoMeta) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width={w}, height={h}" />
    <script src="vendor/gsap.min.js"></script>
    <style>
      * {{ margin: 0; padding: 0; box-sizing: border-box; }}
      html, body {{ margin: 0; width: {w}px; height: {h}px; overflow: hidden; background: #0a0a0a; }}
      #root {{ position: relative; width: 100%; height: 100%; overflow: hidden; font-family: system-ui, -apple-system, "Segoe UI", sans-serif; }}
    </style>
  </head>
  <body>
    <div id="root" data-composition-id="main" data-start="0" data-duration="{d}" data-width="{w}" data-height="{h}">
    </div>
    <script>
      const tl = gsap.timeline({{ paused: true }});
      window.__timelines["main"] = tl;
      tl.seek(0);
    </script>
  </body>
</html>
"#,
        w = meta.width,
        h = meta.height,
        d = meta.seconds
    )
}

/// Make a video project in `folder` (a project folder under the research folder): the composition, its
/// GSAP, and a `video.json` that says it's a video.
#[tauri::command]
pub fn video_new(app: AppHandle, folder: String, title: String, format: Option<String>, seconds: Option<u32>) -> Result<VideoMeta, String> {
    let rt = Runtime::for_app(&app);
    let (dir, _) = preview::project_dir(&app, &folder)?;
    new_project(&rt, &dir, &title, format.as_deref().unwrap_or("landscape"), seconds.unwrap_or(30))
}

fn new_project(rt: &Runtime, dir: &Path, title: &str, format: &str, seconds: u32) -> Result<VideoMeta, String> {
    if !rt.gsap().is_file() {
        return Err("The video tools aren't set up yet. Open Set up and press Set up video.".into());
    }
    let (width, height) = dimensions(format);
    let meta = VideoMeta { title: title.trim().to_string(), format: if ["portrait", "square"].contains(&format) { format.into() } else { "landscape".into() }, width, height, seconds: seconds.clamp(3, 180) };
    std::fs::create_dir_all(dir.join("vendor")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("assets")).map_err(|e| e.to_string())?;
    std::fs::copy(rt.gsap(), dir.join("vendor").join("gsap.min.js")).map_err(|e| e.to_string())?;
    if !dir.join("index.html").exists() {
        std::fs::write(dir.join("index.html"), scaffold(&meta)).map_err(|e| e.to_string())?;
    }
    std::fs::write(dir.join("video.json"), serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    write_helper(rt, dir)?;
    Ok(meta)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Render {
    /// Relative to the project folder.
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoInfo {
    pub is_video: bool,
    pub meta: VideoMeta,
    pub renders: Vec<Render>,
}

#[tauri::command]
pub fn video_info(app: AppHandle, folder: String) -> Result<VideoInfo, String> {
    let (dir, _) = preview::project_dir(&app, &folder)?;
    Ok(info_of(&dir))
}

fn info_of(dir: &Path) -> VideoInfo {
    let meta: Option<VideoMeta> = std::fs::read_to_string(dir.join("video.json")).ok().and_then(|s| serde_json::from_str(&s).ok());
    let mut renders: Vec<Render> = std::fs::read_dir(dir.join("renders"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "mp4" || x == "webm" || x == "gif"))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            let name = e.file_name().to_string_lossy().to_string();
            Some(Render { path: format!("renders/{name}"), name, size: m.len(), modified: m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64 })
        })
        .collect();
    renders.sort_by(|a, b| b.modified.cmp(&a.modified));
    VideoInfo { is_video: meta.is_some(), meta: meta.unwrap_or_default(), renders }
}

// ---------- checking and rendering ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckReport {
    pub ok: bool,
    /// The lines that name a problem, ready to give the worker.
    pub problems: Vec<String>,
    pub text: String,
}

/// What a person (or the worker) needs from `hyperframes check`: whether it passed, and each problem.
fn problems_in(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = vec![];
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        if t.starts_with('✗') || t.contains("Invalid HyperFrame contract") {
            // The line under a problem usually holds its "Fix:" advice.
            let mut item = t.to_string();
            for next in lines.iter().skip(i + 1).take(3) {
                let n = next.trim();
                if n.starts_with("Fix:") {
                    item.push(' ');
                    item.push_str(n);
                    break;
                }
                if n.starts_with('✗') {
                    break;
                }
            }
            if !out.contains(&item) {
                out.push(item);
            }
        }
    }
    out.truncate(12);
    out
}

#[tauri::command]
pub async fn video_check(app: AppHandle, folder: String) -> Result<CheckReport, String> {
    let rt = Runtime::for_app(&app);
    let (dir, _) = preview::project_dir(&app, &folder)?;
    check_project(&rt, &dir).await
}

async fn check_project(rt: &Runtime, dir: &Path) -> Result<CheckReport, String> {
    let (ok, text) = tokio::time::timeout(Duration::from_secs(180), run_hf(rt, dir, &["check"], |_| {}))
        .await
        .map_err(|_| "Checking the video took too long.".to_string())??;
    let problems = problems_in(&text);
    let never_ran = text.contains("Browser session never ran");
    Ok(CheckReport { ok: ok && problems.is_empty() && !never_ran, problems, text: tasks::truncate(&text, 6000) })
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RenderEvent {
    folder: String,
    percent: u8,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderDone {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub seconds_taken: u64,
}

static RENDERING: AtomicBool = AtomicBool::new(false);

/// Render the video. `quality` is "draft" (quick look) or "final". Progress goes out as `video-render`.
#[tauri::command]
pub async fn video_render(app: AppHandle, folder: String, quality: Option<String>) -> Result<RenderDone, String> {
    let rt = Runtime::for_app(&app);
    let (dir, _) = preview::project_dir(&app, &folder)?;
    if RENDERING.swap(true, Ordering::SeqCst) {
        return Err("Another video is being made right now. Wait for it to finish.".into());
    }
    let handle = app.clone();
    let key = folder.clone();
    let result = render_project(&rt, &dir, quality.as_deref().unwrap_or("draft"), move |percent, message| {
        let _ = handle.emit("video-render", RenderEvent { folder: key.clone(), percent, message });
    })
    .await;
    RENDERING.store(false, Ordering::SeqCst);
    result
}

async fn render_project(rt: &Runtime, dir: &Path, quality: &str, mut progress: impl FnMut(u8, String)) -> Result<RenderDone, String> {
    let (name, q) = if quality == "final" { ("final", "high") } else { ("draft", "draft") };
    std::fs::create_dir_all(dir.join("renders")).map_err(|e| e.to_string())?;
    let out = format!("renders/{name}.mp4");
    let _ = std::fs::remove_file(dir.join(&out));
    let started = std::time::Instant::now();
    let (ok, text) = run_hf(rt, dir, &["render", "--quality", q, "--output", &out], |line| {
        if let Some((pct, msg)) = parse_progress(line) {
            progress(pct, msg);
        }
    })
    .await?;
    let file = dir.join(&out);
    if !ok || !file.is_file() {
        let why = problems_in(&text).into_iter().next().unwrap_or_else(|| tasks::truncate(&text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("it stopped without a video"), 200));
        return Err(format!("The video couldn't be made: {why}"));
    }
    Ok(RenderDone { path: out, name: name.into(), size: file.metadata().map(|m| m.len()).unwrap_or(0), seconds_taken: started.elapsed().as_secs() })
}

/// Stop the render or check that is running.
#[tauri::command]
pub fn video_cancel() {
    if let Some(pid) = *CURRENT_PID.lock().unwrap() {
        let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_downloads_are_picked_from_the_published_list() {
        let sums = "aaaa  node-v22.1.0-darwin-x64.tar.gz\n\
                    23b25245dcfb9af7262f8ff142e9e2e0af025368117329e7a7458a51e5922f53  node-v22.23.3-darwin-arm64.tar.gz\n\
                    bbbb  node-v22.23.3-darwin-arm64.tar.xz\n\
                    23b25245dcfb9af7262f8ff142e9e2e0af025368117329e7a7458a51e5922f54  node-v22.23.3-linux-x64.tar.gz\n";
        let (name, sum) = node_archive(sums, "darwin", "arm64").unwrap();
        assert_eq!(name, "node-v22.23.3-darwin-arm64.tar.gz");
        assert_eq!(sum.len(), 64);
        assert!(node_archive(sums, "linux", "x64").unwrap().1.ends_with('4'));
        assert!(node_archive(sums, "darwin", "x64").is_none(), "a checksum that isn't 64 characters is not trusted");
        assert!(node_archive("", "darwin", "arm64").is_none());
        assert_eq!(hex(&Sha256::digest(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn progress_lines_are_read_for_the_percentage() {
        assert_eq!(parse_progress("  █████████████████░░░░░░░░  70%  Capturing frame 178/180 (5 workers)"), Some((70, "Capturing frame 178/180 (5 workers)".into())));
        assert_eq!(parse_progress("  █████████████████████████  100%  Render complete").map(|p| p.0), Some(100));
        assert_eq!(parse_progress("[INFO] cache hit 0%) (304ms)"), None, "log lines aren't progress");
        assert_eq!(parse_progress("rendered in 13.0s"), None);
        assert_eq!(parse_progress("  ██  250%  nonsense"), None);
        assert_eq!(plain("\u{1b}[32m✓\u{1b}[0m Chrome"), "✓ Chrome");
    }

    #[test]
    fn problems_come_with_their_fix() {
        let text = "Lint\n  ✗ gsap_non_transform_motion: GSAP tween on \"#title\" uses motion that snaps: letterSpacing.\n    /x/index.html #title t=0s\n    Fix: do not animate letterSpacing.\n  1 error(s)\nLayout\n  ✗ t=1.38-6s content_overlap #title inside #sub\n        Fix: Give each block its own zone.\n  ◇ fine\n";
        let p = problems_in(text);
        assert_eq!(p.len(), 2);
        assert!(p[0].starts_with("✗ gsap_non_transform_motion") && p[0].ends_with("Fix: do not animate letterSpacing."));
        assert!(p[1].contains("content_overlap") && p[1].contains("Fix: Give each block"));
        assert!(problems_in("Lint\n  ◇ 0 errors\n").is_empty());
    }

    #[test]
    fn a_new_project_is_a_wired_composition_with_local_gsap() {
        let base = std::env::temp_dir().join(format!("jarvis-video-test-{}", tasks::now_ms()));
        let rt = Runtime { base: base.join("rt") };
        std::fs::create_dir_all(rt.gsap().parent().unwrap()).unwrap();
        std::fs::write(rt.gsap(), "/* gsap */").unwrap();
        let dir = base.join("project");
        let meta = new_project(&rt, &dir, "Bakery reel", "portrait", 500).unwrap();
        assert_eq!((meta.width, meta.height, meta.seconds, meta.format.as_str()), (1080, 1920, 180, "portrait"));
        let html = std::fs::read_to_string(dir.join("index.html")).unwrap();
        assert!(html.contains("data-composition-id=\"main\"") && html.contains("data-width=\"1080\"") && html.contains("data-duration=\"180\""));
        assert!(html.contains("window.__timelines[\"main\"]") && html.contains("src=\"vendor/gsap.min.js\""), "no CDN: a render must work offline");
        assert!(!html.contains("http"), "{html}");
        assert_eq!(std::fs::read_to_string(dir.join("vendor/gsap.min.js")).unwrap(), "/* gsap */");
        // An existing composition is never overwritten.
        std::fs::write(dir.join("index.html"), "mine").unwrap();
        new_project(&rt, &dir, "Bakery reel", "square", 10).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("index.html")).unwrap(), "mine");
        let info = info_of(&dir);
        assert!(info.is_video && info.meta.format == "square" && info.renders.is_empty());
        assert!(!info_of(&base).is_video);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn every_tool_is_pinned_and_the_users_shell_is_not_used() {
        let rt = Runtime { base: PathBuf::from("/x/.video-runtime") };
        let env = rt.env();
        let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        assert_eq!(get("PATH").unwrap(), "/x/.video-runtime/node/bin:/x/.video-runtime/bin:/usr/bin:/bin:/usr/sbin:/sbin", "only the toolbox, then the system's own");
        assert_eq!(get("HOME").unwrap(), "/x/.video-runtime/home", "its own home folder: the user's settings and HeyGen sign-in are never read or replaced");
        assert_eq!(get("HEYGEN_NO_ANALYTICS").as_deref(), Some("1"));
        assert!(get("HYPERFRAMES_FFMPEG_PATH").unwrap().ends_with("ffmpeg-static/ffmpeg"));
        assert!(get("HYPERFRAMES_FFPROBE_PATH").unwrap().contains("@ffprobe-installer/"));
        assert_eq!((get("HYPERFRAMES_SKIP_SKILLS").as_deref(), get("HYPERFRAMES_NO_TELEMETRY").as_deref()), (Some("1"), Some("1")));
        assert!(!status_of(&rt).ready, "nothing installed means not ready");
    }

    /// Word times from a real voiceover, through the same code the pipeline uses. Needs a toolbox whose home
    /// has the speech model: `VIDEO_RT=<folder> VIDEO_WAV=<file.wav> cargo test -- --ignored --nocapture transcribe_gives_word_times`
    #[test]
    #[ignore]
    fn transcribe_gives_word_times() {
        let rt = Runtime { base: PathBuf::from(std::env::var("VIDEO_RT").expect("set VIDEO_RT")) };
        let wav = PathBuf::from(std::env::var("VIDEO_WAV").expect("set VIDEO_WAV to a spoken .wav"));
        assert!(rt.captions_ready(), "the speech model isn't installed in this toolbox's home");
        let dir = std::env::temp_dir().join(format!("jarvis-asr-{}", tasks::now_ms()));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::copy(&wav, dir.join("assets/voiceover.wav")).unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let words = runtime.block_on(transcribe_words(&rt, &dir, "assets/voiceover.wav")).expect("no words came back");
        println!("{} words; first: {}; last: {}", words.len(), words[0], words[words.len() - 1]);
        assert!(words.windows(2).all(|p| p[1]["start"].as_f64() >= p[0]["start"].as_f64()), "in order");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Runs the whole local flow against a real toolbox: new project, check, draft render. It needs a
    /// toolbox already set up (the app's, or one made by hand): `VIDEO_RT=<folder> cargo test --
    /// --ignored --nocapture video_flow_renders_a_real_video`
    #[test]
    #[ignore]
    fn video_flow_renders_a_real_video() {
        let rt = Runtime { base: PathBuf::from(std::env::var("VIDEO_RT").expect("set VIDEO_RT to a folder holding node/ and pkg/")) };
        assert!(rt.installed(), "that toolbox isn't complete");
        let dir = std::env::temp_dir().join(format!("jarvis-video-flow-{}", tasks::now_ms()));
        new_project(&rt, &dir, "Flow test", "landscape", 4).unwrap();
        let html = std::fs::read_to_string(dir.join("index.html")).unwrap().replace(
            "    </div>\n    <script>",
            "      <h1 id=\"t\" class=\"clip\" data-start=\"0\" data-duration=\"4\" data-track-index=\"0\" style=\"position:absolute;inset:0;display:flex;align-items:center;justify-content:center;color:#f4f4f5;font-size:96px\">Hello</h1>\n    </div>\n    <script>",
        ).replace("tl.seek(0);", "tl.fromTo(\"#t\", { opacity: 0, y: 40 }, { opacity: 1, y: 0, duration: 0.8 }, 0.2);\n      tl.seek(0);");
        std::fs::write(dir.join("index.html"), html).unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let report = runtime.block_on(check_project(&rt, &dir)).unwrap();
        println!("CHECK ok={} problems={:?}", report.ok, report.problems);
        let mut last = 0;
        let done = runtime.block_on(render_project(&rt, &dir, "draft", |p, m| {
            if p / 25 != last / 25 {
                println!("  {p}% {m}");
            }
            last = p;
        }))
        .unwrap();
        println!("RENDER {} {} KB in {}s -> {}", done.path, done.size / 1024, done.seconds_taken, dir.display());
        assert!(done.size > 10_000, "the video is empty");
        assert_eq!(info_of(&dir).renders.len(), 1);
    }
}
