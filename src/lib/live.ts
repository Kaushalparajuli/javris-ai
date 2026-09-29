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

export class LiveSession {
  private ws?: WebSocket;
  private handle?: string;
  private closedByUser = false;
  private retries = 0;
  ready = false;

  constructor(private cfg: LiveConfig, private h: LiveHandlers) {}

  connect() {
    this.closedByUser = false;
    const ws = new WebSocket(`${WS}?key=${encodeURIComponent(this.cfg.apiKey)}`);
    this.ws = ws;
    ws.onopen = () => {
      ws.send(
        JSON.stringify({
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
        }),
      );
    };
    ws.onmessage = async (ev) => {
      const text = typeof ev.data === "string" ? ev.data : await (ev.data as Blob).text();
      let msg: any;
      try {
        msg = JSON.parse(text);
      } catch {
        return;
      }
      this.route(msg);
    };
    ws.onclose = (ev) => {
      if (this.ws !== ws) return;
      this.ready = false;
      const reason = ev.reason || `Connection closed (code ${ev.code})`;
      // Retry on network drops or server-side session limits, but not on bad keys/models.
      const fatal = ev.code === 1007 || ev.code === 1008 || /api key|not found|permission|invalid/i.test(ev.reason);
      const retry = !this.closedByUser && !fatal && this.retries < 5;
      this.h.onClosed(reason, retry);
      if (retry) {
        this.retries++;
        setTimeout(() => !this.closedByUser && this.connect(), 800 * this.retries);
      }
    };
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
      // The server is about to end this connection. Reconnect with the resumption handle.
      const old = this.ws;
      this.ws = undefined;
      old?.close();
      this.connect();
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
    this.ws = undefined;
  }
}
