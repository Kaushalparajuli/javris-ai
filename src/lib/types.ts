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
  kind: "research" | "image" | "document" | "browser" | "skill" | "code" | "slides";
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
  /** API keys for outside services videos can use, by service id ("elevenlabs"). */
  serviceKeys: Record<string, string>;
  /** Who reads video narration: "auto", "gemini" or "elevenlabs". */
  narrationProvider: string;
  narrationGeminiVoice: string;
  elevenlabsVoice: string;
  elevenlabsModel: string;
  /** The owner's Telegram bot token (from @BotFather). */
  telegramToken: string;
  /** The one Telegram chat Jarvis listens to; 0 when not paired. */
  telegramChatId: number;
  /** Listen for Telegram messages. */
  telegramEnabled: boolean;
  /** Apple's echo cancellation on the microphone, so Jarvis can be interrupted on speakers (beta). */
  echoCancellation: boolean;
  /** Meetings also record what the Mac plays: the other side of a call. */
  meetingSystemAudio: boolean;
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
  /** Why the last scheduled run couldn't start; empty when it did. */
  lastError?: string;
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
  isDir: boolean;
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

// ---- video ----

export interface VideoStatus {
  ready: boolean;
  /** The speech model for exact caption timing is installed (optional). */
  captions: boolean;
  tools: boolean;
  browser: string;
  message: string;
}

export interface HeygenStatus {
  installed: boolean;
  signedIn: boolean;
  message: string;
}

export interface VideoMeta {
  title: string;
  format: "landscape" | "portrait" | "square";
  width: number;
  height: number;
  seconds: number;
}

export interface VideoRender {
  path: string;
  name: string;
  size: number;
  modified: number;
}

export interface VideoInfo {
  isVideo: boolean;
  meta: VideoMeta;
  renders: VideoRender[];
}

/** Where a video is in the making, kept per project folder (see videopipe.rs). */
export interface VideoProgress {
  stage: "plan" | "storyboard" | "media" | "compose" | "check" | "fix" | "render" | "done" | "error" | "task";
  message: string;
  /** The storyboard waiting for an answer, in Markdown. */
  board?: string;
  /** Everything said so far, oldest first. */
  log: string[];
  render?: { percent: number; message: string };
  error?: string;
  note?: string;
}
