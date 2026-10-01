# Jarvis

A macOS voice research assistant built with Tauri 2.

- **Voice:** Gemini Live API, spoken in real time from the app window, with live transcripts.
- **Worker:** the Codex CLI (`codex exec --json`) running on your ChatGPT subscription. It does the actual research in the background and writes a report with sources.
- **Sync:** Gemini delegates work through tool calls. Rust streams Codex's progress to the UI, and when a task finishes, the summary goes back into the live conversation so Jarvis tells you at the next pause.

```
 mic ─▶ Gemini Live ──tool call: start_research──▶ Rust (tasks.rs) ──spawn──▶ codex exec --json
  ▲         ▲                                            │                        │
  │         └──── [WORKER] notice (summary) ◀────────────┴── task-finished ◀──────┘
speaker                                                   task-update ──▶ Tasks pane (live steps)
```

## Setup

1. Install and log in to Codex:
   ```bash
   npm install -g @openai/codex@latest
   codex login
   ```
2. Get a Gemini API key from https://aistudio.google.com/apikey
3. Run the app:
   ```bash
   npm install
   npm run tauri dev
   ```
   For Gmail, Calendar, Drive, Docs, Sheets and YouTube (enable each API in your Google Cloud project; then connect each app you want under Settings, Apps), copy `.env.example` to `.env` and fill in a Google "Desktop app" OAuth client ID and secret. They're compiled into the app at build time (rebuild after changing them); `.env` is not committed.
4. Set up opens on first launch: paste the Gemini key, press Install for the research helper (Codex), then Connect to sign in to ChatGPT. More options are under Settings.
5. Press the mic button (or ⌥Space) and say something like “Jarvis, research the best vector databases for a small startup, deep dive.”

Use headphones if Jarvis hears itself through your speakers.

Build a real `.app` / `.dmg` with `npm run tauri build`. The output goes to `src-tauri/target/release/bundle/`.

## Where things live

| What | Where |
|---|---|
| Research reports | `~/Jarvis/research/0001-topic/report.md` (+ `summary.txt`) |
| Task history | `~/Jarvis/tasks.json` |
| Notes (“remember that…”) | `~/Jarvis/notes.md`, loaded into Jarvis's instructions each session |
| Routines and their runs | `~/Jarvis/routines.json`, `~/Jarvis/routine-runs.json`, results in `~/Jarvis/routines/<routine>/runs/<n>/step-<k>/` |
| Know-how (skills) | `~/Jarvis/skills/<name>/SKILL.md` (+ `templates/`, `examples/`); `.draft` until reviewed |
| Settings (incl. API key) | `~/Library/Application Support/co.fikraventures.jarvis/settings.json` (mode 600) |

## Code map

| File | Role |
|---|---|
| `src/lib/jarvis.ts` | Orchestrator: session, tool handling, transcript, delivering worker results |
| `src/lib/live.ts` | Gemini Live WebSocket client, model discovery, auto-reconnect with session resumption |
| `src/lib/audio.ts`, `public/pcm-worklet.js` | Mic → 16 kHz PCM, 24 kHz playback with instant interrupt, audio levels |
| `src/components/Orb.tsx` | Three.js voice sphere driven by real audio levels |
| `src-tauri/src/tasks.rs` | Runs Codex, parses its JSON events into steps, follow-ups via `codex exec resume` |
| `src-tauri/src/routines.rs` | Routines: runs steps as Codex tasks, passes each step's output to the next, reads mail and calendar, waits for an OK before emailing, schedules, pauses after two failures. Know-how storage and learning |
| `src/lib/routines.ts` | Step kinds in plain words, ready-made routines, and the planner (Gemini text model) that turns a sentence into steps |
| `src/components/RoutinesPage.tsx`, `KnowHowPage.tsx`, `SetupPanel.tsx` | Routines (describe, recipe, try once, approve, fine-tune), know-how review, first-run setup |
| `src-tauri/src/lib.rs` | Window vibrancy (glass), menu bar icon, global shortcuts, hide-on-close |

## Routines and know-how

A routine is a few plain steps Jarvis runs for you, once or on a schedule: look something up, read your email or today's calendar, write a document, email you the result. Say it (“every Monday, check my competitors and email me a summary”), type it on the Routines page, or start from a ready-made one. Jarvis shows the plan in plain words; you try it once, then turn it on. Emailing always waits for your OK unless you tell that routine not to ask (it only ever emails you). A routine that fails twice in a row pauses itself and says why.

Know-how is how Jarvis remembers an approach that worked. After a good result, press “Yes, remember” (or say “remember how you did that”): Codex writes a short SKILL.md from the results, and it waits on the Know-how page for you to read and save. Saved know-how is copied into the worker's folder for any step or research it's attached to.

## Shortcuts

- ⌥Space: talk / mute (works from any app)
- ⌥J: show / hide the window
- ⌥.: stop Jarvis mid-sentence
