import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useMemo, useRef, useState } from "react";
import { highlight, languageOf } from "../lib/highlight";

interface Info {
  baseUrl: string;
  url: string;
  version: number;
  hasPage: boolean;
}
interface PFile {
  path: string;
  size: number;
  modified: number;
}

const SIZES = [
  { id: "desktop", label: "Desktop", width: 0 },
  { id: "tablet", label: "Tablet", width: 768 },
  { id: "phone", label: "Phone", width: 390 },
] as const;

const IMAGE = /\.(png|jpe?g|gif|webp|avif|svg|ico)$/i;

const kb = (n: number) => (n < 1024 ? `${n} B` : `${Math.round(n / 1024)} KB`);

/**
 * The site being built, live: a Preview tab (the running site, in a frame) and a Code tab (the files
 * the worker has written, with syntax colours). Both follow the worker as it saves.
 */
export default function SitePreview({ folder, running }: { folder: string; running: boolean }) {
  const [tab, setTab] = useState<"preview" | "code">("preview");
  const [info, setInfo] = useState<Info | null>(null);
  const [shown, setShown] = useState(0);
  const [size, setSize] = useState<(typeof SIZES)[number]["id"]>("desktop");
  const [error, setError] = useState("");
  const [files, setFiles] = useState<PFile[]>([]);
  const [file, setFile] = useState("");
  const [follow, setFollow] = useState(true);
  const [text, setText] = useState<string | null>(null);
  const [readError, setReadError] = useState("");
  const [copied, setCopied] = useState(false);
  const last = useRef(0);
  const frame = useRef<HTMLIFrameElement>(null);
  const fileRef = useRef("");
  const followRef = useRef(true);
  const tabRef = useRef(tab);
  fileRef.current = file;
  followRef.current = follow;
  tabRef.current = tab;

  useEffect(() => {
    let live = true;
    let timer = 0;
    const tick = async () => {
      try {
        const i = await invoke<Info>("preview_info", { folder });
        if (!live) return;
        setError("");
        setInfo(i);
        // Reload once the files have stopped changing for a poll, so a page isn't torn down mid-write.
        if (i.version === last.current) setShown((v) => (v === i.version ? v : i.version));
        const changed = i.version !== last.current;
        last.current = i.version;
        if (tabRef.current === "code" || changed) {
          const list = await invoke<PFile[]>("preview_files", { folder }).catch(() => [] as PFile[]);
          if (!live) return;
          setFiles(list);
          // Follow the worker: show whichever file was saved last, until the user picks one.
          if (followRef.current && list.length) {
            const newest = [...list].sort((a, b) => b.modified - a.modified)[0].path;
            if (newest !== fileRef.current) setFile(newest);
          } else if (!fileRef.current && list.length) {
            setFile(list.find((f) => f.path === "index.html")?.path ?? list[0].path);
          }
        }
      } catch (e) {
        if (live) setError(String(e));
      }
      if (live) timer = window.setTimeout(tick, running ? 1000 : 4000);
    };
    tick();
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [folder, running]);

  // Read the open file again whenever the files change.
  useEffect(() => {
    if (tab !== "code" || !file || IMAGE.test(file) && !file.endsWith(".svg")) {
      setText(null);
      setReadError("");
      return;
    }
    let live = true;
    invoke<string>("preview_read", { folder, path: file })
      .then((t) => live && (setText(t), setReadError("")))
      .catch((e) => live && (setText(null), setReadError(String(e))));
    return () => {
      live = false;
    };
  }, [tab, file, folder, info?.version]);

  const lines = useMemo(() => (text == null ? [] : highlight(text, file)), [text, file]);
  const width = SIZES.find((s) => s.id === size)!.width;
  const current = files.find((f) => f.path === file);
  const now = Date.now();

  return (
    <div className="site-preview">
      <div className="sp-bar">
        <div className="seg" role="tablist" aria-label="Preview or code">
          <button role="tab" aria-selected={tab === "preview"} className={tab === "preview" ? "on" : ""} onClick={() => setTab("preview")}>
            Preview
          </button>
          <button role="tab" aria-selected={tab === "code"} className={tab === "code" ? "on" : ""} onClick={() => setTab("code")}>
            Code{files.length ? ` · ${files.length}` : ""}
          </button>
        </div>
        <span className="sp-title">
          <i className={running ? "live" : ""} />
          {running ? "Updating live" : ""}
        </span>
        <span className="grow" />
        {tab === "preview" ? (
          <>
            <div className="seg" role="group" aria-label="Screen size">
              {SIZES.map((s) => (
                <button key={s.id} className={size === s.id ? "on" : ""} onClick={() => setSize(s.id)}>
                  {s.label}
                </button>
              ))}
            </div>
            <button className="mini" onClick={() => frame.current && info?.url && (frame.current.src = `${info.url}?r=${Date.now()}`)} disabled={!info?.hasPage}>
              Reload
            </button>
            <button className="mini" onClick={() => info?.url && openUrl(info.url).catch(() => {})} disabled={!info?.hasPage}>
              Open in browser
            </button>
          </>
        ) : (
          <>
            <label className="sp-follow" title="Show the file the worker saved last">
              <input type="checkbox" checked={follow} onChange={(e) => setFollow(e.target.checked)} />
              Follow the worker
            </label>
            <button
              className="mini"
              disabled={text == null}
              onClick={() => {
                navigator.clipboard?.writeText(text ?? "").then(() => {
                  setCopied(true);
                  setTimeout(() => setCopied(false), 1200);
                });
              }}
            >
              {copied ? "Copied" : "Copy"}
            </button>
          </>
        )}
      </div>

      {tab === "preview" ? (
        <div className="sp-stage">
          {error ? (
            <p className="muted sp-note">{running ? "Waiting for the worker to create the project folder…" : error}</p>
          ) : !info?.hasPage ? (
            <p className="muted sp-note">{running ? "The first page appears here as soon as the worker saves it…" : "No web page was created in this folder."}</p>
          ) : (
            <iframe ref={frame} key={shown} src={info.url} title="Website preview" className="sp-frame" style={width ? { width, maxWidth: "100%" } : undefined} />
          )}
        </div>
      ) : (
        <div className="sp-code">
          <ul className="sp-files" aria-label="Files">
            {files.length === 0 && <li className="muted sp-note">{running ? "No files yet…" : "No files."}</li>}
            {files.map((f) => {
              const dir = f.path.includes("/") ? f.path.slice(0, f.path.lastIndexOf("/") + 1) : "";
              const name = f.path.slice(dir.length);
              const fresh = running && now - f.modified < 4000;
              return (
                <li key={f.path}>
                  <button
                    className={f.path === file ? "on" : ""}
                    onClick={() => {
                      setFile(f.path);
                      setFollow(false);
                    }}
                    title={f.path}
                  >
                    {fresh && <i className="dot" aria-label="Being written" />}
                    <span className="fname">
                      {dir && <span className="fdir">{dir}</span>}
                      {name}
                    </span>
                    <small>{kb(f.size)}</small>
                  </button>
                </li>
              );
            })}
          </ul>
          <div className="sp-viewer">
            {file && (
              <div className="sp-vhead">
                <b>{file}</b>
                <span className="muted small">
                  {languageOf(file) === "text" ? "text" : languageOf(file)}
                  {text != null ? ` · ${lines.length} lines` : ""}
                  {current ? ` · ${kb(current.size)}` : ""}
                </span>
              </div>
            )}
            {!file ? (
              <p className="muted sp-note">{running ? "Files appear here as the worker saves them." : "Pick a file."}</p>
            ) : IMAGE.test(file) && !file.endsWith(".svg") && info ? (
              <div className="sp-image">
                <img src={`${info.baseUrl}${file.split("/").map(encodeURIComponent).join("/")}?v=${info.version}`} alt={file} />
              </div>
            ) : readError ? (
              <p className="muted sp-note">{readError}</p>
            ) : (
              <pre className="sp-pre" tabIndex={0}>
                <code>
                  {lines.map((l, i) => (
                    <span className="ln" key={i}>
                      <span className="no">{i + 1}</span>
                      <span className="tx">
                        {l.length === 0 ? "​" : l.map((t, j) => <span key={j} className={t.cls ? `tk-${t.cls}` : undefined}>{t.text}</span>)}
                      </span>
                    </span>
                  ))}
                </code>
              </pre>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
