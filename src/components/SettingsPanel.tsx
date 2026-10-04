import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { confirm } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useRef, useState } from "react";
import { DEFAULT_RULE, RISK_INFO, ruleFor, type Risk, type Rule } from "../lib/approvals";
import { LANGUAGES } from "../lib/languages";
import { listLiveModels } from "../lib/live";
import VideoSettings from "./VideoSettings";
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

/** The sections down the left. `words` are what the search box also matches. */
const NAV = [
  { id: "general", label: "General", words: "name call you notifications notify", icon: "M12 8.5a3.5 3.5 0 1 1 0 7 3.5 3.5 0 0 1 0-7zm0 2a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3zM10.7 2h2.6l.5 2.4 1.7.7 2.1-1.3 1.8 1.8-1.3 2.1.7 1.7 2.4.5v2.6l-2.4.5-.7 1.7 1.3 2.1-1.8 1.8-2.1-1.3-1.7.7-.5 2.4h-2.6l-.5-2.4-1.7-.7-2.1 1.3-1.8-1.8 1.3-2.1-.7-1.7L2 13.3v-2.6l2.4-.5.7-1.7-1.3-2.1 1.8-1.8 2.1 1.3 1.7-.7z" },
  { id: "voice", label: "Voice", words: "gemini api key model language speak nepali english hindi", icon: "M12 3a3 3 0 0 1 3 3v6a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3zm-5 8h2a3 3 0 0 0 6 0h2a5 5 0 0 1-4 4.9V19h-2v-3.1A5 5 0 0 1 7 11z" },
  { id: "video", label: "Video", words: "video narration voiceover elevenlabs voice api key services music captions hyperframes", icon: "M4 6h11a1 1 0 0 1 1 1v3.5l5-3v9l-5-3V17a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1z" },
  { id: "mic", label: "Microphone", words: "mic headphones hey jarvis wake word listen input", icon: "M5 10h2v4H5zm4-4h2v12H9zm4 2h2v8h-2zm4 2h2v4h-2z" },
  { id: "research", label: "Research", words: "codex model thinking folder web search worker", icon: "M10.5 3a7.5 7.5 0 0 1 5.9 12.1l4.3 4.3-1.4 1.4-4.3-4.3A7.5 7.5 0 1 1 10.5 3zm0 2a5.5 5.5 0 1 0 0 11 5.5 5.5 0 0 0 0-11z" },
  { id: "google", label: "Apps", words: "google gmail calendar mail drive docs sheets youtube connect account sign in integrations", icon: "M3 5h18a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1zm1 2v.5l8 5 8-5V7zm16 2.9-8 5-8-5V17h16z" },
  { id: "browser", label: "Browser", words: "chrome playwright websites tasks profile", icon: "M12 3a9 9 0 1 1 0 18 9 9 0 0 1 0-18zm0 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14zm-1 2h2v5h-2zm0 7h2v2h-2z" },
  { id: "screen", label: "Screen & approvals", words: "accessibility screen recording permission approve ask never audit write send", icon: "M12 2 4 5v6c0 5 3.4 9.7 8 11 4.6-1.3 8-6 8-11V5zm0 2.1 6 2.2V11c0 3.9-2.5 7.6-6 8.9-3.5-1.3-6-5-6-8.9V6.3zM11 8h2v5h-2zm0 6h2v2h-2z" },
];

export default function SettingsPanel({ initial, onClose, onSaved, onOpenSetup }: { initial: Settings; onClose: () => void; onSaved: () => void; onOpenSetup?: () => void }) {
  const [s, setS] = useState<Settings>(initial);
  const [models, setModels] = useState<string[]>([]);
  const [modelMsg, setModelMsg] = useState("");
  const [codex, setCodex] = useState<CodexStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [installMsg, setInstallMsg] = useState("");
  const triedInstall = useRef(false);
  const [tab, setTab] = useState("general");
  const [search, setSearch] = useState("");
  const [trusted, setTrusted] = useState<boolean | null>(null);
  const [screenOk, setScreenOk] = useState<boolean | null>(null);
  const checkTrusted = () =>
    invoke<{ trusted: boolean; screen: boolean }>("context_status")
      .then((r) => {
        setTrusted(r.trusted);
        setScreenOk(r.screen);
      })
      .catch(() => {
        setTrusted(false);
        setScreenOk(false);
      });
  const setRule = (risk: Risk, rule: Rule) => setS((x) => ({ ...x, approvals: { ...(x.approvals ?? {}), [risk]: rule } }));
  const [codexModels, setCodexModels] = useState<CodexModels | null>(null);
  const [google, setGoogle] = useState<{ configured: boolean; connected: boolean; email: string; services: { id: string; label: string; connected: boolean; email: string }[] } | null>(null);
  const [googleBusy, setGoogleBusy] = useState("");
  const [googleError, setGoogleError] = useState("");
  const loadGoogle = () => invoke<typeof google>("google_status").then(setGoogle).catch(() => {});
  const connectGoogle = async (service: string) => {
    setGoogleError("");
    setGoogleBusy(service);
    try {
      await invoke<string>("google_connect", { service });
    } catch (e) {
      setGoogleError(String(e));
    }
    setGoogleBusy("");
    loadGoogle();
  };
  const disconnectGoogle = async (service: string) => {
    await invoke("google_disconnect", { service }).catch((e) => setGoogleError(String(e)));
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

  // Signing in to Codex with a ChatGPT account, right here.
  const [loginBusy, setLoginBusy] = useState(false);
  const [loginLines, setLoginLines] = useState<string[]>([]);
  const [loginDevice, setLoginDevice] = useState(false);
  const loginLink = loginLines.map((l) => l.match(/https?:\/\/\S+/)?.[0]).find(Boolean);
  const signIn = async (device = false) => {
    setLoginBusy(true);
    setLoginDevice(device);
    setLoginLines([]);
    try {
      await applyStatus(await invoke<CodexStatus>("codex_login", { device }));
    } catch (e) {
      setCodex((c) => (c ? { ...c, message: `Sign-in failed: ${e}` } : c));
    }
    setLoginBusy(false);
  };
  const signOut = async () => {
    if (!(await confirm("Research and code fixes stop working until you sign in again.", { title: "Sign out of Codex?", kind: "warning", okLabel: "Sign out" }))) return;
    setCodexModels(null);
    await applyStatus(await invoke<CodexStatus>("codex_logout"));
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
    checkTrusted();
    window.addEventListener("focus", checkTrusted);
    const un = listen<string>("codex-install", (e) => setInstallMsg(e.payload));
    const unLogin = listen<string>("codex-login", (e) => setLoginLines((l) => [...l, e.payload].slice(-6)));
    return () => {
      un.then((f) => f());
      unLogin.then((f) => f());
      window.removeEventListener("focus", checkTrusted);
    };
  }, []);

  const save = async () => {
    try {
      await invoke("save_settings", { settings: s });
      // Starts or stops listening for "hey Jarvis", and picks up a changed microphone.
      await invoke("wake_word_set", { enabled: !!s.wakeWord });
      setSaved("Saved");
      onSaved();
      onClose();
    } catch (e) {
      setSaved(`Could not save: ${e}`);
    }
  };

  const q = search.trim().toLowerCase();
  const matches = (n: (typeof NAV)[number]) => !q || `${n.label} ${n.words}`.toLowerCase().includes(q);
  const found = NAV.filter(matches);
  /** With a search, every matching section shows; otherwise just the chosen one. */
  const show = (id: string) => (q ? found.some((n) => n.id === id) : tab === id);
  const title = q ? (found.length ? "Search results" : "Nothing found") : NAV.find((n) => n.id === tab)?.label;

  return (
    <div className="overlay" onClick={onClose}>
      <div className="settings settings-nav glass" onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Settings">
        <nav className="snav" aria-label="Settings sections">
          <label className="snav-search">
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M10.5 3a7.5 7.5 0 0 1 5.9 12.1l4.3 4.3-1.4 1.4-4.3-4.3A7.5 7.5 0 1 1 10.5 3zm0 2a5.5 5.5 0 1 0 0 11 5.5 5.5 0 0 0 0-11z" />
            </svg>
            <input type="search" placeholder="Search" value={search} onChange={(e) => setSearch(e.target.value)} aria-label="Search settings" />
          </label>
          <div className="snav-group">Settings</div>
          {NAV.map((n) => (
            <button
              key={n.id}
              type="button"
              className={`snav-item ${!q && tab === n.id ? "on" : ""} ${q && !matches(n) ? "dim" : ""}`}
              aria-current={!q && tab === n.id ? "page" : undefined}
              onClick={() => {
                setTab(n.id);
                setSearch("");
              }}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d={n.icon} />
              </svg>
              {n.label}
            </button>
          ))}
        </nav>

        <div className="spane">
          <header>
            <h2>{title}</h2>
            <button className="icon-btn" onClick={onClose} aria-label="Close settings" title="Close">
              <svg viewBox="0 0 24 24">
                <path d="m6.4 5 5.6 5.6L17.6 5 19 6.4 13.4 12l5.6 5.6-1.4 1.4-5.6-5.6L6.4 19 5 17.6l5.6-5.6L5 6.4z" />
              </svg>
            </button>
          </header>

          <div className="settings-body">
          <section hidden={!show("general")}>
            {q && <div className="label">{NAV.find((n) => n.id === "general")?.label}</div>}
          <label htmlFor="name">What should Jarvis call you?</label>
          <input id="name" value={s.userName} placeholder="e.g. Kaushal" onChange={(e) => set("userName", e.target.value)} />
          <label className="check">
            <input id="notify" type="checkbox" checked={s.notify ?? true} onChange={(e) => set("notify", e.target.checked)} />
            Notify me when work finishes while Jarvis isn't in front
          </label>
          </section>
          <section hidden={!show("voice")}>
            {q && <div className="label">{NAV.find((n) => n.id === "voice")?.label}</div>}
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
              ? "Jarvis speaks only this language, even if your speech sounds like another. Ask it in words to switch."
              : "Jarvis replies in whatever language you speak to it."}
          </p>
          {modelMsg && <p className="hint">{modelMsg}</p>}
          </section>
          <section hidden={!show("video")}>
            {q && <div className="label">{NAV.find((n) => n.id === "video")?.label}</div>}
            <VideoSettings s={s} set={set} onOpenSetup={onOpenSetup} />
          </section>
          <section hidden={!show("mic")}>
            {q && <div className="label">{NAV.find((n) => n.id === "mic")?.label}</div>}
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
          </section>
          <section hidden={!show("research")}>
            {q && <div className="label">{NAV.find((n) => n.id === "research")?.label}</div>}
          <div className={`status-line ${codex?.loggedIn ? "ok" : "bad"}`}>
            <i />
            <span>
              {installing
                ? installMsg || "Installing Codex…"
                : checking
                  ? "Checking Codex…"
                  : loginBusy
                    ? "Waiting for you to finish signing in…"
                    : codex
                      ? codex.loggedIn
                        ? `${codex.message || "Signed in"} · Codex ${codex.version.replace(/^codex-cli\s*/i, "")}`
                        : codex.found
                          ? "Not signed in to ChatGPT"
                          : codex.message
                      : "Not checked"}
            </span>
            {codex && !codex.found && !installing && (
              <button className="mini" onClick={installCodex}>
                Install
              </button>
            )}
            {codex?.found && !codex.loggedIn && !loginBusy && (
              <button className="btn primary" onClick={() => signIn(false)} disabled={checking || installing}>
                Sign in with ChatGPT
              </button>
            )}
            {loginBusy && (
              <button className="mini" onClick={() => invoke("codex_login_cancel")}>
                Cancel
              </button>
            )}
            {codex?.loggedIn && (
              <button className="mini" onClick={signOut}>
                Sign out
              </button>
            )}
            <button className="mini" onClick={() => checkCodex()} disabled={checking || installing || loginBusy}>
              Check again
            </button>
          </div>
          {codex?.found && !codex.loggedIn && !loginBusy && (
            <>
              <p className="hint">Research and code fixes run on your ChatGPT plan (Plus, Pro, Business or Enterprise). A ChatGPT sign-in page opens in your browser; Jarvis never sees your password.</p>
              {codex.message && !codex.message.startsWith("Not signed in") && <p className="hint warn">{codex.message}</p>}
              <button className="linkbtn" onClick={() => signIn(true)}>
                Browser not opening? Sign in with a code instead
              </button>
            </>
          )}
          {loginBusy && (
            <div className="loginbox">
              <p className="hint">{loginDevice ? "Open the page below on any device and enter the code shown." : "Finish signing in in your browser, then come back here."}</p>
              {loginLink && (
                <button className="linkbtn" onClick={() => openUrl(loginLink).catch(() => {})}>
                  Open the sign-in page
                </button>
              )}
              {loginDevice && loginLines.length > 0 && <pre className="approval-detail">{loginLines.join("\n")}</pre>}
            </div>
          )}
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
          </section>
          <section hidden={!show("google")}>
            {q && <div className="label">{NAV.find((n) => n.id === "google")?.label}</div>}
          <div className="label">Google</div>
          {google && !google.configured && <p className="hint warn">Google sign-in isn't set up in this build (see .env).</p>}
          {(google?.services ?? []).map((svc) => (
            <div key={svc.id} className={`status-line ${svc.connected ? "ok" : "bad"}`}>
              <i />
              <span>
                <b>{svc.label}</b>
                {" · "}
                {googleBusy === svc.id ? "Finish signing in in your browser…" : svc.connected ? `Connected${svc.email ? ` as ${svc.email}` : ""}` : "Not connected"}
              </span>
              {svc.connected ? (
                <button className="mini" onClick={() => disconnectGoogle(svc.id)}>
                  Disconnect
                </button>
              ) : (
                <button className="mini" onClick={() => connectGoogle(svc.id)} disabled={!google?.configured || !!googleBusy}>
                  Connect
                </button>
              )}
            </div>
          ))}
          {googleError && <p className="hint warn">{googleError}</p>}
          <p className="hint">
            Each app is connected on its own, so you only give Jarvis what you want it to use: mail and calendar, files in Drive with Docs and Sheets, or YouTube search. It never sends an
            email, invites anyone or changes a file until you confirm it on screen. Access stays on this computer; Disconnect removes it at Google too. Other providers can be added here
            later.
          </p>
          </section>
          <section hidden={!show("browser")}>
            {q && <div className="label">{NAV.find((n) => n.id === "browser")?.label}</div>}
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
          <section hidden={!show("screen")}>
            {q && <div className="label">{NAV.find((n) => n.id === "screen")?.label}</div>}
          <div className={`status-line ${trusted ? "ok" : "bad"}`}>
            <i />
            <span>{trusted ? "Jarvis can see which app is in front and the text you select." : "Accessibility is off, so Jarvis can't see your selected text or paste into other apps."}</span>
            {!trusted && (
              <button className="mini" onClick={() => invoke("request_accessibility").then(() => setTimeout(checkTrusted, 800))}>
                Turn on
              </button>
            )}
          </div>
          <div className={`status-line ${screenOk ? "ok" : "bad"}`}>
            <i />
            <span>{screenOk ? "Jarvis can look at a window when you ask." : "Screen Recording is off, so Jarvis can't look at a window. After turning it on, quit and reopen Jarvis."}</span>
            {!screenOk && (
              <button className="mini" onClick={() => invoke("request_screen_recording").then(() => setTimeout(checkTrusted, 800))}>
                Turn on
              </button>
            )}
          </div>
          <p className="hint">Jarvis looks only when you ask ("explain this", "fix this error"), never in the background. Text you select is sent to Gemini and, for code fixes, to Codex.</p>
          <label>When Jarvis wants to…</label>
          <div className="rule-table">
            {(["write", "send"] as Risk[]).map((risk) => (
              <div className="rule-row" key={risk}>
                <div>
                  <b>{RISK_INFO[risk].label}</b>
                  <small>{RISK_INFO[risk].hint}</small>
                </div>
                <select aria-label={RISK_INFO[risk].label} value={ruleFor(risk, s)} onChange={(e) => setRule(risk, e.target.value as Rule)}>
                  {RISK_INFO[risk].rules.map((r) => (
                    <option key={r} value={r}>
                      {r === "ask" ? "Ask me" : r === "auto" ? "Do it" : "Never"}
                      {r === DEFAULT_RULE[risk] ? " (default)" : ""}
                    </option>
                  ))}
                </select>
              </div>
            ))}
          </div>
          <p className="hint">Every approval, rejection and automatic action is logged in audit.jsonl in your Jarvis folder.</p>
          </section>
          </div>

          <footer>
            <span className="hint">{saved || "Settings are stored on this Mac only."}</span>
            <button className="btn primary" onClick={save}>
              Save
            </button>
          </footer>
        </div>
      </div>
    </div>
  );
}
