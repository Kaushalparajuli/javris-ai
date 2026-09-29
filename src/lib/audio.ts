import { Channel, invoke } from "@tauri-apps/api/core";

function rms(analyser: AnalyserNode, buf: Float32Array<ArrayBuffer>): number {
  analyser.getFloatTimeDomainData(buf);
  let sum = 0;
  for (let i = 0; i < buf.length; i++) sum += buf[i] * buf[i];
  return Math.sqrt(sum / buf.length);
}

/**
 * Microphone captured natively in Rust (see capture.rs), because the macOS web view
 * doesn't expose audio devices. Delivers base64 16 kHz PCM chunks ready for Gemini.
 */
export class NativeMic {
  private lastLevel = 0;
  onChunk: (pcmBase64: string) => void = () => {};

  async start(device = "") {
    const ch = new Channel<{ pcm: string; level: number }>();
    ch.onmessage = (m) => {
      this.lastLevel = m.level;
      this.onChunk(m.pcm);
    };
    await invoke("mic_start", { onChunk: ch, device });
  }

  level(): number {
    return this.lastLevel;
  }

  stop() {
    this.lastLevel = 0;
    invoke("mic_stop").catch(() => {});
  }
}

/** Web view microphone → 16 kHz PCM chunks (works in a normal browser, not in the macOS app). */
export class MicStream {
  private ctx?: AudioContext;
  private stream?: MediaStream;
  private analyser?: AnalyserNode;
  private buf = new Float32Array(512);
  onChunk: (pcm: ArrayBuffer) => void = () => {};

  async start() {
    // WebKit rejects some constraints, so fall back to progressively simpler requests.
    const attempts: MediaStreamConstraints[] = [
      { audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true } },
      { audio: { echoCancellation: true } },
      { audio: true },
    ];
    let lastError: unknown;
    for (const c of attempts) {
      try {
        this.stream = await navigator.mediaDevices.getUserMedia(c);
        break;
      } catch (e) {
        lastError = e;
        if ((e as DOMException)?.name === "NotAllowedError") break;
      }
    }
    if (!this.stream) throw lastError;
    this.ctx = new AudioContext();
    await this.ctx.audioWorklet.addModule("/pcm-worklet.js");
    const src = this.ctx.createMediaStreamSource(this.stream);
    const node = new AudioWorkletNode(this.ctx, "pcm-recorder", { processorOptions: { targetRate: 16000 } });
    node.port.onmessage = (e) => this.onChunk(e.data as ArrayBuffer);
    this.analyser = this.ctx.createAnalyser();
    this.analyser.fftSize = 512;
    const silent = this.ctx.createGain();
    silent.gain.value = 0;
    src.connect(this.analyser);
    src.connect(node);
    node.connect(silent).connect(this.ctx.destination);
  }

  level(): number {
    return this.analyser ? rms(this.analyser, this.buf) : 0;
  }

  stop() {
    this.stream?.getTracks().forEach((t) => t.stop());
    this.ctx?.close();
    this.ctx = undefined;
    this.analyser = undefined;
  }
}

/** Plays Gemini's 24 kHz PCM audio chunks back to back, and can be cut off instantly. */
export class Player {
  private ctx = new AudioContext();
  private analyser: AnalyserNode;
  private out: GainNode;
  private nextTime = 0;
  private sources = new Set<AudioBufferSourceNode>();
  private buf = new Float32Array(512);

  constructor() {
    this.analyser = this.ctx.createAnalyser();
    this.analyser.fftSize = 512;
    this.out = this.ctx.createGain();
    this.out.connect(this.analyser);
    this.analyser.connect(this.ctx.destination);
  }

  async resume() {
    if (this.ctx.state !== "running") await this.ctx.resume();
  }

  /** A soft rising two-note chime: Jarvis heard its name and is listening. */
  chime() {
    const t = this.ctx.currentTime + 0.02;
    [660, 880].forEach((freq, i) => {
      const at = t + i * 0.09;
      const osc = this.ctx.createOscillator();
      const gain = this.ctx.createGain();
      osc.type = "sine";
      osc.frequency.value = freq;
      gain.gain.setValueAtTime(0, at);
      gain.gain.linearRampToValueAtTime(0.12, at + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.001, at + 0.28);
      osc.connect(gain).connect(this.ctx.destination);
      osc.start(at);
      osc.stop(at + 0.3);
    });
  }

  play(base64: string) {
    const bin = atob(base64);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    const pcm = new Int16Array(bytes.buffer, 0, bytes.length >> 1);
    if (!pcm.length) return;
    const buffer = this.ctx.createBuffer(1, pcm.length, 24000);
    const ch = buffer.getChannelData(0);
    for (let i = 0; i < pcm.length; i++) ch[i] = pcm[i] / 0x8000;
    const src = this.ctx.createBufferSource();
    src.buffer = buffer;
    src.connect(this.out);
    const start = Math.max(this.ctx.currentTime + 0.02, this.nextTime);
    src.start(start);
    this.nextTime = start + buffer.duration;
    this.sources.add(src);
    src.onended = () => this.sources.delete(src);
  }

  get speaking(): boolean {
    return this.ctx.currentTime < this.nextTime;
  }

  stop() {
    this.sources.forEach((s) => {
      try {
        s.stop();
      } catch {
        /* already stopped */
      }
    });
    this.sources.clear();
    this.nextTime = 0;
  }

  level(): number {
    return rms(this.analyser, this.buf);
  }
}

export function toBase64(buf: ArrayBuffer): string {
  const bytes = new Uint8Array(buf);
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(s);
}
