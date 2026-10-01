import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import type { Workspace } from "../lib/types";

/** Create or edit a project: what it's called and what it's for, like Claude. Its folder is made automatically and never changes. */
export default function NewProjectPage({ project, onSaved, onClose }: { project?: Workspace | null; onSaved: (p: Workspace) => void; onClose: () => void }) {
  const [name, setName] = useState(project?.name ?? "");
  const [goal, setGoal] = useState(project?.description ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const create = async () => {
    if (!name.trim() || busy) return;
    setBusy(true);
    setError("");
    try {
      const made = project ?? (await invoke<Workspace>("create_project", { name }));
      const saved = await invoke<Workspace>("save_workspace", { workspace: { ...made, name: name.trim(), description: goal.trim() } });
      onSaved(saved);
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };

  return (
    <section className="libpage knowpage">
      <header className="lib-head">
        <button className="icon-btn back" onClick={onClose} aria-label="Back" title="Back">
          ←
        </button>
        <div>
          <h2>{project ? "Edit project" : "Create a project"}</h2>
          <p className="muted small">Chats in a project share its memories, instructions and folder.</p>
        </div>
      </header>
      <div className="rt-body">
        {error && (
          <div className="banner" role="alert">
            <span>{error}</span>
            <button className="mini" onClick={() => setError("")}>Dismiss</button>
          </div>
        )}
        <form className="bform know-form" onSubmit={(e) => { e.preventDefault(); create(); }}>
          <label>
            What are you working on?
            <input autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="Name your project" />
          </label>
          <label>
            What are you trying to achieve?
            <textarea
              rows={4}
              value={goal}
              onChange={(e) => setGoal(e.target.value)}
              placeholder="Describe your project, goals, subject, and anything Jarvis should keep in mind. Jarvis reads this in every chat in the project."
            />
          </label>
          {project ? <p className="muted small">Files folder: {project.folder}</p> : <p className="muted small">A folder for this project's files is created for you inside your research folder.</p>}
          <div className="bactions">
            {project && (
              <button
                type="button"
                className="mini danger"
                onClick={() => invoke("delete_workspace", { slug: project.slug }).then(() => onSaved(project)).catch((e) => setError(String(e)))}
              >
                Delete project
              </button>
            )}
            <span className="grow" />
            <button type="button" className="mini" onClick={onClose}>Cancel</button>
            <button className="btn primary" disabled={!name.trim() || busy}>{busy ? "Saving…" : project ? "Save" : "Create project"}</button>
          </div>
        </form>
      </div>
    </section>
  );
}
