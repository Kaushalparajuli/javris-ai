// One piece of work, however many worker runs it took. Asking Jarvis to change a document, redo an
// image, polish a video or fix a website starts a new worker run each time; those runs are shown
// together as one item (the latest run up front, the rest as its history) so the tasks pane and the
// Library list each video, site, document, deck or report once.

import type { Task } from "./types";

export type WorkKind = Task["kind"] | "video" | "site";

export interface WorkItem {
  /** Stable key: the first run's id, or the shared project folder. */
  key: string;
  title: string;
  kind: WorkKind;
  /** Every run, oldest first. */
  runs: Task[];
  /** The run shown up front: one still working, otherwise the newest. */
  latest: Task;
  /** The newest finished run. */
  latestDone: Task | null;
  /** The run to open for this item: for a video or site, its newest finished code run (that shows
   *  the player or live preview); otherwise the newest finished run. */
  open: Task | null;
  /** Images made across all runs, newest first (image sets, a video's pictures). */
  images: string[];
  chatIds: Set<string>;
  /** When anything last happened, for sorting. */
  activity: number;
}

const VIDEO_STAGE = /^(Storyboard|Compose|Polish the video|Picture for the video|Render|Review the video)\b/i;

const baseName = (p: string) => p.replace(/\/+$/, "").split("/").pop() ?? p;

/** "gravitational-force-explanation" → "Gravitational force explanation". */
function pretty(folder: string): string {
  // A project's own files folder is named "files": use the project's name instead.
  const parts = folder.replace(/\/+$/, "").split("/");
  const name = parts[parts.length - 1] === "files" && parts.length > 1 ? parts[parts.length - 2] : baseName(folder);
  const s = name.replace(/[-_]+/g, " ").trim();
  return s ? s[0].toUpperCase() + s.slice(1) : folder;
}

export function groupWork(tasks: Task[]): WorkItem[] {
  const byId = new Map(tasks.map((t) => [t.id, t]));
  const rootOf = (t: Task): Task => {
    let r = t;
    const seen = new Set<number>();
    while (r.parentId != null && byId.has(r.parentId) && !seen.has(r.id)) {
      seen.add(r.id);
      r = byId.get(r.parentId)!;
    }
    return r;
  };

  // Pictures made for a video before they carried the video's folder: they belong to the video
  // run started just before them in the same chat.
  const sorted = [...tasks].sort((a, b) => a.startedAt - b.startedAt);
  const videoFolderBefore = (t: Task): string => {
    let found = "";
    for (const x of sorted) {
      if (x.startedAt > t.startedAt) break;
      if (x.chatId === t.chatId && x.project && VIDEO_STAGE.test(x.title) && !/^Picture/i.test(x.title)) found = x.project;
    }
    return found;
  };

  const keyOf = (t: Task): string => {
    const root = rootOf(t);
    if (root.project) return `folder:${root.project.replace(/\/+$/, "")}`;
    if (root.kind === "image" && /^Picture for the video/i.test(root.title)) {
      const folder = videoFolderBefore(root);
      if (folder) return `folder:${folder.replace(/\/+$/, "")}`;
    }
    return `task:${root.id}`;
  };

  const groups = new Map<string, Task[]>();
  for (const t of sorted) {
    const k = keyOf(t);
    const list = groups.get(k);
    if (list) list.push(t);
    else groups.set(k, [t]);
  }

  const items: WorkItem[] = [];
  for (const [key, runs] of groups) {
    const first = runs[0];
    const running = [...runs].reverse().find((r) => r.status === "running");
    const latest = running ?? runs[runs.length - 1];
    const done = [...runs].reverse().find((r) => r.status === "done") ?? null;
    const isVideo = runs.some((r) => VIDEO_STAGE.test(r.title));
    const folder = key.startsWith("folder:") ? key.slice(7) : "";
    const kind: WorkKind = isVideo ? "video" : folder && first.kind === "code" ? "site" : first.kind;
    const title = folder ? pretty(folder) : first.title.replace(/^(Writing|Edit|Follow-up|Change):\s*/i, "");
    const images = runs.flatMap((r) => r.images ?? []).reverse();
    items.push({
      key,
      title,
      kind,
      runs,
      latest,
      latestDone: done,
      open: folder ? ([...runs].reverse().find((r) => r.status === "done" && r.kind === "code") ?? done) : done,
      images: [...new Set(images)],
      chatIds: new Set(runs.map((r) => r.chatId ?? "")),
      activity: Math.max(...runs.map((r) => r.finishedAt ?? r.startedAt)),
    });
  }
  return items.sort((a, b) => b.activity - a.activity);
}
