//! The visual director's eyes. It opens a site in a hidden Chrome at three screen sizes (desktop,
//! tablet, phone), scrolls the page so scroll animations play, measures the layout for problems
//! (overflow, overlaps, tiny or low-contrast text, stuck animations, empty gaps) and saves the whole
//! page as screenshots in slices. The review itself (a vision model looking at the slices) happens in
//! the app; this module only captures, stores and teaches.

use crate::settings;
use crate::tasks;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tokio_tungstenite::tungstenite::Message;

const AUDIT_JS: &str = include_str!("director_audit.js");
/// Taller pages are cut off here; the review sees the first screens in detail.
const MAX_PAGE: u64 = 12_000;

/// (name, width, height, most screens to take, mobile). Each screenshot is one screenful, taken after
/// scrolling to that spot, the way a person sees the page: scroll-triggered animations and 3D scenes
/// that only run while on screen are in their real state.
const SIZES: [(&str, u32, u32, usize, bool); 3] = [("desktop", 1440, 900, 10, false), ("tablet", 820, 1180, 10, false), ("phone", 390, 844, 12, true)];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shot {
    pub viewport: String,
    pub width: u32,
    pub index: usize,
    /// Where on the page this slice starts and how tall it is, in CSS pixels.
    pub y: u64,
    pub height: u64,
    pub path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewportReport {
    pub name: String,
    pub width: u32,
    pub page_height: u64,
    pub audit: Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capture {
    pub shots: Vec<Shot>,
    pub viewports: Vec<ViewportReport>,
}

struct Chrome {
    child: std::process::Child,
    sink: futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, Message>,
    stream: futures_util::stream::SplitStream<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>,
    next: u64,
    session: String,
    profile: PathBuf,
}

impl Chrome {
    async fn launch() -> Result<Chrome, String> {
        let browser = tasks::find_chromium().ok_or("The visual director needs Google Chrome, Microsoft Edge or Brave installed.")?;
        let profile = std::env::temp_dir().join(format!("jarvis-director-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)));
        std::fs::create_dir_all(&profile).map_err(|e| e.to_string())?;
        let mut child = std::process::Command::new(&browser)
            .args(["--headless=new", "--remote-debugging-port=0", "--no-first-run", "--no-default-browser-check", "--hide-scrollbars", "--mute-audio", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"])
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg("about:blank")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Couldn't start the browser: {e}"))?;
        let port_file = profile.join("DevToolsActivePort");
        let started = Instant::now();
        let (port, path) = loop {
            if let Ok(text) = std::fs::read_to_string(&port_file) {
                let mut lines = text.lines();
                if let (Some(p), Some(path)) = (lines.next(), lines.next()) {
                    if let Ok(port) = p.trim().parse::<u16>() {
                        break (port, path.trim().to_string());
                    }
                }
            }
            if started.elapsed() > Duration::from_secs(20) || matches!(child.try_wait(), Ok(Some(_))) {
                let _ = child.kill();
                let _ = std::fs::remove_dir_all(&profile);
                return Err("The browser for the visual director didn't start.".into());
            }
            tokio::time::sleep(Duration::from_millis(120)).await;
        };
        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}{path}").as_str()).await.map_err(|e| format!("Couldn't reach the browser: {e}"))?;
        let (sink, stream) = ws.split();
        let mut c = Chrome { child, sink, stream, next: 0, session: String::new(), profile };
        let t = c.call_raw("Target.createTarget", json!({ "url": "about:blank" }), None).await?;
        let target = t["targetId"].as_str().ok_or("No page.")?.to_string();
        let a = c.call_raw("Target.attachToTarget", json!({ "targetId": target, "flatten": true }), None).await?;
        c.session = a["sessionId"].as_str().ok_or("Couldn't attach to the page.")?.to_string();
        c.call("Page.enable", json!({})).await?;
        c.call("Runtime.enable", json!({})).await?;
        Ok(c)
    }

    async fn call_raw(&mut self, method: &str, params: Value, session: Option<&str>) -> Result<Value, String> {
        self.next += 1;
        let id = self.next;
        let mut m = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            m["sessionId"] = json!(s);
        }
        self.sink.send(Message::Text(m.to_string().into())).await.map_err(|e| e.to_string())?;
        let wait = tokio::time::timeout(Duration::from_secs(40), async {
            while let Some(msg) = self.stream.next().await {
                let Ok(msg) = msg else { return Err("The browser closed.".to_string()) };
                let Ok(text) = msg.to_text() else { continue };
                let Ok(v) = serde_json::from_str::<Value>(text) else { continue };
                if v["id"].as_u64() == Some(id) {
                    if let Some(e) = v.get("error") {
                        return Err(format!("{method}: {}", e["message"].as_str().unwrap_or("failed")));
                    }
                    return Ok(v["result"].clone());
                }
            }
            Err("The browser closed.".to_string())
        });
        wait.await.map_err(|_| format!("{method} took too long."))?
    }

    async fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let s = self.session.clone();
        self.call_raw(method, params, Some(&s)).await
    }

    /// Run JavaScript in the page and get its (JSON) result back.
    async fn eval(&mut self, js: &str) -> Result<Value, String> {
        let r = self.call("Runtime.evaluate", json!({ "expression": js, "awaitPromise": true, "returnByValue": true })).await?;
        if let Some(ex) = r.get("exceptionDetails") {
            return Err(format!("The page's script failed: {}", ex["exception"]["description"].as_str().or(ex["text"].as_str()).unwrap_or("error")));
        }
        Ok(r["result"]["value"].clone())
    }

    /// Wait for the page's load event (or give up after `secs`).
    async fn wait_load(&mut self, secs: u64) {
        let _ = tokio::time::timeout(Duration::from_secs(secs), async {
            while let Some(Ok(msg)) = self.stream.next().await {
                if let Ok(text) = msg.to_text() {
                    if text.contains("\"Page.loadEventFired\"") {
                        return;
                    }
                }
            }
        })
        .await;
    }

    fn close(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

const SCROLL_JS: &str = r#"(async () => {
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  document.documentElement.style.scrollBehavior = 'auto';
  const step = Math.max(300, Math.round(innerHeight * 0.6));
  let y = 0, guard = 0;
  while (y < document.documentElement.scrollHeight && guard++ < 60) { window.scrollTo(0, y); await wait(330); y += step; }
  window.scrollTo(0, document.documentElement.scrollHeight); await wait(500);
  window.scrollTo(0, 0); await wait(900);
  return document.documentElement.scrollHeight;
})()"#;

async fn capture_one(c: &mut Chrome, url: &str, size: (&str, u32, u32, usize, bool), out: &Path) -> Result<(Vec<Shot>, ViewportReport), String> {
    let (name, width, height, max_slices, mobile) = size;
    c.call("Emulation.setDeviceMetricsOverride", json!({ "width": width, "height": height, "deviceScaleFactor": 1, "mobile": mobile })).await?;
    c.call("Page.navigate", json!({ "url": url })).await?;
    c.wait_load(25).await;
    tokio::time::sleep(Duration::from_millis(1800)).await;
    c.eval(SCROLL_JS).await?;
    let audit = c.eval(AUDIT_JS).await.unwrap_or_else(|e| json!({ "error": e }));
    let metrics = c.call("Page.getLayoutMetrics", json!({})).await?;
    let page_h = metrics["cssContentSize"]["height"].as_f64().or(metrics["contentSize"]["height"].as_f64()).unwrap_or(height as f64).ceil() as u64;
    let total = page_h.min(MAX_PAGE);
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut shots = vec![];
    let mut y = 0u64;
    while y < total && shots.len() < max_slices {
        // Scroll there and give animations and lazy scenes a moment, then take the screen as it is.
        let at = c
            .eval(&format!("(async () => {{ window.scrollTo(0, {y}); await new Promise((r) => setTimeout(r, 650)); return Math.round(window.scrollY); }})()"))
            .await?
            .as_u64()
            .unwrap_or(y);
        let r = c.call("Page.captureScreenshot", json!({ "format": "jpeg", "quality": 74 })).await?;
        let data = base64::engine::general_purpose::STANDARD.decode(r["data"].as_str().unwrap_or("")).map_err(|e| e.to_string())?;
        let path = out.join(format!("{name}-{:02}.jpg", shots.len() + 1));
        std::fs::write(&path, data).map_err(|e| e.to_string())?;
        shots.push(Shot { viewport: name.to_string(), width, index: shots.len() + 1, y: at, height: (height as u64).min(page_h.saturating_sub(at)), path: path.display().to_string() });
        y += height as u64;
    }
    Ok((shots, ViewportReport { name: name.to_string(), width, page_height: page_h, audit }))
}

/// Render the site in `folder` at desktop, tablet and phone sizes into `out_dir`.
#[tauri::command]
pub async fn director_capture(app: AppHandle, folder: String, out_dir: String) -> Result<Capture, String> {
    let out = checked_dir(&app, &out_dir)?;
    let url = crate::preview::page_url(&app, &folder)?;
    capture_site(&url, &out).await
}

/// The capture itself: open `url` in a hidden browser at each size and save what it sees into `out`.
pub(crate) async fn capture_site(url: &str, out: &Path) -> Result<Capture, String> {
    let url = url.to_string();
    let mut chrome = Chrome::launch().await?;
    let mut shots = vec![];
    let mut viewports = vec![];
    let result: Result<(), String> = async {
        for size in SIZES {
            let (s, v) = capture_one(&mut chrome, &url, size, out).await?;
            shots.extend(s);
            viewports.push(v);
        }
        Ok(())
    }
    .await;
    chrome.close();
    result?;
    Ok(Capture { shots, viewports })
}

// ---------- storage, under the research folder only ----------

fn checked_dir(app: &AppHandle, dir: &str) -> Result<PathBuf, String> {
    let root = settings::research_root(app);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let real = PathBuf::from(dir).canonicalize().map_err(|e| e.to_string())?;
    let base = root.canonicalize().map_err(|e| e.to_string())?;
    if !real.starts_with(&base) {
        return Err("That folder is outside Jarvis's research folder.".into());
    }
    Ok(real)
}

fn checked_file(app: &AppHandle, path: &str) -> Result<PathBuf, String> {
    let real = PathBuf::from(path).canonicalize().map_err(|_| "That file isn't there.".to_string())?;
    let base = settings::research_root(app).canonicalize().map_err(|e| e.to_string())?;
    if !real.starts_with(&base) || !real.is_file() {
        return Err("That file is outside Jarvis's research folder.".into());
    }
    Ok(real)
}

/// One screenshot as base64 JPEG, for the vision model.
#[tauri::command]
pub fn director_shot(app: AppHandle, path: String) -> Result<String, String> {
    let file = checked_file(&app, &path)?;
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

#[tauri::command]
pub fn director_save(app: AppHandle, dir: String, json: String) -> Result<(), String> {
    let dir = checked_dir(&app, &dir)?;
    serde_json::from_str::<Value>(&json).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("director.json"), json).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn director_load(app: AppHandle, dir: String) -> Option<String> {
    // Only reads: opening a task must not create folders.
    let real = PathBuf::from(&dir).canonicalize().ok()?;
    let base = settings::research_root(&app).canonicalize().ok()?;
    if !real.starts_with(&base) {
        return None;
    }
    std::fs::read_to_string(real.join("director.json")).ok()
}

const LESSONS_HEADER: &str = "# Lessons from the visual director\n\nWhat past reviews found wrong in generated websites, written as rules. Read these before you design and don't repeat them. Edit freely: remove a rule that's wrong, add your own.\n\n";

/// Add what the director learned to the website-design skill's lessons file (skipping repeats).
pub(crate) fn lessons_file(app: &AppHandle) -> PathBuf {
    crate::routines::skills_dir(app).join("website-design").join("references").join("lessons.md")
}

pub(crate) fn add_lessons_to(file: &Path, lessons: &[String]) -> Result<usize, String> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut text = std::fs::read_to_string(file).unwrap_or_else(|_| LESSONS_HEADER.to_string());
    let norm = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
    let mut added = 0;
    for l in lessons {
        let l = l.trim().trim_start_matches(['-', '*', ' ']);
        if l.len() < 12 || l.len() > 400 {
            continue;
        }
        if text.lines().any(|line| norm(line) == norm(l)) {
            continue;
        }
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!("- {l}\n"));
        added += 1;
    }
    if added > 0 {
        std::fs::write(file, text).map_err(|e| e.to_string())?;
    }
    Ok(added)
}

#[tauri::command]
pub fn director_learn(app: AppHandle, lessons: Vec<String>) -> Result<usize, String> {
    add_lessons_to(&lessons_file(&app), &lessons)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lessons_are_added_once_and_never_duplicated() {
        let dir = std::env::temp_dir().join(format!("jarvis-lessons-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("references").join("lessons.md");
        let n = add_lessons_to(&file, &["Keep hero headlines to three lines at most.".into(), "- Align every section heading to the same left edge.".into(), "short".into()]).unwrap();
        assert_eq!(n, 2);
        let again = add_lessons_to(&file, &["keep hero headlines to THREE lines at most".into(), "Give every card the same padding.".into()]).unwrap();
        assert_eq!(again, 1);
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("# Lessons from the visual director"));
        assert_eq!(text.matches("hero headlines").count(), 1);
        assert!(text.contains("- Align every section heading to the same left edge."));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_audit_script_is_one_expression() {
        let code: String = AUDIT_JS.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        let t = code.trim();
        assert!(t.starts_with("(() =>") && t.ends_with(")()"));
    }

    /// Manual: JARVIS_DIRECTOR_URL=http://127.0.0.1:PORT/… JARVIS_DIRECTOR_OUT=/tmp/shots cargo test director_live -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn director_live() {
        let url = std::env::var("JARVIS_DIRECTOR_URL").expect("set JARVIS_DIRECTOR_URL");
        let out = PathBuf::from(std::env::var("JARVIS_DIRECTOR_OUT").unwrap_or("/tmp/director-live".into()));
        let cap = capture_site(&url, &out).await.expect("capture");
        for v in &cap.viewports {
            println!("{} page {}px", v.name, v.page_height);
        }
        println!("{} shots", cap.shots.len());
        std::fs::write(out.join("audit.json"), serde_json::to_string_pretty(&cap.viewports).unwrap()).unwrap();
    }
}
