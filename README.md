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
4. Settings opens on first launch. Paste the key, pick a voice, and check that the Codex status is green.
5. Press the mic button (or ⌥Space) and say something like “Jarvis, research the best vector databases for a small startup, deep dive.”

Use headphones if Jarvis hears itself through your speakers.

Build a real `.app` / `.dmg` with `npm run tauri build`. The output goes to `src-tauri/target/release/bundle/`.

## Where things live

| What | Where |
|---|---|
| Research reports | `~/Jarvis/research/0001-topic/report.md` (+ `summary.txt`) |
| Task history | `~/Jarvis/tasks.json` |
| Notes (“remember that…”) | `~/Jarvis/notes.md`, loaded into Jarvis's instructions each session |
| Settings (incl. API key) | `~/Library/Application Support/co.fikraventures.jarvis/settings.json` (mode 600) |

## Code map

| File | Role |
|---|---|
| `src/lib/jarvis.ts` | Orchestrator: session, tool handling, transcript, delivering worker results |
| `src/lib/live.ts` | Gemini Live WebSocket client, model discovery, auto-reconnect with session resumption |
| `src/lib/audio.ts`, `public/pcm-worklet.js` | Mic → 16 kHz PCM, 24 kHz playback with instant interrupt, audio levels |
| `src/components/Orb.tsx` | Three.js voice sphere driven by real audio levels |
| `src-tauri/src/tasks.rs` | Runs Codex, parses its JSON events into steps, follow-ups via `codex exec resume` |
| `src-tauri/src/lib.rs` | Window vibrancy (glass), menu bar icon, global shortcuts, hide-on-close |

## Shortcuts

- ⌥Space: talk / mute (works from any app)
- ⌥J: show / hide the window
- ⌥.: stop Jarvis mid-sentence
