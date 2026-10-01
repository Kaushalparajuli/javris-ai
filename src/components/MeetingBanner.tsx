import { useEffect, useState } from "react";

/** How long a meeting has been going, as m:ss or h:mm:ss. */
export function clockTime(ms: number): string {
  const t = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const sec = String(t % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}

/** The always-visible sign that Jarvis is recording, with the way to stop it. */
export default function MeetingBanner({ title, startedAt, onStop }: { title: string; startedAt: number; onStop: () => void }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);
  return (
    <div className="meeting-banner" role="status" aria-live="polite">
      <i className="rec-dot" aria-hidden="true" />
      <span>
        Recording <b>{title}</b> · {clockTime(now - startedAt)}
      </span>
      <button className="btn" onClick={onStop}>
        Stop and write up
      </button>
    </div>
  );
}
