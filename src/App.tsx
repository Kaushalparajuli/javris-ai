import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import DocumentEditor from "./components/DocumentEditor";
import ChatRow from "./components/ChatRow";
import ModePicker from "./components/ModePicker";
import { getMode, nextMode, setMode } from "./lib/mode";
import ImageThumb from "./components/ImageThumb";
import LibraryPage from "./components/LibraryPage";
import Orb from "./components/Orb";
import ReportViewer from "./components/ReportViewer";
import SearchPage from "./components/SearchPage";
import BriefingsPage from "./components/BriefingsPage";
import KnowHowPage from "./components/KnowHowPage";
import MemoryPage from "./components/MemoryPage";
import NewProjectPage from "./components/NewProjectPage";
import AppsPage from "./components/AppsPage";
import DayPage from "./components/DayPage";
import ProjectsPage from "./components/ProjectsPage";
import ProjectPage from "./components/ProjectPage";
import RoutinesPage from "./components/RoutinesPage";
import SetupPanel from "./components/SetupPanel";
import ApprovalCard from "./components/ApprovalCard";
import CalendarPage from "./components/CalendarPage";
import MailPage from "./components/MailPage";
import DrivePage from "./components/DrivePage";
import YouTubePage, { type YtIntent, type YtItem } from "./components/YouTubePage";
import YtDock from "./components/YtDock";
import CommandPalette from "./components/CommandPalette";
import MeetingBanner from "./components/MeetingBanner";
import { listen } from "@tauri-apps/api/event";
import SettingsPanel from "./components/SettingsPanel";
import TaskCard from "./components/TaskCard";
import { isImportable } from "./lib/importDoc";
import { useJarvis } from "./lib/jarvis";
import { useMiniBridge } from "./lib/mini";
import type { ChatSummary, CodexStatus, Task, Workspace } from "./lib/types";

const SIDE_KEY = "jarvis.sidebar.collapsed";
/** Projects folded open in the sidebar. */
const OPEN_KEY = "jarvis.projects.open";
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
  useMiniBridge(j);
  const [showSettings, setShowSettings] = useState(false);
  const [showPalette, setShowPalette] = useState(false);
  useEffect(() => {
    const un = listen<string>("shortcut", (e) => {
      if (e.payload === "palette") setShowPalette(true);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);
  // What fills the main area beside the sidebar.
  const [page, setPage] = useState<"chat" | "library" | "search" | "briefings" | "routines" | "knowhow" | "mail" | "calendar" | "drive" | "youtube" | "memory" | "projects" | "project" | "newproject" | "day" | "apps">("chat");
  // Jarvis asking the YouTube page to show results or play a video.
  const [ytIntent, setYtIntent] = useState<YtIntent | null>(null);
  // The video playing now stays at the app level so it keeps going on other pages (mini bar at the bottom).
  const [ytPlaying, setYtPlaying] = useState<YtItem | null>(null);
  const [ytSlot, setYtSlot] = useState<HTMLElement | null>(null);
  const [ytAudioOnly, setYtAudioOnly] = useState(() => {
    try {
      return localStorage.getItem("jarvis.yt.audioOnly") === "1";
    } catch {
      return false;
    }
  });
  const audioOnly = (on: boolean) => {
    setYtAudioOnly(on);
    try {
      localStorage.setItem("jarvis.yt.audioOnly", on ? "1" : "0");
    } catch {
      /* not remembered this time */
    }
  };
  useEffect(() => {
    const on = (e: Event) => {
      setYtIntent({ ...(e as CustomEvent<Omit<YtIntent, "seq">>).detail, seq: Date.now() });
      setPage("youtube");
    };
    window.addEventListener("jarvis-youtube", on);
    return () => window.removeEventListener("jarvis-youtube", on);
  }, []);
  const showLibrary = page === "library";
  const ws = j.workspace;
  // Projects, like Claude Code: each holds its own chats, and a chat works in its project.
  const [projects, setProjects] = useState<Workspace[] | null>(null);
  useEffect(() => {
    const load = () => invoke<Workspace[]>("list_workspaces").then(setProjects).catch(() => {});
    load();
    window.addEventListener("jarvis-workspace", load);
    return () => window.removeEventListener("jarvis-workspace", load);
  }, []);
  // The open chat's project was deleted: reopen the chat, now outside any project.
  useEffect(() => {
    if (projects && ws && !projects.some((p) => p.slug === ws.slug)) j.openChat(j.chatId);
  }, [projects]);
  const [openProjects, setOpenProjects] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem(OPEN_KEY) ?? "[]");
    } catch {
      return [];
    }
  });
  const setOpen = (slug: string, open: boolean) =>
    setOpenProjects((list) => {
      const next = open ? [...new Set([...list, slug])] : list.filter((s) => s !== slug);
      try {
        localStorage.setItem(OPEN_KEY, JSON.stringify(next));
      } catch {
        /* not remembered this time */
      }
      return next;
    });
  // The project of the chat you're in is always unfolded.
  useEffect(() => {
    if (ws && !openProjects.includes(ws.slug)) setOpen(ws.slug, true);
  }, [ws?.slug]);
  const [editProject, setEditProject] = useState<string | null>(null);

  // A project was saved (or deleted) on the create/edit page. A project that still exists goes back
  // to its own page if it was being edited; anything else goes to the project list.
  const [viewProject, setViewProject] = useState<string | null>(null);
  const projectSaved = async () => {
    const list = await invoke<Workspace[]>("list_workspaces").catch(() => []);
    setProjects(list);
    window.dispatchEvent(new Event("jarvis-workspace"));
    const back = editProject && list.some((p) => p.slug === editProject) ? editProject : null;
    setEditProject(null);
    setViewProject(back);
    setPage(back ? "project" : "projects");
  };
  const showProject = (slug: string) => {
    closePanel();
    setViewProject(slug);
    setPage("project");
  };
  /** A chat dragged from the sidebar onto a project (or onto "Chats" to leave its project). */
  const dropChat = (e: React.DragEvent, slug: string) => {
    const id = e.dataTransfer.getData("application/x-jarvis-chat");
    if (!id) return;
    e.preventDefault();
    if (slug) setOpen(slug, true);
    j.moveChat(id, slug).catch((err) => j.setError(String(err)));
  };
  const allowDrop = (e: React.DragEvent) => e.dataTransfer.types.includes("application/x-jarvis-chat") && e.preventDefault();
  const newChatIn = (slug: string) => {
    setPage("chat");
    closePanel();
    if (slug) setOpen(slug, true);
    j.newChat(slug);
  };
  const looseChats = j.chats.filter((c) => !c.workspace || (projects && !projects.some((p) => p.slug === c.workspace)));
  const isMac = navigator.userAgent.includes("Mac");
  // For ducking: is Jarvis talking right now? (Read by the player on a timer, so it can't be state.)
  const speaking = useCallback(() => j.orb.mode() === "speak", [j.orb]);
  const chatRow = (c: ChatSummary) => (
    <ChatRow
      key={c.id}
      chat={c}
      active={j.chatId === c.id && page === "chat" && !docTask}
      projects={projects ?? []}
      onOpen={() => {
        setPage("chat");
        closePanel();
        j.openChat(c.id);
      }}
      onRename={(t) => j.renameChat(c.id, t).catch((e) => j.setError(String(e)))}
      onPin={(p) => j.pinChat(c.id, p).catch((e) => j.setError(String(e)))}
      onMove={(slug) => j.moveChat(c.id, slug).catch((e) => j.setError(String(e)))}
      onDelete={() => j.deleteChat(c.id)}
    />
  );

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
    <div className={`app${ytPlaying && !ytSlot ? " has-mini" : ""}`}>
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
              <button className={`label-link ${page === "projects" && !docTask ? "on" : ""}`} onClick={() => { closePanel(); setPage("projects"); }} title="All projects">
                Projects
              </button>
              <button className="mini" onClick={() => { closePanel(); setEditProject(null); setPage("newproject"); }} title="Create a project">
                + New
              </button>
            </div>
            {(projects ?? []).length === 0 && <p className="side-hint">Make a project to keep its chats, memories and code fixes together.</p>}
            {(projects ?? []).map((p) => {
              const open = openProjects.includes(p.slug);
              const mine = j.chats.filter((c) => c.workspace === p.slug);
              return (
                <div key={p.slug} className={`proj${ws?.slug === p.slug ? " current" : ""}`}>
                  <button className="proj-head" onClick={() => setOpen(p.slug, !open)} onDragOver={allowDrop} onDrop={(e) => dropChat(e, p.slug)} aria-expanded={open} title={p.folder || p.name}>
                    <svg className={`chev${open ? " open" : ""}`} viewBox="0 0 24 24" aria-hidden="true">
                      <path d="m9 6 6 6-6 6-1.4-1.4 4.6-4.6-4.6-4.6z" />
                    </svg>
                    <svg viewBox="0 0 24 24" aria-hidden="true">
                      <path d="M3 6a1 1 0 0 1 1-1h6l2 2h8a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z" />
                    </svg>
                    <b>{p.name}</b>
                    <i role="button" aria-label={`Open ${p.name} page`} title="Project page: instructions, files, chats" onClick={(e) => { e.stopPropagation(); showProject(p.slug); }}>
                      ⋯
                    </i>
                    <i role="button" aria-label={`New chat in ${p.name}`} title={`New chat in ${p.name}`} onClick={(e) => { e.stopPropagation(); newChatIn(p.slug); }}>
                      +
                    </i>
                  </button>
                  {open && (
                    <div className="lib nested">
                      {mine.length === 0 && (
                        <button className="ghost" onClick={() => newChatIn(p.slug)}>
                          <b>+ New chat</b>
                        </button>
                      )}
                      {mine.map(chatRow)}
                    </div>
                  )}
                </div>
              );
            })}
            <div className="label">
              Chats
              <button className="mini" onClick={() => newChatIn("")} title="Start a conversation outside any project">
                New
              </button>
            </div>
            <div className="lib" onDragOver={allowDrop} onDrop={(e) => dropChat(e, "")}>{looseChats.map(chatRow)}</div>
          </div>
          <nav className="nav">
            <div className="navgroup">
              <div className="label">Create</div>
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
            </div>
            <div className="navgroup">
              <div className="label">Apps</div>
            <button
              className={`navitem ${page === "apps" && !docTask ? "on" : ""}`}
              onClick={() => {
                closePanel();
                setPage("apps");
              }}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="M4 4h7v7H4zm9 0h7v7h-7zM4 13h7v7H4zm11.5 0a4.500 4.500 0 1 1 0 9 4.500 4.500 0 0 1 0-9z" />
              </svg>
              All apps
            </button>
            <button
              className={`navitem ${page === "day" && !docTask ? "on" : ""}`}
              onClick={() => {
                closePanel();
                setPage("day");
              }}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="M12 7a5 5 0 1 1 0 10 5 5 0 0 1 0-10zM11 1h2v3h-2zm0 19h2v3h-2zM1 11h3v2H1zm19 0h3v2h-3zM4.2 5.6l1.4-1.4 2.1 2.1-1.4 1.4zm12.1 12.1 1.4-1.4 2.1 2.1-1.4 1.4zM5.6 19.8l-1.4-1.4 2.1-2.1 1.4 1.4zM17.7 7.7l-1.4-1.4 2.1-2.1 1.4 1.4z" />
              </svg>
              Today
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
              className={`navitem ${page === "mail" && !docTask ? "on" : ""}`}
              onClick={() => {
                closePanel();
                setPage("mail");
              }}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="M3 5h18a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1zm1 2v.5l8 5 8-5V7zm16 2.9-8 5-8-5V17h16z" />
              </svg>
              Mail
            </button>
            <button
              className={`navitem ${page === "calendar" && !docTask ? "on" : ""}`}
              onClick={() => {
                closePanel();
                setPage("calendar");
              }}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="M7 2h2v2h6V2h2v2h3a1 1 0 0 1 1 1v15a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h3zM5 9v10h14V9zm2 2h4v4H7z" />
              </svg>
              Calendar
            </button>
            <button
              className={`navitem ${page === "drive" && !docTask ? "on" : ""}`}
              onClick={() => {
                closePanel();
                setPage("drive");
              }}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="M8.2 3h7.6l6.2 10.7-3.8 6.3H5.8L2 13.7zm.9 2L4.3 13.7 6.6 17.9 11.4 9.6zm6.1 0H10l5.6 9.7h5zM8 16l-.1.1.9 1.9h8.6l1.1-1.9z" />
              </svg>
              Drive
            </button>
            <button
              className={`navitem ${page === "youtube" && !docTask ? "on" : ""}`}
              onClick={() => {
                closePanel();
                setPage("youtube");
              }}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="M21.6 7.2a2.5 2.5 0 0 0-1.8-1.8C18.2 5 12 5 12 5s-6.2 0-7.8.4A2.5 2.5 0 0 0 2.4 7.2C2 8.8 2 12 2 12s0 3.2.4 4.8a2.5 2.5 0 0 0 1.8 1.8C5.8 19 12 19 12 19s6.2 0 7.8-.4a2.5 2.5 0 0 0 1.8-1.8c.4-1.6.4-4.8.4-4.8s0-3.2-.4-4.8zM10 15V9l5.2 3z" />
              </svg>
              YouTube
            </button>
            <button
              className={`navitem ${page === "memory" && !docTask ? "on" : ""}`}
              onClick={() => {
                closePanel();
                setPage("memory");
              }}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="M12 2a7 7 0 0 0-4 12.7V18h8v-3.3A7 7 0 0 0 12 2zm-3 18h6v2H9z" />
              </svg>
              Memory{ws ? ` · ${ws.name}` : ""}
            </button>
            </div>
            <div className="navgroup">
              <div className="label">Automate</div>
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
            </div>
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
        ) : page === "apps" ? (
          <AppsPage onOpen={(p) => { closePanel(); setPage(p); }} onOpenSettings={() => setShowSettings(true)} onClose={() => setPage("chat")} />
        ) : page === "day" ? (
          <DayPage
            tasks={j.tasks}
            userName={j.settings?.userName?.trim() ?? ""}
            onBrief={() => {
              setPage("chat");
              j.sendTyped("Brief me on my day: my schedule, mail that needs me, and what you're working on.");
            }}
            onOpenTask={(id) => { setPage("chat"); j.setReportId(id); }}
            onOpenMail={() => setPage("mail")}
            onOpenCalendar={() => setPage("calendar")}
            onOpenSettings={() => setShowSettings(true)}
            onClose={() => setPage("chat")}
          />
        ) : page === "projects" ? (
          <ProjectsPage
            projects={projects ?? []}
            chats={j.chats}
            onNew={() => { setEditProject(null); setPage("newproject"); }}
            onOpen={showProject}
            onNewChat={newChatIn}
            onEdit={(slug) => { setEditProject(slug); setPage("newproject"); }}
            onClose={() => setPage("chat")}
          />
        ) : page === "project" && (projects ?? []).find((p) => p.slug === viewProject) ? (
          <ProjectPage
            key={viewProject!}
            project={(projects ?? []).find((p) => p.slug === viewProject)!}
            chats={j.chats}
            onChanged={() => invoke<Workspace[]>("list_workspaces").then((l) => { setProjects(l); window.dispatchEvent(new Event("jarvis-workspace")); }).catch(() => {})}
            onOpenChat={(id) => { setOpen(viewProject!, true); setPage("chat"); closePanel(); j.openChat(id); }}
            onNewChat={() => newChatIn(viewProject!)}
            onEdit={() => { setEditProject(viewProject); setPage("newproject"); }}
            onClose={() => setPage("projects")}
          />
        ) : page === "newproject" ? (
          <NewProjectPage
            key={editProject ?? "new"}
            project={(projects ?? []).find((p) => p.slug === editProject) ?? null}
            onSaved={projectSaved}
            onClose={() => {
              const back = editProject;
              setEditProject(null);
              setViewProject(back);
              setPage(back ? "project" : "projects");
            }}
          />
        ) : page === "memory" ? (
          <MemoryPage onClose={() => setPage("chat")} />
        ) : page === "knowhow" ? (
          <KnowHowPage tasks={j.tasks} onClose={() => setPage("chat")} />
        ) : page === "mail" ? (
          <MailPage
            onAsk={(t) => {
              setPage("chat");
              j.sendTyped(t);
            }}
            onOpenSettings={() => setShowSettings(true)}
            onClose={() => setPage("chat")}
          />
        ) : page === "calendar" ? (
          <CalendarPage
            settings={j.settings}
            onAsk={(t) => {
              setPage("chat");
              j.sendTyped(t);
            }}
            onOpenSettings={() => setShowSettings(true)}
            onClose={() => setPage("chat")}
          />
        ) : page === "drive" ? (
          <DrivePage
            onAsk={(t) => {
              setPage("chat");
              j.sendTyped(t);
            }}
            onOpenSettings={() => setShowSettings(true)}
            onClose={() => setPage("chat")}
          />
        ) : page === "youtube" ? (
          <YouTubePage
            intent={ytIntent}
            playing={ytPlaying}
            onPlay={setYtPlaying}
            onSlot={setYtSlot}
            audioOnly={ytAudioOnly}
            onAudioOnly={audioOnly}
            onAsk={(t) => {
              setPage("chat");
              j.sendTyped(t);
            }}
            onOpenSettings={() => setShowSettings(true)}
            onClose={() => setPage("chat")}
          />
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
            <div className="chat-proj">
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="M3 6a1 1 0 0 1 1-1h6l2 2h8a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z" />
              </svg>
              <select
                value={ws?.slug ?? ""}
                onChange={(e) => j.moveChat(j.chatId, e.target.value).catch((err) => j.setError(String(err)))}
                aria-label="Project for this chat"
                title="Move this chat to a project"
              >
                <option value="">No project</option>
                {(projects ?? []).map((p) => (
                  <option key={p.slug} value={p.slug}>{p.name}</option>
                ))}
              </select>
              {ws?.folder && <span className="muted small" title={ws.folder}>{ws.folder.replace(/^\/Users\/[^/]+/, "~")}</span>}
              <span className="grow" />
              <select
                className="lang-pick"
                value={["", "en", "ne", "hi"].includes(j.settings?.language ?? "") ? j.settings?.language ?? "" : "other"}
                onChange={(e) => e.target.value !== "other" && j.setLanguage(e.target.value).catch((err) => j.setError(String(err)))}
                aria-label="Language Jarvis speaks"
                title="Language Jarvis speaks"
              >
                <option value="">Auto language</option>
                <option value="en">English</option>
                <option value="ne">नेपाली</option>
                <option value="hi">हिन्दी</option>
                {!["", "en", "ne", "hi"].includes(j.settings?.language ?? "") && <option value="other">{j.settings?.language}</option>}
              </select>
            </div>
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
                  {j.permission && (
                    <button
                      className="mini"
                      onClick={() => {
                        invoke(j.permission === "screen" ? "request_screen_recording" : "request_accessibility").catch(() => {});
                        j.setPermission(null);
                      }}
                    >
                      {j.permission === "screen" ? "Turn on Screen Recording" : "Turn on Accessibility"}
                    </button>
                  )}
                  {j.micBlocked && (
                    <button className="mini" onClick={j.openMicSettings}>
                      Open Microphone settings
                    </button>
                  )}
                  <button
                    className="mini"
                    onClick={() => {
                      j.setError("");
                      j.setPermission(null);
                    }}
                  >
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
                    <li>Select some text in any app, then: “Explain this” or “Rewrite this more politely.”</li>
                    <li>Hit an error in your code: “Look at this error and fix it in the Jarvis project.”</li>
                    <li>Press ⌥⇧Space to type a command instead of speaking.</li>
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
              <button
                type="button"
                className="icon-btn"
                onClick={() => (j.meeting ? j.stopMeeting() : j.startMeeting(""))}
                aria-label={j.meeting ? "Stop recording the meeting" : "Record a meeting"}
                title={j.meeting ? "Stop recording" : "Record a meeting: transcript, decisions and action items"}
              >
                <svg viewBox="0 0 24 24">
                  {j.meeting ? <path d="M7 7h10v10H7z" /> : <path d="M12 7a5 5 0 1 0 0 10 5 5 0 0 0 0-10zm0-5C6.5 2 2 6.5 2 12s4.5 10 10 10 10-4.5 10-10S17.5 2 12 2zm0 18a8 8 0 1 1 0-16 8 8 0 0 1 0 16z" />}
                </svg>
              </button>
              <button type="button" className="icon-btn clip" onClick={pickFiles} aria-label="Attach images" title="Attach logo or reference images">
                <svg viewBox="0 0 24 24">
                  <path d="M16.5 6.5v9.8a4.5 4.5 0 0 1-9 0V5.8a3 3 0 0 1 6 0v9.7a1.5 1.5 0 0 1-3 0V6.5H9v9a3 3 0 0 0 6 0V5.8a4.5 4.5 0 0 0-9 0v10.5a6 6 0 0 0 12 0V6.5z" />
                </svg>
              </button>
              <ModePicker />
              <input
                id="typed"
                className="typebox"
                value={typed}
                onChange={(e) => setTyped(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Tab" && e.shiftKey) {
                    e.preventDefault();
                    setMode(nextMode(getMode()));
                  }
                }}
                placeholder="Type instead of speaking…"
              />
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
                director={j.director[panelTask.id]}
                onReview={() => j.reviewSite(panelTask.id)}
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

      {showPalette && <CommandPalette onSend={(t) => j.sendTyped(t)} onClose={() => setShowPalette(false)} />}
      {j.meeting && <MeetingBanner title={j.meeting.title} startedAt={j.meeting.startedAt} onStop={j.stopMeeting} />}
      {j.controlling && (
        <div className="control-banner" role="status">
          <span>
            Jarvis is controlling <b>{j.controlling}</b>
          </span>
          <button className="btn" onClick={j.stopSpeaking}>
            Stop <kbd>⌥.</kbd>
          </button>
        </div>
      )}
      <ApprovalCard />
      <YtDock playing={ytPlaying} slot={page === "youtube" ? ytSlot : null} audioOnly={ytAudioOnly} onAudioOnly={audioOnly} jarvisSpeaking={speaking} onClose={() => setYtPlaying(null)} onOpenPage={() => { closePanel(); setPage("youtube"); }} />

      {showSettings && j.settings && (
        <SettingsPanel initial={j.settings} onClose={() => setShowSettings(false)} onSaved={() => j.reloadSettings()} />
      )}
    </div>
  );
}
