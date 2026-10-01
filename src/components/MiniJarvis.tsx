import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import { RISK_INFO } from "../lib/approvals";
import type { MiniCmd, MiniLevel, MiniState } from "../lib/mini";
import MeetingBanner from "./MeetingBanner";
import Orb from "./Orb";

const IDLE: MiniState = { connection: "off", micOn: false, status: "Press the mic or ⌥Space to start", error: "", controlling: "", meeting: null, task: null, approvals: [] };
const LABEL = { off: "Offline", connecting: "Connecting", live: "Listening", error: "Offline" } as const;

const send = (c: MiniCmd) => emit("mini-cmd", c).catch(() => {});

/** The small always-on-top window: orb, status, current task, and approvals. */
export default function MiniJarvis() {
  const [s, setS] = useState<MiniState>(IDLE);
  const feed = useRef<MiniLevel>({ mode: "idle", level: 0 });
  const source = useRef({ mode: () => feed.current.mode, level: () => feed.current.level }).current;
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const a = listen<MiniState>("mini-state", (e) => setS(e.payload));
    const b = listen<MiniLevel>("mini-level", (e) => (feed.current = e.payload));
    // Ask for the current state once the listeners are in place.
    Promise.all([a, b]).then(() => send({ type: "hello" }));
    return () => {
      a.then((f) => f());
      b.then((f) => f());
    };
  }, []);

  // Not live: the orb rests.
  useEffect(() => {
    if (s.connection !== "live") feed.current = { mode: "idle", level: 0 };
  }, [s.connection]);

  // The window is only as tall as what's showing.
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const fit = () => invoke("mini_resize", { height: Math.ceil(el.getBoundingClientRect().height) + 2 }).catch(() => {});
    const ro = new ResizeObserver(fit);
    ro.observe(el);
    fit();
    return () => ro.disconnect();
  }, []);

  const a = s.approvals[0];
  return (
    <div className="mini" ref={box}>
      <div className="mini-bar" data-tauri-drag-region>
        <div className="mini-orb" data-tauri-drag-region>
          <Orb source={source} size={64} />
        </div>
        <div className="mini-state" data-tauri-drag-region>
          <b className={s.connection}>{s.connection === "live" ? (s.micOn ? "Listening" : "Muted") : LABEL[s.connection]}</b>
          <span data-tauri-drag-region>{s.error || s.status}</span>
        </div>
        <button className={`icon-btn mini-mic ${s.micOn ? "" : "off"}`} onClick={() => send({ type: "toggle-mic" })} aria-label={s.micOn ? "Mute microphone" : "Start talking"} title="Talk / mute (⌥Space)">
          <svg viewBox="0 0 24 24">
            <path d="M12 14a3 3 0 0 0 3-3V6a3 3 0 0 0-6 0v5a3 3 0 0 0 3 3zm5-3a5 5 0 0 1-10 0H5a7 7 0 0 0 6 6.9V21h2v-3.1A7 7 0 0 0 19 11z" />
          </svg>
        </button>
        <button className="icon-btn" onClick={() => send({ type: "open" })} aria-label="Open Jarvis" title="Open the full window">
          <svg viewBox="0 0 24 24">
            <path d="M4 4h6v2H7.4l3.3 3.3-1.4 1.4L6 7.4V10H4zm16 16h-6v-2h2.6l-3.3-3.3 1.4-1.4 3.3 3.3V14h2z" />
          </svg>
        </button>
      </div>
      {s.meeting && <MeetingBanner title={s.meeting.title} startedAt={s.meeting.startedAt} onStop={() => send({ type: "stop-meeting" })} />}
      {s.controlling && (
        <div className="mini-control">
          <span>Controlling <b>{s.controlling}</b></span>
          <button className="btn" onClick={() => send({ type: "stop-speaking" })} title="Stop (⌥.)">
            Stop
          </button>
        </div>
      )}
      {s.task && (
        <div className="mini-task">
          <b>{s.task.title}</b>
          <span>
            {s.task.step}
            {s.task.count > 1 ? ` · +${s.task.count - 1} more` : ""}
          </span>
        </div>
      )}
      {a && (
        <div className="mini-approval" role="alertdialog" aria-label="Jarvis needs your approval">
          <div className="label">Jarvis wants to · {RISK_INFO[a.risk].label.toLowerCase()}</div>
          <h4>{a.title}</h4>
          {a.detail && <pre>{a.detail}</pre>}
          {s.approvals.length > 1 && <p className="hint">{s.approvals.length - 1} more waiting after this.</p>}
          <div className="approval-actions">
            <button className="btn" onClick={() => send({ type: "approve", id: a.id, ok: false })}>
              Reject
            </button>
            <button className="btn primary" onClick={() => send({ type: "approve", id: a.id, ok: true })}>
              {a.okLabel}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
