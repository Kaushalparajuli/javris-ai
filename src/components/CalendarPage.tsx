import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useMemo, useState } from "react";
import { requestApproval } from "../lib/approvals";
import { addDays, dateInput, describeWhen, isEmail, localDateTime, parseWhen, splitEmails, timeInput, timeZone, type CalEvent } from "../lib/calendar";
import type { Settings } from "../lib/types";
import GateGoogle, { useGoogle } from "./GateGoogle";

const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate());
const parse = parseWhen;
const clock = (d: Date) => d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });

/** Your Google Calendar, read-only: a day or a week, with join links. */
export default function CalendarPage({ settings, onAsk, onOpenSettings, onClose }: { settings: Settings | null; onAsk: (text: string) => void; onOpenSettings: () => void; onClose: () => void }) {
  const google = useGoogle("calendar");
  const [span, setSpan] = useState<1 | 7>(7);
  const [anchor, setAnchor] = useState(() => startOfDay(new Date()));
  const [events, setEvents] = useState<CalEvent[] | null>(null);
  const [selected, setSelected] = useState<CalEvent | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [now, setNow] = useState(new Date());
  /** The event being added (id empty) or changed. */
  const [form, setForm] = useState<CalEvent | null>(null);

  const remove = async (e: CalEvent) => {
    const notify = e.attendeeEmails.length > 0;
    const r = await requestApproval(
      {
        tool: "calendar_delete",
        risk: "delete",
        title: `Delete “${e.title}”`,
        detail: `${describeWhen(e)}${e.location ? `\n${e.location}` : ""}${notify ? `\n\nThese guests will be emailed that it's cancelled:\n${e.attendeeEmails.join("\n")}` : ""}${e.recurring ? "\n\nThis deletes only this one occurrence." : ""}`,
        okLabel: "Delete event",
      },
      settings,
    );
    if (!r.ok) return;
    try {
      await invoke("calendar_delete", { id: e.id, notify });
      setSelected(null);
      load();
    } catch (err) {
      setError(String(err));
    }
  };

  const save = async (e: CalEvent, original: CalEvent | null) => {
    const guests = e.attendeeEmails;
    const notify = guests.length > 0 || (original?.attendeeEmails.length ?? 0) > 0;
    if (notify) {
      const r = await requestApproval(
        {
          tool: original ? "calendar_update" : "calendar_create",
          risk: "send",
          title: original ? `Save changes to “${e.title}”` : `Add “${e.title}” and email invitations`,
          detail: `${describeWhen(e)}\n\nGuests who'll be emailed:\n${[...new Set([...(original?.attendeeEmails ?? []), ...guests])].join("\n")}`,
          okLabel: original ? "Save and notify" : "Add and invite",
        },
        settings,
      );
      if (!r.ok) return false;
    }
    try {
      const zone = timeZone();
      if (original) {
        await invoke("calendar_update", {
          id: original.id,
          patch: { title: e.title, start: e.start, end: e.end, allDay: e.allDay, timeZone: zone, location: e.location, description: e.description, attendees: guests, notify },
        });
      } else {
        await invoke("calendar_create", {
          event: { title: e.title, start: e.start, end: e.end, allDay: e.allDay, timeZone: zone, attendees: guests, location: e.location, description: e.description },
        });
      }
      setForm(null);
      setSelected(null);
      load();
      return true;
    } catch (err) {
      setError(String(err));
      return false;
    }
  };

  const blank = (): CalEvent => {
    const base = new Date(anchor.getTime());
    const today = startOfDay(new Date()).getTime();
    // Start on the day being looked at, at the next whole hour when that's today.
    const at = base.getTime() === today || (span === 7 && anchor.getTime() <= today && addDays(first, 7).getTime() > today) ? new Date() : new Date(base.setHours(9, 0, 0, 0));
    at.setMinutes(0, 0, 0);
    if (at.getHours() < 23 && base.getTime() === today) at.setHours(at.getHours() + 1);
    return { id: "", title: "", start: localDateTime(at), end: localDateTime(new Date(at.getTime() + 3600e3)), allDay: false, location: "", attendees: 0, attendeeEmails: [], description: "", link: "", meet: "", recurring: false };
  };

  // A week starts on Monday. A day view starts on the anchor itself.
  const first = useMemo(() => {
    if (span === 1) return anchor;
    const back = (anchor.getDay() + 6) % 7;
    return addDays(anchor, -back);
  }, [anchor, span]);
  const days = useMemo(() => Array.from({ length: span }, (_, i) => addDays(first, i)), [first, span]);

  const load = useCallback(() => {
    setLoading(true);
    invoke<CalEvent[]>("calendar_list", { from: first.toISOString(), to: addDays(first, span).toISOString() })
      .then((l) => {
        setEvents(l);
        setError("");
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, [first, span]);

  const connected = !!google.state?.connected;
  useEffect(() => {
    if (!connected) return;
    setEvents(null);
    load();
    const t = setInterval(load, 120_000);
    return () => clearInterval(t);
  }, [connected, load]);
  useEffect(() => {
    const t = setInterval(() => setNow(new Date()), 60_000);
    return () => clearInterval(t);
  }, []);

  /** Events that touch a day, all-day first and then by start time. */
  const onDay = (d: Date) => {
    const from = d.getTime();
    const to = addDays(d, 1).getTime();
    return (events ?? [])
      .filter((e) => {
        const s = parse(e.start, e.allDay).getTime();
        const en = parse(e.end || e.start, e.allDay).getTime();
        return s < to && Math.max(en, s + 1) > from;
      })
      .sort((a, b) => Number(b.allDay) - Number(a.allDay) || parse(a.start, a.allDay).getTime() - parse(b.start, b.allDay).getTime());
  };

  const today = startOfDay(now).getTime();
  const title =
    span === 1
      ? first.toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" })
      : `${days[0].toLocaleDateString([], { day: "numeric", month: "short" })} – ${days[6].toLocaleDateString([], { day: "numeric", month: "short", year: "numeric" })}`;

  return (
    <section className="libpage datapage">
      <header>
        <button className="icon-btn back" onClick={onClose} aria-label="Back to the conversation" title="Back">
          <svg viewBox="0 0 24 24">
            <path d="M15.4 4.6 13.9 3.2 6.1 11l7.8 7.8 1.5-1.4L9 11z" />
          </svg>
        </button>
        <div>
          <h2>Calendar</h2>
          <p className="muted small">{connected ? title : google.state?.email || "Google Calendar"}</p>
        </div>
        {connected && (
          <div className="cal-nav">
            <button className="mini" onClick={() => setAnchor(addDays(first, -span))} aria-label="Previous">
              ‹
            </button>
            <button className="mini" onClick={() => setAnchor(startOfDay(new Date()))}>
              Today
            </button>
            <button className="mini" onClick={() => setAnchor(addDays(first, span))} aria-label="Next">
              ›
            </button>
            <div className="seg" role="tablist">
              <button role="tab" aria-selected={span === 1} className={span === 1 ? "on" : ""} onClick={() => setSpan(1)}>
                Day
              </button>
              <button role="tab" aria-selected={span === 7} className={span === 7 ? "on" : ""} onClick={() => setSpan(7)}>
                Week
              </button>
            </div>
            <button className="mini" onClick={load} disabled={loading}>
              {loading ? "Loading…" : "Refresh"}
            </button>
            <button className="btn primary" onClick={() => setForm(blank())}>
              New event
            </button>
          </div>
        )}
      </header>

      {!connected ? (
        <GateGoogle what="calendar" google={google} onOpenSettings={onOpenSettings} />
      ) : (
        <>
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
          <div className={`cal-grid span${span}`}>
            {days.map((d) => {
              const list = onDay(d);
              const isToday = d.getTime() === today;
              return (
                <div key={d.getTime()} className={`cal-day ${isToday ? "today" : ""}`}>
                  <div className="cal-head">
                    <span>{d.toLocaleDateString([], { weekday: "short" })}</span>
                    <b>{d.getDate()}</b>
                  </div>
                  {events === null && <p className="muted small">Loading…</p>}
                  {events !== null && list.length === 0 && <p className="muted small cal-none">Nothing</p>}
                  {list.map((e) => {
                    const s = parse(e.start, e.allDay);
                    const en = parse(e.end || e.start, e.allDay);
                    const live = !e.allDay && s <= now && now < en;
                    const over = !e.allDay && en <= now;
                    return (
                      <button key={e.id + e.start} className={`cal-ev ${e.allDay ? "allday" : ""} ${live ? "live" : ""} ${over ? "over" : ""} ${selected?.id === e.id ? "on" : ""}`} onClick={() => setSelected(selected?.id === e.id ? null : e)}>
                        <time>{e.allDay ? "All day" : `${clock(s)} – ${clock(en)}`}</time>
                        <b>{e.title}</b>
                        {e.location && <span className="muted small">{e.location}</span>}
                      </button>
                    );
                  })}
                </div>
              );
            })}
          </div>
          {selected && (
            <aside className="cal-detail">
              <div>
                <h3>{selected.title}</h3>
                <p className="muted small">
                  {selected.allDay
                    ? parse(selected.start, true).toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" })
                    : `${parse(selected.start, false).toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" })} · ${clock(parse(selected.start, false))} – ${clock(parse(selected.end, false))}`}
                  {selected.attendees > 0 ? ` · ${selected.attendees} people` : ""}
                  {selected.location ? ` · ${selected.location}` : ""}
                </p>
              </div>
              <div className="actions">
                {selected.meet && (
                  <button className="btn primary" onClick={() => openUrl(selected.meet).catch(() => {})}>
                    Join video call
                  </button>
                )}
                <button className="mini" onClick={() => onAsk(`Help me prepare for my calendar event "${selected.title}" on ${parse(selected.start, selected.allDay).toDateString()}. Check my email for anything related to it.`)}>
                  Ask Jarvis to prep me
                </button>
                {selected.link && (
                  <button className="mini" onClick={() => openUrl(selected.link).catch(() => {})}>
                    Open in Google Calendar
                  </button>
                )}
                <button className="mini" onClick={() => setForm(selected)}>
                  Edit
                </button>
                <button className="mini danger" onClick={() => remove(selected)}>
                  Delete
                </button>
                <button className="mini" onClick={() => setSelected(null)}>
                  Close
                </button>
              </div>
            </aside>
          )}
        </>
      )}
      {form && <EventForm event={form} onCancel={() => setForm(null)} onSave={(e) => save(e, form.id ? form : null)} />}
    </section>
  );
}

/** Add or change an event: the fields people actually set. */
function EventForm({ event, onSave, onCancel }: { event: CalEvent; onSave: (e: CalEvent) => Promise<boolean>; onCancel: () => void }) {
  const adding = !event.id;
  const s0 = parseWhen(event.start, event.allDay);
  // An all-day event's end is the day after its last day; the form shows the last day itself.
  const e0 = event.allDay ? addDays(parseWhen(event.end, true), -1) : parseWhen(event.end, false);
  const [title, setTitle] = useState(event.title);
  const [allDay, setAllDay] = useState(event.allDay);
  const [date, setDate] = useState(dateInput(s0));
  const [endDate, setEndDate] = useState(dateInput(e0));
  const [from, setFrom] = useState(event.allDay ? "09:00" : timeInput(s0));
  const [to, setTo] = useState(event.allDay ? "10:00" : timeInput(e0));
  const [where, setWhere] = useState(event.location);
  const [guests, setGuests] = useState(event.attendeeEmails.join(", "));
  const [notes, setNotes] = useState(event.description);
  const [problem, setProblem] = useState("");
  const [busy, setBusy] = useState(false);

  const submit = async (ev: React.FormEvent) => {
    ev.preventDefault();
    const emails = splitEmails(guests);
    const bad = emails.find((x) => !isEmail(x));
    if (!title.trim()) return setProblem("Give the event a name.");
    if (bad) return setProblem(`“${bad}” isn't an email address.`);
    let start: string, end: string;
    if (allDay) {
      if (endDate < date) return setProblem("The last day can't be before the first.");
      start = date;
      end = dateInput(addDays(new Date(`${endDate}T00:00:00`), 1));
    } else {
      start = `${date}T${from}`;
      end = `${endDate}T${to}`;
      if (new Date(end) <= new Date(start)) return setProblem("The end has to be after the start.");
    }
    setProblem("");
    setBusy(true);
    await onSave({ ...event, title: title.trim(), allDay, start, end, location: where.trim(), description: notes, attendeeEmails: emails, attendees: emails.length });
    setBusy(false);
  };

  return (
    <div className="overlay" onClick={onCancel}>
      <form className="eventform glass" onClick={(e) => e.stopPropagation()} onSubmit={submit} aria-label={adding ? "New event" : "Change event"}>
        <h3>{adding ? "New event" : "Change event"}</h3>
        <input autoFocus aria-label="Event name" placeholder="Event name" value={title} onChange={(e) => setTitle(e.target.value)} />
        <label className="check">
          <input type="checkbox" checked={allDay} onChange={(e) => setAllDay(e.target.checked)} />
          All day
        </label>
        <div className="evrow">
          <label>
            {allDay ? "First day" : "Starts"}
            <span>
              <input type="date" value={date} onChange={(e) => (setDate(e.target.value), endDate < e.target.value && setEndDate(e.target.value))} />
              {!allDay && <input type="time" value={from} onChange={(e) => setFrom(e.target.value)} />}
            </span>
          </label>
          <label>
            {allDay ? "Last day" : "Ends"}
            <span>
              <input type="date" value={endDate} min={date} onChange={(e) => setEndDate(e.target.value)} />
              {!allDay && <input type="time" value={to} onChange={(e) => setTo(e.target.value)} />}
            </span>
          </label>
        </div>
        <input aria-label="Where" placeholder="Where (optional)" value={where} onChange={(e) => setWhere(e.target.value)} />
        <input aria-label="Guests" placeholder="Guests' emails, separated by commas (optional)" value={guests} onChange={(e) => setGuests(e.target.value)} />
        <textarea aria-label="Notes" rows={3} placeholder="Notes (optional)" value={notes} onChange={(e) => setNotes(e.target.value)} />
        {event.recurring && <p className="hint">This repeats. Your changes apply to this one occurrence only.</p>}
        {guests.trim() && <p className="hint">Guests get an email when you save. You'll be asked to confirm first.</p>}
        {problem && <p className="error-text">{problem}</p>}
        <div className="approval-actions">
          <button type="button" className="btn" onClick={onCancel}>
            Cancel
          </button>
          <button type="submit" className="btn primary" disabled={busy}>
            {busy ? "Saving…" : adding ? "Add event" : "Save"}
          </button>
        </div>
      </form>
    </div>
  );
}
