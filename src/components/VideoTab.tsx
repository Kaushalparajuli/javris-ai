import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import DOMPurify from "dompurify";
import { marked } from "marked";
import { useMemo, useState } from "react";
import type { VideoInfo, VideoProgress } from "../lib/types";

/** The steps a video goes through, in order. */
const STEPS = [
  { id: "plan", label: "Storyboard", stages: ["plan", "storyboard"] },
  { id: "media", label: "Media", stages: ["media"] },
  { id: "compose", label: "Build", stages: ["compose"] },
  { id: "check", label: "Check", stages: ["check", "fix"] },
  { id: "render", label: "Render", stages: ["render"] },
] as const;

const kb = (n: number) => (n < 1_048_576 ? `${Math.max(1, Math.round(n / 1024))} KB` : `${(n / 1_048_576).toFixed(1)} MB`);

/**
 * A video in the side panel: the storyboard to approve, where the making has got to, the finished
 * video to play, and a way to ask for changes or a final render.
 */
export default function VideoTab({
  folder,
  baseUrl,
  info,
  progress,
  chatId,
  onChanged,
}: {
  folder: string;
  baseUrl: string;
  info: VideoInfo;
  progress?: VideoProgress;
  chatId: string;
  onChanged: () => void;
}) {
  const [feedback, setFeedback] = useState("");
  const [asking, setAsking] = useState(false);
  const [draft, setDraft] = useState("");
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);

  const stage = progress?.stage ?? (info.renders.length ? "done" : "idle");
  const working = !!progress && !["done", "error"].includes(progress.stage);
  const active = STEPS.findIndex((s) => (s.stages as readonly string[]).includes(stage));
  const latest = info.renders[0];
  const board = useMemo(() => (progress?.board ? DOMPurify.sanitize(marked.parse(progress.board, { async: false }) as string) : ""), [progress?.board]);
  const portrait = info.meta.format === "portrait";

  const run = async (what: string, f: () => Promise<unknown>) => {
    setNote("");
    setBusy(true);
    try {
      await f();
    } catch (e) {
      setNote(`${what}: ${e}`);
    }
    setBusy(false);
  };
  const answer = (approve: boolean) =>
    run("Couldn't send that", async () => {
      await invoke("video_answer", { folder, approve, feedback: approve ? null : feedback.trim() });
      setFeedback("");
      setAsking(false);
    });
  const send = () =>
    run("Couldn't start the change", async () => {
      await invoke("video_edit", { folder, instructions: draft.trim(), region: null, chatId });
      setDraft("");
    });
  const renderFinal = () =>
    run("Couldn't render it", async () => {
      await invoke("video_render", { folder, quality: "final" });
      onChanged();
    });

  return (
    <div className="vt">
      <ol className="vt-steps" aria-label="Progress">
        {STEPS.map((s, i) => (
          <li key={s.id} className={i < active || stage === "done" ? "done" : i === active ? (stage === "error" ? "bad" : "now") : ""}>
            <i />
            {s.label}
          </li>
        ))}
      </ol>

      {working && (
        <div className="vt-status" role="status">
          <i />
          <span>
            {progress!.message}
            {progress!.render && stage === "render" && <small> {progress!.render.percent}% · {progress!.render.message}</small>}
          </span>
          <button className="mini" onClick={() => invoke("video_stop", { folder }).catch(() => {})}>
            Stop
          </button>
        </div>
      )}
      {stage === "error" && progress?.error && <p className="error-text vt-note">{progress.error}</p>}
      {stage === "done" && progress?.note && !working && <p className="muted small vt-note">{progress.note}</p>}
      {note && <p className="error-text vt-note">{note}</p>}

      {stage === "storyboard" && progress?.board && (
        <div className="vt-board">
          <div className="label">Storyboard · approve it or ask for changes</div>
          <div
            className="markdown vt-md"
            // Links open in the browser instead of replacing Jarvis's window.
            onClick={(e) => {
              const link = (e.target as HTMLElement).closest("a");
              if (!link) return;
              e.preventDefault();
              if (/^https?:/i.test(link.href)) openUrl(link.href).catch(() => {});
            }}
            dangerouslySetInnerHTML={{ __html: board }}
          />
          {asking ? (
            <form
              className="vt-feedback"
              onSubmit={(e) => {
                e.preventDefault();
                if (feedback.trim()) answer(false);
              }}
            >
              <input id="vt-feedback" autoFocus value={feedback} onChange={(e) => setFeedback(e.target.value)} placeholder="What should change? e.g. “make it 15 seconds” or “add a price scene”" disabled={busy} />
              <button className="btn primary" type="submit" disabled={busy || !feedback.trim()}>
                Update it
              </button>
              <button className="mini" type="button" onClick={() => setAsking(false)}>
                Cancel
              </button>
            </form>
          ) : (
            <div className="row">
              <button className="btn primary" onClick={() => answer(true)} disabled={busy}>
                Looks good, make it
              </button>
              <button className="mini" onClick={() => setAsking(true)} disabled={busy}>
                Change something
              </button>
            </div>
          )}
        </div>
      )}

      {latest ? (
        <div className={`vt-player ${portrait ? "portrait" : ""}`}>
          <video
            key={`${latest.path}-${latest.modified}`}
            controls
            playsInline
            preload="metadata"
            src={`${baseUrl}${latest.path.split("/").map(encodeURIComponent).join("/")}?v=${latest.modified}`}
            style={{ aspectRatio: `${info.meta.width} / ${info.meta.height}` }}
          />
          <div className="vt-meta">
            <span>
              <b>{latest.name === "final" ? "Final" : "Draft"}</b> · {info.meta.seconds}s · {info.meta.width}×{info.meta.height} · {kb(latest.size)}
            </span>
            <span className="grow" />
            {latest.name !== "final" && !working && (
              <button className="btn primary" onClick={renderFinal} disabled={busy}>
                Render the final
              </button>
            )}
            <button className="mini" onClick={() => invoke("reveal_path", { path: `${folder}/${latest.path}` }).catch(() => {})}>
              Show in Finder
            </button>
          </div>
        </div>
      ) : (
        !working &&
        stage !== "storyboard" && <p className="muted small vt-note">No video yet. A draft appears here when the video has been built and checked.</p>
      )}

      {!working && stage !== "storyboard" && info.renders.length > 0 && (
        <form
          className="vt-ask"
          onSubmit={(e) => {
            e.preventDefault();
            if (draft.trim()) send();
          }}
        >
          <div className="ask">
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M12 2l1.8 5.2L19 9l-5.2 1.8L12 16l-1.8-5.2L5 9l5.2-1.8zM19 14l.9 2.1L22 17l-2.1.9L19 20l-.9-2.1L16 17l2.1-.9z" />
            </svg>
            <input id="vt-ask" value={draft} onChange={(e) => setDraft(e.target.value)} placeholder="Tell Jarvis what to change, e.g. “make the intro faster”" disabled={busy} aria-label="Change the video" />
            <button className="btn primary" type="submit" disabled={busy || !draft.trim()}>
              Change it
            </button>
          </div>
        </form>
      )}

      {progress && progress.log.length > 0 && (
        <details className="more vt-log">
          <summary>What happened</summary>
          <ul>
            {progress.log.slice(-40).map((l, i) => (
              <li key={i}>{l}</li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}
