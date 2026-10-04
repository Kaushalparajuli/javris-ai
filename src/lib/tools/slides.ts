// Slide decks: the worker builds a real PowerPoint file (deck.pptx) with pptxgenjs in its own
// folder, and Jarvis can upload it to Google Drive, where it opens as Google Slides.

import { invoke } from "@tauri-apps/api/core";
import type { Task } from "../types";
import type { ToolModule } from "./index";

const declarations = [
  {
    name: "create_slides",
    description:
      "Make a slide deck (a presentation, pitch deck, investor update, lecture slides) as a real PowerPoint file. The worker designs it: a title slide, short bullets, charts from real numbers, speaker notes. It takes a few minutes and shows as a task. To build it from earlier research or a document, pass that task's number as source_task_id. Not for a document (that is a document) or a website.",
    parameters: {
      type: "OBJECT",
      properties: {
        title: { type: "STRING", description: "The deck's title, 2-8 words." },
        brief: {
          type: "STRING",
          description: "Everything the designer needs: the goal, the audience, the story slide by slide if the user gave one, the real names and numbers, tone and colours.",
        },
        slides: { type: "INTEGER", description: "How many slides, 3 to 30. Default 8." },
        source_task_id: { type: "INTEGER", description: "Task number of earlier research or a document to build the deck from, only when the user points at one ('from the Acme research')." },
        research: { type: "BOOLEAN", description: "true to let the designer look things up on the web. Leave out when the facts come from the user or a source task." },
        use_attachments: { type: "BOOLEAN", description: "true to give the designer the images the user attached (logo, photos)." },
      },
      required: ["title", "brief"],
    },
  },
  {
    name: "edit_slides",
    description: "Change a slide deck that was already made (add a slide, shorten bullets, new colours, fix a number). Use this for any change to an existing deck, never create_slides.",
    parameters: {
      type: "OBJECT",
      properties: {
        instructions: { type: "STRING", description: "Exactly what to change." },
        task_id: { type: "INTEGER", description: "The deck's task number. Leave out for the latest deck." },
      },
      required: ["instructions"],
    },
  },
  {
    name: "slides_to_google",
    description: "Upload a finished slide deck to the user's Google Drive, where it opens as Google Slides, and get its link. The app asks the user to approve first.",
    parameters: {
      type: "OBJECT",
      properties: { task_id: { type: "INTEGER", description: "The deck's task number. Leave out for the latest finished deck." } },
    },
  },
];

/** The deck a call means: the one named, or the newest. */
async function findDeck(id: unknown, finished: boolean): Promise<Task | null> {
  const tasks = await invoke<Task[]>("list_tasks");
  const decks = tasks.filter((t) => t.kind === "slides");
  if (Number.isInteger(id)) return decks.find((t) => t.id === id) ?? null;
  return decks.find((t) => !finished || t.status === "done") ?? null;
}

export const slidesTools: ToolModule = {
  declarations,
  changing: ["create_slides", "edit_slides", "slides_to_google"],
  run(fc, ctx) {
    const a = fc.args;
    switch (fc.name) {
      case "create_slides":
        return (async () => {
          const title = String(a.title ?? "Slides").trim() || "Slides";
          const brief = String(a.brief ?? title);
          const slides = Math.max(3, Math.min(30, Number(a.slides) || 8));
          const source = Number.isInteger(a.source_task_id) ? Number(a.source_task_id) : null;
          const refs = a.use_attachments ? (ctx.attachments?.() ?? []) : [];
          const r = await ctx.approve({
            tool: fc.name,
            risk: "write",
            title: `Make the slide deck “${title}”`,
            detail: `${brief}\n\n${slides} slides${source ? `, built from task #${source}` : ""}${refs.length ? `, with ${refs.length} attached image${refs.length > 1 ? "s" : ""}` : ""}. The worker saves deck.pptx in its own folder and touches nothing else.`,
            okLabel: "Make it",
          });
          if (!r.ok) return ctx.declined(r.decision, "no deck was started");
          try {
            const t = await invoke<Task>("start_slides", { title, brief, slides, sourceTask: source, attachments: refs, research: a.research === true, chatId: ctx.chatId });
            ctx.push("tool", `→ create_slides · ${title} · ${slides} slides · task #${t.id}`);
            ctx.openTask(t.id);
            return { task_id: t.id, status: "started", note: "The designer is building the deck; it usually takes a few minutes. You'll get a [WORKER] notice when deck.pptx is ready. Say one short sentence." };
          } catch (e) {
            return { error: String(e) };
          }
        })();
      case "edit_slides":
        return (async () => {
          const deck = await findDeck(a.task_id, false);
          if (!deck) return { error: "There's no slide deck to change yet. Make one first." };
          const instructions = String(a.instructions ?? "").trim();
          if (!instructions) return { error: "Say what should change." };
          const r = await ctx.approve({ tool: fc.name, risk: "write", title: `Change the deck “${deck.title}”`, detail: instructions, okLabel: "Change it" });
          if (!r.ok) return ctx.declined(r.decision, "the deck wasn't changed");
          try {
            const t = await invoke<Task>("slides_edit", { id: deck.id, instructions, chatId: ctx.chatId });
            ctx.push("tool", `→ edit_slides · task #${t.id}`);
            ctx.openTask(t.id);
            return { task_id: t.id, status: "editing", note: "The deck is being changed. You'll get a [WORKER] notice when it's done." };
          } catch (e) {
            return { error: String(e) };
          }
        })();
      case "slides_to_google":
        return (async () => {
          const deck = await findDeck(a.task_id, true);
          if (!deck) return { error: "There's no finished slide deck to upload yet." };
          const r = await ctx.approve({
            tool: fc.name,
            risk: "send",
            title: `Upload “${deck.title}” to Google Slides`,
            detail: "The deck (deck.pptx) is sent to your Google Drive and turned into Google Slides. Only you can see it until you share it.",
            okLabel: "Upload",
          });
          if (!r.ok) return ctx.declined(r.decision, "the deck wasn't uploaded");
          try {
            const up = await invoke<{ id: string; link: string }>("slides_upload", { taskId: deck.id });
            ctx.push("tool", `→ slides_to_google · ${up.link}`);
            return { status: "uploaded", link: up.link, note: "The deck is in Google Drive as Google Slides; the link is on screen. Don't read the link aloud." };
          } catch (e) {
            return { error: String(e) };
          }
        })();
      default:
        return null;
    }
  },
};
