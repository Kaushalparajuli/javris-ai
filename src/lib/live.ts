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

export class LiveSession {
  private ws?: WebSocket;
  /** A replacement connection being set up after a goAway, swapped in once ready. */
  private next?: WebSocket;
  private handle?: string;
  private closedByUser = false;
  private retries = 0;
  private attempt = 0;
  ready = false;

  constructor(private cfg: LiveConfig, private h: LiveHandlers) {}

  connect() {
    this.closedByUser = false;
    this.ws = this.open(false);
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

  /** Open a socket. A hand-off socket only becomes the active one once its setup completes. */
  private open(handoff: boolean): WebSocket {
    const n = ++this.attempt;
    const resumed = !!this.handle;
    const openedAt = Date.now();
    let wasReady = false;
    log(`#${n} connecting model=${this.cfg.model} resume=${resumed} handoff=${handoff}`);
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
        if (handoff) {
          if (this.next !== ws || this.closedByUser) return ws.close();
          const old = this.ws;
          this.ws = ws;
          this.next = undefined;
          old?.close();
          log(`#${n} hand-off complete`);
          this.ready = true;
          this.retries = 0;
          return;
        }
      }
      if (ws === this.ws) this.route(msg);
      else if (ws === this.next && msg.sessionResumptionUpdate?.newHandle) this.handle = msg.sessionResumptionUpdate.newHandle;
    };
    ws.onclose = (ev) => {
      const secs = ((Date.now() - openedAt) / 1000).toFixed(0);
      log(`#${n} closed code=${ev.code} reason="${ev.reason}" after ${secs}s ready=${wasReady}`);
      if (ws === this.next) {
        // The hand-off connection failed; the current one keeps going until it closes.
        this.next = undefined;
        return;
      }
      if (ws !== this.ws || this.closedByUser) return;
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
        if (!this.closedByUser && this.ws === ws) this.ws = this.open(false);
      }, delay);
    };
    return ws;
  }

  private route(msg: any) {
    if (msg.setupComplete) {
      this.ready = true;
      this.retries = 0;
      this.h.onReady();
    }
    const sc = msg.serverContent;
    if (sc) {
      if (sc.interrupted) this.h.onInterrupted();
      if (sc.inputTranscription?.text) this.h.onInputText(sc.inputTranscription.text);
      if (sc.outputTranscription?.text) this.h.onOutputText(sc.outputTranscription.text);
      for (const part of sc.modelTurn?.parts ?? []) {
        if (part.inlineData?.data) this.h.onAudio(part.inlineData.data);
      }
      if (sc.turnComplete) this.h.onTurnComplete();
    }
    if (msg.toolCall?.functionCalls?.length) this.h.onToolCall(msg.toolCall.functionCalls);
    if (msg.sessionResumptionUpdate?.resumable && msg.sessionResumptionUpdate.newHandle) {
      this.handle = msg.sessionResumptionUpdate.newHandle;
    }
    if (msg.goAway) {
      // The server will end this connection soon. Open the replacement now and swap once it's ready,
      // so the conversation carries on without a gap.
      log(`goAway timeLeft=${msg.goAway.timeLeft ?? "?"}`);
      if (!this.next && this.handle) this.next = this.open(true);
    }
  }

  private send(obj: object) {
    if (this.ws?.readyState === WebSocket.OPEN && this.ready) this.ws.send(JSON.stringify(obj));
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
    this.send({ clientContent: { turns: [{ role: "user", parts: [{ text }] }], turnComplete: true } });
  }

  /** A user turn with text plus images (e.g. attached logos) that the model can see. */
  sendTextWithImages(text: string, images: { mimeType: string; data: string }[]) {
    const parts: object[] = [{ text }, ...images.map((img) => ({ inlineData: img }))];
    this.send({ clientContent: { turns: [{ role: "user", parts }], turnComplete: true } });
  }

  sendToolResponses(responses: { id: string; name: string; response: object }[]) {
    this.send({ toolResponse: { functionResponses: responses } });
  }

  close() {
    this.closedByUser = true;
    this.ready = false;
    this.ws?.close();
    this.next?.close();
    this.ws = undefined;
    this.next = undefined;
  }
}
