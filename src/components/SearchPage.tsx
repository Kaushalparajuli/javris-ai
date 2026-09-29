import { invoke } from "@tauri-apps/api/core";
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
  const input = useRef<HTMLInputElement>(null);
  const needle = q.trim();

  useEffect(() => input.current?.focus(), []);

  useEffect(() => {
    if (needle.length < 2) {
      setHits([]);
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
          {needle.length < 2 ? "" : busy ? "Searching…" : `${hits.length} result${hits.length === 1 ? "" : "s"}`}
        </span>
      </header>

      {needle.length >= 2 && !busy && hits.length === 0 && <p className="muted small pad">Nothing found for “{needle}”.</p>}

      <div className="results">
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
    </section>
  );
}
