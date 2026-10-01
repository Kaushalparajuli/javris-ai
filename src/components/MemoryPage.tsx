import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import type { Memory, Workspace } from "../lib/types";

const KINDS = ["person", "project", "decision", "preference", "fact", "note"];
const blankWs: Workspace = { slug: "", name: "", description: "", folder: "", verify: [], url: "", created: 0 };

/** What Jarvis remembers, and the project workspaces it's sorted into. */
export default function MemoryPage({ onClose }: { onClose: () => void }) {
  const [tab, setTab] = useState<"memory" | "workspaces">("memory");
  const [mems, setMems] = useState<Memory[]>([]);
  const [spaces, setSpaces] = useState<Workspace[]>([]);
  const [active, setActive] = useState("");
  const [q, setQ] = useState("");
  const [filter, setFilter] = useState("all");
  const [text, setText] = useState("");
  const [kind, setKind] = useState("fact");
  const [form, setForm] = useState<Workspace | null>(null);
  const [verify, setVerify] = useState("");
  const [error, setError] = useState("");

  const loadSpaces = () => {
    invoke<Workspace[]>("list_workspaces").then(setSpaces).catch(() => {});
    invoke<Workspace | null>("get_active_workspace").then((w) => setActive(w?.slug ?? "")).catch(() => {});
  };
  const loadMems = () => {
    const ws = filter === "all" ? undefined : filter === "global" ? "" : filter;
    const run = q.trim()
      ? invoke<Memory[]>("memory_search", { query: q, workspace: ws, limit: 100 })
      : invoke<Memory[]>("memory_list", { workspace: ws, limit: 200 });
    run.then(setMems).catch((e) => setError(String(e)));
  };
  useEffect(loadSpaces, []);
  useEffect(loadMems, [q, filter, tab]);

  const add = async () => {
    if (!text.trim()) return;
    try {
      await invoke("memory_add", { kind, text, workspace: active || "", source: "user" });
      setText("");
      loadMems();
    } catch (e) {
      setError(String(e));
    }
  };
  const act = (p: Promise<unknown>) => p.then(loadMems).catch((e) => setError(String(e)));
  const nameOf = (slug: string) => (slug ? spaces.find((s) => s.slug === slug)?.name ?? slug : "Everywhere");

  const saveWs = async () => {
    if (!form) return;
    try {
      await invoke("save_workspace", { workspace: { ...form, verify: verify.split("\n").map((l) => l.trim()).filter(Boolean) } });
      setForm(null);
      loadSpaces();
    } catch (e) {
      setError(String(e));
    }
  };
  const pickFolder = async () => {
    const f = await openDialog({ directory: true });
    if (typeof f === "string" && form) setForm({ ...form, folder: f });
  };
  const choose = (slug: string) =>
    invoke("set_active_workspace", { slug })
      .then(() => {
        setActive(slug);
        window.dispatchEvent(new Event("jarvis-workspace"));
      })
      .catch((e) => setError(String(e)));

  return (
    <section className="libpage knowpage">
      <header className="lib-head">
        <button className="icon-btn back" onClick={form ? () => setForm(null) : onClose} aria-label="Back" title="Back">
          ←
        </button>
        <div>
          <h2>Memory</h2>
          <p className="muted small">What Jarvis remembers about you and your projects. Stored on this Mac.</p>
        </div>
        <div className="seg" style={{ marginLeft: "auto" }}>
          <button className={tab === "memory" ? "on" : ""} onClick={() => setTab("memory")}>Memories</button>
          <button className={tab === "workspaces" ? "on" : ""} onClick={() => setTab("workspaces")}>Workspaces</button>
        </div>
      </header>
      <div className="rt-body">
        {error && (
          <div className="banner" role="alert">
            <span>{error}</span>
            <button className="mini" onClick={() => setError("")}>Dismiss</button>
          </div>
        )}
        {tab === "memory" && (
          <>
            <div className="mem-bar">
              <input placeholder="Search memories" value={q} onChange={(e) => setQ(e.target.value)} />
              <select value={filter} onChange={(e) => setFilter(e.target.value)}>
                <option value="all">All</option>
                <option value="global">Everywhere</option>
                {spaces.map((s) => (
                  <option key={s.slug} value={s.slug}>{s.name}</option>
                ))}
              </select>
            </div>
            <form className="mem-add" onSubmit={(e) => { e.preventDefault(); add(); }}>
              <input placeholder={`Tell Jarvis something to remember${active ? ` (in ${nameOf(active)})` : ""}`} value={text} onChange={(e) => setText(e.target.value)} />
              <select value={kind} onChange={(e) => setKind(e.target.value)}>
                {KINDS.map((k) => <option key={k}>{k}</option>)}
              </select>
              <button className="btn primary" disabled={!text.trim()}>Remember</button>
            </form>
            {mems.length === 0 && <p className="muted">Nothing here yet. Say "remember that…" to Jarvis, or add something above.</p>}
            {mems.map((m) => (
              <div key={m.id} className="kh-card mem">
                <div className="grow">
                  <div>{m.text}</div>
                  <div className="muted small">{m.kind} · {nameOf(m.workspace)} · {new Date(m.created * 1000).toLocaleDateString()}</div>
                </div>
                <button className="mini" onClick={() => act(invoke("memory_update", { id: m.id, pinned: !m.pinned }))}>{m.pinned ? "Unpin" : "Pin"}</button>
                <button className="mini danger" onClick={() => act(invoke("memory_delete", { id: m.id }))}>Forget</button>
              </div>
            ))}
          </>
        )}
        {tab === "workspaces" && !form && (
          <>
            <p className="muted small">A workspace is a project (Fikra, GSoft, Personal). Jarvis files memories under the active one, and code fixes use its folder.</p>
            <button className={`kh-card ws-none ${active === "" ? "on" : ""}`} onClick={() => choose("")}>No workspace</button>
            {spaces.map((w) => (
              <div key={w.slug} className={`kh-card ${active === w.slug ? "on" : ""}`}>
                <div className="grow">
                  <strong>{w.name}</strong> {active === w.slug && <span className="muted small">· active</span>}
                  <div className="muted small">{w.description || "No description"}{w.folder ? ` · ${w.folder}` : ""}</div>
                </div>
                {active !== w.slug && <button className="mini" onClick={() => choose(w.slug)}>Use</button>}
                <button className="mini" onClick={() => { setForm(w); setVerify(w.verify.join("\n")); }}>Edit</button>
              </div>
            ))}
            <button className="btn primary" onClick={() => { setForm({ ...blankWs }); setVerify(""); }}>New workspace</button>
          </>
        )}
        {tab === "workspaces" && form && (
          <form className="bform know-form" onSubmit={(e) => { e.preventDefault(); saveWs(); }}>
            <label>Name<input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} placeholder="Fikra" /></label>
            <label>What it is<textarea rows={2} value={form.description} onChange={(e) => setForm({ ...form, description: e.target.value })} /></label>
            <label>Code folder (optional)
              <div className="row"><input value={form.folder} onChange={(e) => setForm({ ...form, folder: e.target.value })} /><button type="button" className="mini" onClick={pickFolder}>Choose…</button></div>
            </label>
            <label>Checks to run after changes (one command per line)
              <textarea rows={3} value={verify} onChange={(e) => setVerify(e.target.value)} placeholder={"npm run build\nnpm test"} />
            </label>
            <label>Web address to look at after a change (optional)<input value={form.url} onChange={(e) => setForm({ ...form, url: e.target.value })} placeholder="http://localhost:5173" /></label>
            <div className="bactions">
              {form.slug && <button type="button" className="mini danger" onClick={() => invoke("delete_workspace", { slug: form.slug }).then(() => { setForm(null); loadSpaces(); }).catch((e) => setError(String(e)))}>Delete</button>}
              <span className="grow" />
              <button type="button" className="mini" onClick={() => setForm(null)}>Cancel</button>
              <button className="btn primary" disabled={!form.name.trim()}>Save</button>
            </div>
          </form>
        )}
      </div>
    </section>
  );
}
