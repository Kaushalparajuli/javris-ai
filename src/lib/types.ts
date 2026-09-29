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
  startedAt: number;
  finishedAt: number | null;
  searches: number;
  sources: number;
  kind: "research" | "image";
  images: string[];
  refs: string[];
}

export interface Settings {
  geminiApiKey: string;
  geminiModel: string;
  voice: string;
  userName: string;
  codexPath: string;
  researchDir: string;
  codexModel: string;
  codexReasoning: string;
  webSearch: boolean;
  headphones: boolean;
  micDevice: string;
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
