import type { Briefing } from "./types";

export const WEEKDAYS = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

/** "Weekdays at 09:00", "Mondays at 08:30", "Every day at 07:00". */
export function scheduleText(b: Pick<Briefing, "repeat" | "weekday" | "time">) {
  if (b.repeat === "weekly") return `${WEEKDAYS[b.weekday] ?? "Monday"}s at ${b.time}`;
  if (b.repeat === "weekdays") return `Weekdays at ${b.time}`;
  return `Every day at ${b.time}`;
}

/** "Mon 5 Oct, 09:00". */
export function when(ms: number | null) {
  if (!ms) return "";
  return new Date(ms).toLocaleString([], { weekday: "short", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
}

export const blankBriefing = (chatId: string): Briefing => ({
  id: "",
  title: "",
  request: "",
  repeat: "weekdays",
  weekday: 0,
  time: "09:00",
  depth: "quick",
  chatId,
  enabled: true,
  createdAt: 0,
  lastRun: null,
  lastTask: null,
  nextRun: null,
});
