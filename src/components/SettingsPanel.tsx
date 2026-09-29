import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
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
  const [codexModels, setCodexModels] = useState<CodexModels | null>(null);
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

  const checkCodex = async () => {
    setChecking(true);
    const status = await invoke<CodexStatus>("codex_status");
    setCodex(status);
    setChecking(false);
    if (status.loggedIn) setCodexModels(await invoke<CodexModels>("codex_models"));
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
    checkCodex();
    loadMics();
  }, []);

  const save = async () => {
    try {
      await invoke("save_settings", { settings: s });
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
              {checking
                ? "Checking Codex…"
                : codex
                  ? codex.loggedIn
                    ? `Ready · ${codex.version} · ${codex.path}`
                    : codex.message
                  : "Not checked"}
            </span>
            <button className="mini" onClick={checkCodex}>
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
