//! What Jarvis remembers: people, projects, decisions, preferences and facts, in a small SQLite
//! file (<research root>/memory.db) with full-text search. Each memory can belong to a project
//! workspace, and records where it came from (a chat, a meeting, the user's own words).
//!
//! Search uses SQLite's trigram index, which matches parts of words in any script, so Nepali and
//! Hindi memories are found as readily as English ones. When a Gemini key is set, each memory also
//! gets an embedding, so "what did we decide on cost?" finds a memory that says "pricing". The two
//! rankings are merged; without a key or a connection, search is keyword-only.

use crate::settings;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::AppHandle;

const KINDS: [&str; 7] = ["person", "project", "decision", "preference", "fact", "meeting", "note"];

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: i64,
    pub kind: String,
    pub text: String,
    /// Workspace slug, or empty for memories that apply everywhere.
    pub workspace: String,
    /// Where it came from: "chat:<id>", "meeting", "user", "notes.md".
    pub source: String,
    pub created: i64,
    pub pinned: bool,
}

fn open(path: &std::path::Path) -> Result<Connection, String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let db = Connection::open(path).map_err(|e| format!("Couldn't open memory: {e}"))?;
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS memories(
            id INTEGER PRIMARY KEY,
            kind TEXT NOT NULL,
            text TEXT NOT NULL,
            workspace TEXT NOT NULL DEFAULT '',
            source TEXT NOT NULL DEFAULT '',
            created INTEGER NOT NULL,
            pinned INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT);
         CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(text, content='memories', content_rowid='id', tokenize='trigram');
         CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
            INSERT INTO memories_fts(rowid, text) VALUES (new.id, new.text);
         END;
         CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
            INSERT INTO memories_fts(memories_fts, rowid, text) VALUES ('delete', old.id, old.text);
         END;
         CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE OF text ON memories BEGIN
            INSERT INTO memories_fts(memories_fts, rowid, text) VALUES ('delete', old.id, old.text);
            INSERT INTO memories_fts(rowid, text) VALUES (new.id, new.text);
         END;",
    )
    .map_err(|e| format!("Couldn't set up memory: {e}"))?;
    // Added after the first release; fails harmlessly when the column is already there.
    let _ = db.execute("ALTER TABLE memories ADD COLUMN embedding BLOB", []);
    Ok(db)
}

fn db(app: &AppHandle) -> Result<Connection, String> {
    open(&settings::research_root(app).join("memory.db"))
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<Memory> {
    Ok(Memory { id: r.get(0)?, kind: r.get(1)?, text: r.get(2)?, workspace: r.get(3)?, source: r.get(4)?, created: r.get(5)?, pinned: r.get::<_, i64>(6)? != 0 })
}

const COLS: &str = "m.id, m.kind, m.text, m.workspace, m.source, m.created, m.pinned";

/// Words long enough for the trigram index, quoted so punctuation can't break the query.
pub(crate) fn fts_query(q: &str) -> Option<String> {
    let words: Vec<String> = q
        .split(|c: char| !c.is_alphanumeric() && !('\u{0900}'..='\u{097F}').contains(&c))
        .filter(|w| w.chars().count() >= 3)
        .map(|w| format!("\"{}\"", w.replace('"', "")))
        .collect();
    if words.is_empty() {
        None
    } else {
        Some(words.join(" OR "))
    }
}

pub(crate) const EMBED_MODEL: &str = "gemini-embedding-001";
pub(crate) const EMBED_DIMS: usize = 768;
/// Below this similarity a memory isn't considered related to the question.
pub(crate) const MIN_SIMILARITY: f32 = 0.55;

pub(crate) fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
    v
}

pub(crate) fn to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub(crate) fn from_bytes(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

pub(crate) fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// A unit-length embedding of `text` from Gemini. `task` is RETRIEVAL_DOCUMENT or RETRIEVAL_QUERY.
pub(crate) async fn embed(key: &str, text: &str, task: &str) -> Result<Vec<f32>, String> {
    let body = serde_json::json!({
        "model": format!("models/{EMBED_MODEL}"),
        "content": { "parts": [{ "text": text }] },
        "taskType": task,
        "outputDimensionality": EMBED_DIMS,
    });
    let res = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?
        .post(format!("https://generativelanguage.googleapis.com/v1beta/models/{EMBED_MODEL}:embedContent"))
        .header("x-goog-api-key", key)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("Embedding failed ({})", res.status()));
    }
    let v: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
    let values: Vec<f32> = v["embedding"]["values"].as_array().map(|a| a.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect()).unwrap_or_default();
    if values.is_empty() {
        return Err("Embedding came back empty.".into());
    }
    Ok(normalize(values))
}

static EMBEDDING: AtomicBool = AtomicBool::new(false);

/// Give every memory that has no embedding yet one. Quietly stops at the first failure (no key,
/// offline); it runs again next time a memory is added or searched.
pub async fn embed_missing(app: AppHandle) {
    let key = settings::load(&app).gemini_api_key;
    if key.is_empty() || EMBEDDING.swap(true, Ordering::SeqCst) {
        return;
    }
    loop {
        let batch: Vec<(i64, String)> = match db(&app).and_then(|db| {
            let mut st = db.prepare("SELECT id, text FROM memories WHERE embedding IS NULL LIMIT 40").map_err(|e| e.to_string())?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|e| e.to_string())?;
            Ok(rows.flatten().collect())
        }) {
            Ok(b) => b,
            Err(_) => break,
        };
        if batch.is_empty() {
            break;
        }
        let mut failed = false;
        for (id, text) in batch {
            match embed(&key, &text, "RETRIEVAL_DOCUMENT").await {
                Ok(v) => {
                    if let Ok(db) = db(&app) {
                        let _ = db.execute("UPDATE memories SET embedding = ?1 WHERE id = ?2 AND text = ?3", params![to_bytes(&v), id, text]);
                    }
                }
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        if failed {
            break;
        }
    }
    EMBEDDING.store(false, Ordering::SeqCst);
}

fn spawn_embed(app: &AppHandle) {
    tauri::async_runtime::spawn(embed_missing(app.clone()));
}

/// Memories closest in meaning to the question, best first.
fn semantic_in(db: &Connection, query: &[f32], workspace: &str, limit: usize) -> Result<Vec<Memory>, String> {
    let scope = "(?1 = '' OR m.workspace = ?1 OR m.workspace = '')";
    let sql = format!("SELECT {COLS}, m.embedding FROM memories m WHERE m.embedding IS NOT NULL AND {scope}");
    let mut st = db.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = st
        .query_map(params![workspace], |r| Ok((row(r)?, r.get::<_, Vec<u8>>(7)?)))
        .map_err(|e| e.to_string())?;
    let mut scored: Vec<(f32, Memory)> = rows
        .flatten()
        .map(|(m, e)| (dot(query, &from_bytes(&e)), m))
        .filter(|(sim, _)| *sim >= MIN_SIMILARITY)
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok(scored.into_iter().take(limit).map(|(_, m)| m).collect())
}

/// Merge two rankings (reciprocal rank fusion): things both lists like come first.
fn fuse(keyword: Vec<Memory>, meaning: Vec<Memory>, limit: usize) -> Vec<Memory> {
    let mut score: std::collections::HashMap<i64, (f32, Memory)> = std::collections::HashMap::new();
    for list in [keyword, meaning] {
        for (rank, m) in list.into_iter().enumerate() {
            let add = 1.0 / (60.0 + rank as f32);
            score.entry(m.id).and_modify(|e| e.0 += add).or_insert((add, m));
        }
    }
    let mut all: Vec<(f32, Memory)> = score.into_values().collect();
    all.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(b.1.pinned.cmp(&a.1.pinned)));
    all.into_iter().take(limit).map(|(_, m)| m).collect()
}

fn add_in(db: &Connection, kind: &str, text: &str, workspace: &str, source: &str) -> Result<Memory, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("There's nothing to remember.".into());
    }
    let kind = if KINDS.contains(&kind) { kind } else { "fact" };
    // The same thing said twice is one memory.
    let same: Option<i64> = db
        .query_row("SELECT id FROM memories WHERE lower(text) = lower(?1) AND workspace = ?2", params![text, workspace], |r| r.get(0))
        .ok();
    let id = match same {
        Some(id) => id,
        None => {
            db.execute("INSERT INTO memories(kind, text, workspace, source, created) VALUES (?1, ?2, ?3, ?4, ?5)", params![kind, text, workspace, source, now()])
                .map_err(|e| e.to_string())?;
            db.last_insert_rowid()
        }
    };
    db.query_row(&format!("SELECT {COLS} FROM memories m WHERE m.id = ?1"), params![id], row).map_err(|e| e.to_string())
}

fn search_in(db: &Connection, query: &str, workspace: &str, limit: usize) -> Result<Vec<Memory>, String> {
    let limit = limit.clamp(1, 50) as i64;
    let scope = "(?2 = '' OR m.workspace = ?2 OR m.workspace = '')";
    let found = match fts_query(query) {
        Some(fts) => {
            let sql = format!(
                "SELECT {COLS} FROM memories_fts f JOIN memories m ON m.id = f.rowid WHERE memories_fts MATCH ?1 AND {scope} ORDER BY m.pinned DESC, bm25(memories_fts), m.created DESC LIMIT ?3"
            );
            let mut st = db.prepare(&sql).map_err(|e| e.to_string())?;
            let rows = st.query_map(params![fts, workspace, limit], row).map_err(|e| e.to_string())?;
            rows.flatten().collect::<Vec<_>>()
        }
        None => vec![],
    };
    if !found.is_empty() || query.trim().chars().count() >= 3 && fts_query(query).is_some() {
        return Ok(found);
    }
    // Too short for the index: plain substring match.
    let like = format!("%{}%", query.trim().replace('%', "").replace('_', ""));
    let sql = format!("SELECT {COLS} FROM memories m WHERE m.text LIKE ?1 AND {scope} ORDER BY m.pinned DESC, m.created DESC LIMIT ?3");
    let mut st = db.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = st.query_map(params![like, workspace, limit], row).map_err(|e| e.to_string())?;
    Ok(rows.flatten().collect())
}

#[tauri::command]
pub fn memory_add(app: AppHandle, kind: String, text: String, workspace: Option<String>, source: Option<String>) -> Result<Memory, String> {
    let m = add_in(&db(&app)?, &kind, &text, workspace.as_deref().unwrap_or(""), source.as_deref().unwrap_or("user"))?;
    spawn_embed(&app);
    Ok(m)
}

/// Memories matching some words or the meaning of the question, most relevant first.
/// `workspace` also includes the global ones.
#[tauri::command]
pub async fn memory_search(app: AppHandle, query: String, workspace: Option<String>, limit: Option<usize>) -> Result<Vec<Memory>, String> {
    let ws = workspace.unwrap_or_default();
    let limit = limit.unwrap_or(8);
    let keyword = search_in(&db(&app)?, &query, &ws, limit)?;
    let key = settings::load(&app).gemini_api_key;
    if key.is_empty() || query.trim().chars().count() < 3 {
        return Ok(keyword);
    }
    spawn_embed(&app);
    let Ok(q) = embed(&key, query.trim(), "RETRIEVAL_QUERY").await else { return Ok(keyword) };
    let meaning = semantic_in(&db(&app)?, &q, &ws, limit.clamp(1, 50))?;
    Ok(fuse(keyword, meaning, limit.clamp(1, 50)))
}

/// Newest first, for the Memory page. Empty `workspace` lists everything.
#[tauri::command]
pub fn memory_list(app: AppHandle, workspace: Option<String>, kind: Option<String>, limit: Option<usize>) -> Result<Vec<Memory>, String> {
    let db = db(&app)?;
    let ws = workspace.unwrap_or_default();
    let kind = kind.unwrap_or_default();
    let sql = format!(
        "SELECT {COLS} FROM memories m WHERE (?1 = '' OR m.workspace = ?1) AND (?2 = '' OR m.kind = ?2) ORDER BY m.pinned DESC, m.created DESC, m.id DESC LIMIT ?3"
    );
    let mut st = db.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = st.query_map(params![ws, kind, limit.unwrap_or(200).clamp(1, 1000) as i64], row).map_err(|e| e.to_string())?;
    Ok(rows.flatten().collect())
}

/// What goes into Jarvis's instructions at the start of a session: pinned memories, preferences
/// and the latest facts, for this workspace and for everywhere.
#[tauri::command]
pub fn memory_brief(app: AppHandle, workspace: Option<String>, limit: Option<usize>) -> Result<Vec<Memory>, String> {
    let db = db(&app)?;
    let ws = workspace.unwrap_or_default();
    let sql = format!(
        "SELECT {COLS} FROM memories m WHERE (m.workspace = '' OR m.workspace = ?1) ORDER BY m.pinned DESC, (m.kind = 'preference') DESC, m.created DESC, m.id DESC LIMIT ?2"
    );
    let mut st = db.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = st.query_map(params![ws, limit.unwrap_or(25).clamp(1, 100) as i64], row).map_err(|e| e.to_string())?;
    Ok(rows.flatten().collect())
}

#[tauri::command]
pub fn memory_update(app: AppHandle, id: i64, text: Option<String>, kind: Option<String>, pinned: Option<bool>, workspace: Option<String>) -> Result<(), String> {
    let db = db(&app)?;
    if let Some(t) = text.filter(|t| !t.trim().is_empty()) {
        db.execute("UPDATE memories SET text = ?1, embedding = NULL WHERE id = ?2", params![t.trim(), id]).map_err(|e| e.to_string())?;
        spawn_embed(&app);
    }
    if let Some(k) = kind.filter(|k| KINDS.contains(&k.as_str())) {
        db.execute("UPDATE memories SET kind = ?1 WHERE id = ?2", params![k, id]).map_err(|e| e.to_string())?;
    }
    if let Some(p) = pinned {
        db.execute("UPDATE memories SET pinned = ?1 WHERE id = ?2", params![p as i64, id]).map_err(|e| e.to_string())?;
    }
    if let Some(w) = workspace {
        db.execute("UPDATE memories SET workspace = ?1 WHERE id = ?2", params![w, id]).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn memory_delete(app: AppHandle, id: i64) -> Result<(), String> {
    db(&app)?.execute("DELETE FROM memories WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    Ok(())
}

/// Bring in the lines of the old notes.md once, so nothing the user asked Jarvis to remember is lost.
pub fn import_notes(app: &AppHandle) {
    let Ok(db) = db(app) else { return };
    let done: Option<String> = db.query_row("SELECT value FROM meta WHERE key = 'notes_imported'", [], |r| r.get(0)).ok();
    if done.is_some() {
        return;
    }
    let notes = std::fs::read_to_string(settings::research_root(app).join("notes.md")).unwrap_or_default();
    for line in notes.lines() {
        let text = line.trim().trim_start_matches("- ");
        let text = text.split("<!--").next().unwrap_or("").trim();
        if !text.is_empty() {
            let _ = add_in(&db, "note", text, "", "notes.md");
        }
    }
    let _ = db.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('notes_imported', '1')", []);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> Connection {
        open(std::path::Path::new(":memory:")).unwrap()
    }

    #[test]
    fn remembers_and_finds_by_part_of_a_word() {
        let db = temp();
        add_in(&db, "decision", "We decided the Fikra pricing is NPR 4,999 per month", "fikra", "chat:1").unwrap();
        add_in(&db, "person", "Sita Rai is the client contact at Omega Edu Park", "", "user").unwrap();
        let hits = search_in(&db, "what did we decide about pricing", "fikra", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].text.contains("4,999"));
        // Global memories show in every workspace; another workspace's don't.
        assert_eq!(search_in(&db, "Sita", "gsoft", 5).unwrap().len(), 1);
        assert_eq!(search_in(&db, "pricing", "gsoft", 5).unwrap().len(), 0);
    }

    #[test]
    fn nepali_text_is_searchable() {
        let db = temp();
        add_in(&db, "fact", "मेरो कम्पनीको नाम फिक्रा भेन्चर्स हो", "", "user").unwrap();
        assert_eq!(search_in(&db, "फिक्रा", "", 5).unwrap().len(), 1);
    }

    #[test]
    fn the_same_memory_is_stored_once() {
        let db = temp();
        let a = add_in(&db, "fact", "Kaushal prefers short answers", "", "user").unwrap();
        let b = add_in(&db, "fact", "kaushal prefers short answers", "", "chat:2").unwrap();
        assert_eq!(a.id, b.id);
    }

    #[test]
    fn short_queries_and_punctuation_do_not_break_search() {
        let db = temp();
        add_in(&db, "fact", "Port 80 is closed on the office router", "", "user").unwrap();
        assert_eq!(search_in(&db, "80", "", 5).unwrap().len(), 1);
        assert!(search_in(&db, "\"; DROP TABLE memories; --", "", 5).is_ok());
        assert_eq!(search_in(&db, "router?", "", 5).unwrap().len(), 1);
    }

    #[test]
    fn edits_and_deletes_keep_the_index_in_step() {
        let db = temp();
        let m = add_in(&db, "fact", "The demo is on Friday", "", "user").unwrap();
        db.execute("UPDATE memories SET text = 'The demo is on Monday' WHERE id = ?1", params![m.id]).unwrap();
        assert_eq!(search_in(&db, "Friday", "", 5).unwrap().len(), 0);
        assert_eq!(search_in(&db, "Monday", "", 5).unwrap().len(), 1);
        db.execute("DELETE FROM memories WHERE id = ?1", params![m.id]).unwrap();
        assert_eq!(search_in(&db, "Monday", "", 5).unwrap().len(), 0);
    }

    #[test]
    fn finds_by_meaning_and_merges_rankings() {
        let db = temp();
        let a = add_in(&db, "decision", "Monthly subscription is NPR 4,999", "", "user").unwrap();
        let b = add_in(&db, "fact", "The office is in Lalitpur", "", "user").unwrap();
        let unit = |x: f32, y: f32| normalize(vec![x, y]);
        db.execute("UPDATE memories SET embedding = ?1 WHERE id = ?2", params![to_bytes(&unit(1.0, 0.1)), a.id]).unwrap();
        db.execute("UPDATE memories SET embedding = ?1 WHERE id = ?2", params![to_bytes(&unit(0.0, 1.0)), b.id]).unwrap();
        let hits = semantic_in(&db, &unit(1.0, 0.0), "", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, a.id);
        // Something both rankings like beats something only one does.
        let fused = fuse(vec![b.clone(), a.clone()], vec![a.clone()], 5);
        assert_eq!(fused[0].id, a.id);
        assert_eq!(fused.len(), 2);
    }

    #[test]
    fn embeddings_survive_storage() {
        let v = normalize(vec![3.0, 4.0]);
        assert_eq!(from_bytes(&to_bytes(&v)), v);
        assert!((dot(&v, &v) - 1.0).abs() < 1e-6);
    }
}
