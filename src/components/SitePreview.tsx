import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { DirectorState } from "../lib/director";
import { safeFileName } from "../lib/exportDocx";
import { highlight, languageOf } from "../lib/highlight";
import { askSavePath, fileNameOf } from "../lib/saveAs";
import { type Picked, pickedForWorker, pickedLabel } from "../lib/sitepick";
import type { Task, VideoInfo, VideoProgress } from "../lib/types";
import DirectorPanel from "./DirectorPanel";
import VideoTab from "./VideoTab";

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
interface Version {
  n: number;
  label: string;
  at: number;
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
export default function SitePreview({
  folder,
  running,
  director,
  onReview,
  chatId,
  onOpenTask,
  videoProgress,
}: {
  folder: string;
  running: boolean;
  director?: DirectorState;
  onReview: () => void;
  /** How far the making of this folder's video has got, if it is a video. */
  videoProgress?: VideoProgress;
  /** The conversation a change made here belongs to. */
  chatId: string;
  /** Show a task that was just started (a change) in the panel. */
  onOpenTask: (id: number) => void;
}) {
  const [tab, setTab] = useState<"video" | "preview" | "code" | "review">("preview");
  const [vinfo, setVinfo] = useState<VideoInfo | null>(null);
  const shownVideo = useRef(false);
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
  const [picking, setPicking] = useState(false);
  const [picked, setPicked] = useState<Picked | null>(null);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [versions, setVersions] = useState<Version[] | null>(null);
  const [note, setNote] = useState("");
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

  // A video project opens on its video; the page itself is only the first frame.
  const folderVersion = info?.version;
  useEffect(() => {
    let live = true;
    invoke<VideoInfo>("video_info", { folder })
      .then((v) => {
        if (!live) return;
        setVinfo(v);
        if (v.isVideo && !shownVideo.current) {
          shownVideo.current = true;
          setTab("video");
        }
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [folder, folderVersion, videoProgress?.stage]);
  const isVideo = !!vinfo?.isVideo;

  // A video's page is a fixed canvas (1920x1080 and so on): it is scaled to fit the panel and shown at
  // the moment picked on the scrubber, since frame 0 of an animated video is usually still empty.
  const stage = useRef<HTMLDivElement>(null);
  const [box, setBox] = useState({ w: 0, h: 0 });
  useEffect(() => {
    const el = stage.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setBox({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  }, [tab, isVideo, info?.hasPage]);
  const seconds = vinfo?.meta.seconds || 0;
  const [at, setAt] = useState<number | null>(null);
  const moment = at ?? Math.min(2, Math.max(0, seconds - 0.1));
  const seekFrame = useCallback(() => {
    if (isVideo) frame.current?.contentWindow?.postMessage({ type: "jv-seek", t: moment }, "*");
  }, [isVideo, moment]);
  useEffect(seekFrame, [seekFrame]);
  const canvasW = vinfo?.meta.width || 1920;
  const canvasH = vinfo?.meta.height || 1080;
  const fit = box.w && box.h ? Math.min((box.w - 24) / canvasW, (box.h - 24) / canvasH, 1) : 0;

  // The page reports what the user clicked while "Point at a part" is on.
  useEffect(() => {
    const onMessage = (e: MessageEvent) => {
      if (e.source !== frame.current?.contentWindow || e.data?.type !== "jv-picked") return;
      setPicked(e.data.desc as Picked);
      setPicking(false);
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, []);
  const tellFrame = useCallback(() => frame.current?.contentWindow?.postMessage({ type: "jv-pick", on: picking }, "*"), [picking]);
  useEffect(tellFrame, [tellFrame]);

  const send = async () => {
    const instructions = draft.trim();
    if (!instructions || sending) return;
    setSending(true);
    setNote("");
    try {
      const t = await invoke<Task>("site_change", { folder, instructions, region: picked ? pickedForWorker(picked) : null, chatId });
      setDraft("");
      setPicked(null);
      onOpenTask(t.id);
    } catch (e) {
      setNote(String(e));
    }
    setSending(false);
  };
  const openVersions = async () => {
    if (versions) return setVersions(null);
    setVersions(await invoke<Version[]>("site_versions", { folder }).catch(() => []));
  };
  const goBack = async (v: Version) => {
    try {
      await invoke("restore_site_version", { folder, n: v.n });
      setVersions(null);
      setNote(`Went back to how it was ${v.label.replace(/^Before: /, "before “")}${v.label.startsWith("Before: ") ? "”" : ""}. The version you left is saved too.`);
      setShown((n) => n + 1);
    } catch (e) {
      setNote(String(e));
    }
  };
  const exportZip = async () => {
    try {
      const name = folder.split("/").filter(Boolean).pop() ?? "website";
      const path = await askSavePath(safeFileName(name, "zip"), { name: "Zip file", ext: "zip" });
      if (!path) return;
      setNote("Making the zip…");
      await invoke<string>("export_site", { folder, path });
      setNote(`Saved ${fileNameOf(path)}`);
    } catch (e) {
      setNote(`Couldn't make the zip: ${e}`);
    }
  };

  const lines = useMemo(() => (text == null ? [] : highlight(text, file)), [text, file]);
  const width = SIZES.find((s) => s.id === size)!.width;
  const current = files.find((f) => f.path === file);
  const now = Date.now();

  return (
    <div className="site-preview">
      <div className="sp-bar">
        <div className="seg" role="tablist" aria-label="Preview or code">
          {isVideo && (
            <button role="tab" aria-selected={tab === "video"} className={tab === "video" ? "on" : ""} onClick={() => setTab("video")}>
              Video
            </button>
          )}
          <button role="tab" aria-selected={tab === "preview"} className={tab === "preview" ? "on" : ""} onClick={() => setTab("preview")}>
            {isVideo ? "Frame" : "Preview"}
          </button>
          <button role="tab" aria-selected={tab === "code"} className={tab === "code" ? "on" : ""} onClick={() => setTab("code")}>
            Code{files.length ? ` · ${files.length}` : ""}
          </button>
          <button role="tab" aria-selected={tab === "review"} className={tab === "review" ? "on" : ""} onClick={() => setTab("review")}>
            Review{director?.rounds.length ? ` · ${director.rounds[director.rounds.length - 1].review?.score ?? "…"}` : director && director.status !== "done" && director.status !== "error" ? " · …" : ""}
          </button>
        </div>
        <span className="sp-title">
          <i className={running ? "live" : ""} />
          {running ? "Updating live" : ""}
        </span>
        <span className="grow" />
        {tab === "review" || tab === "video" ? null : tab === "preview" ? (
          <>
            {isVideo ? (
              <label className="sp-scrub" title="Which moment of the video to show">
                <input
                  type="range"
                  min={0}
                  max={seconds}
                  step={0.1}
                  value={moment}
                  onChange={(e) => setAt(Number(e.target.value))}
                  aria-label="Moment in the video"
                  disabled={!seconds}
                />
                <span>{moment.toFixed(1)}s</span>
              </label>
            ) : (
              <div className="seg" role="group" aria-label="Screen size">
                {SIZES.map((s) => (
                  <button key={s.id} className={size === s.id ? "on" : ""} onClick={() => setSize(s.id)}>
                    {s.label}
                  </button>
                ))}
              </div>
            )}
            <button className={`mini ${picking ? "primary" : ""}`} aria-pressed={picking} onClick={() => setPicking((p) => !p)} disabled={!info?.hasPage}>
              {picking ? "Click a part of the page…" : "Point at a part"}
            </button>
            <button className="mini" onClick={() => frame.current && info?.url && (frame.current.src = `${info.url}?jvpick=1&r=${Date.now()}`)} disabled={!info?.hasPage}>
              Reload
            </button>
            <button className="mini" onClick={openVersions} aria-expanded={!!versions} disabled={!info?.hasPage}>
              Versions
            </button>
            <button className="mini" onClick={exportZip} disabled={!info?.hasPage}>
              Zip
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

      {tab === "preview" && !isVideo && versions && (
        <div className="sp-versions" role="list" aria-label="Earlier versions">
          {versions.length === 0 && <p className="muted small">No earlier versions yet. The site is saved before every change.</p>}
          {versions.map((v) => (
            <div key={v.n} className="sp-version" role="listitem">
              <span>
                <b>{v.label.replace(/^Before: /, "Before: ")}</b>
                <small>
                  {new Date(v.at).toLocaleDateString([], { day: "numeric", month: "short" })},{" "}
                  {new Date(v.at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
                </small>
              </span>
              <button className="mini" onClick={() => goBack(v)} disabled={running}>
                Go back to this
              </button>
            </div>
          ))}
        </div>
      )}

      {tab === "video" && vinfo && info ? (
        <VideoTab folder={folder} baseUrl={info.baseUrl} info={vinfo} progress={videoProgress} chatId={chatId} onChanged={() => setShown((n) => n + 1)} />
      ) : tab === "review" ? (
        <div className="sp-review">
          <DirectorPanel state={director} running={running} onReview={onReview} />
        </div>
      ) : tab === "preview" ? (
        <div className={`sp-stage${isVideo ? " sp-stage-video" : ""}`} ref={stage}>
          {error ? (
            <p className="muted sp-note">{running ? "Waiting for the worker to create the project folder…" : error}</p>
          ) : !info?.hasPage ? (
            <p className="muted sp-note">{running ? "The first page appears here as soon as the worker saves it…" : "No web page was created in this folder."}</p>
          ) : isVideo ? (
            <div className="sp-canvas" style={{ width: canvasW * fit, height: canvasH * fit }}>
              {fit > 0 && (
                <iframe
                  ref={frame}
                  key={shown}
                  src={`${info.url}?jvpick=1`}
                  title="Video frame"
                  className="sp-frame sp-frame-video"
                  onLoad={() => {
                    tellFrame();
                    seekFrame();
                  }}
                  style={{ width: canvasW, height: canvasH, transform: `scale(${fit})` }}
                />
              )}
            </div>
          ) : (
            <iframe
              ref={frame}
              key={shown}
              src={`${info.url}?jvpick=1`}
              title="Website preview"
              className="sp-frame"
              onLoad={tellFrame}
              style={width ? { width, maxWidth: "100%" } : undefined}
            />
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

      {tab === "preview" && info?.hasPage && !isVideo && (
        <form
          className="sp-ask"
          onSubmit={(e) => {
            e.preventDefault();
            send();
          }}
        >
          {picked && (
            <div className="sp-picked">
              <span>
                Changing <b>{pickedLabel(picked)}</b>
              </span>
              <button type="button" className="mini" onClick={() => setPicked(null)} aria-label="Stop pointing at this">
                ×
              </button>
            </div>
          )}
          {note && <p className="muted small sp-note-line">{note}</p>}
          <div className="ask">
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M12 2l1.8 5.2L19 9l-5.2 1.8L12 16l-1.8-5.2L5 9l5.2-1.8zM19 14l.9 2.1L22 17l-2.1.9L19 20l-.9-2.1L16 17l2.1-.9z" />
            </svg>
            <input
              id="site-ask"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              placeholder={picked ? "What should change here?" : "Tell Jarvis what to change, e.g. “make the header darker”"}
              disabled={sending}
              aria-label="Change the website"
            />
            <button className="btn primary" type="submit" disabled={!draft.trim() || sending || running}>
              {sending ? "Sending…" : "Change it"}
            </button>
          </div>
        </form>
      )}
    </div>
  );
}
