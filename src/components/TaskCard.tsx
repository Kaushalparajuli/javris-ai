import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import type { Task } from "../lib/types";
import type { WorkItem } from "../lib/workItems";
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

const KIND_ICON: Record<string, string> = {
  research: "M10.5 4a6.5 6.5 0 0 1 5.2 10.4l4.4 4.4-1.4 1.4-4.4-4.4A6.5 6.5 0 1 1 10.5 4zm0 2a4.5 4.5 0 1 0 0 9 4.5 4.5 0 0 0 0-9z",
  document: "M6 2h8l6 6v13a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1zm7 1.5V9h5.5zM8 13h8v2H8zm0 4h8v2H8z",
  image: "M4 5h16a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1zm1 2v8.6l3.5-3.5 3 3 4-4L19 14.6V7zm4 1.5a1.5 1.5 0 1 1 0 3 1.5 1.5 0 0 1 0-3z",
  video: "M4 6h11a1 1 0 0 1 1 1v3.5l5-3v9l-5-3V17a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1z",
  site: "M12 3a9 9 0 1 1 0 18 9 9 0 0 1 0-18zm-1.9 2.3A7 7 0 0 0 5.1 11h3a14 14 0 0 1 2-5.7zm3.8 0A14 14 0 0 1 15.9 11h3a7 7 0 0 0-5-5.7zM10.1 11h3.8A12 12 0 0 0 12 5.6 12 12 0 0 0 10.1 11zm-5 2a7 7 0 0 0 5 5.7A14 14 0 0 1 8.1 13zm5 0A12 12 0 0 0 12 18.4a12 12 0 0 0 1.9-5.4zm5.8 0a14 14 0 0 1-2 5.7 7 7 0 0 0 5-5.7z",
  code: "m8.6 7.4 1.4 1.4L6.8 12l3.2 3.2-1.4 1.4L4 12zm6.8 0L20 12l-4.6 4.6-1.4-1.4 3.2-3.2-3.2-3.2z",
  browser: "M12 3a9 9 0 1 1 0 18 9 9 0 0 1 0-18zm0 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14z",
  slides: "M3 4h18v2h-1v10H4V6H3zm3 2v8h12V6zm5 11h2v2l2 2h-2.6L12 19.6 10.6 21H8l2-2z",
};

/**
 * One piece of work in the Activity pane, as a compact row: a thumbnail or icon, the name, and one
 * line of status. Running work adds a thin progress bar and the current step. Opening it shows the
 * full result and summary in the side panel; Finder, Drive and Stop appear on hover.
 */
export default function TaskCard({ task, onOpen, item }: { task: Task; onOpen: (id: number) => void; item?: WorkItem }) {
  const [now, setNow] = useState(Date.now());
  const [drive, setDrive] = useState<{ state: "busy" | "done" | "error"; link?: string; error?: string } | null>(null);
  const running = task.status === "running";
  const check = task.kind === "code" && task.status === "done" && !!task.verify;
  useEffect(() => {
    if (!running) return;
    const i = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(i);
  }, [running]);

  const doneSteps = task.steps.filter((s) => s.done).length;
  const progress = Math.min(92, 8 + doneSteps * 7);
  const step = [...task.steps].reverse().find((s) => !s.done) ?? task.steps[task.steps.length - 1];
  const openId = (!running && item?.open?.id) || task.id;
  const earlier = item ? item.runs.filter((r) => r.id !== task.id).reverse() : [];
  const kind = item?.kind === "video" || item?.kind === "site" ? item.kind : task.kind;
  const kindLabel =
    kind === "video" ? "Video" : kind === "site" ? "Website" : kind === "image" ? "Image" : kind === "document" ? "Document" : kind === "browser" ? "Web task" : kind === "code" ? "Code" : kind === "skill" ? "Saved method" : kind === "slides" ? "Slides" : task.depth === "quick" ? "Quick research" : "Research";
  const images = item && task.kind === "image" ? item.images : task.images;
  const thumb = images[0];
  const status = running
    ? `${step?.label ?? "Starting…"} · ${elapsed(task, now)}`
    : check
      ? CHECK_PILL[task.verify as "running" | "passed" | "failed"]
      : task.status === "done"
        ? `${kindLabel} · ${clock(task.finishedAt ?? task.startedAt)}`
        : `${PILL[task.status]}${task.error ? ` · ${task.error}` : ""}`;
  const canDrive = task.status === "done" && (task.kind === "document" || task.kind === "research" || task.kind === "browser") && !task.routine;

  return (
    // The whole row opens the work in the side panel; its own buttons stop the click there.
    <article
      className={`task compact ${running ? "live" : ""} ${task.status}${check && task.verify === "failed" ? " failed" : ""}`}
      role="button"
      tabIndex={0}
      aria-label={`Open ${item?.title ?? task.title}`}
      title={task.summary || undefined}
      onClick={() => onOpen(openId)}
      onKeyDown={(e) => {
        if ((e.key === "Enter" || e.key === " ") && e.target === e.currentTarget) {
          e.preventDefault();
          onOpen(openId);
        }
      }}
    >
      <div className="tc-row">
        <div className="tc-icon">
          {thumb ? (
            <ImageThumb path={thumb} className="tc-thumb" alt="" />
          ) : (
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d={KIND_ICON[kind] ?? KIND_ICON.research} />
            </svg>
          )}
        </div>
        <div className="tc-text">
          <h3>{item?.title ?? task.title}</h3>
          <div className="tc-status">
            {!running && task.status === "done" && !check && <i className="tc-dot done" aria-hidden="true" />}
            <span>{status}</span>
          </div>
        </div>
        <div className="tc-actions" onClick={(e) => e.stopPropagation()}>
          {running && (
            <button title="Stop" aria-label="Stop" onClick={() => invoke("cancel_task", { id: task.id }).catch(() => {})}>
              <svg viewBox="0 0 24 24"><path d="M7 7h10v10H7z" /></svg>
            </button>
          )}
          {canDrive &&
            (drive?.state === "done" ? (
              <button title="Open in Google Drive" aria-label="Open in Google Drive" onClick={() => openUrl(drive.link!).catch(() => {})}>
                <svg viewBox="0 0 24 24"><path d="M14 3h7v7h-2V6.4l-7.3 7.3-1.4-1.4L17.6 5H14zM5 5h6v2H6v11h11v-5h2v6a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z" /></svg>
              </button>
            ) : (
              <button title={drive?.error ?? (drive?.state === "busy" ? "Saving to Google Drive…" : "Save to Google Drive")} aria-label="Save to Google Drive" disabled={drive?.state === "busy"} onClick={() => saveToDrive(task, setDrive)}>
                <svg viewBox="0 0 24 24"><path d="M12 4a6 6 0 0 1 5.8 4.6A4.5 4.5 0 0 1 17.5 19H7a5 5 0 0 1-.9-9.9A6 6 0 0 1 12 4zm-1 7.4V16h2v-4.6l1.6 1.6 1.4-1.4L12 7.6 8 11.6l1.4 1.4z" /></svg>
              </button>
            ))}
          <button title="Show in Finder" aria-label="Show in Finder" onClick={() => invoke("reveal_task", { id: task.id }).catch(() => {})}>
            <svg viewBox="0 0 24 24"><path d="M3 6a1 1 0 0 1 1-1h6l2 2h8a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z" /></svg>
          </button>
        </div>
      </div>
      {running && (
        <div className="bar">
          <b style={{ width: `${progress}%` }} />
        </div>
      )}
      {earlier.length > 0 && (
        <details className="runs" onClick={(e) => e.stopPropagation()}>
          <summary>{earlier.length + 1} versions</summary>
          <ul>
            {earlier.map((r) => (
              <li key={r.id}>
                <button className="run" onClick={() => onOpen(r.id)}>
                  <span>{r.title}</span>
                  <small>
                    {clock(r.startedAt)} · {PILL[r.status]}
                  </small>
                </button>
              </li>
            ))}
          </ul>
        </details>
      )}
    </article>
  );
}
