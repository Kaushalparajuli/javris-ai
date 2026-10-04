//! The meeting's full recording and its speaker-labelled transcript.
//!
//! While a meeting is recorded, the front end appends the audio (16 kHz mono 16-bit, the microphone
//! mixed with the other side of the call) to meeting.wav in the meeting folder, so nothing big is
//! held in memory. Afterwards transcribe_meeting uploads it to Google's Files API and asks
//! gemini-3.5-transcribe for a transcript with speaker labels, written to transcript-speakers.md.
//! The live transcript (transcript.md) made during the meeting is left as it is.

use crate::settings;
use base64::Engine;
use serde::Serialize;
use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use tauri::AppHandle;

const RATE: u32 = 16_000;
const HEADER: u64 = 44;
/// Google labels speakers in at most 30 minutes of audio per request.
const PIECE_SECONDS: u64 = 30 * 60;
const API: &str = "https://generativelanguage.googleapis.com";
const MODEL: &str = "gemini-3.5-transcribe";

/// `dir` if it is one of Jarvis's meeting folders (a plain folder right inside <research>/meetings).
fn meeting_dir(app: &AppHandle, dir: &str) -> Result<PathBuf, String> {
    let root = settings::research_root(app).join("meetings");
    let path = PathBuf::from(dir);
    let plain = path.components().all(|c| !matches!(c, Component::ParentDir | Component::CurDir));
    if !plain || path.parent() != Some(root.as_path()) || !path.is_dir() {
        return Err("That isn't one of Jarvis's meeting folders.".into());
    }
    Ok(path)
}

// ---------- the recording ----------

/// A WAV header for `data_len` bytes of 16 kHz mono 16-bit PCM.
fn wav_header(data_len: u32) -> [u8; 44] {
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&data_len.saturating_add(36).to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&1u16.to_le_bytes()); // mono
    h[24..28].copy_from_slice(&RATE.to_le_bytes());
    h[28..32].copy_from_slice(&(RATE * 2).to_le_bytes());
    h[32..34].copy_from_slice(&2u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_len.to_le_bytes());
    h
}

/// Add PCM to the end of the recording, starting the file with a header the first time.
fn append_pcm(path: &Path, pcm: &[u8]) -> std::io::Result<()> {
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    if f.metadata()?.len() == 0 {
        f.write_all(&wav_header(0))?;
    }
    f.write_all(pcm)
}

/// Write the real sizes into the header (also repairs a recording cut off by a crash).
/// Returns the number of PCM bytes.
fn finalize_wav(path: &Path) -> std::io::Result<u64> {
    let mut f = OpenOptions::new().read(true).write(true).open(path)?;
    let len = f.metadata()?.len();
    if len < HEADER {
        return Ok(0);
    }
    let data = (len - HEADER) & !1;
    f.seek(SeekFrom::Start(0))?;
    f.write_all(&wav_header(data.min(u32::MAX as u64 - 36) as u32))?;
    Ok(data)
}

/// Append a stretch of the meeting's audio (base64 16 kHz mono 16-bit PCM) to meeting.wav.
#[tauri::command]
pub fn meeting_audio_append(app: AppHandle, dir: String, pcm: String) -> Result<(), String> {
    let dir = meeting_dir(&app, &dir)?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(pcm.as_bytes()).map_err(|_| "That audio wasn't valid.".to_string())?;
    append_pcm(&dir.join("meeting.wav"), &bytes).map_err(|e| format!("Couldn't save the meeting's audio: {e}"))
}

/// Finish meeting.wav when the recording stops.
#[tauri::command]
pub fn meeting_audio_finish(app: AppHandle, dir: String) -> Result<(), String> {
    let dir = meeting_dir(&app, &dir)?;
    let path = dir.join("meeting.wav");
    if path.exists() {
        finalize_wav(&path).map_err(|e| format!("Couldn't finish the meeting's audio file: {e}"))?;
    }
    Ok(())
}

/// Byte ranges (start, length) of the PCM, in pieces of at most `piece` bytes.
fn pieces(data_len: u64, piece: u64) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < data_len {
        let len = piece.min(data_len - at);
        out.push((at, len));
        at += len;
    }
    out
}

/// One piece of the recording as a complete WAV file.
fn read_piece(path: &Path, start: u64, len: u64) -> std::io::Result<Vec<u8>> {
    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(HEADER + start))?;
    let mut out = Vec::with_capacity(HEADER as usize + len as usize);
    out.extend_from_slice(&wav_header(len as u32));
    f.take(len).read_to_end(&mut out)?;
    Ok(out)
}

// ---------- reading Google's answer ----------

/// "12.340s" (or a bare number) → seconds.
fn parse_offset(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().trim_end_matches('s').parse().ok(),
        _ => None,
    }
}

/// 75.2 → "01:15"; past an hour, "1:01:15".
fn clock(secs: f64) -> String {
    let t = secs.max(0.0) as u64;
    let (h, m, s) = (t / 3600, (t / 60) % 60, t % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// "spk_1" → "Speaker 1".
fn speaker_name(label: &str) -> String {
    match label.strip_prefix("spk_").or_else(|| label.strip_prefix("speaker_")) {
        Some(n) if !n.is_empty() => format!("Speaker {n}"),
        _ if label.trim().is_empty() => "Speaker ?".into(),
        _ => label.to_string(),
    }
}

struct Turn {
    speaker: String,
    start: f64,
    end: f64,
    words: Vec<String>,
}

/// A long pause starts a new line even when the same person keeps talking.
const PAUSE_SECONDS: f64 = 5.0;

/// Lines like "[01:15] Speaker 1: …" from a generateContent response, with times shifted by
/// `offset` seconds (where this piece starts in the meeting). Falls back to the plain text when
/// there are no word annotations.
fn speaker_lines(resp: &Value, offset: f64) -> Vec<String> {
    let parts = resp["candidates"][0]["content"]["parts"].as_array().cloned().unwrap_or_default();
    let mut turns: Vec<Turn> = Vec::new();
    let mut text = String::new();
    for part in &parts {
        let Some(t) = part.get("audioTranscription") else {
            if let Some(s) = part["text"].as_str() {
                text.push_str(s);
            }
            continue;
        };
        let speaker = t["speakerLabel"].as_str().unwrap_or_default().to_string();
        let words = t["words"].as_array().cloned().unwrap_or_default();
        if words.is_empty() {
            // A whole segment without word timings.
            if let Some(s) = t["text"].as_str().filter(|s| !s.trim().is_empty()) {
                let start = parse_offset(&t["startOffset"]).or_else(|| turns.last().map(|l| l.end)).unwrap_or(0.0);
                let end = parse_offset(&t["endOffset"]).unwrap_or(start);
                turns.push(Turn { speaker, start, end, words: vec![s.trim().to_string()] });
            }
            continue;
        }
        for w in words {
            let word = w["word"].as_str().unwrap_or_default().trim().to_string();
            if word.is_empty() {
                continue;
            }
            let start = parse_offset(&w["startOffset"]).or_else(|| turns.last().map(|l| l.end)).unwrap_or(0.0);
            let end = parse_offset(&w["endOffset"]).unwrap_or(start);
            match turns.last_mut() {
                Some(cur) if cur.speaker == speaker && start - cur.end < PAUSE_SECONDS => {
                    cur.words.push(word);
                    cur.end = end.max(cur.end);
                }
                _ => turns.push(Turn { speaker: speaker.clone(), start, end, words: vec![word] }),
            }
        }
    }
    if turns.is_empty() {
        return text.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
    }
    turns.iter().map(|t| format!("[{}] {}: {}", clock(offset + t.start), speaker_name(&t.speaker), t.words.join(" "))).collect()
}

/// The finished transcript file, one section per 30-minute piece when there is more than one.
fn render(sections: &[(f64, f64, Vec<String>)]) -> String {
    let mut out = String::from("# Speaker-labelled transcript\n\n");
    let several = sections.len() > 1;
    if several {
        out.push_str("_Speaker labels restart in each 30-minute part, so \"Speaker 1\" in one part may be someone else in the next._\n\n");
    }
    for (i, (start, end, lines)) in sections.iter().enumerate() {
        if several {
            out.push_str(&format!("## Part {} ({} – {})\n\n", i + 1, clock(*start), clock(*end)));
        }
        if lines.is_empty() {
            out.push_str("_No speech._\n\n");
        }
        for l in lines {
            out.push_str(l);
            out.push_str("\n\n");
        }
    }
    out
}

// ---------- talking to Google ----------

fn google_problem(body: &str, status: reqwest::StatusCode) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|j| j["error"]["message"].as_str().map(String::from))
        .unwrap_or_else(|| format!("Google returned {status}."))
}

/// Upload one WAV file with the Files API's resumable upload; returns (name, uri) once it's ready.
async fn upload(client: &reqwest::Client, key: &str, wav: Vec<u8>) -> Result<(String, String), String> {
    let start = client
        .post(format!("{API}/upload/v1beta/files"))
        .header("x-goog-api-key", key)
        .header("X-Goog-Upload-Protocol", "resumable")
        .header("X-Goog-Upload-Command", "start")
        .header("X-Goog-Upload-Header-Content-Length", wav.len().to_string())
        .header("X-Goog-Upload-Header-Content-Type", "audio/wav")
        .json(&json!({ "file": { "display_name": "meeting" } }))
        .send()
        .await
        .map_err(|e| format!("Couldn't reach Google to upload the recording: {e}"))?;
    let status = start.status();
    let upload_url = start.headers().get("x-goog-upload-url").and_then(|v| v.to_str().ok()).map(String::from);
    let Some(upload_url) = upload_url else {
        let body = start.text().await.unwrap_or_default();
        return Err(format!("Google wouldn't take the recording: {}", google_problem(&body, status)));
    };
    let done = client
        .post(upload_url)
        .header("x-goog-api-key", key)
        .header("X-Goog-Upload-Offset", "0")
        .header("X-Goog-Upload-Command", "upload, finalize")
        .header("Content-Type", "audio/wav")
        .body(wav)
        .send()
        .await
        .map_err(|e| format!("Uploading the recording failed: {e}"))?;
    let status = done.status();
    let body = done.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("Uploading the recording failed: {}", google_problem(&body, status)));
    }
    let j: Value = serde_json::from_str(&body).map_err(|_| "Google's reply to the upload wasn't readable.".to_string())?;
    let name = j["file"]["name"].as_str().ok_or("Google's reply to the upload had no file name.")?.to_string();
    let mut uri = j["file"]["uri"].as_str().unwrap_or_default().to_string();
    let mut state = j["file"]["state"].as_str().unwrap_or_default().to_string();

    // Google processes the file before it can be used; this normally takes seconds.
    let mut waited = 0;
    while state != "ACTIVE" {
        if state == "FAILED" {
            delete(client, key, &name).await;
            return Err("Google couldn't process the recording.".into());
        }
        if waited >= 300 {
            delete(client, key, &name).await;
            return Err("Google took too long to process the recording. Try again later.".into());
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
        waited += 2;
        let res = client.get(format!("{API}/v1beta/{name}")).header("x-goog-api-key", key).send().await;
        if let Ok(res) = res {
            if let Ok(f) = res.json::<Value>().await {
                state = f["state"].as_str().unwrap_or_default().to_string();
                if let Some(u) = f["uri"].as_str() {
                    uri = u.to_string();
                }
            }
        }
    }
    Ok((name, uri))
}

async fn delete(client: &reqwest::Client, key: &str, name: &str) {
    let _ = client.delete(format!("{API}/v1beta/{name}")).header("x-goog-api-key", key).send().await;
}

/// Ask the transcription model for speaker-labelled text of one uploaded file.
async fn transcribe_file(client: &reqwest::Client, key: &str, uri: &str) -> Result<Value, String> {
    let body = json!({
        "contents": [{ "parts": [{ "fileData": { "fileUri": uri, "mimeType": "audio/wav" } }] }],
        "generationConfig": { "audioTranscriptionConfig": { "diarization": true } },
    });
    let res = client
        .post(format!("{API}/v1beta/models/{MODEL}:generateContent"))
        .header("x-goog-api-key", key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Couldn't reach Google to transcribe the recording: {e}"))?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(format!(
            "Your Gemini key can't use Google's {MODEL} model, which labels who said what ({}). The transcript made during the meeting is still in its folder.",
            google_problem(&text, status)
        ));
    }
    if !status.is_success() {
        return Err(format!("Google couldn't transcribe the recording: {}", google_problem(&text, status)));
    }
    serde_json::from_str(&text).map_err(|_| "Google's transcript wasn't readable.".to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerTranscript {
    path: String,
    text: String,
}

/// Transcribe the meeting's recording with speaker labels into transcript-speakers.md.
/// Long meetings go up in 30-minute pieces (Google's limit for labelling speakers).
#[tauri::command]
pub async fn transcribe_meeting(app: AppHandle, dir: String) -> Result<SpeakerTranscript, String> {
    let dir = meeting_dir(&app, &dir)?;
    let wav = dir.join("meeting.wav");
    if !wav.exists() {
        return Err("This meeting has no recording to transcribe.".into());
    }
    let data = finalize_wav(&wav).map_err(|e| format!("Couldn't read the meeting's recording: {e}"))?;
    if data < RATE as u64 * 2 {
        return Err("The recording is too short to transcribe.".into());
    }
    let key = settings::load(&app).gemini_api_key;
    if key.trim().is_empty() {
        return Err("Add your Gemini API key in Settings first.".into());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20 * 60))
        .build()
        .map_err(|e| format!("Couldn't prepare the upload: {e}"))?;

    let bytes_per_second = RATE as u64 * 2;
    let mut sections = Vec::new();
    for (start, len) in pieces(data, PIECE_SECONDS * bytes_per_second) {
        let piece = read_piece(&wav, start, len).map_err(|e| format!("Couldn't read the meeting's recording: {e}"))?;
        let (name, uri) = upload(&client, &key, piece).await?;
        let result = transcribe_file(&client, &key, &uri).await;
        delete(&client, &key, &name).await;
        let from = (start / bytes_per_second) as f64;
        let to = ((start + len) / bytes_per_second) as f64;
        sections.push((from, to, speaker_lines(&result?, from)));
    }

    let text = render(&sections);
    let path = dir.join("transcript-speakers.md");
    std::fs::write(&path, &text).map_err(|e| format!("Couldn't save the transcript: {e}"))?;
    Ok(SpeakerTranscript { path: path.display().to_string(), text })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("jarvis-{name}-{}.wav", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn the_recording_is_a_valid_wav_as_it_grows() {
        let path = temp("wav");
        append_pcm(&path, &[1, 0, 2, 0]).unwrap();
        append_pcm(&path, &[3, 0]).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 44 + 6);
        assert_eq!(finalize_wav(&path).unwrap(), 6);
        let b = std::fs::read(&path).unwrap();
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(b[4..8].try_into().unwrap()), 36 + 6);
        assert_eq!(&b[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes(b[22..24].try_into().unwrap()), 1, "mono");
        assert_eq!(u32::from_le_bytes(b[24..28].try_into().unwrap()), 16_000);
        assert_eq!(u32::from_le_bytes(b[28..32].try_into().unwrap()), 32_000, "bytes per second");
        assert_eq!(u16::from_le_bytes(b[34..36].try_into().unwrap()), 16, "bits");
        assert_eq!(&b[36..40], b"data");
        assert_eq!(u32::from_le_bytes(b[40..44].try_into().unwrap()), 6);
        assert_eq!(&b[44..], &[1, 0, 2, 0, 3, 0]);
        // More audio after finishing (a resumed recording) is picked up by the next finish.
        append_pcm(&path, &[4, 0]).unwrap();
        assert_eq!(finalize_wav(&path).unwrap(), 8);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_piece_is_its_own_wav_file() {
        let path = temp("piece");
        let pcm: Vec<u8> = (0..20u8).collect();
        append_pcm(&path, &pcm).unwrap();
        finalize_wav(&path).unwrap();
        let p = read_piece(&path, 8, 6).unwrap();
        assert_eq!(p.len(), 44 + 6);
        assert_eq!(u32::from_le_bytes(p[40..44].try_into().unwrap()), 6);
        assert_eq!(&p[44..], &[8, 9, 10, 11, 12, 13]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn long_recordings_split_into_30_minute_pieces() {
        let per_piece = PIECE_SECONDS * 32_000;
        assert_eq!(pieces(0, per_piece), vec![]);
        assert_eq!(pieces(1000, per_piece), vec![(0, 1000)]);
        assert_eq!(pieces(per_piece, per_piece), vec![(0, per_piece)]);
        // 70 minutes: 30 + 30 + 10.
        let seventy = 70 * 60 * 32_000;
        let p = pieces(seventy, per_piece);
        assert_eq!(p, vec![(0, per_piece), (per_piece, per_piece), (2 * per_piece, 10 * 60 * 32_000)]);
        assert!(p.iter().all(|(s, l)| s % 2 == 0 && l % 2 == 0), "pieces never split a sample");
    }

    #[test]
    fn words_become_speaker_lines() {
        let resp = json!({ "candidates": [{ "content": { "parts": [
            { "audioTranscription": { "speakerLabel": "spk_1", "words": [
                { "word": "Hello", "startOffset": "0.100s", "endOffset": "0.450s" },
                { "word": "everyone.", "startOffset": "0.500s", "endOffset": "0.900s" } ] } },
            { "audioTranscription": { "speakerLabel": "spk_1", "words": [
                { "word": "Shall", "startOffset": "1.2s", "endOffset": "1.4s" },
                { "word": "we?", "startOffset": "1.5s", "endOffset": "1.7s" } ] } },
            { "audioTranscription": { "speakerLabel": "spk_2", "words": [
                { "word": "नमस्ते", "startOffset": "75.0s", "endOffset": "75.6s" } ] } },
            { "audioTranscription": { "speakerLabel": "spk_2", "words": [
                { "word": "Later", "startOffset": "90.0s", "endOffset": "90.4s" } ] } },
            { "text": "Hello everyone. Shall we? नमस्ते Later" }
        ] } }] });
        assert_eq!(
            speaker_lines(&resp, 0.0),
            vec![
                "[00:00] Speaker 1: Hello everyone. Shall we?".to_string(),
                "[01:15] Speaker 2: नमस्ते".to_string(),
                "[01:30] Speaker 2: Later".to_string(),
            ]
        );
        // The second 30-minute piece is shifted to meeting time.
        assert_eq!(speaker_lines(&resp, 1800.0)[1], "[31:15] Speaker 2: नमस्ते");
        assert_eq!(speaker_lines(&resp, 3600.0)[0], "[1:00:00] Speaker 1: Hello everyone. Shall we?");
    }

    #[test]
    fn plain_text_is_kept_when_there_are_no_words() {
        let resp = json!({ "candidates": [{ "content": { "parts": [ { "text": "Speaker 1: hi\n\nSpeaker 2: hello\n" } ] } }] });
        assert_eq!(speaker_lines(&resp, 0.0), vec!["Speaker 1: hi", "Speaker 2: hello"]);
        assert!(speaker_lines(&json!({}), 0.0).is_empty());
    }

    #[test]
    fn speakers_and_times_read_naturally() {
        assert_eq!(speaker_name("spk_3"), "Speaker 3");
        assert_eq!(speaker_name(""), "Speaker ?");
        assert_eq!(speaker_name("Alice"), "Alice");
        assert_eq!(clock(5.9), "00:05");
        assert_eq!(clock(3725.0), "1:02:05");
        assert_eq!(parse_offset(&json!("12.5s")), Some(12.5));
        assert_eq!(parse_offset(&json!(3)), Some(3.0));
        assert_eq!(parse_offset(&json!(null)), None);
    }

    #[test]
    fn several_parts_say_labels_restart() {
        let one = render(&[(0.0, 600.0, vec!["[00:01] Speaker 1: hi".into()])]);
        assert!(!one.contains("restart") && !one.contains("## Part"));
        let two = render(&[(0.0, 1800.0, vec!["a".into()]), (1800.0, 2400.0, vec![])]);
        assert!(two.contains("Speaker labels restart in each 30-minute part"));
        assert!(two.contains("## Part 2 (30:00 – 40:00)"));
        assert!(two.contains("_No speech._"));
    }
}
