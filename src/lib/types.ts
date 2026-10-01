export type TaskStatus = "running" | "done" | "failed" | "cancelled";

export interface Step {
  id: string;
  label: string;
  detail: string;
  done: boolean;
}

export interface Task {
  id: number;
  title: string;
  request: string;
  depth: "quick" | "deep";
  status: TaskStatus;
  steps: Step[];
  summary: string;
  error: string;
  dir: string;
  threadId: string;
  parentId: number | null;
  chatId: string;
  /** For a document opened from a file: that file's path, so edits can be saved back. */
  source: string;
  startedAt: number;
  finishedAt: number | null;
  searches: number;
  sources: number;
  kind: "research" | "image" | "document" | "browser" | "skill" | "code";
  images: string[];
  refs: string[];
  /** For a step of a routine: the run it belongs to. */
  routine: string;
  /** For a code task: the project folder it works in. */
  project?: string;
  /** For a code task whose project has checks: running, passed or failed. */
  verify?: "" | "running" | "passed" | "failed";
}

export interface Settings {
  geminiApiKey: string;
  geminiModel: string;
  voice: string;
  language: string;
  userName: string;
  codexPath: string;
  researchDir: string;
  codexModel: string;
  codexReasoning: string;
  webSearch: boolean;
  headphones: boolean;
  micDevice: string;
  notify: boolean;
  /** Listen for "hey Jarvis" on this computer. */
  wakeWord: boolean;
  /** How to treat each kind of action: "ask", "auto" or "never". See lib/approvals.ts. */
  approvals: Record<string, string>;
}

export interface CodexStatus {
  found: boolean;
  path: string;
  version: string;
  loggedIn: boolean;
  message: string;
}

export type Who = "you" | "jarvis" | "tool" | "notice";

export interface Msg {
  id: number;
  who: Who;
  text: string;
}

export interface Chat {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  messages: Msg[];
  /** Slug of the project (workspace) it belongs to, or "". */
  workspace: string;
  pinned: boolean;
}

export interface ChatSummary {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  count: number;
  workspace: string;
  pinned: boolean;
  /** The user renamed it, so Jarvis keeps the name. */
  titleLocked: boolean;
}

export type OrbMode = "idle" | "listen" | "speak" | "think";
export type Connection = "off" | "connecting" | "live" | "error";

export interface ReasoningLevel {
  effort: string;
  description: string;
}

export interface CodexModel {
  slug: string;
  displayName: string;
  description: string;
  defaultReasoning: string;
  reasoningLevels: ReasoningLevel[];
}

export interface CodexModels {
  models: CodexModel[];
  defaultModel: string;
  defaultReasoning: string;
  error: string;
}

export interface Attachment {
  name: string;
  path: string;
}

export interface MicDevice {
  name: string;
  isDefault: boolean;
}

export interface Briefing {
  id: string;
  title: string;
  /** What each run should research. */
  request: string;
  repeat: "daily" | "weekdays" | "weekly";
  /** For weekly briefings: 0 = Monday … 6 = Sunday. */
  weekday: number;
  /** Local time, "HH:MM". */
  time: string;
  depth: "quick" | "deep";
  chatId: string;
  enabled: boolean;
  createdAt: number;
  lastRun: number | null;
  lastTask: number | null;
  nextRun: number | null;
}

export type StepKind = "research" | "write" | "inbox" | "calendar" | "email_me";

export interface RoutineStep {
  kind: StepKind;
  /** What the step does, in plain words. */
  text: string;
  /** Know-how (folder names) the step follows. */
  knowHow: string[];
}

export interface Routine {
  id: string;
  title: string;
  /** What the user asked for, in their words. */
  request: string;
  steps: RoutineStep[];
  repeat: "manual" | "daily" | "weekdays" | "weekly";
  /** For weekly routines: 0 = Monday … 6 = Sunday. */
  weekday: number;
  /** Local time, "HH:MM". */
  time: string;
  depth: "quick" | "deep";
  /** Runs on its schedule. Only possible after one successful try. */
  enabled: boolean;
  tried: boolean;
  /** Send the result email without asking (it only goes to the user). */
  autoSend: boolean;
  chatId: string;
  createdAt: number;
  lastRun: number | null;
  lastRunId: string;
  nextRun: number | null;
  failures: number;
  /** Why it paused itself. Empty when it's fine. */
  pausedReason: string;
}

export interface RunStep {
  kind: StepKind;
  text: string;
  status: "waiting" | "running" | "done" | "failed" | "skipped" | "asking";
  taskId: number | null;
  note: string;
}

export interface Run {
  id: string;
  routineId: string;
  title: string;
  number: number;
  status: "running" | "asking" | "done" | "failed" | "cancelled";
  steps: RunStep[];
  dir: string;
  resultTask: number | null;
  summary: string;
  approval: { to: string; subject: string; body: string; draftId: string } | null;
  error: string;
  startedAt: number;
  finishedAt: number | null;
}

export interface KnowHow {
  slug: string;
  name: string;
  /** When to use it, in one sentence. */
  description: string;
  /** The steps to follow. */
  body: string;
  /** Written by Jarvis and not reviewed yet. */
  draft: boolean;
  updatedAt: number;
  files: string[];
}

export interface Workspace {
  slug: string;
  name: string;
  description: string;
  folder: string;
  verify: string[];
  /** How Jarvis should work in every chat in the project. */
  instructions: string;
  url: string;
  created: number;
}

export interface ProjectFile {
  name: string;
  size: number;
  /** Seconds since 1970. */
  modified: number;
}

export interface Memory {
  id: number;
  kind: string;
  text: string;
  workspace: string;
  source: string;
  created: number;
  pinned: boolean;
}
