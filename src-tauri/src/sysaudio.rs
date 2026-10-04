//! Native audio through the bundled `jarvis-audio` helper (src-tauri/helpers/jarvis-audio.swift):
//! recording what the Mac plays, so meetings get the other side of a call, and the microphone with
//! Apple's echo cancellation, so Jarvis doesn't hear itself on speakers.
//!
//! The helper writes 16 kHz mono 16-bit PCM to stdout and status lines to stderr. It quits on its
//! own when Jarvis exits (it watches its parent), so it never outlives the app.

use base64::Engine;
use serde::Serialize;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Mutex};
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter};

const CHUNK: usize = 640; // 40 ms at 16 kHz
const RATE: usize = 16_000;
/// After this much audio that is exactly zero, macOS is probably withholding the sound.
const SILENT_HINT_SECONDS: usize = 10;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioChunk {
    /// Base64 of little-endian i16 samples at 16 kHz.
    pcm: String,
    /// RMS level of this chunk, 0..1.
    level: f32,
}

/// What the voice-processing helper reports about Jarvis's voice.
#[derive(Clone, Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VoiceEvent {
    /// "level" while speaking, "idle" when the queue drained, "ended" if the helper stopped.
    kind: String,
    level: f32,
    /// Frames (24 kHz) played or dropped since the helper started.
    done: u64,
    /// Why it ended, as a sentence for the user; empty otherwise.
    message: String,
}

/// One status line from the helper's stderr.
#[derive(Debug, PartialEq)]
enum Status {
    Ready,
    Error(String),
    Warn(String),
    Level { level: f32, done: u64 },
    Idle { done: u64 },
}

fn parse_status(line: &str) -> Option<Status> {
    let line = line.trim();
    let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
    match word {
        "ready" => Some(Status::Ready),
        "error" => Some(Status::Error(rest.trim().to_string())),
        "warn" => Some(Status::Warn(rest.trim().to_string())),
        "level" => {
            let mut parts = rest.split_whitespace();
            let level = parts.next()?.parse().ok()?;
            let done = parts.next()?.parse().ok()?;
            Some(Status::Level { level, done })
        }
        "idle" => Some(Status::Idle { done: rest.trim().parse().ok()? }),
        _ => None,
    }
}

/// The helper sits next to Jarvis's executable: Tauri copies it to target/<profile> in development
/// and into Jarvis.app/Contents/MacOS in the bundle.
fn helper_path() -> Result<PathBuf, String> {
    let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("jarvis-audio")));
    if let Some(p) = beside.filter(|p| p.exists()) {
        return Ok(p);
    }
    let built = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries").join(format!("jarvis-audio-{}", env!("JARVIS_TARGET")));
    if built.exists() {
        return Ok(built);
    }
    Err("Jarvis's audio helper is missing. Reinstall Jarvis (or rebuild it with Xcode installed).".into())
}

fn encode(samples: &[i16]) -> String {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn level_of(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sq: f32 = samples.iter().map(|&s| (s as f32 / 32768.0).powi(2)).sum();
    (sq / samples.len() as f32).sqrt()
}

/// Read 16-bit PCM from `out` and hand it on in 40 ms chunks. `on_samples` sees every chunk
/// first (for the silence check).
fn pump_pcm(mut out: impl Read, mut emit: impl FnMut(&[i16]) -> bool) {
    let mut buf = [0u8; 4096];
    let mut carry: Option<u8> = None;
    let mut samples: Vec<i16> = Vec::with_capacity(CHUNK);
    loop {
        let n = match out.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let mut bytes = &buf[..n];
        if let Some(lo) = carry.take() {
            samples.push(i16::from_le_bytes([lo, bytes[0]]));
            bytes = &bytes[1..];
        }
        let mut pairs = bytes.chunks_exact(2);
        for p in &mut pairs {
            samples.push(i16::from_le_bytes([p[0], p[1]]));
            if samples.len() == CHUNK {
                if !emit(&samples) {
                    return;
                }
                samples.clear();
            }
        }
        carry = pairs.remainder().first().copied();
        if samples.len() == CHUNK {
            if !emit(&samples) {
                return;
            }
            samples.clear();
        }
    }
}

/// Counts leading samples that are exactly zero; says once when there have been enough of them.
struct SilenceWatch {
    zeros: usize,
    done: bool,
}

impl SilenceWatch {
    fn new() -> Self {
        Self { zeros: 0, done: false }
    }

    /// True the moment `SILENT_HINT_SECONDS` of exact zeros have gone by with nothing else.
    fn feed(&mut self, samples: &[i16]) -> bool {
        if self.done {
            return false;
        }
        if samples.iter().any(|&s| s != 0) {
            self.done = true;
            return false;
        }
        self.zeros += samples.len();
        if self.zeros >= SILENT_HINT_SECONDS * RATE {
            self.done = true;
            return true;
        }
        false
    }
}

/// Wait for "ready" (Ok) or an error line / exit (Err) from a helper starting up.
fn await_ready(rx: &mpsc::Receiver<Result<(), String>>, timeout: Duration, what: &str) -> Result<(), String> {
    match rx.recv_timeout(timeout) {
        Ok(r) => r,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!("{what} didn't start in time. Try again in a moment.")),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(format!("{what} stopped as soon as it started.")),
    }
}

fn kill(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

// ---------- the other side of calls ----------

static SYSCAP: Mutex<Option<Child>> = Mutex::new(None);

/// Start recording what the Mac plays (everything except Jarvis itself) as 16 kHz mono chunks.
/// Emits "system-audio-silent" once if the first 10 s are pure digital silence, which is what
/// macOS delivers when Jarvis isn't allowed to record system audio.
#[tauri::command]
pub fn system_audio_start(app: AppHandle, on_chunk: Channel<AudioChunk>) -> Result<(), String> {
    system_audio_stop();
    let mut child = Command::new(helper_path()?)
        .arg("syscap")
        .arg(std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't start recording the Mac's sound: {e}"))?;
    let stdout = child.stdout.take().ok_or("Couldn't read the Mac's sound.")?;
    let stderr = child.stderr.take().ok_or("Couldn't read the Mac's sound.")?;

    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    std::thread::spawn(move || {
        let mut ready = Some(ready_tx);
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            match parse_status(&line) {
                Some(Status::Ready) => {
                    if let Some(tx) = ready.take() {
                        let _ = tx.send(Ok(()));
                    }
                }
                Some(Status::Error(msg)) => {
                    eprintln!("system audio: {msg}");
                    if let Some(tx) = ready.take() {
                        let _ = tx.send(Err(msg));
                    }
                }
                _ => eprintln!("system audio: {line}"),
            }
        }
    });

    std::thread::spawn(move || {
        let mut watch = SilenceWatch::new();
        pump_pcm(stdout, |samples| {
            if watch.feed(samples) {
                let _ = app.emit("system-audio-silent", ());
            }
            on_chunk.send(AudioChunk { pcm: encode(samples), level: level_of(samples) }).is_ok()
        });
    });

    if let Err(e) = await_ready(&ready_rx, Duration::from_secs(10), "Recording the Mac's sound") {
        kill(&mut child);
        return Err(e);
    }
    *SYSCAP.lock().unwrap() = Some(child);
    Ok(())
}

#[tauri::command]
pub fn system_audio_stop() {
    if let Some(mut child) = SYSCAP.lock().unwrap().take() {
        kill(&mut child);
    }
}

// ---------- microphone with echo cancellation ----------

struct Voice {
    child: Child,
    /// Frames for the helper's stdin, written in order by one thread.
    to_helper: mpsc::Sender<Vec<u8>>,
}

static VOICE: Mutex<Option<Voice>> = Mutex::new(None);

/// One frame of the helper's stdin protocol: a little-endian length, then the bytes.
/// An empty frame means "drop everything queued".
fn frame(pcm: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + pcm.len());
    out.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

/// Start the microphone with echo cancellation. Mic audio arrives on `on_chunk` (16 kHz chunks),
/// and how Jarvis's voice is playing arrives on `on_voice`.
#[tauri::command]
pub fn vp_start(app: AppHandle, on_chunk: Channel<AudioChunk>, on_voice: Channel<VoiceEvent>, device: Option<String>) -> Result<(), String> {
    vp_stop(app.clone());
    let mut child = Command::new(helper_path()?)
        .arg("vpio")
        .arg(device.unwrap_or_default())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't start the microphone with echo cancellation: {e}"))?;
    let stdout = child.stdout.take().ok_or("Couldn't read the microphone.")?;
    let stderr = child.stderr.take().ok_or("Couldn't read the microphone.")?;
    let mut stdin = child.stdin.take().ok_or("Couldn't reach the microphone helper.")?;

    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let voice_events = on_voice.clone();
    std::thread::spawn(move || {
        let mut ready = Some(ready_tx);
        let mut last_error = String::new();
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            match parse_status(&line) {
                Some(Status::Ready) => {
                    if let Some(tx) = ready.take() {
                        let _ = tx.send(Ok(()));
                    }
                }
                Some(Status::Error(msg)) => {
                    eprintln!("echo cancellation: {msg}");
                    last_error = msg.clone();
                    if let Some(tx) = ready.take() {
                        let _ = tx.send(Err(msg));
                    }
                }
                Some(Status::Level { level, done }) => {
                    let _ = voice_events.send(VoiceEvent { kind: "level".into(), level, done, message: String::new() });
                }
                Some(Status::Idle { done }) => {
                    let _ = voice_events.send(VoiceEvent { kind: "idle".into(), level: 0.0, done, message: String::new() });
                }
                Some(Status::Warn(msg)) => eprintln!("echo cancellation: {msg}"),
                None => eprintln!("echo cancellation: {line}"),
            }
        }
        // stderr closes when the helper exits, whether we stopped it or it stopped by itself.
        if ready.is_none() {
            let _ = voice_events.send(VoiceEvent { kind: "ended".into(), level: 0.0, done: 0, message: last_error });
        }
    });

    std::thread::spawn(move || {
        pump_pcm(stdout, |samples| on_chunk.send(AudioChunk { pcm: encode(samples), level: level_of(samples) }).is_ok());
    });

    let (to_helper, from_app) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        for bytes in from_app {
            if stdin.write_all(&bytes).and_then(|_| stdin.flush()).is_err() {
                break;
            }
        }
        // Dropping stdin tells the helper to quit.
    });

    if let Err(e) = await_ready(&ready_rx, Duration::from_secs(12), "The microphone with echo cancellation") {
        kill(&mut child);
        return Err(e);
    }
    *VOICE.lock().unwrap() = Some(Voice { child, to_helper });
    // Jarvis is listening for real now, so the wake-word listener steps aside.
    crate::wakeword::set_session_mic(&app, true);
    Ok(())
}

/// Queue 24 kHz mono 16-bit PCM (base64) to play through the echo-cancelled output.
#[tauri::command]
pub fn vp_play(pcm: String) -> Result<(), String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(pcm.as_bytes()).map_err(|_| "That audio wasn't valid.".to_string())?;
    send_to_voice(frame(&bytes))
}

/// Drop everything queued to play (Jarvis was interrupted).
#[tauri::command]
pub fn vp_flush() -> Result<(), String> {
    send_to_voice(frame(&[]))
}

fn send_to_voice(bytes: Vec<u8>) -> Result<(), String> {
    match VOICE.lock().unwrap().as_ref() {
        Some(v) => v.to_helper.send(bytes).map_err(|_| "Echo cancellation stopped.".to_string()),
        None => Err("Echo cancellation isn't running.".into()),
    }
}

#[tauri::command]
pub fn vp_stop(app: AppHandle) {
    if let Some(mut v) = VOICE.lock().unwrap().take() {
        drop(v.to_helper);
        kill(&mut v.child);
        crate::wakeword::set_session_mic(&app, false);
    }
}

// ---------- which devices are in use ----------

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioRoute {
    input_name: String,
    output_name: String,
    /// "bluetooth", "usb", "builtin"… or empty when unknown.
    input_transport: String,
    output_transport: String,
    same_device: bool,
    /// Sound goes to something on or in the ears, so Jarvis can't hear itself.
    likely_headphones: bool,
}

fn route_of(input_name: String, input_transport: String, output_name: String, output_transport: String) -> AudioRoute {
    let same_device = !input_name.is_empty() && input_name == output_name;
    let out = output_name.to_lowercase();
    let named = ["airpods", "headphone", "buds", "headset", "earphone", "beats"].iter().any(|w| out.contains(w));
    let bluetooth_headset = same_device && output_transport == "bluetooth";
    AudioRoute { likely_headphones: named || bluetooth_headset, input_name, output_name, input_transport, output_transport, same_device }
}

/// The default microphone and speakers, and whether they look like headphones. Asks the helper
/// (which knows the connection type), falling back to the device names alone.
#[tauri::command]
pub fn audio_route() -> AudioRoute {
    if let Some(r) = helper_route() {
        return r;
    }
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let name = |d: Option<cpal::Device>| d.and_then(|d| d.description().ok()).map(|x| x.name().to_string()).unwrap_or_default();
    route_of(name(host.default_input_device()), String::new(), name(host.default_output_device()), String::new())
}

fn helper_route() -> Option<AudioRoute> {
    let mut child = Command::new(helper_path().ok()?).arg("route").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut out = String::new();
    let stdout = child.stdout.take()?;
    // A stuck helper mustn't hang the caller: give up after two seconds.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = BufReader::new(stdout).read_to_string(&mut s);
        let _ = tx.send(s);
    });
    match rx.recv_timeout(Duration::from_secs(2)) {
        Ok(s) => out.push_str(&s),
        Err(_) => {
            kill(&mut child);
            return None;
        }
    }
    let _ = child.wait();
    let j: serde_json::Value = serde_json::from_str(out.trim()).ok()?;
    let field = |side: &str, key: &str| j[side][key].as_str().unwrap_or_default().to_string();
    Some(route_of(field("input", "name"), field("input", "transport"), field("output", "name"), field("output", "transport")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_lines_are_understood() {
        assert_eq!(parse_status("ready 48000 Hz"), Some(Status::Ready));
        assert_eq!(parse_status("ready"), Some(Status::Ready));
        assert_eq!(parse_status("error No microphone is connected."), Some(Status::Error("No microphone is connected.".into())));
        assert_eq!(parse_status("level 0.1234 48000"), Some(Status::Level { level: 0.1234, done: 48000 }));
        assert_eq!(parse_status("idle 96000\n"), Some(Status::Idle { done: 96000 }));
        assert!(matches!(parse_status("warn something"), Some(Status::Warn(_))));
        assert_eq!(parse_status("level nonsense"), None);
        assert_eq!(parse_status("idle"), None);
        assert_eq!(parse_status("hello"), None);
    }

    #[test]
    fn pcm_is_cut_into_40ms_chunks_even_across_odd_reads() {
        // 1500 samples as bytes, delivered by a reader that splits them at odd offsets.
        let samples: Vec<i16> = (0..1500).map(|i| i as i16 - 700).collect();
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        struct Odd(Vec<u8>, usize);
        impl Read for Odd {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let n = 333.min(self.0.len() - self.1).min(buf.len());
                buf[..n].copy_from_slice(&self.0[self.1..self.1 + n]);
                self.1 += n;
                Ok(n)
            }
        }
        let mut got: Vec<i16> = Vec::new();
        let mut chunks = 0;
        pump_pcm(Odd(bytes, 0), |c| {
            assert_eq!(c.len(), CHUNK);
            chunks += 1;
            got.extend_from_slice(c);
            true
        });
        assert_eq!(chunks, 2);
        assert_eq!(got, samples[..1280]);
    }

    #[test]
    fn ten_seconds_of_exact_zeros_hint_once() {
        let mut w = SilenceWatch::new();
        let zeros = vec![0i16; CHUNK];
        let mut hints = 0;
        for _ in 0..(SILENT_HINT_SECONDS * RATE / CHUNK + 50) {
            if w.feed(&zeros) {
                hints += 1;
            }
        }
        assert_eq!(hints, 1);

        let mut w = SilenceWatch::new();
        let mut quiet = vec![0i16; CHUNK];
        quiet[3] = 1;
        assert!(!w.feed(&quiet));
        for _ in 0..(SILENT_HINT_SECONDS * RATE / CHUNK + 50) {
            assert!(!w.feed(&zeros), "real sound came through, so no hint");
        }
    }

    #[test]
    fn frames_carry_their_length() {
        assert_eq!(frame(&[]), vec![0, 0, 0, 0]);
        assert_eq!(frame(&[1, 2, 3]), vec![3, 0, 0, 0, 1, 2, 3]);
    }

    /// Runs the real helper for three seconds. Ignored by default: it records the Mac's sound and
    /// may ask for permission. `cargo test syscap_records -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn syscap_records_the_macs_sound() {
        let mut child = Command::new(helper_path().unwrap())
            .args(["syscap", &std::process::id().to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let first = BufReader::new(stderr).lines().next().unwrap().unwrap();
        println!("helper said: {first}");
        assert_eq!(parse_status(&first), Some(Status::Ready));
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut chunks, mut loud) = (0, 0);
            pump_pcm(stdout, |c| {
                chunks += 1;
                if level_of(c) > 0.0 {
                    loud += 1;
                }
                let _ = tx.send((chunks, loud));
                true
            });
        });
        std::thread::sleep(Duration::from_secs(3));
        kill(&mut child);
        let (chunks, loud) = rx.try_iter().last().unwrap_or((0, 0));
        println!("{chunks} chunks of 40 ms in 3 s, {loud} with sound");
        assert!(chunks > 50, "about 75 chunks expected");
    }

    #[test]
    #[ignore]
    fn the_route_comes_from_the_helper() {
        let r = helper_route().expect("the helper answered");
        println!("{r:?}");
    }

    #[test]
    fn headphones_are_recognised() {
        let r = route_of("AirPods Pro".into(), "bluetooth".into(), "AirPods Pro".into(), "bluetooth".into());
        assert!(r.same_device && r.likely_headphones);
        let r = route_of("CMF Buds".into(), "bluetooth".into(), "CMF Buds".into(), "bluetooth".into());
        assert!(r.likely_headphones);
        let r = route_of("Jabra Evolve".into(), "bluetooth".into(), "Jabra Evolve".into(), "bluetooth".into());
        assert!(r.likely_headphones, "a Bluetooth device that is both mic and speaker is a headset");
        let r = route_of("Shure MV7".into(), "usb".into(), "External Headphones".into(), "builtin".into());
        assert!(!r.same_device && r.likely_headphones);
        let r = route_of("Shure MV7".into(), "usb".into(), "Mac mini Speakers".into(), "builtin".into());
        assert!(!r.likely_headphones);
        let r = route_of("".into(), "".into(), "".into(), "".into());
        assert!(!r.same_device && !r.likely_headphones);
    }
}
