import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useRef, useState } from "react";
import GateGoogle, { useGoogle } from "./GateGoogle";

export interface YtItem {
  kind: string;
  id: string;
  title: string;
  channel: string;
  published: string;
  description: string;
  link: string;
  thumbnail: string;
}

/** What Jarvis (or anything else) asks the YouTube page to show. */
export interface YtIntent {
  /** Changes with every request, so asking for the same thing twice still takes effect. */
  seq: number;
  query?: string;
  items?: YtItem[];
  /** A video id to start playing. */
  play?: string;
}

const TABS = [
  { id: "search", label: "Search" },
  { id: "subscriptions", label: "Subscriptions" },
  { id: "liked", label: "Liked" },
] as const;

function ago(iso: string) {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return "";
  return d.toLocaleDateString([], { day: "numeric", month: "short", year: "numeric" });
}

/** YouTube inside Jarvis: search, browse your subscriptions and likes, and play videos in a panel. */
export default function YouTubePage({ intent, onAsk, onOpenSettings, onClose }: { intent: YtIntent | null; onAsk: (text: string) => void; onOpenSettings: () => void; onClose: () => void }) {
  const google = useGoogle("youtube");
  const connected = !!google.state?.connected;
  const [tab, setTab] = useState<(typeof TABS)[number]["id"]>("search");
  const [typed, setTyped] = useState("");
  const [items, setItems] = useState<YtItem[] | null>(null);
  const [playing, setPlaying] = useState<YtItem | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const seq = useRef(0);

  const run = useCallback((fn: () => Promise<YtItem[]>) => {
    const mine = ++seq.current;
    setLoading(true);
    setError("");
    fn()
      .then((l) => mine === seq.current && setItems(l))
      .catch((e) => mine === seq.current && setError(String(e)))
      .finally(() => mine === seq.current && setLoading(false));
  }, []);

  const search = useCallback(
    (q: string) => {
      if (!q.trim()) return;
      setTab("search");
      setTyped(q);
      run(() => invoke<YtItem[]>("yt_search", { query: q, max: 18, kind: "video" }));
    },
    [run],
  );

  // Subscriptions and likes load when their tab opens.
  useEffect(() => {
    if (!connected || tab === "search") return;
    run(() => invoke<YtItem[]>("yt_mine", { what: tab, max: 24 }));
  }, [connected, tab, run]);

  // Something Jarvis wants shown: results it already has, a search to run, or a video to play.
  const handled = useRef(0);
  useEffect(() => {
    if (!intent || intent.seq === handled.current) return;
    handled.current = intent.seq;
    if (intent.items) {
      setTab("search");
      setTyped(intent.query ?? "");
      setItems(intent.items);
      setError("");
    } else if (intent.query && connected) {
      search(intent.query);
    }
    if (intent.play) {
      const known = intent.items?.find((i) => i.id === intent.play);
      setPlaying(known ?? { kind: "video", id: intent.play, title: "", channel: "", published: "", description: "", link: `https://www.youtube.com/watch?v=${intent.play}`, thumbnail: "" });
    }
  }, [intent, connected, search]);

  const pick = (i: YtItem) => {
    if (i.kind === "video") setPlaying(i);
    else if (i.kind === "channel") {
      setTab("search");
      search(i.title);
    } else openUrl(i.link).catch(() => {});
  };

  return (
    <section className="libpage datapage">
      <header>
        <button className="icon-btn back" onClick={onClose} aria-label="Back to the conversation" title="Back">
          <svg viewBox="0 0 24 24">
            <path d="M15.4 4.6 13.9 3.2 6.1 11l7.8 7.8 1.5-1.4L9 11z" />
          </svg>
        </button>
        <div>
          <h2>YouTube</h2>
          <p className="muted small">{google.state?.email || "Search and watch without leaving Jarvis"}</p>
        </div>
        {connected && (
          <form
            className="find"
            onSubmit={(e) => {
              e.preventDefault();
              search(typed);
            }}
          >
            <input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="Search YouTube…" aria-label="Search YouTube" />
          </form>
        )}
      </header>

      {!connected ? (
        <GateGoogle what="videos" app="YouTube" google={google} onOpenSettings={onOpenSettings} />
      ) : (
        <>
          <div className="chips" role="tablist">
            {TABS.map((t) => (
              <button key={t.id} role="tab" aria-selected={tab === t.id} className={`chipbtn ${tab === t.id ? "on" : ""}`} onClick={() => setTab(t.id)}>
                {t.label}
              </button>
            ))}
          </div>
          {error && (
            <div className="banner" role="alert">
              <span>{error}</span>
              <div className="actions">
                <button className="mini" onClick={() => setError("")}>
                  Dismiss
                </button>
              </div>
            </div>
          )}
          <div className={`yt ${playing ? "playing" : ""}`}>
            {playing && (
              <div className="yt-player">
                <div className="yt-frame">
                  <iframe
                    key={playing.id}
                    src={`https://www.youtube-nocookie.com/embed/${encodeURIComponent(playing.id)}?autoplay=1&rel=0`}
                    title={playing.title || "YouTube video"}
                    allow="autoplay; encrypted-media; picture-in-picture; fullscreen"
                    referrerPolicy="strict-origin-when-cross-origin"
                    allowFullScreen
                  />
                </div>
                <h3>{playing.title || "Playing"}</h3>
                {playing.channel && (
                  <p className="muted small">
                    {playing.channel}
                    {playing.published ? ` · ${ago(playing.published)}` : ""}
                  </p>
                )}
                <div className="actions">
                  <button className="mini" onClick={() => openUrl(playing.link).catch(() => {})}>
                    Open on YouTube
                  </button>
                  <button className="mini" onClick={() => onAsk(`Tell me about this YouTube video: ${playing.link}`)}>
                    Ask Jarvis about it
                  </button>
                  <button className="mini" onClick={() => setPlaying(null)}>
                    Close player
                  </button>
                </div>
                <p className="hint">If the video says it can't be played here, use Open on YouTube.</p>
              </div>
            )}
            <div className="yt-results" aria-busy={loading}>
              {items === null && !loading && tab === "search" && <p className="muted pad">Search for something, or ask Jarvis to find a video.</p>}
              {loading && <p className="muted pad">Loading…</p>}
              {items?.length === 0 && !loading && <p className="muted pad">Nothing found.</p>}
              <ul className="yt-grid">
                {items?.map((i) => (
                  <li key={`${i.kind}-${i.id}`}>
                    <button className={`yt-card ${playing?.id === i.id ? "on" : ""}`} onClick={() => pick(i)}>
                      {i.thumbnail ? <img src={i.thumbnail} alt="" loading="lazy" /> : <span className="yt-noimg" />}
                      <b>{i.title}</b>
                      <span className="muted small">
                        {i.kind !== "video" ? `${i.kind} · ` : ""}
                        {i.channel}
                        {i.published ? ` · ${ago(i.published)}` : ""}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          </div>
        </>
      )}
    </section>
  );
}
