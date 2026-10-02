// The visual director. After Jarvis builds a website it looks at the result the way a design
// director would: the page is rendered at desktop, tablet and phone sizes (see director.rs), a
// vision model reviews the screenshots together with the measured layout facts, and the issues it
// finds go back to the code worker to fix. This repeats until the page is good or the rounds run
// out, and what was learned is written into the website-design skill so the next site starts better.

import { invoke } from "@tauri-apps/api/core";
import { API, pickModels } from "./routines";

export type Severity = "high" | "medium" | "low";

export interface Issue {
  id: string;
  severity: Severity;
  viewport: "desktop" | "tablet" | "phone" | "all";
  /** Where on the page: "hero", "services cards", "footer". */
  area: string;
  /** alignment, spacing, overflow, hierarchy, contrast, imagery, typography, responsive, motion, content */
  kind: string;
  problem: string;
  /** A concrete fix, with a selector and values where possible. */
  fix: string;
}

export interface Review {
  /** 0 to 100. */
  score: number;
  verdict: "ship" | "fix";
  summary: string;
  issues: Issue[];
  strengths: string[];
  /** General rules this review teaches, written for the next build. */
  lessons: string[];
}

export interface Shot {
  viewport: string;
  width: number;
  index: number;
  y: number;
  height: number;
  path: string;
}
export interface ViewportReport {
  name: string;
  width: number;
  pageHeight: number;
  audit: Record<string, unknown>;
}
export interface Capture {
  shots: Shot[];
  viewports: ViewportReport[];
}

export interface Round {
  n: number;
  at: number;
  /** The screenshots reviewed (kept on disk, shown in the Review tab). */
  shots: Shot[];
  pageHeights: Record<string, number>;
  review: Review | null;
  /** The worker task that fixed this round's issues, if any. */
  fixTaskId: number | null;
  note?: string;
}

export type DirectorStatus = "capturing" | "reviewing" | "fixing" | "done" | "error";

export interface DirectorState {
  taskId: number;
  folder: string;
  title: string;
  status: DirectorStatus;
  /** The round in progress (1-based). */
  round: number;
  maxRounds: number;
  rounds: Round[];
  message: string;
  startedAt: number;
  finishedAt: number | null;
  lessonsAdded: number;
}

export const MAX_ROUNDS = 3;

// ---------- what the model is asked ----------

export const DIRECTOR_RULES = `You are a senior visual director and art director reviewing a freshly built website before it ships. You are shown screenshots of the real rendered page at desktop (1440px), tablet (820px) and phone (390px) widths, one screenful each, in scroll order, plus measurements the browser took of the layout. Judge it the way a demanding design director would, then give the developer a precise list of what to fix.

What to look at, in this order:
1. Hierarchy and first impression: is the main message and the main action obvious within two seconds, above the fold, at each size?
2. Alignment: do headings, text blocks, cards and images share the same left edges and the same column grid? Is anything off by a few pixels, floating, or sticking out past the container or the screen edge?
3. Spacing and rhythm: is vertical spacing consistent (an 8px scale) between sections, inside cards, between heading and text? Flag cramped areas and flag big empty dead zones (a card with a void in the middle, a heading far from the paragraph it introduces, a section with half its height unused).
4. Typography: headlines wrapping to four or five lines or leaving one lone word on the last line; line lengths over about 75 characters; text under 12px; weak size steps between heading levels; inconsistent label styles.
5. Imagery and visuals: pictures, mockups or 3D scenes that are cropped awkwardly, clipped by their container, overlapping text, blank, broken, stretched, or placed with no relation to the text beside them.
6. Collisions: text on text, text on busy imagery, badges or labels covering content, sticky headers hiding content.
7. Contrast and legibility: coloured text on coloured backgrounds, grey on lime, thin text on images (the measurements list measured contrast ratios; under 4.5 for body text is a failure).
8. Consistency: the same component should have the same padding, radius, border and type everywhere.
9. Responsiveness: tablet and phone are not just narrower desktops. Check wrapping, stacking order, tap targets (at least 40px), whether images and cards fit, and whether anything overflows sideways.
10. Motion end-states: content that is still invisible, half-faded or off-position in a screenshot (a scroll animation that never finished) is a bug. 3D or canvas areas that are empty are a bug.

Rules for your answer:
- Be specific and visible. Say where ("the second service card, desktop"), what is wrong, and a concrete fix with a CSS selector, property and value when the measurements or the picture let you name them ("h1#hero-title: remove max-width:10ch, font-size clamp(3.6rem, 6vw, 6rem), text-wrap: balance").
- Do not redesign. Keep the concept, copy, colours and structure. Fix execution, not taste. Do not ask for new sections or new content.
- Severity: "high" = looks broken or unprofessional, or hurts the main message or action (overflow, overlap, empty visual, unreadable text, headline wrapped to five lines, main button below the fold on desktop). "medium" = visibly off but not broken (uneven spacing, dead zone, misalignment, weak contrast above 3:1, cropped mockup). "low" = polish.
- At most 12 issues, most serious first. One issue per root cause: do not list the same cause for every card or every size separately; say "all" for viewport when it applies at every size.
- The layout measurements are facts. Overlaps reported for collapsed accordion answers, items inside a closed mobile menu, and 3D canvases reading as blank in a measurement are usually not real: trust the pictures.
- Also list up to 4 real strengths, so good work isn't undone.
- "lessons": up to 5 general rules this review teaches (not about this site's content): imperative sentences a developer could follow next time, for example "Never cap a large heading's max-width below the width its text needs; ch limits under 12 force five-line headlines."
- score: 0 to 100 for how ready this is to ship. 90+ means a director would sign off. verdict "ship" only if there are no high issues and no more than two medium issues.
- If you are given the previous round's issues, check each against the new screenshots. Report an issue again only if it is still visible, and start its problem with "Still: ". Do not invent new low-value issues to fill the list; a page that is fixed should score well.`;

const ISSUE = {
  type: "OBJECT",
  properties: {
    severity: { type: "STRING", enum: ["high", "medium", "low"] },
    viewport: { type: "STRING", enum: ["desktop", "tablet", "phone", "all"] },
    area: { type: "STRING" },
    kind: { type: "STRING" },
    problem: { type: "STRING" },
    fix: { type: "STRING" },
  },
  required: ["severity", "viewport", "area", "kind", "problem", "fix"],
};

const SCHEMA = {
  type: "OBJECT",
  properties: {
    score: { type: "INTEGER" },
    verdict: { type: "STRING", enum: ["ship", "fix"] },
    summary: { type: "STRING" },
    issues: { type: "ARRAY", items: ISSUE },
    strengths: { type: "ARRAY", items: { type: "STRING" } },
    lessons: { type: "ARRAY", items: { type: "STRING" } },
  },
  required: ["score", "verdict", "summary", "issues", "strengths", "lessons"],
};

// ---------- preparing what the model sees ----------

/** At most `n` items from `list`, evenly spread, keeping the first and last. */
export function spread<T>(list: T[], n: number): T[] {
  if (list.length <= n) return list;
  if (n <= 1) return list.slice(0, 1);
  const out: T[] = [];
  for (let i = 0; i < n; i++) out.push(list[Math.round((i * (list.length - 1)) / (n - 1))]);
  return out.filter((x, i) => out.indexOf(x) === i);
}

/** The screenshots to send: every desktop screen, a spread of tablet and phone ones. */
export function sampleShots(shots: Shot[]): Shot[] {
  const of = (v: string) => shots.filter((s) => s.viewport === v).sort((a, b) => a.index - b.index);
  return [...spread(of("desktop"), 10), ...spread(of("tablet"), 5), ...spread(of("phone"), 7)];
}

type A = Record<string, unknown>;
const arr = (v: unknown): A[] => (Array.isArray(v) ? (v as A[]) : []);

/** The measured facts, trimmed to what matters, as compact text. */
export function factsFor(viewports: ViewportReport[]): string {
  return viewports
    .map((v) => {
      const a = v.audit as A;
      const lines: string[] = [`${v.name} (${v.width}px wide, page ${v.pageHeight}px tall):`];
      if (a.horizontalOverflow) lines.push(`  SIDEWAYS OVERFLOW; elements past the edge: ${JSON.stringify(arr(a.overflowing).slice(0, 5))}`);
      const heads = arr(a.headlines).filter((h) => Number(h.lines) >= 4 || (Number(h.lines) >= 3 && Number(h.lastLine) < 0.3));
      if (heads.length) lines.push(`  headlines wrapping badly: ${heads.map((h) => `${h.el} "${String(h.text).slice(0, 28)}" ${h.lines} lines at ${h.px}px in ${h.w}px`).join("; ")}`);
      const low = arr(a.lowContrast);
      if (low.length) lines.push(`  low contrast: ${low.slice(0, 6).map((l) => `${l.el} "${String(l.text).slice(0, 20)}" ${l.ratio}:1`).join("; ")}`);
      const small = arr(a.smallText);
      if (small.length) lines.push(`  text under 12px: ${small.slice(0, 6).map((s) => `${s.el} ${s.px}px`).join("; ")}`);
      const stuck = arr(a.stuckHidden).filter((s) => !/^(a|button|nav|li)\b/.test(String(s.el)));
      if (stuck.length) lines.push(`  still invisible after scrolling: ${stuck.slice(0, 5).map((s) => `${s.el} "${String(s.text).slice(0, 20)}"`).join("; ")}`);
      const ov = arr(a.overlaps);
      if (ov.length) lines.push(`  text overlapping text (may be hidden accordion answers): ${ov.slice(0, 4).map((o) => `${o.a} over ${o.b} at y=${o.y}`).join("; ")}`);
      const sec = arr(a.sections).filter((s) => (typeof s.bottomGap === "number" && s.bottomGap < 0) || (typeof s.filled === "number" && s.filled < 0.5));
      if (sec.length) lines.push(`  sections with overflowing or mostly empty content: ${sec.map((s) => `${s.el} height ${s.h}px, bottom gap ${s.bottomGap}px, ${Math.round(Number(s.filled) * 100)}% used`).join("; ")}`);
      const tt = arr(a.smallTargets);
      if (tt.length) lines.push(`  small tap targets: ${tt.slice(0, 5).map((t) => `${t.el} ${t.w}x${t.h}`).join("; ")}`);
      const im = arr(a.images);
      if (im.length) lines.push(`  image problems: ${JSON.stringify(im.slice(0, 5))}`);
      const first = a.firstCtaY;
      if (typeof first === "number") lines.push(`  first main button at y=${first}px (screen is ${v.name === "desktop" ? 900 : v.name === "tablet" ? 1180 : 844}px tall)`);
      const lefts = a.headingLefts;
      if (Array.isArray(lefts) && lefts.length > 1) lines.push(`  section headings start at different left edges: ${lefts.join(", ")}`);
      if (lines.length === 1) lines.push("  nothing flagged by the measurements.");
      return lines.join("\n");
    })
    .join("\n");
}

/** Make the model's answer safe to use: clamp numbers, cap lists, give issues ids, drop empties. */
export function normalizeReview(raw: unknown): Review {
  const r = (raw ?? {}) as A;
  const str = (v: unknown, max = 600) => String(v ?? "").trim().slice(0, max);
  const sev = (v: unknown): Severity => (v === "high" || v === "medium" || v === "low" ? v : "medium");
  const vp = (v: unknown): Issue["viewport"] => (v === "desktop" || v === "tablet" || v === "phone" || v === "all" ? v : "all");
  const rank = { high: 0, medium: 1, low: 2 } as const;
  const issues: Issue[] = arr(r.issues)
    .map((i) => ({ severity: sev(i.severity), viewport: vp(i.viewport), area: str(i.area, 80), kind: str(i.kind, 40), problem: str(i.problem), fix: str(i.fix) }))
    .filter((i) => i.problem.length > 5)
    .sort((a, b) => rank[a.severity] - rank[b.severity])
    .slice(0, 12)
    .map((i, n) => ({ id: `i${n + 1}`, ...i }));
  const score = Math.max(0, Math.min(100, Math.round(Number(r.score) || 0)));
  const list = (v: unknown, n: number) => (Array.isArray(v) ? v.map((x) => str(x, 300)).filter((x) => x.length > 5).slice(0, n) : []);
  const high = issues.filter((i) => i.severity === "high").length;
  const medium = issues.filter((i) => i.severity === "medium").length;
  // The verdict is decided by the issues, whatever the model said.
  const verdict: Review["verdict"] = high === 0 && medium <= 2 ? "ship" : "fix";
  return { score, verdict, summary: str(r.summary, 700), issues, strengths: list(r.strengths, 4), lessons: list(r.lessons, 5) };
}

/** Whether another round of fixes is worth doing. */
export function needsFix(review: Review): boolean {
  return review.issues.some((i) => i.severity === "high" || i.severity === "medium") && review.verdict !== "ship";
}

/** Look at the screenshots and measurements and write the review. */
export async function reviewSite(apiKey: string, capture: Capture, brief: string, previous: Issue[] = []): Promise<Review> {
  const shots = sampleShots(capture.shots);
  const parts: Record<string, unknown>[] = [
    { text: `What was asked for (the brief the site was built from):\n${brief.slice(0, 1500)}` },
    { text: `Measurements from the browser:\n${factsFor(capture.viewports)}` },
  ];
  if (previous.length) {
    parts.push({ text: `Issues from the previous round, to check against these new screenshots:\n${previous.map((i) => `- [${i.severity}] ${i.viewport} ${i.area}: ${i.problem}`).join("\n")}` });
  }
  for (const s of shots) {
    const data = await invoke<string>("director_shot", { path: s.path });
    parts.push({ text: `[${s.viewport} ${s.width}px, screen ${s.index}, page position ${s.y}px]` });
    parts.push({ inlineData: { mimeType: "image/jpeg", data } });
  }
  const body = JSON.stringify({
    systemInstruction: { parts: [{ text: DIRECTOR_RULES }] },
    contents: [{ role: "user", parts }],
    generationConfig: { responseMimeType: "application/json", responseSchema: SCHEMA, temperature: 0.2, maxOutputTokens: 6000 },
  });
  let lastError = "The visual director couldn't get a review from Gemini.";
  for (const model of await pickModels(apiKey)) {
    const res = await fetch(`${API}/models/${model}:generateContent?key=${encodeURIComponent(apiKey)}`, { method: "POST", headers: { "Content-Type": "application/json" }, body });
    if (!res.ok) {
      lastError = `Gemini said ${res.status} to the review request.`;
      if ([404, 429, 500, 503].includes(res.status)) continue;
      break;
    }
    const data = await res.json();
    try {
      const text = (data.candidates?.[0]?.content?.parts ?? []).map((p: { text?: string }) => p.text ?? "").join("");
      return normalizeReview(JSON.parse(text));
    } catch {
      lastError = "Gemini's review wasn't in the expected form.";
    }
  }
  throw new Error(lastError);
}

/** What the code worker is told to fix. */
export function fixRequest(review: Review, round: number): string {
  const list = review.issues
    .filter((i) => i.severity !== "low" || review.issues.length <= 4)
    .map((i, n) => `${n + 1}. [${i.severity}] ${i.viewport === "all" ? "all sizes" : i.viewport}, ${i.area} (${i.kind}): ${i.problem}\n   Fix: ${i.fix}`)
    .join("\n");
  return `The visual director reviewed this site (round ${round}, score ${review.score}/100). ${review.summary}\n\nIssues to fix, most serious first:\n${list}\n\nThings that are already good and must stay as they are: ${review.strengths.join("; ") || "the overall concept"}.`;
}

/** One short line for the user about how a review went. */
export function summaryLine(state: DirectorState): string {
  const done = state.rounds.filter((r) => r.review);
  const first = done[0]?.review;
  const last = done[done.length - 1]?.review;
  if (!first || !last) return state.message;
  const fixed = state.rounds.filter((r) => r.fixTaskId != null).length;
  const left = last.issues.filter((i) => i.severity !== "low").length;
  return `Score ${first.score}${done.length > 1 ? ` → ${last.score}` : ""}/100 after ${done.length} look${done.length === 1 ? "" : "s"} and ${fixed} fix round${fixed === 1 ? "" : "s"}; ${left === 0 ? "no serious issues left" : `${left} issue${left === 1 ? "" : "s"} still open`}.`;
}
