//! "Hey Jarvis": a wake word recognised on this computer.
//!
//! While it's on, the microphone is checked here in 80 ms slices and the audio goes nowhere.
//! Only when the phrase is heard does Jarvis start a real voice session. It steps aside while a
//! session has the mic, so Jarvis can't wake itself by saying its own name.
//!
//! The models are openWakeWord's pre-trained "hey jarvis" set (three small ONNX files, built
//! into the app). openWakeWord's code is Apache 2.0, but its pre-trained models are
//! CC BY-NC-SA 4.0: fine for personal use, not for selling Jarvis. See models/wakeword/NOTICE.md.
//! They run with tract, which is pure Rust, so this works the same on macOS and Windows.

use crate::settings;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tract_onnx::prelude::*;

const MEL_MODEL: &[u8] = include_bytes!("../models/wakeword/melspectrogram.onnx");
const EMBEDDING_MODEL: &[u8] = include_bytes!("../models/wakeword/embedding_model.onnx");
const WAKE_MODEL: &[u8] = include_bytes!("../models/wakeword/hey_jarvis_v0.1.onnx");

/// 80 ms of 16 kHz audio: the step openWakeWord works in.
const CHUNK: usize = 1280;
/// Three extra 10 ms hops so spectrogram frames line up across chunks.
const CONTEXT: usize = 480;
/// Spectrogram frames per embedding, and embeddings per wake-word check.
const MEL_WINDOW: usize = 76;
const EMBEDDINGS: usize = 16;
/// A slice counts as "heard" at this score or above.
const THRESHOLD: f32 = 0.8;
/// Slices in a row needed to wake. A real "hey Jarvis" holds a high score for several slices;
/// near-misses like "hey Travis" spike once. Tested on synthesized voices: every "hey Jarvis"
/// held 0.8+ for at least two slices, and no near-miss did.
const CONFIRM: u32 = 2;
/// Ignore the phrase for a moment after it's heard, so one "hey Jarvis" wakes once.
const COOLDOWN: Duration = Duration::from_secs(2);

type Plan = Arc<TypedRunnableModel>;

/// Turns per-slice scores into a single "wake": true once, when enough high slices come in a row.
#[derive(Default)]
struct Trigger {
    streak: u32,
}

impl Trigger {
    fn hear(&mut self, score: f32) -> bool {
        self.streak = if score >= THRESHOLD { self.streak + 1 } else { 0 };
        self.streak == CONFIRM
    }
}

fn load(bytes: &[u8], shape: &[usize]) -> TractResult<Plan> {
    tract_onnx::onnx()
        .model_for_read(&mut std::io::Cursor::new(bytes))?
        .with_input_fact(0, f32::fact(shape).into())?
        .into_optimized()?
        .into_runnable()
}

/// The three-stage openWakeWord pipeline: audio → log-mel spectrogram → speech embeddings →
/// "hey jarvis" score. Fed 80 ms at a time.
pub struct Detector {
    mel: Plan,
    embedding: Plan,
    wake: Plan,
    audio: Vec<f32>,
    mels: Vec<[f32; 32]>,
    embeddings: Vec<Vec<f32>>,
}

impl Detector {
    pub fn new() -> TractResult<Self> {
        let mut d = Detector {
            mel: load(MEL_MODEL, &[1, CHUNK + CONTEXT])?,
            embedding: load(EMBEDDING_MODEL, &[1, MEL_WINDOW, 32, 1])?,
            wake: load(WAKE_MODEL, &[1, EMBEDDINGS, 96])?,
            audio: vec![],
            mels: vec![],
            embeddings: vec![],
        };
        d.reset();
        Ok(d)
    }

    /// Forget what was heard (after a detection, or when listening resumes).
    pub fn reset(&mut self) {
        self.audio = vec![0.0; CONTEXT];
        // openWakeWord starts its spectrogram buffer at ones.
        self.mels = vec![[1.0; 32]; MEL_WINDOW];
        self.embeddings.clear();
    }

    /// Feed 80 ms of 16 kHz mono audio. Returns how sure it is the phrase was just said, 0 to 1.
    pub fn push(&mut self, chunk: &[i16]) -> TractResult<f32> {
        // The spectrogram model takes raw 16-bit sample values, not -1..1.
        self.audio.extend(chunk.iter().map(|&s| s as f32));
        let keep = CHUNK + CONTEXT;
        if self.audio.len() > keep {
            self.audio.drain(..self.audio.len() - keep);
        }
        let input = tract_ndarray::Array2::from_shape_vec((1, keep), self.audio.clone())?;
        let out = self.mel.run(tvec!(input.into_tensor().into()))?;
        // openWakeWord's scaling of the model's output.
        for frame in out[0].to_plain_array_view::<f32>()?.iter().map(|v| v / 10.0 + 2.0).collect::<Vec<_>>().chunks_exact(32) {
            self.mels.push(frame.try_into().unwrap());
        }
        if self.mels.len() > 4 * MEL_WINDOW {
            self.mels.drain(..self.mels.len() - 2 * MEL_WINDOW);
        }

        let window: Vec<f32> = self.mels[self.mels.len() - MEL_WINDOW..].iter().flatten().copied().collect();
        let x = tract_ndarray::Array4::from_shape_vec((1, MEL_WINDOW, 32, 1), window)?;
        let e = self.embedding.run(tvec!(x.into_tensor().into()))?;
        self.embeddings.push(e[0].to_plain_array_view::<f32>()?.iter().copied().collect());
        if self.embeddings.len() > EMBEDDINGS {
            self.embeddings.remove(0);
        }
        if self.embeddings.len() < EMBEDDINGS {
            return Ok(0.0); // still filling up: about 1.3 s after starting
        }

        let features: Vec<f32> = self.embeddings.iter().flatten().copied().collect();
        let x = tract_ndarray::Array3::from_shape_vec((1, EMBEDDINGS, 96), features)?;
        let score = self.wake.run(tvec!(x.into_tensor().into()))?;
        Ok(score[0].to_plain_array_view::<f32>()?.iter().copied().next().unwrap_or(0.0))
    }
}

#[derive(Default)]
pub struct WakeState {
    stop: Mutex<Option<mpsc::Sender<()>>>,
    /// A voice session has the mic, so Jarvis is already listening.
    session_mic: Arc<AtomicBool>,
}

pub fn set_session_mic(app: &AppHandle, on: bool) {
    app.state::<WakeState>().session_mic.store(on, Ordering::SeqCst);
}

/// Start listening for "hey Jarvis" (restarting if it already is, e.g. after a mic change).
pub fn start(app: &AppHandle) -> Result<(), String> {
    stop(app);
    let state = app.state::<WakeState>();
    let device = settings::load(app).mic_device;
    let session_mic = state.session_mic.clone();
    let (audio_tx, audio_rx) = mpsc::channel::<Vec<i16>>();

    // Recognition runs on its own thread, so the audio callback never waits on it.
    let handle = app.clone();
    std::thread::spawn(move || {
        let mut detector = match Detector::new() {
            Ok(d) => d,
            Err(e) => return eprintln!("wake word: couldn't load the models: {e}"),
        };
        let mut pending: Vec<i16> = Vec::with_capacity(2 * CHUNK);
        let mut trigger = Trigger::default();
        let mut last = Instant::now() - COOLDOWN;
        let mut was_paused = false;
        // Ends when the microphone stream closes.
        while let Ok(samples) = audio_rx.recv() {
            if session_mic.load(Ordering::SeqCst) {
                was_paused = true;
                pending.clear();
                continue;
            }
            if was_paused {
                detector.reset();
                was_paused = false;
            }
            pending.extend_from_slice(&samples);
            while pending.len() >= CHUNK {
                let chunk: Vec<i16> = pending.drain(..CHUNK).collect();
                match detector.push(&chunk) {
                    Ok(score) => {
                        if trigger.hear(score) && last.elapsed() >= COOLDOWN {
                            last = Instant::now();
                            detector.reset();
                            trigger = Trigger::default();
                            crate::context::snapshot(&handle);
                            let _ = handle.emit("wake-word", score);
                        }
                    }
                    Err(e) => eprintln!("wake word: {e}"),
                }
            }
        }
    });

    // cpal streams aren't Send on macOS, so the stream lives on its own thread too.
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    std::thread::spawn(move || {
        let opened = crate::capture::open_input(&device, move |samples, _| {
            let _ = audio_tx.send(samples.to_vec());
        });
        match opened {
            Ok(stream) => {
                let _ = ready_tx.send(Ok(()));
                let _ = stop_rx.recv();
                drop(stream);
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
            }
        }
    });
    ready_rx.recv().map_err(|_| "The wake-word listener stopped unexpectedly.".to_string())??;
    *state.stop.lock().unwrap() = Some(stop_tx);
    Ok(())
}

pub fn stop(app: &AppHandle) {
    if let Some(tx) = app.state::<WakeState>().stop.lock().unwrap().take() {
        let _ = tx.send(());
    }
}

/// Turn "hey Jarvis" on or off (Settings), or restart it after the microphone changes.
#[tauri::command]
pub fn wake_word_set(app: AppHandle, enabled: bool) -> Result<(), String> {
    if enabled {
        start(&app)
    } else {
        stop(&app);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scores 16 kHz mono WAV files: `WAKE_WAVS=/path/a.wav:/path/b.wav cargo test -- --ignored --nocapture wake_word_scores`
    #[test]
    #[ignore]
    fn wake_word_scores() {
        let files = std::env::var("WAKE_WAVS").expect("set WAKE_WAVS");
        for file in files.split(':') {
            let bytes = std::fs::read(file).unwrap();
            // WAV: a 12-byte header, then chunks; the samples are in the "data" chunk.
            let mut pos = 12;
            let data = loop {
                let id = &bytes[pos..pos + 4];
                let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
                if id == b"data" {
                    break &bytes[pos + 8..(pos + 8 + size).min(bytes.len())];
                }
                pos += 8 + size + (size & 1);
            };
            let samples: Vec<i16> = data.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
            let mut d = Detector::new().unwrap();
            let started = Instant::now();
            let (mut best, mut at) = (0.0f32, 0usize);
            let mut n = 0;
            let mut trace = vec![];
            let mut trigger = Trigger::default();
            let mut woke = false;
            for (i, chunk) in samples.chunks_exact(CHUNK).enumerate() {
                let s = d.push(chunk).unwrap();
                n += 1;
                woke |= trigger.hear(s);
                if s >= 0.2 {
                    trace.push(format!("{:.2}", s));
                }
                if s > best {
                    best = s;
                    at = i;
                }
            }
            let name = std::path::Path::new(file).file_stem().unwrap().to_string_lossy().to_string();
            let per = started.elapsed().as_secs_f64() * 1000.0 / n.max(1) as f64;
            println!("{name:14} peak {best:.3} at {:.2}s  {}  ({per:.1} ms per 80 ms)  scores above 0.2 in a row: [{}]", at as f64 * 0.08, if woke { "WAKE" } else { "-" }, trace.join(" "));
        }
    }
}
