import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState } from "react";
import { day } from "../lib/format";

interface Hit {
  kind: "chat" | "document" | "research" | "image" | "browser";
  chatId: string;
  taskId: number | null;
  title: string;
  snippet: string;
  at: number;
  inTitle: boolean;
}

interface FileHit {
  path: string;
  name: string;
  folder: string;
  snippet: string;
}

interface IndexStatus {
  folders: string[];
  files: number;
  chunks: number;
  embedded: number;
  scanning: boolean;
}

const GROUPS: { kind: Hit["kind"]; label: string }[] = [
  { kind: "chat", label: "Chats" },
  { kind: "document", label: "Documents" },
  { kind: "research", label: "Reports" },
  { kind: "browser", label: "Browser tasks" },
  { kind: "image", label: "Images" },
];

/** Wrap each case-insensitive match of `needle` in <mark>. */
function highlight(text: string, needle: string) {
  if (!needle) return text;
  const safe = needle.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return text.split(new RegExp(`(${safe})`, "giu")).map((part, i) => (i % 2 ? <mark key={i}>{part}</mark> : part));
}

/** Search across chats, documents, reports and images, as you type. */
export default function SearchPage({
  openId,
  onOpenChat,
  onOpenTask,
  onClose,
}: {
  openId: number | null;
  onOpenChat: (id: string) => void;
  onOpenTask: (id: number) => void;
  onClose: () => void;
}) {
  const [q, setQ] = useState("");
  const [hits, setHits] = useState<Hit[]>([]);
  const [busy, setBusy] = useState(false);
  const [files, setFiles] = useState<FileHit[]>([]);
  const [index, setIndex] = useState<IndexStatus | null>(null);
  const [indexBusy, setIndexBusy] = useState(false);
  const [indexError, setIndexError] = useState("");
  const input = useRef<HTMLInputElement>(null);
  const needle = q.trim();

  useEffect(() => input.current?.focus(), []);
  useEffect(() => {
    invoke<IndexStatus>("index_status").then(setIndex).catch(() => {});
  }, []);

  const changeFolders = async (job: Promise<IndexStatus>) => {
    setIndexBusy(true);
    setIndexError("");
    try {
      setIndex(await job);
    } catch (e) {
      setIndexError(String(e));
    } finally {
      setIndexBusy(false);
    }
  };
  const addFolder = async () => {
    const picked = await openDialog({ directory: true, multiple: false, title: "Choose a folder for Jarvis to search" }).catch(() => null);
    if (typeof picked === "string") changeFolders(invoke<IndexStatus>("index_add_folder", { path: picked }));
  };

  useEffect(() => {
    if (needle.length < 2) {
      setHits([]);
      setFiles([]);
      setBusy(false);
      return;
    }
    let live = true;
    setBusy(true);
    const t = window.setTimeout(() => {
      invoke<Hit[]>("search", { query: needle })
        .then((h) => live && setHits(h))
        .catch(() => live && setHits([]))
        .finally(() => live && setBusy(false));
      invoke<FileHit[]>("index_search", { query: needle, limit: 8 })
        .then((f) => live && setFiles(f))
        .catch(() => live && setFiles([]));
    }, 180);
    return () => {
      live = false;
      window.clearTimeout(t);
    };
  }, [needle]);

  const groups = useMemo(() => GROUPS.map((g) => ({ ...g, hits: hits.filter((h) => h.kind === g.kind) })).filter((g) => g.hits.length), [hits]);

  return (
    <section className="libpage searchpage">
      <header>
        <button className="icon-btn back" onClick={onClose} aria-label="Back to the conversation" title="Back">
          <svg viewBox="0 0 24 24">
            <path d="M15.4 4.6 13.9 3.2 6.1 11l7.8 7.8 1.5-1.4L9 11z" />
          </svg>
        </button>
        <label className="search-field">
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M10.5 4a6.5 6.5 0 0 1 5.2 10.4l4.4 4.4-1.4 1.4-4.4-4.4A6.5 6.5 0 1 1 10.5 4zm0 2a4.5 4.5 0 1 0 0 9 4.5 4.5 0 0 0 0-9z" />
          </svg>
          <input
            ref={input}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && (q ? setQ("") : onClose())}
            placeholder="Search chats, documents, reports and images…"
            aria-label="Search"
          />
        </label>
        <span className="muted small count" aria-live="polite">
          {needle.length < 2 ? "" : busy ? "Searching…" : `${hits.length + files.length} result${hits.length + files.length === 1 ? "" : "s"}`}
        </span>
      </header>

      {needle.length >= 2 && !busy && hits.length === 0 && files.length === 0 && <p className="muted small pad">Nothing found for “{needle}”.</p>}

      <div className="results">
        {files.length > 0 && (
          <section>
            <div className="label">Files · {files.length}</div>
            {files.map((f) => (
              <button key={f.path} className="hit" onClick={() => invoke("reveal_path", { path: f.path }).catch(() => {})} title={f.path}>
                <b>{highlight(f.name, needle)}</b>
                <span className="when">{f.folder.split("/").pop()}</span>
                <span className="snip">{highlight(f.snippet, needle)}</span>
              </button>
            ))}
          </section>
        )}
        {groups.map((g) => (
          <section key={g.kind}>
            <div className="label">
              {g.label} · {g.hits.length}
            </div>
            {g.hits.map((h) => (
              <button
                key={`${h.kind}-${h.taskId ?? h.chatId}`}
                className={`hit ${h.taskId != null && h.taskId === openId ? "on" : ""}`}
                onClick={() => (h.kind === "chat" ? onOpenChat(h.chatId) : h.taskId != null && onOpenTask(h.taskId))}
              >
                <b>{highlight(h.title, needle)}</b>
                <span className="when">{day(h.at)}</span>
                {h.snippet && <span className="snip">{highlight(h.snippet, needle)}</span>}
              </button>
            ))}
          </section>
        ))}
      </div>

      <section className="index-box">
        <div className="label">
          Files Jarvis can search
          <button className="mini" onClick={addFolder} disabled={indexBusy}>
            Add folder
          </button>
        </div>
        {index && index.folders.length === 0 && <p className="muted small">Add a folder of notes, documents or PDFs and search them here or by voice. Only folders you add are read.</p>}
        {index?.folders.map((f) => (
          <div className="folder-row" key={f}>
            <span title={f}>{f.replace(/^\/Users\/[^/]+/, "~")}</span>
            <button className="mini" onClick={() => changeFolders(invoke<IndexStatus>("index_remove_folder", { path: f }))} disabled={indexBusy}>
              Remove
            </button>
          </div>
        ))}
        {index && index.folders.length > 0 && (
          <p className="muted small">
            {indexBusy || index.scanning ? "Reading files… " : ""}
            {index.files} files · {index.chunks} passages · {index.embedded >= index.chunks ? "searchable by meaning" : `${index.embedded}/${index.chunks} ready for meaning search`}
            <button className="mini" onClick={() => changeFolders(invoke<IndexStatus>("index_refresh"))} disabled={indexBusy}>
              Refresh
            </button>
          </p>
        )}
        {indexError && <p className="error-text">{indexError}</p>}
      </section>
    </section>
  );
}
