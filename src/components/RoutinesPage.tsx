import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { confirm } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { WEEKDAYS, when } from "../lib/briefings";
import { blankRoutine, KIND_NAMES, LEVELS, minutesFor, planRoutine, routineWhen, STEP_INFO, TEMPLATES, type Template } from "../lib/routines";
import type { KnowHow, Routine, RoutineStep, Run, StepKind, Task } from "../lib/types";

const ICONS: Record<Template["icon"] | "routine", string> = {
  chart: "M3 19 9 11l4 4 5-8 3 5v7z",
  mail: "M3 5h18v14H3zm2 2v.4l7 5.1 7-5.1V7z",
  calendar: "M7 2h2v2h6V2h2v2h3v17H4V4h3zm-1 7v10h12V9z",
  tag: "M12 2h9v9l-10 10-9-9zm4.5 3a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3z",
  news: "M4 4h16v16H4zm2 2v4h12V6zm0 6v2h12v-2zm0 4v2h8v-2z",
  routine: "M12 3a9 9 0 1 0 9 9h-2a7 7 0 1 1-2.1-5L14 10h7V3l-2.6 2.6A9 9 0 0 0 12 3z",
};

function Icon({ d }: { d: string }) {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d={d} />
    </svg>
  );
}

const iconFor = (r: Routine) => {
  const kinds = r.steps.map((s) => s.kind);
  if (kinds.includes("inbox")) return ICONS.mail;
  if (kinds.includes("calendar")) return ICONS.calendar;
  return ICONS.routine;
};

/** Routines: say what you want or pick a ready-made one, check the plan, try it once, turn it on. */
export default function RoutinesPage({
  chatId,
  apiKey,
  tasks,
  focusId,
  onFocused,
  onOpenTask,
  onClose,
}: {
  chatId: string;
  apiKey: string;
  tasks: Task[];
  focusId: string | null;
  onFocused: () => void;
  onOpenTask: (id: number) => void;
  onClose: () => void;
}) {
  const [routines, setRoutines] = useState<Routine[]>([]);
  const [runs, setRuns] = useState<Run[]>([]);
  const [knowHow, setKnowHow] = useState<KnowHow[]>([]);
  const [openId, setOpenId] = useState<string | null>(null);
  const [error, setError] = useState("");

  const loadRoutines = () => invoke<Routine[]>("list_routines").then(setRoutines).catch((e) => setError(String(e)));
  const loadRuns = () => invoke<Run[]>("list_runs", { routineId: null }).then(setRuns).catch(() => {});
  const loadKnow = () => invoke<KnowHow[]>("list_know_how").then(setKnowHow).catch(() => {});
  useEffect(() => {
    loadRoutines();
    loadRuns();
    loadKnow();
    const a = listen("routines-changed", loadRoutines);
    const b = listen<Run>("routine-run", (e) => setRuns((list) => [e.payload, ...list.filter((r) => r.id !== e.payload.id)].sort((x, y) => y.startedAt - x.startedAt)));
    const c = listen("know-how-changed", loadKnow);
    return () => {
      a.then((f) => f());
      b.then((f) => f());
      c.then((f) => f());
    };
  }, []);

  // The voice made or changed a routine: show it.
  useEffect(() => {
    if (!focusId) return;
    setOpenId(focusId);
    loadRoutines();
    onFocused();
  }, [focusId]);

  const create = async (fields: Partial<Routine>) => {
    const r = await invoke<Routine>("save_routine", { routine: { ...blankRoutine(chatId), ...fields } });
    await loadRoutines();
    setOpenId(r.id);
  };

  const open = routines.find((r) => r.id === openId);

  return (
    <section className="libpage routinespage">
      <header>
        <button className="icon-btn back" onClick={open ? () => setOpenId(null) : onClose} aria-label="Back" title="Back">
          <svg viewBox="0 0 24 24">
            <path d="M15.4 4.6 13.9 3.2 6.1 11l7.8 7.8 1.5-1.4L9 11z" />
          </svg>
        </button>
        <div>
          <h2>{open ? open.title : "Routines"}</h2>
          <p className="muted small">{open ? routineWhen(open) : "Work Jarvis does for you, on a schedule or whenever you ask"}</p>
        </div>
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
        {open ? (
          <RoutineView
            key={open.id}
            routine={open}
            runs={runs.filter((r) => r.routineId === open.id)}
            knowHow={knowHow}
            tasks={tasks}
            apiKey={apiKey}
            onError={setError}
            onOpenTask={onOpenTask}
            onDeleted={() => {
              setOpenId(null);
              loadRoutines();
              loadRuns();
            }}
          />
        ) : (
          <Home routines={routines} runs={runs} knowHow={knowHow} apiKey={apiKey} onOpen={setOpenId} onCreate={create} onError={setError} />
        )}
      </div>
    </section>
  );
}

// ---------- home: describe, your routines, templates ----------

function Home({
  routines,
  runs,
  knowHow,
  apiKey,
  onOpen,
  onCreate,
  onError,
}: {
  routines: Routine[];
  runs: Run[];
  knowHow: KnowHow[];
  apiKey: string;
  onOpen: (id: string) => void;
  onCreate: (r: Partial<Routine>) => Promise<void>;
  onError: (e: string) => void;
}) {
  const [ask, setAsk] = useState("");
  const [planning, setPlanning] = useState(false);
  const [question, setQuestion] = useState("");
  const [answer, setAnswer] = useState("");
  const [tpl, setTpl] = useState<Template | null>(null);
  const [tplAnswer, setTplAnswer] = useState("");

  const plan = async (request: string) => {
    setPlanning(true);
    onError("");
    try {
      const p = await planRoutine(apiKey, request, knowHow);
      if (p.question && !question) {
        setQuestion(p.question);
      } else {
        await onCreate({ ...p.routine, request });
        setAsk("");
        setQuestion("");
        setAnswer("");
      }
    } catch (e) {
      onError(String(e instanceof Error ? e.message : e));
    }
    setPlanning(false);
  };

  const applyTemplate = async (t: Template, a = "") => {
    try {
      await onCreate(t.make(a.trim()));
      setTpl(null);
      setTplAnswer("");
    } catch (e) {
      onError(String(e));
    }
  };

  return (
    <>
      <form
        className="rt-ask-wrap"
        onSubmit={(e) => {
          e.preventDefault();
          if (question) plan(`${ask.trim()}\n(${question} ${answer.trim()})`);
          else if (ask.trim()) plan(ask.trim());
        }}
      >
        <div className="hello">
          <h3>What should Jarvis do for you?</h3>
          <p>Say it the way you'd ask a person. Jarvis works out the steps. You can also just tell Jarvis out loud.</p>
        </div>
        <div className="rt-ask">
          <input
            id="rt-ask"
            value={ask}
            onChange={(e) => {
              setAsk(e.target.value);
              setQuestion("");
            }}
            placeholder="e.g. Every Monday morning, check my competitors and email me a short summary"
            disabled={planning}
          />
          <button className="btn primary" type="submit" disabled={planning || !ask.trim() || (!!question && !answer.trim())}>
            {planning ? "Planning…" : question ? "Continue" : "Plan it"}
          </button>
        </div>
        {question && (
          <label className="rt-question">
            {question}
            <input id="rt-answer" autoFocus value={answer} onChange={(e) => setAnswer(e.target.value)} disabled={planning} />
          </label>
        )}
      </form>

      {routines.length > 0 && (
        <div className="rt-list">
          <div className="label">Your routines</div>
          {routines.map((r) => {
            const last = runs.find((x) => x.routineId === r.id);
            const busy = last && (last.status === "running" || last.status === "asking");
            const state = busy
              ? last!.status === "asking"
                ? { cls: "running", text: "Needs you" }
                : { cls: "running", text: "Running" }
              : r.pausedReason
                ? { cls: "failed", text: "Paused" }
                : r.enabled
                  ? { cls: "done", text: "On" }
                  : r.repeat === "manual"
                    ? { cls: "cancelled", text: "When asked" }
                    : r.tried
                      ? { cls: "cancelled", text: "Off" }
                      : { cls: "cancelled", text: "Not tried yet" };
            return (
              <button key={r.id} className={`rt-row ${r.pausedReason ? "paused" : ""}`} onClick={() => onOpen(r.id)}>
                <span className="rt-ic">
                  <Icon d={iconFor(r)} />
                </span>
                <span className="rt-txt">
                  <b>{r.title}</b>
                  <small className={r.pausedReason ? "err" : ""}>
                    {r.pausedReason ||
                      [routineWhen(r), r.enabled && r.nextRun ? `next ${when(r.nextRun)}` : "", last?.status === "done" ? `last result ${when(last.startedAt)}` : ""].filter(Boolean).join(" · ")}
                  </small>
                </span>
                <span className={`pill ${state.cls}`}>{state.text}</span>
              </button>
            );
          })}
        </div>
      )}

      <div className="label">{routines.length ? "Start from a ready-made routine" : "Or start from one of these"}</div>
      <div className="tpl-grid">
        {TEMPLATES.map((t) =>
          tpl?.id === t.id && t.ask ? (
            <form
              key={t.id}
              className="tpl open"
              onSubmit={(e) => {
                e.preventDefault();
                if (tplAnswer.trim()) applyTemplate(t, tplAnswer);
              }}
            >
              <b>{t.title}</b>
              <label>
                {t.ask.question}
                <input id={`tpl-${t.id}`} autoFocus value={tplAnswer} onChange={(e) => setTplAnswer(e.target.value)} placeholder={t.ask.placeholder} />
              </label>
              <div className="row">
                <button type="submit" className="mini primary" disabled={!tplAnswer.trim()}>
                  Make it
                </button>
                <button type="button" className="mini" onClick={() => setTpl(null)}>
                  Cancel
                </button>
              </div>
            </form>
          ) : (
            <button key={t.id} className="tpl" onClick={() => (t.ask ? (setTpl(t), setTplAnswer("")) : applyTemplate(t))}>
              <span className="ic">
                <Icon d={ICONS[t.icon]} />
              </span>
              <b>{t.title}</b>
              <small>{t.blurb}</small>
              <span className="q">{t.ask ? `Asks: ${t.ask.question.replace(/\?$/, "").toLowerCase()}` : "Asks: nothing"}</span>
            </button>
          ),
        )}
      </div>
    </>
  );
}

// ---------- one routine: the plan, trying it, its latest run ----------

function RoutineView({
  routine: r,
  runs,
  knowHow,
  tasks,
  apiKey,
  onError,
  onOpenTask,
  onDeleted,
}: {
  routine: Routine;
  runs: Run[];
  knowHow: KnowHow[];
  tasks: Task[];
  apiKey: string;
  onError: (e: string) => void;
  onOpenTask: (id: number) => void;
  onDeleted: () => void;
}) {
  const [changing, setChanging] = useState(false);
  const [change, setChange] = useState("");
  const [planning, setPlanning] = useState(false);
  const [editing, setEditing] = useState<Routine | null>(null);
  const [runId, setRunId] = useState<string | null>(null);
  const run = runs.find((x) => x.id === runId) ?? runs[0];
  const busy = runs.some((x) => x.status === "running" || x.status === "asking");
  const waiting = runs.some((x) => x.status === "asking");

  const save = async (next: Routine) => {
    try {
      await invoke<Routine>("save_routine", { routine: next });
      onError("");
      return true;
    } catch (e) {
      onError(String(e));
      return false;
    }
  };
  const start = async () => {
    try {
      const x = await invoke<Run>("run_routine", { id: r.id });
      setRunId(x.id);
      onError("");
    } catch (e) {
      onError(String(e));
    }
  };
  const revise = async () => {
    if (!change.trim()) return;
    setPlanning(true);
    try {
      const p = await planRoutine(apiKey, change.trim(), knowHow, r);
      if (await save({ ...r, ...p.routine, title: p.routine.title || r.title })) {
        setChange("");
        setChanging(false);
      }
    } catch (e) {
      onError(String(e instanceof Error ? e.message : e));
    }
    setPlanning(false);
  };
  const remove = async () => {
    if (!(await confirm(`Delete the “${r.title}” routine? Its past results stay in your Library.`, { title: "Delete routine", kind: "warning", okLabel: "Delete" }))) return;
    await invoke("delete_routine", { id: r.id }).catch((e) => onError(String(e)));
    onDeleted();
  };

  if (editing) return <FineTune draft={editing} knowHow={knowHow} onChange={setEditing} onCancel={() => setEditing(null)} onSave={async () => (await save(editing)) && setEditing(null)} />;

  const knowName = (slug: string) => knowHow.find((k) => k.slug === slug)?.name ?? slug;

  return (
    <>
      {r.pausedReason && (
        <div className="rt-paused" role="status">
          <span>{r.pausedReason}</span>
          <button className="mini" onClick={start} disabled={busy}>
            Try again now
          </button>
        </div>
      )}

      <div className="recipe">
        <span className="when">{r.repeat === "manual" ? "Whenever you ask, I'll:" : `${routineWhen(r)}, I'll:`}</span>
        <ol>
          {r.steps.map((s, i) => (
            <li key={i}>
              <span className="n">{i + 1}</span>
              <div>
                <p>
                  {s.kind === "email_me" && !r.autoSend ? (
                    <>
                      {/[.!?]$/.test(s.text) ? s.text : `${s.text}.`} <b>I'll ask you before sending.</b>
                    </>
                  ) : (
                    s.text
                  )}
                </p>
                <small>
                  <i className={`lvl ${STEP_INFO[s.kind].level}`} />
                  {s.kind === "email_me" && r.autoSend ? "Emails you without asking" : STEP_INFO[s.kind].label}
                  {s.knowHow.length > 0 && <span className="kh-use"> · follows {s.knowHow.map(knowName).join(", ")}</span>}
                </small>
              </div>
            </li>
          ))}
        </ol>
        <div className="facts">
          <span>
            Takes about <b>{minutesFor(r)} minute{minutesFor(r) === 1 ? "" : "s"}</b>
          </span>
          <span>
            Detail <b>{r.depth === "deep" ? "Thorough" : "Quick"}</b>
          </span>
          {r.enabled && r.nextRun && (
            <span>
              Next <b>{when(r.nextRun)}</b>
            </span>
          )}
        </div>

        {changing ? (
          <form
            className="rt-change"
            onSubmit={(e) => {
              e.preventDefault();
              revise();
            }}
          >
            <input id="rt-change" autoFocus value={change} onChange={(e) => setChange(e.target.value)} placeholder="Tell Jarvis what to change, e.g. “make it Fridays” or “add Milvus”" disabled={planning} />
            <button className="btn primary" type="submit" disabled={planning || !change.trim()}>
              {planning ? "Changing…" : "Change it"}
            </button>
            <button className="mini" type="button" onClick={() => setChanging(false)}>
              Cancel
            </button>
          </form>
        ) : (
          <div className="acts">
            {!r.tried ? (
              <>
                <button className="btn primary" onClick={start} disabled={busy}>
                  {waiting ? "Waiting for your OK" : busy ? "Trying it…" : "Try it once now"}
                </button>
                <button className="mini" onClick={() => setChanging(true)}>
                  Change something
                </button>
                <span className="hint">{r.repeat === "manual" ? "See a real result first." : "You can turn it on after the first try."}</span>
              </>
            ) : (
              <>
                {r.repeat !== "manual" && (
                  <label className="rt-on">
                    <span className="switch">
                      <input type="checkbox" checked={r.enabled} onChange={() => save({ ...r, enabled: !r.enabled })} />
                      <i />
                    </span>
                    {r.enabled ? "On" : "Off"}
                  </label>
                )}
                <button className="btn primary" onClick={start} disabled={busy}>
                  {waiting ? "Waiting for your OK" : busy ? "Running…" : "Run it now"}
                </button>
                <button className="mini" onClick={() => setChanging(true)}>
                  Change something
                </button>
              </>
            )}
            <span className="grow" />
            <button className="mini" onClick={() => setEditing({ ...r, steps: r.steps.map((s) => ({ ...s, knowHow: [...s.knowHow] })) })}>
              Fine-tune
            </button>
            <button className="mini danger" onClick={remove}>
              Delete
            </button>
          </div>
        )}
      </div>

      <div className="legend">
        {LEVELS.map((l) => (
          <span key={l.level}>
            <i className={`lvl ${l.level}`} />
            {l.text}
          </span>
        ))}
      </div>

      {run && <RunView run={run} routine={r} firstDone={run.status === "done" && runs.filter((x) => x.status === "done").length === 1} tasks={tasks} onError={onError} onOpenTask={onOpenTask} onRunAgain={start} onTurnOn={() => save({ ...r, enabled: true })} />}

      {runs.length > 1 && (
        <div className="rt-history">
          <div className="label">Earlier results</div>
          {runs.slice(0, 8).map((x) => (
            <button key={x.id} className={x.id === run?.id ? "on" : ""} onClick={() => setRunId(x.id)}>
              <span>Run {x.number}</span>
              <span>{when(x.startedAt)}</span>
              <span className={`pill ${x.status === "asking" ? "running" : x.status}`}>{RUN_LABEL[x.status]}</span>
            </button>
          ))}
        </div>
      )}
    </>
  );
}

const RUN_LABEL: Record<Run["status"], string> = { running: "Running", asking: "Needs you", done: "Done", failed: "Didn't finish", cancelled: "Stopped" };

/** What a running step is doing right now, in a few words. */
function nowDoing(run: Run, tasks: Task[]) {
  const i = run.steps.findIndex((s) => s.status === "running");
  if (i < 0) return "Getting started…";
  const s = run.steps[i];
  const t = s.taskId != null ? tasks.find((x) => x.id === s.taskId) : undefined;
  const live = t?.steps[t.steps.length - 1]?.label;
  const what = s.kind === "inbox" ? "Reading your email" : s.kind === "calendar" ? "Checking your calendar" : s.kind === "email_me" ? "Preparing the email" : live ?? "Starting";
  return `Step ${i + 1} of ${run.steps.length}: ${what}…`;
}

function RunView({
  run,
  routine,
  firstDone,
  tasks,
  onError,
  onOpenTask,
  onRunAgain,
  onTurnOn,
}: {
  run: Run;
  routine: Routine;
  firstDone: boolean;
  tasks: Task[];
  onError: (e: string) => void;
  onOpenTask: (id: number) => void;
  onRunAgain: () => void;
  onTurnOn: () => void;
}) {
  const [always, setAlways] = useState(false);
  const [answering, setAnswering] = useState(false);
  const [remember, setRemember] = useState<"ask" | "writing" | "no">("ask");
  const done = run.steps.filter((s) => s.status === "done" || s.status === "skipped").length;
  const pct = run.status === "running" ? Math.max(6, Math.round((done / run.steps.length) * 100)) : 100;

  const answer = async (send: boolean) => {
    setAnswering(true);
    try {
      await invoke<Run>("answer_run", { id: run.id, send, always: send && always });
    } catch (e) {
      onError(String(e));
    }
    setAnswering(false);
  };
  const learn = async () => {
    try {
      await invoke<Task>("learn_know_how", { name: routine.title, runId: run.id, taskId: null, chatId: routine.chatId });
      setRemember("writing");
    } catch (e) {
      onError(String(e));
    }
  };
  const firstTry = firstDone && !routine.enabled && routine.repeat !== "manual" && !routine.pausedReason;

  return (
    <div className="rt-run">
      <div className="friendly">
        <span className="line">
          {run.status === "running"
            ? nowDoing(run, tasks)
            : run.status === "asking"
              ? `Almost done. Step ${run.steps.length} of ${run.steps.length} is waiting for you.`
              : run.status === "done"
                ? `Finished ${when(run.finishedAt)}.`
                : run.status === "cancelled"
                  ? "Stopped."
                  : "This run didn't finish."}
        </span>
        <div className={`rt-bar ${run.status}`}>
          <i style={{ width: `${pct}%` }} />
        </div>
      </div>

      {run.status === "failed" && (
        <div className="rt-failed">
          <p>{run.error || "Something went wrong."}</p>
          <button className="mini" onClick={onRunAgain}>
            Try again
          </button>
        </div>
      )}

      {run.resultTask != null && (run.status === "done" || run.status === "asking") && (
        <div className="rt-result">
          <h4>Your result is ready</h4>
          {run.summary && <p>{run.summary}</p>}
          <div className="row">
            <button className="btn primary" onClick={() => onOpenTask(run.resultTask!)}>
              Open it
            </button>
          </div>
        </div>
      )}

      {run.status === "asking" && run.approval && (
        <div className="rt-approve">
          <p>
            Jarvis wants to email you this. Send it to <b>{run.approval.to}</b>?
          </p>
          <div className="mail">
            <div className="hd">
              Subject <b>{run.approval.subject}</b>
            </div>
            <div className="bd">{run.approval.body.length > 700 ? `${run.approval.body.slice(0, 700)}…` : run.approval.body}</div>
          </div>
          <div className="row">
            <button className="btn gold" onClick={() => answer(true)} disabled={answering}>
              Send
            </button>
            <button className="mini" onClick={() => answer(false)} disabled={answering}>
              Don't send
            </button>
          </div>
          <label className="check">
            <input id="always-send" type="checkbox" checked={always} onChange={(e) => setAlways(e.target.checked)} />
            Don't ask again for this routine (it only ever emails you)
          </label>
        </div>
      )}

      {firstTry && (
        <div className="rt-turnon">
          <span>That was the first try. Want Jarvis to do this {routineWhen(routine).toLowerCase()}?</span>
          <button className="btn primary" onClick={onTurnOn}>
            Turn it on
          </button>
        </div>
      )}

      {run.status === "done" && run.resultTask != null && remember !== "no" && (
        <div className="rt-remember">
          {remember === "ask" ? (
            <>
              <p>That went well. Want me to remember how I did it, so similar work is faster and more consistent next time?</p>
              <div className="row">
                <button className="mini primary" onClick={learn}>
                  Yes, remember
                </button>
                <button className="mini" onClick={() => setRemember("no")}>
                  No thanks
                </button>
              </div>
            </>
          ) : (
            <p className="resolved">Writing it down. You can look it over on the Know-how page.</p>
          )}
        </div>
      )}

      <details className="more">
        <summary>Details</summary>
        <ul className="rt-steps">
          {run.steps.map((s, i) => (
            <li key={i} className={s.status}>
              <i />
              <div>
                <b>{s.text}</b>
                {s.note && <small>{s.note}</small>}
              </div>
              {s.taskId != null && (s.status === "done" || s.status === "failed") && (
                <button className="mini" onClick={() => onOpenTask(s.taskId!)}>
                  Open
                </button>
              )}
            </li>
          ))}
        </ul>
        {(run.status === "running" || run.status === "asking") && (
          <button className="mini" onClick={() => invoke("cancel_run", { id: run.id }).catch((e) => onError(String(e)))}>
            Stop this run
          </button>
        )}
      </details>
    </div>
  );
}

// ---------- fine-tune: every setting and step, for people who want them ----------

function FineTune({
  draft: d,
  knowHow,
  onChange,
  onCancel,
  onSave,
}: {
  draft: Routine;
  knowHow: KnowHow[];
  onChange: (r: Routine) => void;
  onCancel: () => void;
  onSave: () => void;
}) {
  const set = <K extends keyof Routine>(k: K, v: Routine[K]) => onChange({ ...d, [k]: v });
  const setStep = (i: number, s: Partial<RoutineStep>) => set("steps", d.steps.map((x, j) => (j === i ? { ...x, ...s } : x)));
  const move = (i: number, by: number) => {
    const steps = [...d.steps];
    const [s] = steps.splice(i, 1);
    steps.splice(i + by, 0, s);
    set("steps", steps);
  };
  const usable = knowHow.filter((k) => !k.draft);

  return (
    <form
      className="bform finetune"
      onSubmit={(e) => {
        e.preventDefault();
        onSave();
      }}
    >
      <div className="label">Fine-tune</div>
      <label>
        Name
        <input id="ft-title" value={d.title} onChange={(e) => set("title", e.target.value)} />
      </label>
      <div className="brow">
        <div>
          <span className="blabel">Runs</span>
          <div className="seg" role="group" aria-label="Runs">
            {(["manual", "daily", "weekdays", "weekly"] as const).map((x) => (
              <button type="button" key={x} className={d.repeat === x ? "on" : ""} onClick={() => set("repeat", x)}>
                {x === "manual" ? "When I ask" : x === "daily" ? "Every day" : x === "weekdays" ? "Weekdays" : "Weekly"}
              </button>
            ))}
          </div>
        </div>
        {d.repeat === "weekly" && (
          <label>
            Day
            <select id="ft-day" value={d.weekday} onChange={(e) => set("weekday", Number(e.target.value))}>
              {WEEKDAYS.map((w, i) => (
                <option key={w} value={i}>
                  {w}
                </option>
              ))}
            </select>
          </label>
        )}
        {d.repeat !== "manual" && (
          <label>
            Time
            <input id="ft-time" type="time" value={d.time} onChange={(e) => set("time", e.target.value)} />
          </label>
        )}
        <div>
          <span className="blabel">Detail</span>
          <div className="seg" role="group" aria-label="Detail">
            <button type="button" className={d.depth === "quick" ? "on" : ""} onClick={() => set("depth", "quick")}>
              Quick
            </button>
            <button type="button" className={d.depth === "deep" ? "on" : ""} onClick={() => set("depth", "deep")}>
              Thorough
            </button>
          </div>
        </div>
      </div>

      <div className="ft-steps">
        {d.steps.map((s, i) => (
          <div key={i} className="ft-step">
            <div className="ft-head">
              <span className="n">{i + 1}</span>
              <select id={`ft-kind-${i}`} value={s.kind} onChange={(e) => setStep(i, { kind: e.target.value as StepKind })} aria-label={`Step ${i + 1} kind`}>
                {(Object.keys(KIND_NAMES) as StepKind[]).map((k) => (
                  <option key={k} value={k}>
                    {KIND_NAMES[k]}
                  </option>
                ))}
              </select>
              <span className="grow" />
              <button type="button" className="mini" onClick={() => move(i, -1)} disabled={i === 0} aria-label="Move up">
                ↑
              </button>
              <button type="button" className="mini" onClick={() => move(i, 1)} disabled={i === d.steps.length - 1} aria-label="Move down">
                ↓
              </button>
              <button type="button" className="mini" onClick={() => set("steps", d.steps.filter((_, j) => j !== i))} aria-label="Remove step">
                Remove
              </button>
            </div>
            <textarea id={`ft-text-${i}`} rows={2} value={s.text} onChange={(e) => setStep(i, { text: e.target.value })} placeholder="What should this step do?" />
            {usable.length > 0 && (s.kind === "research" || s.kind === "write") && (
              <div className="ft-know">
                <span className="blabel">Follows</span>
                {usable.map((k) => {
                  const on = s.knowHow.includes(k.slug);
                  return (
                    <button
                      type="button"
                      key={k.slug}
                      className={`kchip ${on ? "on" : ""}`}
                      aria-pressed={on}
                      onClick={() => setStep(i, { knowHow: on ? s.knowHow.filter((x) => x !== k.slug) : [...s.knowHow, k.slug] })}
                    >
                      {k.name}
                    </button>
                  );
                })}
              </div>
            )}
          </div>
        ))}
        <button type="button" className="mini" onClick={() => set("steps", [...d.steps, { kind: "research", text: "", knowHow: [] }])} disabled={d.steps.length >= 8}>
          + Add a step
        </button>
      </div>

      {d.steps.some((s) => s.kind === "email_me") && (
        <label className="check">
          <input id="ft-auto" type="checkbox" checked={d.autoSend} onChange={(e) => set("autoSend", e.target.checked)} />
          Send the email without asking me first (it only ever goes to you)
        </label>
      )}

      <div className="bactions">
        <button type="button" className="mini" onClick={onCancel}>
          Cancel
        </button>
        <button type="submit" className="btn primary" disabled={!d.steps.length || d.steps.some((s) => !s.text.trim())}>
          Save
        </button>
      </div>
    </form>
  );
}
