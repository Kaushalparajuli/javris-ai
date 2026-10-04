import { invoke } from "@tauri-apps/api/core";
import { confirm, open as openDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import { day } from "../lib/format";
import type { ChatSummary, ProjectFile, Workspace } from "../lib/types";

function size(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

/** One project: its standing instructions, its files and its chats. */
export default function ProjectPage({
  project,
  chats,
  onChanged,
  onOpenChat,
  onNewChat,
  onEdit,
  onClose,
}: {
  project: Workspace;
  chats: ChatSummary[];
  onChanged: () => void;
  onOpenChat: (id: string) => void;
  onNewChat: () => void;
  onEdit: () => void;
  onClose: () => void;
}) {
  const [instructions, setInstructions] = useState(project.instructions);
  const [saved, setSaved] = useState(project.instructions);
  const [files, setFiles] = useState<ProjectFile[]>([]);
  const [q, setQ] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    invoke<ProjectFile[]>("project_files", { slug: project.slug }).then(setFiles).catch(() => {});
  }, [project.slug]);

  const saveInstructions = async () => {
    if (instructions === saved) return;
    try {
      await invoke("save_workspace", { workspace: { ...project, instructions } });
      setSaved(instructions);
      onChanged();
    } catch (e) {
      setError(String(e));
    }
  };

  const addFiles = async () => {
    const picked = await openDialog({ multiple: true, title: `Add files to ${project.name}` });
    if (!picked) return;
    setBusy(true);
    setError("");
    try {
      setFiles(await invoke<ProjectFile[]>("project_add_files", { slug: project.slug, paths: Array.isArray(picked) ? picked : [picked] }));
    } catch (e) {
      setError(String(e));
    }
    setBusy(false);
  };
  const removeFile = async (name: string) => {
    const ok = await confirm(`Move “${name}” to the Trash?\n\nIt's removed from this project. You can get it back from the Trash.`, {
      title: "Remove file",
      kind: "warning",
      okLabel: "Move to Trash",
    });
    if (!ok) return;
    invoke<ProjectFile[]>("project_remove_file", { slug: project.slug, name })
      .then(setFiles)
      .catch((e) => setError(String(e)));
  };

  const mine = chats.filter((c) => c.workspace === project.slug && (!q.trim() || (c.title || "New chat").toLowerCase().includes(q.trim().toLowerCase())));

  return (
    <section className="libpage knowpage">
      <header className="lib-head">
        <button className="icon-btn back" onClick={onClose} aria-label="Back to projects" title="Back to projects">
          ←
        </button>
        <div>
          <h2>{project.name}</h2>
          <p className="muted small">{project.description || "No description yet."}</p>
        </div>
        <span className="grow" />
        <button className="mini" onClick={onEdit}>Edit details</button>
        <button className="btn primary" onClick={onNewChat}>+ New chat</button>
      </header>
      <div className="rt-body proj-body">
        {error && (
          <div className="banner" role="alert">
            <span>{error}</span>
            <button className="mini" onClick={() => setError("")}>Dismiss</button>
          </div>
        )}
        <div className="pp-cols">
          <div className="pp-col">
            <section className="pp-card">
              <div className="pp-head">
                <h3>Instructions</h3>
                {instructions !== saved && <button className="mini" onClick={saveInstructions}>Save</button>}
              </div>
              <textarea
                rows={7}
                value={instructions}
                onChange={(e) => setInstructions(e.target.value)}
                onBlur={saveInstructions}
                placeholder="Tell Jarvis how to work in this project: tone, rules, what to focus on, things to avoid. It reads this at the start of every chat here."
              />
              <p className="muted small">Saved automatically when you click away.</p>
            </section>
            <section className="pp-card">
              <div className="pp-head">
                <h3>Files</h3>
                <button className="mini" onClick={addFiles} disabled={busy}>{busy ? "Adding…" : "+ Add files"}</button>
              </div>
              {files.length === 0 ? (
                <p className="muted small">Add notes, briefs or data. Jarvis can read the text files in any chat in this project. Each website it builds gets its own folder here.</p>
              ) : (
                <ul className="pp-files">
                  {files.map((f) => (
                    <li key={f.name}>
                      <button className="pp-fname" title="Show in Finder" onClick={() => revealItemInDir(`${project.folder}/${f.name}`).catch(() => {})}>
                        {f.isDir ? `📁 ${f.name}/` : f.name}
                      </button>
                      <span className="muted small">{f.isDir ? "folder" : size(f.size)}</span>
                      {!f.isDir && <button className="mini danger" onClick={() => removeFile(f.name)} aria-label={`Remove ${f.name}`}>Remove</button>}
                    </li>
                  ))}
                </ul>
              )}
              <p className="muted small" title={project.folder}>
                Folder: {project.folder.replace(/^\/Users\/[^/]+/, "~")}{" "}
                <button className="mini" onClick={() => revealItemInDir(`${project.folder}/${files[0]?.name ?? ""}`.replace(/\/$/, "")).catch(() => {})}>Show in Finder</button>
              </p>
            </section>
          </div>
          <section className="pp-card pp-chats">
            <div className="pp-head">
              <h3>Chats</h3>
            </div>
            <input className="proj-search" placeholder="Search chats in this project" value={q} onChange={(e) => setQ(e.target.value)} />
            {mine.length === 0 ? (
              <p className="muted small">{q ? "No chat matches." : "No chats yet. Start one with “New chat”."}</p>
            ) : (
              <ul className="pp-chatlist">
                {mine.map((c) => (
                  <li key={c.id}>
                    <button onClick={() => onOpenChat(c.id)}>
                      <b>{c.pinned ? "📌 " : ""}{c.title || "New chat"}</b>
                      <span className="muted small">{day(c.updatedAt)}</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </section>
        </div>
      </div>
    </section>
  );
}
