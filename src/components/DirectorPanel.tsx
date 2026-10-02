import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import type { DirectorState, Issue, Round, Shot } from "../lib/director";

const STEPS = [
  { id: "capturing", label: "Look" },
  { id: "reviewing", label: "Review" },
  { id: "fixing", label: "Fix" },
] as const;

function Thumb({ shot, onOpen }: { shot: Shot; onOpen: (src: string, caption: string) => void }) {
  const [src, setSrc] = useState("");
  useEffect(() => {
    let live = true;
    invoke<string>("director_shot", { path: shot.path })
      .then((b64) => live && setSrc(`data:image/jpeg;base64,${b64}`))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [shot.path]);
  const caption = `${shot.viewport} · screen ${shot.index} · ${shot.y}px down the page`;
  return (
    <button className={`dp-thumb ${shot.viewport}`} onClick={() => src && onOpen(src, caption)} title={caption} aria-label={`Open ${caption}`}>
      {src ? <img src={src} alt={caption} /> : <span className="dp-thumb-wait" />}
    </button>
  );
}

const scoreClass = (n: number) => (n >= 85 ? "good" : n >= 65 ? "ok" : "bad");

function IssueRow({ issue }: { issue: Issue }) {
  return (
    <li className={`dp-issue ${issue.severity}`}>
      <div className="dp-issue-head">
        <span className={`dp-sev ${issue.severity}`}>{issue.severity}</span>
        <span className="dp-where">
          {issue.viewport === "all" ? "all sizes" : issue.viewport} · {issue.area}
        </span>
        <span className="dp-kind">{issue.kind}</span>
      </div>
      <p className="dp-problem">{issue.problem}</p>
      <p className="dp-fix">
        <b>Fix</b> {issue.fix}
      </p>
    </li>
  );
}

/** The visual director's work on a site: what it looked at, what it found, what it fixed, what it learned. */
export default function DirectorPanel({ state, running, onReview }: { state?: DirectorState; running: boolean; onReview: () => void }) {
  const [roundNo, setRoundNo] = useState(0);
  const [view, setView] = useState<"desktop" | "tablet" | "phone">("desktop");
  const [big, setBig] = useState<{ src: string; caption: string } | null>(null);
  const busy = !!state && state.status !== "done" && state.status !== "error";

  // Follow the newest round while the director works.
  const latest = state?.rounds.length ?? 0;
  useEffect(() => {
    if (latest) setRoundNo(latest);
  }, [latest]);

  if (!state) {
    return (
      <div className="dp dp-empty">
        <h4>Visual director</h4>
        <p className="muted">
          After a website is built, Jarvis looks at it at desktop, tablet and phone sizes like a design director, lists what's off (alignment, spacing, cropping, contrast, mobile layout), has the code worker fix it, and looks again.
        </p>
        <button className="btn primary" onClick={onReview} disabled={running}>
          {running ? "Wait for the build to finish" : "Review this site now"}
        </button>
      </div>
    );
  }

  const round: Round | undefined = state.rounds.find((r) => r.n === roundNo) ?? state.rounds[state.rounds.length - 1];
  const review = round?.review;
  const first = state.rounds[0]?.review;
  const shots = (round?.shots ?? []).filter((s) => s.viewport === view);
  const active = STEPS.findIndex((s) => s.id === state.status);

  return (
    <div className="dp">
      <div className="dp-top">
        <div className="dp-steps" aria-label="Progress">
          {STEPS.map((s, i) => (
            <span key={s.id} className={`${state.status === s.id ? "now" : ""} ${state.status === "done" || (active > i && active !== -1) ? "done" : ""}`}>
              {s.label}
            </span>
          ))}
          <span className={state.status === "done" ? "done" : state.status === "error" ? "err" : ""}>{state.status === "error" ? "Stopped" : "Done"}</span>
        </div>
        <span className="grow" />
        <button className="mini" onClick={onReview} disabled={busy || running}>
          {busy ? "Working…" : "Review again"}
        </button>
      </div>
      <p className={`dp-message ${state.status === "error" ? "err" : ""}`}>{state.message}</p>

      {state.rounds.length > 0 && (
        <div className="dp-rounds" role="tablist" aria-label="Rounds">
          {state.rounds.map((r) => (
            <button key={r.n} role="tab" aria-selected={r.n === round?.n} className={r.n === round?.n ? "on" : ""} onClick={() => setRoundNo(r.n)}>
              Round {r.n}
              {r.review && <b className={scoreClass(r.review.score)}>{r.review.score}</b>}
              {r.fixTaskId != null && <i title="Fixed afterwards">🛠</i>}
            </button>
          ))}
          {first && state.rounds.length > 1 && state.rounds[state.rounds.length - 1].review && (
            <span className="dp-trend muted small">
              {first.score} → {state.rounds[state.rounds.length - 1].review!.score}
            </span>
          )}
        </div>
      )}

      {review && round && (
        <>
          <div className="dp-score">
            <div className={`dp-ring ${scoreClass(review.score)}`}>
              <b>{review.score}</b>
              <span>/100</span>
            </div>
            <div>
              <div className={`dp-verdict ${review.verdict}`}>{review.verdict === "ship" ? "Ready to ship" : "Needs fixes"}</div>
              <p className="dp-summary">{review.summary}</p>
            </div>
          </div>

          <div className="dp-section">
            <h4>
              Issues <span className="muted">{review.issues.length}</span>
            </h4>
            {review.issues.length === 0 ? <p className="muted small">Nothing the director would send back.</p> : <ul className="dp-issues">{review.issues.map((i) => <IssueRow key={i.id} issue={i} />)}</ul>}
          </div>

          {review.strengths.length > 0 && (
            <div className="dp-section">
              <h4>Working well</h4>
              <ul className="dp-list">{review.strengths.map((s, i) => <li key={i}>{s}</li>)}</ul>
            </div>
          )}

          <div className="dp-section">
            <div className="dp-shots-head">
              <h4>What the director saw</h4>
              <div className="seg" role="group" aria-label="Screen size">
                {(["desktop", "tablet", "phone"] as const).map((v) => (
                  <button key={v} className={view === v ? "on" : ""} onClick={() => setView(v)}>
                    {v}
                  </button>
                ))}
              </div>
            </div>
            <div className="dp-thumbs">{shots.map((s) => <Thumb key={s.path} shot={s} onOpen={(src, caption) => setBig({ src, caption })} />)}</div>
          </div>
        </>
      )}

      {state.status === "done" && state.lessonsAdded > 0 && (
        <p className="dp-learned">
          Learned {state.lessonsAdded} new design rule{state.lessonsAdded === 1 ? "" : "s"}. They're saved in the Website design skill (Know-how page), so the next site starts better.
        </p>
      )}

      {big && (
        <div className="dp-big" role="dialog" aria-label={big.caption} onClick={() => setBig(null)}>
          <figure>
            <img src={big.src} alt={big.caption} />
            <figcaption>{big.caption} · click to close</figcaption>
          </figure>
        </div>
      )}
    </div>
  );
}
