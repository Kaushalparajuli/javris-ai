//! Google Docs: create documents from Markdown, read them, and add to them.
//!
//! Docs addresses text by UTF-16 position, so the Markdown is turned into one insert followed by
//! style requests whose positions are counted in UTF-16 units.

use crate::google::{api, url_with, with_scope_hint};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::AppHandle;

const DOCS: &str = "https://docs.googleapis.com/v1/documents";
const MAX_TEXT: usize = 60_000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocRef {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) link: String,
}

fn doc_ref(v: &Value) -> DocRef {
    let id = v["documentId"].as_str().unwrap_or("").to_string();
    DocRef { link: format!("https://docs.google.com/document/d/{id}/edit"), id, title: v["title"].as_str().unwrap_or("").into() }
}

fn id_ok(id: &str) -> Result<&str, String> {
    if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')) {
        Ok(id)
    } else {
        Err("That isn't a Google Doc id.".into())
    }
}

// ---------- Markdown to Docs ----------

#[derive(Clone, Copy, PartialEq)]
enum Block {
    Para,
    Heading(u8),
    Bullet,
    Numbered,
}

enum Mark {
    Bold,
    Italic,
    Link(String),
}

/// Strip inline Markdown (`**bold**`, `*italic*`, `[text](url)`, `` `code` ``) from a line,
/// returning plain text and the marked character ranges (in chars of the plain text).
fn inline(line: &str) -> (String, Vec<(usize, usize, Mark)>) {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut n = 0usize;
    let mut marks = vec![];
    let mut i = 0;
    let find = |from: usize, pat: &[char]| -> Option<usize> { (from..chars.len().saturating_sub(pat.len() - 1)).find(|&k| chars[k..k + pat.len()] == *pat) };
    while i < chars.len() {
        let c = chars[i];
        if c == '*' && chars.get(i + 1) == Some(&'*') {
            if let Some(end) = find(i + 2, &['*', '*']).filter(|&e| e > i + 2) {
                let (inner, _) = inline(&chars[i + 2..end].iter().collect::<String>());
                let len = inner.chars().count();
                out.push_str(&inner);
                marks.push((n, n + len, Mark::Bold));
                n += len;
                i = end + 2;
                continue;
            }
        } else if c == '*' && chars.get(i + 1).is_some_and(|x| !x.is_whitespace() && *x != '*') {
            if let Some(end) = find(i + 1, &['*']).filter(|&e| e > i + 1 && !chars[e - 1].is_whitespace()) {
                let inner: String = chars[i + 1..end].iter().collect();
                let len = inner.chars().count();
                out.push_str(&inner);
                marks.push((n, n + len, Mark::Italic));
                n += len;
                i = end + 1;
                continue;
            }
        } else if c == '`' {
            if let Some(end) = find(i + 1, &['`']).filter(|&e| e > i + 1) {
                let inner: String = chars[i + 1..end].iter().collect();
                n += inner.chars().count();
                out.push_str(&inner);
                i = end + 1;
                continue;
            }
        } else if c == '[' {
            if let Some(close) = find(i + 1, &[']']).filter(|&e| chars.get(e + 1) == Some(&'(')) {
                if let Some(paren) = find(close + 2, &[')']) {
                    let label: String = chars[i + 1..close].iter().collect();
                    let url: String = chars[close + 2..paren].iter().collect();
                    let len = label.chars().count();
                    out.push_str(&label);
                    if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:") {
                        marks.push((n, n + len, Mark::Link(url)));
                    }
                    n += len;
                    i = paren + 1;
                    continue;
                }
            }
        }
        out.push(c);
        n += 1;
        i += 1;
    }
    (out, marks)
}

fn classify(line: &str) -> (Block, &str) {
    let t = line.trim_start();
    let hashes = t.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') {
        return (Block::Heading(hashes as u8), t[hashes + 1..].trim());
    }
    if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("• ")) {
        return (Block::Bullet, rest);
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") ")) {
        return (Block::Numbered, &t[digits + 2..]);
    }
    (Block::Para, t)
}

fn utf16(s: &str) -> usize {
    s.encode_utf16().count()
}

fn range(a: usize, b: usize) -> Value {
    json!({ "startIndex": a, "endIndex": b })
}

/// The `batchUpdate` requests that put `md` into a document at UTF-16 position `at`.
fn markdown_requests(md: &str, at: usize) -> Vec<Value> {
    let mut text = String::new();
    let mut pos = at;
    let mut styles: Vec<Value> = vec![];
    // Paragraph ranges of list runs, merged so a list is one list.
    let mut lists: Vec<(Block, usize, usize)> = vec![];
    for raw in md.lines() {
        // Blank lines are dropped: Docs spaces paragraphs itself.
        if raw.trim().is_empty() {
            continue;
        }
        if raw.trim_start().starts_with("---") && raw.trim().chars().all(|c| c == '-') {
            continue;
        }
        let (block, body) = classify(raw);
        let (plain, marks) = inline(body);
        let start = pos;
        // Positions inside the line, in UTF-16 units.
        let chars: Vec<char> = plain.chars().collect();
        let off = |k: usize| start + utf16(&chars[..k.min(chars.len())].iter().collect::<String>());
        for (a, b, m) in &marks {
            let (a, b) = (off(*a), off(*b));
            if a >= b {
                continue;
            }
            let (style, fields) = match m {
                Mark::Bold => (json!({ "bold": true }), "bold"),
                Mark::Italic => (json!({ "italic": true }), "italic"),
                Mark::Link(u) => (json!({ "link": { "url": u } }), "link"),
            };
            styles.push(json!({ "updateTextStyle": { "range": range(a, b), "textStyle": style, "fields": fields } }));
        }
        let end = start + utf16(&plain) + 1;
        match block {
            Block::Heading(n) => styles.push(json!({ "updateParagraphStyle": {
                "range": range(start, end),
                "paragraphStyle": { "namedStyleType": format!("HEADING_{}", n.min(6)) },
                "fields": "namedStyleType"
            }})),
            Block::Bullet | Block::Numbered => match lists.last_mut() {
                Some((b, _, e)) if *b == block && *e == start => *e = end,
                _ => lists.push((block, start, end)),
            },
            Block::Para => {}
        }
        text.push_str(&plain);
        text.push('\n');
        pos = end;
    }
    let mut reqs = vec![];
    if text.is_empty() {
        return reqs;
    }
    reqs.push(json!({ "insertText": { "location": { "index": at }, "text": text } }));
    // Inserted text takes on the style around it; reset it so an append doesn't inherit a heading.
    reqs.push(json!({ "updateParagraphStyle": { "range": range(at, pos), "paragraphStyle": { "namedStyleType": "NORMAL_TEXT" }, "fields": "namedStyleType" } }));
    reqs.extend(styles);
    for (b, s, e) in lists {
        let preset = if b == Block::Bullet { "BULLET_DISC_CIRCLE_SQUARE" } else { "NUMBERED_DECIMAL_ALPHA_ROMAN" };
        reqs.push(json!({ "createParagraphBullets": { "range": range(s, e), "bulletPreset": preset } }));
    }
    reqs
}

// ---------- reading ----------

fn push_text(elements: &Value, out: &mut String) {
    for el in elements.as_array().into_iter().flatten() {
        if let Some(p) = el["paragraph"]["elements"].as_array() {
            for e in p {
                if let Some(t) = e["textRun"]["content"].as_str() {
                    out.push_str(t);
                }
            }
        } else if let Some(rows) = el["table"]["tableRows"].as_array() {
            for r in rows {
                let cells: Vec<String> = r["tableCells"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| {
                        let mut s = String::new();
                        push_text(&c["content"], &mut s);
                        s.trim().replace('\n', " ")
                    })
                    .collect();
                out.push_str(&cells.join(" | "));
                out.push('\n');
            }
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocText {
    id: String,
    title: String,
    link: String,
    text: String,
    truncated: bool,
}

#[tauri::command]
pub async fn doc_read(app: AppHandle, id: String) -> Result<DocText, String> {
    let id = id_ok(&id)?;
    let v = api(&app, reqwest::Method::GET, &format!("{DOCS}/{id}"), None).await.map_err(|e| with_scope_hint(e, "Docs"))?;
    let mut text = String::new();
    push_text(&v["body"]["content"], &mut text);
    let truncated = text.len() > MAX_TEXT;
    if truncated {
        let mut end = MAX_TEXT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    let r = doc_ref(&v);
    Ok(DocText { id: r.id, title: r.title, link: r.link, text, truncated })
}

// ---------- writing ----------

async fn batch(app: &AppHandle, id: &str, requests: Vec<Value>) -> Result<Value, String> {
    if requests.is_empty() {
        return Ok(Value::Null);
    }
    api(app, reqwest::Method::POST, &format!("{DOCS}/{id}:batchUpdate"), Some(json!({ "requests": requests })))
        .await
        .map_err(|e| with_scope_hint(e, "Docs"))
}

/// Make a new Google Doc from Markdown.
#[tauri::command]
pub async fn doc_create(app: AppHandle, title: String, markdown: String) -> Result<DocRef, String> {
    create(&app, title, markdown).await
}

pub(crate) async fn create(app: &AppHandle, title: String, markdown: String) -> Result<DocRef, String> {
    let app = app.clone();
    let title = if title.trim().is_empty() { "Untitled".to_string() } else { title.trim().to_string() };
    let v = api(&app, reqwest::Method::POST, DOCS, Some(json!({ "title": title }))).await.map_err(|e| with_scope_hint(e, "Docs"))?;
    let r = doc_ref(&v);
    batch(&app, &r.id, markdown_requests(&markdown, 1)).await?;
    Ok(r)
}

/// Add Markdown to the end of a Doc.
#[tauri::command]
pub async fn doc_append(app: AppHandle, id: String, markdown: String) -> Result<DocRef, String> {
    let id = id_ok(&id)?.to_string();
    let url = url_with(&format!("{DOCS}/{id}"), &[("fields", "documentId,title,body.content.endIndex")]);
    let v = api(&app, reqwest::Method::GET, &url, None).await.map_err(|e| with_scope_hint(e, "Docs"))?;
    // The body always ends with a newline Docs won't let you write past.
    let end = v["body"]["content"].as_array().and_then(|c| c.last()).and_then(|e| e["endIndex"].as_u64()).unwrap_or(2) as usize;
    batch(&app, &id, markdown_requests(&markdown, end.saturating_sub(1).max(1))).await?;
    Ok(doc_ref(&v))
}

/// Replace every occurrence of some text in a Doc. Returns how many were changed.
#[tauri::command]
pub async fn doc_replace_text(app: AppHandle, id: String, find: String, replace: String) -> Result<u64, String> {
    let id = id_ok(&id)?.to_string();
    if find.is_empty() {
        return Err("Nothing to find.".into());
    }
    let req = json!({ "replaceAllText": { "containsText": { "text": find, "matchCase": true }, "replaceText": replace } });
    let v = batch(&app, &id, vec![req]).await?;
    Ok(v["replies"][0]["replaceAllText"]["occurrencesChanged"].as_u64().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_marks_use_plain_text_positions() {
        let (t, m) = inline("Say **hi** to [Sam](https://x.example) now");
        assert_eq!(t, "Say hi to Sam now");
        assert_eq!(m.len(), 2);
        assert_eq!((m[0].0, m[0].1), (4, 6));
        assert_eq!((m[1].0, m[1].1), (10, 13));
        assert!(matches!(&m[1].2, Mark::Link(u) if u == "https://x.example"));
    }

    #[test]
    fn unclosed_marks_stay_as_text() {
        assert_eq!(inline("2 * 3 and **open").0, "2 * 3 and **open");
        assert_eq!(inline("use `code` here").0, "use code here");
    }

    #[test]
    fn javascript_links_are_not_linked() {
        let (t, m) = inline("[x](javascript:void)");
        assert_eq!(t, "x");
        assert!(m.is_empty());
    }

    #[test]
    fn blocks_are_classified() {
        assert!(matches!(classify("## Plan"), (Block::Heading(2), "Plan")));
        assert!(matches!(classify("- one"), (Block::Bullet, "one")));
        assert!(matches!(classify("12. twelve"), (Block::Numbered, "twelve")));
        assert!(matches!(classify("#hashtag"), (Block::Para, _)));
    }

    #[test]
    fn requests_count_in_utf16() {
        // "नमस्ते" is 6 chars; emoji is 2 UTF-16 units.
        let reqs = markdown_requests("# नमस्ते 😀\n\n- **a**\n- b\n\n1. c", 1);
        assert_eq!(reqs[0]["insertText"]["text"], "नमस्ते 😀\na\nb\nc\n");
        let heading = reqs.iter().find(|r| r.get("updateParagraphStyle").is_some_and(|u| u["paragraphStyle"]["namedStyleType"] == "HEADING_1")).unwrap();
        // 6 chars + space + 2 units + newline = 10 units from index 1.
        assert_eq!(heading["updateParagraphStyle"]["range"], json!({ "startIndex": 1, "endIndex": 11 }));
        let bullets: Vec<_> = reqs.iter().filter(|r| r.get("createParagraphBullets").is_some()).collect();
        assert_eq!(bullets.len(), 2, "one bullet list and one numbered list");
        assert_eq!(bullets[0]["createParagraphBullets"]["range"], json!({ "startIndex": 11, "endIndex": 15 }));
    }

    #[test]
    fn empty_markdown_makes_no_requests() {
        assert!(markdown_requests("  \n\n", 1).is_empty());
    }

    #[test]
    fn doc_text_includes_tables() {
        let body = json!([
            { "paragraph": { "elements": [{ "textRun": { "content": "Hello\n" } }] } },
            { "table": { "tableRows": [{ "tableCells": [
                { "content": [{ "paragraph": { "elements": [{ "textRun": { "content": "A\n" } }] } }] },
                { "content": [{ "paragraph": { "elements": [{ "textRun": { "content": "B\n" } }] } }] }
            ] }] } }
        ]);
        let mut s = String::new();
        push_text(&body, &mut s);
        assert_eq!(s, "Hello\nA | B\n");
    }

    #[test]
    fn ids_are_checked() {
        assert!(id_ok("1AbC-_9").is_ok());
        assert!(id_ok("../x").is_err());
        assert!(id_ok("").is_err());
    }
}
