// One gate for actions that change something outside Jarvis: write files, send mail, and later
// delete, buy, deploy or control apps. Every tool says what kind of action it is; the user's rule
// for that kind decides whether it runs on its own, waits for a click, or never runs.
//
// Approvals are always answered on screen by a click, never by voice, so nothing in an email, a
// web page or the model's own mistake can approve an action by speaking or writing the words.
// Every decision is appended to ~/Jarvis/audit.jsonl.

import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { getMode } from "./mode";
import type { Settings } from "./types";

export type Risk = "read" | "search" | "write" | "send" | "delete" | "purchase" | "deploy" | "control";
export type Rule = "ask" | "auto" | "never";

export const RISK_INFO: Record<Risk, { label: string; hint: string; rules: Rule[] }> = {
  read: { label: "Read", hint: "Looking at your calendar, mail or screen", rules: ["auto"] },
  search: { label: "Search", hint: "Searching the web", rules: ["auto"] },
  write: { label: "Change files", hint: "Code fixes in your projects, pasting text into other apps", rules: ["ask", "auto", "never"] },
  send: { label: "Send", hint: "Emails and calendar invitations to other people", rules: ["ask", "never"] },
  delete: { label: "Delete", hint: "Removing files or data", rules: ["ask", "never"] },
  purchase: { label: "Buy", hint: "Anything that costs money", rules: ["ask", "never"] },
  deploy: { label: "Deploy", hint: "Publishing to a live site or server", rules: ["ask", "never"] },
  control: { label: "Control apps", hint: "Clicking and typing in other apps", rules: ["ask", "never"] },
};

export const DEFAULT_RULE: Record<Risk, Rule> = {
  read: "auto",
  search: "auto",
  write: "ask",
  send: "ask",
  delete: "ask",
  purchase: "ask",
  deploy: "ask",
  control: "ask",
};

/**
 * The rule that applies right now: the Settings rule for this kind of action, adjusted by the mode
 * under the prompt box. Manual asks for every change; Auto lets file and document changes through;
 * Show lets nothing that changes anything through. A rule of "never" always holds.
 */
export function effectiveRule(risk: Risk, settings: Settings | null): Rule {
  const base = ruleFor(risk, settings);
  if (risk === "read" || risk === "search" || base === "never") return base;
  const mode = getMode();
  if (mode === "show") return "never";
  if (mode === "manual") return "ask";
  return risk === "write" ? "auto" : base;
}

/** The rule in force for a kind of action. Anything the setting can't allow falls back to asking. */
export function ruleFor(risk: Risk, settings: Settings | null): Rule {
  const set = settings?.approvals?.[risk] as Rule | undefined;
  const allowed = RISK_INFO[risk].rules;
  return set && allowed.includes(set) ? set : DEFAULT_RULE[risk];
}

export interface Approval {
  id: number;
  risk: Risk;
  /** What Jarvis wants to do, one line: "Fix the build error in Jarvis". */
  title: string;
  /** The specifics worth reading before agreeing: recipient, folder, text to paste. */
  detail: string;
  okLabel: string;
}

export type Decision = "approved" | "declined" | "auto" | "blocked";

interface Pending extends Approval {
  resolve: (ok: boolean) => void;
}

let seq = 0;
let queue: Pending[] = [];
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((l) => l());

function audit(entry: Record<string, unknown>) {
  invoke("audit_log", { entry }).catch(() => {});
}

/** Record something that ran without asking, so the trail is complete. */
export function auditAuto(tool: string, risk: Risk, summary: string) {
  if (risk === "read" || risk === "search") return;
  audit({ tool, risk, decision: "auto", summary });
}

/**
 * Ask whether an action may go ahead. Resolves true to proceed. Honours the rule for its kind:
 * "auto" proceeds, "never" refuses, "ask" waits for a click on the approval card.
 */
export function requestApproval(
  req: { tool: string; risk: Risk; title: string; detail: string; okLabel?: string },
  settings: Settings | null,
): Promise<{ ok: boolean; decision: Decision }> {
  const rule = effectiveRule(req.risk, settings);
  const log = (decision: Decision) => audit({ tool: req.tool, risk: req.risk, decision, summary: req.title, detail: req.detail.slice(0, 600) });
  if (rule === "never") {
    log("blocked");
    return Promise.resolve({ ok: false, decision: "blocked" });
  }
  if (rule === "auto") {
    log("auto");
    return Promise.resolve({ ok: true, decision: "auto" });
  }
  return new Promise((done) => {
    let closeRemote: ((ok: boolean) => void) | void;
    const item: Pending = {
      id: ++seq,
      risk: req.risk,
      title: req.title,
      detail: req.detail,
      okLabel: req.okLabel ?? "Approve",
      resolve: (ok) => {
        // The first answer wins, on screen or remote; later ones are ignored.
        if (!queue.some((p) => p.id === item.id)) return;
        queue = queue.filter((p) => p.id !== item.id);
        changed();
        closeRemote?.(ok);
        log(ok ? "approved" : "declined");
        done({ ok, decision: ok ? "approved" : "declined" });
      },
    };
    queue = [...queue, item];
    changed();
    closeRemote = remoteApprover?.(item, (ok) => item.resolve(ok));
    invoke("notify_approval", { title: "Jarvis needs your OK", body: req.title }).catch(() => {});
  });
}

/**
 * Somewhere else the owner can answer an approval with a click (their phone, through Telegram).
 * It gets each approval that waits for a click and a way to answer it, and may return a function
 * that is called once the approval is answered either way. The card on screen still works.
 */
export type RemoteApprover = (approval: Approval, answer: (ok: boolean) => void) => ((ok: boolean) => void) | void;
let remoteApprover: RemoteApprover | null = null;
export function setRemoteApprover(fn: RemoteApprover | null) {
  remoteApprover = fn;
}

/** The approvals waiting for a click, oldest first. */
export function useApprovals() {
  const [, tick] = useState(0);
  useEffect(() => {
    const l = () => tick((n) => n + 1);
    listeners.add(l);
    return () => {
      listeners.delete(l);
    };
  }, []);
  return {
    pending: queue as Approval[],
    answer: (id: number, ok: boolean) => queue.find((p) => p.id === id)?.resolve(ok),
  };
}

/** Anything still waiting when a session ends counts as declined. */
export function declineAll() {
  [...queue].forEach((p) => p.resolve(false));
}
