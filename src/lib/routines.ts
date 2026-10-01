// Routines in plain words: what each kind of step means, ready-made routines, and the planner
// that turns "every Monday, check my competitors and email me" into steps.

import { WEEKDAYS } from "./briefings";
import type { KnowHow, Routine, RoutineStep, StepKind } from "./types";

/** How each kind of step is described, and what it's allowed to do. */
export const STEP_INFO: Record<StepKind, { label: string; level: "look" | "prep" | "act"; minutes: { quick: number; deep: number } }> = {
  research: { label: "Looks things up", level: "look", minutes: { quick: 3, deep: 8 } },
  inbox: { label: "Reads your email", level: "look", minutes: { quick: 1, deep: 1 } },
  calendar: { label: "Reads your calendar", level: "look", minutes: { quick: 1, deep: 1 } },
  write: { label: "Prepares a document", level: "prep", minutes: { quick: 2, deep: 3 } },
  email_me: { label: "Emails you, always asks first", level: "act", minutes: { quick: 0, deep: 0 } },
};

/** Names for the kind picker in the fine-tune editor. */
export const KIND_NAMES: Record<StepKind, string> = {
  research: "Look something up",
  write: "Write a document",
  inbox: "Read my recent email",
  calendar: "Read today's calendar",
  email_me: "Email me the result",
};

export const LEVELS = [
  { level: "look", text: "Looks things up" },
  { level: "prep", text: "Prepares drafts and documents" },
  { level: "act", text: "Sends email. Always asks first." },
] as const;

export function minutesFor(r: Pick<Routine, "steps" | "depth">) {
  return Math.max(1, r.steps.reduce((n, s) => n + STEP_INFO[s.kind].minutes[r.depth], 0));
}

/** "Every Monday at 08:00", "Weekdays at 07:30", "When you ask". */
export function routineWhen(r: Pick<Routine, "repeat" | "weekday" | "time">) {
  if (r.repeat === "manual") return "When you ask";
  if (r.repeat === "weekly") return `Every ${WEEKDAYS[r.weekday] ?? "Monday"} at ${r.time}`;
  if (r.repeat === "weekdays") return `Weekdays at ${r.time}`;
  return `Every day at ${r.time}`;
}

export const blankRoutine = (chatId: string): Routine => ({
  id: "",
  title: "",
  request: "",
  steps: [],
  repeat: "manual",
  weekday: 0,
  time: "09:00",
  depth: "quick",
  enabled: false,
  tried: false,
  autoSend: false,
  chatId,
  createdAt: 0,
  lastRun: null,
  lastRunId: "",
  nextRun: null,
  failures: 0,
  pausedReason: "",
});

const step = (kind: StepKind, text: string): RoutineStep => ({ kind, text, knowHow: [] });

export interface Template {
  id: string;
  title: string;
  blurb: string;
  icon: "chart" | "mail" | "calendar" | "tag" | "news";
  /** One question to ask before creating it, if any. */
  ask?: { question: string; placeholder: string };
  make(answer: string): Partial<Routine>;
}

export const TEMPLATES: Template[] = [
  {
    id: "competitors",
    title: "Weekly competitor watch",
    blurb: "Prices and news from companies you name, compared with last week.",
    icon: "chart",
    ask: { question: "Which companies should Jarvis watch?", placeholder: "e.g. Pinecone, Weaviate, Qdrant" },
    make: (a) => ({
      title: "Weekly competitor watch",
      request: `Every Monday morning, check ${a} and email me a short summary.`,
      repeat: "weekly",
      weekday: 0,
      time: "08:00",
      depth: "deep",
      steps: [
        step("research", `Look up the latest prices, product news and announcements for ${a}`),
        step("write", "Write a one-page summary: what changed since last time first, then a short section on each company"),
        step("email_me", "Email the summary to you"),
      ],
    }),
  },
  {
    id: "inbox",
    title: "Morning inbox digest",
    blurb: "What came in overnight, sorted into reply today, this week, or skip.",
    icon: "mail",
    make: () => ({
      title: "Morning inbox digest",
      request: "Every weekday morning, sort my new email so I know what to answer first.",
      repeat: "weekdays",
      time: "07:30",
      depth: "quick",
      steps: [
        step("inbox", "Read the email that came in since yesterday"),
        step("write", "Sort it into reply today, reply this week, and skip, with one line on each email"),
      ],
    }),
  },
  {
    id: "meetings",
    title: "Meeting prep",
    blurb: "Each weekday morning: today's meetings, background, and questions to ask.",
    icon: "calendar",
    make: () => ({
      title: "Meeting prep",
      request: "Every weekday morning, prepare me for today's meetings.",
      repeat: "weekdays",
      time: "08:00",
      depth: "quick",
      steps: [
        step("calendar", "Check today's meetings"),
        step("research", "Look up background on the companies and topics in those meetings"),
        step("write", "Write a short prep note for each meeting: what it's about, useful background, and questions to ask"),
      ],
    }),
  },
  {
    id: "price",
    title: "Price check",
    blurb: "Checks a product's price every day and notes when it drops.",
    icon: "tag",
    ask: { question: "What should Jarvis check the price of?", placeholder: "e.g. Sony WH-1000XM6 headphones" },
    make: (a) => ({
      title: `Price check: ${a}`.slice(0, 60),
      request: `Every day, check the price of ${a} and tell me when it drops.`,
      repeat: "daily",
      time: "09:00",
      depth: "quick",
      steps: [
        step("research", `Check today's price of ${a} at the main shops that sell it`),
        step("write", "Write a short note with the best price today, where to buy it, and whether it went down since last time"),
      ],
    }),
  },
  {
    id: "news",
    title: "Weekly news on a topic",
    blurb: "Five stories worth reading, each in two sentences, with links.",
    icon: "news",
    ask: { question: "Which topic?", placeholder: "e.g. AI regulation in Asia" },
    make: (a) => ({
      title: `Weekly news: ${a}`.slice(0, 60),
      request: `Every Friday afternoon, send me the week's most important news about ${a}.`,
      repeat: "weekly",
      weekday: 4,
      time: "17:00",
      depth: "quick",
      steps: [
        step("research", `Find the five most important stories this week about ${a}`),
        step("write", "Summarize each story in two sentences, with a link to read more"),
        step("email_me", "Email it to you"),
      ],
    }),
  },
];

/** How a routine is planned. Shared by the typed planner and the voice model's create_routine tool. */
export const PLANNING_RULES = `A routine is 1 to 5 short steps, each one of these kinds:
- research: look something up on the web (prices, news, facts). The worker searches and writes a report with sources.
- write: write a document from what earlier steps found (a summary, a digest, a note). It doesn't browse the web.
- inbox: read the user's email from the last day. Put it before a write step that uses it.
- calendar: read the user's calendar for today. Put it before steps that use it.
- email_me: email the result to the user. Only as the last step, and only if they asked to be emailed or sent something. The app always asks them before sending.
Write each step's text as a short plain sentence a non-technical person understands, starting with a verb ("Look up…", "Write…").
Usually finish with a write step so there's a clear result. Don't add steps the user didn't need.
Schedule: repeat is manual (only when asked), daily, weekdays or weekly; time is local 24-hour HH:MM; weekday is 0 = Monday … 6 = Sunday. "Morning" means 08:00 unless they said otherwise.
Depth: deep for thorough analysis or comparisons, quick otherwise.`;

const API = "https://generativelanguage.googleapis.com/v1beta";

let plannerModels: string[] = [];

/** Fast text models this key can use, newest first. */
async function pickModels(apiKey: string) {
  if (plannerModels.length) return plannerModels;
  const res = await fetch(`${API}/models?pageSize=1000&key=${encodeURIComponent(apiKey)}`);
  if (!res.ok) throw new Error("Jarvis couldn't reach Google with your Gemini key.");
  const data = await res.json();
  const names: string[] = (data.models ?? [])
    .filter((m: { supportedGenerationMethods?: string[] }) => m.supportedGenerationMethods?.includes("generateContent"))
    .map((m: { name: string }) => m.name.replace(/^models\//, ""))
    .filter((n: string) => /flash/.test(n) && !/image|tts|audio|live|embed|vision|thinking/.test(n));
  const score = (n: string) => parseFloat(n.match(/(\d+(\.\d+)?)/)?.[1] ?? "0") * 10 - (/lite/.test(n) ? 4 : 0) - (/preview|exp/.test(n) ? 2 : 0) + (/latest/.test(n) ? 1 : 0);
  plannerModels = names.sort((a, b) => score(b) - score(a)).slice(0, 4);
  if (!plannerModels.length) plannerModels = ["gemini-2.5-flash"];
  return plannerModels;
}

const SCHEMA = {
  type: "OBJECT",
  properties: {
    title: { type: "STRING", description: "A short name, 2-5 words." },
    question: { type: "STRING", description: "One short question, only if something essential is missing (like which companies to watch). Otherwise empty." },
    steps: {
      type: "ARRAY",
      items: {
        type: "OBJECT",
        properties: {
          kind: { type: "STRING", enum: ["research", "write", "inbox", "calendar", "email_me"] },
          text: { type: "STRING" },
          know_how: { type: "ARRAY", items: { type: "STRING" }, description: "Names of know-how from the list that fit this step." },
        },
        required: ["kind", "text"],
      },
    },
    repeat: { type: "STRING", enum: ["manual", "daily", "weekdays", "weekly"] },
    time: { type: "STRING" },
    weekday: { type: "INTEGER" },
    depth: { type: "STRING", enum: ["quick", "deep"] },
  },
  required: ["title", "steps", "repeat", "depth"],
};

export interface Plan {
  question: string;
  routine: Partial<Routine>;
}

/**
 * Plan a routine from the user's words, or change one ("make it Fridays", "add Milvus").
 * Uses the same Gemini key as the voice, with a fast text model.
 */
export async function planRoutine(apiKey: string, request: string, knowHow: KnowHow[], current?: Routine): Promise<Plan> {
  if (!apiKey) throw new Error("Add your Gemini key in Set up first.");
  const models = await pickModels(apiKey);
  const know = knowHow.filter((k) => !k.draft).map((k) => `- ${k.slug}: ${k.name}. ${k.description}`).join("\n");
  const system = `You turn a person's request into a routine that an assistant called Jarvis runs for them.\n\n${PLANNING_RULES}\n\n${
    know ? `Know-how Jarvis has (attach by name to the steps it fits):\n${know}` : "Jarvis has no know-how yet, so leave know_how empty."
  }\n\nToday is ${new Date().toLocaleDateString("en-GB", { weekday: "long", day: "numeric", month: "long", year: "numeric" })}.`;
  const user = current
    ? `This is the routine now:\n${JSON.stringify({ title: current.title, steps: current.steps.map((s) => ({ kind: s.kind, text: s.text, know_how: s.knowHow })), repeat: current.repeat, time: current.time, weekday: current.weekday, depth: current.depth })}\n\nChange it like this, keeping everything else the same: "${request}"`
    : `Request: "${request}"`;
  const body = JSON.stringify({
    systemInstruction: { parts: [{ text: system }] },
    contents: [{ role: "user", parts: [{ text: user }] }],
    generationConfig: { responseMimeType: "application/json", responseSchema: SCHEMA, temperature: 0.2 },
  });
  // A busy or retired model shouldn't stop planning: fall back to the next one.
  let data: any = null;
  let problem = "";
  for (const model of models) {
    const res = await fetch(`${API}/models/${model}:generateContent?key=${encodeURIComponent(apiKey)}`, { method: "POST", headers: { "Content-Type": "application/json" }, body });
    if (res.ok) {
      data = await res.json();
      break;
    }
    problem = (await res.json().catch(() => ({})))?.error?.message || `Google returned ${res.status}.`;
    if (![404, 429, 500, 503].includes(res.status)) break;
  }
  if (!data) throw new Error(problem.includes("high demand") ? "Google's planning service is busy right now. Try again in a minute." : problem);
  const text: string = data.candidates?.[0]?.content?.parts?.map((p: { text?: string }) => p.text ?? "").join("") ?? "";
  let raw: Record<string, unknown>;
  try {
    raw = JSON.parse(text);
  } catch {
    throw new Error("Jarvis couldn't make a plan from that. Try saying it another way.");
  }
  return { question: String(raw.question ?? "").trim(), routine: fromPlan(raw, knowHow) };
}

/** Turn a plan from either model into routine fields, dropping anything that doesn't fit. */
export function fromPlan(raw: Record<string, unknown>, knowHow: KnowHow[]): Partial<Routine> {
  const known = new Set(knowHow.filter((k) => !k.draft).map((k) => k.slug));
  const kinds = Object.keys(STEP_INFO) as StepKind[];
  let steps: RoutineStep[] = (Array.isArray(raw.steps) ? raw.steps : [])
    .map((s: Record<string, unknown>) => ({
      kind: (kinds.includes(s?.kind as StepKind) ? s.kind : "research") as StepKind,
      text: String(s?.text ?? "").trim(),
      knowHow: (Array.isArray(s?.know_how) ? s.know_how : Array.isArray(s?.knowHow) ? s.knowHow : []).map(String).filter((k: string) => known.has(k)),
    }))
    .filter((s) => s.text)
    .slice(0, 8);
  // Emailing the user only makes sense once, at the end.
  const email = steps.find((s) => s.kind === "email_me");
  steps = steps.filter((s) => s.kind !== "email_me");
  if (email) steps.push(email);
  const repeat = ["manual", "daily", "weekdays", "weekly"].includes(String(raw.repeat)) ? (raw.repeat as Routine["repeat"]) : "manual";
  const time = /^\d{1,2}:\d{2}$/.test(String(raw.time ?? "")) ? String(raw.time).padStart(5, "0") : "09:00";
  let weekday = Number(raw.weekday);
  if (typeof raw.weekday === "string") weekday = WEEKDAYS.findIndex((d) => d.toLowerCase() === String(raw.weekday).toLowerCase());
  return {
    title: String(raw.title ?? "").trim().slice(0, 60),
    steps,
    repeat,
    time,
    weekday: weekday >= 0 && weekday <= 6 ? weekday : 0,
    depth: raw.depth === "deep" ? "deep" : "quick",
  };
}
