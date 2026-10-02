// How much Jarvis may do on its own, picked under the prompt box:
//   show   — plan first: Jarvis can look things up, but changes nothing and says what it would do.
//   auto   — changes to files and documents go ahead on their own; sending, deleting, buying,
//            deploying and controlling other apps still wait for a click.
//   manual — every change waits for a click.
// The mode sits on top of the per-kind rules in Settings: "never" always stays never.

import { useEffect, useState } from "react";

export type Mode = "show" | "auto" | "manual";

export const MODES: { id: Mode; label: string; hint: string }[] = [
  { id: "show", label: "Show", hint: "Plan first. Jarvis tells you what it would do and changes nothing." },
  { id: "auto", label: "Auto", hint: "Does the work itself. Still asks before sending, deleting, buying or controlling apps." },
  { id: "manual", label: "Manual", hint: "Asks before every change." },
];

const KEY = "jarvis.mode";

function load(): Mode {
  try {
    const m = localStorage.getItem(KEY);
    if (m === "show" || m === "auto" || m === "manual") return m;
  } catch {
    /* not remembered */
  }
  return "manual";
}

let current: Mode = load();
const listeners = new Set<() => void>();

export const getMode = () => current;

export function setMode(m: Mode) {
  current = m;
  try {
    localStorage.setItem(KEY, m);
  } catch {
    /* not remembered */
  }
  listeners.forEach((l) => l());
}

export function nextMode(m: Mode): Mode {
  return MODES[(MODES.findIndex((x) => x.id === m) + 1) % MODES.length].id;
}

export function useMode(): Mode {
  const [, tick] = useState(0);
  useEffect(() => {
    const l = () => tick((n) => n + 1);
    listeners.add(l);
    return () => {
      listeners.delete(l);
    };
  }, []);
  return current;
}

/** Tools that change something or start work. In Show mode they're held back. */
export const CHANGING_TOOLS = new Set([
  "start_research", "research_many", "create_image", "follow_up", "create_document", "edit_document", "browse",
  "calendar_create", "calendar_update", "calendar_delete", "email_draft", "email_send",
  "drive_save", "doc_create", "doc_append", "sheet_create", "sheet_append", "sheet_update",
  "schedule_briefing", "cancel_briefing", "create_routine", "update_routine", "run_routine", "remember_how",
  "start_meeting", "mac_open", "mac_click", "mac_type", "mac_key", "mac_scroll",
  "fix_in_project", "build_project", "replace_selection",
]);
