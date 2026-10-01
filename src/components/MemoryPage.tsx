import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import type { Memory, Workspace } from "../lib/types";

const KINDS = ["person", "project", "decision", "preference", "fact", "note"];
/** What the filter chips offer: the kinds you can add, plus the ones Jarvis files itself. */
const FILTER_KINDS = [...KINDS, "meeting"];

function source(s: string) {
  if (s.startsWith("chat:")) return "From a chat";
  if (s.startsWith("meeting:")) return "From a meeting";
  if (s === "user" || !s) return "Added by you";
  return s;
}

/** What Jarvis remembers: search it, sort it by project and kind, add to it, correct it. */
export default function MemoryPage({ onClose }: { onClose: () => void }) {
  const [mems, setMems] = useState<Memory[]>([]);
  const [spaces, setSpaces] = useState<Workspace[]>([]);
  const [active, setActive] = useState("");
  const [q, setQ] = useState("");
  const [project, setProject] = useState("all");
  const [kindFilter, setKindFilter] = useState("all");
  const [text, setText] = useState("");
  const [kind, setKind] = useState("fact");
  const [scope, setScope] = useState<string | null>(null);
  const [editing, setEditing] = useState<{ id: number; text: string; kind: string } | null>(null);
  const [sure, setSure] = useState<number | null>(null);
  const [error, setError] = useState("");

  const loadSpaces = () => {
    invoke<Workspace[]>("list_workspaces").then(setSpaces).catch(() => {});
    invoke<Workspace | null>("get_active_workspace").then((w) => setActive(w?.slug ?? "")).catch(() => {});
  };
  const loadMems = () => {
    const ws = project === "all" ? undefined : project === "global" ? "" : project;
    const run = q.trim()
      ? invoke<Memory[]>("memory_search", { query: q, workspace: ws, limit: 150 })
      : invoke<Memory[]>("memory_list", { workspace: ws, limit: 300 });
    run.then(setMems).catch((e) => setError(String(e)));
  };
  useEffect(loadSpaces, []);
  useEffect(loadMems, [q, project]);

  const nameOf = (slug: string) => (slug ? spaces.find((s) => s.slug === slug)?.name ?? slug : "Everywhere");
  const addScope = scope ?? active;

  const add = async () => {
    if (!text.trim()) return;
    try {
      await invoke("memory_add", { kind, text, workspace: addScope, source: "user" });
      setText("");
      loadMems();
    } catch (e) {
      setError(String(e));
    }
  };
  const act = (p: Promise<unknown>) => p.then(loadMems).catch((e) => setError(String(e)));
  const saveEdit = async () => {
    if (!editing) return;
    const e = editing;
    setEditing(null);
    if (e.text.trim()) await act(invoke("memory_update", { id: e.id, text: e.text, kind: e.kind }));
  };

  const counts = useMemo(() => {
    const c = new Map<string, number>();
    for (const m of mems) c.set(m.kind, (c.get(m.kind) ?? 0) + 1);
    return c;
  }, [mems]);
  const shown = mems.filter((m) => kindFilter === "all" || m.kind === kindFilter);
  const pinned = shown.filter((m) => m.pinned);
  const rest = shown.filter((m) => !m.pinned);

  const row = (m: Memory) => (
    <li key={m.id} className={`mem-row${m.pinned ? " pinned" : ""}`}>
      <span className={`mem-kind k-${m.kind}`}>{m.kind}</span>
      <div className="mem-body">
        {editing?.id === m.id ? (
          <div className="mem-edit">
            <textarea
              autoFocus
              rows={2}
              value={editing.text}
              onChange={(e) => setEditing({ ...editing, text: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) saveEdit();
                else if (e.key === "Escape") setEditing(null);
              }}
            />
            <div className="mem-edit-bar">
              <select value={editing.kind} onChange={(e) => setEditing({ ...editing, kind: e.target.value })}>
                {FILTER_KINDS.map((k) => <option key={k}>{k}</option>)}
              </select>
              <span className="grow" />
              <button className="mini" onClick={() => setEditing(null)}>Cancel</button>
              <button className="mini primary" onClick={saveEdit}>Save</button>
            </div>
          </div>
        ) : (
          <>
            <p className="mem-text">{m.text}</p>
            <div className="mem-meta">
              <span className="mem-proj">{nameOf(m.workspace)}</span>
              <span>{source(m.source)}</span>
              <span>{new Date(m.created * 1000).toLocaleDateString([], { day: "numeric", month: "short", year: "numeric" })}</span>
            </div>
          </>
        )}
      </div>
      {editing?.id !== m.id && (
        <div className="mem-actions">
          <button className={`icon-btn${m.pinned ? " on" : ""}`} onClick={() => act(invoke("memory_update", { id: m.id, pinned: !m.pinned }))} title={m.pinned ? "Unpin" : "Pin to the top"} aria-label={m.pinned ? "Unpin" : "Pin"}>
            <svg viewBox="0 0 24 24"><path d="M14 3 21 10l-2 1-3 3 .5 4.5-1.5 1.5-3.5-3.5L6 21l-1-1 4.5-4.5L6 12l1.5-1.5L12 11l3-3z" /></svg>
          </button>
          <button className="icon-btn" onClick={() => setEditing({ id: m.id, text: m.text, kind: m.kind })} title="Edit" aria-label="Edit">
            <svg viewBox="0 0 24 24"><path d="M3 17.3V21h3.7L18 9.7 14.3 6zM20.7 7a1 1 0 0 0 0-1.4l-2.3-2.3a1 1 0 0 0-1.4 0l-1.8 1.8L19 8.8z" /></svg>
          </button>
          {sure === m.id ? (
            <button className="mini danger" onClick={() => { setSure(null); act(invoke("memory_delete", { id: m.id })); }} onBlur={() => setSure(null)} autoFocus>
              Forget?
            </button>
          ) : (
            <button className="icon-btn" onClick={() => setSure(m.id)} title="Forget" aria-label="Forget">
              <svg viewBox="0 0 24 24"><path d="M9 3h6l1 2h4v2H4V5h4zM6 9h12l-1 12H7z" /></svg>
            </button>
          )}
        </div>
      )}
    </li>
  );

  return (
    <section className="libpage knowpage">
      <header className="lib-head">
        <button className="icon-btn back" onClick={onClose} aria-label="Back" title="Back">
          ←
        </button>
        <div>
          <h2>Memory{mems.length ? <span className="mem-count">{mems.length}</span> : null}</h2>
          <p className="muted small">What Jarvis remembers about you and your projects. Stored on this Mac.</p>
        </div>
      </header>
      <div className="rt-body proj-body mem-body-wrap">
        {error && (
          <div className="banner" role="alert">
            <span>{error}</span>
            <button className="mini" onClick={() => setError("")}>Dismiss</button>
          </div>
        )}

        <form className="mem-compose" onSubmit={(e) => { e.preventDefault(); add(); }}>
          <input placeholder="Tell Jarvis something to remember…" value={text} onChange={(e) => setText(e.target.value)} aria-label="New memory" />
          <select value={kind} onChange={(e) => setKind(e.target.value)} aria-label="Kind">
            {KINDS.map((k) => <option key={k}>{k}</option>)}
          </select>
          <select value={addScope} onChange={(e) => setScope(e.target.value)} aria-label="Where it applies">
            <option value="">Everywhere</option>
            {spaces.map((s) => <option key={s.slug} value={s.slug}>{s.name}</option>)}
          </select>
          <button className="btn primary" disabled={!text.trim()}>Remember</button>
        </form>

        <div className="mem-toolbar">
          <div className="mem-search">
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M10.5 4a6.5 6.5 0 0 1 5.2 10.4l4.4 4.4-1.4 1.4-4.4-4.4A6.5 6.5 0 1 1 10.5 4zm0 2a4.5 4.5 0 1 0 0 9 4.5 4.5 0 0 0 0-9z" /></svg>
            <input placeholder="Search memories" value={q} onChange={(e) => setQ(e.target.value)} aria-label="Search memories" />
          </div>
          <select value={project} onChange={(e) => setProject(e.target.value)} aria-label="Project">
            <option value="all">All projects</option>
            <option value="global">Everywhere</option>
            {spaces.map((s) => <option key={s.slug} value={s.slug}>{s.name}</option>)}
          </select>
        </div>

        <div className="mem-chips" role="tablist" aria-label="Kind">
          <button className={`chipbtn ${kindFilter === "all" ? "on" : ""}`} onClick={() => setKindFilter("all")}>All · {mems.length}</button>
          {FILTER_KINDS.filter((k) => counts.get(k)).map((k) => (
            <button key={k} className={`chipbtn ${kindFilter === k ? "on" : ""}`} onClick={() => setKindFilter(k)}>{k} · {counts.get(k)}</button>
          ))}
        </div>

        {shown.length === 0 ? (
          <div className="mem-empty">
            <h3>{q || kindFilter !== "all" || project !== "all" ? "No memory matches" : "Nothing remembered yet"}</h3>
            <p className="muted">{q || kindFilter !== "all" || project !== "all" ? "Try a different search or filter." : "Say “remember that…” to Jarvis, or add something above. Jarvis also picks out decisions and preferences from your chats and meetings."}</p>
          </div>
        ) : (
          <>
            {pinned.length > 0 && (
              <>
                <div className="mem-group">Pinned</div>
                <ul className="mem-list">{pinned.map(row)}</ul>
              </>
            )}
            {rest.length > 0 && (
              <>
                {pinned.length > 0 && <div className="mem-group">Everything else</div>}
                <ul className="mem-list">{rest.map(row)}</ul>
              </>
            )}
          </>
        )}
      </div>
    </section>
  );
}
