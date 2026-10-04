//! The video's second pair of eyes. After a draft is rendered, a handful of its frames are shown to Gemini,
//! which names layout problems the checker can't see (a title running through a drawing, text on a face, a
//! broken picture). Jarvis hands what it found to the worker as one more round of fixes.

use base64::Engine;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

const MODEL: &str = "gemini-2.5-flash";
const GEMINI: &str = "https://generativelanguage.googleapis.com";
const FRAMES: usize = 12;

pub(crate) struct Frame {
    pub at: f64,
    pub jpeg: Vec<u8>,
}

/// Evenly spaced frames from a finished video, small enough to send (640 px wide).
pub(crate) async fn frames(ffmpeg: &Path, video: &Path, seconds: f64, work: &Path) -> Vec<Frame> {
    let _ = std::fs::create_dir_all(work);
    let mut out = vec![];
    for i in 0..FRAMES {
        // Away from the very start and end, where fades make a frame meaningless.
        let at = ((i as f64 + 0.5) / FRAMES as f64 * seconds).clamp(0.3, (seconds - 0.3).max(0.3));
        let file = work.join(format!("review-{i}.jpg"));
        let ok = tokio::process::Command::new(ffmpeg)
            .args(["-y", "-loglevel", "error", "-ss", &format!("{at:.2}"), "-i"])
            .arg(video)
            .args(["-frames:v", "1", "-vf", "scale=640:-2", "-q:v", "4"])
            .arg(&file)
            .stdin(std::process::Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            if let Ok(jpeg) = std::fs::read(&file) {
                out.push(Frame { at, jpeg });
            }
        }
        let _ = std::fs::remove_file(&file);
    }
    out
}

fn prompt(title: &str, times: &[f64]) -> String {
    format!(
        "You are the art director checking frames from a finished video called \"{title}\". The frames are in order, taken at these seconds: {}.\n\
         Report only real layout defects you can see: text overlapping other text or a drawing by mistake, text cut off at an edge or too close to it, text that is hard to read (low contrast, tiny), \
         a title or caption covering someone's face, an empty box or a broken picture, a clearly misaligned element. \
         A frame can be caught halfway through a crossfade, when two scenes show through each other: that is normal, do NOT report it. \
         Do not comment on taste, colours or content. If nothing is wrong, return an empty list. At most 5 problems, the most serious first.\n\
         Answer as JSON: {{\"problems\": [{{\"second\": number, \"issue\": \"what is wrong and where on the screen, in one sentence\"}}]}}",
        times.iter().map(|t| format!("{t:.1}")).collect::<Vec<_>>().join(", ")
    )
}

pub(crate) fn parse_problems(v: &Value) -> Vec<String> {
    let text: String = v["candidates"][0]["content"]["parts"].as_array().map(|p| p.iter().filter_map(|x| x["text"].as_str()).collect()).unwrap_or_default();
    let list: Value = serde_json::from_str(text.trim().trim_start_matches("```json").trim_end_matches("```").trim()).unwrap_or(Value::Null);
    list["problems"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    let issue = p["issue"].as_str()?.trim();
                    if issue.is_empty() {
                        return None;
                    }
                    Some(match p["second"].as_f64() {
                        Some(s) => format!("At about {s:.1}s: {issue}"),
                        None => issue.to_string(),
                    })
                })
                .take(5)
                .collect()
        })
        .unwrap_or_default()
}

/// Ask Gemini what is wrong with these frames. `Err` means the review couldn't be done (the video is still fine to keep).
pub(crate) async fn review(base: &str, key: &str, title: &str, frames: &[Frame]) -> Result<Vec<String>, String> {
    if frames.is_empty() {
        return Err("No frames could be taken.".into());
    }
    let times: Vec<f64> = frames.iter().map(|f| f.at).collect();
    let mut parts = vec![json!({ "text": prompt(title, &times) })];
    for f in frames {
        parts.push(json!({ "inline_data": { "mime_type": "image/jpeg", "data": base64::engine::general_purpose::STANDARD.encode(&f.jpeg) } }));
    }
    let res = reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|e| e.to_string())?
        .post(format!("{base}/v1beta/models/{MODEL}:generateContent"))
        .header("x-goog-api-key", key)
        .json(&json!({ "contents": [{ "role": "user", "parts": parts }], "generationConfig": { "responseMimeType": "application/json", "temperature": 0.2 } }))
        .send()
        .await
        .map_err(|e| format!("Couldn't reach Google: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("The review failed ({}).", res.status()));
    }
    let v: Value = res.json().await.map_err(|e| e.to_string())?;
    Ok(parse_problems(&v))
}

pub(crate) fn gemini_base() -> &'static str {
    GEMINI
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn problems_are_read_from_the_answer() {
        let answer = |t: &str| json!({ "candidates": [{ "content": { "parts": [{ "text": t }] } }] });
        let p = parse_problems(&answer(r#"{"problems":[{"second":12.5,"issue":"The title runs through the rainbow."},{"issue":"  "},{"issue":"Caption is cut off at the right edge."}]}"#));
        assert_eq!(p, vec!["At about 12.5s: The title runs through the rainbow.".to_string(), "Caption is cut off at the right edge.".to_string()]);
        assert!(parse_problems(&answer(r#"{"problems":[]}"#)).is_empty());
        assert_eq!(parse_problems(&answer("```json\n{\"problems\":[{\"second\":1,\"issue\":\"x\"}]}\n```")).len(), 1, "a fenced answer still reads");
        assert!(parse_problems(&answer("not json")).is_empty());
        assert!(parse_problems(&json!({})).is_empty());
        let many = (0..9).map(|i| json!({ "second": i, "issue": "bad" })).collect::<Vec<_>>();
        assert_eq!(parse_problems(&answer(&json!({ "problems": many }).to_string())).len(), 5, "at most five");
    }

    #[tokio::test]
    async fn frames_are_sent_and_the_answer_is_read() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 65536];
            let mut got = 0;
            loop {
                let n = s.read(&mut buf[got..]).await.unwrap();
                got += n;
                let t = String::from_utf8_lossy(&buf[..got]).to_string();
                if let Some(at) = t.find("\r\n\r\n") {
                    let len: usize = t.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
                    if got >= at + 4 + len || n == 0 {
                        break;
                    }
                }
            }
            let reply = json!({ "candidates": [{ "content": { "parts": [{ "text": "{\"problems\":[{\"second\":3,\"issue\":\"Title overlaps the picture.\"}]}" }] } }] }).to_string();
            s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()).as_bytes()).await.unwrap();
            String::from_utf8_lossy(&buf[..got]).to_string()
        });
        let frames = vec![Frame { at: 1.0, jpeg: vec![1, 2, 3] }, Frame { at: 3.0, jpeg: vec![4, 5, 6] }];
        let problems = review(&base, "key123", "My video", &frames).await.unwrap();
        assert_eq!(problems, vec!["At about 3.0s: Title overlaps the picture."]);
        let request = seen.await.unwrap();
        assert!(request.starts_with("POST /v1beta/models/gemini-2.5-flash:generateContent "));
        assert!(request.to_ascii_lowercase().contains("x-goog-api-key: key123"));
        assert_eq!(request.matches("inline_data").count(), 2, "both frames are attached");
        assert!(request.contains("crossfade"), "the prompt tells it not to report fades");
        assert!(review(&base, "k", "t", &[]).await.is_err());
    }
}
