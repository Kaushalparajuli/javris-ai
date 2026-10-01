// Shared bits for reading and changing calendar events, used by the Calendar page and the voice tools.

export interface CalEvent {
  id: string;
  title: string;
  /** RFC 3339 date-time, or YYYY-MM-DD for all-day events. */
  start: string;
  end: string;
  allDay: boolean;
  location: string;
  attendees: number;
  attendeeEmails: string[];
  description: string;
  link: string;
  meet: string;
  /** One occurrence of a repeating event: changes and deletes apply to this one only. */
  recurring: boolean;
}

export const timeZone = () => Intl.DateTimeFormat().resolvedOptions().timeZone;

const pad = (n: number) => String(n).padStart(2, "0");
export const dateInput = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
export const timeInput = (d: Date) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;
/** "YYYY-MM-DDTHH:MM" in local time, which is what the calendar commands take. */
export const localDateTime = (d: Date) => `${dateInput(d)}T${timeInput(d)}`;

/** A Google all-day value ("2026-10-01") is a calendar date, not a moment: read it as local midnight. */
export function parseWhen(s: string, allDay: boolean) {
  if (allDay) {
    const [y, m, d] = s.split("-").map(Number);
    return new Date(y, m - 1, d);
  }
  return new Date(s);
}

export const addDays = (d: Date, n: number) => new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);

/** "Thu 2 Oct, 3:00 PM – 4:00 PM", or "Thu 2 Oct (all day)". */
export function describeWhen(e: Pick<CalEvent, "start" | "end" | "allDay">) {
  const s = parseWhen(e.start, e.allDay);
  const day = s.toLocaleDateString([], { weekday: "short", day: "numeric", month: "short" });
  if (e.allDay) return `${day} (all day)`;
  const t = (d: Date) => d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  return `${day}, ${t(s)} – ${t(parseWhen(e.end, false))}`;
}

export const isEmail = (x: string) => /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(x);
/** Split a typed list of guests on commas, semicolons and spaces. */
export const splitEmails = (text: string) => text.split(/[\s,;]+/).map((x) => x.trim()).filter(Boolean);
