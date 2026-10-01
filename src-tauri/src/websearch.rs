//! Google Search for quick factual questions, through Gemini's search grounding. It uses the
//! Gemini key Jarvis already has: no extra sign-in or scope. Deep research stays with Codex.

use crate::settings;
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;
use tauri::AppHandle;

const MODEL: &str = "gemini-2.5-flash";
const MAX_SOURCES: usize = 6;

#[derive(Serialize, Debug, PartialEq)]
pub struct Source {
    title: String,
    uri: String,
}

#[derive(Serialize)]
pub struct Answer {
    answer: String,
    sources: Vec<Source>,
    queries: Vec<String>,
}

/// The answer text, the pages it leaned on, and the searches Google ran, from a grounded reply.
fn parse_answer(v: &Value) -> Answer {
    let cand = &v["candidates"][0];
    let answer = cand["content"]["parts"]
        .as_array()
        .map(|p| p.iter().filter_map(|x| x["text"].as_str()).collect::<Vec<_>>().join(""))
        .unwrap_or_default()
        .trim()
        .to_string();
    let meta = &cand["groundingMetadata"];
    let mut sources: Vec<Source> = vec![];
    for c in meta["groundingChunks"].as_array().into_iter().flatten() {
        let uri = c["web"]["uri"].as_str().unwrap_or("");
        if uri.is_empty() || sources.iter().any(|s| s.uri == uri) {
            continue;
        }
        sources.push(Source { title: c["web"]["title"].as_str().unwrap_or(uri).to_string(), uri: uri.to_string() });
        if sources.len() == MAX_SOURCES {
            break;
        }
    }
    let queries = meta["webSearchQueries"].as_array().map(|a| a.iter().filter_map(|q| q.as_str().map(String::from)).collect()).unwrap_or_default();
    Answer { answer, sources, queries }
}

/// Ask Google Search a question and get a short answer with its sources.
#[tauri::command]
pub async fn web_search(app: AppHandle, query: String) -> Result<Answer, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("Nothing to search for.".into());
    }
    let key = settings::load(&app).gemini_api_key;
    if key.is_empty() {
        return Err("Add your Gemini API key in Settings to use Google Search.".into());
    }
    let today = chrono::Local::now().format("%A %-d %B %Y");
    let body = json!({
        "systemInstruction": { "parts": [{ "text": format!("Today is {today}. Answer the question in at most a short paragraph, with the facts and figures asked for. Use the search results; say so if they disagree or don't answer it.") }] },
        "contents": [{ "role": "user", "parts": [{ "text": query }] }],
        "tools": [{ "google_search": {} }],
    });
    let res = reqwest::Client::builder()
        .timeout(Duration::from_secs(40))
        .build()
        .map_err(|e| e.to_string())?
        .post(format!("https://generativelanguage.googleapis.com/v1beta/models/{MODEL}:generateContent"))
        .header("x-goog-api-key", key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Couldn't reach Google: {e}"))?;
    if !res.status().is_success() {
        let status = res.status();
        let v: Value = res.json().await.unwrap_or(Value::Null);
        return Err(format!("Google Search failed ({status}): {}", v["error"]["message"].as_str().unwrap_or("")));
    }
    let v: Value = res.json().await.map_err(|e| e.to_string())?;
    let a = parse_answer(&v);
    if a.answer.is_empty() {
        return Err("Google Search came back with no answer.".into());
    }
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answer_collects_text_and_unique_sources() {
        let v = json!({ "candidates": [{
            "content": { "parts": [{ "text": "Kathmandu is " }, { "text": "the capital." }] },
            "groundingMetadata": {
                "webSearchQueries": ["capital of nepal"],
                "groundingChunks": [
                    { "web": { "uri": "https://a.example", "title": "A" } },
                    { "web": { "uri": "https://a.example", "title": "A again" } },
                    { "web": { "uri": "https://b.example" } }
                ]
            }
        }]});
        let a = parse_answer(&v);
        assert_eq!(a.answer, "Kathmandu is the capital.");
        assert_eq!(a.sources.len(), 2);
        assert_eq!(a.sources[1], Source { title: "https://b.example".into(), uri: "https://b.example".into() });
        assert_eq!(a.queries, vec!["capital of nepal"]);
    }

    #[test]
    fn empty_reply_has_no_answer() {
        assert!(parse_answer(&json!({})).answer.is_empty());
    }
}
