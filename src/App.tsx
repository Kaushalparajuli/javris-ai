import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import ImageThumb from "./components/ImageThumb";
import Orb from "./components/Orb";
import ReportViewer from "./components/ReportViewer";
import SettingsPanel from "./components/SettingsPanel";
import TaskCard from "./components/TaskCard";
import { useJarvis } from "./lib/jarvis";
import type { Task } from "./lib/types";

const MODE_LABEL = { off: "Offline", connecting: "Connecting", live: "Listening", error: "Offline" } as const;

function day(ms: number) {
  const d = new Date(ms);
  const today = new Date();
  const diff = Math.round((today.setHours(0, 0, 0, 0) - new Date(ms).setHours(0, 0, 0, 0)) / 86400000);
  if (diff === 0) return "today";
  if (diff === 1) return "yesterday";
  return d.toLocaleDateString([], { day: "numeric", month: "short" });
}

export default function App() {
  const j = useJarvis();
  const [showSettings, setShowSettings] = useState(false);
  const [typed, setTyped] = useState("");
  const [filter, setFilter] = useState<"active" | "all">("active");
  const transcriptRef = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState(false);
  const attachRef = useRef(j.attach);
  attachRef.current = j.attach;

  // Drag images from Finder onto the window to attach them.
  useEffect(() => {
    const un = getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      if (p.type === "enter" || p.type === "over") setDragging(true);
      else if (p.type === "leave") setDragging(false);
      else if (p.type === "drop") {
        setDragging(false);
        attachRef.current(p.paths);
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  const pickFiles = async () => {
    const picked = await openDialog({
      multiple: true,
      title: "Attach logo or reference images",
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }],
    });
    if (picked) j.attach(Array.isArray(picked) ? picked : [picked]);
  };

  // First run: ask for the API key.
  useEffect(() => {
    if (j.settings && !j.settings.geminiApiKey) setShowSettings(true);
  }, [j.settings?.geminiApiKey]);

  useEffect(() => {
    const el = transcriptRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [j.messages]);

  const running = j.tasks.filter((t) => t.status === "running");
  const shown: Task[] = filter === "active" ? j.tasks.filter((t) => t.status === "running" || Date.now() - (t.finishedAt ?? 0) < 6 * 3600e3).slice(0, 12) : j.tasks;
  const library = j.tasks.filter((t) => t.status === "done" && (!t.parentId || t.kind === "image"));
  const reportTask = j.tasks.find((t) => t.id === j.reportId);

  return (
    <div className="app">
      <div className="aura" aria-hidden="true">
        <i />
        <i />
        <i />
      </div>

      <header className="titlebar" data-tauri-drag-region>
        <div className="brand" data-tauri-drag-region>
          Jarvis
        </div>
        <div className="conn" data-tauri-drag-region>
          <span>
            <i className={`dot ${j.connection}`} />
            Gemini {j.connection === "live" ? "Live" : MODE_LABEL[j.connection].toLowerCase()}
          </span>
          <span>
            <i className={`dot ${running.length ? "busy" : "idle"}`} />
            Codex · {running.length ? `${running.length} running` : "idle"}
          </span>
          <button className="icon-btn" onClick={() => setShowSettings(true)} aria-label="Settings" title="Settings">
            <svg viewBox="0 0 24 24">
              <path d="M19.4 13a7.6 7.6 0 0 0 0-2l2.1-1.6-2-3.5-2.5 1a7.4 7.4 0 0 0-1.7-1L15 3.3h-4l-.4 2.6a7.4 7.4 0 0 0-1.7 1l-2.5-1-2 3.5L6.6 11a7.6 7.6 0 0 0 0 2l-2.1 1.6 2 3.5 2.5-1a7.4 7.4 0 0 0 1.7 1l.3 2.6h4l.4-2.6a7.4 7.4 0 0 0 1.7-1l2.5 1 2-3.5zM13 15.5a3.5 3.5 0 1 1 0-7 3.5 3.5 0 0 1 0 7z" transform="translate(-1 0)" />
            </svg>
          </button>
        </div>
      </header>

      <div className="panes">
        <aside className="sidebar">
          <div>
            <div className="label">Library</div>
            <div className="lib">
              {library.length === 0 && <p className="muted small pad">Finished research appears here. Try saying “Jarvis, research…”.</p>}
              {library.slice(0, 30).map((t) => (
                <button key={t.id} className={j.reportId === t.id ? "on" : ""} onClick={() => j.setReportId(t.id)}>
                  {t.title}
                  <span>
                    {day(t.startedAt)} · {t.kind === "image" ? `image${t.images.length > 1 ? `s · ${t.images.length}` : ""}` : t.depth === "quick" ? "quick" : "deep dive"}
                  </span>
                </button>
              ))}
            </div>
          </div>
          <div className="shortcuts">
            <div className="label">Shortcuts</div>
            <div className="keys">
              <span>
                <kbd>⌥</kbd>
                <kbd>Space</kbd>
              </span>
              <span>Talk / mute</span>
              <span>
                <kbd>⌥</kbd>
                <kbd>J</kbd>
              </span>
              <span>Show / hide</span>
              <span>
                <kbd>⌥</kbd>
                <kbd>.</kbd>
              </span>
              <span>Stop speaking</span>
            </div>
          </div>
        </aside>

        <section className="center">
          <div className="stage">
            <Orb source={j.orb} />
            <div className="state">
              <b className={j.connection}>{j.connection === "live" ? (j.micOn ? "Listening" : "Muted") : MODE_LABEL[j.connection]}</b>
              <span>{j.status}</span>
            </div>
          </div>

          {j.error && (
            <div className="banner" role="alert">
              <span>{j.error}</span>
              <div className="actions">
                {j.micBlocked && (
                  <button className="mini" onClick={j.openMicSettings}>
                    Open Microphone settings
                  </button>
                )}
                <button className="mini" onClick={() => j.setError("")}>
                  Dismiss
                </button>
              </div>
            </div>
          )}

          <div className="transcript" ref={transcriptRef} aria-live="polite">
            {j.messages.length === 0 && (
              <div className="empty">
                <p>Ask for anything that needs research, for example:</p>
                <ul>
                  <li>“Research the best vector databases for a small startup. Deep dive.”</li>
                  <li>“Quick check: what's the latest stable version of Tauri?”</li>
                  <li>“Make me an image of a minimalist mountain logo in navy and gold.”</li>
                  <li>“Remember that I prefer sources from the last 12 months.”</li>
                </ul>
              </div>
            )}
            {j.messages.map((m) => (
              <div key={m.id} className={`msg ${m.who}`}>
                <div className="who">{m.who === "notice" ? "worker" : m.who}</div>
                {m.who === "tool" ? <div className="chip">{m.text}</div> : <p>{m.text}</p>}
              </div>
            ))}
          </div>

          {j.attachments.length > 0 && (
            <div className="attachments">
              {j.attachments.map((a) => (
                <div key={a.path} className="attachment" title={a.name}>
                  <ImageThumb path={a.path} className="att-img" alt={a.name} />
                  <span>{a.name}</span>
                  <button type="button" aria-label={`Remove ${a.name}`} onClick={() => j.removeAttachment(a.path)}>
                    ×
                  </button>
                </div>
              ))}
              <span className="muted small">Jarvis can see these and will pass them to the image worker.</span>
            </div>
          )}

          <form
            className="dock"
            onSubmit={(e) => {
              e.preventDefault();
              j.sendTyped(typed);
              setTyped("");
            }}
          >
            <button type="button" className={`mic ${j.micOn ? "" : "off"}`} onClick={j.toggleMic} aria-label={j.micOn ? "Mute microphone" : "Start talking"}>
              <svg viewBox="0 0 24 24">
                <path d="M12 14a3 3 0 0 0 3-3V5a3 3 0 1 0-6 0v6a3 3 0 0 0 3 3zm5-3a5 5 0 0 1-10 0H5a7 7 0 0 0 6 6.92V21h2v-3.08A7 7 0 0 0 19 11h-2z" />
              </svg>
            </button>
            <button type="button" className="icon-btn clip" onClick={pickFiles} aria-label="Attach images" title="Attach logo or reference images">
              <svg viewBox="0 0 24 24">
                <path d="M16.5 6.5v9.8a4.5 4.5 0 0 1-9 0V5.8a3 3 0 0 1 6 0v9.7a1.5 1.5 0 0 1-3 0V6.5H9v9a3 3 0 0 0 6 0V5.8a4.5 4.5 0 0 0-9 0v10.5a6 6 0 0 0 12 0V6.5z" />
              </svg>
            </button>
            <input id="typed" className="typebox" value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="Type instead of speaking…" />
            {j.connection !== "off" && (
              <button type="button" className="mini" onClick={j.disconnect}>
                End session
              </button>
            )}
          </form>
        </section>

        <aside className="tasks">
          <header>
            <div className="label">Worker tasks</div>
            <div className="seg" role="group" aria-label="Filter tasks">
              <button className={filter === "active" ? "on" : ""} onClick={() => setFilter("active")}>
                Recent
              </button>
              <button className={filter === "all" ? "on" : ""} onClick={() => setFilter("all")}>
                All
              </button>
            </div>
          </header>
          {shown.length === 0 && <p className="muted small">No tasks yet. When you ask for research, Codex picks it up here and you can watch each step.</p>}
          {shown.map((t) => (
            <TaskCard key={t.id} task={t} onOpen={j.setReportId} />
          ))}
        </aside>
      </div>

      {dragging && (
        <div className="dropzone" aria-hidden="true">
          <div>
            <b>Drop images to attach</b>
            <span>Logos, product photos or style references for Jarvis</span>
          </div>
        </div>
      )}

      {reportTask && <ReportViewer task={reportTask} onClose={() => j.setReportId(null)} />}
      {showSettings && j.settings && (
        <SettingsPanel initial={j.settings} onClose={() => setShowSettings(false)} onSaved={() => j.reloadSettings()} />
      )}
    </div>
  );
}
