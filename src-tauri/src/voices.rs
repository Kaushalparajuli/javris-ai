//! Third-party voices for video narration. Each provider turns a script into an audio file plus the time
//! of every word (for captions). Today: ElevenLabs. Another service is added the same way: a key in
//! `Settings::service_keys`, a function here, and a line in `speak` that chooses it.

use crate::settings;
use base64::Engine;
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;
use tauri::AppHandle;

const ELEVENLABS: &str = "https://api.elevenlabs.io";
/// ElevenLabs' stock voice "Rachel": available on every plan, used until the person picks one.
const DEFAULT_ELEVENLABS_VOICE: &str = "21m00Tcm4TlvDq8ikWAM";
/// One request can speak this many characters on the multilingual model.
const MAX_CHARS: usize = 4500;

pub struct Speech {
    pub audio: Vec<u8>,
    /// File extension of `audio`.
    pub ext: &'static str,
    /// Each word with its start and end in seconds.
    pub words: Vec<Value>,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(120)).build().unwrap_or_default()
}

/// What ElevenLabs said went wrong, in words a person can act on.
fn explain(status: u16, body: &str) -> String {
    let detail: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let msg = detail["detail"]["message"].as_str().or(detail["detail"].as_str()).unwrap_or("");
    match status {
        401 => "ElevenLabs didn't accept the API key. Check it in Settings, Video.".into(),
        402 | 403 if msg.to_lowercase().contains("quota") || msg.to_lowercase().contains("credit") => "Your ElevenLabs plan is out of credits.".into(),
        403 => format!("ElevenLabs refused this: {}", if msg.is_empty() { "the key may not be allowed to do that" } else { msg }),
        404 => "ElevenLabs doesn't know that voice. Pick another one in Settings, Video.".into(),
        422 => format!("ElevenLabs couldn't use that: {}", if msg.is_empty() { "check the voice and model" } else { msg }),
        429 => "ElevenLabs is busy or you've hit your limit. Try again in a moment.".into(),
        _ => format!("ElevenLabs returned {status}{}", if msg.is_empty() { String::new() } else { format!(": {msg}") }),
    }
}

/// Words with start and end times from ElevenLabs' per-character timing.
pub(crate) fn words_from_alignment(chars: &[Value], starts: &[Value], ends: &[Value]) -> Vec<Value> {
    let mut words = vec![];
    let (mut text, mut start, mut end) = (String::new(), 0.0f64, 0.0f64);
    let flush = |text: &mut String, start: f64, end: f64, words: &mut Vec<Value>| {
        if !text.is_empty() {
            words.push(json!({ "text": text.as_str(), "start": (start * 100.0).round() / 100.0, "end": (end * 100.0).round() / 100.0 }));
            text.clear();
        }
    };
    for (i, c) in chars.iter().enumerate() {
        let c = c.as_str().unwrap_or("");
        if c.chars().all(char::is_whitespace) {
            flush(&mut text, start, end, &mut words);
            continue;
        }
        let (s, e) = (starts.get(i).and_then(Value::as_f64).unwrap_or(end), ends.get(i).and_then(Value::as_f64).unwrap_or(end));
        if text.is_empty() {
            start = s;
        }
        text.push_str(c);
        end = e;
    }
    flush(&mut text, start, end, &mut words);
    words
}

/// Speak `text` with ElevenLabs. `base` is the service address (tests point it elsewhere).
pub async fn elevenlabs_speech(base: &str, key: &str, voice: &str, model: &str, text: &str) -> Result<Speech, String> {
    if key.trim().is_empty() {
        return Err("There's no ElevenLabs API key. Add one in Settings, Video.".into());
    }
    if text.chars().count() > MAX_CHARS {
        return Err(format!("The narration is {} characters long; ElevenLabs takes up to {MAX_CHARS} at a time. Shorten the script.", text.chars().count()));
    }
    let voice = if voice.trim().is_empty() { DEFAULT_ELEVENLABS_VOICE } else { voice.trim() };
    if !voice.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("That ElevenLabs voice id isn't valid.".into());
    }
    let model = if model.trim().is_empty() { "eleven_multilingual_v2" } else { model.trim() };
    let resp = client()
        .post(format!("{base}/v1/text-to-speech/{voice}/with-timestamps?output_format=mp3_44100_128"))
        .header("xi-api-key", key.trim())
        .json(&json!({ "text": text, "model_id": model, "voice_settings": { "stability": 0.5, "similarity_boost": 0.75, "style": 0.2, "use_speaker_boost": true } }))
        .send()
        .await
        .map_err(|e| format!("Couldn't reach ElevenLabs: {e}"))?;
    let status = resp.status().as_u16();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !(200..300).contains(&status) {
        return Err(explain(status, &body));
    }
    let v: Value = serde_json::from_str(&body).map_err(|_| "ElevenLabs sent something Jarvis couldn't read.".to_string())?;
    let audio = base64::engine::general_purpose::STANDARD.decode(v["audio_base64"].as_str().unwrap_or("")).map_err(|_| "ElevenLabs sent no audio.".to_string())?;
    if audio.is_empty() {
        return Err("ElevenLabs sent no audio.".into());
    }
    let a = &v["alignment"];
    let empty = vec![];
    let words = words_from_alignment(a["characters"].as_array().unwrap_or(&empty), a["character_start_times_seconds"].as_array().unwrap_or(&empty), a["character_end_times_seconds"].as_array().unwrap_or(&empty));
    Ok(Speech { audio, ext: "mp3", words })
}

/// Speak a video's narration with ElevenLabs when the settings choose it: `None` means "use Gemini instead".
pub async fn speak(app: &AppHandle, text: &str) -> Option<Result<Speech, String>> {
    let s = settings::load(app);
    let key = s.service_keys.get("elevenlabs").cloned().unwrap_or_default();
    let wants = match s.narration_provider.as_str() {
        "elevenlabs" => true,
        "gemini" => false,
        _ => !key.trim().is_empty(),
    };
    if !wants {
        return None;
    }
    Some(elevenlabs_speech(ELEVENLABS, &key, &s.elevenlabs_voice, &s.elevenlabs_model, text).await)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceInfo {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
}

async fn list_voices(base: &str, key: &str) -> Result<Vec<VoiceInfo>, String> {
    if key.trim().is_empty() {
        return Err("Add the ElevenLabs API key first.".into());
    }
    let resp = client().get(format!("{base}/v1/voices")).header("xi-api-key", key.trim()).send().await.map_err(|e| format!("Couldn't reach ElevenLabs: {e}"))?;
    let status = resp.status().as_u16();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !(200..300).contains(&status) {
        return Err(explain(status, &body));
    }
    let v: Value = serde_json::from_str(&body).map_err(|_| "ElevenLabs sent something Jarvis couldn't read.".to_string())?;
    let mut list: Vec<VoiceInfo> = v["voices"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some(VoiceInfo {
                        id: x["voice_id"].as_str()?.to_string(),
                        name: x["name"].as_str().unwrap_or("Voice").to_string(),
                        category: x["category"].as_str().unwrap_or("").to_string(),
                        description: ["gender", "accent", "descriptive", "use_case"].iter().filter_map(|k| x["labels"][*k].as_str()).collect::<Vec<_>>().join(", "),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    // The person's own voices first, then the rest by name.
    list.sort_by(|a, b| (a.category == "premade").cmp(&(b.category == "premade")).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(list)
}

/// The voices an ElevenLabs key can use. Pass the key typed in the box (not saved yet) or leave it out for the saved one.
#[tauri::command]
pub async fn elevenlabs_voices(app: AppHandle, key: Option<String>) -> Result<Vec<VoiceInfo>, String> {
    let key = key.filter(|k| !k.trim().is_empty()).unwrap_or_else(|| settings::load(&app).service_keys.get("elevenlabs").cloned().unwrap_or_default());
    list_voices(ELEVENLABS, &key).await
}

/// Check a service's key without saving it. Returns a short message for the person.
#[tauri::command]
pub async fn service_test(app: AppHandle, id: String, key: Option<String>) -> Result<String, String> {
    match id.as_str() {
        "elevenlabs" => {
            let voices = elevenlabs_voices(app, key).await?;
            Ok(format!("Works. {} voice{} available.", voices.len(), if voices.len() == 1 { "" } else { "s" }))
        }
        other => Err(format!("Jarvis doesn't know a service called {other}.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn chars(s: &str) -> Vec<Value> {
        s.chars().map(|c| json!(c.to_string())).collect()
    }

    #[test]
    fn character_times_become_word_times() {
        let text = "Hi, Rani!  Call";
        let c = chars(text);
        let starts: Vec<Value> = (0..c.len()).map(|i| json!(i as f64 * 0.1)).collect();
        let ends: Vec<Value> = (0..c.len()).map(|i| json!(i as f64 * 0.1 + 0.1)).collect();
        let w = words_from_alignment(&c, &starts, &ends);
        assert_eq!(w.iter().map(|x| x["text"].as_str().unwrap()).collect::<Vec<_>>(), vec!["Hi,", "Rani!", "Call"], "punctuation stays with its word; extra spaces are ignored");
        assert_eq!((w[0]["start"].as_f64(), w[0]["end"].as_f64()), (Some(0.0), Some(0.3)));
        assert_eq!((w[1]["start"].as_f64(), w[1]["end"].as_f64()), (Some(0.4), Some(0.9)));
        assert!(words_from_alignment(&[], &[], &[]).is_empty());
        assert_eq!(words_from_alignment(&chars("नमस्ते दुनिया"), &[], &[]).len(), 2, "any script, even with no times");
    }

    #[test]
    fn problems_are_explained_in_plain_words() {
        assert!(explain(401, "{}").contains("didn't accept the API key"));
        assert!(explain(402, r#"{"detail":{"message":"You have no credits left"}}"#).is_empty() == false);
        assert!(explain(403, r#"{"detail":{"message":"quota exceeded"}}"#).contains("out of credits"));
        assert!(explain(404, "").contains("doesn't know that voice"));
        assert!(explain(429, "").contains("busy"));
        assert!(explain(500, "oops").contains("500"));
    }

    /// A stand-in for ElevenLabs on this computer, answering one request with `reply`.
    async fn fake_service(status: &str, reply: String) -> (String, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let status = status.to_string();
        let seen = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 16384];
            let mut got = 0;
            loop {
                let n = s.read(&mut buf[got..]).await.unwrap();
                got += n;
                let text = String::from_utf8_lossy(&buf[..got]).to_string();
                if let Some(at) = text.find("\r\n\r\n") {
                    let len: usize = text.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
                    if got >= at + 4 + len {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let request = String::from_utf8_lossy(&buf[..got]).to_string();
            let head = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", reply.len());
            s.write_all(head.as_bytes()).await.unwrap();
            s.write_all(reply.as_bytes()).await.unwrap();
            request
        });
        (base, seen)
    }

    #[tokio::test]
    async fn a_script_is_spoken_and_timed() {
        let text = "Hello Patan";
        let c = chars(text);
        let starts: Vec<f64> = (0..c.len()).map(|i| i as f64 * 0.05).collect();
        let ends: Vec<f64> = starts.iter().map(|s| s + 0.05).collect();
        let reply = json!({ "audio_base64": base64::engine::general_purpose::STANDARD.encode(b"ID3fake-mp3"), "alignment": { "characters": c, "character_start_times_seconds": starts, "character_end_times_seconds": ends } }).to_string();
        let (base, seen) = fake_service("200 OK", reply).await;
        let sp = elevenlabs_speech(&base, "sk_test", "VOICEID123", "eleven_multilingual_v2", text).await.unwrap();
        assert_eq!((sp.audio.as_slice(), sp.ext), (b"ID3fake-mp3".as_slice(), "mp3"));
        assert_eq!(sp.words.len(), 2);
        assert_eq!(sp.words[0]["text"], "Hello");
        assert_eq!(sp.words[1]["text"], "Patan");
        let request = seen.await.unwrap();
        assert!(request.starts_with("POST /v1/text-to-speech/VOICEID123/with-timestamps?output_format=mp3_44100_128 "), "{request}");
        assert!(request.to_ascii_lowercase().contains("xi-api-key: sk_test"));
        assert!(request.contains("\"model_id\":\"eleven_multilingual_v2\"") && request.contains("\"text\":\"Hello Patan\""));
    }

    #[tokio::test]
    async fn a_refused_key_and_a_bad_voice_are_reported_without_calling_out() {
        let (base, _seen) = fake_service("401 Unauthorized", r#"{"detail":{"status":"invalid_api_key"}}"#.into()).await;
        let e = elevenlabs_speech(&base, "wrong", "", "", "Hi").await.err().unwrap();
        assert!(e.contains("didn't accept the API key"), "{e}");
        assert!(elevenlabs_speech("http://127.0.0.1:1", "", "v", "", "Hi").await.err().unwrap().contains("no ElevenLabs API key"));
        assert!(elevenlabs_speech("http://127.0.0.1:1", "k", "../x", "", "Hi").await.err().unwrap().contains("isn't valid"), "a voice id can't carry a path");
        assert!(elevenlabs_speech("http://127.0.0.1:1", "k", "v", "", &"a".repeat(5000)).await.err().unwrap().contains("Shorten"));
    }

    #[tokio::test]
    async fn a_voice_list_puts_the_persons_own_voices_first() {
        let reply = json!({ "voices": [
            { "voice_id": "p1", "name": "Rachel", "category": "premade", "labels": { "gender": "female", "accent": "american" } },
            { "voice_id": "c1", "name": "Zed (mine)", "category": "cloned" },
            { "voice_id": "p2", "name": "Adam", "category": "premade" }
        ] })
        .to_string();
        let (base, seen) = fake_service("200 OK", reply).await;
        let list = list_voices(&base, "k").await.unwrap();
        assert_eq!(list.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(), vec!["c1", "p2", "p1"]);
        assert_eq!(list[2].description, "female, american");
        assert!(seen.await.unwrap().starts_with("GET /v1/voices "));
    }
}
