import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { day } from "../lib/format";
import type { ChatSummary, Workspace } from "../lib/types";
import IconButton from "./IconButton";

/** Every project as a card: what it is, how many chats, when it was last used. */
export default function ProjectsPage({
  projects,
  chats,
  onNew,
  onOpen,
  onNewChat,
  onEdit,
  onDeleted,
  onClose,
}: {
  projects: Workspace[];
  chats: ChatSummary[];
  onNew: () => void;
  onOpen: (slug: string) => void;
  onNewChat: (slug: string) => void;
  onEdit: (slug: string) => void;
  /** A project was deleted: reload the list. */
  onDeleted: () => void;
  onClose: () => void;
}) {
  const [q, setQ] = useState("");
  const [error, setError] = useState("");

  const remove = async (p: Workspace, count: number) => {
    const chatsLine = count ? `Its ${count} ${count === 1 ? "chat moves" : "chats move"} to Recents. ` : "";
    const ok = await confirm(`Delete the project “${p.name}”?\n\n${chatsLine}Its files folder stays on your Mac.`, {
      title: "Delete project",
      kind: "warning",
      okLabel: "Delete",
    });
    if (!ok) return;
    try {
      await invoke("delete_workspace", { slug: p.slug });
      onDeleted();
    } catch (e) {
      setError(`Couldn't delete “${p.name}”: ${e}`);
    }
  };
  const rows = projects
    .map((p) => {
      const mine = chats.filter((c) => c.workspace === p.slug);
      return { p, count: mine.length, last: mine.reduce((m, c) => Math.max(m, c.updatedAt), 0) };
    })
    .filter(({ p }) => !q.trim() || `${p.name} ${p.description}`.toLowerCase().includes(q.trim().toLowerCase()))
    .sort((a, b) => (b.last || b.p.created) - (a.last || a.p.created));

  return (
    <section className="libpage knowpage">
      <header className="lib-head">
        <button className="icon-btn back" onClick={onClose} aria-label="Back" title="Back">
          ←
        </button>
        <div>
          <h2>Projects</h2>
          <p className="muted small">Each project keeps its own chats, memories and files.</p>
        </div>
        <button className="btn primary" onClick={onNew}>+ New project</button>
      </header>
      <div className="rt-body proj-body">
        {error && <p className="error-text small">{error}</p>}
        {projects.length > 0 && <input className="proj-search" placeholder="Search projects" value={q} onChange={(e) => setQ(e.target.value)} />}
        {projects.length === 0 ? (
          <div className="proj-empty">
            <h3>No projects yet</h3>
            <p className="muted">Make a project for something you work on often. Chats inside it share its memories, instructions and files.</p>
            <button className="btn primary" onClick={onNew}>Create your first project</button>
          </div>
        ) : rows.length === 0 ? (
          <p className="muted">No project matches “{q}”.</p>
        ) : (
          <div className="proj-list">
            {rows.map(({ p, count, last }) => (
              <div key={p.slug} className="proj-row" role="button" tabIndex={0} onClick={() => onOpen(p.slug)} onKeyDown={(e) => e.key === "Enter" && onOpen(p.slug)}>
                <span className="proj-avatar" aria-hidden="true">{p.name.trim().charAt(0).toUpperCase()}</span>
                <div className="proj-main">
                  <strong>{p.name}</strong>
                  <p className={p.description ? "" : "muted"}>{p.description || "No description yet."}</p>
                </div>
                <span className="proj-meta muted small">
                  {count} {count === 1 ? "chat" : "chats"}
                  {last ? ` · updated ${day(last)}` : ""}
                </span>
                <span className="proj-actions">
                  <IconButton icon="newChat" label={`New chat in ${p.name}`} onClick={(e) => { e.stopPropagation(); onNewChat(p.slug); }} />
                  <IconButton icon="edit" label={`Edit ${p.name}`} onClick={(e) => { e.stopPropagation(); onEdit(p.slug); }} />
                  <IconButton icon="delete" label={`Delete ${p.name}`} danger onClick={(e) => { e.stopPropagation(); remove(p, count); }} />
                </span>
              </div>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}
