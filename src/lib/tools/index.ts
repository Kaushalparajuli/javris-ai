// Voice tools that live in their own files. Each module declares its tools for Gemini, names the
// ones that change something (Show mode holds those back), and runs its own calls. jarvis.ts asks
// these modules about any tool name it doesn't handle itself.

import type { Risk } from "../approvals";
import type { FunctionCall } from "../live";
import type { Settings } from "../types";
import { slidesTools } from "./slides";
import { sources } from "./sources";

export interface ToolContext {
  /** What to call the user in replies to Gemini. */
  name: string;
  settings: Settings | null;
  chatId: string;
  /** Add a line to the transcript, e.g. push("tool", "→ sec_filings · Apple"). */
  push(who: "tool" | "jarvis" | "notice", text: string): void;
  /** Ask on screen first (follows the mode and the Settings rule for this kind of action). */
  approve(req: { tool: string; risk: Risk; title: string; detail: string; okLabel?: string }): Promise<{ ok: boolean; decision: string }>;
  /** The reply to hand Gemini when an approval was declined or blocked. */
  declined(decision: string, consequence: string): object;
  /** Queue a "[WORKER] …" notice that Jarvis speaks at the next pause. */
  notify(text: string): void;
  /** Show a task (report, document, images) in the side panel. */
  openTask(id: number): void;
  /** Paths of the files the user attached to the window (logos, photos), if any. */
  attachments?(): string[];
}

export interface ToolModule {
  declarations: object[];
  /** Tool names that change something or start work. */
  changing: string[];
  /** Run the call, or return null when the tool isn't this module's. */
  run(fc: FunctionCall, ctx: ToolContext): Promise<object> | null;
}

const MODULES: ToolModule[] = [sources, slidesTools];

export const extraTools = (): object[] => MODULES.flatMap((m) => m.declarations);

export const extraChanging = (name: string) => MODULES.some((m) => m.changing.includes(name));

export async function runExtraTool(fc: FunctionCall, ctx: ToolContext): Promise<object | null> {
  for (const m of MODULES) {
    const r = m.run(fc, ctx);
    if (r) return await r;
  }
  return null;
}
