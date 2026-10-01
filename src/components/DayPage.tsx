import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useState } from "react";
import type { CalEvent } from "../lib/calendar";
import type { Briefing, Routine, Task } from "../lib/types";

interface Mail {
  id: string;
  from: string;
  subject: string;
  snippet: string;
}

const hm = (iso: string) => new Date(iso).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
const sender = (from: string) => from.replace(/<.*>/, "").replace(/"/g, "").trim() || from;

/** Routines and briefings that will run later today. */
function dueToday(routines: Routine[], briefings: Briefing[]): { title: string; at: Date }[] {
  const now = new Date();
  const end = new Date(now).setHours(23, 59, 59, 999);
  const out: { title: string; at: Date }[] = [];
  for (const b of briefings) if (b.enabled && b.nextRun && b.nextRun > now.getTime() && b.nextRun <= end) out.push({ title: b.title, at: new Date(b.nextRun) });
  const monday0 = (now.getDay() + 6) % 7;
  for (const r of routines) {
    if (!r.enabled || r.repeat === "manual") continue;
    const ok = r.repeat === "daily" || (r.repeat === "weekdays" && monday0 < 5) || (r.repeat === "weekly" && r.weekday === monday0);
    const [h, m] = r.time.split(":").map(Number);
    const at = new Date(now);
    at.setHours(h || 0, m || 0, 0, 0);
    if (ok && at > now) out.push({ title: r.title, at });
  }
  return out.sort((a, b) => a.at.getTime() - b.at.getTime());
}

/** Your day at a glance: what's on, what needs a reply, what Jarvis is working on, what runs later. */
export default function DayPage({
  tasks,
  userName,
  onBrief,
  onOpenTask,
  onOpenMail,
  onOpenCalendar,
  onOpenSettings,
  onClose,
}: {
  tasks: Task[];
  userName: string;
  onBrief: () => void;
  onOpenTask: (id: number) => void;
  onOpenMail: () => void;
  onOpenCalendar: () => void;
  onOpenSettings: () => void;
  onClose: () => void;
}) {
  const [events, setEvents] = useState<CalEvent[] | null>(null);
  const [mail, setMail] = useState<Mail[] | null>(null);
  const [later, setLater] = useState<{ title: string; at: Date }[]>([]);
  const [problems, setProblems] = useState<{ cal?: string; mail?: string }>({});
  const [loading, setLoading] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    const start = new Date();
    start.setHours(0, 0, 0, 0);
    const end = new Date(start);
    end.setHours(23, 59, 59, 999);
    const [ev, ml, rt, br] = await Promise.all([
      invoke<CalEvent[]>("calendar_list", { from: start.toISOString(), to: end.toISOString() }).then((v) => ({ v }), (e) => ({ e: String(e) })),
      invoke<Mail[]>("mail_search", { query: "is:unread in:inbox newer_than:2d -category:promotions -category:social", max: 8 }).then((v) => ({ v }), (e) => ({ e: String(e) })),
      invoke<Routine[]>("list_routines").catch(() => [] as Routine[]),
      invoke<Briefing[]>("list_briefings").catch(() => [] as Briefing[]),
    ]);
    setEvents("v" in ev ? ev.v : []);
    setMail("v" in ml ? ml.v : []);
    setProblems({ cal: "e" in ev ? ev.e : undefined, mail: "e" in ml ? ml.e : undefined });
    setLater(dueToday(rt, br));
    setLoading(false);
  }, []);
  useEffect(() => {
    load();
  }, [load]);

  const now = new Date();
  const hour = now.getHours();
  const greeting = hour < 12 ? "Good morning" : hour < 18 ? "Good afternoon" : "Good evening";
  const all = [...(events ?? [])].sort((a, b) => Number(b.allDay) - Number(a.allDay) || new Date(a.start).getTime() - new Date(b.start).getTime());
  const isPast = (e: CalEvent) => !e.allDay && new Date(e.end) <= now;
  const isNow = (e: CalEvent) => !e.allDay && new Date(e.start) <= now && new Date(e.end) > now;
  const left = all.filter((e) => !isPast(e));
  const next = all.find((e) => !e.allDay && new Date(e.start) > now);
  const current = all.find(isNow);
  const dayAgo = Date.now() - 86400e3;
  const work = tasks.filter((t) => !t.parentId && !t.routine && t.kind !== "skill" && (t.status === "running" || (t.finishedAt ?? 0) > dayAgo)).slice(0, 6);
  const running = work.filter((t) => t.status === "running").length;
  const needsGoogle = (msg?: string) => !!msg && /connect|not connected/i.test(msg);
  const until = (iso: string) => {
    const mins = Math.max(0, Math.round((new Date(iso).getTime() - now.getTime()) / 60000));
    return mins < 60 ? `in ${mins} min` : `in ${Math.floor(mins / 60)} h${mins % 60 ? ` ${mins % 60} min` : ""}`;
  };
  const GoogleNote = ({ msg }: { msg: string }) => (
    <p className="day-empty">
      {needsGoogle(msg) ? "Connect Google to see this." : msg}
      {needsGoogle(msg) && (
        <button className="mini" onClick={onOpenSettings}>Open Settings</button>
      )}
    </p>
  );

  return (
    <section className="libpage knowpage day">
      <header className="lib-head">
        <button className="icon-btn back" onClick={onClose} aria-label="Back" title="Back">
          ←
        </button>
        <div>
          <h2>{greeting}{userName ? `, ${userName}` : ""}</h2>
          <p className="muted small">{now.toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" })}</p>
        </div>
        <span className="grow" />
        <button className="mini" onClick={load} disabled={loading}>{loading ? "Refreshing…" : "Refresh"}</button>
        <button className="btn primary" onClick={onBrief}>Brief me</button>
      </header>

      <div className="rt-body proj-body day-body">
        <div className="day-stats">
          <button className="day-stat" onClick={onOpenCalendar}>
            <b>{events === null ? "–" : left.length}</b>
            <span>{left.length === 1 ? "event left" : "events left"}</span>
          </button>
          <button className="day-stat" onClick={onOpenMail}>
            <b>{mail === null ? "–" : mail.length}</b>
            <span>unread mail</span>
          </button>
          <div className="day-stat">
            <b>{running}</b>
            <span>{running === 1 ? "task running" : "tasks running"}</span>
          </div>
          <div className="day-stat">
            <b>{later.length}</b>
            <span>scheduled later</span>
          </div>
        </div>

        <div className="day-cols">
          <div className="day-main-col">
            {(current || next) && (
              <section className="day-hero">
                <span className="day-hero-tag">{current ? "Happening now" : `Up next · ${until(next!.start)}`}</span>
                <h3>{(current ?? next)!.title || "(no title)"}</h3>
                <p className="muted">
                  {hm((current ?? next)!.start)} – {hm((current ?? next)!.end)}
                  {(current ?? next)!.location ? ` · ${(current ?? next)!.location}` : ""}
                  {(current ?? next)!.attendees > 1 ? ` · ${(current ?? next)!.attendees} people` : ""}
                </p>
                {(current ?? next)!.meet && (
                  <button className="btn primary" onClick={() => openUrl((current ?? next)!.meet).catch(() => {})}>Join meeting</button>
                )}
              </section>
            )}

            <section className="day-card">
              <div className="day-card-head">
                <h3>Schedule</h3>
                <button className="mini" onClick={onOpenCalendar}>Open calendar</button>
              </div>
              {problems.cal ? (
                <GoogleNote msg={problems.cal} />
              ) : events === null ? (
                <p className="day-empty">Loading…</p>
              ) : all.length === 0 ? (
                <p className="day-empty">Your calendar is clear today.</p>
              ) : (
                <ol className="day-timeline">
                  {all.map((e, i) => {
                    const showNow = !isPast(e) && !e.allDay && (i === 0 || isPast(all[i - 1]) || all[i - 1].allDay) && !isNow(e) && all.some(isPast);
                    return (
                      <li key={e.id} className={`${isPast(e) ? "past" : ""} ${isNow(e) ? "now" : ""} ${e.id === next?.id ? "next" : ""}`}>
                        {showNow && <div className="day-nowline"><span>Now · {hm(now.toISOString())}</span></div>}
                        <span className="day-time">{e.allDay ? "All day" : hm(e.start)}</span>
                        <span className="day-dot" aria-hidden="true" />
                        <span className="day-what">
                          <b>{e.title || "(no title)"}</b>
                          <span className="muted small">
                            {[e.allDay ? "" : `until ${hm(e.end)}`, e.location, e.attendees > 1 ? `${e.attendees} people` : ""].filter(Boolean).join(" · ")}
                          </span>
                        </span>
                      </li>
                    );
                  })}
                </ol>
              )}
            </section>
          </div>

          <div className="day-side-col">
            <section className="day-card">
              <div className="day-card-head">
                <h3>Needs a look</h3>
                <button className="mini" onClick={onOpenMail}>Open mail</button>
              </div>
              {problems.mail ? (
                <GoogleNote msg={problems.mail} />
              ) : mail === null ? (
                <p className="day-empty">Loading…</p>
              ) : mail.length === 0 ? (
                <p className="day-empty">Inbox is quiet. No unread mail in the last two days.</p>
              ) : (
                <ul className="day-rows">
                  {mail.map((m) => (
                    <li key={m.id}>
                      <button onClick={onOpenMail}>
                        <b>{sender(m.from)}</b>
                        <span>{m.subject || "(no subject)"}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </section>

            <section className="day-card">
              <div className="day-card-head">
                <h3>Jarvis's work</h3>
              </div>
              {work.length === 0 ? (
                <p className="day-empty">Nothing running or finished in the last day.</p>
              ) : (
                <ul className="day-rows">
                  {work.map((t) => (
                    <li key={t.id}>
                      <button onClick={() => onOpenTask(t.id)}>
                        <span className={`day-pill ${t.status}`}>{{ running: "Running", failed: "Failed", cancelled: "Stopped", done: "Done" }[t.status]}</span>
                        <span>{t.title || t.request.slice(0, 60)}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </section>

            <section className="day-card">
              <div className="day-card-head">
                <h3>Later today</h3>
              </div>
              {later.length === 0 ? (
                <p className="day-empty">No routines or briefings are due.</p>
              ) : (
                <ul className="day-rows">
                  {later.map((l, i) => (
                    <li key={i}>
                      <div className="day-later">
                        <span className="day-time">{l.at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</span>
                        <span>{l.title}</span>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
            </section>
          </div>
        </div>
      </div>
    </section>
  );
}
