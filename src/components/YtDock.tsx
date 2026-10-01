import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useRef, useState } from "react";
import type { YtItem } from "./YouTubePage";

type Box = { top: number; left: number; width: number; height: number };

/**
 * The YouTube player, mounted once for the whole app so a video keeps playing when you leave the
 * YouTube page. On that page it sits over the page's video area (`slot`); anywhere else it shrinks
 * to a mini bar at the bottom. It's the same iframe in both, so playback never restarts.
 */
export default function YtDock({
  playing,
  slot,
  audioOnly,
  onAudioOnly,
  jarvisSpeaking,
  onClose,
  onOpenPage,
}: {
  playing: YtItem | null;
  slot: HTMLElement | null;
  /** Hide the picture and keep just the sound (and the cover art). */
  audioOnly: boolean;
  onAudioOnly: (on: boolean) => void;
  /** True while Jarvis is talking: the music dips so you can hear it. */
  jarvisSpeaking: () => boolean;
  onClose: () => void;
  onOpenPage: () => void;
}) {
  const frame = useRef<HTMLIFrameElement>(null);
  const [box, setBox] = useState<Box | null>(null);
  const [paused, setPaused] = useState(false);
  // Follow the page's video area, wherever scrolling or resizing moves it.
  useEffect(() => {
    if (!slot) return setBox(null);
    let raf = 0;
    const tick = () => {
      const r = slot.getBoundingClientRect();
      setBox((b) => (b && b.top === r.top && b.left === r.left && b.width === r.width && b.height === r.height ? b : { top: r.top, left: r.left, width: r.width, height: r.height }));
      raf = requestAnimationFrame(tick);
    };
    tick();
    return () => cancelAnimationFrame(raf);
  }, [slot, playing?.id]);

  // A new video starts playing.
  useEffect(() => setPaused(false), [playing?.id]);

  // Ducking: fade the music down while Jarvis speaks, and back up after.
  const volume = useRef(100);
  const setVolume = (v: number) => {
    volume.current = v;
    frame.current?.contentWindow?.postMessage(JSON.stringify({ event: "command", func: "setVolume", args: [v] }), "*");
  };
  useEffect(() => {
    if (!playing) return;
    volume.current = 100;
    const t = setInterval(() => {
      const target = jarvisSpeaking() ? 18 : 100;
      if (volume.current === target) return;
      const step = target < volume.current ? -20 : 10;
      setVolume(step < 0 ? Math.max(target, volume.current + step) : Math.min(target, volume.current + step));
    }, 90);
    return () => clearInterval(t);
  }, [playing?.id, jarvisSpeaking]);

  // Hear play/pause from the player itself, so the mini bar's button always matches.
  useEffect(() => {
    const on = (e: MessageEvent) => {
      if (e.source !== frame.current?.contentWindow || typeof e.data !== "string") return;
      try {
        const m = JSON.parse(e.data);
        const state = m.event === "onStateChange" ? m.info : m.event === "infoDelivery" ? m.info?.playerState : undefined;
        if (state === 1) setPaused(false);
        else if (state === 2 || state === 0) setPaused(true);
      } catch {
        /* not a player message */
      }
    };
    window.addEventListener("message", on);
    return () => window.removeEventListener("message", on);
  }, []);

  if (!playing) return null;
  const command = (func: string) => frame.current?.contentWindow?.postMessage(JSON.stringify({ event: "command", func, args: "" }), "*");
  const toggle = () => {
    command(paused ? "playVideo" : "pauseVideo");
    setPaused(!paused);
  };
  const full = !!slot && !!box;

  return (
    <div className={`yt-dock ${full ? "full" : "mini"}`} style={full ? { top: box.top, left: box.left, width: box.width, height: box.height } : undefined}>
      <div className={`yt-dock-video${audioOnly ? " audio" : ""}`}>
        {audioOnly && (
          <div className="yt-cover" style={{ backgroundImage: `url(https://i.ytimg.com/vi/${encodeURIComponent(playing.id)}/mqdefault.jpg)` }}>
            {full && (
              <button className="yt-cover-btn" onClick={toggle} aria-label={paused ? "Play" : "Pause"}>
                <svg viewBox="0 0 24 24">{paused ? <path d="M8 5v14l11-7z" /> : <path d="M7 5h4v14H7zm6 0h4v14h-4z" />}</svg>
                <span>Audio only</span>
              </button>
            )}
          </div>
        )}
        <iframe
          ref={frame}
          key={playing.id}
          src={`https://www.youtube-nocookie.com/embed/${encodeURIComponent(playing.id)}?autoplay=1&rel=0&enablejsapi=1`}
          title={playing.title || "YouTube video"}
          allow="autoplay; encrypted-media; picture-in-picture; fullscreen"
          referrerPolicy="strict-origin-when-cross-origin"
          allowFullScreen
          onLoad={() => {
            frame.current?.contentWindow?.postMessage(JSON.stringify({ event: "listening", id: 1, channel: "widget" }), "*");
            if (volume.current !== 100) setVolume(volume.current);
          }}
        />
      </div>
      {!full && (
        <div className="yt-dock-info">
          <button className="yt-dock-text" onClick={onOpenPage} title="Back to YouTube">
            <b>{playing.title || "Playing from YouTube"}</b>
            <span className="muted small">{playing.channel || "YouTube"}</span>
          </button>
          <button className="icon-btn" onClick={toggle} aria-label={paused ? "Play" : "Pause"} title={paused ? "Play" : "Pause"}>
            <svg viewBox="0 0 24 24">{paused ? <path d="M8 5v14l11-7z" /> : <path d="M7 5h4v14H7zm6 0h4v14h-4z" />}</svg>
          </button>
          <button className={`icon-btn${audioOnly ? " on" : ""}`} onClick={() => onAudioOnly(!audioOnly)} aria-pressed={audioOnly} aria-label="Audio only" title={audioOnly ? "Show video" : "Audio only"}>
            <svg viewBox="0 0 24 24">
              <path d="M12 3a9 9 0 0 0-9 9v6a3 3 0 0 0 3 3h2v-8H5v-1a7 7 0 0 1 14 0v1h-3v8h2a3 3 0 0 0 3-3v-6a9 9 0 0 0-9-9z" />
            </svg>
          </button>
          <button className="icon-btn" onClick={() => openUrl(playing.link).catch(() => {})} aria-label="Open on YouTube" title="Open on YouTube">
            <svg viewBox="0 0 24 24">
              <path d="M14 3h7v7h-2V6.4l-8.3 8.3-1.4-1.4L17.6 5H14zM5 5h6v2H5v12h12v-6h2v8H3V5z" />
            </svg>
          </button>
          <button className="icon-btn" onClick={onClose} aria-label="Stop and close" title="Stop and close">
            <svg viewBox="0 0 24 24">
              <path d="m6.4 5 5.6 5.6L17.6 5 19 6.4 13.4 12l5.6 5.6-1.4 1.4-5.6-5.6L6.4 19 5 17.6l5.6-5.6L5 6.4z" />
            </svg>
          </button>
        </div>
      )}
    </div>
  );
}
