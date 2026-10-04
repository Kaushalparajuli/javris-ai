//! File index: search the folders the user chose, by words and by meaning.
//!
//! Only folders added on purpose are read. Text is pulled out of notes, documents, PDFs and decks,
//! cut into chunks and stored in <research root>/files.db with a full-text index. With a Gemini key
//! each chunk also gets an embedding, so a question finds the right paragraph even when the words
//! differ. Only chunks of text go to the embedding service, never whole folders or file names.
//! Re-scanning skips files that haven't changed.

use crate::memory::{dot, embed, fts_query, from_bytes, normalize, to_bytes, EMBED_DIMS, EMBED_MODEL, MIN_SIMILARITY};
use crate::settings;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::AppHandle;

const MAX_FILES_PER_FOLDER: usize = 4000;
const MAX_DEPTH: usize = 8;
const MAX_BYTES: u64 = 8 * 1024 * 1024;
const CHUNK_CHARS: usize = 800;
const OVERLAP: usize = 100;
const MAX_CHUNKS_PER_FILE: usize = 150;
const MIN_CHUNK: usize = 40;
/// Chunks sent for embedding in one request (the API takes up to 100).
const BATCH: usize = 64;
/// A ceiling on how much one index may send out for embedding.
const MAX_EMBEDDED: i64 = 20_000;

const SKIP_DIRS: [&str; 9] = ["node_modules", "target", "build", "dist", "Library", "venv", "__pycache__", "Pods", "DerivedData"];
const TEXT_EXT: [&str; 6] = ["md", "markdown", "txt", "csv", "tsv", "log"];
const CONVERT_EXT: [&str; 5] = ["docx", "doc", "rtf", "odt", "html"];
const SPOTLIGHT_EXT: [&str; 4] = ["pdf", "pptx", "key", "pages"];

/// Reads a PDF with macOS's PDFKit, through the built-in JavaScript automation.
const PDF_SCRIPT: &str = r#"ObjC.import("PDFKit");
function run(argv) {
  var doc = $.PDFDocument.alloc.initWithURL($.NSURL.fileURLWithPath(argv[0]));
  if (!doc || doc.isNil()) return "";
  var s = doc.string;
  return s && !s.isNil() ? ObjC.unwrap(s) : "";
}"#;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileHit {
    pub path: String,
    pub name: String,
    pub folder: String,
    pub snippet: String,
}

#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatus {
    pub folders: Vec<String>,
    pub files: i64,
    pub chunks: i64,
    pub embedded: i64,
    pub scanning: bool,
}

/// Whether a scan is running, and whether another was asked for while it ran (say a folder was
/// added), in which case the scan goes round once more when it's done.
struct Scan {
    running: bool,
    again: bool,
}

static SCAN: Mutex<Scan> = Mutex::new(Scan { running: false, again: false });

/// Claim the scanner. If it's busy, ask it to go round again instead and return false.
fn begin_scan(scan: &Mutex<Scan>) -> bool {
    let mut s = scan.lock().unwrap_or_else(|e| e.into_inner());
    if s.running {
        s.again = true;
        return false;
    }
    *s = Scan { running: true, again: false };
    true
}

/// A pass is done: true if someone asked for another one meanwhile, otherwise the scanner is free.
fn scan_again(scan: &Mutex<Scan>) -> bool {
    let mut s = scan.lock().unwrap_or_else(|e| e.into_inner());
    if std::mem::take(&mut s.again) {
        return true;
    }
    s.running = false;
    false
}

/// Longest a converter may take over one file before it's stopped and the file skipped.
const EXTRACT_LIMIT: Duration = Duration::from_secs(20);
static EMBEDDING: AtomicBool = AtomicBool::new(false);

fn open(path: &Path) -> Result<Connection, String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let db = Connection::open(path).map_err(|e| format!("Couldn't open the file index: {e}"))?;
    db.execute_batch(
        "PRAGMA foreign_keys = OFF;
         CREATE TABLE IF NOT EXISTS files(
            id INTEGER PRIMARY KEY, folder TEXT NOT NULL, path TEXT NOT NULL UNIQUE, mtime INTEGER NOT NULL, size INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS chunks(
            id INTEGER PRIMARY KEY, file_id INTEGER NOT NULL, ord INTEGER NOT NULL, text TEXT NOT NULL, embedding BLOB
         );
         CREATE INDEX IF NOT EXISTS chunks_file ON chunks(file_id);
         CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(text, content='chunks', content_rowid='id', tokenize='trigram');
         CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
            INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
         END;
         CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
            INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES ('delete', old.id, old.text);
         END;",
    )
    .map_err(|e| format!("Couldn't set up the file index: {e}"))?;
    Ok(db)
}

fn db(app: &AppHandle) -> Result<Connection, String> {
    open(&settings::research_root(app).join("files.db"))
}

fn folders_path(app: &AppHandle) -> PathBuf {
    settings::research_root(app).join("index_folders.json")
}

fn load_folders(app: &AppHandle) -> Vec<String> {
    std::fs::read_to_string(folders_path(app)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_folders(app: &AppHandle, list: &[String]) -> Result<(), String> {
    let path = folders_path(app);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, serde_json::to_string_pretty(list).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

// ---------- reading files ----------

/// Cut text into overlapping pieces, on paragraph boundaries where it can.
pub fn chunk(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        let t = cur.trim();
        if t.chars().count() >= MIN_CHUNK {
            out.push(t.to_string());
        }
        cur.clear();
    };
    for para in text.split("\n\n") {
        let para = para.split_whitespace().collect::<Vec<_>>().join(" ");
        if para.is_empty() {
            continue;
        }
        let chars: Vec<char> = para.chars().collect();
        if chars.len() > CHUNK_CHARS {
            flush(&mut cur, &mut out);
            let mut start = 0;
            while start < chars.len() {
                let end = (start + CHUNK_CHARS).min(chars.len());
                cur = chars[start..end].iter().collect();
                flush(&mut cur, &mut out);
                if end == chars.len() {
                    break;
                }
                start = end - OVERLAP;
            }
        } else {
            if cur.chars().count() + chars.len() > CHUNK_CHARS {
                flush(&mut cur, &mut out);
            }
            if !cur.is_empty() {
                cur.push('\n');
            }
            cur.push_str(&para);
        }
        if out.len() >= MAX_CHUNKS_PER_FILE {
            return out;
        }
    }
    flush(&mut cur, &mut out);
    out.truncate(MAX_CHUNKS_PER_FILE);
    out
}

fn ext_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn supported(path: &Path) -> bool {
    let e = ext_of(path);
    TEXT_EXT.contains(&e.as_str()) || CONVERT_EXT.contains(&e.as_str()) || SPOTLIGHT_EXT.contains(&e.as_str())
}

/// What a converter prints, or nothing if it fails to start or takes longer than `limit` (then
/// it's stopped, so one bad file can't hold up the whole index).
fn text_from(cmd: &mut Command, limit: Duration) -> String {
    let Ok(mut child) = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else {
        return String::new();
    };
    let out = child.stdout.take();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = vec![];
        if let Some(mut o) = out {
            let _ = o.read_to_end(&mut bytes);
        }
        let _ = tx.send(bytes);
    });
    match rx.recv_timeout(limit) {
        Ok(bytes) => {
            let _ = child.wait();
            String::from_utf8_lossy(&bytes).into_owned()
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            String::new()
        }
    }
}

/// The text of a file, or nothing when it can't be read.
fn extract(path: &Path) -> String {
    let e = ext_of(path);
    if TEXT_EXT.contains(&e.as_str()) {
        return std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    }
    if CONVERT_EXT.contains(&e.as_str()) {
        // macOS's own converter handles Word, RTF, OpenDocument and HTML.
        return text_from(Command::new("/usr/bin/textutil").args(["-convert", "txt", "-stdout"]).arg(path), EXTRACT_LIMIT);
    }
    if e == "pdf" {
        return text_from(Command::new("/usr/bin/osascript").args(["-l", "JavaScript", "-e", PDF_SCRIPT]).arg(path), EXTRACT_LIMIT);
    }
    // Decks and Pages files: the text Spotlight already extracted.
    let text = text_from(Command::new("/usr/bin/mdls").args(["-raw", "-name", "kMDItemTextContent"]).arg(path), EXTRACT_LIMIT);
    if text.trim() == "(null)" {
        String::new()
    } else {
        text
    }
}

fn mtime_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Supported files under a folder, skipping hidden and build folders.
fn walk(folder: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut stack = vec![(folder.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                if depth < MAX_DEPTH && !SKIP_DIRS.contains(&name.as_str()) && !name.ends_with(".app") {
                    stack.push((path, depth + 1));
                }
            } else if ft.is_file() && supported(&path) && out.len() < MAX_FILES_PER_FOLDER {
                out.push(path);
            }
        }
    }
    out
}

/// Store (or replace) one file's chunks.
fn store_file(db: &Connection, folder: &str, path: &str, mtime: i64, size: i64, chunks: &[String]) -> Result<(), String> {
    let old: Option<i64> = db.query_row("SELECT id FROM files WHERE path = ?1", params![path], |r| r.get(0)).ok();
    if let Some(id) = old {
        db.execute("DELETE FROM chunks WHERE file_id = ?1", params![id]).map_err(|e| e.to_string())?;
        db.execute("DELETE FROM files WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    }
    db.execute("INSERT INTO files(folder, path, mtime, size) VALUES (?1, ?2, ?3, ?4)", params![folder, path, mtime, size]).map_err(|e| e.to_string())?;
    let id = db.last_insert_rowid();
    for (i, c) in chunks.iter().enumerate() {
        db.execute("INSERT INTO chunks(file_id, ord, text) VALUES (?1, ?2, ?3)", params![id, i as i64, c]).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn forget_file(db: &Connection, path: &str) {
    if let Ok(id) = db.query_row::<i64, _, _>("SELECT id FROM files WHERE path = ?1", params![path], |r| r.get(0)) {
        let _ = db.execute("DELETE FROM chunks WHERE file_id = ?1", params![id]);
        let _ = db.execute("DELETE FROM files WHERE id = ?1", params![id]);
    }
}

/// Bring one folder's rows up to date: new and changed files are read, vanished ones are dropped.
fn scan_folder(db: &Connection, folder: &str) -> Result<(), String> {
    let mut seen: HashSet<String> = HashSet::new();
    for path in walk(Path::new(folder)) {
        let p = path.display().to_string();
        seen.insert(p.clone());
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if meta.len() > MAX_BYTES {
            continue;
        }
        let (mtime, size) = (mtime_secs(&meta), meta.len() as i64);
        let same: Option<(i64, i64)> = db.query_row("SELECT mtime, size FROM files WHERE path = ?1", params![p], |r| Ok((r.get(0)?, r.get(1)?))).ok();
        if same == Some((mtime, size)) {
            continue;
        }
        let chunks = chunk(&extract(&path));
        store_file(db, folder, &p, mtime, size, &chunks)?;
    }
    let mut st = db.prepare("SELECT path FROM files WHERE folder = ?1").map_err(|e| e.to_string())?;
    let known: Vec<String> = st.query_map(params![folder], |r| r.get(0)).map_err(|e| e.to_string())?.flatten().collect();
    for p in known.into_iter().filter(|p| !seen.contains(p)) {
        forget_file(db, &p);
    }
    Ok(())
}

// ---------- embeddings ----------

/// Embeddings for a batch of chunk texts, in order.
async fn embed_batch(key: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
    let requests: Vec<serde_json::Value> = texts
        .iter()
        .map(|t| serde_json::json!({ "model": format!("models/{EMBED_MODEL}"), "content": { "parts": [{ "text": t }] }, "taskType": "RETRIEVAL_DOCUMENT", "outputDimensionality": EMBED_DIMS }))
        .collect();
    let res = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?
        .post(format!("https://generativelanguage.googleapis.com/v1beta/models/{EMBED_MODEL}:batchEmbedContents"))
        .header("x-goog-api-key", key)
        .json(&serde_json::json!({ "requests": requests }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("Embedding failed ({})", res.status()));
    }
    let v: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
    let out: Vec<Vec<f32>> = v["embeddings"]
        .as_array()
        .map(|a| a.iter().map(|e| normalize(e["values"].as_array().map(|x| x.iter().filter_map(|n| n.as_f64().map(|f| f as f32)).collect()).unwrap_or_default())).collect())
        .unwrap_or_default();
    if out.len() != texts.len() || out.iter().any(|v| v.is_empty()) {
        return Err("Embedding came back incomplete.".into());
    }
    Ok(out)
}

/// Embed chunks that have none yet. Quietly stops at the first failure and tries again next time.
pub async fn embed_pending(app: AppHandle) {
    let key = settings::load(&app).gemini_api_key;
    if key.is_empty() || EMBEDDING.swap(true, Ordering::SeqCst) {
        return;
    }
    loop {
        let batch: Vec<(i64, String)> = match db(&app).and_then(|db| {
            let have: i64 = db.query_row("SELECT COUNT(*) FROM chunks WHERE embedding IS NOT NULL", [], |r| r.get(0)).map_err(|e| e.to_string())?;
            if have >= MAX_EMBEDDED {
                return Ok(vec![]);
            }
            let mut st = db.prepare("SELECT id, text FROM chunks WHERE embedding IS NULL LIMIT ?1").map_err(|e| e.to_string())?;
            let rows = st.query_map(params![BATCH as i64], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|e| e.to_string())?;
            Ok(rows.flatten().collect())
        }) {
            Ok(b) => b,
            Err(_) => break,
        };
        if batch.is_empty() {
            break;
        }
        let texts: Vec<String> = batch.iter().map(|(_, t)| t.clone()).collect();
        let Ok(vectors) = embed_batch(&key, &texts).await else { break };
        if let Ok(db) = db(&app) {
            for ((id, _), v) in batch.iter().zip(vectors) {
                let _ = db.execute("UPDATE chunks SET embedding = ?1 WHERE id = ?2", params![to_bytes(&v), id]);
            }
        }
    }
    EMBEDDING.store(false, Ordering::SeqCst);
}

// ---------- searching ----------

struct Row {
    chunk: i64,
    path: String,
    folder: String,
    text: String,
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<Row> {
    Ok(Row { chunk: r.get(0)?, path: r.get(1)?, folder: r.get(2)?, text: r.get(3)? })
}

fn keyword_in(db: &Connection, query: &str, limit: usize) -> Result<Vec<Row>, String> {
    let Some(fts) = fts_query(query) else { return Ok(vec![]) };
    let mut st = db
        .prepare("SELECT c.id, f.path, f.folder, c.text FROM chunks_fts x JOIN chunks c ON c.id = x.rowid JOIN files f ON f.id = c.file_id WHERE chunks_fts MATCH ?1 ORDER BY bm25(chunks_fts) LIMIT ?2")
        .map_err(|e| e.to_string())?;
    let rows = st.query_map(params![fts, limit as i64], row).map_err(|e| e.to_string())?;
    Ok(rows.flatten().collect())
}

fn semantic_in(db: &Connection, query: &[f32], limit: usize) -> Result<Vec<Row>, String> {
    let mut st = db
        .prepare("SELECT c.id, f.path, f.folder, c.text, c.embedding FROM chunks c JOIN files f ON f.id = c.file_id WHERE c.embedding IS NOT NULL")
        .map_err(|e| e.to_string())?;
    let rows = st.query_map([], |r| Ok((row(r)?, r.get::<_, Vec<u8>>(4)?))).map_err(|e| e.to_string())?;
    let mut scored: Vec<(f32, Row)> = rows.flatten().map(|(r, e)| (dot(query, &from_bytes(&e)), r)).filter(|(s, _)| *s >= MIN_SIMILARITY).collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok(scored.into_iter().take(limit).map(|(_, r)| r).collect())
}

/// Merge the two rankings (reciprocal rank fusion) and keep each file's best chunk.
fn fuse(keyword: Vec<Row>, meaning: Vec<Row>, limit: usize) -> Vec<FileHit> {
    let mut score: std::collections::HashMap<i64, (f32, Row)> = std::collections::HashMap::new();
    for list in [keyword, meaning] {
        for (rank, r) in list.into_iter().enumerate() {
            let add = 1.0 / (60.0 + rank as f32);
            score.entry(r.chunk).and_modify(|e| e.0 += add).or_insert((add, r));
        }
    }
    let mut all: Vec<(f32, Row)> = score.into_values().collect();
    all.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut seen = HashSet::new();
    let mut out = vec![];
    for (_, r) in all {
        if !seen.insert(r.path.clone()) {
            continue;
        }
        let name = Path::new(&r.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        out.push(FileHit { snippet: r.text.chars().take(320).collect(), name, path: r.path, folder: r.folder });
        if out.len() >= limit {
            break;
        }
    }
    out
}

// ---------- commands ----------

fn status_of(app: &AppHandle) -> IndexStatus {
    let mut s = IndexStatus { folders: load_folders(app), scanning: SCAN.lock().unwrap_or_else(|e| e.into_inner()).running, ..Default::default() };
    if let Ok(db) = db(app) {
        s.files = db.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap_or(0);
        s.chunks = db.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0)).unwrap_or(0);
        s.embedded = db.query_row("SELECT COUNT(*) FROM chunks WHERE embedding IS NOT NULL", [], |r| r.get(0)).unwrap_or(0);
    }
    s
}

/// Re-read every chosen folder, then embed what's new. One scan at a time.
/// A request while a scan is running (a folder added mid-scan) makes it go round once more.
pub async fn rescan(app: AppHandle) {
    if !begin_scan(&SCAN) {
        return;
    }
    loop {
        let handle = app.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            if let Ok(db) = db(&handle) {
                for f in load_folders(&handle) {
                    let _ = scan_folder(&db, &f);
                }
            }
        })
        .await;
        if !scan_again(&SCAN) {
            break;
        }
    }
    embed_pending(app).await;
}

/// Keep the index fresh while Jarvis sits in the menu bar: a quick re-scan every 15 minutes.
/// Unchanged files are skipped, so with nothing new this costs a directory walk.
pub async fn run_scheduler(app: AppHandle) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(15 * 60)).await;
        if !load_folders(&app).is_empty() {
            rescan(app.clone()).await;
        }
    }
}

#[tauri::command]
pub fn index_status(app: AppHandle) -> IndexStatus {
    status_of(&app)
}

#[tauri::command]
pub async fn index_add_folder(app: AppHandle, path: String) -> Result<IndexStatus, String> {
    let dir = crate::tasks::check_project(&app, &path)?.display().to_string();
    let mut list = load_folders(&app);
    if !list.contains(&dir) {
        list.push(dir);
        save_folders(&app, &list)?;
    }
    rescan(app.clone()).await;
    Ok(status_of(&app))
}

#[tauri::command]
pub async fn index_remove_folder(app: AppHandle, path: String) -> Result<IndexStatus, String> {
    let mut list = load_folders(&app);
    list.retain(|f| f != &path);
    save_folders(&app, &list)?;
    let db = db(&app)?;
    let ids: Vec<i64> = db.prepare("SELECT id FROM files WHERE folder = ?1").and_then(|mut st| st.query_map(params![path], |r| r.get(0)).map(|r| r.flatten().collect())).unwrap_or_default();
    for id in ids {
        let _ = db.execute("DELETE FROM chunks WHERE file_id = ?1", params![id]);
        let _ = db.execute("DELETE FROM files WHERE id = ?1", params![id]);
    }
    Ok(status_of(&app))
}

#[tauri::command]
pub async fn index_refresh(app: AppHandle) -> IndexStatus {
    rescan(app.clone()).await;
    status_of(&app)
}

/// Passages from the chosen folders that answer `query`, best file first.
#[tauri::command]
pub async fn index_search(app: AppHandle, query: String, limit: Option<usize>) -> Result<Vec<FileHit>, String> {
    let limit = limit.unwrap_or(8).clamp(1, 30);
    let keyword = keyword_in(&db(&app)?, &query, limit * 3)?;
    let key = settings::load(&app).gemini_api_key;
    let mut meaning = vec![];
    if !key.is_empty() && query.trim().chars().count() >= 3 {
        if let Ok(q) = embed(&key, query.trim(), "RETRIEVAL_QUERY").await {
            meaning = semantic_in(&db(&app)?, &q, limit * 3)?;
        }
    }
    Ok(fuse(keyword, meaning, limit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rescan_asked_for_mid_scan_runs_after_it() {
        let scan = Mutex::new(Scan { running: false, again: false });
        assert!(begin_scan(&scan), "the first scan starts");
        assert!(!begin_scan(&scan), "a second waits for the first");
        assert!(!begin_scan(&scan));
        assert!(scan_again(&scan), "the first goes round once more");
        assert!(!scan_again(&scan), "then it's done");
        assert!(!scan.lock().unwrap().running);
        assert!(begin_scan(&scan), "and the next scan can start");
    }

    #[test]
    fn a_stuck_converter_is_stopped() {
        let started = std::time::Instant::now();
        let text = text_from(Command::new("/bin/sleep").arg("30"), Duration::from_millis(300));
        assert_eq!(text, "");
        assert!(started.elapsed() < Duration::from_secs(5), "took {:?}", started.elapsed());
        assert_eq!(text_from(Command::new("/bin/echo").arg("hello"), Duration::from_secs(5)).trim(), "hello");
    }

    fn temp() -> Connection {
        open(Path::new(":memory:")).unwrap()
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jarvis-index-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn text_is_cut_into_overlapping_pieces() {
        let para = "word ".repeat(400);
        let chunks = chunk(&para);
        assert!(chunks.len() >= 2);
        assert!(chunks.iter().all(|c| c.chars().count() <= CHUNK_CHARS));
        assert!(chunk("tiny").is_empty());
        let two = chunk("First paragraph about pricing for the Fikra product.\n\nSecond paragraph about the office lease in Lalitpur.");
        assert_eq!(two.len(), 1, "short paragraphs share a chunk");
    }

    #[test]
    fn a_folder_is_read_changed_and_forgotten() {
        let dir = scratch("scan");
        std::fs::write(dir.join("pricing.md"), "# Pricing\n\nThe Fikra monthly subscription costs NPR 4,999 per month for schools.").unwrap();
        std::fs::write(dir.join("photo.jpg"), "not text").unwrap();
        std::fs::create_dir_all(dir.join("node_modules")).unwrap();
        std::fs::write(dir.join("node_modules/skip.md"), "should never be indexed because it is dependencies").unwrap();
        std::fs::write(dir.join(".hidden.md"), "hidden notes that should be skipped entirely please").unwrap();
        let db = temp();
        let folder = dir.display().to_string();
        scan_folder(&db, &folder).unwrap();
        let files: i64 = db.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(files, 1);
        let hits = fuse(keyword_in(&db, "subscription pricing", 5).unwrap(), vec![], 5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "pricing.md");
        assert!(hits[0].snippet.contains("4,999"));

        // An unchanged file isn't read again; a changed one is.
        std::fs::write(dir.join("pricing.md"), "# Pricing\n\nThe Fikra plan is now NPR 6,500 per month for every school.").unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options().write(true).open(dir.join("pricing.md")).unwrap().set_modified(later).unwrap();
        scan_folder(&db, &folder).unwrap();
        assert!(keyword_in(&db, "4,999", 5).unwrap().is_empty());
        assert_eq!(keyword_in(&db, "6,500", 5).unwrap().len(), 1);

        std::fs::remove_file(dir.join("pricing.md")).unwrap();
        scan_folder(&db, &folder).unwrap();
        assert!(keyword_in(&db, "6,500", 5).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn word_documents_are_read() {
        let dir = scratch("docx");
        std::fs::write(dir.join("memo.txt"), "The board approved the Omega Edu Park partnership agreement on Monday.").unwrap();
        let out = Command::new("/usr/bin/textutil").args(["-convert", "docx", "memo.txt"]).current_dir(&dir).status();
        if out.map(|s| s.success()).unwrap_or(false) {
            std::fs::remove_file(dir.join("memo.txt")).unwrap();
            let text = extract(&dir.join("memo.docx"));
            assert!(text.contains("Omega Edu Park"), "got {text:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pdfs_are_read() {
        let dir = scratch("pdf");
        std::fs::write(dir.join("note.txt"), "Quarterly review of the Gsoft payroll migration plan and timeline.").unwrap();
        let made = Command::new("/usr/sbin/cupsfilter").arg(dir.join("note.txt")).output();
        if let Ok(o) = made {
            if o.status.success() && o.stdout.starts_with(b"%PDF") {
                std::fs::write(dir.join("note.pdf"), &o.stdout).unwrap();
                let text = extract(&dir.join("note.pdf"));
                assert!(text.contains("payroll migration"), "got {text:?}");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn meaning_search_finds_what_words_miss() {
        let db = temp();
        store_file(&db, "/f", "/f/a.md", 1, 1, &["The subscription costs NPR 4,999 per month.".to_string()]).unwrap();
        store_file(&db, "/f", "/f/b.md", 1, 1, &["The office is in Lalitpur near the ring road.".to_string()]).unwrap();
        let unit = |x: f32, y: f32| normalize(vec![x, y]);
        db.execute("UPDATE chunks SET embedding = ?1 WHERE text LIKE '%subscription%'", params![to_bytes(&unit(1.0, 0.1))]).unwrap();
        db.execute("UPDATE chunks SET embedding = ?1 WHERE text LIKE '%Lalitpur%'", params![to_bytes(&unit(0.0, 1.0))]).unwrap();
        // "how much do we charge" shares no words with either file.
        assert!(keyword_in(&db, "how much do we charge", 5).unwrap().is_empty());
        let hits = fuse(vec![], semantic_in(&db, &unit(1.0, 0.0), 5).unwrap(), 5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "a.md");
    }
}
