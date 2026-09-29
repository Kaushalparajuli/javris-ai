import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import { LANGUAGES } from "../lib/languages";
import { listLiveModels } from "../lib/live";
import type { CodexModels, CodexStatus, MicDevice, Settings } from "../lib/types";

const VOICES = ["Charon", "Puck", "Kore", "Fenrir", "Aoede", "Leda", "Orus", "Zephyr"];

const LEVEL_NAMES: Record<string, string> = {
  minimal: "Minimal",
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra high",
  max: "Max",
  ultra: "Ultra",
};
const levelName = (l: string) => LEVEL_NAMES[l] ?? l;

export default function SettingsPanel({ initial, onClose, onSaved }: { initial: Settings; onClose: () => void; onSaved: () => void }) {
  const [s, setS] = useState<Settings>(initial);
  const [models, setModels] = useState<string[]>([]);
  const [modelMsg, setModelMsg] = useState("");
  const [codex, setCodex] = useState<CodexStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [installMsg, setInstallMsg] = useState("");
  const triedInstall = useRef(false);
  const [codexModels, setCodexModels] = useState<CodexModels | null>(null);
  const [google, setGoogle] = useState<{ configured: boolean; connected: boolean; email: string } | null>(null);
  const [googleBusy, setGoogleBusy] = useState(false);
  const [googleError, setGoogleError] = useState("");
  const loadGoogle = () => invoke<typeof google>("google_status").then(setGoogle).catch(() => {});
  const connectGoogle = async () => {
    setGoogleError("");
    setGoogleBusy(true);
    try {
      // Sign-in reads the client ID from saved settings, so save first.
      await invoke("save_settings", { settings: s });
      await invoke<string>("google_connect");
    } catch (e) {
      setGoogleError(String(e));
    }
    setGoogleBusy(false);
    loadGoogle();
  };
  const disconnectGoogle = async () => {
    await invoke("google_disconnect").catch((e) => setGoogleError(String(e)));
    loadGoogle();
  };
  const [browser, setBrowser] = useState<{ installed: boolean; browser: string; message: string } | null>(null);
  const [browserBusy, setBrowserBusy] = useState(false);
  const loadBrowser = () => invoke<typeof browser>("browser_status").then(setBrowser).catch(() => {});
  const setUpBrowser = async () => {
    setBrowserBusy(true);
    try {
      setBrowser(await invoke<typeof browser>("install_browser_tools"));
    } catch (e) {
      setBrowser((b) => ({ installed: false, browser: b?.browser ?? "", message: String(e) }));
    }
    setBrowserBusy(false);
  };
  const [mics, setMics] = useState<MicDevice[] | null>(null);
  const loadMics = () => invoke<MicDevice[]>("list_mics").then(setMics).catch(() => setMics([]));
  const [saved, setSaved] = useState("");

  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => setS((x) => ({ ...x, [k]: v }));

  const loadModels = async (key: string) => {
    if (!key) return;
    setModelMsg("Loading models…");
    try {
      const list = await listLiveModels(key);
      setModels(list);
      setModelMsg(list.length ? `${list.length} Live models available. Auto picks ${list[0]}.` : "This key has no Live models.");
    } catch (e) {
      setModelMsg(`Could not load models: ${e}`);
    }
  };

  const applyStatus = async (status: CodexStatus) => {
    setCodex(status);
    if (status.loggedIn) setCodexModels(await invoke<CodexModels>("codex_models"));
  };

  // Fetch Codex into Jarvis's own folder so research works without opening a terminal.
  const installCodex = async () => {
    triedInstall.current = true;
    setInstalling(true);
    setInstallMsg("Starting…");
    try {
      await applyStatus(await invoke<CodexStatus>("install_codex"));
    } catch (e) {
      setCodex({ found: false, path: "", version: "", loggedIn: false, message: `Could not install Codex: ${e}` });
    }
    setInstalling(false);
    setInstallMsg("");
  };

  const checkCodex = async (autoInstall = false) => {
    setChecking(true);
    const status = await invoke<CodexStatus>("codex_status");
    setChecking(false);
    await applyStatus(status);
    if (autoInstall && !status.found && !triedInstall.current) await installCodex();
  };

  // The model Codex will actually use, and the thinking levels it supports.
  const activeSlug = s.codexModel || codexModels?.defaultModel || "";
  const active = codexModels?.models.find((m) => m.slug === activeSlug);
  const levels = active?.reasoningLevels ?? [];
  const pickModel = (slug: string) => {
    const next = codexModels?.models.find((m) => m.slug === (slug || codexModels.defaultModel));
    const supported = next?.reasoningLevels.some((l) => l.effort === s.codexReasoning);
    setS((x) => ({
      ...x,
      codexModel: slug,
      codexReasoning: ["auto", "default"].includes(x.codexReasoning) || supported ? x.codexReasoning : "auto",
    }));
  };
  const reasoningHint =
    s.codexReasoning === "auto"
      ? "Quick tasks think Low, deep dives think High."
      : s.codexReasoning === "default"
        ? `Uses your Codex setting${codexModels?.defaultReasoning ? ` (${levelName(codexModels.defaultReasoning)})` : ""}.`
        : levels.find((l) => l.effort === s.codexReasoning)?.description ?? "";

  useEffect(() => {
    loadModels(initial.geminiApiKey);
    checkCodex(true);
    loadMics();
    loadBrowser();
    loadGoogle();
    const un = listen<string>("codex-install", (e) => setInstallMsg(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  const save = async () => {
    try {
      await invoke("save_settings", { settings: s });
      // Starts or stops listening for "hey Jarvis", and picks up a changed microphone.
      await invoke("wake_word_set", { enabled: !!s.wakeWord });
      setSaved("Saved");
      onSaved();
    } catch (e) {
      setSaved(`Could not save: ${e}`);
    }
  };

  return (
    <div className="overlay" onClick={onClose}>
      <div className="settings glass" onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Settings">
        <header>
          <h2>Settings</h2>
          <button className="mini" onClick={onClose}>
            Close
          </button>
        </header>

        <section>
          <div className="label">Voice · Gemini Live</div>
          <label htmlFor="key">Gemini API key</label>
          <input
            id="key"
            type="password"
            value={s.geminiApiKey}
            placeholder="Paste a key from aistudio.google.com"
            onChange={(e) => set("geminiApiKey", e.target.value.trim())}
            onBlur={(e) => loadModels(e.target.value.trim())}
          />
          <div className="row">
            <div>
              <label htmlFor="model">Live model</label>
              <select id="model" value={s.geminiModel} onChange={(e) => set("geminiModel", e.target.value)}>
                <option value="">Auto (newest)</option>
                {models.map((m) => (
                  <option key={m} value={m}>
                    {m}
                  </option>
                ))}
              </select>
            </div>
            <div>
              <label htmlFor="voice">Voice</label>
              <select id="voice" value={s.voice} onChange={(e) => set("voice", e.target.value)}>
                {VOICES.map((v) => (
                  <option key={v}>{v}</option>
                ))}
              </select>
            </div>
          </div>
          <label htmlFor="language">Language</label>
          <select id="language" value={s.language} onChange={(e) => set("language", e.target.value)}>
            <option value="">Auto (match whoever is talking)</option>
            {LANGUAGES.map((l) => (
              <option key={l.code} value={l.code}>
                {l.name}
              </option>
            ))}
          </select>
          <p className="hint">
            {s.language
              ? "Jarvis opens in this language and switches if you speak another one."
              : "Jarvis replies in whatever language you speak to it."}
          </p>
          {modelMsg && <p className="hint">{modelMsg}</p>}
          <label htmlFor="mic">Microphone</label>
          <div className="inline">
            <select id="mic" value={s.micDevice} onChange={(e) => set("micDevice", e.target.value)}>
              <option value="">System default{mics?.find((m) => m.isDefault) ? ` (${mics.find((m) => m.isDefault)!.name})` : ""}</option>
              {mics?.map((m) => (
                <option key={m.name} value={m.name}>
                  {m.name}
                </option>
              ))}
              {s.micDevice && mics && !mics.some((m) => m.name === s.micDevice) && <option value={s.micDevice}>{s.micDevice} (not connected)</option>}
            </select>
            <button className="mini" type="button" onClick={loadMics}>
              Refresh
            </button>
          </div>
          {mics && mics.length === 0 && (
            <p className="hint warn">No microphone is connected. This Mac has no built-in mic: connect AirPods, a headset or USB mic, or use your iPhone (System Settings → Sound → Input), then press Refresh.</p>
          )}
          <label className="check">
            <input id="wake" type="checkbox" checked={!!s.wakeWord} onChange={(e) => set("wakeWord", e.target.checked)} />
            Listen for “Hey Jarvis”
          </label>
          <p className="hint">
            Recognised on this computer: audio isn't sent anywhere until you say it. Your system shows the microphone indicator while Jarvis listens. Saying “Jarvis” on its
            own can wake it too.
          </p>
          <label className="check">
            <input id="headphones" type="checkbox" checked={s.headphones} onChange={(e) => set("headphones", e.target.checked)} />
            I'm using headphones (interrupt Jarvis by talking)
          </label>
          <p className="hint">Without headphones, Jarvis stops listening while it speaks so it doesn't hear itself. Press ⌥. to cut it off.</p>
          <label htmlFor="name">What should Jarvis call you?</label>
          <input id="name" value={s.userName} placeholder="e.g. Kaushal" onChange={(e) => set("userName", e.target.value)} />
        </section>

        <section>
          <div className="label">Research worker · Codex</div>
          <div className={`status-line ${codex?.loggedIn ? "ok" : "bad"}`}>
            <i />
            <span>
              {installing
                ? installMsg || "Installing Codex…"
                : checking
                  ? "Checking Codex…"
                  : codex
                    ? codex.loggedIn
                      ? `Ready · ${codex.version} · ${codex.path}`
                      : codex.message
                    : "Not checked"}
            </span>
            {codex && !codex.found && !installing && (
              <button className="mini" onClick={installCodex}>
                Install
              </button>
            )}
            <button className="mini" onClick={() => checkCodex()} disabled={checking || installing}>
              Check again
            </button>
          </div>
          <label htmlFor="codexPath">Codex path (optional)</label>
          <input id="codexPath" value={s.codexPath} placeholder="Found automatically, e.g. /opt/homebrew/bin/codex" onChange={(e) => set("codexPath", e.target.value)} />
          <div className="row">
            <div>
              <label htmlFor="codexModel">Codex model</label>
              <select id="codexModel" value={s.codexModel} onChange={(e) => pickModel(e.target.value)}>
                <option value="">
                  Codex default
                  {codexModels?.defaultModel
                    ? ` (${codexModels.models.find((m) => m.slug === codexModels.defaultModel)?.displayName ?? codexModels.defaultModel})`
                    : ""}
                </option>
                {codexModels?.models.map((m) => (
                  <option key={m.slug} value={m.slug}>
                    {m.displayName}
                  </option>
                ))}
                {s.codexModel && !codexModels?.models.some((m) => m.slug === s.codexModel) && <option value={s.codexModel}>{s.codexModel}</option>}
              </select>
            </div>
            <div>
              <label htmlFor="reasoning">Thinking</label>
              <select id="reasoning" value={s.codexReasoning || "auto"} onChange={(e) => set("codexReasoning", e.target.value)}>
                <option value="auto">Auto (by task depth)</option>
                <option value="default">Codex default</option>
                {levels.map((l) => (
                  <option key={l.effort} value={l.effort}>
                    {levelName(l.effort)}
                  </option>
                ))}
              </select>
            </div>
          </div>
          <p className="hint">
            {active?.description ? `${active.displayName}: ${active.description} ` : ""}
            {reasoningHint}
            {codexModels?.error ? ` ${codexModels.error}` : ""}
          </p>
          <label htmlFor="dir">Research folder</label>
          <input id="dir" value={s.researchDir} placeholder="~/Jarvis" onChange={(e) => set("researchDir", e.target.value)} />
          <label className="check">
            <input id="web" type="checkbox" checked={s.webSearch} onChange={(e) => set("webSearch", e.target.checked)} />
            Let Codex search the web
          </label>
          <label className="check">
            <input id="notify" type="checkbox" checked={s.notify ?? true} onChange={(e) => set("notify", e.target.checked)} />
            Notify me when work finishes while Jarvis isn't in front
          </label>
        </section>

        <section>
          <div className="label">Google · Calendar and Gmail</div>
          <div className={`status-line ${google?.connected ? "ok" : "bad"}`}>
            <i />
            <span>
              {googleBusy
                ? "Finish signing in in your browser…"
                : google?.connected
                  ? `Connected${google.email ? ` as ${google.email}` : ""}`
                  : s.googleClientId?.trim()
                    ? "Not connected"
                    : "Add your Google client ID below, then connect"}
            </span>
            {google?.connected ? (
              <button className="mini" onClick={disconnectGoogle}>
                Disconnect
              </button>
            ) : (
              <button className="mini" onClick={connectGoogle} disabled={!s.googleClientId?.trim() || googleBusy}>
                Connect
              </button>
            )}
          </div>
          {googleError && <p className="hint warn">{googleError}</p>}
          <label htmlFor="gid">Client ID</label>
          <input id="gid" value={s.googleClientId ?? ""} placeholder="…apps.googleusercontent.com" onChange={(e) => set("googleClientId", e.target.value.trim())} />
          <label htmlFor="gsecret">Client secret</label>
          <input id="gsecret" type="password" value={s.googleClientSecret ?? ""} placeholder="GOCSPX-…" onChange={(e) => set("googleClientSecret", e.target.value.trim())} />
          <details className="setup">
            <summary>How to get these (once, about five minutes)</summary>
            <ol>
              <li>Open console.cloud.google.com and create a project, or pick one.</li>
              <li>Under APIs &amp; Services → Library, enable the Gmail API and the Google Calendar API.</li>
              <li>
                Under APIs &amp; Services → OAuth consent screen, name the app “Jarvis”. Choose Internal if you use Google Workspace. Otherwise choose External and add your
                own address as a test user; in that case Google asks you to connect again about once a week.
              </li>
              <li>Under APIs &amp; Services → Credentials, create an OAuth client ID with the type Desktop app.</li>
              <li>Paste its client ID and secret here, then press Connect and approve access in your browser.</li>
            </ol>
          </details>
          <p className="hint">
            Jarvis can read your calendar and mail and write drafts. It never sends an email or invites anyone until you confirm it on screen. Access stays on this computer;
            Disconnect removes it at Google too.
          </p>
        </section>

        <section>
          <div className="label">Browser · for browser tasks</div>
          <div className={`status-line ${browser?.installed ? "ok" : "bad"}`}>
            <i />
            <span>{browserBusy ? "Setting up the browser tools…" : browser?.message ?? "Checking…"}</span>
            {browser && !browser.installed && browser.browser && !browserBusy && (
              <button className="mini" onClick={setUpBrowser}>
                Set up
              </button>
            )}
          </div>
          <p className="hint">
            Browser tasks run in Jarvis's own hidden browser and show live in the side panel. It has its own profile, separate from yours, so it isn't signed in
            anywhere. To let it use a site you have an account on, click into the live view when Codex isn't using it and sign in, or open Jarvis's browser
            here and sign in once.
          </p>
          <button className="mini" onClick={() => invoke("open_browser_profile").catch((e) => setBrowser((b) => (b ? { ...b, message: String(e) } : b)))} disabled={!browser?.browser}>
            Open Jarvis's browser
          </button>
        </section>

        <footer>
          <span className="hint">{saved || "Settings are stored on this Mac only."}</span>
          <button className="btn primary" onClick={save}>
            Save
          </button>
        </footer>
      </div>
    </div>
  );
}
