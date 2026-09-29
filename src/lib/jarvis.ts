// The orchestrator: connects the Gemini Live voice session to the Codex worker tasks.
//
// Voice  → Gemini Live calls a tool (start_research, follow_up, …)
// Tools  → Rust starts `codex exec` and streams progress as `task-update` events
// Result → `task-finished` is queued as a [WORKER] notice and sent to Gemini when
//          nobody is talking, so Jarvis speaks the summary at a natural pause.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { NativeMic, Player } from "./audio";
import { FunctionCall, listLiveModels, LiveSession } from "./live";
import { loadImage } from "../components/ImageThumb";
import type { Attachment, Connection, Msg, OrbMode, Settings, Task, Who } from "./types";

/** Shrink an image to a JPEG the Live model can look at (max 1024 px). */
async function toJpeg(dataUrl: string, max = 1024): Promise<string> {
  const img = new Image();
  img.src = dataUrl;
  await img.decode();
  const scale = Math.min(1, max / Math.max(img.width, img.height));
  const c = document.createElement("canvas");
  c.width = Math.round(img.width * scale);
  c.height = Math.round(img.height * scale);
  const g = c.getContext("2d")!;
  g.fillStyle = "#fff"; // transparent logos stay visible
  g.fillRect(0, 0, c.width, c.height);
  g.drawImage(img, 0, 0, c.width, c.height);
  return c.toDataURL("image/jpeg", 0.85).split(",")[1];
}

const TOOLS = [
  {
    name: "start_research",
    description:
      "Start a background research task run by the research worker (an AI agent with web search that writes a report with sources). Use for anything needing current information, several sources, comparisons, or a written report.",
    parameters: {
      type: "OBJECT",
      properties: {
        title: { type: "STRING", description: "Short title for the task, 3-8 words." },
        request: { type: "STRING", description: "Full, specific research request including any constraints the user mentioned." },
        depth: { type: "STRING", enum: ["quick", "deep"], description: "quick = fast fact check; deep = thorough multi-source analysis." },
      },
      required: ["title", "request", "depth"],
    },
  },
  {
    name: "create_image",
    description:
      "Create one or more images with the image worker (runs in the background, usually under two minutes). Use when the user asks you to make, draw, design or generate an image, icon, logo, illustration or picture.",
    parameters: {
      type: "OBJECT",
      properties: {
        title: { type: "STRING", description: "Short title, 2-6 words." },
        prompt: {
          type: "STRING",
          description:
            "The full creative brief: purpose/use, subject, style, colors, composition, mood, and any exact text to render (in quotes).",
        },
        aspect: { type: "STRING", enum: ["square", "landscape", "portrait", "wide", "tall"], description: "square 1:1, landscape 3:2, portrait 2:3, wide 16:9, tall 9:16." },
        count: { type: "INTEGER", description: "How many variations, 1-4. Default 1." },
        use_attachments: { type: "BOOLEAN", description: "true to give the worker the images the user attached (logo, references)." },
        reference_instructions: {
          type: "STRING",
          description: "How the worker should use each attachment, e.g. 'logo.png is the company logo, place it top-left unchanged; photo.jpg is a style reference only'.",
        },
      },
      required: ["title", "prompt"],
    },
  },
  {
    name: "follow_up",
    description:
      "Continue an existing task: a follow-up question about research, or changes to an image (e.g. 'make it darker'). The worker keeps the same context.",
    parameters: {
      type: "OBJECT",
      properties: {
        task_id: { type: "INTEGER" },
        question: { type: "STRING" },
      },
      required: ["task_id", "question"],
    },
  },
  {
    name: "task_status",
    description: "Get the status of recent research tasks (running, done, failed) with their summaries.",
    parameters: { type: "OBJECT", properties: {} },
  },
  {
    name: "cancel_task",
    description: "Stop a running research task.",
    parameters: { type: "OBJECT", properties: { task_id: { type: "INTEGER" } }, required: ["task_id"] },
  },
  {
    name: "open_report",
    description: "Show a task's result on the user's screen: the written report for research, or the images for an image task.",
    parameters: { type: "OBJECT", properties: { task_id: { type: "INTEGER" } }, required: ["task_id"] },
  },
  {
    name: "save_note",
    description: "Save a note to the user's notes file when they ask you to remember or note something.",
    parameters: { type: "OBJECT", properties: { text: { type: "STRING" } }, required: ["text"] },
  },
];

let userName = "";
const name0 = () => userName || "The user";

function systemPrompt(s: Settings, notes: string) {
  const name = s.userName.trim() || "the user";
  const recentNotes = notes.trim().split("\n").slice(-20).join("\n").replace(/<!--.*?-->/g, "");
  return `You are Jarvis, a calm, sharp and slightly witty voice assistant for ${name}. You talk like a trusted chief of staff: short, natural spoken sentences.

You have a research worker: a separate AI agent that searches the web, reads sources and writes a report. It is thorough but slow (one to ten minutes).
- When ${name} asks for anything that needs current information, several sources, comparisons, or a written report, call start_research. Choose depth "quick" for simple fact checks and "deep" for analysis. If the request is ambiguous, ask one short question first.
- After starting a task, say in one sentence that it's underway, then keep the conversation going. Never invent research results or pretend a task finished.
- Messages that start with [WORKER] come from the system, not from ${name}. When one arrives, tell ${name} the result naturally in two to four sentences, then offer to open the report or dig deeper.
- Images: before calling create_image, make sure you have what's needed for a great result. Unless the request is already specific, ask short questions, one or two at a time:
  what it's for (logo, app icon, social post, poster, website hero, wallpaper…), the style (photo, 3D, flat, illustration, minimal…), colors or brand, any exact text that must appear, the shape (square, wide, tall), and whether they have a logo, product photo or reference image. Tell ${name} they can drag images onto the Jarvis window or click the paperclip to attach them.
  Keep it to two or three short rounds at most. If ${name} says "just make it" or similar, go ahead with sensible choices. Before starting, confirm the brief in one sentence.
  Messages starting with [APP] come from the app, not ${name}. When one says images were attached, look at them, say briefly what you see, and ask how to use them if it isn't obvious (include exactly, restyle, or just inspiration).
  When calling create_image with attachments, set use_attachments to true and explain in reference_instructions exactly how each file should be used. After starting, say it's being made; describe the result when the [WORKER] notice arrives.
- Use follow_up for questions about an existing task's findings or for changes to an image, task_status when asked about progress, open_report to show a report or images, save_note when asked to remember something.
- For casual conversation or things you already know well, answer directly without tools.
- You are speaking, not writing: no markdown, no lists, no URLs read aloud.
${recentNotes ? `\nThings ${name} asked you to remember:\n${recentNotes}` : ""}`;
}

export function useJarvis() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [connection, setConnection] = useState<Connection>("off");
  const [status, setStatus] = useState("Press the mic or ⌥Space to start");
  const [micOn, setMicOn] = useState(false);
  const [messages, setMessages] = useState<Msg[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [reportId, setReportId] = useState<number | null>(null);
  const [error, setError] = useState("");
  const [micBlocked, setMicBlocked] = useState(false);
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const attachmentsRef = useRef<Attachment[]>([]);
  const unsentAttachments = useRef<Attachment[]>([]);
  attachmentsRef.current = attachments;

  const live = useRef<LiveSession | null>(null);
  const mic = useRef<NativeMic | null>(null);
  const headphones = useRef(false);
  const micDevice = useRef("");
  const player = useRef<Player | null>(null);
  const micOnRef = useRef(false);
  const tasksRef = useRef<Task[]>([]);
  const notices = useRef<string[]>([]);
  const lastUserSpeech = useRef(0);
  const msgSeq = useRef(0);
  const currentUser = useRef<number | null>(null);
  const currentJarvis = useRef<number | null>(null);

  tasksRef.current = tasks;
  micOnRef.current = micOn;

  // ---------- transcript ----------
  const push = useCallback((who: Who, text: string) => {
    const id = ++msgSeq.current;
    setMessages((m) => [...m.slice(-200), { id, who, text }]);
    return id;
  }, []);
  const append = useCallback((id: number, text: string) => {
    setMessages((m) => m.map((x) => (x.id === id ? { ...x, text: x.text + text } : x)));
  }, []);

  // ---------- settings + tasks ----------
  const reloadSettings = useCallback(async () => {
    const s = await invoke<Settings>("get_settings");
    setSettings(s);
    headphones.current = s.headphones;
    userName = s.userName.trim();
    micDevice.current = s.micDevice ?? "";
    return s;
  }, []);

  useEffect(() => {
    reloadSettings();
    invoke<Task[]>("list_tasks").then(setTasks);
    invoke<string>("mic_status").then((st) => {
      if (st === "undetermined") invoke("request_mic");
      if (st === "denied" || st === "restricted") setMicBlocked(true);
    });
    const unUpdate = listen<Task>("task-update", (e) => {
      setTasks((list) => {
        const rest = list.filter((t) => t.id !== e.payload.id);
        return [e.payload, ...rest].sort((a, b) => b.id - a.id);
      });
    });
    const unFinish = listen<Task>("task-finished", (e) => {
      const t = e.payload;
      const body =
        t.status === "done" && t.kind === "image"
          ? `Image task #${t.id} "${t.title}" is finished: ${t.images.length} image(s) are on screen now. Worker says: ${t.summary}`
          : t.status === "done"
          ? `Research task #${t.id} "${t.title}" is finished. Summary: ${t.summary} (${t.sources} sources; the full report is available.)`
          : t.status === "cancelled"
            ? `Research task #${t.id} "${t.title}" was cancelled.`
            : `Research task #${t.id} "${t.title}" failed: ${t.error}`;
      notices.current.push(`[WORKER] ${body}`);
      if (t.status === "done" && t.kind === "image") setReportId(t.id);
    });
    return () => {
      unUpdate.then((f) => f());
      unFinish.then((f) => f());
    };
  }, [reloadSettings]);

  // Deliver worker notices when Jarvis is quiet and the user hasn't spoken for a moment.
  useEffect(() => {
    const timer = setInterval(() => {
      const session = live.current;
      if (!notices.current.length || !session?.ready) return;
      if (player.current?.speaking) return;
      if (Date.now() - lastUserSpeech.current < 1500) return;
      const text = notices.current.shift()!;
      push("notice", text.replace("[WORKER] ", ""));
      currentJarvis.current = null;
      session.sendText(text);
    }, 500);
    return () => clearInterval(timer);
  }, [push]);

  // ---------- tools ----------
  const runTool = useCallback(
    async (fc: FunctionCall): Promise<object> => {
      const a = fc.args ?? {};
      try {
        switch (fc.name) {
          case "start_research": {
            const depth = a.depth === "quick" ? "quick" : "deep";
            const t = await invoke<Task>("start_task", {
              title: String(a.title ?? a.request ?? "Research"),
              request: String(a.request ?? a.title ?? ""),
              depth,
            });
            push("tool", `→ start_research · ${depth} · task #${t.id}`);
            return { task_id: t.id, status: "started", note: "Running in the background. You will get a [WORKER] notice when it finishes." };
          }
          case "create_image": {
            const refs = a.use_attachments ? attachmentsRef.current.map((x) => x.path) : [];
            const t = await invoke<Task>("start_image", {
              title: String(a.title ?? "Image"),
              prompt: String(a.prompt ?? a.title ?? ""),
              count: a.count ? Number(a.count) : 1,
              aspect: a.aspect ? String(a.aspect) : "square",
              refs,
              refNotes: String(a.reference_instructions ?? ""),
            });
            if (refs.length) setAttachments([]);
            push("tool", `→ create_image · ${a.aspect ?? "square"}${refs.length ? ` · ${refs.length} reference${refs.length > 1 ? "s" : ""}` : ""} · task #${t.id}`);
            return { task_id: t.id, status: "started", note: "Generating in the background. You will get a [WORKER] notice when it's ready." };
          }
          case "follow_up": {
            const t = await invoke<Task>("followup_task", { id: Number(a.task_id), question: String(a.question ?? "") });
            push("tool", `→ follow_up · task #${a.task_id} → #${t.id}`);
            return { task_id: t.id, status: "started" };
          }
          case "task_status": {
            push("tool", "→ task_status");
            return {
              tasks: tasksRef.current.slice(0, 8).map((t) => ({
                id: t.id,
                kind: t.kind,
                title: t.title,
                status: t.status,
                current_step: t.status === "running" ? t.steps[t.steps.length - 1]?.label ?? "starting" : undefined,
                summary: t.summary || undefined,
                error: t.error || undefined,
              })),
            };
          }
          case "cancel_task":
            await invoke("cancel_task", { id: Number(a.task_id) });
            push("tool", `→ cancel_task · #${a.task_id}`);
            return { ok: true };
          case "open_report":
            setReportId(Number(a.task_id));
            push("tool", `→ open_report · #${a.task_id}`);
            return { ok: true, note: "The report is now on screen." };
          case "save_note":
            await invoke("append_note", { text: String(a.text ?? "") });
            push("tool", "→ save_note");
            return { ok: true };
          default:
            return { error: `Unknown tool ${fc.name}` };
        }
      } catch (e) {
        const msg = String(e);
        push("tool", `✕ ${fc.name}: ${msg}`);
        return { error: msg };
      }
    },
    [push],
  );

  // ---------- attachments ----------
  /** Show attached images to the Live model so Jarvis can talk about them. */
  const shareAttachments = useCallback(async (list: Attachment[]) => {
    const session = live.current;
    if (!session?.ready) {
      unsentAttachments.current.push(...list);
      return;
    }
    const images = [];
    for (const a of list) {
      try {
        images.push({ mimeType: "image/jpeg", data: await toJpeg(await loadImage(a.path)) });
      } catch {
        /* unreadable image: still mention the file */
      }
    }
    const names = list.map((a) => a.name).join(", ");
    currentJarvis.current = null;
    session.sendTextWithImages(
      `[APP] ${name0()} attached ${list.length} image${list.length > 1 ? "s" : ""}: ${names}. They are shown below. They will be passed to the image worker if you set use_attachments.`,
      images,
    );
  }, []);

  const attach = useCallback(
    async (paths: string[]) => {
      if (!paths.length) return;
      try {
        const added = await invoke<Attachment[]>("import_attachments", { paths });
        if (!added.length) return;
        setAttachments((cur) => [...cur, ...added].slice(-6));
        push("notice", `Attached ${added.map((a) => a.name).join(", ")}`);
        shareAttachments(added);
      } catch (e) {
        setError(String(e));
      }
    },
    [push, shareAttachments],
  );

  const removeAttachment = useCallback((path: string) => {
    setAttachments((cur) => cur.filter((a) => a.path !== path));
  }, []);

  // ---------- session ----------
  const stopMic = useCallback(() => {
    mic.current?.stop();
    mic.current = null;
    live.current?.sendAudioEnd();
    setMicOn(false);
  }, []);

  const startMic = useCallback(async () => {
    // Ask macOS first; the web view sees no microphone until the app itself is allowed.
    const allowed = await invoke<boolean>("request_mic").catch(() => true);
    if (!allowed) {
      setMicBlocked(true);
      setError("Jarvis isn't allowed to use the microphone. Turn on Jarvis under Privacy & Security → Microphone, then press the mic again.");
      return;
    }
    setMicBlocked(false);
    const m = new NativeMic();
    m.onChunk = (pcm) => {
      if (!micOnRef.current) return;
      // Without headphones, pause listening while Jarvis talks so it doesn't hear itself.
      if (!headphones.current && player.current?.speaking) return;
      live.current?.sendAudioBase64(pcm);
    };
    try {
      await m.start(micDevice.current);
    } catch (e) {
      setError(`Microphone unavailable: ${e}`);
      return;
    }
    mic.current = m;
    micOnRef.current = true;
    setMicOn(true);
  }, []);

  // Switch microphones right away if the choice changes mid-conversation.
  useEffect(() => {
    if (!micOnRef.current || !mic.current) return;
    mic.current.stop();
    mic.current = null;
    micOnRef.current = false;
    startMic();
  }, [settings?.micDevice]);

  const connect = useCallback(async () => {
    const s = await reloadSettings();
    if (!s.geminiApiKey) {
      setError("Add your Gemini API key in Settings first.");
      return false;
    }
    setError("");
    setConnection("connecting");
    setStatus("Connecting to Gemini Live…");
    let model = s.geminiModel;
    if (!model) {
      try {
        model = (await listLiveModels(s.geminiApiKey))[0];
      } catch (e) {
        setConnection("error");
        setError(`Could not reach Gemini: ${e}`);
        return false;
      }
      if (!model) {
        setConnection("error");
        setError("Your API key has no access to a Live model. Pick one in Settings.");
        return false;
      }
    }
    const notes = await invoke<string>("read_notes").catch(() => "");
    player.current ??= new Player();
    await player.current.resume();

    const session = new LiveSession(
      { apiKey: s.geminiApiKey, model, voice: s.voice || "Charon", systemPrompt: systemPrompt(s, notes), tools: TOOLS },
      {
        onReady: () => {
          setConnection("live");
          setStatus(`Live · ${model}`);
          if (unsentAttachments.current.length) {
            const pending = unsentAttachments.current.splice(0);
            setTimeout(() => shareAttachments(pending), 300);
          }
        },
        onClosed: (reason, retry) => {
          console.warn("Gemini Live closed:", reason);
          if (retry) {
            setConnection("connecting");
            setStatus(`Reconnecting… (${reason})`);
          } else {
            setConnection("error");
            setError(reason);
            setStatus("Disconnected");
          }
        },
        onAudio: (b64) => {
          currentUser.current = null;
          player.current?.play(b64);
        },
        onInputText: (text) => {
          lastUserSpeech.current = Date.now();
          if (currentUser.current == null) currentUser.current = push("you", text.trimStart());
          else append(currentUser.current, text);
        },
        onOutputText: (text) => {
          currentUser.current = null;
          if (currentJarvis.current == null) currentJarvis.current = push("jarvis", text.trimStart());
          else append(currentJarvis.current, text);
        },
        onTurnComplete: () => {
          currentJarvis.current = null;
          currentUser.current = null;
        },
        onInterrupted: () => {
          player.current?.stop();
          currentJarvis.current = null;
        },
        onToolCall: async (calls) => {
          const responses = await Promise.all(
            calls.map(async (fc) => ({ id: fc.id, name: fc.name, response: await runTool(fc) })),
          );
          live.current?.sendToolResponses(responses);
        },
      },
    );
    live.current?.close();
    live.current = session;
    session.connect();
    return true;
  }, [append, push, reloadSettings, runTool, shareAttachments]);

  const disconnect = useCallback(() => {
    stopMic();
    live.current?.close();
    live.current = null;
    player.current?.stop();
    setConnection("off");
    setStatus("Press the mic or ⌥Space to start");
  }, [stopMic]);

  const toggleMic = useCallback(async () => {
    if (micOnRef.current) {
      stopMic();
      return;
    }
    if (!live.current) {
      const ok = await connect();
      if (!ok) return;
    }
    await startMic();
  }, [connect, startMic, stopMic]);

  const stopSpeaking = useCallback(() => {
    player.current?.stop();
  }, []);

  const sendTyped = useCallback(
    async (text: string) => {
      if (!text.trim()) return;
      if (!live.current) {
        const ok = await connect();
        if (!ok) return;
        // Wait for setup to finish before sending.
        const session = () => live.current as LiveSession | null;
        for (let i = 0; i < 50 && !session()?.ready; i++) await new Promise((r) => setTimeout(r, 100));
      }
      push("you", text.trim());
      currentUser.current = null;
      live.current?.sendText(text.trim());
    },
    [connect, push],
  );

  // Global shortcuts from the Rust side.
  useEffect(() => {
    const un = listen<string>("shortcut", (e) => {
      if (e.payload === "toggle-mic") toggleMic();
      if (e.payload === "stop-speaking") stopSpeaking();
    });
    return () => {
      un.then((f) => f());
    };
  }, [toggleMic, stopSpeaking]);

  // ---------- orb feed (read every animation frame, no re-render) ----------
  const orb = useRef({
    mode: (): OrbMode => {
      if (!live.current) return "idle";
      if (player.current?.speaking) return "speak";
      const running = tasksRef.current.some((t) => t.status === "running");
      const micLevel = mic.current?.level() ?? 0;
      if (running && micLevel < 0.02) return "think";
      return "listen";
    },
    level: (): number => {
      if (player.current?.speaking) return Math.min(1, player.current.level() * 4.5);
      return Math.min(1, (mic.current?.level() ?? 0) * 6);
    },
  }).current;

  return {
    settings,
    reloadSettings,
    connection,
    status,
    micOn,
    messages,
    tasks,
    reportId,
    setReportId,
    error,
    setError,
    micBlocked,
    attachments,
    attach,
    removeAttachment,
    openMicSettings: () => invoke("open_mic_settings"),
    toggleMic,
    disconnect,
    stopSpeaking,
    sendTyped,
    orb,
  };
}
