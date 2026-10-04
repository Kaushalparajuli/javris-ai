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

// ---------- echo cancellation ----------
// With echo cancellation on, the microphone and Jarvis's voice both go through one native helper
// (jarvis-audio vpio, see sysaudio.rs) that runs Apple's voice processing: Jarvis's voice plays
// through the same audio engine that records the mic, so the canceller knows what to remove and
// Jarvis can be interrupted by voice even on speakers.

/** What the voice conversation needs from a microphone. */
export interface VoiceMic {
  onChunk: (pcmBase64: string) => void;
  start(device?: string): Promise<void>;
  level(): number;
  stop(): void;
  /** True when Jarvis's own voice is removed from what the mic hears. */
  readonly echoCancelled?: boolean;
}

/** What the voice conversation needs from a speaker. */
export interface VoicePlayer {
  resume(): Promise<void>;
  chime(): void;
  play(base64: string): void;
  readonly speaking: boolean;
  stop(): void;
  level(): number;
}

type VoiceEvent = { kind: "level" | "idle" | "ended"; level: number; done: number; message: string };

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** Frames of 16-bit audio in a base64 string. */
function framesIn(b64: string): number {
  const pad = b64.endsWith("==") ? 2 : b64.endsWith("=") ? 1 : 0;
  return Math.max(0, Math.floor((b64.length * 3) / 4) - pad) >> 1;
}

/** The one voice-processing helper shared by the mic and the player. Every call to Rust goes
 * through one queue, so starts, audio, flushes and stops reach the helper in order. */
class VoiceHelper {
  running = false;
  micActive = false;
  onMic: ((pcm: string, level: number) => void) | null = null;
  onEnded: ((message: string) => void) | null = null;
  outLevel = 0;
  /** 24 kHz frames sent to play, reported played (or dropped), and dropped by our own flush. */
  private written = 0;
  private done = 0;
  private flushedTo = 0;
  private device = "";
  private generation = 0;
  private queue: Promise<unknown> = Promise.resolve();

  get speaking(): boolean {
    return this.running && Math.max(this.done, this.flushedTo) < this.written;
  }

  private enqueue<T>(fn: () => Promise<T>): Promise<T> {
    const p = this.queue.then(fn);
    this.queue = p.catch(() => {});
    return p;
  }

  private reset() {
    this.running = false;
    this.generation++;
    this.written = this.done = this.flushedTo = 0;
    this.outLevel = 0;
  }

  start(device: string): Promise<void> {
    return this.enqueue(async () => {
      if (this.running && this.device === device) return;
      if (this.running) {
        this.reset();
        await invoke("vp_stop").catch(() => {});
      }
      const gen = ++this.generation;
      const chunks = new Channel<{ pcm: string; level: number }>();
      chunks.onmessage = (m) => {
        if (gen === this.generation) this.onMic?.(m.pcm, m.level);
      };
      const events = new Channel<VoiceEvent>();
      events.onmessage = (e) => {
        if (gen === this.generation) this.event(e);
      };
      await invoke("vp_start", { onChunk: chunks, onVoice: events, device });
      this.running = true;
      this.device = device;
    });
  }

  private event(e: VoiceEvent) {
    if (e.kind === "ended") {
      this.reset();
      this.onEnded?.(e.message || "Echo cancellation stopped.");
      return;
    }
    this.done = Math.max(this.done, e.done);
    this.outLevel = e.kind === "level" ? e.level : 0;
    if (e.kind === "idle") this.stopIfUnused();
  }

  play(b64: string) {
    const frames = framesIn(b64);
    if (!frames) return;
    this.written += frames;
    const gen = this.generation;
    this.enqueue(() => invoke("vp_play", { pcm: b64 })).catch(() => {
      // The helper went away mid-sentence; don't stay "speaking" forever.
      if (gen === this.generation) this.done = this.written;
    });
  }

  flush() {
    this.flushedTo = this.written;
    this.outLevel = 0;
    if (this.running) this.enqueue(() => invoke("vp_flush")).catch(() => {});
    this.stopIfUnused();
  }

  /** Close the helper (and so the microphone) once nothing needs it. */
  stopIfUnused() {
    if (!this.running || this.micActive || this.speaking) return;
    this.reset();
    this.enqueue(() => invoke("vp_stop")).catch(() => {});
  }
}

const voiceHelper = new VoiceHelper();

/** The microphone with Apple's echo cancellation. Same interface as NativeMic. If voice processing
 * can't start, it falls back to the plain microphone and `echoCancelled` turns false, so the
 * caller can go back to pausing the mic while Jarvis talks. */
export class VoiceProcessedMic implements VoiceMic {
  onChunk: (pcmBase64: string) => void = () => {};
  /** Told when the microphone stops by itself and can't be restarted. */
  onError: (message: string) => void = () => {};
  echoCancelled = true;
  /** Why echo cancellation isn't in use, when it fell back. */
  fallbackReason = "";
  private lastLevel = 0;
  private active = false;
  private device = "";
  private plain: NativeMic | null = null;
  private restarts: number[] = [];

  async start(device = "") {
    this.active = true;
    this.device = device;
    voiceHelper.micActive = true;
    voiceHelper.onMic = (pcm, level) => {
      if (!this.active || this.plain) return;
      this.lastLevel = level;
      this.onChunk(pcm);
    };
    voiceHelper.onEnded = (message) => this.restart(message);
    try {
      await voiceHelper.start(device);
      this.echoCancelled = true;
      // Stopped while it was starting.
      if (!this.active) voiceHelper.stopIfUnused();
    } catch (e) {
      voiceHelper.micActive = false;
      if (!this.active) return;
      this.echoCancelled = false;
      this.fallbackReason = errorText(e);
      console.warn("Echo cancellation unavailable, using the plain microphone:", this.fallbackReason);
      const m = new NativeMic();
      m.onChunk = (pcm) => {
        if (!this.active) return;
        this.lastLevel = m.level();
        this.onChunk(pcm);
      };
      await m.start(device);
      this.plain = m;
    }
  }

  /** The helper stopped by itself: start it again, a few times a minute at most. */
  private restart(message: string) {
    if (!this.active || this.plain) return;
    const now = Date.now();
    this.restarts = this.restarts.filter((t) => now - t < 60_000);
    if (this.restarts.length >= 3) {
      this.onError(message);
      return;
    }
    this.restarts.push(now);
    setTimeout(() => {
      if (this.active && !this.plain) this.start(this.device).catch((e) => this.onError(errorText(e)));
    }, 500);
  }

  level(): number {
    return this.plain ? this.plain.level() : this.lastLevel;
  }

  stop() {
    this.active = false;
    this.lastLevel = 0;
    this.plain?.stop();
    this.plain = null;
    voiceHelper.micActive = false;
    voiceHelper.stopIfUnused();
  }
}

/** Plays Jarvis's voice through the echo-cancelled output while the echo-cancelled mic is on,
 * and through the web view (like Player) otherwise. Same interface as Player. */
export class NativePlayer implements VoicePlayer {
  private web = new Player();

  async resume() {
    await this.web.resume();
  }

  chime() {
    this.web.chime();
  }

  /** Keep a sentence on the output it started on, so the two never overlap. */
  private useHelper(): boolean {
    if (voiceHelper.speaking) return true;
    if (this.web.speaking) return false;
    return voiceHelper.running;
  }

  play(base64: string) {
    if (this.useHelper()) voiceHelper.play(base64);
    else this.web.play(base64);
  }

  get speaking(): boolean {
    return voiceHelper.speaking || this.web.speaking;
  }

  stop() {
    voiceHelper.flush();
    this.web.stop();
  }

  level(): number {
    return voiceHelper.speaking ? voiceHelper.outLevel : this.web.level();
  }
}

/** Microphone and speaker for a voice conversation: echo-cancelled when the setting is on,
 * otherwise the plain NativeMic and Player. */
export function createVoiceIO(settings: { echoCancellation?: boolean }): { mic: VoiceMic; player: VoicePlayer } {
  return { mic: createMic(settings), player: createPlayer(settings) };
}

export function createMic(settings: { echoCancellation?: boolean }): VoiceMic {
  return settings.echoCancellation ? new VoiceProcessedMic() : new NativeMic();
}

export function createPlayer(settings: { echoCancellation?: boolean }): VoicePlayer {
  return settings.echoCancellation ? new NativePlayer() : new Player();
}

export interface AudioRoute {
  inputName: string;
  outputName: string;
  /** "bluetooth", "usb", "builtin"… or "" when unknown. */
  inputTransport: string;
  outputTransport: string;
  sameDevice: boolean;
  /** Sound goes to headphones or earbuds, so Jarvis can't hear itself. */
  likelyHeadphones: boolean;
}

/** The current default microphone and speakers, and whether they look like headphones. */
export function audioRoute(): Promise<AudioRoute> {
  return invoke<AudioRoute>("audio_route");
}
