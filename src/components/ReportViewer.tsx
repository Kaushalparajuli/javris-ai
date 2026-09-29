import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import DOMPurify from "dompurify";
import { marked } from "marked";
import { useEffect, useState } from "react";
import type { Task } from "../lib/types";
import ImageThumb from "./ImageThumb";

export default function ReportViewer({ task, onClose }: { task: Task | undefined; onClose: () => void }) {
  const [html, setHtml] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    if (!task || task.kind === "image") return;
    setError("");
    invoke<string>("read_report", { id: task.id })
      .then(async (md) => setHtml(DOMPurify.sanitize(await marked.parse(md))))
      .catch((e) => setError(String(e)));
  }, [task?.id, task?.status]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  if (!task) return null;

  // Links open in the default browser, not inside Jarvis.
  const onClick = (e: React.MouseEvent) => {
    const a = (e.target as HTMLElement).closest("a");
    if (a?.href?.startsWith("http")) {
      e.preventDefault();
      openUrl(a.href);
    }
  };

  return (
    <div className="overlay" onClick={onClose}>
      <div className="report glass" onClick={(e) => e.stopPropagation()} role="dialog" aria-label={`Report: ${task.title}`}>
        <header>
          <div>
            <div className="label">
              {task.kind === "image" ? "Images" : "Report"} · task #{task.id}
            </div>
            <h2>{task.title}</h2>
          </div>
          <div className="actions">
            <button className="mini" onClick={() => invoke("reveal_task", { id: task.id })}>
              Show in Finder
            </button>
            <button className="mini" onClick={onClose}>
              Close
            </button>
          </div>
        </header>
        {task.kind === "image" ? (
          <div className="gallery">
            <p className="prompt">{task.request}</p>
            {task.refs?.length > 0 && (
              <div className="refs">
                <span className="label">References</span>
                {task.refs.map((r) => (
                  <ImageThumb key={r} path={r} className="ref" alt="reference" />
                ))}
              </div>
            )}
            {task.images.length === 0 && <p className="muted">No images yet.</p>}
            {task.images.map((p) => (
              <figure key={p}>
                <ImageThumb path={p} className="full" alt={task.title} />
                <figcaption>
                  <span>{p.split("/").pop()}</span>
                  <button className="mini" onClick={() => invoke("reveal_path", { path: p })}>
                    Show in Finder
                  </button>
                </figcaption>
              </figure>
            ))}
            {task.summary && <p className="summary">{task.summary}</p>}
          </div>
        ) : error ? (
          <p className="error-text">{error}</p>
        ) : (
          <div className="markdown" onClick={onClick} dangerouslySetInnerHTML={{ __html: html }} />
        )}
      </div>
    </div>
  );
}
