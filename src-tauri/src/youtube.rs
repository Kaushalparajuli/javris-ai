//! YouTube Data API: search videos, channels and playlists, look at a video, and read the
//! user's own subscriptions and liked videos. Read-only.
//!
//! Search costs 100 of the 10,000 daily quota units, so it is capped per run of the app.
//! Transcripts aren't part of the official API for other people's videos and aren't offered.

use crate::google::{api, url_with, with_scope_hint};
use serde::Serialize;
use serde_json::Value;
use std::sync::atomic::{AtomicU32, Ordering};
use tauri::AppHandle;

const BASE: &str = "https://www.googleapis.com/youtube/v3";
/// Searches allowed per run of the app: 60 x 100 units uses 60% of a day's default quota.
const SEARCH_BUDGET: u32 = 60;
static SEARCHES: AtomicU32 = AtomicU32::new(0);

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// "video", "channel" or "playlist".
    kind: String,
    id: String,
    title: String,
    channel: String,
    published: String,
    description: String,
    link: String,
    thumbnail: String,
}

fn link_for(kind: &str, id: &str) -> String {
    match kind {
        "channel" => format!("https://www.youtube.com/channel/{id}"),
        "playlist" => format!("https://www.youtube.com/playlist?list={id}"),
        _ => format!("https://www.youtube.com/watch?v={id}"),
    }
}

fn clip(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n { format!("{t}…") } else { t }
}

/// Search results carry the kind and id inside `id`; list endpoints put it elsewhere.
fn item_from(v: &Value) -> Option<Item> {
    let s = &v["snippet"];
    let (kind, id) = if let Some(k) = v["id"]["kind"].as_str() {
        let kind = k.strip_prefix("youtube#")?;
        (kind.to_string(), v["id"][format!("{kind}Id")].as_str()?.to_string())
    } else if let Some(vid) = s["resourceId"]["videoId"].as_str() {
        ("video".to_string(), vid.to_string())
    } else if let Some(ch) = s["resourceId"]["channelId"].as_str() {
        ("channel".to_string(), ch.to_string())
    } else {
        ("video".to_string(), v["id"].as_str()?.to_string())
    };
    Some(Item {
        link: link_for(&kind, &id),
        kind,
        id,
        title: s["title"].as_str().unwrap_or("").into(),
        channel: s["channelTitle"].as_str().or(s["videoOwnerChannelTitle"].as_str()).unwrap_or("").into(),
        published: s["publishedAt"].as_str().unwrap_or("").into(),
        description: clip(s["description"].as_str().unwrap_or(""), 300),
        thumbnail: s["thumbnails"]["medium"]["url"].as_str().or(s["thumbnails"]["default"]["url"].as_str()).unwrap_or("").into(),
    })
}

fn items(v: &Value) -> Vec<Item> {
    v["items"].as_array().map(|a| a.iter().filter_map(item_from).collect()).unwrap_or_default()
}

fn clamp(max: Option<u32>) -> String {
    max.unwrap_or(10).clamp(1, 25).to_string()
}

/// Search YouTube. `kind` is "video" (default), "channel" or "playlist".
#[tauri::command]
pub async fn yt_search(app: AppHandle, query: String, max: Option<u32>, kind: Option<String>) -> Result<Vec<Item>, String> {
    if query.trim().is_empty() {
        return Err("Nothing to search for.".into());
    }
    if SEARCHES.fetch_add(1, Ordering::SeqCst) >= SEARCH_BUDGET {
        return Err("YouTube search limit reached for this session (it protects the daily quota). Restart Jarvis to search again.".into());
    }
    let kind = match kind.as_deref() {
        Some("channel") => "channel",
        Some("playlist") => "playlist",
        _ => "video",
    };
    let url = url_with(&format!("{BASE}/search"), &[("part", "snippet"), ("q", query.trim()), ("type", kind), ("maxResults", &clamp(max))]);
    let v = api(&app, reqwest::Method::GET, &url, None).await.map_err(|e| with_scope_hint(e, "YouTube"))?;
    Ok(items(&v))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Video {
    id: String,
    title: String,
    channel: String,
    published: String,
    /// Like "12:34" or "1:02:03".
    duration: String,
    views: String,
    likes: String,
    description: String,
    link: String,
}

/// ISO 8601 durations like PT1H2M3S, as clock time.
fn clock(iso: &str) -> String {
    let rest = iso.strip_prefix("PT").unwrap_or("");
    let (mut h, mut m, mut s, mut n) = (0u64, 0u64, 0u64, 0u64);
    for c in rest.chars() {
        match c {
            '0'..='9' => n = n * 10 + c.to_digit(10).unwrap() as u64,
            'H' => (h, n) = (n, 0),
            'M' => (m, n) = (n, 0),
            'S' => (s, n) = (n, 0),
            _ => {}
        }
    }
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

fn video_from(v: &Value) -> Video {
    let id = v["id"].as_str().unwrap_or("").to_string();
    Video {
        link: link_for("video", &id),
        title: v["snippet"]["title"].as_str().unwrap_or("").into(),
        channel: v["snippet"]["channelTitle"].as_str().unwrap_or("").into(),
        published: v["snippet"]["publishedAt"].as_str().unwrap_or("").into(),
        duration: clock(v["contentDetails"]["duration"].as_str().unwrap_or("")),
        views: v["statistics"]["viewCount"].as_str().unwrap_or("").into(),
        likes: v["statistics"]["likeCount"].as_str().unwrap_or("").into(),
        description: clip(v["snippet"]["description"].as_str().unwrap_or(""), 1500),
        id,
    }
}

/// Details of one video, from its id or a YouTube link.
#[tauri::command]
pub async fn yt_video(app: AppHandle, id: String) -> Result<Video, String> {
    let id = video_id(&id).ok_or("That isn't a YouTube video id or link.")?;
    let url = url_with(&format!("{BASE}/videos"), &[("part", "snippet,statistics,contentDetails"), ("id", &id)]);
    let v = api(&app, reqwest::Method::GET, &url, None).await.map_err(|e| with_scope_hint(e, "YouTube"))?;
    v["items"].get(0).map(video_from).ok_or_else(|| "No such video.".to_string())
}

/// The 11-character video id from an id or a youtube.com / youtu.be link.
fn video_id(s: &str) -> Option<String> {
    let s = s.trim();
    let ok = |t: &str| (t.len() == 11 && t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))).then(|| t.to_string());
    if let Some(id) = ok(s) {
        return Some(id);
    }
    let url = tauri::Url::parse(s).ok()?;
    let host = url.host_str()?.trim_start_matches("www.").trim_start_matches("m.");
    match host {
        "youtu.be" => ok(url.path().trim_start_matches('/')),
        "youtube.com" => url
            .query_pairs()
            .find(|(k, _)| k == "v")
            .and_then(|(_, v)| ok(&v))
            .or_else(|| url.path().strip_prefix("/shorts/").or(url.path().strip_prefix("/live/")).and_then(ok)),
        _ => None,
    }
}

/// The videos in a playlist, in order.
#[tauri::command]
pub async fn yt_playlist_items(app: AppHandle, playlist_id: String, max: Option<u32>) -> Result<Vec<Item>, String> {
    let url = url_with(&format!("{BASE}/playlistItems"), &[("part", "snippet"), ("playlistId", playlist_id.trim()), ("maxResults", &clamp(max))]);
    let v = api(&app, reqwest::Method::GET, &url, None).await.map_err(|e| with_scope_hint(e, "YouTube"))?;
    Ok(items(&v))
}

/// The user's own: "subscriptions" (channels) or "liked" (videos).
#[tauri::command]
pub async fn yt_mine(app: AppHandle, what: String, max: Option<u32>) -> Result<Vec<Item>, String> {
    let url = match what.as_str() {
        "subscriptions" => url_with(&format!("{BASE}/subscriptions"), &[("part", "snippet"), ("mine", "true"), ("order", "relevance"), ("maxResults", &clamp(max))]),
        "liked" => url_with(&format!("{BASE}/videos"), &[("part", "snippet"), ("myRating", "like"), ("maxResults", &clamp(max))]),
        _ => return Err("Ask for \"subscriptions\" or \"liked\".".into()),
    };
    let v = api(&app, reqwest::Method::GET, &url, None).await.map_err(|e| with_scope_hint(e, "YouTube"))?;
    Ok(items(&v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn video_ids_from_links() {
        for s in ["dQw4w9WgXcQ", "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=5", "https://youtu.be/dQw4w9WgXcQ?si=x", "https://m.youtube.com/shorts/dQw4w9WgXcQ"] {
            assert_eq!(video_id(s).as_deref(), Some("dQw4w9WgXcQ"), "{s}");
        }
        assert_eq!(video_id("https://evil.example/watch?v=dQw4w9WgXcQ"), None);
        assert_eq!(video_id("short"), None);
    }

    #[test]
    fn durations_read_as_clock_time() {
        assert_eq!(clock("PT1H2M3S"), "1:02:03");
        assert_eq!(clock("PT4M5S"), "4:05");
        assert_eq!(clock("PT45S"), "0:45");
        assert_eq!(clock(""), "0:00");
    }

    #[test]
    fn items_from_search_and_lists() {
        let search = json!({ "items": [
            { "id": { "kind": "youtube#video", "videoId": "abc" }, "snippet": { "title": "T", "channelTitle": "C" } },
            { "id": { "kind": "youtube#channel", "channelId": "UC1" }, "snippet": { "title": "Chan" } }
        ]});
        let r = items(&search);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].link, "https://www.youtube.com/watch?v=abc");
        assert_eq!(r[1].kind, "channel");
        let sub = json!({ "items": [{ "id": "sid", "snippet": { "title": "S", "resourceId": { "channelId": "UC9" } } }] });
        assert_eq!(items(&sub)[0].link, "https://www.youtube.com/channel/UC9");
        let liked = json!({ "items": [{ "id": "vid12345678", "snippet": { "title": "L" } }] });
        assert_eq!(items(&liked)[0].id, "vid12345678");
    }
}
