import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import type { CodexStatus, Settings } from "../lib/types";

/** Everything Jarvis needs, in three buttons: the voice key, the research helper, the ChatGPT sign-in. */
export default function SetupPanel({
  settings,
  onSaved,
  onClose,
  onAdvanced,
}: {
  settings: Settings;
  onSaved: () => void;
  onClose: () => void;
  onAdvanced: () => void;
}) {
  const [key, setKey] = useState(settings.geminiApiKey);
  const [keyMsg, setKeyMsg] = useState("");
  const [codex, setCodex] = useState<CodexStatus | null>(null);
  const [busy, setBusy] = useState<"" | "install" | "login">("");
  const [line, setLine] = useState("");
  const [link, setLink] = useState("");

  const check = () => invoke<CodexStatus>("codex_status").then(setCodex);
  useEffect(() => {
    check();
    const a = listen<string>("codex-install", (e) => setLine(e.payload));
    // The sign-in prints its link; show it in case the browser didn't open.
    const b = listen<string>("codex-login", (e) => {
      const url = e.payload.match(/https?:\/\/\S+/)?.[0];
      if (url) setLink(url);
    });
    return () => {
      a.then((f) => f());
      b.then((f) => f());
    };
  }, []);

  const saveKey = async () => {
    try {
      await invoke("save_settings", { settings: { ...settings, geminiApiKey: key.trim() } });
      setKeyMsg(key.trim() ? "Saved." : "");
      onSaved();
    } catch (e) {
      setKeyMsg(`Couldn't save: ${e}`);
    }
  };
  const install = async () => {
    setBusy("install");
    setLine("Starting…");
    setCodex(await invoke<CodexStatus>("install_codex"));
    setBusy("");
    setLine("");
  };
  const login = async () => {
    setBusy("login");
    setLink("");
    setCodex(await invoke<CodexStatus>("codex_login"));
    setBusy("");
  };

  const keyDone = !!settings.geminiApiKey;
  const installed = !!codex?.found;
  const signedIn = !!codex?.loggedIn;
  const allDone = keyDone && installed && signedIn;

  return (
    <div className="overlay" onClick={onClose}>
      <div className="setup-panel glass" onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Set up Jarvis">
        <header>
          <div>
            <h2>{allDone ? "Jarvis is ready" : "Let's get Jarvis ready"}</h2>
            <p className="muted small">Three steps. You only do this once.</p>
          </div>
          <button className="mini" onClick={onClose}>
            {allDone ? "Done" : "Later"}
          </button>
        </header>

        <ol className="setup-steps">
          <li className={keyDone ? "ok" : ""}>
            <span className="n">{keyDone ? "✓" : "1"}</span>
            <div>
              <b>Turn on Jarvis's voice</b>
              <small>
                Jarvis talks through Google Gemini. Get a free key from Google AI Studio, then paste it here.{" "}
                <a href="https://aistudio.google.com/apikey" onClick={(e) => (e.preventDefault(), openUrl("https://aistudio.google.com/apikey"))}>
                  Get a key
                </a>
              </small>
              <div className="setup-key">
                <input id="setup-key" type="password" value={key} placeholder="Paste your Gemini key" onChange={(e) => setKey(e.target.value)} />
                <button className="mini" onClick={saveKey} disabled={key.trim() === settings.geminiApiKey}>
                  Save
                </button>
              </div>
              {keyMsg && <small>{keyMsg}</small>}
            </div>
          </li>
          <li className={installed ? "ok" : ""}>
            <span className="n">{installed ? "✓" : "2"}</span>
            <div>
              <b>Install the research helper</b>
              <small>{busy === "install" ? line || "Installing…" : installed ? "Installed." : "Jarvis uses it to read the web and write reports. About a minute."}</small>
            </div>
            {!installed && (
              <button className="btn primary" onClick={install} disabled={!!busy || codex === null}>
                {busy === "install" ? "Installing…" : "Install"}
              </button>
            )}
          </li>
          <li className={signedIn ? "ok" : ""}>
            <span className="n">{signedIn ? "✓" : "3"}</span>
            <div>
              <b>Connect your ChatGPT account</b>
              <small>
                {signedIn
                  ? "Connected."
                  : busy === "login"
                    ? "Finish signing in in your browser, then come back here."
                    : "Research runs on your ChatGPT plan. A sign-in page opens in your browser."}
              </small>
              {busy === "login" && link && (
                <small>
                  Browser didn't open?{" "}
                  <a href={link} onClick={(e) => (e.preventDefault(), openUrl(link))}>
                    Open the sign-in page
                  </a>
                </small>
              )}
              {!signedIn && installed && codex?.found && codex.message && !codex.message.startsWith("Not signed in") && busy !== "login" && (
                <small className="warn">{codex.message}</small>
              )}
            </div>
            {!signedIn && (
              <button className="btn" onClick={login} disabled={!installed || !!busy}>
                {busy === "login" ? "Waiting…" : "Connect"}
              </button>
            )}
          </li>
        </ol>

        <footer>
          <button className="linkish" onClick={onAdvanced}>
            More settings
          </button>
          {allDone ? (
            <button className="btn primary" onClick={onClose}>
              Start using Jarvis
            </button>
          ) : (
            <button className="mini" onClick={check} disabled={!!busy}>
              Check again
            </button>
          )}
        </footer>
      </div>
    </div>
  );
}
