# Jarvis Roadmap

Built from the "Feature Expansion Ideas" list, checked against the code as of commit `121e552`.

## Where Jarvis already is

Several ideas on the list exist in some form. They should be extended, not rebuilt.

| Idea | Status | Where |
|---|---|---|
| 2. Browser agent | **Built.** Codex drives a hidden Chrome via Playwright MCP, live view in the side panel | `browser.rs`, `BrowserView.tsx` |
| 9. Gmail + Calendar | **Built.** Search, read, draft, send-after-confirm; list and create events | `google.rs` |
| 10. Briefings | **Partly built.** Scheduled *research* briefings. No "your day" digest yet | `briefings.rs` |
| 15. Image understanding | **Partly built.** Drag-drop and paperclip images go to Gemini Live and `create_image` | `jarvis.ts` attachments |
| 18. Skill library | **Partly built.** Local know-how (`SKILL.md`) with draft-then-review | `routines.rs`, `KnowHowPage.tsx` |
| 20. Approvals | **Partly built.** Hard-coded for email send and routine email steps only | `google.rs`, `routines.rs` |
| 3. Memory | **Basic.** `notes.md`, last 20 lines injected into the prompt | `jarvis.ts` `systemPrompt` |
| 12. Search | **Basic.** Keyword search over Jarvis's own chats, docs, reports | `search.rs` |
| 5. Multi-agent | **One worker.** Codex, with follow-ups via `codex exec resume` | `tasks.rs` |

Not started: screen awareness, computer control, active-app context, workspaces, verification, GitHub, meeting mode, clipboard, command palette, mini window.

## What to build, what to skip

**Build (high value, fits the architecture):** approval engine, active-app/selected-text context, screen look, command palette, Mac control, semantic memory, workspaces, verification loop, GitHub, "your day" digest, mini window.

**Build later / carefully:** meeting mode (needs system-audio capture plus clear consent UX), clipboard watcher (opt-in only), parallel specialist agents (cost and coordination).

**Skip for now:**

- **11. Notification intelligence.** macOS has no public API to read other apps' notifications; the notification database is private and protected. Instead: let Jarvis classify *its own* inputs (mail, calendar, GitHub, routines) in the digest.
- **19. Learn from demonstration.** Large research project. The existing "remember how you did that" covers most of the value.
- **18. Skill marketplace from remote repos.** Installing third-party instructions that can drive the browser and Mac is a security risk. Keep skills local and reviewed.
- **Notion / Jira / Linear / Slack.** Add one only when there's a real daily need; each is an OAuth app plus API surface to maintain.

## Phases

Estimates are rough, for one person working with Codex.

### Phase 1 — Context and safety (about 1–2 weeks)

Goal: Jarvis knows what you're looking at, and every risky action goes through one gate.

1. **Approval engine** (`src-tauri/src/policy.rs`, `ApprovalCard.tsx`)
   - Every tool call carries a risk class: `read`, `search`, `write`, `send`, `delete`, `purchase`, `deploy`, `control` (Mac input).
   - Policy per class in Settings: `auto` / `ask` / `never`. Defaults: read+search auto, write ask-once-per-task, everything else ask.
   - Move the existing email confirm and routine approval onto this. One approval card UI, also usable by voice ("yes, send it").
   - Log every approved/rejected action to `~/Jarvis/audit.jsonl`.
2. **Active-app context** (`context.rs`)
   - Frontmost app and window title (`NSWorkspace`), selected text via the Accessibility API (`AXSelectedText`), falling back to simulating ⌘C and restoring the clipboard.
   - Requires the Accessibility permission. Add it to Setup with an explanation.
   - New tool `get_context`. Gemini calls it for "explain this", "rewrite this", "reply to this".
   - "Rewrite this" result: paste back with approval (class `write`).
3. **Screen look** (`screen.rs`)
   - Capture the frontmost window with ScreenCaptureKit (Screen Recording permission). Send it to Gemini Live as an image, the same way attachments go now.
   - Tool `look_at_screen`. Trigger: "look at this", "what's this error".
   - Only on request. No continuous capture.
4. **Command palette**
   - New shortcut (e.g. ⌥⇧Space; ⌥Space stays voice) opens a small floating input.
   - Typed text goes into the same orchestrator as voice (`sendText`), with `/research`, `/remember`, `/email`, `/open` prefixes as shortcuts.

**Done when:** you can select an error in Terminal, press the shortcut, say "fix this", and Jarvis explains it and offers to start a Codex task with the error text included.

### Phase 2 — Mac control and verification (about 2–3 weeks)

Goal: Jarvis can act in native apps, and checks its own work.

1. **Mac agent** (`mac.rs`)
   - Read the UI as an Accessibility tree (roles, labels, positions) rather than raw screenshots. It's cheaper and more reliable; use the screenshot as fallback.
   - Actions: click element, type, key combo, open app/URL, scroll. Every action has class `control` and goes through the approval engine; "allow for this task" per app.
   - Driven by Codex through a small local MCP server Jarvis exposes, the same pattern as the Playwright browser.
   - Visible "Jarvis is controlling <App>" banner, and ⌥. stops it instantly.
2. **Verification loop** (extends `tasks.rs`)
   - For coding tasks: after Codex finishes, run the project's test/build command, launch it if it's an app, screenshot via the Mac or browser agent, and run a second Codex pass as reviewer with the diff, test output and screenshot.
   - Report: pass, or a list of problems, with an offer to fix.
   - Per-workspace `verify` config: commands to run, URL or app to open.

**Done when:** "fix the settings scroll bug" ends with Jarvis saying the tests pass and showing a screenshot of the fixed modal.

### Phase 3 — Memory and workspaces (about 2 weeks)

Goal: Jarvis stops starting from zero.

1. **Memory store** (`memory.rs`, SQLite via `rusqlite`)
   - Tables: `memories` (kind: person / project / decision / preference / fact, text, source, workspace, created), FTS5 index, embeddings column.
   - Embeddings from the Gemini embedding API (key already exists). Hybrid search: FTS5 keyword plus cosine similarity.
   - At the end of each chat, a cheap Gemini text call extracts decisions and facts and stores them with a link to the chat. Shown on a Memory page to edit or delete.
   - Tool `recall` replaces stuffing `notes.md` into the prompt. Import `notes.md` once.
2. **Workspaces**
   - `~/Jarvis/projects/<name>/` with `workspace.json` (repo path, folders, verify commands), and memory, tasks, research and know-how tagged by workspace.
   - "Switch to Fikra" sets the active workspace. Shown in the title bar, used to filter recall and set Codex's working directory.
3. **File index**
   - Index user-chosen folders only: metadata, plus text extracted from md, txt, pdf, docx, pptx. Chunked and embedded; re-index on change (FSEvents).
   - Extends `search.rs` and the `recall` tool. Never sends whole folders to a model.

**Done when:** "what did we decide about Fikra pricing?" answers with the decision, the date and a link to the source chat.

### Phase 4 — GitHub and specialist workers (about 2 weeks)

1. **GitHub** via the `gh` CLI (already logged in on your Mac; no OAuth app to build).
   - Tools: `github_inbox` (PRs to review, assigned issues, failing CI), `github_fix_issue` (branch, Codex fixes in the workspace repo, verification loop, push, open PR after approval), `github_review_pr` (Codex reviews the diff and posts comments after approval).
2. **Specialist workers**
   - Not separate agent frameworks. Different Codex runs with role prompts from know-how (researcher, coder, reviewer, analyst), plus a planner that splits a request into steps. Routines already chain steps; reuse that engine for ad-hoc plans.
   - Parallel runs only where steps are independent (e.g. research three competitors at once). Cap concurrency at 3 in Settings.

### Phase 5 — Daily layer (about 1–2 weeks)

1. **"Your day" digest.** A new briefing kind that pulls calendar, unread important mail, GitHub inbox, routine results and open tasks, and ranks them. Runs on schedule and on "what needs my attention?". Spoken and shown as a card.
2. **Mini Jarvis.** A small always-on-top Tauri window: orb, status, current task progress, approval prompts. Click to open the full window.
3. **Meeting mode.** Explicit start/stop, visible indicator, mic plus system audio (ScreenCaptureKit audio). Transcript, then decisions and action items written into memory and the workspace.
4. **Clipboard watcher** (opt-in, off by default). Only reacts to error-shaped text, and only offers, never acts.

## Progress

**Phase 1, first milestone: built (needs real-world testing).**

- Approval engine: `src/lib/approvals.ts`, `ApprovalCard.tsx`, `policy.rs`. Email send and calendar invites now use it. Rules for "Change files" and "Send" are in Settings → Your screen · Approvals. Everything is logged to `~/Jarvis/audit.jsonl`. Approvals are click-only, never voice.
- Context: `context.rs` reads the front app, window title and selected text (Accessibility API, ⌘C fallback). Tools: `get_context`, `replace_selection`.
- Code worker: `fix_in_project` starts Codex inside a project folder (`start_code_task`, `resolve_project` in `tasks.rs`), after approval. Report lands in `~/Jarvis/code/`.

- Screen look: `look_at_screen` photographs the front window on request (Screen Recording permission) and shows it to Gemini.
- Command palette: ⌥⇧Space (or the menu bar) opens a typed command box; `research`, `explain`, `rewrite`, `reply`, `fix`, `look`, `email`, `calendar`, `remember`, `browse`. It reads the app you were in first, so "this" works.

**Phase 5, "your day" digest: first cut built.** The `my_day` tool gathers the rest of today's events, unread mail from the last two days, recent Jarvis tasks and upcoming briefings; Gemini ranks and speaks it. Also `day` in the command palette. A scheduled version is the "Your day" routine template (weekdays 07:45: calendar, inbox, then a ranked note). Not yet: a spoken digest on launch, GitHub inbox.

Routine email approvals now use the same on-screen card (risk `send`, audit-logged, click-only). The old voice tool `answer_routine_email` is gone, so a spoken yes can no longer send a routine's email.

## Suggested first milestone

Phase 1 items 1 and 2: the **approval engine** and **"explain / fix / rewrite this"** using the selected text. The approval engine is the foundation everything later depends on. Selected-text context is the fastest visible win, since it makes the existing voice and Codex flow work on whatever you're looking at.

**Phase 3, memory:** the SQLite store (FTS5 trigram search, workspaces, notes.md import, Memory page) already existed. Added: when you leave a chat, `memoryExtract.ts` asks a Gemini text model for up to 6 lasting decisions, facts and preferences and saves them linked to that chat (edit or delete on the Memory page). Search is now hybrid: with a Gemini key, every memory gets a 768-dim `gemini-embedding-001` vector (stored in `memory.db`, filled in the background and on launch), and `memory_search` merges keyword and meaning rankings. Without a key or offline it stays keyword-only. Still missing: indexing files and chats, not just memories.

**Phase 5, Mini Jarvis: built (needs real-world testing).** A second always-on-top window (`mini.rs`, `MiniJarvis.tsx`, `lib/mini.ts`), toggled with ⌥M or the menu bar. It shows the orb, status, the running task's latest step, and approval cards with Reject / Approve buttons. It has no logic of its own: the main window publishes a snapshot (`mini-state`, `mini-level`) and the mini window sends clicks back (`mini-cmd`). An approval that arrives while the main window isn't in front brings the mini window up.

**Phase 2, Mac control: first cut built (needs real-world testing; macOS Accessibility permission required).** `mac.rs` reads another app's window as numbered controls through the Accessibility API and acts on them. Tools: `mac_read`, `mac_open`, `mac_click`, `mac_type`, `mac_key`, `mac_scroll`.
- Class `control`: the first action in each app asks "Let Jarvis control <app>", valid until stop (⌥.) or the conversation ends. Buttons labelled send/post/delete/buy, and ⌘Q/⌘W/⌘⌫, ask again every time (classes `send`, `delete`, `purchase`). Everything is audit-logged.
- Stop (⌥. or the banner/mini window button) trips a kill switch in Rust and revokes every grant; nothing runs until the user allows control again.
- Never types into password fields; only opens apps by name and http/https/mailto links; text read from apps is flagged to the model as data, not instructions.
- A banner ("Jarvis is controlling <app>") shows in both windows.
- Not yet: Codex-driven long tasks over this (the tools are voice-driven for now), the verification loop, per-app "allow always".

**Phase 2, verification loop: first cut built (needs real-world testing).** When a code task finishes, `verify.rs` runs the commands listed under the matching workspace's "verify" setting (Memory page → workspace) in the project folder, through a login shell, 5 minutes max each, stopping at the first failure. The task isn't announced as done until they finish: Jarvis then says the checks pass, or that they failed and offers a `follow_up` fix with the output. The result is appended to the task's `report.md` and stored as `Task.verify` (`running` / `passed` / `failed`). Only commands the user wrote in the workspace are ever run. The task card shows "Checking…", "Done · checks pass" or "Checks failed". Not yet: opening the workspace `url` and screenshotting it, and a second reviewer pass over the diff.

**Phase 3, file index: built.** `fileindex.rs` indexes only the folders you add (Search page → "Files Jarvis can search"). It reads Markdown/text/CSV, Word/RTF/HTML (macOS `textutil`), PDFs (PDFKit) and decks (Spotlight text), cuts them into ~800-character passages in `files.db`, skips hidden and build folders, and re-reads only changed files on launch and on Refresh. With a Gemini key, passages are embedded (`batchEmbedContents`, capped at 20,000) so search works by meaning as well as words; only passage text is sent, never file names or whole folders. Search results show on the Search page, and the voice tool `search_files` returns passages with file paths. Not yet: live re-indexing on file changes (FSEvents), OCR for scanned PDFs, Excel/Numbers.

**Phase 4/5, parallel workers: built.** At most `maxWorkers` (settings.json, default 3, 1–8) Codex workers run at once; extra tasks wait, show "Waiting for a free worker" on their card, and can still be cancelled. The voice tool `research_many` starts 2–5 independent research jobs together and reports them once, with all results, when the last finishes (`combine` says what to do with them). Not yet: role prompts per job (researcher/reviewer/analyst), a Settings control for `maxWorkers`.

**Phase 5, meeting mode: built (microphone only).** Start from the record button beside the mic or by voice (`start_meeting`); it needs an approval click, switches the voice assistant off (the mic is shared), and shows a red "Recording" banner in the main and Mini windows. `meeting.ts` records the mic, sends 45-second WAV chunks to a Gemini model for a speaker-labelled transcript (silence is skipped), and `meeting.rs` appends each chunk to `~/Jarvis/meetings/<date>-<title>/transcript.md` as it arrives. Stopping writes `notes.md` (summary, decisions, action items, open questions), saves a meeting memory, each decision and each action item to memory, and posts the notes in the chat. Not yet: system audio (the far side of a video call is only heard if it comes out of the speakers), speaker names, live transcript on screen.

**File index:** now re-scans every 15 minutes while Jarvis runs, in addition to launch and Refresh.
