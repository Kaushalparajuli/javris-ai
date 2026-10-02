import { invoke } from "@tauri-apps/api/core";
import { join } from "@tauri-apps/api/path";
import { openUrl } from "@tauri-apps/plugin-opener";
import DOMPurify from "dompurify";
import { marked } from "marked";
import { useEffect, useRef, useState } from "react";
import { safeFileName } from "../lib/exportDocx";
import { printableHtml } from "../lib/exportPdf";
import { askSavePath, fileNameOf } from "../lib/saveAs";
import type { DirectorState } from "../lib/director";
import type { Task } from "../lib/types";
import BrowserView from "./BrowserView";
import SitePreview from "./SitePreview";
import ImageThumb from "./ImageThumb";
import { hydrateImages } from "../lib/docImages";
import PanelControls from "./PanelControls";

/**
 * A research report or an image task in the side panel. While the worker runs it shows the
 * live steps and keeps re-reading the report, so anything saved early appears straight away.
 */
export default function ReportViewer({
  task,
  browserBusy,
  expanded,
  onToggleExpand,
  onClose,
  director,
  onReview,
}: {
  task: Task;
  /** The visual director's work on this site, if it has looked at it. */
  director?: DirectorState;
  onReview: () => void;
  /** A browser task is driving Jarvis's browser right now. */
  browserBusy: boolean;
  expanded: boolean;
  onToggleExpand: () => void;
  onClose: () => void;
}) {
  const [html, setHtml] = useState("");
  const [markdown, setMarkdown] = useState("");
  const [error, setError] = useState("");
  const [pdfNote, setPdfNote] = useState("");
  const running = task.status === "running";
  const bodyRef = useRef<HTMLDivElement>(null);
  // Pictures linked from the report (saved next to it) load from the task's folder.
  useEffect(() => {
    if (bodyRef.current) hydrateImages(bodyRef.current, task.dir);
  }, [html, task.dir]);

  useEffect(() => {
    if (task.kind === "image") return;
    let live = true;
    const load = () =>
      invoke<string>("read_report", { id: task.id })
        .then(async (md) => {
          if (!live) return;
          setMarkdown(md);
          setHtml(DOMPurify.sanitize(await marked.parse(md)));
          setError("");
        })
        .catch((e) => live && setError(String(e)));
    load();
    const poll = running ? window.setInterval(load, 1500) : undefined;
    return () => {
      live = false;
      window.clearInterval(poll);
    };
  }, [task.id, task.kind, running]);

  // Web links open in the default browser. Links to the task's own files (like a screenshot)
  // show that file instead of trying to load it inside Jarvis.
  const onClick = async (e: React.MouseEvent) => {
    const a = (e.target as HTMLElement).closest("a");
    if (!a) return;
    const raw = a.getAttribute("href") ?? "";
    if (/^https?:/i.test(raw)) {
      e.preventDefault();
      openUrl(raw);
    } else if (raw && !raw.startsWith("#") && !/^[a-z]+:/i.test(raw)) {
      e.preventDefault();
      invoke("reveal_path", { path: await join(task.dir, decodeURIComponent(raw)) }).catch(() => {});
    }
  };

  // A website build shows its preview first; what the worker is doing goes underneath it.
  const hasPreview = task.kind === "code" && !!task.project && task.project.includes("/projects/");
  const steps = task.steps.slice(hasPreview ? -80 : -6);
  const logRef = useRef<HTMLDivElement>(null);
  const stuck = useRef(true);
  // Keep the newest step in view, unless the user scrolled up to read earlier ones.
  useEffect(() => {
    const el = logRef.current;
    if (el && stuck.current) el.scrollTop = el.scrollHeight;
  }, [steps.length, task.steps[task.steps.length - 1]?.done]);

  const exportPdf = async () => {
    try {
      const path = await askSavePath(safeFileName(task.title, "pdf"), { name: "PDF", ext: "pdf" });
      if (!path) return;
      setPdfNote("Making the PDF…");
      await invoke<string>("export_pdf", { path, html: printableHtml(task.title, markdown) });
      setPdfNote(`Saved ${fileNameOf(path)}`);
    } catch (e) {
      setPdfNote("");
      setError(`Couldn't make the PDF: ${e}`);
    }
  };

  return (
    <section className="report-panel">
      <header>
        <div className="report-head">
          <div className="label">
            {task.kind === "image" ? "Images" : task.kind === "browser" ? "Browser task" : "Report"} · task #{task.id}
            {running ? " · running" : task.status !== "done" ? ` · ${task.status}` : ""}
          </div>
          <h2>{task.title}</h2>
        </div>
        {pdfNote && <span className="muted small">{pdfNote}</span>}
        {task.kind !== "image" && markdown && !running && (
          <button className="mini" onClick={exportPdf}>
            PDF
          </button>
        )}
        <button className="mini" onClick={() => invoke("reveal_task", { id: task.id }).catch(() => {})}>
          Show file
        </button>
        <PanelControls expanded={expanded} onToggleExpand={onToggleExpand} onClose={onClose} />
      </header>

      {running && !hasPreview && (
        <ul className="steps live-steps">
          {steps.length === 0 && (
            <li className="now">
              <i />
              <span>Starting the worker…</span>
            </li>
          )}
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

      {task.kind === "browser" && <BrowserView canUse={!browserBusy} />}
      {hasPreview && <SitePreview folder={task.project!} running={running} director={director} onReview={onReview} />}
      {hasPreview && (running || steps.length > 0) && (
        <details className="activity" open={running}>
          <summary>
            Worker activity
            <span className="muted small">
              {running ? " · live" : ""} · {task.steps.length} step{task.steps.length === 1 ? "" : "s"}
            </span>
          </summary>
          <div
            className="activity-log"
            ref={logRef}
            onScroll={(e) => {
              const el = e.currentTarget;
              stuck.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
            }}
          >
            <ul className="steps">
              {steps.length === 0 && (
                <li className="now">
                  <i />
                  <span>Starting the worker…</span>
                </li>
              )}
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
          </div>
        </details>
      )}

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
          {task.images.length === 0 && <p className="muted">{running ? "Images appear here when they're ready." : "No images yet."}</p>}
          {task.images.map((p) => (
            <figure key={p}>
              <ImageThumb path={p} className="full" alt={task.title} />
              <figcaption>
                <span>{p.split(/[\\/]/).pop()}</span>
                <button className="mini" onClick={() => invoke("reveal_path", { path: p })}>
                  Show file
                </button>
              </figcaption>
            </figure>
          ))}
          {task.summary && <p className="summary">{task.summary}</p>}
        </div>
      ) : html ? (
        <div className="markdown" onClick={onClick}>
          <div ref={bodyRef} dangerouslySetInnerHTML={{ __html: html }} />
          {task.kind === "browser" && task.images.length > 0 && (
            <div className="shots">
              <div className="label">Screenshots</div>
              {task.images.map((p) => (
                <figure key={p}>
                  <ImageThumb path={p} className="full" alt={`Screenshot from ${task.title}`} />
                  <figcaption>
                    <span>{p.split(/[\\/]/).pop()}</span>
                    <button className="mini" onClick={() => invoke("reveal_path", { path: p })}>
                      Show file
                    </button>
                  </figcaption>
                </figure>
              ))}
            </div>
          )}
        </div>
      ) : running ? (
        <p className="muted pad-lg">The report appears here as soon as the worker saves it.</p>
      ) : (
        <p className="error-text pad-lg">{task.error || error || "This task has no report."}</p>
      )}
    </section>
  );
}
