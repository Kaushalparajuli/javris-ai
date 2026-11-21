// Tool lines in the conversation, in plain words. Jarvis records each tool it uses as a short
// line like "→ start_research · deep · task #79"; on screen that becomes "Started deep research",
// and pure look-ups (checking progress, reading the selection) aren't shown at all. The saved
// transcript keeps the original line.

/** What each tool did, as the start of a sentence. Tools mapped to null aren't shown. */
const PHRASES: Record<string, string | null> = {
  // Work Jarvis starts
  start_research: "Started research",
  research_many: "Started research",
  follow_up: "Following up",
  create_image: "Making an image",
  create_document: "Writing a document",
  edit_document: "Editing the document",
  create_slides: "Making a slide deck",
  edit_slides: "Changing the slides",
  slides_to_google: "Saved the slides to Google Slides",
  make_video: "Making a video",
  edit_video: "Changing the video",
  render_video: "Rendering the final video",
  approve_storyboard: "Storyboard",
  browse: "Browsing the web",
  build_project: "Building",
  fix_in_project: "Fixing",
  "visual director": "Reviewing the design",
  review_site: "Reviewing the design",
  cancel_task: "Stopped a task",
  // Mail and calendar
  email_draft: "Drafted an email",
  email_send: "Sent an email",
  email_search: "Searched your email",
  email_read: "Read an email",
  calendar_events: "Checked your calendar",
  calendar_create: "Added to your calendar",
  calendar_update: "Updated your calendar",
  calendar_delete: "Removed from your calendar",
  my_day: "Looked at your day",
  // Google Drive
  drive_search: "Searched Google Drive",
  drive_read: "Read from Google Drive",
  drive_save: "Saved to Google Drive",
  doc_create: "Created a Google Doc",
  doc_append: "Added to a Google Doc",
  doc_read: "Read a Google Doc",
  sheet_create: "Created a Google Sheet",
  sheet_append: "Added rows to a Google Sheet",
  sheet_update: "Updated a Google Sheet",
  sheet_read: "Read a Google Sheet",
  // Search and sources
  web_search: "Searched the web",
  youtube_search: "Searched YouTube",
  youtube_play: "Playing on YouTube",
  youtube_video: "Looked up a video",
  youtube_ask: "Watched a YouTube video",
  sec_filings: "Looked up SEC filings",
  sec_search: "Searched SEC filings",
  sec_read: "Read an SEC filing",
  paper_search: "Searched research papers",
  search_files: "Searched your files",
  read: "Read a file",
  // Other apps on this Mac
  mac_open: "Opened",
  mac_click: "Clicked",
  mac_type: "Typed",
  mac_key: "Pressed",
  mac_scroll: "Scrolled",
  replace_selection: "Replaced the selected text",
  look_at_screen: "Looked at your screen",
  // Routines, memory, projects
  create_routine: "Created a routine",
  update_routine: "Updated a routine",
  run_routine: "Ran a routine",
  schedule_briefing: "Scheduled a briefing",
  cancel_briefing: "Cancelled a briefing",
  remember_how: "Saved how to do this",
  project: "Switched project",
  "meeting started": "Recording a meeting",
  "plan only": "Planned without doing anything",
  // Look-ups that need no line of their own
  task_status: null,
  get_context: null,
  open_report: null,
  list_routines: null,
  list_briefings: null,
  recall: null,
  remember: null,
  sheet_info: null,
  read_project_file: null,
  mac_read: null,
  switch_project: null,
};

/** Details that only make sense to the app: task numbers, counts of characters, raw ids. */
function cleanDetail(d: string): string {
  return d
    .replace(/\btask #\d+(\s*→\s*#\d+)?/gi, "")
    .replace(/(^|\s)#\d+\b/g, "")
    .replace(/\b\d+ chars\b/, "")
    .replace(/^(deep|quick)$/i, (m) => m.toLowerCase())
    .trim();
}

export interface ToolLine {
  text: string;
  error: boolean;
}

/** The on-screen version of a tool line, or null when it shouldn't be shown. */
export function toolLine(raw: string): ToolLine | null {
  const s = raw.trim();
  // "✕ name: message" — a tool that failed.
  const failed = /^✕\s*([a-z_ ]+?):\s*([\s\S]*)$/i.exec(s);
  if (failed) {
    const phrase = PHRASES[failed[1].trim()];
    const what = phrase ? phrase.charAt(0).toLowerCase() + phrase.slice(1) : failed[1].replace(/_/g, " ");
    return { text: `Couldn't finish: ${what}. ${failed[2].trim()}`, error: true };
  }
  const m = /^→\s*(.+)$/.exec(s);
  if (!m) return { text: s, error: false };
  const parts = m[1].split("·").map((p) => p.trim());
  const name = parts.shift() ?? "";
  const phrase = PHRASES[name];
  if (phrase === null) return null;
  const details = parts.map(cleanDetail).filter(Boolean);
  if (phrase === undefined) {
    // A tool without a phrase yet: show its name in words rather than code.
    const words = name.replace(/_/g, " ");
    return { text: [words.charAt(0).toUpperCase() + words.slice(1), ...details].join(" · "), error: false };
  }
  // "Started research" + "deep" reads better as "Started deep research".
  if ((name === "start_research" || name === "research_many") && details[0] && /^(deep|quick)$/.test(details[0])) {
    return { text: `Started ${details.shift()} research${details.length ? ` · ${details.join(" · ")}` : ""}`, error: false };
  }
  return { text: details.length ? `${phrase} · ${details.join(" · ")}` : phrase, error: false };
}

/**
 * A finished-work update in plain words. The text was written for Jarvis to read (it carries task
 * numbers so Jarvis can follow up, and quotes the worker); on screen those parts are dropped.
 * "Image task #80 "Trekker" is finished: … Worker says: I created…" → "Image "Trekker" is finished: … I created…"
 */
export function noticeLine(raw: string): string {
  return raw
    .replace(/^\[WORKER\]\s*/, "")
    .replace(/\s*\(task #\d+\)/gi, "")
    .replace(/\b(Image|Browser|Code|Research|Slides?|Video) task #\d+\s*/g, (_m, kind: string) => (kind === "Browser" ? "Web task " : `${kind} `))
    .replace(/\btask #\d+\b/gi, "the task")
    .replace(/\s*Worker says:\s*/g, " ")
    .replace(/\s*\(If it asked a question[^)]*\)/g, "")
    .replace(/\s*Say so in a sentence\.?/g, "")
    // A raw log line from the worker isn't an explanation anyone can act on.
    .replace(/failed: \d{4}-\d{2}-\d{2}T\S+\s+(ERROR|WARN)[\s\S]*$/, "failed. Open it in Activity to see what went wrong.")
    .replace(/\s{2,}/g, " ")
    .trim();
}
