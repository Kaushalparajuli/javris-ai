// Telegram remote: the owner talks to Jarvis from their phone.
//
// The Rust side (src-tauri/src/telegram.rs) listens to the one paired chat and sends each text as a
// `telegram-message` event. This bridge turns it into a typed turn in Jarvis's live session, collects
// Jarvis's reply (the transcript of what it would have said) and sends it back as text. While a phone
// turn is going, the Mac stays quiet, approvals are also sent to the phone as buttons, and work
// started during the turn is sent to the phone when it finishes.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";
import { setRemoteApprover } from "./approvals";
import type { Task } from "./types";

export interface TelegramBridgeOptions {
  /** Give Jarvis's live session a typed turn, connecting first if it isn't connected. */
  sendTyped(text: string): void | Promise<void>;
  /** What to call the owner ("Kaushal"). */
  userName(): string;
}

export interface TelegramBridge {
  /** Feed every piece of Jarvis's output transcription (what it says) here. */
  onJarvisText(text: string): void;
  /** Call when the live session reports turnComplete. */
  onTurnComplete(): void;
  /** True while Jarvis is answering the phone: don't play its voice on the Mac. */
  isRemoteTurn(): boolean;
  /** Call when the owner talks or types on the Mac, so replies go back to the screen and speakers. */
  endRemoteTurn(): void;
  stop(): void;
}

/** After a reply has been sent, keep listening this long for more (a tool may still be working). */
const LINGER_MS = 20_000;
/** A turn with no reply at all gives up after this long. */
const QUIET_MS = 120_000;
/** Telegram shows "typing…" for about five seconds. */
const TYPING_EVERY_MS = 4_500;
const MAX_FILES = 4;

interface TurnWindow {
  from: number;
  to: number | null;
}

interface Asked {
  answer: (ok: boolean) => void;
  text: string;
  messageId: number | null;
  /** Answered before Telegram said which message it was. */
  closedWith: boolean | null;
}

const send = (text: string) => invoke("telegram_send", { text }).catch(() => {});

/** The file worth sending for a finished task, besides its summary. */
function filesFor(t: Task): string[] {
  const dir = t.dir.replace(/\/+$/, "");
  switch (t.kind) {
    case "image":
      return t.images.slice(-MAX_FILES);
    case "document":
      return [`${dir}/document.md`];
    case "slides":
      return [`${dir}/deck.pptx`];
    case "skill":
      return [`${dir}/SKILL.md`];
    default:
      return [`${dir}/report.md`];
  }
}

export function startTelegramBridge(opts: TelegramBridgeOptions): TelegramBridge {
  let active = false;
  let buffer = "";
  let endTimer: ReturnType<typeof setTimeout> | null = null;
  let typingTimer: ReturnType<typeof setInterval> | null = null;
  let stopped = false;
  /** When phone turns were going, so finished work can be traced back to the phone. */
  const windows: TurnWindow[] = [];
  const asked = new Map<number, Asked>();
  const unlisten: Promise<UnlistenFn>[] = [];

  const typing = () => invoke("telegram_typing").catch(() => {});

  const endAfter = (ms: number) => {
    if (endTimer) clearTimeout(endTimer);
    endTimer = setTimeout(() => {
      // Never end while an approval is waiting on the phone.
      if (asked.size) return endAfter(ms);
      end();
    }, ms);
  };

  const begin = () => {
    if (!active) windows.push({ from: Date.now(), to: null });
    if (windows.length > 50) windows.shift();
    active = true;
    typing();
    if (!typingTimer) typingTimer = setInterval(typing, TYPING_EVERY_MS);
    endAfter(QUIET_MS);
  };

  const flush = () => {
    const text = buffer.replace(/\s+\n/g, "\n").trim();
    buffer = "";
    if (text) send(text);
    return !!text;
  };

  const end = () => {
    if (endTimer) clearTimeout(endTimer);
    if (typingTimer) clearInterval(typingTimer);
    endTimer = typingTimer = null;
    flush();
    if (active) {
      const w = windows[windows.length - 1];
      if (w && w.to === null) w.to = Date.now();
    }
    active = false;
  };

  const startedFromPhone = (t: Task) => windows.some((w) => t.startedAt >= w.from - 1000 && t.startedAt <= (w.to ?? Date.now()) + 5000);

  unlisten.push(
    listen<{ id: number; text: string }>("telegram-message", async (e) => {
      const text = e.payload.text.trim();
      if (!text || stopped) return;
      begin();
      try {
        await opts.sendTyped(`[APP] From ${opts.userName() || "the owner"}'s phone via Telegram: ${text}. Reply in short written form; they can't hear you.`);
      } catch (err) {
        send(`Jarvis couldn't take that: ${err}`);
        end();
      }
    }),
  );

  unlisten.push(
    listen<{ id: string; data: string; messageId: number }>("telegram-callback", (e) => {
      const m = /^(ok|no):(\d+)$/.exec(e.payload.data);
      const a = m ? asked.get(Number(m[2])) : undefined;
      if (!m || !a) {
        // A button from an approval that's already been answered or has gone.
        invoke("telegram_ask_close", { messageId: e.payload.messageId, text: null }).catch(() => {});
        return;
      }
      a.answer(m[1] === "ok");
    }),
  );

  unlisten.push(
    listen<Task>("task-finished", async (e) => {
      const t = e.payload;
      if (t.routine || t.status === "cancelled" || !startedFromPhone(t)) return;
      if (t.status !== "done") {
        send(`${t.title} failed: ${t.error || "no reason was given."}`);
        return;
      }
      await send(`${t.title} is ready.${t.summary ? `\n\n${t.summary}` : ""}`);
      for (const path of filesFor(t)) {
        await invoke("telegram_send_file", { path, caption: null }).catch(() => {});
      }
    }),
  );

  setRemoteApprover((approval, answer) => {
    if (!active) return;
    const text = `Jarvis needs your OK\n\n${approval.title}${approval.detail ? `\n\n${approval.detail.slice(0, 1500)}` : ""}`;
    const entry: Asked = { answer, text, messageId: null, closedWith: null };
    asked.set(approval.id, entry);
    const close = (messageId: number, ok: boolean) =>
      invoke("telegram_ask_close", { messageId, text: `${text}\n\n${ok ? "✓ Approved" : "✕ Declined"}` }).catch(() => {});
    invoke<number>("telegram_ask", { text, approveLabel: approval.okLabel, id: approval.id })
      .then((messageId) => {
        entry.messageId = messageId;
        if (entry.closedWith !== null) close(messageId, entry.closedWith);
      })
      .catch(() => asked.delete(approval.id));
    return (ok) => {
      asked.delete(approval.id);
      if (entry.messageId !== null) close(entry.messageId, ok);
      else entry.closedWith = ok;
    };
  });

  return {
    onJarvisText(text) {
      if (!active) return;
      buffer += text;
      endAfter(QUIET_MS);
    },
    onTurnComplete() {
      if (!active) return;
      // A reply was sent: wait a little in case Jarvis says more after a tool finishes. No reply
      // yet: a tool is probably still working, so keep waiting.
      if (flush()) endAfter(LINGER_MS);
    },
    isRemoteTurn: () => active,
    endRemoteTurn: () => {
      if (active) end();
    },
    stop() {
      stopped = true;
      end();
      setRemoteApprover(null);
      unlisten.forEach((u) => u.then((f) => f()));
    },
  };
}

/**
 * The bridge for a React component: started on mount, stopped on unmount. The options may change
 * every render; the latest ones are always used. The returned object is stable.
 */
export function useTelegramBridge(opts: TelegramBridgeOptions): TelegramBridge {
  const latest = useRef(opts);
  latest.current = opts;
  const bridge = useRef<TelegramBridge | null>(null);
  const proxy = useRef<TelegramBridge>({
    onJarvisText: (t) => bridge.current?.onJarvisText(t),
    onTurnComplete: () => bridge.current?.onTurnComplete(),
    isRemoteTurn: () => bridge.current?.isRemoteTurn() ?? false,
    endRemoteTurn: () => bridge.current?.endRemoteTurn(),
    stop: () => bridge.current?.stop(),
  });
  useEffect(() => {
    bridge.current = startTelegramBridge({
      sendTyped: (text) => latest.current.sendTyped(text),
      userName: () => latest.current.userName(),
    });
    return () => {
      bridge.current?.stop();
      bridge.current = null;
    };
  }, []);
  return proxy.current;
}
