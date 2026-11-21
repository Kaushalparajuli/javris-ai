import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useRef, useState } from "react";
import type { CalEvent } from "../lib/calendar";
import type { ChatSummary, OrbMode, Routine } from "../lib/types";
import type { WorkItem } from "../lib/workItems";
import Orb from "./Orb";
import TaskCard from "./TaskCard";

interface Mail {
  id: string;
  from: string;
  subject: string;
  snippet: string;
}

const hm = (iso: string) => new Date(iso).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
const sender = (from: string) => from.replace(/<.*>/, "").replace(/"/g, "").trim() || from;
const shortDay = (ms: number) => {
  const d = new Date(ms);
  const today = new Date();
  if (d.toDateString() === today.toDateString()) return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return d.toLocaleDateString([], { day: "numeric", month: "short" });
};

/** Routines that will run later today. */
function dueToday(routines: Routine[]): { title: string; at: Date }[] {
  const now = new Date();
  const monday0 = (now.getDay() + 6) % 7;
  const out: { title: string; at: Date }[] = [];
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

/** Quick starts: each puts the beginning of a request in the box for you to finish. */
const STARTERS: { label: string; text: string; icon: string }[] = [
  { label: "Research", text: "Research ", icon: "M10.5 4a6.5 6.5 0 0 1 5.2 10.4l4.4 4.4-1.4 1.4-4.4-4.4A6.5 6.5 0 1 1 10.5 4zm0 2a4.5 4.5 0 1 0 0 9 4.5 4.5 0 0 0 0-9z" },
  { label: "Make an image", text: "Make an image of ", icon: "M4 5h16a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1zm1 2v8.6l3.5-3.5 3 3 4-4L19 14.6V7zm4 1.5a1.5 1.5 0 1 1 0 3 1.5 1.5 0 0 1 0-3z" },
  { label: "Write a document", text: "Write a document about ", icon: "M6 2h8l6 6v13a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1zm7 1.5V9h5.5zM8 13h8v2H8zm0 4h8v2H8z" },
];

/**
 * The start screen: ask Jarvis anything, see what your day holds and what Jarvis is working on,
 * and pick up recent work or a recent conversation.
 */
export default function HomePage({
  userName,
  work,
  chats,
  orb,
  onAsk,
  onTalk,
  onBrief,
  onOpenTask,
  onOpenChat,
  onOpenMail,
  onOpenCalendar,
  onOpenLibrary,
  onOpenSettings,
}: {
  userName: string;
  work: WorkItem[];
  chats: ChatSummary[];
  orb: { mode(): OrbMode; level(): number };
  onAsk: (text: string) => void;
  onTalk: () => void;
  onBrief: () => void;
  onOpenTask: (id: number) => void;
  onOpenChat: (id: string) => void;
  onOpenMail: () => void;
  onOpenCalendar: () => void;
  onOpenLibrary: () => void;
  onOpenSettings: () => void;
}) {
  const [text, setText] = useState("");
  const input = useRef<HTMLInputElement>(null);
  const [events, setEvents] = useState<CalEvent[] | null>(null);
  const [mail, setMail] = useState<Mail[] | null>(null);
  const [later, setLater] = useState<{ title: string; at: Date }[]>([]);
  const [problems, setProblems] = useState<{ cal?: string; mail?: string }>({});

  const load = useCallback(async () => {
    const start = new Date();
    start.setHours(0, 0, 0, 0);
    const end = new Date(start);
    end.setHours(23, 59, 59, 999);
    const [ev, ml, rt] = await Promise.all([
      invoke<CalEvent[]>("calendar_list", { from: start.toISOString(), to: end.toISOString() }).then((v) => ({ v }), (e) => ({ e: String(e) })),
      invoke<Mail[]>("mail_search", { query: "is:unread in:inbox newer_than:2d -category:promotions -category:social", max: 5 }).then((v) => ({ v }), (e) => ({ e: String(e) })),
      invoke<Routine[]>("list_routines").catch(() => [] as Routine[]),
    ]);
    setEvents("v" in ev ? ev.v : []);
    setMail("v" in ml ? ml.v : []);
    setProblems({ cal: "e" in ev ? ev.e : undefined, mail: "e" in ml ? ml.e : undefined });
    setLater(dueToday(rt));
  }, []);
  useEffect(() => {
    load();
    input.current?.focus();
  }, [load]);

  const now = new Date();
  const hour = now.getHours();
  const greeting = hour < 5 ? "Good night" : hour < 12 ? "Good morning" : hour < 18 ? "Good afternoon" : "Good evening";
  const upcoming = [...(events ?? [])]
    .filter((e) => e.allDay || new Date(e.end) > now)
    .sort((a, b) => Number(b.allDay) - Number(a.allDay) || new Date(a.start).getTime() - new Date(b.start).getTime());
  const happening = upcoming.find((e) => !e.allDay && new Date(e.start) <= now);
  const inProgress = work.filter((w) => w.latest.status === "running");
  const recent = work.filter((w) => w.latest.status !== "running" && w.open && !w.latest.routine).slice(0, 5);
  const recentChats = [...chats].filter((c) => c.count > 0).sort((a, b) => b.updatedAt - a.updatedAt).slice(0, 4);
  const needsGoogle = (msg?: string) => !!msg && /connect|not connected/i.test(msg);

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const t = text.trim();
    if (!t) return;
    setText("");
    onAsk(t);
  };
  const start = (t: string) => {
    setText(t);
    requestAnimationFrame(() => {
      const el = input.current;
      if (!el) return;
      el.focus();
      el.setSelectionRange(t.length, t.length);
    });
  };
  const Connect = ({ msg }: { msg: string }) => (
    <p className="home-empty">
      {needsGoogle(msg) ? "Connect Google to see this." : msg}
      {needsGoogle(msg) && (
        <button className="home-link" onClick={onOpenSettings}>
          Connect
        </button>
      )}
    </p>
  );

  return (
    <section className="libpage home">
      <div className="home-inner">
        <div className="home-hero">
          <Orb source={orb} size={96} />
          <h1>
            {greeting}
            {userName ? `, ${userName}` : ""}
          </h1>
          <p className="muted">{now.toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" })}</p>
          <form className="home-ask" onSubmit={submit}>
            <input ref={input} id="home-ask" value={text} onChange={(e) => setText(e.target.value)} placeholder="Ask Jarvis anything…" aria-label="Ask Jarvis" />
            {text.trim() ? (
              <button type="submit" className="home-send" aria-label="Ask" title="Ask (Enter)">
                <svg viewBox="0 0 24 24" aria-hidden="true">
                  <path d="M12 4l7 7-1.4 1.4L13 7.8V20h-2V7.8l-4.6 4.6L5 11z" />
                </svg>
              </button>
            ) : (
              <button type="button" className="home-send mic" onClick={onTalk} aria-label="Talk to Jarvis" title="Talk (⌥Space)">
                <svg viewBox="0 0 24 24" aria-hidden="true">
                  <path d="M12 14a3 3 0 0 0 3-3V5a3 3 0 1 0-6 0v6a3 3 0 0 0 3 3zm5-3a5 5 0 0 1-10 0H5a7 7 0 0 0 6 6.92V21h2v-3.08A7 7 0 0 0 19 11h-2z" />
                </svg>
              </button>
            )}
          </form>
          <div className="home-starters">
            {STARTERS.map((s) => (
              <button key={s.label} type="button" onClick={() => start(s.text)}>
                <svg viewBox="0 0 24 24" aria-hidden="true">
                  <path d={s.icon} />
                </svg>
                {s.label}
              </button>
            ))}
            <button type="button" onClick={onBrief}>
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="M12 7a5 5 0 1 1 0 10 5 5 0 0 1 0-10zM11 1h2v3h-2zm0 19h2v3h-2zM1 11h3v2H1zm19 0h3v2h-3z" />
              </svg>
              Brief me on my day
            </button>
          </div>
        </div>

        <div className="home-grid">
          <section className="home-card">
            <header>
              <h2>Today</h2>
              <button className="home-link" onClick={onOpenCalendar}>
                Calendar
              </button>
            </header>
            {problems.cal ? (
              <Connect msg={problems.cal} />
            ) : events === null ? (
              <p className="home-empty">Loading…</p>
            ) : upcoming.length === 0 ? (
              <p className="home-empty">Nothing else on your calendar today.</p>
            ) : (
              <ul className="home-rows">
                {upcoming.slice(0, 5).map((e) => (
                  <li key={e.id} className={e === happening ? "now" : ""}>
                    <span className="home-time">{e.allDay ? "All day" : hm(e.start)}</span>
                    <span className="home-what">
                      <b>{e.title || "(no title)"}</b>
                      {e === happening && <small>Happening now</small>}
                    </span>
                    {e.meet && (
                      <button className="home-link" onClick={() => openUrl(e.meet).catch(() => {})}>
                        Join
                      </button>
                    )}
                  </li>
                ))}
              </ul>
            )}
            {later.length > 0 && (
              <p className="home-foot">
                Later: {later.map((l) => `${l.title} at ${l.at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`).join(" · ")}
              </p>
            )}
          </section>

          <section className="home-card">
            <header>
              <h2>Needs a look</h2>
              <button className="home-link" onClick={onOpenMail}>
                Mail
              </button>
            </header>
            {problems.mail ? (
              <Connect msg={problems.mail} />
            ) : mail === null ? (
              <p className="home-empty">Loading…</p>
            ) : mail.length === 0 ? (
              <p className="home-empty">No unread mail from the last two days.</p>
            ) : (
              <ul className="home-rows mail">
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

          {inProgress.length > 0 && (
            <section className="home-card">
              <header>
                <h2>In progress</h2>
              </header>
              <div className="home-work">
                {inProgress.map((w) => (
                  <TaskCard key={w.key} task={w.latest} item={w} onOpen={onOpenTask} />
                ))}
              </div>
            </section>
          )}

          <section className="home-card">
            <header>
              <h2>Recent work</h2>
              <button className="home-link" onClick={onOpenLibrary}>
                Library
              </button>
            </header>
            {recent.length === 0 ? (
              <p className="home-empty">Research, images and documents Jarvis makes show up here.</p>
            ) : (
              <div className="home-work">
                {recent.map((w) => (
                  <TaskCard key={w.key} task={w.latest} item={w} onOpen={onOpenTask} />
                ))}
              </div>
            )}
          </section>

          {recentChats.length > 0 && (
            <section className="home-card">
              <header>
                <h2>Continue</h2>
              </header>
              <ul className="home-rows chats">
                {recentChats.map((c) => (
                  <li key={c.id}>
                    <button onClick={() => onOpenChat(c.id)}>
                      <b>{c.title || "New chat"}</b>
                      <span className="home-time">{shortDay(c.updatedAt)}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          )}
        </div>
      </div>
    </section>
  );
}
