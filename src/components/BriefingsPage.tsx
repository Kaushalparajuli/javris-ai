import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { confirm } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { blankBriefing, scheduleText, WEEKDAYS, when } from "../lib/briefings";
import type { Briefing, Task } from "../lib/types";

/** Research that runs by itself on a schedule: the list, and a form to add or change one. */
export default function BriefingsPage({
  chatId,
  onOpenTask,
  onClose,
}: {
  chatId: string;
  onOpenTask: (id: number) => void;
  onClose: () => void;
}) {
  const [list, setList] = useState<Briefing[]>([]);
  const [form, setForm] = useState<Briefing | null>(null);
  const [error, setError] = useState("");

  const load = () => invoke<Briefing[]>("list_briefings").then(setList).catch((e) => setError(String(e)));
  useEffect(() => {
    load();
    // The scheduler updates last and next runs as briefings go out.
    const un = listen("briefings-changed", load);
    return () => {
      un.then((f) => f());
    };
  }, []);

  const save = async (b: Briefing) => {
    try {
      await invoke<Briefing>("save_briefing", { briefing: b });
      setForm(null);
      setError("");
      load();
    } catch (e) {
      setError(String(e));
    }
  };

  const remove = async (b: Briefing) => {
    if (!(await confirm(`Stop and remove the “${b.title}” briefing?`, { title: "Remove briefing", kind: "warning", okLabel: "Remove" }))) return;
    await invoke("delete_briefing", { id: b.id }).catch((e) => setError(String(e)));
    load();
  };

  const runNow = async (b: Briefing) => {
    try {
      const task = await invoke<Task>("run_briefing_now", { id: b.id });
      onOpenTask(task.id);
    } catch (e) {
      setError(String(e));
    }
  };

  const set = <K extends keyof Briefing>(k: K, v: Briefing[K]) => setForm((f) => (f ? { ...f, [k]: v } : f));

  return (
    <section className="libpage briefpage">
      <header>
        <button className="icon-btn back" onClick={onClose} aria-label="Back to the conversation" title="Back">
          <svg viewBox="0 0 24 24">
            <path d="M15.4 4.6 13.9 3.2 6.1 11l7.8 7.8 1.5-1.4L9 11z" />
          </svg>
        </button>
        <div>
          <h2>Briefings</h2>
          <p className="muted small">Research that runs by itself on a schedule</p>
        </div>
        <button className="btn primary" onClick={() => setForm(blankBriefing(chatId))} disabled={!!form}>
          New briefing
        </button>
      </header>

      <div className="brief-body">
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

        {form && (
          <form
            className="bform"
            onSubmit={(e) => {
              e.preventDefault();
              save(form);
            }}
          >
            <div className="label">{form.id ? "Change briefing" : "New briefing"}</div>
            <label>
              What should it cover?
              <textarea
                autoFocus
                rows={3}
                value={form.request}
                onChange={(e) => set("request", e.target.value)}
                placeholder="e.g. The day's AI and startup news in Nepal and India, with anything about funding rounds"
              />
            </label>
            <div className="brow">
              <label>
                Name
                <input value={form.title} onChange={(e) => set("title", e.target.value)} placeholder="e.g. Morning AI news" />
              </label>
              <label>
                Time
                <input type="time" value={form.time} onChange={(e) => set("time", e.target.value)} required />
              </label>
            </div>
            <div className="brow">
              <div>
                <span className="blabel">Repeat</span>
                <div className="seg" role="group" aria-label="Repeat">
                  {(["daily", "weekdays", "weekly"] as const).map((r) => (
                    <button type="button" key={r} className={form.repeat === r ? "on" : ""} onClick={() => set("repeat", r)}>
                      {r === "daily" ? "Every day" : r === "weekdays" ? "Weekdays" : "Weekly"}
                    </button>
                  ))}
                </div>
              </div>
              {form.repeat === "weekly" && (
                <label>
                  Day
                  <select value={form.weekday} onChange={(e) => set("weekday", Number(e.target.value))}>
                    {WEEKDAYS.map((d, i) => (
                      <option key={d} value={i}>
                        {d}
                      </option>
                    ))}
                  </select>
                </label>
              )}
              <div>
                <span className="blabel">Depth</span>
                <div className="seg" role="group" aria-label="Depth">
                  <button type="button" className={form.depth === "quick" ? "on" : ""} onClick={() => set("depth", "quick")}>
                    Quick
                  </button>
                  <button type="button" className={form.depth === "deep" ? "on" : ""} onClick={() => set("depth", "deep")}>
                    Deep dive
                  </button>
                </div>
              </div>
            </div>
            <div className="bactions">
              <button type="button" className="mini" onClick={() => setForm(null)}>
                Cancel
              </button>
              <button type="submit" className="btn primary" disabled={!form.request.trim()}>
                {form.id ? "Save" : "Schedule it"}
              </button>
            </div>
          </form>
        )}

        {list.length === 0 && !form && (
          <div className="empty">
            <p>No briefings yet. Add one here, or just say:</p>
            <ul>
              <li>“Every weekday at 9, brief me on AI news.”</li>
              <li>“On Mondays at 8, give me a deep dive on my competitors.”</li>
            </ul>
          </div>
        )}

        {list.map((b) => (
          <article key={b.id} className={`briefing ${b.enabled ? "" : "paused"}`}>
            <div className="btop">
              <h3>{b.title}</h3>
              <label className="switch" title={b.enabled ? "Pause" : "Resume"}>
                <input type="checkbox" checked={b.enabled} onChange={() => save({ ...b, enabled: !b.enabled })} />
                <i />
              </label>
            </div>
            <p className="breq">{b.request}</p>
            <div className="bmeta">
              <span>{scheduleText(b)}</span>
              <span>{b.depth === "deep" ? "Deep dive" : "Quick"}</span>
              <span>{b.enabled ? (b.nextRun ? `Next: ${when(b.nextRun)}` : "Not scheduled") : "Paused"}</span>
              {b.lastRun && <span>Last: {when(b.lastRun)}</span>}
            </div>
            {b.lastError && <p className="error-text small">Last run couldn't start: {b.lastError}</p>}
            <div className="actions">
              {b.lastTask != null && (
                <button className="mini" onClick={() => onOpenTask(b.lastTask!)}>
                  Open latest
                </button>
              )}
              <button className="mini" onClick={() => runNow(b)}>
                Run now
              </button>
              <button className="mini" onClick={() => setForm(b)}>
                Change
              </button>
              <button className="mini" onClick={() => remove(b)}>
                Remove
              </button>
            </div>
          </article>
        ))}

        <p className="muted small">Briefings run while Jarvis is open. Closing the window keeps it running in the menu bar. A run missed while Jarvis was closed happens once when it opens again.</p>
      </div>
    </section>
  );
}
