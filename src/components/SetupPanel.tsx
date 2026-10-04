import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import type { CodexStatus, HeygenStatus, Settings, VideoStatus } from "../lib/types";

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
  // Making videos is optional: its own tools, and HeyGen's free catalog of music, photos, icons and voices.
  const [video, setVideo] = useState<VideoStatus | null>(null);
  const [heygen, setHeygen] = useState<HeygenStatus | null>(null);
  const [vbusy, setVbusy] = useState<"" | "video" | "heygen" | "captions">("");
  const [vline, setVline] = useState("");
  const [vpct, setVpct] = useState<number | null>(null);
  const [verror, setVerror] = useState("");
  const [hlink, setHlink] = useState("");

  const check = () => {
    invoke<VideoStatus>("video_status").then(setVideo).catch(() => {});
    invoke<HeygenStatus>("heygen_status").then(setHeygen).catch(() => {});
    return invoke<CodexStatus>("codex_status").then(setCodex);
  };
  useEffect(() => {
    check();
    const a = listen<string>("codex-install", (e) => setLine(e.payload));
    // The sign-in prints its link; show it in case the browser didn't open.
    const b = listen<string>("codex-login", (e) => {
      const url = e.payload.match(/https?:\/\/\S+/)?.[0];
      if (url) setLink(url);
    });
    const c = listen<{ message: string; percent: number | null }>("video-setup", (e) => {
      setVline(e.payload.message);
      setVpct(e.payload.percent);
    });
    const d = listen<string>("heygen-login", (e) => setHlink(e.payload));
    return () => {
      a.then((f) => f());
      b.then((f) => f());
      c.then((f) => f());
      d.then((f) => f());
    };
  }, []);

  const setUpVideo = async () => {
    setVbusy("video");
    setVerror("");
    setVline("Starting…");
    try {
      setVideo(await invoke<VideoStatus>("video_setup"));
    } catch (e) {
      setVerror(String(e));
    }
    setVbusy("");
    setVpct(null);
  };
  const getCaptions = async () => {
    setVbusy("captions");
    setVerror("");
    setVline("Downloading the speech model…");
    try {
      setVideo(await invoke<VideoStatus>("captions_install"));
    } catch (e) {
      setVerror(String(e));
    }
    setVbusy("");
    setVpct(null);
  };
  const connectHeygen = async () => {
    setVbusy("heygen");
    setVerror("");
    setHlink("");
    setVline("Getting HeyGen…");
    try {
      setHeygen(await invoke<HeygenStatus>("heygen_connect"));
    } catch (e) {
      setVerror(String(e));
    }
    setVbusy("");
    setVpct(null);
  };

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

        <div className="setup-body">
        <ol className="setup-steps">
          <li className={keyDone ? "done" : ""}>
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
          <li className={installed ? "done" : ""}>
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
          <li className={signedIn ? "done" : ""}>
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

        <div className="setup-extra">
          <div className="label">Optional · make videos</div>
          <ul className="setup-steps">
            <li className={video?.ready ? "done" : ""}>
              <span className="n">{video?.ready ? "✓" : "+"}</span>
              <div>
                <b>Video tools</b>
                <small>
                  {vbusy === "video" ? vline || "Setting up…" : video?.ready ? "Ready. Jarvis can make videos." : video?.message ?? "Checking…"}
                </small>
                {vbusy === "video" && vpct != null && (
                  <div className="setup-bar" aria-hidden="true">
                    <i style={{ width: `${vpct}%` }} />
                  </div>
                )}
                {verror && vbusy === "" && <small className="warn">{verror}</small>}
              </div>
              {!video?.ready && (
                <button className="btn primary" onClick={setUpVideo} disabled={!!vbusy || video === null}>
                  {vbusy === "video" ? "Setting up…" : "Set up video"}
                </button>
              )}
            </li>
            <li className={video?.captions ? "done" : ""}>
              <span className="n">{video?.captions ? "✓" : "+"}</span>
              <div>
                <b>Exact caption timing</b>
                <small>
                  {vbusy === "captions" ? vline : video?.captions ? "Installed. Captions follow the spoken words exactly." : "Optional. Without it, captions are timed by estimate. About 700 MB, once."}
                </small>
              </div>
              {!video?.captions && (
                <button className="btn" onClick={getCaptions} disabled={!!vbusy || !video?.ready}>
                  {vbusy === "captions" ? "Downloading…" : "Download"}
                </button>
              )}
            </li>
            <li className={heygen?.signedIn ? "done" : ""}>
              <span className="n">{heygen?.signedIn ? "✓" : "+"}</span>
              <div>
                <b>HeyGen catalog (free)</b>
                <small>
                  {vbusy === "heygen" ? "Finish signing in in your browser, then come back here." : heygen?.signedIn ? "Connected. Music, photos, icons and voices are available to videos." : "Music, photos and icons for videos. Optional: videos still get sound effects and Jarvis-made pictures without it."}
                </small>
                {vbusy === "heygen" && hlink && (
                  <small>
                    Browser didn't open?{" "}
                    <a href={hlink} onClick={(e) => (e.preventDefault(), openUrl(hlink))}>
                      Open the sign-in page
                    </a>
                  </small>
                )}
              </div>
              {!heygen?.signedIn && (
                <button className="btn" onClick={connectHeygen} disabled={!!vbusy || !video?.ready}>
                  {vbusy === "heygen" ? "Waiting…" : "Connect"}
                </button>
              )}
            </li>
          </ul>
        </div>
        </div>

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
