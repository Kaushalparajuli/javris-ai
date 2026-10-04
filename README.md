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
| `src-tauri/src/videokit.rs` | Video toolbox: private Node/HyperFrames/FFmpeg install, HeyGen CLI install and sign-in, project scaffold, check, render with progress |
| `src-tauri/src/voices.rs` | Third-party narration voices (ElevenLabs) and service key checks |
| `src-tauri/src/videoreview.rs` | The art director: Gemini looks at draft frames for layout problems |
| `src-tauri/src/videopipe.rs` | The video stages: storyboard, approval, media, compose, check and fix, draft |
| `src-tauri/src/cutout.rs` | Object sheet → separate transparent PNGs (no model) |
| `src-tauri/src/sitekit.rs` | Site extras: Three.js for 3D, versions and go-back, point-at-a-part, zip export |
| `src-tauri/src/routines.rs` | Routines: runs steps as Codex tasks, passes each step's output to the next, reads mail and calendar, waits for an OK before emailing, schedules, pauses after two failures. Know-how storage and learning |
| `src/lib/routines.ts` | Step kinds in plain words, ready-made routines, and the planner (Gemini text model) that turns a sentence into steps |
| `src/components/RoutinesPage.tsx`, `KnowHowPage.tsx`, `SetupPanel.tsx` | Routines (describe, recipe, try once, approve, fine-tune), know-how review, first-run setup |
| `src-tauri/src/lib.rs` | Window vibrancy (glass), menu bar icon, global shortcuts, hide-on-close |

## Routines and know-how

A routine is a few plain steps Jarvis runs for you, once or on a schedule: look something up, read your email or today's calendar, write a document, email you the result. Say it (“every Monday, check my competitors and email me a summary”), type it on the Routines page, or start from a ready-made one. Jarvis shows the plan in plain words; you try it once, then turn it on. Emailing always waits for your OK unless you tell that routine not to ask (it only ever emails you). A routine that fails twice in a row pauses itself and says why.

Know-how is how Jarvis remembers an approach that worked. After a good result, press “Yes, remember” (or say “remember how you did that”): Codex writes a short SKILL.md from the results, and it waits on the Know-how page for you to read and save. Saved know-how is copied into the worker's folder for any step or research it's attached to.

## Videos

Say “make a 20-second Instagram reel for my bakery”. Jarvis asks a few questions, then makes the video with HeyGen's open-source [HyperFrames](https://github.com/heygen-com/hyperframes) (a video is an HTML page with a GSAP timeline, rendered frame by frame in Chrome and encoded with FFmpeg). Nothing has to be installed by hand: **Set up → Video tools** downloads a private toolbox (about 300 MB, once) into `~/Jarvis/.video-runtime/`: Node 22 (checked against nodejs.org's published checksum), HyperFrames, FFmpeg, FFprobe and GSAP. Your own Node, FFmpeg and Chrome profile are never used, and the toolbox runs with its own home folder, so your settings and any HeyGen sign-in you already have are never read or replaced. An installed Chrome, Edge or Brave is reused for drawing the frames; otherwise HyperFrames fetches one.

The steps, shown in the Video tab of the side panel:

1. **Storyboard.** The code worker (Codex) writes `STORYBOARD.md` (goal, palette, fonts, scene table, narration) and `requests.json` (what media it wants). You read it and approve it, or ask for changes.
2. **Media.** Jarvis, not the worker, fetches and makes what was requested: HyperFrames catalog effects (`hyperframes add`), sound effects (19 are bundled), music, photos, icons and logos from HeyGen's free catalog when it's connected (Set up → HeyGen), pictures generated with Codex (a photo the catalog can't give is generated instead), a Gemini voiceover (your Gemini key; no HeyGen account needed) with caption timings, and **object sheets**: one picture of several separate objects on a plain background, cut into transparent PNGs with no AI model (`src-tauri/src/cutout.rs`).
3. **Build.** The worker composes `index.html` from the storyboard and the finished media, following the 21 HyperFrames skills (bundled; `scripts/build-hyperframes-skills.mjs`) and Jarvis's own `video-design` skill.
4. **Check and fix.** Jarvis runs HyperFrames' checker (structure, layout, motion, text contrast) and has the worker fix what it finds, up to twice.
5. **Draft and art director.** Jarvis renders a draft MP4 that plays in the panel, then shows 12 of its frames to Gemini, which names layout problems the checker can't see (text running through a drawing, a caption on a face). If it finds any, the worker fixes them once and the draft is re-rendered. “Render the final” makes the full-quality version.

Speech in files you give it (a talking video, an interview) is transcribed automatically, so captions and highlights land on the words really spoken (this installs the speech model the first time).

**Settings → Video** chooses who reads the narration: Gemini (default) or **ElevenLabs** with your own API key (voice list, model, exact word timing for captions). More services can be added later: one entry in `VideoSettings.tsx`, a provider in `src-tauri/src/voices.rs`, and a case in `service_test`. Keys are stored in Jarvis's settings and sent only to their own service.

Optional extras in Set up: **Exact caption timing** (a local speech model, about 700 MB, so captions follow the spoken words exactly; without it they are timed by estimate) and **HeyGen catalog** (a free sign-in that unlocks music, photos and icons; sound effects, pictures made with Codex, cut-outs and the Gemini voiceover work without it). Voiceover languages and the speech model: the model covers English and most European languages.

Developers: in a debug build, `JARVIS_VIDEO_RT=<folder>` points the toolbox somewhere else (to test setup without touching `~/Jarvis`), and `JARVIS_E2E_VIDEO=<brief.json> JARVIS_E2E_LOG=<file>` makes the app run a whole video by itself at start-up and log every stage (see `e2e_hook` in `videopipe.rs`). Never set them in a normal run: each start would make a video.

The worker's sandbox can't start a browser, a local server or the network, so it only writes files (and runs `./hf lint`). Everything that needs a browser or the internet is done by Jarvis between the steps. This also means a web page or a catalog entry can't talk the worker into fetching or sending anything. Changes (“make the intro faster”) go through the same check and draft steps.

## Shortcuts

- ⌥Space: talk / mute (works from any app)
- ⌥J: show / hide the window
- ⌥.: stop Jarvis mid-sentence
