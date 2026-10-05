//! The video's second pair of eyes. After a draft is rendered, a handful of its frames are shown to Gemini,
//! which names layout problems the checker can't see (a title running through a drawing, text on a face, a
//! broken picture). Jarvis hands what it found to the worker as one more round of fixes.

use base64::Engine;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

/// Newest first. Each model has its own quota, so when one is used up (429) or gone (404) the next is tried.
const MODELS: [&str; 3] = ["gemini-3.8-flash", "gemini-3.5-flash-lite", "gemini-2.5-flash"];
const GEMINI: &str = "https://generativelanguage.googleapis.com";
/// About one frame every three seconds, at least 12 and at most 24: a 60-second video is seen every 3 s, not every 5.
fn frame_count(seconds: f64) -> usize {
    ((seconds / 3.0).round() as usize).clamp(12, 24)
}

pub(crate) struct Frame {
    pub at: f64,
    pub jpeg: Vec<u8>,
}

/// Evenly spaced frames from a finished video, small enough to send (640 px wide).
pub(crate) async fn frames(ffmpeg: &Path, video: &Path, seconds: f64, work: &Path) -> Vec<Frame> {
    let _ = std::fs::create_dir_all(work);
    let mut out = vec![];
    let count = frame_count(seconds);
    for i in 0..count {
        // Away from the very start and end, where fades make a frame meaningless.
        let at = ((i as f64 + 0.5) / count as f64 * seconds).clamp(0.3, (seconds - 0.3).max(0.3));
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
        "You are the art director of a motion-design studio checking frames from a finished video called \"{title}\". The frames are in order, taken at these seconds: {}. \
         The standard is a produced motion-graphics piece, not a slide deck or a page of app screenshots.\n\
         Report two kinds of problem.\n\
         1. Defects: text overlapping other text or a drawing by mistake, text cut off at an edge or too close to it, text that is hard to read (low contrast), \
         a title covering someone's face, an empty box or a broken picture, a clearly misaligned element, a strip of plain background showing along an edge because a photo or scene doesn't cover the whole frame.\n\
         2. Weak design: text too small to read on a phone (smaller than about 1/27 of the frame height, including text inside mock-ups and cards); \
         the subject small and floating in a mostly empty frame instead of filling it (a card or window using less than about half the frame); \
         scenes that look like the same thing again (the same white card on the same background); a frame that reads as a slide or a screenshot rather than a designed shot; \
         decoration that adds nothing and repeats in every frame.\n\
         The video is full of motion, so a frame can be caught mid-move: halfway through a crossfade with two scenes showing through each other, a coloured panel or shape wiping across the frame, a blurred camera pan, \
         words or letters still flying in, tilted or scaled, half revealed by a mask. That is normal, do NOT report it. \
         Each problem says what is wrong, where, and what to do instead, specifically (\"the email card fills a third of the frame and its body text is tiny: push the camera in so the subject line and one bullet fill the frame\"). \
         If nothing is wrong, return an empty list. At most 6 problems, the most serious first.\n\
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
                .take(6)
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
    let http = reqwest::Client::builder().timeout(Duration::from_secs(90)).build().map_err(|e| e.to_string())?;
    let body = json!({ "contents": [{ "role": "user", "parts": parts }], "generationConfig": { "responseMimeType": "application/json", "temperature": 0.2 } });
    let mut last = String::new();
    for model in MODELS {
        let res = http
            .post(format!("{base}/v1beta/models/{model}:generateContent"))
            .header("x-goog-api-key", key)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Couldn't reach Google: {e}"))?;
        let status = res.status();
        if status.is_success() {
            let v: Value = res.json().await.map_err(|e| e.to_string())?;
            return Ok(parse_problems(&v));
        }
        last = format!("The review failed ({status}).");
        // A used-up quota, a retired model or a busy one: the next model has its own quota and servers.
        if !(status == reqwest::StatusCode::TOO_MANY_REQUESTS || status == reqwest::StatusCode::NOT_FOUND || status.is_server_error()) {
            break;
        }
    }
    Err(last)
}

pub(crate) fn gemini_base() -> &'static str {
    GEMINI
}

// ---------- blank stretches ----------

/// A frame whose brightest and darkest pixels are this close (8-bit luma) is one flat colour: nothing on
/// screen, or something covering everything (a wipe panel left resting over the canvas).
const FLAT_RANGE: f64 = 12.0;
/// Frames sampled per second for the blank check.
const FLAT_FPS: f64 = 4.0;

/// Stretches of flat frames from ffmpeg's signalstats log, as (start, length) in seconds. Only stretches of
/// at least three samples count (a flash or a wipe passing through is shorter).
pub(crate) fn parse_flat(log: &str) -> Vec<(f64, f64)> {
    let mut samples: Vec<(f64, bool)> = vec![];
    let (mut t, mut ymin) = (None, None);
    for line in log.lines() {
        if let Some(v) = line.split("pts_time:").nth(1).and_then(|v| v.split_whitespace().next()).and_then(|v| v.parse::<f64>().ok()) {
            t = Some(v);
        } else if let Some(v) = line.split("signalstats.YMIN=").nth(1).and_then(|v| v.trim().parse::<f64>().ok()) {
            ymin = Some(v);
        } else if let Some(v) = line.split("signalstats.YMAX=").nth(1).and_then(|v| v.trim().parse::<f64>().ok()) {
            if let (Some(at), Some(lo)) = (t, ymin) {
                samples.push((at, v - lo <= FLAT_RANGE));
            }
        }
    }
    let step = 1.0 / FLAT_FPS;
    let mut out = vec![];
    let mut run: Option<(f64, usize)> = None;
    for (at, flat) in samples.iter().copied().chain(std::iter::once((f64::MAX, false))) {
        match (flat, run) {
            (true, None) => run = Some((at, 1)),
            (true, Some((s, n))) => run = Some((s, n + 1)),
            (false, Some((s, n))) => {
                if n >= 3 {
                    out.push((s, n as f64 * step));
                }
                run = None;
            }
            (false, None) => {}
        }
    }
    out
}

pub(crate) fn blank_problems(flat: &[(f64, f64)]) -> Vec<String> {
    if flat.is_empty() {
        return vec![];
    }
    let list = flat.iter().take(6).map(|(s, d)| format!("{s:.1}s to {:.1}s", s + d)).collect::<Vec<_>>().join(", ");
    vec![format!(
        "The whole frame is one flat colour from {list}: nothing shows, or something is covering the entire canvas. The usual cause is a full-frame panel \
         (a wipe, cover or flash) whose CSS rests over the canvas and that is only moved away by a tween starting later, or a scene hidden by its own start state. \
         Make every transition panel a clip that exists only while it moves (class=\"clip\" with its own data-start and data-duration) or starts off-screen, and make sure content is visible throughout."
    )]
}

/// Measure the blank stretches of a rendered video with ffmpeg. Empty when ffmpeg can't run.
pub(crate) async fn blanks(ffmpeg: &Path, video: &Path) -> Vec<(f64, f64)> {
    let out = tokio::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(video)
        .args(["-vf", &format!("fps={FLAT_FPS},scale=320:-2,signalstats,metadata=mode=print"), "-map", "0:v", "-f", "null", "-"])
        .stdin(std::process::Stdio::null())
        .output()
        .await;
    match out {
        Ok(o) => parse_flat(&String::from_utf8_lossy(&o.stderr)),
        Err(_) => vec![],
    }
}

// ---------- still stretches ----------

/// How much of the picture may change while still counting as still (ffmpeg freezedetect's noise), and
/// the shortest stretch that counts. A gentle 2% breathing or a far-off drift stays under it; a word
/// landing, a camera move or a card arriving goes over it.
const STILL_NOISE: &str = "0.015";
const STILL_MIN: f64 = 1.5;

/// Stretches of the video where the picture barely changes: (start, length) in seconds, read from
/// ffmpeg's freezedetect. A stretch that runs to the end has its length measured to `seconds`.
pub(crate) fn parse_freezes(log: &str, seconds: f64) -> Vec<(f64, f64)> {
    let mut out = vec![];
    let mut open: Option<f64> = None;
    for line in log.lines() {
        let value = |key: &str| line.split(key).nth(1).and_then(|v| v.trim().split_whitespace().next()).and_then(|v| v.parse::<f64>().ok());
        if let Some(s) = value("freeze_start:") {
            open = Some(s);
        } else if let Some(d) = value("freeze_duration:") {
            if let Some(s) = open.take() {
                out.push((s, d));
            }
        }
    }
    if let Some(s) = open {
        out.push((s, (seconds - s).max(0.0)));
    }
    out
}

/// What is wrong with a video's still stretches, in sentences for the worker. The last three seconds
/// (the call to action, meant to be read) don't count. Flags a long still or too much still time.
pub(crate) fn still_problems(freezes: &[(f64, f64)], seconds: f64) -> Vec<String> {
    let end = (seconds - 3.0).max(0.0);
    let stills: Vec<(f64, f64)> = freezes.iter().filter(|(s, _)| *s < end).map(|(s, d)| (*s, d.min(end - s))).filter(|(_, d)| *d >= STILL_MIN).collect();
    let total: f64 = stills.iter().map(|(_, d)| d).sum();
    let longest = stills.iter().map(|(_, d)| *d).fold(0.0, f64::max);
    if longest < 2.5 && total <= end * 0.25 {
        return vec![];
    }
    let mut worst = stills.clone();
    worst.sort_by(|a, b| b.1.total_cmp(&a.1));
    let list = worst.iter().take(6).map(|(s, d)| format!("{s:.1}s to {:.1}s", s + d)).collect::<Vec<_>>().join(", ");
    vec![format!(
        "The picture barely changes for {total:.0} of the first {end:.0} seconds; the longest still stretches are {list}. \
         In each of them make something visibly happen every second: the camera travels to the next detail, the next element lands, a value ticks, a line draws. \
         A slow 2% breath is not enough on its own."
    )]
}

/// Measure the still stretches of a rendered video with ffmpeg. Empty when ffmpeg can't run.
pub(crate) async fn stills(ffmpeg: &Path, video: &Path, seconds: f64) -> Vec<(f64, f64)> {
    let out = tokio::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(video)
        .args(["-vf", &format!("scale=480:-2,freezedetect=n={STILL_NOISE}:d={STILL_MIN}"), "-map", "0:v", "-f", "null", "-"])
        .stdin(std::process::Stdio::null())
        .output()
        .await;
    match out {
        Ok(o) => parse_freezes(&String::from_utf8_lossy(&o.stderr), seconds),
        Err(_) => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn longer_videos_are_looked_at_more_often() {
        assert_eq!(frame_count(8.0), 12, "a short sting still gets twelve looks");
        assert_eq!(frame_count(54.0), 18);
        assert_eq!(frame_count(64.0), 21);
        assert_eq!(frame_count(180.0), 24, "never more than 24 frames in one request");
    }

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
        assert_eq!(parse_problems(&answer(&json!({ "problems": many }).to_string())).len(), 6, "at most six");
    }

    #[test]
    fn blank_stretches_are_found_in_signalstats() {
        // Four samples a second; luma range per frame. Flat from 0 to 1.0 (four samples), a single flat frame at 2.0, varied otherwise.
        let ranges = [(0.0, 0), (0.25, 3), (0.5, 0), (0.75, 1), (1.0, 120), (1.25, 90), (2.0, 5), (2.25, 140)];
        let log: String = ranges
            .iter()
            .map(|(t, r)| format!("[Parsed_metadata_3 @ 0x1] frame:0 pts:0 pts_time:{t}\n[Parsed_metadata_3 @ 0x1] lavfi.signalstats.YMIN=40\n[Parsed_metadata_3 @ 0x1] lavfi.signalstats.YAVG=60\n[Parsed_metadata_3 @ 0x1] lavfi.signalstats.YMAX={}\n", 40 + r))
            .collect();
        let flat = parse_flat(&log);
        assert_eq!(flat, vec![(0.0, 1.0)], "a lone flat frame is a flash, not a blank");
        let p = blank_problems(&flat);
        assert!(p.len() == 1 && p[0].contains("from 0.0s to 1.0s") && p[0].contains("transition panel"));
        assert!(blank_problems(&[]).is_empty());
        let ending = "[m] frame:0 pts:0 pts_time:5\n[m] lavfi.signalstats.YMIN=10\n[m] lavfi.signalstats.YMAX=12\n".repeat(1)
            + "[m] frame:1 pts:1 pts_time:5.25\n[m] lavfi.signalstats.YMIN=10\n[m] lavfi.signalstats.YMAX=12\n[m] frame:2 pts:2 pts_time:5.5\n[m] lavfi.signalstats.YMIN=10\n[m] lavfi.signalstats.YMAX=11\n";
        assert_eq!(parse_flat(&ending), vec![(5.0, 0.75)], "a blank running to the end is still reported");
    }

    #[test]
    fn still_stretches_are_read_from_ffmpeg_and_judged() {
        let log = "[freezedetect @ 0x1] lavfi.freezedetect.freeze_start: 3\n[freezedetect @ 0x1] lavfi.freezedetect.freeze_duration: 1.6\n\
                   [freezedetect @ 0x1] lavfi.freezedetect.freeze_end: 4.6\n[freezedetect @ 0x1] lavfi.freezedetect.freeze_start: 5.6\n\
                   [freezedetect @ 0x1] lavfi.freezedetect.freeze_duration: 2.9\n[freezedetect @ 0x1] lavfi.freezedetect.freeze_end: 8.5\n\
                   [freezedetect @ 0x1] lavfi.freezedetect.freeze_start: 57.5\n";
        let f = parse_freezes(log, 60.0);
        assert_eq!(f, vec![(3.0, 1.6), (5.6, 2.9), (57.5, 2.5)], "a stretch still open at the end runs to the end");
        let p = still_problems(&f, 60.0);
        assert!(p.len() == 1 && p[0].contains("5.6s to 8.5s"), "a 2.9-second still is too long: {p:?}");
        assert!(!p[0].contains("57.5"), "the closing hold doesn't count");
        // The motion template: one 2-second still in the middle and a held ending.
        assert!(still_problems(&[(5.1, 2.03), (9.6, 2.4)], 12.0).is_empty());
        // Many short stills add up.
        let choppy: Vec<(f64, f64)> = (0..12).map(|i| (i as f64 * 4.0, 2.0)).collect();
        assert_eq!(still_problems(&choppy, 60.0).len(), 1);
        assert!(still_problems(&[], 30.0).is_empty());
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
        assert!(request.starts_with("POST /v1beta/models/gemini-3.8-flash:generateContent "), "the newest model is asked first");
        assert!(request.to_ascii_lowercase().contains("x-goog-api-key: key123"));
        assert_eq!(request.matches("inline_data").count(), 2, "both frames are attached");
        assert!(request.contains("crossfade"), "the prompt tells it not to report fades");
        assert!(review(&base, "k", "t", &[]).await.is_err());
    }

    #[tokio::test]
    async fn a_used_up_model_hands_over_to_the_next() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let served = tokio::spawn(async move {
            let mut paths = vec![];
            for reply in [("429 Too Many Requests", r#"{"error":{"code":429}}"#.to_string()), ("200 OK", json!({ "candidates": [{ "content": { "parts": [{ "text": r#"{"problems":[]}"# }] } }] }).to_string())] {
                let (mut s, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 1 << 20];
                let mut got = 0;
                loop {
                    let n = s.read(&mut buf[got..]).await.unwrap();
                    got += n;
                    let text = String::from_utf8_lossy(&buf[..got]);
                    if let Some(head) = text.find("\r\n\r\n") {
                        let len = text[..head].lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
                        if got >= head + 4 + len || n == 0 {
                            paths.push(text.lines().next().unwrap_or("").to_string());
                            break;
                        }
                    }
                }
                s.write_all(format!("HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", reply.0, reply.1.len(), reply.1).as_bytes()).await.unwrap();
            }
            paths
        });
        let frames = vec![Frame { at: 1.0, jpeg: vec![1, 2, 3] }];
        assert_eq!(review(&base, "k", "t", &frames).await.unwrap(), Vec::<String>::new());
        let paths = served.await.unwrap();
        assert!(paths[0].contains("gemini-3.8-flash") && paths[1].contains("gemini-3.5-flash-lite"), "{paths:?}");
    }
}
