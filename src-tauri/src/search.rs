//! One search across conversations, documents, reports and images.

use crate::chats;
use crate::tasks::{self, Status, DOCUMENT_FILE};
use serde::Serialize;
use std::path::Path;
use tauri::AppHandle;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    /// "chat", "document", "research" or "image".
    kind: String,
    /// The conversation, for chats (and the one a task came from).
    chat_id: String,
    /// The task, for documents, reports and images.
    task_id: Option<u32>,
    title: String,
    /// A short piece of text around the match; empty when only the title matched.
    snippet: String,
    at: u64,
    in_title: bool,
}

/// Characters of context on each side of a match.
const CONTEXT: usize = 70;
const MAX_HITS: usize = 60;

/// A short, readable piece of `text` around the first match of `needle` (already lowercase).
fn snippet(text: &str, needle: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let at = lower.find(needle)?;
    let end_at = at + needle.len();
    // Lowercasing can shift byte positions in a few scripts. Then show the lowercase text rather
    // than risk cutting a character in half.
    let aligned = lower.len() == text.len() && text.is_char_boundary(at) && text.is_char_boundary(end_at);
    let src = if aligned { text } else { lower.as_str() };
    let start = src[..at].char_indices().rev().nth(CONTEXT).map(|(i, _)| i).unwrap_or(0);
    let end = src[end_at..].char_indices().nth(CONTEXT).map(|(i, _)| end_at + i).unwrap_or(src.len());
    // Markdown punctuation reads as noise in a one-line preview.
    let body: String = src[start..end].chars().map(|c| if matches!(c, '#' | '*' | '|' | '`' | '>') { ' ' } else { c }).collect();
    let body = body.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(format!("{}{}{}", if start > 0 { "…" } else { "" }, body, if end < src.len() { "…" } else { "" }))
}

#[tauri::command]
pub fn search(app: AppHandle, query: String) -> Vec<Hit> {
    let needle = query.trim().to_lowercase();
    if needle.chars().count() < 2 {
        return vec![];
    }
    let mut hits = vec![];

    // Conversations: the title, then anything that was said.
    for chat in chats::all_chats(&app) {
        let in_title = chat.title.to_lowercase().contains(&needle);
        let said = chat.messages.iter().filter(|m| m.who == "you" || m.who == "jarvis").find_map(|m| snippet(&m.text, &needle));
        if in_title || said.is_some() {
            let title = if chat.title.is_empty() { "New chat".to_string() } else { chat.title.clone() };
            hits.push(Hit { kind: "chat".into(), chat_id: chat.id, task_id: None, title, snippet: said.unwrap_or_default(), at: chat.updated_at, in_title });
        }
    }

    // Finished work: the report or document itself, then what was asked for and the summary.
    // Edits and follow-ups share their original's file, so only originals are listed.
    for t in tasks::snapshot(&app) {
        if t.parent_id.is_some() || t.status != Status::Done {
            continue;
        }
        let file = match t.kind.as_str() {
            "document" => Some(DOCUMENT_FILE),
            "research" | "browser" => Some("report.md"),
            _ => None,
        };
        let body = file.and_then(|f| std::fs::read_to_string(Path::new(&t.dir).join(f)).ok()).unwrap_or_default();
        let in_title = t.title.to_lowercase().contains(&needle);
        let found = snippet(&body, &needle).or_else(|| snippet(&t.request, &needle)).or_else(|| snippet(&t.summary, &needle));
        if in_title || found.is_some() {
            hits.push(Hit {
                kind: t.kind.clone(),
                chat_id: t.chat_id.clone(),
                task_id: Some(t.id),
                title: t.title.clone(),
                snippet: found.unwrap_or_default(),
                at: t.finished_at.unwrap_or(t.started_at),
                in_title,
            });
        }
    }

    // Title matches first, then the most recent.
    hits.sort_by(|a, b| b.in_title.cmp(&a.in_title).then(b.at.cmp(&a.at)));
    hits.truncate(MAX_HITS);
    hits
}

#[cfg(test)]
mod tests {
    use super::snippet;

    #[test]
    fn snippets_show_the_match_in_context() {
        let text = "## Budget\n\nThe **venue** costs NPR 40,000 per night. ".repeat(1) + &"filler ".repeat(40) + "Tail.";
        let s = snippet(&text, "venue").unwrap();
        assert!(s.contains("venue costs NPR 40,000"), "{s}");
        assert!(!s.contains('#') && !s.contains('*'), "markdown left in: {s}");
        assert!(s.ends_with('…'), "should show it continues: {s}");
        assert!(snippet(&text, "missing").is_none());
        assert!(snippet("Revenue Grew", "revenue grew").is_some(), "case-insensitive");
        let nepali = "योजना: पोखरामा टोली भेला हुनेछ।";
        assert!(snippet(nepali, "पोखरामा").unwrap().contains("पोखरामा टोली"));
    }
}
