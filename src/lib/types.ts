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
  kind: "research" | "image" | "document" | "browser";
  images: string[];
  refs: string[];
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
  /** The user's own Google OAuth "Desktop app" client, for Calendar and Gmail. */
  googleClientId: string;
  googleClientSecret: string;
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
}

export interface ChatSummary {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  count: number;
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
