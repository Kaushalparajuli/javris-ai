// Dates and repeat schedules for routines: "Weekdays at 09:00", "Mon 5 Oct, 09:00".

export const WEEKDAYS = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

/** "Weekdays at 09:00", "Mondays at 08:30", "Every day at 07:00". */
export function scheduleText(s: { repeat: string; weekday: number; time: string }) {
  if (s.repeat === "weekly") return `${WEEKDAYS[s.weekday] ?? "Monday"}s at ${s.time}`;
  if (s.repeat === "weekdays") return `Weekdays at ${s.time}`;
  return `Every day at ${s.time}`;
}

/** "Mon 5 Oct, 09:00". */
export function when(ms: number | null) {
  if (!ms) return "";
  return new Date(ms).toLocaleString([], { weekday: "short", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
}
