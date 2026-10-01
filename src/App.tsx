import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import DocumentEditor from "./components/DocumentEditor";
import ImageThumb from "./components/ImageThumb";
import LibraryPage from "./components/LibraryPage";
import Orb from "./components/Orb";
import ReportViewer from "./components/ReportViewer";
import SearchPage from "./components/SearchPage";
import BriefingsPage from "./components/BriefingsPage";
import KnowHowPage from "./components/KnowHowPage";
import RoutinesPage from "./components/RoutinesPage";
import SetupPanel from "./components/SetupPanel";
import SettingsPanel from "./components/SettingsPanel";
import TaskCard from "./components/TaskCard";
import { day } from "./lib/format";
import { isImportable } from "./lib/importDoc";
import { useJarvis } from "./lib/jarvis";
import type { CodexStatus, Task } from "./lib/types";

const SIDE_KEY = "jarvis.sidebar.collapsed";
/** Below this width the sidebar stops being a column and opens as a drawer instead. */
const NARROW = "(max-width: 1100px)";

function useMedia(query: string) {
  const [matches, setMatches] = useState(() => matchMedia(query).matches);
  useEffect(() => {
    const m = matchMedia(query);
    const on = () => setMatches(m.matches);
    m.addEventListener("change", on);
    on();
    return () => m.removeEventListener("change", on);
  }, [query]);
  return matches;
}

const MID_KEY = "jarvis.panel.conversationWidth";
const MID_DEFAULT = 380;
const MID_MIN = 280;
const PANEL_MIN = 420;

const MODE_LABEL = { off: "Offline", connecting: "Connecting", live: "Listening", error: "Offline" } as const;

export default function App() {
  const j = useJarvis();
  const [showSettings, setShowSettings] = useState(false);
  // What fills the main area beside the sidebar.
  const [page, setPage] = useState<"chat" | "library" | "search" | "briefings" | "routines" | "knowhow">("chat");
  const showLibrary = page === "library";
  const isMac = navigator.userAgent.includes("Mac");

  // ⌘K (Ctrl+K on Windows) jumps to search from anywhere.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((isMac ? e.metaKey : e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPage("search");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [isMac]);

  // Sidebar: a column you can collapse on wide windows, a drawer over the content on narrow ones.
  const narrow = useMedia(NARROW);
  const [sideCollapsed, setSideCollapsed] = useState(() => {
    try {
      return localStorage.getItem(SIDE_KEY) === "1";
    } catch {
      return false;
    }
  });
  const [drawer, setDrawer] = useState(false);
  useEffect(() => {
    if (!narrow) setDrawer(false);
  }, [narrow]);
  useEffect(() => {
    if (!drawer) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setDrawer(false);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [drawer]);
  const sidebarShown = narrow ? drawer : !sideCollapsed;
  const toggleSidebar = () => {
    if (narrow) return setDrawer((d) => !d);
    setSideCollapsed((c) => {
      try {
        localStorage.setItem(SIDE_KEY, c ? "0" : "1");
      } catch {
        /* not remembered this time */
      }
      return !c;
    });
  };
  const [typed, setTyped] = useState("");
  const [filter, setFilter] = useState<"active" | "all">("active");
  const transcriptRef = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState(false);
  const attachRef = useRef(j.attach);
  attachRef.current = j.attach;
  const importRef = useRef(j.importFiles);
  importRef.current = j.importFiles;

  // Drag images from Finder onto the window to attach them.
  useEffect(() => {
    const un = getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      if (p.type === "enter" || p.type === "over") setDragging(true);
      else if (p.type === "leave") setDragging(false);
      else if (p.type === "drop") {
        setDragging(false);
        // Documents open in the editor; everything else (images) is attached for Jarvis to see.
        const docs = p.paths.filter(isImportable);
        const rest = p.paths.filter((x) => !isImportable(x));
        if (docs.length) importRef.current(docs);
        if (rest.length) attachRef.current(rest);
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

  // First run: walk through setup (voice key, research helper, ChatGPT sign-in).
  const [showSetup, setShowSetup] = useState(false);
  const [setupDone, setSetupDone] = useState(true);
  const setupChecked = useRef(false);
  const checkSetup = async (open: boolean) => {
    const codex = await invoke<CodexStatus>("codex_status").catch(() => null);
    const done = !!j.settings?.geminiApiKey && !!codex?.found && !!codex?.loggedIn;
    setSetupDone(done);
    if (open && !done) setShowSetup(true);
  };
  useEffect(() => {
    if (!j.settings || setupChecked.current) return;
    setupChecked.current = true;
    checkSetup(true);
  }, [j.settings]);

  // A routine the voice just made or changed opens on the Routines page.
  useEffect(() => {
    if (j.focusRoutine) setPage("routines");
  }, [j.focusRoutine]);

  useEffect(() => {
    const el = transcriptRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [j.messages]);

  // The tasks pane shows work started in the conversation that's open.
  // Tasks saved before chats existed have no chatId, so they stay visible everywhere.
  const chatTasks = j.tasks.filter((t) => !t.chatId || t.chatId === j.chatId);
  const running = chatTasks.filter((t) => t.status === "running");
  const shown: Task[] = filter === "active" ? chatTasks.filter((t) => t.status === "running" || Date.now() - (t.finishedAt ?? 0) < 6 * 3600e3).slice(0, 12) : chatTasks;
  const library = j.tasks.filter((t) => t.status === "done" && (!t.parentId || t.kind === "image"));
  const openTask = j.tasks.find((t) => t.id === j.reportId);
  // Documents open in the editor; follow-up edits share the original's file, so open the original.
  const docTask = ((): Task | undefined => {
    let t = openTask?.kind === "document" ? openTask : undefined;
    while (t?.parentId) {
      const parent = j.tasks.find((x) => x.id === t!.parentId);
      if (!parent) break;
      t = parent;
    }
    return t;
  })();
  const reportTask = openTask && openTask.kind !== "document" && openTask.kind !== "skill" ? openTask : undefined;
  // Know-how opens on its own page, where it can be reviewed and saved.
  useEffect(() => {
    if (openTask?.kind !== "skill") return;
    j.setReportId(null);
    setPage("knowhow");
  }, [openTask?.id]);
  // Whatever is open shows in the side panel, in place of the Worker tasks pane.
  const panelTask = docTask ?? reportTask;
  const closePanel = () => j.setReportId(null);

  // Full width hides the conversation; it resets whenever the panel closes.
  const [expanded, setExpanded] = useState(false);
  useEffect(() => {
    if (!panelTask) setExpanded(false);
  }, [!!panelTask]);

  // Width of the narrowed conversation beside the panel, dragged with the splitter and remembered.
  const [midW, setMidW] = useState(() => {
    try {
      const saved = Number(localStorage.getItem(MID_KEY));
      return saved >= MID_MIN ? saved : MID_DEFAULT;
    } catch {
      return MID_DEFAULT;
    }
  });
  const midRef = useRef(midW);
  midRef.current = midW;
  const panesRef = useRef<HTMLDivElement>(null);

  const rememberWidth = (w: number) => {
    try {
      localStorage.setItem(MID_KEY, String(w));
    } catch {
      /* not remembered this time */
    }
  };
  /** Keep the conversation readable and the panel at least PANEL_MIN wide. */
  const clampWidth = (w: number) => {
    const panes = panesRef.current;
    const side = (panes?.querySelector(".sidebar") as HTMLElement | null)?.offsetWidth ?? 0;
    const max = (panes?.clientWidth ?? 1400) - side - PANEL_MIN;
    return Math.round(Math.max(MID_MIN, Math.min(max, w)));
  };
  const startResize = (e: React.PointerEvent<HTMLDivElement>) => {
    const panes = panesRef.current;
    if (!panes) return;
    e.preventDefault();
    const handle = e.currentTarget;
    handle.setPointerCapture(e.pointerId);
    const left = panes.getBoundingClientRect().left + ((panes.querySelector(".sidebar") as HTMLElement | null)?.offsetWidth ?? 0);
    document.body.classList.add("resizing");
    const move = (ev: PointerEvent) => setMidW(clampWidth(ev.clientX - left));
    const up = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", up);
      handle.removeEventListener("pointercancel", up);
      handle.removeEventListener("lostpointercapture", up);
      document.body.classList.remove("resizing");
      rememberWidth(midRef.current);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up);
    handle.addEventListener("pointercancel", up);
    // If the drag ends any other way, don't leave the page stuck in resize mode (it blocks scrolling).
    handle.addEventListener("lostpointercapture", up);
  };
  const nudgeWidth = (e: React.KeyboardEvent) => {
    const step = e.key === "ArrowLeft" ? -32 : e.key === "ArrowRight" ? 32 : 0;
    if (!step) return;
    e.preventDefault();
    const w = clampWidth(midRef.current + step);
    setMidW(w);
    rememberWidth(w);
  };
  const resetWidth = () => {
    setMidW(MID_DEFAULT);
    rememberWidth(MID_DEFAULT);
  };

  return (
    <div className="app">
      <div className="aura" aria-hidden="true">
        <i />
        <i />
        <i />
      </div>

      <header className="titlebar" data-tauri-drag-region>
        <button
          className="icon-btn side-toggle"
          onClick={toggleSidebar}
          aria-label={sidebarShown ? "Hide sidebar" : "Show sidebar"}
          aria-expanded={sidebarShown}
          title={sidebarShown ? "Hide sidebar" : "Show sidebar"}
        >
          <svg viewBox="0 0 24 24">
            <path d="M4 5h16a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1zm1 2v10h4V7zm6 0v10h8V7z" />
          </svg>
        </button>
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

      <div
        ref={panesRef}
        className={`panes${panelTask ? " with-panel" : ""}${panelTask && expanded ? " expanded" : ""}${!narrow && sideCollapsed ? " side-collapsed" : ""}${narrow && drawer ? " drawer-open" : ""}`}
        style={{ "--mid-w": `${midW}px` } as React.CSSProperties}
      >
        {narrow && drawer && <div className="drawer-backdrop" onClick={() => setDrawer(false)} aria-hidden="true" />}
        <aside
          className="sidebar"
          aria-hidden={!sidebarShown}
          onClickCapture={(e) => {
            const target = e.target as HTMLElement;
            if (narrow && target.closest("button") && !target.closest("i[role=button]")) setDrawer(false);
          }}
        >
          <button className={`search-trigger ${page === "search" ? "on" : ""}`} onClick={() => setPage("search")}>
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M10.5 4a6.5 6.5 0 0 1 5.2 10.4l4.4 4.4-1.4 1.4-4.4-4.4A6.5 6.5 0 1 1 10.5 4zm0 2a4.5 4.5 0 1 0 0 9 4.5 4.5 0 0 0 0-9z" />
            </svg>
            Search
            <kbd>{isMac ? "⌘K" : "Ctrl K"}</kbd>
          </button>
          <div className="chats">
            <div className="label">
              Chats
              <button
                className="mini"
                onClick={() => {
                  setPage("chat");
                  closePanel();
                  j.newChat();
                }}
                title="Start a new conversation"
              >
                New
              </button>
            </div>
            <div className="lib">
              {j.chats.map((c) => (
                <button
                  key={c.id}
                  className={j.chatId === c.id && page === "chat" && !docTask ? "on" : ""}
                  onClick={() => {
                    setPage("chat");
                    closePanel();
                    j.openChat(c.id);
                  }}
                  title={c.title || "New chat"}
                >
                  <b>{c.title || "New chat"}</b>
                  <span>{day(c.updatedAt)}</span>
                  <i
                    role="button"
                    aria-label={`Delete ${c.title || "New chat"}`}
                    title="Delete"
                    onClick={(e) => {
                      e.stopPropagation();
                      j.deleteChat(c.id);
                    }}
                  >
                    ×
                  </i>
                </button>
              ))}
            </div>
          </div>
          <nav className="nav">
            <button
              className="navitem"
              onClick={() => {
                setPage("chat");
                j.newDocument().catch((e) => j.setError(String(e)));
              }}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="M5 3h9l5 5v5.2l-2 2V9h-4V5H7v14h6.5l-2 2H5zm14.7 10.3 1.4 1.4-6.4 6.4-2.1.6.6-2.1z" />
              </svg>
              New document
            </button>
            <button
              className="navitem"
              onClick={() => {
                setPage("chat");
                j.openFiles().catch((e) => j.setError(String(e)));
              }}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="M4 5a1 1 0 0 1 1-1h5l2 2h7a1 1 0 0 1 1 1v3H8.6a1 1 0 0 0-.94.66L4 20.4zm4.9 7H21l-3.2 8H5.7z" />
              </svg>
              Open file…
            </button>
            <button
              className={`navitem ${showLibrary && !docTask ? "on" : ""}`}
              onClick={() => {
                closePanel();
                setPage("library");
              }}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="M4 4h5v16H4zm6.5 0h4v16h-4zM17 4.7l3.6 15.6-2 .5L15 5.2z" />
              </svg>
              Library
              {library.length > 0 && <span>{library.length}</span>}
            </button>
            <button
              className={`navitem ${page === "briefings" && !docTask ? "on" : ""}`}
              onClick={() => {
                closePanel();
                setPage("briefings");
              }}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="M12 3a9 9 0 1 1 0 18 9 9 0 0 1 0-18zm0 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14zm1 2v4.6l3.2 1.9-1 1.7L11 12.7V7z" />
              </svg>
              Briefings
            </button>
            <button
              className={`navitem ${page === "routines" && !docTask ? "on" : ""}`}
              onClick={() => {
                closePanel();
                setPage("routines");
              }}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="M12 3a9 9 0 1 0 9 9h-2a7 7 0 1 1-2.1-5L14 10h7V3l-2.6 2.6A9 9 0 0 0 12 3z" />
              </svg>
              Routines
            </button>
            <button
              className={`navitem ${page === "knowhow" && !docTask ? "on" : ""}`}
              onClick={() => {
                closePanel();
                setPage("knowhow");
              }}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="m12 2 2.9 6.2 6.6.8-4.9 4.6 1.3 6.6L12 16.9l-5.9 3.3 1.3-6.6-4.9-4.6 6.6-.8z" />
              </svg>
              Know-how
            </button>
            {!setupDone && (
              <button className="navitem setup-nav" onClick={() => setShowSetup(true)}>
                <svg viewBox="0 0 24 24" aria-hidden="true">
                  <path d="M12 2a10 10 0 1 1 0 20 10 10 0 0 1 0-20zm-1 5v7h2V7zm0 9v2h2v-2z" />
                </svg>
                Finish setting up
              </button>
            )}
          </nav>
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

        {showLibrary ? (
          <LibraryPage
            items={library}
            chatId={j.chatId}
            chats={j.chats}
            openId={j.reportId}
            onOpen={j.setReportId}
            onNew={() => j.newDocument().catch((e) => j.setError(String(e)))}
            onOpenFile={() => j.openFiles().catch((e) => j.setError(String(e)))}
            onClose={() => setPage("chat")}
          />
        ) : page === "routines" ? (
          <RoutinesPage
            chatId={j.chatId}
            apiKey={j.settings?.geminiApiKey ?? ""}
            tasks={j.tasks}
            focusId={j.focusRoutine}
            onFocused={() => j.setFocusRoutine(null)}
            onOpenTask={j.setReportId}
            onClose={() => setPage("chat")}
          />
        ) : page === "knowhow" ? (
          <KnowHowPage tasks={j.tasks} onClose={() => setPage("chat")} />
        ) : page === "briefings" ? (
          <BriefingsPage chatId={j.chatId} onOpenTask={j.setReportId} onClose={() => setPage("chat")} />
        ) : page === "search" ? (
          <SearchPage
            openId={j.reportId}
            onOpenChat={(id) => {
              closePanel();
              setPage("chat");
              j.openChat(id);
            }}
            onOpenTask={j.setReportId}
            onClose={() => setPage("chat")}
          />
        ) : (
          <section className="center">
            <div className="stage">
              <Orb source={j.orb} size={panelTask ? 150 : 280} />
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
        )}

        {panelTask ? (
          <aside className="panel" aria-label={panelTask.title}>
            {!expanded && (
              <div
                className="splitter"
                role="separator"
                aria-orientation="vertical"
                aria-label="Resize the panel. Arrow keys move it; double-click resets."
                title="Drag to resize · double-click to reset"
                tabIndex={0}
                onPointerDown={startResize}
                onDoubleClick={resetWidth}
                onKeyDown={nudgeWidth}
              />
            )}
            {docTask ? (
              <DocumentEditor
                key={docTask.id}
                doc={docTask}
                tasks={j.tasks}
                micOn={j.micOn}
                onToggleMic={j.toggleMic}
                onWrite={(q) => j.writeDocument(docTask.id, q)}
                expanded={expanded}
                onToggleExpand={() => setExpanded((x) => !x)}
                onClose={closePanel}
              />
            ) : (
              <ReportViewer
                key={panelTask.id}
                task={panelTask}
                browserBusy={j.tasks.some((t) => t.kind === "browser" && t.status === "running")}
                expanded={expanded}
                onToggleExpand={() => setExpanded((x) => !x)}
                onClose={closePanel}
              />
            )}
          </aside>
        ) : (
          page === "chat" && (
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
              {shown.length === 0 && <p className="muted small">No tasks in this chat yet. When you ask for research, Codex picks it up here and you can watch each step.</p>}
              {shown.map((t) => (
                <TaskCard key={t.id} task={t} onOpen={j.setReportId} />
              ))}
            </aside>
          )
        )}
      </div>

      {dragging && (
        <div className="dropzone" aria-hidden="true">
          <div>
            <b>Drop to attach or open</b>
            <span>Images are attached for Jarvis. Word, Markdown and text files open in the editor.</span>
          </div>
        </div>
      )}

      {showSetup && j.settings && (
        <SetupPanel
          settings={j.settings}
          onSaved={() => j.reloadSettings()}
          onClose={() => {
            setShowSetup(false);
            checkSetup(false);
          }}
          onAdvanced={() => {
            setShowSetup(false);
            setShowSettings(true);
          }}
        />
      )}

      {showSettings && j.settings && (
        <SettingsPanel initial={j.settings} onClose={() => setShowSettings(false)} onSaved={() => j.reloadSettings()} />
      )}
    </div>
  );
}
