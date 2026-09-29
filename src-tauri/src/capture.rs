//! Native microphone capture. The macOS web view in Tauri reports no audio devices,
//! so we record with CoreAudio (via cpal), convert to 16 kHz mono 16-bit PCM for
//! Gemini Live, and stream ~40 ms chunks to the UI over a Tauri channel.

use base64::Engine;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::Serialize;
use std::sync::{mpsc, Mutex};
use tauri::ipc::Channel;
use tauri::{AppHandle, State};

const TARGET_RATE: f64 = 16_000.0;
const CHUNK: usize = 640; // 40 ms at 16 kHz

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicChunk {
    /// Base64 of little-endian i16 samples, ready for Gemini's realtimeInput.
    pcm: String,
    /// RMS level of this chunk, 0..1.
    level: f32,
}

#[derive(Default)]
pub struct MicState {
    stop: Mutex<Option<mpsc::Sender<()>>>,
}

/// Downmix + resample (box filter) into fixed-size PCM chunks.
struct Resampler {
    ratio: f64,
    phase: f64,
    sum: f32,
    count: u32,
    out: Vec<i16>,
    sq: f32,
}

impl Resampler {
    fn new(input_rate: u32) -> Self {
        Self { ratio: input_rate as f64 / TARGET_RATE, phase: 0.0, sum: 0.0, count: 0, out: Vec::with_capacity(CHUNK), sq: 0.0 }
    }

    fn push(&mut self, mono: f32, emit: &mut impl FnMut(&[i16], f32)) {
        self.sum += mono;
        self.count += 1;
        self.phase += 1.0;
        if self.phase >= self.ratio {
            self.phase -= self.ratio;
            let v = (self.sum / self.count as f32).clamp(-1.0, 1.0);
            self.sum = 0.0;
            self.count = 0;
            self.sq += v * v;
            self.out.push((v * if v < 0.0 { 32768.0 } else { 32767.0 }) as i16);
            if self.out.len() == CHUNK {
                let level = (self.sq / CHUNK as f32).sqrt();
                emit(&self.out, level);
                self.out.clear();
                self.sq = 0.0;
            }
        }
    }
}

fn encode(samples: &[i16]) -> String {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicDevice {
    name: String,
    is_default: bool,
}

fn device_name(d: &cpal::Device) -> String {
    d.description().map(|x| x.name().to_string()).unwrap_or_default()
}

/// Microphones currently connected, default first.
#[tauri::command]
pub fn list_mics() -> Vec<MicDevice> {
    let host = cpal::default_host();
    let default = host.default_input_device().map(|d| device_name(&d)).unwrap_or_default();
    let mut out: Vec<MicDevice> = host
        .input_devices()
        .map(|it| it.map(|d| device_name(&d)).filter(|n| !n.is_empty()).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .map(|name| MicDevice { is_default: name == default, name })
        .collect();
    out.sort_by_key(|d| !d.is_default);
    out.dedup_by(|a, b| a.name == b.name);
    out
}

/// The chosen mic by name, or the system default when empty or no longer connected.
fn pick_device(host: &cpal::Host, wanted: &str) -> Option<cpal::Device> {
    if !wanted.is_empty() {
        if let Ok(mut devices) = host.input_devices() {
            if let Some(d) = devices.find(|d| device_name(d) == wanted) {
                return Some(d);
            }
        }
    }
    host.default_input_device()
}

fn build_stream(on_chunk: Channel<MicChunk>, wanted: &str) -> Result<cpal::Stream, String> {
    open_input(wanted, move |samples: &[i16], level: f32| {
        let _ = on_chunk.send(MicChunk { pcm: encode(samples), level });
    })
}

/// Open the microphone and hand every 40 ms of 16 kHz mono audio to `emit`, with its level.
/// Used by the voice session and by the wake-word listener.
pub(crate) fn open_input(wanted: &str, mut emit: impl FnMut(&[i16], f32) + Send + 'static) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = pick_device(&host, wanted).ok_or("No microphone is connected. Plug in a headset or USB mic, connect AirPods, or pick your iPhone under System Settings → Sound → Input.")?;
    let supported = device.default_input_config().map_err(|_| "No microphone is connected. Plug in a headset or USB mic, connect AirPods, or pick your iPhone under System Settings → Sound → Input.".to_string())?;
    let channels = supported.channels() as usize;
    let format = supported.sample_format();
    let config = supported.config();
    let mut rs = Resampler::new(config.sample_rate);
    let err = |e| eprintln!("microphone stream error: {e}");

    macro_rules! stream_of {
        ($t:ty, $to_f32:expr) => {
            device.build_input_stream::<$t, _, _>(
                config,
                move |data: &[$t], _| {
                    for frame in data.chunks(channels.max(1)) {
                        let mono = frame.iter().map(|&s| $to_f32(s)).sum::<f32>() / frame.len() as f32;
                        rs.push(mono, &mut emit);
                    }
                },
                err,
                None,
            )
        };
    }

    let stream = match format {
        cpal::SampleFormat::F32 => stream_of!(f32, |s: f32| s),
        cpal::SampleFormat::I16 => stream_of!(i16, |s: i16| s as f32 / 32768.0),
        cpal::SampleFormat::I32 => stream_of!(i32, |s: i32| s as f32 / 2147483648.0),
        other => return Err(format!("Unsupported microphone sample format: {other:?}")),
    }
    .map_err(|e| format!("Could not open the microphone: {e}"))?;
    stream.play().map_err(|e| format!("Could not start the microphone: {e}"))?;
    Ok(stream)
}

#[tauri::command]
pub fn mic_start(app: AppHandle, state: State<'_, MicState>, on_chunk: Channel<MicChunk>, device: Option<String>) -> Result<(), String> {
    let wanted = device.unwrap_or_default();
    stop_stream(&state);
    // cpal streams aren't Send on macOS, so each one lives on its own thread.
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    std::thread::spawn(move || match build_stream(on_chunk, &wanted) {
        Ok(stream) => {
            let _ = ready_tx.send(Ok(()));
            let _ = stop_rx.recv();
            drop(stream);
        }
        Err(e) => {
            let _ = ready_tx.send(Err(e));
        }
    });
    ready_rx.recv().map_err(|_| "The microphone thread stopped unexpectedly.".to_string())??;
    *state.stop.lock().unwrap() = Some(stop_tx);
    // Jarvis is listening for real now, so the wake-word listener steps aside.
    crate::wakeword::set_session_mic(&app, true);
    Ok(())
}

#[tauri::command]
pub fn mic_stop(app: AppHandle, state: State<'_, MicState>) {
    stop_stream(&state);
    crate::wakeword::set_session_mic(&app, false);
}

fn stop_stream(state: &MicState) {
    if let Some(tx) = state.stop.lock().unwrap().take() {
        let _ = tx.send(());
    }
}
