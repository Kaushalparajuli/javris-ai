import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { confirm } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { day } from "../lib/format";
import type { KnowHow, Routine, Task } from "../lib/types";

type Form = { slug: string; name: string; description: string; body: string; draft: boolean; files: string[] };
const blank: Form = { slug: "", name: "", description: "", body: "", draft: false, files: [] };

/** Things Jarvis knows how to do (skills): a list, and a plain editor for each one. */
export default function KnowHowPage({ tasks, onClose }: { tasks: Task[]; onClose: () => void }) {
  const [list, setList] = useState<KnowHow[]>([]);
  const [routines, setRoutines] = useState<Routine[]>([]);
  const [form, setForm] = useState<Form | null>(null);
  const [error, setError] = useState("");

  const load = () => {
    invoke<KnowHow[]>("list_know_how").then(setList).catch((e) => setError(String(e)));
    invoke<Routine[]>("list_routines").then(setRoutines).catch(() => {});
  };
  useEffect(() => {
    load();
    const a = listen("know-how-changed", load);
    const b = listen("routines-changed", load);
    return () => {
      a.then((f) => f());
      b.then((f) => f());
    };
  }, []);
  // A draft Jarvis just finished writing shows up here.
  const writing = tasks.filter((t) => t.kind === "skill" && t.status === "running");
  const finished = tasks.filter((t) => t.kind === "skill" && t.status !== "running").length;
  useEffect(load, [writing.length, finished]);

  const usedBy = (slug: string) => routines.filter((r) => r.steps.some((s) => s.knowHow.includes(slug))).map((r) => r.title);

  const save = async () => {
    if (!form) return;
    try {
      await invoke<KnowHow>("save_know_how", { slug: form.slug || null, name: form.name, description: form.description, body: form.body });
      setForm(null);
      setError("");
      load();
    } catch (e) {
      setError(String(e));
    }
  };
  const remove = async (k: Form) => {
    const users = usedBy(k.slug);
    const also = users.length ? `\n\n${users.join(", ")} will stop using it.` : "";
    if (!(await confirm(`Delete “${k.name}”?${also}`, { title: "Delete know-how", kind: "warning", okLabel: "Delete" }))) return;
    await invoke("delete_know_how", { slug: k.slug }).catch((e) => setError(String(e)));
    setForm(null);
    load();
  };
  const set = <K extends keyof Form>(k: K, v: Form[K]) => setForm((f) => (f ? { ...f, [k]: v } : f));

  return (
    <section className="libpage knowpage">
      <header>
        <button className="icon-btn back" onClick={form ? () => setForm(null) : onClose} aria-label="Back" title="Back">
          <svg viewBox="0 0 24 24">
            <path d="M15.4 4.6 13.9 3.2 6.1 11l7.8 7.8 1.5-1.4L9 11z" />
          </svg>
        </button>
        <div>
          <h2>{form ? form.name || "New know-how" : "Know-how"}</h2>
          <p className="muted small">{form ? "Steps Jarvis follows whenever this kind of work comes up" : "Things Jarvis knows how to do. Routines and research follow them."}</p>
        </div>
        {!form && (
          <button className="btn primary" onClick={() => setForm({ ...blank })}>
            New know-how
          </button>
        )}
      </header>

      <div className="rt-body">
        {error && (
          <div className="banner" role="alert">
            <span>{error}</span>
            <div className="actions">
              <button className="mini" onClick={() => setError("")}>
                Dismiss
              </button>
            </div>
          </div>
        )}

        {form ? (
          <form
            className="bform know-form"
            onSubmit={(e) => {
              e.preventDefault();
              save();
            }}
          >
            {form.draft && <p className="draft-note">Jarvis wrote this from work that went well. Read it over, change anything, then save it to start using it.</p>}
            <label>
              Name
              <input id="kh-name" value={form.name} onChange={(e) => set("name", e.target.value)} placeholder="e.g. Compare competitors" autoFocus={!form.slug} />
            </label>
            <label>
              When to use it
              <input id="kh-desc" value={form.description} onChange={(e) => set("description", e.target.value)} placeholder="e.g. Comparing prices and features of several companies" />
            </label>
            <label>
              Steps to follow
              <textarea
                id="kh-body"
                rows={14}
                value={form.body}
                onChange={(e) => set("body", e.target.value)}
                placeholder={"1. Find 4–6 direct competitors. Prefer each company's own pricing page.\n2. For each one, note plan names, monthly price and free tier.\n3. Put it in one table. Leave a cell blank rather than guess."}
              />
            </label>
            {form.files.length > 0 && (
              <p className="muted small">
                Also in this know-how: {form.files.join(", ")}
              </p>
            )}
            <div className="bactions">
              {form.slug && (
                <button type="button" className="mini danger" onClick={() => remove(form)}>
                  Delete
                </button>
              )}
              <span className="grow" />
              <button type="button" className="mini" onClick={() => setForm(null)}>
                Cancel
              </button>
              <button type="submit" className="btn primary" disabled={!form.name.trim() || !form.body.trim()}>
                {form.draft ? "Looks good, save it" : "Save"}
              </button>
            </div>
          </form>
        ) : (
          <>
            {writing.map((t) => (
              <div key={t.id} className="kh-card writing">
                <b>{t.title.replace(/^Learning: /, "")}</b>
                <p>Jarvis is writing down how it did this…</p>
              </div>
            ))}
            {list.length === 0 && writing.length === 0 && (
              <div className="empty">
                <p>Nothing here yet. After a routine or a piece of research goes well, Jarvis offers to remember how it did it. You can also say:</p>
                <ul>
                  <li>“Remember how you did that competitor report.”</li>
                  <li>“Save that as know-how called Compare competitors.”</li>
                </ul>
              </div>
            )}
            <div className="kh-grid">
              {list.map((k) => {
                const users = usedBy(k.slug);
                return (
                  <button key={k.slug} className={`kh-card ${k.draft ? "draft" : ""}`} onClick={() => setForm({ ...k })}>
                    <span className="kh-top">
                      <b>{k.name}</b>
                      {k.draft && <span className="pill running">Needs review</span>}
                    </span>
                    <p>{k.description || "No description yet."}</p>
                    <span className="kh-foot">
                      <span>{users.length ? `Used by ${users.length} routine${users.length > 1 ? "s" : ""}` : "Not in a routine yet"}</span>
                      <span>{day(k.updatedAt)}</span>
                    </span>
                  </button>
                );
              })}
            </div>
          </>
        )}
      </div>
    </section>
  );
}
