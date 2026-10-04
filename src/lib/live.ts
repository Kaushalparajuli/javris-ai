import { invoke } from "@tauri-apps/api/core";
// Minimal Gemini Live API client over WebSocket.
// Docs: https://ai.google.dev/gemini-api/docs/live

import { toBase64 } from "./audio";

const API = "https://generativelanguage.googleapis.com/v1beta";
const WS =
  "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";

export interface FunctionCall {
  id: string;
  name: string;
  args: Record<string, unknown>;
}

export interface LiveHandlers {
  onReady(): void;
  onClosed(reason: string, willRetry: boolean): void;
  onAudio(base64: string): void;
  onInputText(text: string): void;
  onOutputText(text: string): void;
  onTurnComplete(): void;
  onInterrupted(): void;
  onToolCall(calls: FunctionCall[]): void;
}

export interface LiveConfig {
  apiKey: string;
  model: string;
  voice: string;
  systemPrompt: string;
  tools: object[];
}

/** Lists models that support the Live API, best guess first. */
export async function listLiveModels(apiKey: string): Promise<string[]> {
  const res = await fetch(`${API}/models?pageSize=1000&key=${encodeURIComponent(apiKey)}`);
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new Error(body?.error?.message || `Google returned ${res.status}`);
  }
  const data = await res.json();
  const names: string[] = (data.models ?? [])
    .filter((m: { supportedGenerationMethods?: string[] }) => m.supportedGenerationMethods?.includes("bidiGenerateContent"))
    .map((m: { name: string }) => m.name.replace(/^models\//, ""))
    // Transcribe/translate/robotics models are Live-capable but not conversational.
    .filter((n: string) => !/transcribe|translate|robotics/i.test(n));
  const score = (n: string) => {
    const version = parseFloat(n.match(/(\d+(\.\d+)?)/)?.[1] ?? "0");
    return (
      (n.includes("live") ? 200 : 0) +
      (n.includes("native-audio") || n.includes("audio") ? 100 : 0) +
      version * 10 -
      (n.includes("exp") ? 5 : 0)
    );
  };
  return names.sort((a, b) => score(b) - score(a));
}

function log(line: string) {
  invoke("app_log", { name: "live", line }).catch(() => {});
}

const MAX_RETRIES = 8;

/** Protobuf durations arrive as strings like "50s" or "4.5s". */
function parseSeconds(v: unknown): number | undefined {
  const n = typeof v === "number" ? v : parseFloat(String(v ?? ""));
  return Number.isFinite(n) ? n : undefined;
}

export class LiveSession {
  private ws?: WebSocket;
  /** The connection being closed on purpose after a goAway; its close starts the resumed one. */
  private rotating?: WebSocket;
  private rotateBy = 0;
  private rotateTimer?: number;
  /** For finding a quiet moment: Jarvis mid-reply, the user mid-sentence, or a tool call unanswered. */
  private modelBusy = false;
  private lastUserText = 0;
  private toolsPending = 0;
  /** Which connection each tool call came from: a reply after a switch-over can't use the old id. */
  private callSocket = new Map<string, WebSocket>();
  /** Calls Gemini took back because the conversation moved on; their results are dropped. */
  private cancelled = new Set<string>();
  /** Turns sent while reconnecting, delivered once the new connection is ready. Audio isn't kept. */
  private queued: object[] = [];
  private handle?: string;
  private closedByUser = false;
  private retries = 0;
  private attempt = 0;
  ready = false;

  constructor(private cfg: LiveConfig, private h: LiveHandlers) {}

  connect() {
    this.closedByUser = false;
    this.ws = this.open();
  }

  private setupMessage() {
    return {
      setup: {
        model: `models/${this.cfg.model}`,
        generationConfig: {
          responseModalities: ["AUDIO"],
          speechConfig: { voiceConfig: { prebuiltVoiceConfig: { voiceName: this.cfg.voice } } },
        },
        systemInstruction: { parts: [{ text: this.cfg.systemPrompt }] },
        tools: [{ functionDeclarations: this.cfg.tools }],
        inputAudioTranscription: {},
        outputAudioTranscription: {},
        contextWindowCompression: { slidingWindow: {} },
        sessionResumption: this.handle ? { handle: this.handle } : {},
      },
    };
  }

  private open(): WebSocket {
    const n = ++this.attempt;
    const resumed = !!this.handle;
    const openedAt = Date.now();
    let wasReady = false;
    log(`#${n} connecting model=${this.cfg.model} resume=${resumed}`);
    const ws = new WebSocket(`${WS}?key=${encodeURIComponent(this.cfg.apiKey)}`);

    ws.onopen = () => ws.send(JSON.stringify(this.setupMessage()));
    ws.onmessage = async (ev) => {
      const text = typeof ev.data === "string" ? ev.data : await (ev.data as Blob).text();
      let msg: any;
      try {
        msg = JSON.parse(text);
      } catch {
        return;
      }
      if (msg.setupComplete) {
        wasReady = true;
        log(`#${n} ready in ${Date.now() - openedAt}ms`);
      }
      if (ws === this.ws) this.route(msg);
    };
    ws.onclose = (ev) => {
      const secs = ((Date.now() - openedAt) / 1000).toFixed(0);
      log(`#${n} closed code=${ev.code} reason="${ev.reason}" after ${secs}s ready=${wasReady}`);
      if (ws === this.rotating) {
        // Closed on purpose at a quiet moment: resume the same conversation on a fresh connection.
        this.rotating = undefined;
        if (!this.closedByUser && this.ws === ws) setTimeout(() => !this.closedByUser && this.ws === ws && (this.ws = this.open()), 300);
        return;
      }
      if (ws !== this.ws || this.closedByUser) return;
      this.stopRotateWatch();
      this.modelBusy = false;
      this.toolsPending = 0;
      this.ready = false;

      let reason = ev.reason || `Connection closed (code ${ev.code})`;
      if (!wasReady && resumed) {
        // Most likely an expired resume token: start a fresh session instead of giving up.
        log(`#${n} resume rejected; retrying without handle`);
        this.handle = undefined;
      }
      const badConfig =
        !wasReady && !resumed && (ev.code === 1007 || ev.code === 1008 || /api key|permission|not found|unsupported|invalid argument/i.test(ev.reason));
      if (badConfig) {
        this.h.onClosed(reason, false);
        return;
      }
      if (this.retries >= MAX_RETRIES) {
        reason = `${reason}. Gave up after ${MAX_RETRIES} attempts; press the mic to reconnect.`;
        this.h.onClosed(reason, false);
        return;
      }
      const delay = Math.min(500 * 2 ** this.retries, 15000);
      this.retries++;
      this.h.onClosed(reason, true);
      setTimeout(() => {
        if (!this.closedByUser && this.ws === ws) this.ws = this.open();
      }, delay);
    };
    return ws;
  }

  private route(msg: any) {
    if (msg.setupComplete) {
      this.ready = true;
      this.retries = 0;
      this.h.onReady();
      for (const m of this.queued.splice(0)) this.send(m);
    }
    const sc = msg.serverContent;
    if (sc) {
      if (sc.modelTurn) this.modelBusy = true;
      if (sc.interrupted || sc.turnComplete) this.modelBusy = false;
      if (sc.inputTranscription?.text) this.lastUserText = Date.now();
      if (sc.interrupted) this.h.onInterrupted();
      if (sc.inputTranscription?.text) this.h.onInputText(sc.inputTranscription.text);
      if (sc.outputTranscription?.text) this.h.onOutputText(sc.outputTranscription.text);
      for (const part of sc.modelTurn?.parts ?? []) {
        if (part.inlineData?.data) this.h.onAudio(part.inlineData.data);
      }
      if (sc.turnComplete) this.h.onTurnComplete();
    }
    for (const id of msg.toolCallCancellation?.ids ?? []) {
      this.cancelled.add(id);
      log(`tool call ${id} cancelled by Gemini`);
    }
    if (msg.toolCall?.functionCalls?.length) {
      this.toolsPending += msg.toolCall.functionCalls.length;
      for (const fc of msg.toolCall.functionCalls) if (fc.id && this.ws) this.callSocket.set(fc.id, this.ws);
      this.h.onToolCall(msg.toolCall.functionCalls);
    }
    if (msg.sessionResumptionUpdate?.resumable && msg.sessionResumptionUpdate.newHandle) {
      this.handle = msg.sessionResumptionUpdate.newHandle;
    }
    if (msg.goAway) {
      // The server will end this connection soon. Gemini won't resume a session while its old
      // connection is still open, so switch over in sequence, at the first quiet moment before
      // the deadline, rather than being cut off mid-sentence when time runs out.
      const left = parseSeconds(msg.goAway.timeLeft) ?? 30;
      log(`goAway timeLeft=${msg.goAway.timeLeft ?? "?"}`);
      this.rotateBy = Date.now() + Math.max(0, left - 5) * 1000;
      this.watchForQuiet();
    }
  }

  private watchForQuiet() {
    this.stopRotateWatch();
    this.rotateTimer = window.setInterval(() => {
      const ws = this.ws;
      if (!ws || this.closedByUser || !this.handle) return this.stopRotateWatch();
      const quiet = !this.modelBusy && this.toolsPending === 0 && Date.now() - this.lastUserText > 1500;
      if (!quiet && Date.now() < this.rotateBy) return;
      this.stopRotateWatch();
      log(quiet ? "rotating at a quiet moment" : "rotating at the deadline");
      this.rotating = ws;
      this.ready = false;
      ws.close(1000, "rotate");
    }, 250);
  }

  private stopRotateWatch() {
    window.clearInterval(this.rotateTimer);
    this.rotateTimer = undefined;
  }

  private send(obj: object) {
    if (this.ws?.readyState === WebSocket.OPEN && this.ready) this.ws.send(JSON.stringify(obj));
  }

  /** Send a whole turn now, or keep it for the next connection if this one is switching over. */
  private sendTurn(obj: object) {
    if (this.ready && this.ws?.readyState === WebSocket.OPEN) return this.send(obj);
    if (this.closedByUser || !this.ws) return;
    this.queued.push(obj);
    if (this.queued.length > 20) this.queued.shift();
  }

  sendAudio(pcm: ArrayBuffer) {
    this.send({ realtimeInput: { audio: { data: toBase64(pcm), mimeType: "audio/pcm;rate=16000" } } });
  }

  sendAudioBase64(pcmBase64: string) {
    this.send({ realtimeInput: { audio: { data: pcmBase64, mimeType: "audio/pcm;rate=16000" } } });
  }

  sendAudioEnd() {
    this.send({ realtimeInput: { audioStreamEnd: true } });
  }

  /** A typed message, or a system notice, as a complete user turn. */
  sendText(text: string) {
    this.sendTurn({ clientContent: { turns: [{ role: "user", parts: [{ text }] }], turnComplete: true } });
  }

  /** A user turn with text plus images (e.g. attached logos) that the model can see. */
  sendTextWithImages(text: string, images: { mimeType: string; data: string }[]) {
    const parts: object[] = [{ text }, ...images.map((img) => ({ inlineData: img }))];
    this.sendTurn({ clientContent: { turns: [{ role: "user", parts }], turnComplete: true } });
  }

  sendToolResponses(all: { id: string; name: string; response: object }[]) {
    this.toolsPending = Math.max(0, this.toolsPending - all.length);
    const responses = all.filter((r) => !this.cancelled.delete(r.id));
    const current = responses
      .filter((r) => this.callSocket.get(r.id) === this.ws)
      // Gemini 3.8 Live runs tools in the background and keeps talking. WHEN_IDLE has it speak the
      // result at the next pause instead of cutting itself off mid-sentence.
      .map((r) => ({ ...r, scheduling: "WHEN_IDLE" }));
    const stale = responses.filter((r) => this.callSocket.get(r.id) !== this.ws);
    for (const r of responses) this.callSocket.delete(r.id);
    if (current.length) this.send({ toolResponse: { functionResponses: current } });
    // Asked for before the connection was renewed: the new one doesn't know that call id, so pass
    // the result on as a note instead of losing it.
    for (const r of stale) {
      log(`tool result for ${r.name} arrived after a reconnect; passed on as a note`);
      this.sendText(`[APP] Result of ${r.name}, which you asked for just before the connection was renewed: ${JSON.stringify(r.response)}`);
    }
  }

  close() {
    this.closedByUser = true;
    this.ready = false;
    this.ws?.close();
    this.stopRotateWatch();
    this.ws = undefined;
    this.rotating = undefined;
    this.queued = [];
    this.callSocket.clear();
    this.cancelled.clear();
  }
}
