import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useRef, useState } from "react";
import GateGoogle, { useGoogle } from "./GateGoogle";

interface DriveFile {
  id: string;
  name: string;
  kind: string;
  modified: string;
  owner: string;
  link: string;
}
interface FileText {
  name: string;
  text: string;
  truncated: boolean;
}

const FILTERS = [
  { id: "all", label: "All" },
  { id: "doc", label: "Docs" },
  { id: "sheet", label: "Sheets" },
  { id: "slides", label: "Slides" },
  { id: "pdf", label: "PDFs" },
];

const KIND_LABEL: Record<string, string> = { doc: "Doc", sheet: "Sheet", slides: "Slides", folder: "Folder", pdf: "PDF", file: "File" };
/** Kinds whose text can be previewed (Drive exports them as text). */
const READABLE = new Set(["doc", "sheet", "slides", "file"]);

function when(iso: string) {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return "";
  return d.toDateString() === new Date().toDateString() ? d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }) : d.toLocaleDateString([], { day: "numeric", month: "short", year: "numeric" });
}

/** Rows of cells from CSV text, with quoted fields and doubled quotes. */
export function parseCsv(text: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let cell = "";
  let quoted = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quoted) {
      if (c === '"' && text[i + 1] === '"') {
        cell += '"';
        i++;
      } else if (c === '"') quoted = false;
      else cell += c;
    } else if (c === '"') quoted = true;
    else if (c === ",") {
      row.push(cell);
      cell = "";
    } else if (c === "\n" || c === "\r") {
      if (c === "\r" && text[i + 1] === "\n") i++;
      row.push(cell);
      rows.push(row);
      row = [];
      cell = "";
    } else cell += c;
  }
  if (cell || row.length) {
    row.push(cell);
    rows.push(row);
  }
  return rows;
}

/** Your Google Drive: recent files, search, and a text preview. */
export default function DrivePage({ onAsk, onOpenSettings, onClose }: { onAsk: (text: string) => void; onOpenSettings: () => void; onClose: () => void }) {
  const google = useGoogle("drive");
  const connected = !!google.state?.connected;
  const [filter, setFilter] = useState("all");
  const [query, setQuery] = useState("");
  const [typed, setTyped] = useState("");
  const [files, setFiles] = useState<DriveFile[] | null>(null);
  const [open, setOpen] = useState<DriveFile | null>(null);
  const [content, setContent] = useState<FileText | null>(null);
  const [reading, setReading] = useState(false);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const seq = useRef(0);

  const load = useCallback(() => {
    const mine = ++seq.current;
    setLoading(true);
    invoke<DriveFile[]>("drive_search", { query, max: 50 })
      .then((l) => {
        if (mine !== seq.current) return;
        setFiles(l);
        setError("");
      })
      .catch((e) => mine === seq.current && setError(String(e)))
      .finally(() => mine === seq.current && setLoading(false));
  }, [query]);

  useEffect(() => {
    if (connected) load();
  }, [connected, load]);

  const pick = (f: DriveFile) => {
    setOpen(f);
    setContent(null);
    if (!READABLE.has(f.kind)) return;
    setReading(true);
    invoke<FileText>("drive_read", { id: f.id })
      .then(setContent)
      .catch((e) => setError(String(e)))
      .finally(() => setReading(false));
  };

  const shown = (files ?? []).filter((f) => filter === "all" || f.kind === filter);

  return (
    <section className="libpage datapage">
      <header>
        <button className="icon-btn back" onClick={onClose} aria-label="Back to the conversation" title="Back">
          <svg viewBox="0 0 24 24">
            <path d="M15.4 4.6 13.9 3.2 6.1 11l7.8 7.8 1.5-1.4L9 11z" />
          </svg>
        </button>
        <div>
          <h2>Drive</h2>
          <p className="muted small">{google.state?.email || "Google Drive"}</p>
        </div>
        {connected && (
          <>
            <form
              className="find"
              onSubmit={(e) => {
                e.preventDefault();
                setQuery(typed.trim());
                setOpen(null);
              }}
            >
              <input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="Search names and contents…" aria-label="Search Drive" />
            </form>
            <button className="mini" onClick={load} disabled={loading}>
              {loading ? "Loading…" : "Refresh"}
            </button>
          </>
        )}
      </header>

      {!connected ? (
        <GateGoogle what="files" app="Google Drive" google={google} onOpenSettings={onOpenSettings} />
      ) : (
        <>
          <div className="chips" role="tablist">
            {FILTERS.map((f) => (
              <button key={f.id} role="tab" aria-selected={filter === f.id} className={`chipbtn ${filter === f.id ? "on" : ""}`} onClick={() => setFilter(f.id)}>
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
            <ul className="mail-list" aria-label="Files">
              {files === null && <li className="muted pad">Loading…</li>}
              {files && shown.length === 0 && <li className="muted pad">No files here.</li>}
              {shown.map((f) => (
                <li key={f.id}>
                  <button className={`mail-row drive-row ${open?.id === f.id ? "on" : ""}`} onClick={() => pick(f)}>
                    <span className={`kindtag k-${f.kind}`}>{KIND_LABEL[f.kind] ?? "File"}</span>
                    <b>{f.name}</b>
                    <time>{when(f.modified)}</time>
                    {f.owner && <span className="snip">{f.owner}</span>}
                  </button>
                </li>
              ))}
            </ul>
            <article className="mail-read">
              {!open ? (
                <p className="muted pad">Pick a file to preview it here.</p>
              ) : (
                <>
                  <h3>{open.name}</h3>
                  <p className="muted small">
                    {KIND_LABEL[open.kind] ?? "File"}
                    {open.owner ? ` · ${open.owner}` : ""}
                    {open.modified ? ` · changed ${when(open.modified)}` : ""}
                  </p>
                  <div className="actions">
                    <button className="mini" onClick={() => openUrl(open.link).catch(() => {})} disabled={!open.link}>
                      Open in Drive
                    </button>
                    {READABLE.has(open.kind) && (
                      <button className="mini" onClick={() => onAsk(`Read my Google Drive file "${open.name}" (file id ${open.id}) and summarize it.`)}>
                        Ask Jarvis to summarize
                      </button>
                    )}
                  </div>
                  {reading && <p className="muted">Reading…</p>}
                  {!READABLE.has(open.kind) && <p className="muted">Jarvis can't show this kind of file here. Use Open in Drive.</p>}
                  {content &&
                    (open.kind === "sheet" ? (
                      <div className="sheet-wrap">
                        <table className="sheet-table">
                          <tbody>
                            {parseCsv(content.text)
                              .slice(0, 200)
                              .map((r, i) => (
                                <tr key={i}>
                                  {r.slice(0, 30).map((c, j) => (i === 0 ? <th key={j}>{c}</th> : <td key={j}>{c}</td>))}
                                </tr>
                              ))}
                          </tbody>
                        </table>
                      </div>
                    ) : (
                      <pre className="mail-body">{content.text}</pre>
                    ))}
                  {content?.truncated && <p className="muted small">Showing the start of the file.</p>}
                </>
              )}
            </article>
          </div>
        </>
      )}
    </section>
  );
}
