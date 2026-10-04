import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import type { Settings, VideoStatus } from "../lib/types";

/**
 * Outside services that videos can use. To add another, give it an entry here, give it a provider in
 * src-tauri/src/voices.rs (or wherever it is used) and a case in `service_test`. Its key is kept in
 * `Settings.serviceKeys[id]` and only ever sent to that service.
 */
const SERVICES = [
  {
    id: "elevenlabs",
    name: "ElevenLabs",
    blurb: "Natural narration in many voices and languages, with exact word timing for captions.",
    keyLabel: "ElevenLabs API key",
    keyHint: "Paste a key from elevenlabs.io (Profile, API Keys). It is used only for narration.",
    docs: "https://elevenlabs.io/app/settings/api-keys",
  },
];

const GEMINI_VOICES = ["Kore", "Charon", "Puck", "Fenrir", "Aoede", "Leda", "Orus", "Zephyr"];
const ELEVEN_MODELS = [
  { id: "eleven_multilingual_v2", name: "Multilingual v2 (best quality, 29 languages)" },
  { id: "eleven_turbo_v2_5", name: "Turbo v2.5 (faster, cheaper)" },
  { id: "eleven_flash_v2_5", name: "Flash v2.5 (fastest, cheapest)" },
];

interface Voice {
  id: string;
  name: string;
  category: string;
  description: string;
}

export default function VideoSettings({ s, set, onOpenSetup }: { s: Settings; set: <K extends keyof Settings>(k: K, v: Settings[K]) => void; onOpenSetup?: () => void }) {
  const [status, setStatus] = useState<VideoStatus | null>(null);
  const [voices, setVoices] = useState<Voice[]>([]);
  const [msg, setMsg] = useState<Record<string, { ok: boolean; text: string }>>({});
  const [busy, setBusy] = useState("");
  const keys = s.serviceKeys ?? {};
  const setKey = (id: string, v: string) => set("serviceKeys", { ...keys, [id]: v.trim() });

  useEffect(() => {
    invoke<VideoStatus>("video_status").then(setStatus).catch(() => {});
  }, []);

  const loadVoices = async () => {
    setBusy("voices");
    try {
      setVoices(await invoke<Voice[]>("elevenlabs_voices", { key: keys.elevenlabs || null }));
      setMsg((m) => ({ ...m, elevenlabs: { ok: true, text: "" } }));
    } catch (e) {
      setMsg((m) => ({ ...m, elevenlabs: { ok: false, text: String(e) } }));
    }
    setBusy("");
  };
  const test = async (id: string) => {
    setBusy(id);
    try {
      const text = await invoke<string>("service_test", { id, key: keys[id] || null });
      setMsg((m) => ({ ...m, [id]: { ok: true, text } }));
      if (id === "elevenlabs") loadVoices();
    } catch (e) {
      setMsg((m) => ({ ...m, [id]: { ok: false, text: String(e) } }));
    }
    setBusy("");
  };

  const provider = s.narrationProvider || "auto";
  const eleven = !!keys.elevenlabs;
  const voiceInList = voices.some((v) => v.id === s.elevenlabsVoice);

  return (
    <>
      <label htmlFor="narr">Who reads the narration?</label>
      <select id="narr" value={provider} onChange={(e) => set("narrationProvider", e.target.value)}>
        <option value="auto">Automatic (ElevenLabs when it has a key, otherwise Gemini)</option>
        <option value="gemini">Gemini voice</option>
        <option value="elevenlabs">ElevenLabs</option>
      </select>
      {provider === "elevenlabs" && !eleven && <p className="hint warn">Add an ElevenLabs key below, or videos will use Gemini's voice.</p>}

      {provider !== "elevenlabs" && (
        <>
          <label htmlFor="gvoice">Gemini narration voice</label>
          <select id="gvoice" value={s.narrationGeminiVoice || "Kore"} onChange={(e) => set("narrationGeminiVoice", e.target.value)}>
            {GEMINI_VOICES.map((v) => (
              <option key={v}>{v}</option>
            ))}
          </select>
        </>
      )}

      <div className="label" style={{ marginTop: 18 }}>
        Services
      </div>
      {SERVICES.map((svc) => (
        <div key={svc.id} className="svc">
          <b>{svc.name}</b>
          <p className="hint">{svc.blurb}</p>
          <label htmlFor={`k-${svc.id}`}>{svc.keyLabel}</label>
          <input id={`k-${svc.id}`} type="password" autoComplete="off" value={keys[svc.id] ?? ""} placeholder={svc.keyHint} onChange={(e) => setKey(svc.id, e.target.value)} />
          <div className="row" style={{ alignItems: "center", gap: 8 }}>
            <button className="mini" type="button" onClick={() => test(svc.id)} disabled={!keys[svc.id] || busy === svc.id}>
              {busy === svc.id ? "Checking…" : "Check the key"}
            </button>
            <button className="mini" type="button" onClick={() => openUrl(svc.docs).catch(() => {})}>
              Get a key
            </button>
            {msg[svc.id]?.text && <span className={`hint ${msg[svc.id].ok ? "" : "warn"}`}>{msg[svc.id].text}</span>}
          </div>

          {svc.id === "elevenlabs" && eleven && (
            <>
              <label htmlFor="evoice">ElevenLabs voice</label>
              <div className="row" style={{ gap: 8 }}>
                <select id="evoice" value={s.elevenlabsVoice} onChange={(e) => set("elevenlabsVoice", e.target.value)}>
                  <option value="">Rachel (default)</option>
                  {s.elevenlabsVoice && !voiceInList && <option value={s.elevenlabsVoice}>Saved voice ({s.elevenlabsVoice.slice(0, 8)}…)</option>}
                  {voices.map((v) => (
                    <option key={v.id} value={v.id}>
                      {v.name}
                      {v.category && v.category !== "premade" ? ` (${v.category})` : ""}
                      {v.description ? ` · ${v.description}` : ""}
                    </option>
                  ))}
                </select>
                <button className="mini" type="button" onClick={loadVoices} disabled={busy === "voices"}>
                  {busy === "voices" ? "Loading…" : "Load my voices"}
                </button>
              </div>
              <label htmlFor="emodel">Model</label>
              <select id="emodel" value={s.elevenlabsModel || "eleven_multilingual_v2"} onChange={(e) => set("elevenlabsModel", e.target.value)}>
                {ELEVEN_MODELS.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.name}
                  </option>
                ))}
              </select>
              <p className="hint">The script is sent to ElevenLabs to be spoken, and uses your ElevenLabs credits. Save settings to apply.</p>
            </>
          )}
        </div>
      ))}
      <p className="hint">More services (music, images, stock footage) can be added here later.</p>

      <div className="label" style={{ marginTop: 18 }}>
        Video tools
      </div>
      <div className={`status-line ${status?.ready ? "ok" : "bad"}`}>
        <i />
        <span>{status ? (status.ready ? "Video tools are installed." : status.message || "Video tools aren't installed yet.") : "Checking…"}</span>
        {status && !status.ready && onOpenSetup && (
          <button className="mini" type="button" onClick={onOpenSetup}>
            Set up
          </button>
        )}
      </div>
      <div className={`status-line ${status?.captions ? "ok" : ""}`}>
        <i />
        <span>{status?.captions ? "Exact caption timing is on (speech model installed)." : "Caption timing is estimated. Installing the speech model makes it exact."}</span>
      </div>
    </>
  );
}
