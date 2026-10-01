import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import type { Task } from "../lib/types";
import ImageThumb from "./ImageThumb";

function elapsed(t: Task, now: number) {
  const ms = (t.finishedAt ?? now) - t.startedAt;
  const s = Math.max(0, Math.round(ms / 1000));
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
}

function clock(ms: number) {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

const PILL: Record<Task["status"], string> = {
  running: "Running",
  done: "Done",
  failed: "Failed",
  cancelled: "Cancelled",
};

/** A finished code task shows how the project's own checks went instead of just "Done". */
const CHECK_PILL = { running: "Checking…", passed: "Done · checks pass", failed: "Checks failed" };

/** Upload a finished document or report to Google Drive as a Google Doc. */
function saveToDrive(task: Task, set: (d: { state: "busy" | "done" | "error"; link?: string; error?: string }) => void) {
  set({ state: "busy" });
  const file = task.kind === "document" ? "document.md" : "report.md";
  invoke<{ link: string }>("drive_upload", { path: `${task.dir}/${file}`, name: task.title })
    .then((f) => set({ state: "done", link: f.link }))
    .catch((e) => set({ state: "error", error: String(e) }));
}

export default function TaskCard({ task, onOpen }: { task: Task; onOpen: (id: number) => void }) {
  const [now, setNow] = useState(Date.now());
  const [drive, setDrive] = useState<{ state: "busy" | "done" | "error"; link?: string; error?: string } | null>(null);
  const running = task.status === "running";
  const check = task.kind === "code" && task.status === "done" && !!task.verify;
  useEffect(() => {
    if (!running) return;
    const i = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(i);
  }, [running]);

  const steps = task.steps.slice(-6);
  const doneSteps = task.steps.filter((s) => s.done).length;
  const progress = running ? Math.min(92, 8 + doneSteps * 7) : 100;

  return (
    // The whole card opens the task in the side panel; its own buttons stop the click there.
    <article
      className={`task ${running ? "live" : ""} ${task.status}`}
      role="button"
      tabIndex={0}
      aria-label={`Open ${task.title}`}
      onClick={() => onOpen(task.id)}
      onKeyDown={(e) => {
        if ((e.key === "Enter" || e.key === " ") && e.target === e.currentTarget) {
          e.preventDefault();
          onOpen(task.id);
        }
      }}
    >
      <div className="task-top">
        <h3>{task.title}</h3>
        <span className={`pill ${check ? (task.verify === "running" ? "running" : task.verify === "failed" ? "failed" : "done") : task.status}`}>
          {check ? CHECK_PILL[task.verify as "running" | "passed" | "failed"] : PILL[task.status]}
          {!check && !running && task.status === "done" ? ` · ${elapsed(task, now)}` : ""}
        </span>
      </div>
      <div className="meta">
        #{task.id} · {task.kind === "image" ? "image" : task.kind === "document" ? "document" : task.kind === "browser" ? "browser" : task.kind === "code" ? "code" : task.kind === "skill" ? "know-how" : task.depth === "quick" ? "quick" : "deep dive"} · {clock(task.startedAt)}
        {running ? ` · ${elapsed(task, now)}` : ""}
        {task.parentId ? ` · follow-up to #${task.parentId}` : ""}
        {task.refs?.length ? ` · ${task.refs.length} reference${task.refs.length > 1 ? "s" : ""}` : ""}
      </div>
      {running && (
        <div className="bar">
          <b style={{ width: `${progress}%` }} />
        </div>
      )}
      {running && steps.length > 0 && (
        <ul className="steps">
          {steps.map((s, i) => (
            <li key={s.id || i} className={s.done ? "done" : "now"}>
              <i />
              <span>
                {s.label}
                {s.detail && <small>{s.detail}</small>}
              </span>
            </li>
          ))}
        </ul>
      )}
      {running && steps.length === 0 && <p className="muted small">Starting Codex…</p>}
      {task.kind === "image" && task.images.length > 0 && (
        <div className={`thumbs n${Math.min(task.images.length, 4)}`}>
          {task.images.slice(0, 4).map((p) => (
            <ImageThumb key={p} path={p} className="thumb" alt={task.title} />
          ))}
        </div>
      )}
      {task.summary && !running && <p className="summary">{task.summary}</p>}
      {task.error && task.status !== "done" && <p className="error-text">{task.error}</p>}
      {!running && (task.sources > 0 || task.searches > 0) && (
        <div className="sources">
          {task.sources > 0 && <span>{task.sources} sources</span>}
          {task.searches > 0 && <span>{task.searches} searches</span>}
        </div>
      )}
      <div className="actions" onClick={(e) => e.stopPropagation()}>
        {task.status === "done" && (
          <button className="mini" onClick={() => onOpen(task.id)}>
            {task.kind === "image" || task.kind === "browser" ? "View" : task.kind === "document" ? "Open document" : "Open report"}
          </button>
        )}
        <button className="mini" onClick={() => invoke("reveal_task", { id: task.id }).catch(() => {})}>
          Show file
        </button>
        {task.status === "done" && (task.kind === "document" || task.kind === "research" || task.kind === "browser") && !task.routine && (
          drive?.state === "done" ? (
            <button className="mini" onClick={() => openUrl(drive.link!).catch(() => {})}>
              Saved · Open in Drive
            </button>
          ) : (
            <button className="mini" disabled={drive?.state === "busy"} title={drive?.error} onClick={() => saveToDrive(task, setDrive)}>
              {drive?.state === "busy" ? "Saving…" : drive?.state === "error" ? "Retry Drive" : "Save to Drive"}
            </button>
          )
        )}
        {running && (
          <button className="mini" onClick={() => invoke("cancel_task", { id: task.id }).catch(() => {})}>
            Stop
          </button>
        )}
      </div>
    </article>
  );
}
