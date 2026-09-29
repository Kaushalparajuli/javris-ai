import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";

interface Frame {
  /** JPEG, base64. */
  data: string;
  width: number;
  height: number;
  url: string;
  title: string;
}

/** Keys passed through to the page as keys; everything printable is sent as text. */
const KEYS = new Set(["Enter", "Backspace", "Tab", "Escape", "Delete", "ArrowLeft", "ArrowUp", "ArrowRight", "ArrowDown", "Home", "End", "PageUp", "PageDown"]);

/**
 * Jarvis's browser, live. Codex drives it during a browser task; the rest of the time you can
 * click, scroll and type in it yourself, for example to sign in or get past a bot check.
 */
export default function BrowserView({ canUse }: { canUse: boolean }) {
  const [frame, setFrame] = useState<Frame | null>(null);
  const [focused, setFocused] = useState(false);
  const [note, setNote] = useState("");
  const screen = useRef<HTMLDivElement>(null);
  const wheel = useRef({ dy: 0, x: 0, y: 0, timer: 0 });

  useEffect(() => {
    invoke<Frame | null>("browser_frame")
      .then((f) => f && setFrame(f))
      .catch(() => {});
    const onFrame = listen<Frame>("browser-frame", (e) => setFrame(e.payload));
    const onClose = listen("browser-closed", () => setFrame(null));
    return () => {
      onFrame.then((f) => f());
      onClose.then((f) => f());
    };
  }, []);

  if (!frame) return null;

  const send = (event: object) => {
    setNote("");
    invoke("browser_input", { event }).catch((e) => setNote(String(e)));
  };
  /** Where the pointer is, as a fraction of the picture. */
  const at = (e: React.MouseEvent) => {
    const r = e.currentTarget.getBoundingClientRect();
    return { x: (e.clientX - r.left) / r.width, y: (e.clientY - r.top) / r.height };
  };
  const host = frame.url.replace(/^https?:\/\//, "").replace(/\/$/, "");

  return (
    <div className="bview">
      <div className="bbar">
        <button className="icon-btn" onClick={() => send({ kind: "back" })} disabled={!canUse} aria-label="Back a page" title="Back">
          <svg viewBox="0 0 24 24">
            <path d="M15.4 4.6 13.9 3.2 6.1 11l7.8 7.8 1.5-1.4L9 11z" />
          </svg>
        </button>
        <span className="live">
          <i />
          Live
        </span>
        <span className="url" title={frame.url}>
          {host || "about:blank"}
        </span>
        <button
          className="mini"
          onClick={() => invoke("close_browser").catch((e) => setNote(String(e)))}
          disabled={!canUse}
          title="Close Jarvis's browser. It also closes itself after 20 idle minutes."
        >
          Close
        </button>
      </div>
      <div
        ref={screen}
        className={`bscreen ${canUse ? "usable" : ""} ${focused ? "focused" : ""}`}
        tabIndex={canUse ? 0 : -1}
        role="application"
        aria-label={`Live view of ${frame.title || host}`}
        onClick={(e) => {
          if (!canUse) return;
          screen.current?.focus();
          send({ kind: "click", ...at(e) });
        }}
        onWheel={(e) => {
          if (!canUse) return;
          // Wheel events come in bursts; send them a few at a time.
          const w = wheel.current;
          Object.assign(w, at(e));
          w.dy += e.deltaY;
          if (!w.timer) {
            w.timer = window.setTimeout(() => {
              send({ kind: "scroll", x: w.x, y: w.y, dy: w.dy });
              w.dy = 0;
              w.timer = 0;
            }, 60);
          }
        }}
        onKeyDown={(e) => {
          if (!canUse || e.metaKey || e.ctrlKey || e.altKey) return;
          if (KEYS.has(e.key)) {
            e.preventDefault();
            send({ kind: "key", key: e.key });
          } else if (e.key.length === 1) {
            e.preventDefault();
            send({ kind: "text", text: e.key });
          }
        }}
        onPaste={(e) => {
          const text = e.clipboardData.getData("text/plain");
          if (canUse && text) {
            e.preventDefault();
            send({ kind: "text", text });
          }
        }}
        onFocus={() => setFocused(true)}
        onBlur={() => setFocused(false)}
      >
        <img src={`data:image/jpeg;base64,${frame.data}`} alt="" draggable={false} />
      </div>
      <p className="bhint">
        {!canUse
          ? "Codex is using the browser. You can use it yourself when it's done."
          : focused
            ? "Typing goes to the page. Click outside it to stop."
            : "Click into the page to use it, for example to sign in or finish a check."}
      </p>
      {note && <p className="error-text bnote">{note}</p>}
    </div>
  );
}
