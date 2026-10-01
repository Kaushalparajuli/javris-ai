import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useRef, useState } from "react";
import GateGoogle, { useGoogle } from "./GateGoogle";

interface MailSummary {
  id: string;
  threadId: string;
  from: string;
  subject: string;
  date: string;
  snippet: string;
  unread: boolean;
}
interface MailMessage {
  id: string;
  from: string;
  to: string;
  subject: string;
  date: string;
  body: string;
}

const FOLDERS = [
  { label: "Inbox", q: "in:inbox" },
  { label: "Unread", q: "is:unread in:inbox" },
  { label: "Starred", q: "is:starred" },
  { label: "Sent", q: "in:sent" },
  { label: "Drafts", q: "in:drafts" },
];

/** "Sita Rai <sita@x.com>" → "Sita Rai"; a bare address stays as it is. */
export function senderName(from: string) {
  const m = from.match(/^\s*"?([^"<]+?)"?\s*<.+>\s*$/);
  return (m ? m[1] : from).trim() || from;
}

function when(date: string) {
  const d = new Date(date);
  if (isNaN(d.getTime())) return "";
  const same = d.toDateString() === new Date().toDateString();
  return same ? d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }) : d.toLocaleDateString([], { day: "numeric", month: "short" });
}

/** Your Gmail, read-only: a list, a reading pane, search, and shortcuts to ask Jarvis about a message. */
export default function MailPage({ onAsk, onOpenSettings, onClose }: { onAsk: (text: string) => void; onOpenSettings: () => void; onClose: () => void }) {
  const google = useGoogle("mail");
  const [folder, setFolder] = useState(0);
  const [query, setQuery] = useState("");
  const [typed, setTyped] = useState("");
  const [list, setList] = useState<MailSummary[] | null>(null);
  const [open, setOpen] = useState<MailSummary | null>(null);
  const [message, setMessage] = useState<MailMessage | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const seq = useRef(0);

  const q = query.trim() || FOLDERS[folder].q;

  const load = useCallback(() => {
    const mine = ++seq.current;
    setLoading(true);
    invoke<MailSummary[]>("mail_search", { query: q, max: 40 })
      .then((l) => {
        if (mine !== seq.current) return;
        setList(l);
        setError("");
      })
      .catch((e) => mine === seq.current && setError(String(e)))
      .finally(() => mine === seq.current && setLoading(false));
  }, [q]);

  const connected = !!google.state?.connected;
  useEffect(() => {
    if (!connected) return;
    load();
    const t = setInterval(load, 120_000);
    return () => clearInterval(t);
  }, [connected, load]);

  const pick = (m: MailSummary) => {
    setOpen(m);
    setMessage(null);
    invoke<MailMessage>("mail_read", { id: m.id })
      .then((full) => setMessage(full))
      .catch((e) => setError(String(e)));
  };

  const unread = (list ?? []).filter((m) => m.unread).length;

  return (
    <section className="libpage datapage">
      <header>
        <button className="icon-btn back" onClick={onClose} aria-label="Back to the conversation" title="Back">
          <svg viewBox="0 0 24 24">
            <path d="M15.4 4.6 13.9 3.2 6.1 11l7.8 7.8 1.5-1.4L9 11z" />
          </svg>
        </button>
        <div>
          <h2>Mail</h2>
          <p className="muted small">{google.state?.email || "Gmail"}{connected && unread > 0 ? ` · ${unread} unread` : ""}</p>
        </div>
        {connected && (
          <>
            <form
              className="find"
              onSubmit={(e) => {
                e.preventDefault();
                setQuery(typed);
              }}
            >
              <input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="Search mail (from:sita invoice)…" aria-label="Search mail" />
            </form>
            <button className="mini" onClick={load} disabled={loading}>
              {loading ? "Loading…" : "Refresh"}
            </button>
          </>
        )}
      </header>

      {!connected ? (
        <GateGoogle what="mail" google={google} onOpenSettings={onOpenSettings} />
      ) : (
        <>
          <div className="chips" role="tablist">
            {FOLDERS.map((f, i) => (
              <button
                key={f.label}
                role="tab"
                aria-selected={!query && folder === i}
                className={`chipbtn ${!query && folder === i ? "on" : ""}`}
                onClick={() => {
                  setFolder(i);
                  setQuery("");
                  setTyped("");
                  setOpen(null);
                }}
              >
                {f.label}
              </button>
            ))}
            {query && (
              <button
                className="chipbtn on"
                onClick={() => {
                  setQuery("");
                  setTyped("");
                }}
              >
                Search: {query} ×
              </button>
            )}
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
          <div className="split">
            <ul className="mail-list" aria-label="Messages">
              {list === null && <li className="muted pad">Loading…</li>}
              {list?.length === 0 && <li className="muted pad">Nothing here.</li>}
              {list?.map((m) => (
                <li key={m.id}>
                  <button className={`mail-row ${open?.id === m.id ? "on" : ""} ${m.unread ? "unread" : ""}`} onClick={() => pick(m)}>
                    <span className="dot" aria-label={m.unread ? "Unread" : undefined} />
                    <b>{senderName(m.from)}</b>
                    <time>{when(m.date)}</time>
                    <span className="subj">{m.subject || "(no subject)"}</span>
                    <span className="snip">{m.snippet}</span>
                  </button>
                </li>
              ))}
            </ul>
            <article className="mail-read">
              {!open ? (
                <p className="muted pad">Pick a message to read it here.</p>
              ) : (
                <>
                  <h3>{open.subject || "(no subject)"}</h3>
                  <p className="muted small">
                    From {message?.from ?? open.from}
                    {message?.to ? ` · To ${message.to}` : ""}
                    <br />
                    {new Date(open.date).toString() !== "Invalid Date" ? new Date(open.date).toLocaleString([], { dateStyle: "medium", timeStyle: "short" }) : open.date}
                  </p>
                  <div className="actions">
                    <button className="mini" onClick={() => onAsk(`Summarize the email from ${senderName(open.from)} with the subject "${open.subject}" (email id ${open.id}).`)}>
                      Ask Jarvis to summarize
                    </button>
                    <button className="mini" onClick={() => onAsk(`Draft a reply to the email from ${senderName(open.from)} with the subject "${open.subject}" (email id ${open.id}). Read it first, then write a short, polite reply.`)}>
                      Draft a reply
                    </button>
                    <button className="mini" onClick={() => openUrl(`https://mail.google.com/mail/u/0/#all/${open.threadId}`).catch(() => {})}>
                      Open in Gmail
                    </button>
                  </div>
                  <pre className="mail-body">{message ? message.body || "(no text in this message)" : "Loading…"}</pre>
                </>
              )}
            </article>
          </div>
        </>
      )}
    </section>
  );
}
