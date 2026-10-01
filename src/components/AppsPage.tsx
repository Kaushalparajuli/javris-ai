import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";

interface Service {
  id: string;
  label: string;
  connected: boolean;
  email: string;
}
interface GoogleStatus {
  configured: boolean;
  services: Service[];
}

type Page = "mail" | "calendar" | "drive" | "youtube" | "memory" | "day" | "briefings" | "routines" | "knowhow" | "library";

const tile = (bg: string, glyph: ReactNode) => (
  <span className="app-icon" style={{ background: bg }} aria-hidden="true">
    <svg viewBox="0 0 24 24">{glyph}</svg>
  </span>
);

/** Apps you can connect (they sign in with Google), each with the page it opens. */
const CONNECT: { id: string; name: string; blurb: string; page: Page; icon: ReactNode }[] = [
  { id: "mail", name: "Gmail", blurb: "Read, search and draft email. Nothing is sent without your OK.", page: "mail", icon: tile("#fff", <path fill="#ea4335" d="M3 6.5 12 13l9-6.5V6a1 1 0 0 0-1-1H4a1 1 0 0 0-1 1zM3 8.9V18a1 1 0 0 0 1 1h2.5v-8.2zm18 0-3.5 1.9V19H20a1 1 0 0 0 1-1z" />) },
  { id: "calendar", name: "Google Calendar", blurb: "See your day, find free time, create and change events.", page: "calendar", icon: tile("#fff", <path fill="#4285f4" d="M7 2h2v2h6V2h2v2h3a1 1 0 0 1 1 1v15a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h3zM5 9v10h14V9zm2 2h4v4H7z" />) },
  { id: "drive", name: "Google Drive", blurb: "Find files, read Docs and Sheets, and save finished work.", page: "drive", icon: tile("#fff", <path fill="#34a853" d="M8.2 3h7.6l6.2 10.7-3.8 6.3H5.8L2 13.7z" />) },
  { id: "youtube", name: "YouTube", blurb: "Search, browse your subscriptions and play videos in Jarvis.", page: "youtube", icon: tile("#ff0033", <path fill="#fff" d="M21.6 7.2a2.5 2.5 0 0 0-1.8-1.8C18.2 5 12 5 12 5s-6.2 0-7.8.4A2.5 2.5 0 0 0 2.4 7.2C2 8.8 2 12 2 12s0 3.2.4 4.8a2.5 2.5 0 0 0 1.8 1.8C5.8 19 12 19 12 19s6.2 0 7.8-.4a2.5 2.5 0 0 0 1.8-1.8c.4-1.6.4-4.8.4-4.8s0-3.2-.4-4.8zM10 15V9l5.2 3z" />) },
];

/** Features that come with Jarvis, so there's nothing to connect. */
const BUILT_IN: { name: string; blurb: string; page?: Page; icon: ReactNode }[] = [
  { name: "Today", blurb: "Your schedule, mail that needs you and what Jarvis is working on.", page: "day", icon: tile("#e3b964", <path fill="#1b1405" d="M12 7a5 5 0 1 1 0 10 5 5 0 0 1 0-10zM11 1h2v3h-2zm0 19h2v3h-2zM1 11h3v2H1zm19 0h3v2h-3z" />) },
  { name: "Memory", blurb: "What Jarvis remembers about you and your projects.", page: "memory", icon: tile("#c8a0ff", <path fill="#1d1030" d="M12 2a7 7 0 0 0-4 12.7V18h8v-3.3A7 7 0 0 0 12 2zm-3 18h6v2H9z" />) },
  { name: "Briefings", blurb: "Research reports delivered on a schedule.", page: "briefings", icon: tile("#7fdcff", <path fill="#06202b" d="M12 3a9 9 0 1 1 0 18 9 9 0 0 1 0-18zm1 4h-2v5.4l3.8 2.3 1-1.7-2.8-1.7z" />) },
  { name: "Routines", blurb: "Multi-step jobs Jarvis can repeat for you.", page: "routines", icon: tile("#78dca0", <path fill="#06241a" d="M12 4a8 8 0 0 1 7.5 5.2L21 8v5h-5l1.8-1.8A5.5 5.5 0 0 0 12 6.5 5.5 5.5 0 0 0 6.5 12H4a8 8 0 0 1 8-8zm0 16a8 8 0 0 1-7.5-5.2L3 16v-5h5l-1.8 1.8A5.5 5.5 0 0 0 12 17.5c2 0 3.7-1.1 4.7-2.7l2.1 1.2A8 8 0 0 1 12 20z" />) },
  { name: "Know-how", blurb: "Skills Jarvis learned from how you do things.", page: "knowhow", icon: tile("#ffaa78", <path fill="#2b1306" d="m12 2 2.9 6.3 6.9.7-5.2 4.6 1.5 6.8L12 17l-6.1 3.4 1.5-6.8L2.2 9l6.9-.7z" />) },
  { name: "Research and code worker", blurb: "Codex does the deep research, writes documents and fixes code.", icon: tile("#10a37f", <path fill="#fff" d="M9 4 3 12l6 8 1.6-1.2L5.6 12l5-6.8zm6 0-1.6 1.2L18.4 12l-5 6.8L15 20l6-8z" />) },
  { name: "Browser agent", blurb: "Opens websites, fills forms and reads pages for you, on request.", icon: tile("#4285f4", <path fill="#fff" d="M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20zm-1 2.1V8H8.6A8 8 0 0 1 11 4.1zM4.3 13h3.2c.1 1.4.4 2.7 1 3.8A8 8 0 0 1 4.3 13zm3.2-2H4.3a8 8 0 0 1 4.2-5.8c-.6 1.1-.9 2.4-1 5.8z" />) },
  { name: "Mac control", blurb: "Clicks and types in other apps, only when you allow it.", icon: tile("#9aa4b5", <path fill="#10131a" d="M5 3h14a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2h-5v2h3v2H7v-2h3v-2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z" />) },
  { name: "Meeting notes", blurb: "Records a meeting and writes the summary, decisions and actions.", icon: tile("#ff7a7a", <path fill="#2b0808" d="M12 3a3 3 0 0 1 3 3v6a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3zm-5 8h2a3 3 0 0 0 6 0h2a5 5 0 0 1-4 4.9V19h-2v-3.1A5 5 0 0 1 7 11z" />) },
];

const SOON: { name: string; blurb: string; icon: ReactNode }[] = [
  { name: "GitHub", blurb: "Triage pull requests, issues and CI, and hand fixes to the code worker.", icon: tile("#161b22", <path fill="#fff" d="M12 2a10 10 0 0 0-3.2 19.5c.5.1.7-.2.7-.5v-1.8c-2.8.6-3.4-1.2-3.4-1.2-.5-1.2-1.1-1.5-1.1-1.5-.9-.6.1-.6.1-.6 1 .1 1.5 1 1.5 1 .9 1.5 2.3 1.1 2.9.8.1-.7.3-1.1.6-1.3-2.2-.3-4.6-1.1-4.6-5 0-1.1.4-2 1-2.7-.1-.3-.4-1.3.1-2.7 0 0 .8-.3 2.8 1a9.600 9.600 0 0 1 5 0c1.900-1.300 2.800-1 2.800-1 .5 1.400.2 2.400.1 2.700.6.700 1 1.600 1 2.700 0 3.900-2.400 4.700-4.600 5 .4.3.7.9.7 1.900v2.800c0 .3.2.6.7.5A10 10 0 0 0 12 2z" />) },
];

function Row({ icon, name, blurb, right, onClick }: { icon: ReactNode; name: string; blurb: string; right?: ReactNode; onClick?: () => void }) {
  return (
    <li className={`app-row${onClick ? " click" : ""}`}>
      <button className="app-main" onClick={onClick} disabled={!onClick} tabIndex={onClick ? 0 : -1}>
        {icon}
        <span className="app-text">
          <b>{name}</b>
          <span>{blurb}</span>
        </span>
      </button>
      <span className="app-right">{right}</span>
    </li>
  );
}

/** A directory of what Jarvis can work with: connect an app, open it, or disconnect it. */
export default function AppsPage({ onOpen, onOpenSettings, onClose }: { onOpen: (page: Page) => void; onOpenSettings: () => void; onClose: () => void }) {
  const [status, setStatus] = useState<GoogleStatus | null>(null);
  const [q, setQ] = useState("");
  const [busy, setBusy] = useState("");
  const [menu, setMenu] = useState("");
  const [error, setError] = useState("");
  const box = useRef<HTMLDivElement>(null);

  const load = useCallback(() => invoke<GoogleStatus>("google_status").then(setStatus).catch(() => setStatus({ configured: false, services: [] })), []);
  useEffect(() => {
    load();
  }, [load]);
  useEffect(() => {
    if (!menu) return;
    const away = (e: MouseEvent) => !(e.target as HTMLElement).closest(".app-menu, .app-more") && setMenu("");
    window.addEventListener("mousedown", away);
    return () => window.removeEventListener("mousedown", away);
  }, [menu]);

  const svc = (id: string) => status?.services.find((s) => s.id === id);
  const connect = async (id: string) => {
    setBusy(id);
    setError("");
    try {
      await invoke("google_connect", { service: id });
      await load();
    } catch (e) {
      setError(String(e));
    }
    setBusy("");
  };
  const disconnect = async (id: string) => {
    setMenu("");
    await invoke("google_disconnect", { service: id }).catch((e) => setError(String(e)));
    load();
  };

  const match = (name: string, blurb: string) => !q.trim() || `${name} ${blurb}`.toLowerCase().includes(q.trim().toLowerCase());
  const connectable = CONNECT.filter((a) => match(a.name, a.blurb));
  const builtIn = BUILT_IN.filter((a) => match(a.name, a.blurb));
  const soon = SOON.filter((a) => match(a.name, a.blurb));
  const installed = CONNECT.filter((a) => svc(a.id)?.connected);

  return (
    <section className="libpage knowpage apps">
      <header className="lib-head">
        <button className="icon-btn back" onClick={onClose} aria-label="Back" title="Back">
          ←
        </button>
        <div>
          <h2>Apps</h2>
          <p className="muted small">Connect apps to let Jarvis work across your tools.</p>
        </div>
      </header>
      <div className="rt-body proj-body apps-body" ref={box}>
        <div className="apps-search">
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M10.5 4a6.5 6.5 0 0 1 5.2 10.4l4.4 4.4-1.4 1.4-4.4-4.4A6.5 6.5 0 1 1 10.5 4zm0 2a4.5 4.5 0 1 0 0 9 4.5 4.5 0 0 0 0-9z" /></svg>
          <input placeholder="Search apps" value={q} onChange={(e) => setQ(e.target.value)} aria-label="Search apps" />
        </div>

        {error && (
          <div className="banner" role="alert">
            <span>{error}</span>
            <button className="mini" onClick={() => setError("")}>Dismiss</button>
          </div>
        )}
        {status && !status.configured && (
          <div className="banner" role="status">
            <span>Google sign-in isn't set up in this build, so these apps can't be connected yet (see .env).</span>
            <button className="mini" onClick={onOpenSettings}>Settings</button>
          </div>
        )}

        {!q && installed.length > 0 && (
          <div className="apps-sec">
            <h3>Installed</h3>
            <div className="apps-installed">
              {installed.map((a) => (
                <button key={a.id} className="apps-chip" onClick={() => onOpen(a.page)} title={`Open ${a.name}`}>
                  {a.icon}
                  <span>{a.name}</span>
                </button>
              ))}
            </div>
          </div>
        )}

        {connectable.length > 0 && (
          <div className="apps-sec">
            <h3>Connect your accounts</h3>
            <ul className="apps-list">
              {connectable.map((a) => {
                const s = svc(a.id);
                const on = !!s?.connected;
                return (
                  <Row
                    key={a.id}
                    icon={a.icon}
                    name={a.name}
                    blurb={busy === a.id ? "Finish signing in in your browser…" : on ? `Connected${s?.email ? ` as ${s.email}` : ""}` : a.blurb}
                    onClick={on ? () => onOpen(a.page) : undefined}
                    right={
                      on ? (
                        <div className="app-more-wrap">
                          <button className="app-round app-more" onClick={() => setMenu(menu === a.id ? "" : a.id)} aria-label={`${a.name} options`} title="Options">
                            ⋯
                          </button>
                          {menu === a.id && (
                            <div className="app-menu" role="menu">
                              <button role="menuitem" onClick={() => { setMenu(""); onOpen(a.page); }}>Open</button>
                              <button role="menuitem" className="danger" onClick={() => disconnect(a.id)}>Disconnect</button>
                            </div>
                          )}
                        </div>
                      ) : (
                        <button className="app-round" onClick={() => connect(a.id)} disabled={!status?.configured || !!busy} aria-label={`Connect ${a.name}`} title={status?.configured ? "Connect" : "Google sign-in isn't set up"}>
                          {busy === a.id ? "…" : "+"}
                        </button>
                      )
                    }
                  />
                );
              })}
            </ul>
          </div>
        )}

        {builtIn.length > 0 && (
          <div className="apps-sec">
            <h3>Built into Jarvis</h3>
            <ul className="apps-list">
              {builtIn.map((a) => (
                <Row key={a.name} icon={a.icon} name={a.name} blurb={a.blurb} onClick={a.page ? () => onOpen(a.page!) : undefined} right={a.page ? <span className="app-open">Open</span> : <span className="app-badge">Built in</span>} />
              ))}
            </ul>
          </div>
        )}

        {soon.length > 0 && (
          <div className="apps-sec">
            <h3>Coming soon</h3>
            <ul className="apps-list">
              {soon.map((a) => (
                <Row key={a.name} icon={a.icon} name={a.name} blurb={a.blurb} right={<span className="app-badge">Soon</span>} />
              ))}
            </ul>
          </div>
        )}

        {!connectable.length && !builtIn.length && !soon.length && <p className="muted">No app matches “{q}”.</p>}
      </div>
    </section>
  );
}
