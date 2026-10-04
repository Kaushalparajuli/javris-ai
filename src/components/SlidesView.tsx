import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useState } from "react";
import type { Task } from "../lib/types";

interface SlidesInfo {
  deck: string;
  ready: boolean;
  link: string;
}

/** A slide deck in the side panel: open it on the Mac, or put it in Google Slides. */
export default function SlidesView({ task }: { task: Task }) {
  const [info, setInfo] = useState<SlidesInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const load = useCallback(() => {
    invoke<SlidesInfo>("slides_info", { id: task.id })
      .then(setInfo)
      .catch((e) => setError(String(e)));
  }, [task.id]);

  useEffect(() => {
    load();
    const un = listen<Task>("task-update", (e) => e.payload.id === task.id && load());
    return () => {
      un.then((f) => f());
    };
  }, [load, task.id]);

  const upload = async () => {
    setBusy(true);
    setError("");
    try {
      const r = await invoke<{ id: string; link: string }>("slides_upload", { taskId: task.id });
      setInfo((i) => (i ? { ...i, link: r.link } : i));
    } catch (e) {
      setError(String(e));
    }
    setBusy(false);
  };

  const running = task.status === "running";
  return (
    <div className="slides-view">
      <p className="prompt">{task.request}</p>
      {running && <p className="muted small">Codex is building the deck. It appears here when deck.pptx is saved.</p>}
      {task.summary && !running && <p className="summary">{task.summary}</p>}
      {info?.ready && (
        <div className="actions">
          <button className="mini" onClick={() => invoke("slides_open", { id: task.id }).catch((e) => setError(String(e)))}>
            Open in Keynote
          </button>
          <button className="mini" onClick={() => invoke("reveal_task", { id: task.id }).catch((e) => setError(String(e)))}>
            Show in Finder
          </button>
          {info.link ? (
            <button className="mini" onClick={() => openUrl(info.link).catch(() => {})}>
              Open in Google Slides
            </button>
          ) : (
            <button className="mini" disabled={busy} onClick={upload}>
              {busy ? "Uploading…" : "Upload to Google Slides"}
            </button>
          )}
        </div>
      )}
      {!info?.ready && !running && <p className="muted small">There's no deck.pptx in this task's folder.</p>}
      {error && <p className="error-text small">{error}</p>}
    </div>
  );
}
