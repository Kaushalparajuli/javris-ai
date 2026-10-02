//! Live preview of a site Jarvis is building.
//!
//! The web view can't open files from disk, and a site needs its CSS, scripts and images to resolve
//! like they would on a real server. So a tiny web server runs on this computer only (127.0.0.1, a
//! free port), serving the folders under <research root>/projects. The app shows it in a frame and
//! reloads it when files change. It only answers GET and HEAD, only for requests that name this
//! address as the host, never serves anything outside the projects folder, and hides dot-files.

use crate::settings;
use serde::Serialize;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, UNIX_EPOCH};
use tauri::AppHandle;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const MAX_FILE: u64 = 50 * 1024 * 1024;

static PORT: OnceLock<u16> = OnceLock::new();
/// What the server serves. Updated on every request for a preview, in case the research folder moved.
static ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewFile {
    /// Relative to the project folder, with forward slashes.
    pub path: String,
    pub size: u64,
    /// Milliseconds since 1970.
    pub modified: u64,
}

const MAX_LISTED: usize = 400;
const MAX_READ: u64 = 1024 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewInfo {
    /// The folder's address on the preview server, ending in "/", for images and other files.
    pub base_url: String,
    /// The page to show, or empty if the folder has no HTML yet.
    pub url: String,
    /// Changes whenever something in the folder is added, removed or edited.
    pub version: u64,
    pub has_page: bool,
}

fn root(app: &AppHandle) -> PathBuf {
    settings::research_root(app).join("projects")
}

fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "pdf" => "application/pdf",
        "txt" | "md" => "text/plain; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// A request path as a relative path under the root, or None if it tries anything but plain names.
fn relative(target: &str) -> Option<PathBuf> {
    let path = target.split(['?', '#']).next()?;
    let decoded = percent_decode(path)?;
    if decoded.contains('\0') || decoded.contains('\\') {
        return None;
    }
    let mut out = PathBuf::new();
    for part in decoded.split('/').filter(|p| !p.is_empty()) {
        if part == ".." || part.starts_with('.') {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

fn host_ok(host: &str, port: u16) -> bool {
    let h = host.trim().to_ascii_lowercase();
    h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}")
}

async fn respond(stream: &mut TcpStream, status: &str, ctype: &str, body: &[u8], head_only: bool) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    if !head_only {
        let _ = stream.write_all(body).await;
    }
    let _ = stream.shutdown().await;
}

async fn handle(mut stream: TcpStream, port: u16) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    let read = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                return Some(());
            }
            if buf.len() > 8192 {
                return None;
            }
        }
    })
    .await;
    if !matches!(read, Ok(Some(()))) {
        return;
    }
    let text = String::from_utf8_lossy(&buf).to_string();
    let mut lines = text.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (method, target) = (first.next().unwrap_or(""), first.next().unwrap_or("/"));
    let host = lines.find_map(|l| l.strip_prefix("Host:").or_else(|| l.strip_prefix("host:"))).unwrap_or("");
    if !host_ok(host, port) {
        return respond(&mut stream, "403 Forbidden", "text/plain", b"Forbidden", false).await;
    }
    let head_only = method == "HEAD";
    if method != "GET" && !head_only {
        return respond(&mut stream, "405 Method Not Allowed", "text/plain", b"Method not allowed", false).await;
    }
    let Some(root) = ROOT.lock().ok().and_then(|r| r.clone()) else {
        return respond(&mut stream, "404 Not Found", "text/plain", b"Not found", head_only).await;
    };
    let resolved = relative(target).and_then(|rel| {
        let mut full = root.join(rel);
        if full.is_dir() {
            full = full.join("index.html");
        }
        let real = full.canonicalize().ok()?;
        let base = root.canonicalize().ok()?;
        (real.starts_with(&base) && real.is_file()).then_some(real)
    });
    match resolved {
        Some(path) if std::fs::metadata(&path).map(|m| m.len() <= MAX_FILE).unwrap_or(false) => match tokio::fs::read(&path).await {
            Ok(bytes) => respond(&mut stream, "200 OK", mime(&path), &bytes, head_only).await,
            Err(_) => respond(&mut stream, "404 Not Found", "text/plain", b"Not found", head_only).await,
        },
        _ => respond(&mut stream, "404 Not Found", "text/plain", b"Not found", head_only).await,
    }
}

/// Start the server the first time it's needed. Returns its port.
fn ensure_server() -> Result<u16, String> {
    if let Some(p) = PORT.get() {
        return Ok(*p);
    }
    let std_listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| format!("Couldn't start the preview: {e}"))?;
    std_listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let port = std_listener.local_addr().map_err(|e| e.to_string())?.port();
    // Another call may have won the race: keep its server and let this listener go.
    if PORT.set(port).is_err() {
        return Ok(*PORT.get().unwrap());
    }
    tauri::async_runtime::spawn(async move {
        let Ok(listener) = TcpListener::from_std(std_listener) else { return };
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                tauri::async_runtime::spawn(handle(stream, port));
            }
        }
    });
    Ok(port)
}

fn fingerprint(dir: &Path, depth: u8, acc: &mut (u64, u64, u64)) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let Ok(meta) = e.metadata() else { continue };
        if meta.is_dir() {
            if depth < 4 {
                fingerprint(&e.path(), depth + 1, acc);
            }
        } else {
            acc.0 += 1;
            acc.1 += meta.len();
            let m = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
            acc.2 = acc.2.max(m);
        }
    }
}

/// A project folder under the research folder, resolved, or an error saying why not.
fn project_dir(app: &AppHandle, folder: &str) -> Result<(PathBuf, PathBuf), String> {
    let root = root(app);
    let base = root.canonicalize().map_err(|_| "There's nothing to preview yet.".to_string())?;
    let dir = PathBuf::from(folder).canonicalize().map_err(|_| "That folder isn't there yet.".to_string())?;
    let rel = dir.strip_prefix(&base).map_err(|_| "Only projects made in the research folder can be previewed.".to_string())?.to_path_buf();
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err("Bad folder.".into());
    }
    Ok((dir, rel))
}

/// Every file the project has so far (a few levels deep, no dot-files or node_modules), newest first.
fn list_dir(dir: &Path) -> Vec<PreviewFile> {
    fn walk(base: &Path, at: &Path, depth: u8, out: &mut Vec<PreviewFile>) {
        for e in std::fs::read_dir(at).into_iter().flatten().flatten() {
            if out.len() >= MAX_LISTED {
                return;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "node_modules" {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                if depth < 5 {
                    walk(base, &e.path(), depth + 1, out);
                }
            } else if let Ok(rel) = e.path().strip_prefix(base) {
                let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
                out.push(PreviewFile { path: rel.to_string_lossy().replace('\\', "/"), size: meta.len(), modified });
            }
        }
    }
    let mut out = vec![];
    walk(dir, dir, 0, &mut out);
    out.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    out
}

/// A file's text, for the code view. Only plain names under `dir`; binary and very large files are refused.
fn read_text(dir: &Path, rel: &str) -> Result<String, String> {
    let rel = relative(&format!("/{rel}")).ok_or("That file name can't be used.")?;
    let real = dir.join(rel).canonicalize().map_err(|_| "That file isn't there.".to_string())?;
    if !real.starts_with(dir) || !real.is_file() {
        return Err("That file isn't there.".into());
    }
    let size = std::fs::metadata(&real).map_err(|e| e.to_string())?.len();
    if size > MAX_READ {
        return Err(format!("This file is {} KB, too large to show here.", size / 1024));
    }
    String::from_utf8(std::fs::read(&real).map_err(|e| e.to_string())?).map_err(|_| "This isn't a text file.".to_string())
}

#[tauri::command]
pub fn preview_files(app: AppHandle, folder: String) -> Result<Vec<PreviewFile>, String> {
    let (dir, _) = project_dir(&app, &folder)?;
    Ok(list_dir(&dir))
}

#[tauri::command]
pub fn preview_read(app: AppHandle, folder: String, path: String) -> Result<String, String> {
    let (dir, _) = project_dir(&app, &folder)?;
    read_text(&dir, &path)
}

/// The page address for a project folder's site, or an error if it has no page yet.
pub(crate) fn page_url(app: &AppHandle, folder: &str) -> Result<String, String> {
    let info = preview_info(app.clone(), folder.to_string())?;
    if info.has_page {
        Ok(info.url)
    } else {
        Err("There's no web page in this folder yet.".into())
    }
}

/// Where to see the site in `folder` (a project folder under the research folder), and a number
/// that changes when its files do.
#[tauri::command]
pub fn preview_info(app: AppHandle, folder: String) -> Result<PreviewInfo, String> {
    let (dir, rel) = project_dir(&app, &folder)?;
    *ROOT.lock().map_err(|_| "Preview is busy.".to_string())? = Some(root(&app));
    let port = ensure_server()?;
    let mut acc = (0, 0, 0);
    fingerprint(&dir, 0, &mut acc);
    let version = acc.0.wrapping_mul(1_000_003) ^ acc.1.wrapping_mul(31) ^ acc.2;
    // index.html if there is one, otherwise the first page in the folder.
    let page = if dir.join("index.html").is_file() {
        Some("index.html".to_string())
    } else {
        let mut pages: Vec<String> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| !n.starts_with('.') && (n.ends_with(".html") || n.ends_with(".htm")))
            .collect();
        pages.sort();
        pages.into_iter().next()
    };
    let rel_url: String = rel.components().map(|c| c.as_os_str().to_string_lossy().replace(' ', "%20")).collect::<Vec<_>>().join("/");
    let url = match &page {
        Some(p) => format!("http://127.0.0.1:{port}/{rel_url}/{}", p.replace(' ', "%20")),
        None => String::new(),
    };
    Ok(PreviewInfo { base_url: format!("http://127.0.0.1:{port}/{rel_url}/"), url, version, has_page: page.is_some() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_under_the_root() {
        assert_eq!(relative("/site/files/css/app.css?v=3"), Some(PathBuf::from("site/files/css/app.css")));
        assert_eq!(relative("/a%20b/c.html"), Some(PathBuf::from("a b/c.html")));
        assert_eq!(relative("/"), Some(PathBuf::new()));
        assert_eq!(relative("/../etc/passwd"), None);
        assert_eq!(relative("/a/%2e%2e/b"), None);
        assert_eq!(relative("/.env"), None);
        assert_eq!(relative("/a/.git/config"), None);
        assert_eq!(relative("/a\\b"), None);
        assert_eq!(relative("/bad%zz"), None);
    }

    #[test]
    fn the_code_view_lists_and_reads_only_inside_the_project() {
        let tmp = std::env::temp_dir().join(format!("jarvis-codeview-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let site = tmp.join("site");
        std::fs::create_dir_all(site.join("css")).unwrap();
        std::fs::create_dir_all(site.join("node_modules")).unwrap();
        std::fs::write(site.join("index.html"), "<h1>Hi</h1>").unwrap();
        std::fs::write(site.join("css/a.css"), "body{}").unwrap();
        std::fs::write(site.join("pic.png"), [0x89, b'P', 0xff, 0xfe]).unwrap();
        std::fs::write(site.join(".env"), "SECRET=1").unwrap();
        std::fs::write(site.join("node_modules/x.js"), "x").unwrap();
        std::fs::write(tmp.join("outside.txt"), "no").unwrap();
        let site = site.canonicalize().unwrap();

        let paths: Vec<String> = list_dir(&site).into_iter().map(|f| f.path).collect();
        assert_eq!(paths, vec!["css/a.css", "index.html", "pic.png"]);
        assert_eq!(read_text(&site, "index.html").unwrap(), "<h1>Hi</h1>");
        assert_eq!(read_text(&site, "css/a.css").unwrap(), "body{}");
        assert!(read_text(&site, "pic.png").unwrap_err().contains("text"));
        assert!(read_text(&site, ".env").is_err());
        assert!(read_text(&site, "../outside.txt").is_err());
        assert!(read_text(&site, "css").is_err());
        assert!(read_text(&site, "missing.html").is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn only_this_host_is_answered() {
        assert!(host_ok(" 127.0.0.1:5000", 5000));
        assert!(host_ok("localhost:5000", 5000));
        assert!(!host_ok("evil.example:5000", 5000));
        assert!(!host_ok("127.0.0.1:5001", 5000));
    }

    #[test]
    fn files_get_the_right_type() {
        assert_eq!(mime(Path::new("a/index.html")), "text/html; charset=utf-8");
        assert_eq!(mime(Path::new("x.CSS")), "text/css; charset=utf-8");
        assert_eq!(mime(Path::new("x.unknown")), "application/octet-stream");
    }

    async fn get(port: u16, target: &str, host: &str) -> String {
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        c.write_all(format!("GET {target} HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes()).await.unwrap();
        let mut out = Vec::new();
        c.read_to_end(&mut out).await.unwrap();
        String::from_utf8_lossy(&out).to_string()
    }

    #[tokio::test]
    async fn serves_a_site_and_nothing_else() {
        let tmp = std::env::temp_dir().join(format!("jarvis-preview-test-{}", std::process::id()));
        let site = tmp.join("projects").join("demo").join("files");
        std::fs::create_dir_all(site.join("css")).unwrap();
        std::fs::write(site.join("index.html"), "<h1>Hi</h1>").unwrap();
        std::fs::write(site.join("css").join("a.css"), "body{}").unwrap();
        std::fs::write(tmp.join("secret.txt"), "no").unwrap();
        std::fs::write(site.join(".hidden"), "no").unwrap();
        *ROOT.lock().unwrap() = Some(tmp.join("projects"));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (s, _) = listener.accept().await.unwrap();
                tokio::spawn(handle(s, port));
            }
        });
        let host = format!("127.0.0.1:{port}");

        let page = get(port, "/demo/files/index.html", &host).await;
        assert!(page.starts_with("HTTP/1.1 200 OK"), "{page}");
        assert!(page.contains("text/html") && page.contains("<h1>Hi</h1>") && page.contains("no-store"));
        let dir = get(port, "/demo/files/", &host).await;
        assert!(dir.contains("<h1>Hi</h1>"), "a folder shows its index.html");
        let css = get(port, "/demo/files/css/a.css", &host).await;
        assert!(css.contains("text/css") && css.contains("body{}"));
        assert!(get(port, "/demo/files/nope.html", &host).await.starts_with("HTTP/1.1 404"));
        assert!(get(port, "/demo/files/../../../secret.txt", &host).await.starts_with("HTTP/1.1 404"));
        assert!(get(port, "/demo/files/%2e%2e/%2e%2e/%2e%2e/secret.txt", &host).await.starts_with("HTTP/1.1 404"));
        assert!(get(port, "/demo/files/.hidden", &host).await.starts_with("HTTP/1.1 404"));
        assert!(get(port, "/demo/files/index.html", "evil.example").await.starts_with("HTTP/1.1 403"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
