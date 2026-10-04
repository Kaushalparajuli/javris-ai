import { useMemo, useState } from "react";
import { day } from "../lib/format";
import type { ChatSummary, Task } from "../lib/types";
import ImageThumb from "./ImageThumb";

type Kind = "all" | "research" | "document" | "slides" | "image" | "browser";
type Scope = "all" | "chat";

const SCOPE_KEY = "jarvis.library.scope";
function savedScope(): Scope {
  try {
    return localStorage.getItem(SCOPE_KEY) === "chat" ? "chat" : "all";
  } catch {
    return "all";
  }
}

const KINDS: { id: Kind; label: string }[] = [
  { id: "all", label: "All" },
  { id: "research", label: "Reports" },
  { id: "document", label: "Docs" },
  { id: "slides", label: "Decks" },
  { id: "browser", label: "Browser" },
  { id: "image", label: "Images" },
];

/** What a finished task is, in one short phrase under its title. */
function subtitle(t: Task) {
  if (t.kind === "slides") return "slide deck";
  if (t.kind === "image") return t.images.length > 1 ? `${t.images.length} images` : "image";
  if (t.kind === "browser") return t.images.length ? `browser · ${t.images.length} screenshot${t.images.length === 1 ? "" : "s"}` : "browser";
  if (t.kind === "document") return t.sources ? `document · ${t.sources} source${t.sources === 1 ? "" : "s"}` : "document";
  const depth = t.depth === "quick" ? "quick" : "deep dive";
  return t.sources ? `${depth} · ${t.sources} source${t.sources === 1 ? "" : "s"}` : depth;
}

/** Everything the worker has finished, as a full page beside the chat list. */
export default function LibraryPage({
  items,
  chatId,
  chats,
  openId,
  onOpen,
  onNew,
  onOpenFile,
  onClose,
}: {
  items: Task[];
  /** The conversation that's open, for the "This chat" view. */
  chatId: string;
  chats: ChatSummary[];
  openId: number | null;
  onOpen: (id: number) => void;
  onNew: () => void;
  onOpenFile: () => void;
  onClose: () => void;
}) {
  const [q, setQ] = useState("");
  const [kind, setKind] = useState<Kind>("all");
  const [scope, setScope] = useState<Scope>(savedScope);
  const pickScope = (next: Scope) => {
    setScope(next);
    try {
      localStorage.setItem(SCOPE_KEY, next);
    } catch {
      /* not remembered this time */
    }
  };
  const chatName = useMemo(() => new Map(chats.map((c) => [c.id, c.title || "New chat"])), [chats]);
  const inScope = useMemo(() => (scope === "all" ? items : items.filter((t) => t.chatId === chatId)), [items, scope, chatId]);

  const shown = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return inScope
      .filter((t) => (kind === "all" ? true : t.kind === kind))
      .filter((t) => (needle ? `${t.title} ${t.request} ${t.summary}`.toLowerCase().includes(needle) : true))
      .sort((a, b) => (b.finishedAt ?? b.startedAt) - (a.finishedAt ?? a.startedAt));
  }, [inScope, q, kind]);

  return (
    <section className="libpage">
      <header>
        <button className="icon-btn back" onClick={onClose} aria-label="Back to the conversation" title="Back">
          <svg viewBox="0 0 24 24">
            <path d="M15.4 4.6 13.9 3.2 6.1 11l7.8 7.8 1.5-1.4L9 11z" />
          </svg>
        </button>
        <div>
          <h2>Library</h2>
          <p className="muted small">
            {inScope.length === 0 ? "Nothing yet" : `${inScope.length} item${inScope.length === 1 ? "" : "s"}`}
            {scope === "chat" ? ` in “${chatName.get(chatId) ?? "this chat"}”` : " across all chats"}
          </p>
        </div>
        <input className="typebox find" value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search the library…" aria-label="Search the library" />
        <button className="mini" onClick={onOpenFile}>
          Open file…
        </button>
        <button className="btn primary" onClick={onNew}>
          New document
        </button>
        <div className="seg" role="group" aria-label="Which chats">
          <button className={scope === "all" ? "on" : ""} onClick={() => pickScope("all")}>
            All chats
          </button>
          <button className={scope === "chat" ? "on" : ""} onClick={() => pickScope("chat")}>
            This chat
          </button>
        </div>
        <div className="seg" role="group" aria-label="Filter by type">
          {KINDS.map((k) => (
            <button key={k.id} className={kind === k.id ? "on" : ""} onClick={() => setKind(k.id)}>
              {k.label}
            </button>
          ))}
        </div>
      </header>

      {shown.length === 0 ? (
        <p className="muted small pad">
          {items.length === 0
            ? "Finished research, documents and images land here. Try saying “Jarvis, research the best vector databases. Deep dive.”"
            : inScope.length === 0
              ? "Nothing from this chat yet. Switch to All chats to see everything."
              : "Nothing matches that."}
        </p>
      ) : (
        <div className="grid">
          {shown.map((t) => (
            <button key={t.id} className={`card ${openId === t.id ? "on" : ""}`} onClick={() => onOpen(t.id)} title={t.title}>
              <div className="thumb">
                {(t.kind === "image" || t.kind === "browser") && t.images[0] ? (
                  <ImageThumb path={t.images[0]} alt={t.title} className="shot" />
                ) : t.kind === "document" ? (
                  <svg viewBox="0 0 24 24" aria-hidden="true">
                    <path d="M5 3h9l5 5v5.2l-2 2V9h-4V5H7v14h6.5l-2 2H5zm14.7 10.3 1.4 1.4-6.4 6.4-2.1.6.6-2.1z" />
                  </svg>
                ) : (
                  <svg viewBox="0 0 24 24" aria-hidden="true">
                    <path d="M6 2h8l4 4v16H6zm7 1.5V7h3.5zM8 11h8v1.5H8zm0 3.5h8V16H8zm0 3.5h5v1.5H8z" />
                  </svg>
                )}
              </div>
              <b>{t.title}</b>
              <span>
                {day(t.finishedAt ?? t.startedAt)} · {subtitle(t)}
              </span>
              {scope === "all" && t.chatId && chatName.has(t.chatId) && <small className="from">{chatName.get(t.chatId)}</small>}
            </button>
          ))}
        </div>
      )}
    </section>
  );
}
