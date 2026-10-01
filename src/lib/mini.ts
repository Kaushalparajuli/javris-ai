// The Mini Jarvis window is a second web view with no Jarvis logic of its own. The main window
// sends it a snapshot of what's going on; it sends back what the user clicked.

import { emit, listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef } from "react";
import { useApprovals, type Approval } from "./approvals";
import type { useJarvis } from "./jarvis";
import type { Connection, OrbMode } from "./types";

export interface MiniState {
  connection: Connection;
  micOn: boolean;
  status: string;
  error: string;
  /// The app Jarvis is clicking and typing in right now, if any.
  controlling: string;
  /// A meeting being recorded, if any.
  meeting: { title: string; startedAt: number } | null;
  /// The newest task still running, with its latest step.
  task: { title: string; step: string; count: number } | null;
  approvals: Approval[];
}

export interface MiniLevel {
  mode: OrbMode;
  level: number;
}

export type MiniCmd =
  | { type: "hello" }
  | { type: "toggle-mic" }
  | { type: "stop-speaking" }
  | { type: "open" }
  | { type: "stop-meeting" }
  | { type: "approve"; id: number; ok: boolean };

/** Runs in the main window: publishes state to the mini window and carries out its clicks. */
export function useMiniBridge(j: ReturnType<typeof useJarvis>) {
  const { pending, answer } = useApprovals();
  const latest = useRef({ j, answer });
  latest.current = { j, answer };

  const running = j.tasks.filter((t) => t.status === "running" && !t.parentId);
  const newest = running[running.length - 1];
  const step = newest?.steps[newest.steps.length - 1];
  const state: MiniState = {
    connection: j.connection,
    micOn: j.micOn,
    status: j.status,
    error: j.error,
    controlling: j.controlling,
    meeting: j.meeting,
    task: newest ? { title: newest.title, step: step?.label ?? "Starting…", count: running.length } : null,
    approvals: pending.map(({ id, risk, title, detail, okLabel }) => ({ id, risk, title, detail, okLabel })),
  };
  const json = JSON.stringify(state);
  const sent = useRef(json);
  sent.current = json;

  useEffect(() => {
    emit("mini-state", JSON.parse(json)).catch(() => {});
  }, [json]);

  // A new approval while the main window isn't in front: bring up the mini window to ask.
  const waiting = useRef(0);
  useEffect(() => {
    if (pending.length > waiting.current && !document.hasFocus()) invoke("mini_show").catch(() => {});
    waiting.current = pending.length;
  }, [pending.length]);

  // The orb's level, about ten times a second while a session is on.
  useEffect(() => {
    if (j.connection !== "live") return;
    const t = setInterval(() => emit("mini-level", { mode: j.orb.mode(), level: j.orb.level() } satisfies MiniLevel).catch(() => {}), 100);
    return () => clearInterval(t);
  }, [j.connection, j.orb]);

  useEffect(() => {
    const un = listen<MiniCmd>("mini-cmd", (e) => {
      const c = e.payload;
      const { j, answer } = latest.current;
      if (c.type === "hello") emit("mini-state", JSON.parse(sent.current)).catch(() => {});
      else if (c.type === "toggle-mic") j.toggleMic();
      else if (c.type === "stop-speaking") j.stopSpeaking();
      else if (c.type === "stop-meeting") j.stopMeeting();
      else if (c.type === "open") invoke("show_main").catch(() => {});
      else if (c.type === "approve") answer(c.id, c.ok);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);
}
