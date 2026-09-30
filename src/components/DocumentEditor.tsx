import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import DOMPurify from "dompurify";
import { marked } from "marked";
import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import { markdownToDocx, safeFileName } from "../lib/exportDocx";
import { printableHtml } from "../lib/exportPdf";
import { hydrateImages, inlineMarkdownImages, loadMarkdownImages, restoreImageSources } from "../lib/docImages";
import { loadHtmlToMarkdown, textToBase64 } from "../lib/importDoc";
import { confirm } from "@tauri-apps/plugin-dialog";
import { askSavePath, fileNameOf } from "../lib/saveAs";
import type { Task } from "../lib/types";
import PanelControls from "./PanelControls";

type View = "write" | "split" | "preview";
type SaveState = "saved" | "saving" | "unsaved" | "error";
type Fmt = "h1" | "h2" | "bold" | "italic" | "strike" | "code" | "link" | "bullet" | "numbered" | "task" | "quote" | "table" | "divider";

const VIEWS: { id: View; label: string }[] = [
  { id: "write", label: "Markdown" },
  { id: "split", label: "Split" },
  { id: "preview", label: "Preview" },
];

/** Inline formats wrap the selection: [before, after, placeholder]. */
const WRAP: Partial<Record<Fmt, [string, string, string]>> = {
  bold: ["**", "**", "bold text"],
  italic: ["*", "*", "italic text"],
  strike: ["~~", "~~", "struck text"],
  code: ["`", "`", "code"],
};

/** Line formats toggle a prefix on every selected line. */
const LINE: Partial<Record<Fmt, (i: number) => string>> = {
  h1: () => "# ",
  h2: () => "## ",
  quote: () => "> ",
  bullet: () => "- ",
  numbered: (i) => `${i + 1}. `,
  task: () => "- [ ] ",
};
const ANY_PREFIX = /^(#{1,6} |> |- \[[ xX]\] |[-*+] |\d+\. )/;

const TOOLS: { fmt: Fmt; label: string; title: string }[] = [
  { fmt: "h1", label: "H1", title: "Heading" },
  { fmt: "h2", label: "H2", title: "Subheading" },
  { fmt: "bold", label: "B", title: "Bold (⌘/Ctrl B)" },
  { fmt: "italic", label: "I", title: "Italic (⌘/Ctrl I)" },
  { fmt: "strike", label: "S", title: "Strikethrough" },
  { fmt: "code", label: "</>", title: "Inline code" },
  { fmt: "link", label: "Link", title: "Link (⌘/Ctrl K)" },
  { fmt: "bullet", label: "•", title: "Bulleted list" },
  { fmt: "numbered", label: "1.", title: "Numbered list" },
  { fmt: "task", label: "☐", title: "Checklist" },
  { fmt: "quote", label: "❝", title: "Quote" },
  { fmt: "table", label: "Table", title: "Insert a table" },
  { fmt: "divider", label: "—", title: "Divider" },
];

const words = (s: string) => (s.trim() ? s.trim().split(/\s+/).length : 0);

interface Props {
  doc: Task;
  tasks: Task[];
  micOn: boolean;
  onToggleMic: () => void;
  /** Ask Codex to write or change this document. */
  onWrite: (instructions: string) => Promise<unknown>;
  expanded: boolean;
  onToggleExpand: () => void;
  onClose: () => void;
}

/** Markdown editor with a live preview, shown in the side panel. Codex writes into it while you watch. */
export default function DocumentEditor({ doc, tasks, micOn, onToggleMic, onWrite, expanded, onToggleExpand, onClose }: Props) {
  const [text, setText] = useState("");
  const [title, setTitle] = useState(doc.title);
  // Documents open as a readable preview; Write and Split are one click away.
  const [view, setView] = useState<View>("preview");
  const [save, setSave] = useState<SaveState>("saved");
  const [ask, setAsk] = useState("");
  const [flash, setFlash] = useState("");
  const [error, setError] = useState("");
  const editorRef = useRef<HTMLTextAreaElement>(null);
  const previewRef = useRef<HTMLDivElement>(null);

  // Latest values for the save timer and the cleanup that saves on the way out.
  const textRef = useRef(text);
  const titleRef = useRef(title);
  textRef.current = text;
  titleRef.current = title;
  const dirty = useRef(false);
  const timer = useRef<number | undefined>(undefined);

  // Editing the formatted document directly. The Markdown stays the real document: typing in the
  // formatted view is turned back into Markdown a moment later, so Codex, exports and saving back
  // all keep working from the same text.
  const richRef = useRef<HTMLDivElement | null>(null);
  const toMarkdown = useRef<((html: string) => string) | null>(null);
  /** The Markdown produced by the last typing in the formatted view, so it isn't re-rendered under the caret. */
  const typedHere = useRef<string | null>(null);
  const richTimer = useRef<number | undefined>(undefined);
  const lastFocus = useRef<"source" | "rich">("rich");
  useEffect(() => {
    loadHtmlToMarkdown().then((fn) => (toMarkdown.current = fn));
    document.execCommand("defaultParagraphSeparator", false, "p");
  }, []);

  /** Turn pending typing in the formatted view into Markdown now. True if the text changed. */
  const commitRich = () => {
    if (richTimer.current === undefined) return false;
    window.clearTimeout(richTimer.current);
    richTimer.current = undefined;
    const el = richRef.current;
    const convert = toMarkdown.current;
    if (!el || !convert) return false;
    const md = convert(restoreImageSources(el.innerHTML));
    if (md === textRef.current) return false;
    typedHere.current = md;
    textRef.current = md;
    setText(md);
    dirty.current = true;
    return true;
  };

  // Codex working on this document right now (writing it, or an edit). It owns the file until it's done.
  const job = tasks.find((t) => t.status === "running" && t.dir === doc.dir);
  const locked = !!job;

  // ---------- load & save ----------
  const load = useCallback(async () => {
    try {
      const t = await invoke<string>("read_document", { id: doc.id });
      if (!dirty.current) setText(t);
    } catch (e) {
      setError(String(e));
    }
  }, [doc.id]);

  const flush = useCallback(async () => {
    window.clearTimeout(timer.current);
    commitRich();
    if (!dirty.current) return;
    dirty.current = false;
    setSave("saving");
    try {
      await invoke("save_document", { id: doc.id, content: textRef.current, title: titleRef.current });
      setSave("saved");
    } catch (e) {
      dirty.current = true;
      setSave("error");
      setError(`Couldn't save: ${e}`);
    }
  }, [doc.id]);

  const changed = () => {
    dirty.current = true;
    setSave("unsaved");
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(flush, 600);
  };

  // Opening a document loads it; leaving it (or switching) saves pending typing first.
  useEffect(() => {
    dirty.current = false;
    setTitle(doc.title);
    setSave("saved");
    setError("");
    load();
    return () => {
      flush();
    };
  }, [doc.id]);

  // While Codex works, follow the file as it saves. New sections appear at the end, so follow them
  // down, but only while you're at the bottom: scroll up to read and it stops pulling you down.
  const follow = useRef(true);
  const trackFollow = (el: HTMLElement) => {
    follow.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
  };
  useEffect(() => {
    if (!job) return;
    follow.current = true;
    const poll = window.setInterval(async () => {
      const next = await invoke<string>("read_document", { id: doc.id }).catch(() => null);
      const prev = textRef.current;
      if (next == null || next === prev) return;
      dirty.current = false;
      setText(next);
      if (follow.current && next.startsWith(prev.trimEnd())) {
        requestAnimationFrame(() => {
          for (const el of [editorRef.current, previewRef.current]) if (el) el.scrollTop = el.scrollHeight;
        });
      }
    }, 700);
    return () => {
      window.clearInterval(poll);
      dirty.current = false;
      load();
    };
  }, [job?.id, load]);

  // ---------- preview ----------
  const deferred = useDeferredValue(text);
  const html = useMemo(() => DOMPurify.sanitize(marked.parse(deferred, { async: false }) as string), [deferred]);

  const htmlRef = useRef(html);
  htmlRef.current = html;
  // When the formatted view appears (opening, or switching layout), fill it with the document.
  // The HTML last put into the formatted view (before its images were loaded from disk).
  const rendered = useRef<string | null>(null);
  const dirRef = useRef(doc.dir);
  dirRef.current = doc.dir;
  const setRich = useCallback((el: HTMLDivElement | null) => {
    if (el && el !== richRef.current) {
      el.innerHTML = htmlRef.current;
      rendered.current = htmlRef.current;
      hydrateImages(el, dirRef.current);
    }
    richRef.current = el;
  }, []);
  // Changes from anywhere else (loading, Codex, the Markdown view) re-render it. Its own typing
  // doesn't, so the caret stays where you are.
  useEffect(() => {
    const el = richRef.current;
    if (!el) return;
    if (typedHere.current !== null && deferred === typedHere.current) return;
    typedHere.current = null;
    if (rendered.current !== html) {
      el.innerHTML = html;
      rendered.current = html;
      hydrateImages(el, doc.dir);
    }
  }, [html]);

  const onRichInput = () => {
    window.clearTimeout(richTimer.current);
    richTimer.current = window.setTimeout(() => {
      if (commitRich()) changed();
    }, 250);
  };

  // Links open in the browser with ⌘/Ctrl-click. A plain click places the caret for editing.
  const onPreviewClick = (e: React.MouseEvent) => {
    const a = (e.target as HTMLElement).closest("a");
    if (!a) return;
    e.preventDefault();
    if (a.href?.startsWith("http") && (locked || e.metaKey || e.ctrlKey)) openUrl(a.href);
  };

  // Scrolling the source moves the preview to the same place.
  const syncScroll = () => {
    const a = editorRef.current;
    if (a) trackFollow(a);
    const b = previewRef.current;
    if (!a || !b || job) return;
    const ratio = a.scrollTop / Math.max(1, a.scrollHeight - a.clientHeight);
    b.scrollTop = ratio * (b.scrollHeight - b.clientHeight);
  };

  // ---------- formatting ----------
  /** Replace the selection, keeping the platform's undo history when it can. */
  const replace = (el: HTMLTextAreaElement, start: number, end: number, insert: string, select: [number, number]) => {
    el.focus();
    el.setSelectionRange(start, end);
    if (!document.execCommand("insertText", false, insert)) {
      const next = el.value.slice(0, start) + insert + el.value.slice(end);
      setText(next);
      changed();
      requestAnimationFrame(() => el.setSelectionRange(...select));
      return;
    }
    el.setSelectionRange(...select);
  };

  /** Format the selection in the formatted view, in place. */
  const formatRich = (fmt: Fmt) => {
    const el = richRef.current;
    if (!el) return;
    el.focus();
    const exec = (cmd: string, value?: string) => document.execCommand(cmd, false, value);
    const block = (tag: string) => exec("formatBlock", document.queryCommandValue("formatBlock").toUpperCase() === tag ? "P" : tag);
    const selected = window.getSelection()?.toString() ?? "";
    const escape = (t: string) => t.replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]!);
    switch (fmt) {
      case "bold":
        exec("bold");
        break;
      case "italic":
        exec("italic");
        break;
      case "strike":
        exec("strikeThrough");
        break;
      case "code":
        exec("insertHTML", `<code>${escape(selected || "code")}</code>`);
        break;
      case "h1":
        block("H1");
        break;
      case "h2":
        block("H2");
        break;
      case "quote":
        block("BLOCKQUOTE");
        break;
      case "bullet":
        exec("insertUnorderedList");
        break;
      case "numbered":
        exec("insertOrderedList");
        break;
      case "task":
        exec("insertHTML", `<ul><li><input type="checkbox"> ${escape(selected || "Task")}</li></ul>`);
        break;
      case "divider":
        exec("insertHorizontalRule");
        break;
      case "table":
        exec("insertHTML", "<table><thead><tr><th>Column</th><th>Column</th></tr></thead><tbody><tr><td>Cell</td><td>Cell</td></tr></tbody></table><p><br></p>");
        break;
      case "link": {
        const url = /^https?:\/\/\S+$/.test(selected.trim()) ? selected.trim() : "https://";
        exec("createLink", url);
        if (url === "https://") note("Link added. Set its address in the Markdown view.");
        break;
      }
    }
    onRichInput();
  };

  const format = (fmt: Fmt) => {
    if (locked) return;
    if (view === "preview" || (view === "split" && lastFocus.current === "rich")) return formatRich(fmt);
    const el = editorRef.current;
    if (!el) return;
    const { selectionStart: s, selectionEnd: e, value } = el;
    const sel = value.slice(s, e);

    const wrap = WRAP[fmt];
    if (wrap) {
      const [before, after, placeholder] = wrap;
      const body = sel || placeholder;
      replace(el, s, e, before + body + after, [s + before.length, s + before.length + body.length]);
      return;
    }
    if (fmt === "link") {
      const label = sel || "link text";
      const url = s + label.length + 3;
      replace(el, s, e, `[${label}](https://)`, [url, url + 8]);
      return;
    }
    if (fmt === "divider" || fmt === "table") {
      const block = fmt === "divider" ? "---" : "| Column | Column |\n| --- | --- |\n| Cell | Cell |";
      const lead = s > 0 && value[s - 1] !== "\n" ? "\n\n" : s > 0 ? "\n" : "";
      const insert = `${lead}${block}\n\n`;
      replace(el, s, e, insert, [s + insert.length, s + insert.length]);
      return;
    }

    // Line formats work on whole lines, and toggle off when every line already has them.
    const prefix = LINE[fmt]!;
    const from = s === 0 ? 0 : value.lastIndexOf("\n", s - 1) + 1;
    // A selection ending just after a line break shouldn't pull in the next line.
    const endAt = e > s && value[e - 1] === "\n" ? e - 1 : e;
    const nl = value.indexOf("\n", endAt);
    const to = nl === -1 ? value.length : nl;
    const lines = value.slice(from, to).split("\n");
    const has = lines.every((l, i) => l.startsWith(prefix(i)));
    const next = lines.map((l, i) => (has ? l.slice(prefix(i).length) : prefix(i) + l.replace(ANY_PREFIX, ""))).join("\n");
    replace(el, from, to, next, [from, from + next.length]);
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
    const key = e.key.toLowerCase();
    const map: Record<string, Fmt> = { b: "bold", i: "italic", k: "link" };
    if (map[key]) {
      e.preventDefault();
      format(map[key]);
    } else if (key === "s") {
      e.preventDefault();
      flush();
    }
  };

  const onRichKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
    const key = e.key.toLowerCase();
    if (key === "k") {
      e.preventDefault();
      formatRich("link");
    } else if (key === "s") {
      e.preventDefault();
      flush();
    }
  };

  // ---------- actions ----------
  const note = (msg: string) => {
    setFlash(msg);
    window.setTimeout(() => setFlash((m) => (m === msg ? "" : m)), 2200);
  };

  const submitAsk = async (e: React.FormEvent) => {
    e.preventDefault();
    const q = ask.trim();
    if (!q || locked) return;
    setAsk("");
    setError("");
    await flush();
    onWrite(q).catch((err) => setError(String(err)));
  };

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      note("Copied Markdown");
    } catch (e) {
      setError(`Couldn't copy: ${e}`);
    }
  };

  const exportWord = async () => {
    try {
      await flush();
      const path = await askSavePath(safeFileName(title, "docx"), { name: "Word document", ext: "docx" });
      if (!path) return;
      note("Building the Word file…");
      const data = await markdownToDocx(title, text, true, await loadMarkdownImages(text, doc.dir));
      await invoke<string>("save_export", { path, dataBase64: data });
      note(`Saved ${fileNameOf(path)}`);
    } catch (e) {
      setError(`Couldn't export: ${e}`);
    }
  };

  const step = job?.steps[job.steps.length - 1];
  const exportPdf = async () => {
    try {
      await flush();
      const path = await askSavePath(safeFileName(title, "pdf"), { name: "PDF", ext: "pdf" });
      if (!path) return;
      note("Making the PDF…");
      await invoke<string>("export_pdf", { path, html: printableHtml(title, await inlineMarkdownImages(text, doc.dir)) });
      note(`Saved ${fileNameOf(path)}`);
    } catch (e) {
      setError(`Couldn't make the PDF: ${e}`);
    }
  };

  // Opened from a file: it can be written back there, in that file's own format.
  const source = doc.source ? { path: doc.source, name: doc.source.split(/[\\/]/).pop() ?? doc.source, ext: doc.source.split(".").pop()?.toLowerCase() ?? "" } : null;
  const saveToSource = async () => {
    if (!source) return;
    try {
      await flush();
      const replace = await confirm(`Replace ${source.name} with this version?\n\nThe untouched original stays in the document's folder.`, {
        title: "Save to original file",
        kind: "warning",
        okLabel: "Replace",
      });
      if (!replace) return;
      note(`Saving ${source.name}…`);
      const data =
        source.ext === "docx"
          ? await markdownToDocx(title, text, false, await loadMarkdownImages(text, doc.dir))
          : textToBase64(
              source.ext === "html" || source.ext === "htm" ? printableHtml(title, await inlineMarkdownImages(text, doc.dir), false) : text,
            );
      await invoke<string>("save_to_source", { id: doc.id, dataBase64: data });
      note(`Saved to ${source.name}`);
    } catch (e) {
      setError(`Couldn't save to ${source.name}: ${e}`);
    }
  };

  const status = job
    ? `Codex · ${step ? step.label : "Starting"}…`
    : flash || { saved: "Saved", saving: "Saving…", unsaved: "Editing…", error: "Couldn't save" }[save];
  const stopJob = () => job && invoke("cancel_task", { id: job.id }).catch((e) => setError(String(e)));

  const count = words(text);

  return (
    <section className="docpage">
      <header>
        <div className="doc-title">
          <input
            value={title}
            onChange={(e) => {
              setTitle(e.target.value);
              changed();
            }}
            placeholder="Untitled document"
            aria-label="Document title"
            disabled={locked}
          />
          <span className={`doc-status ${locked ? "live" : save}`} aria-live="polite">
            <i />
            {status}
          </span>
        </div>
        {expanded && (
          <button
            className={`icon-btn mic-mini ${micOn ? "on" : ""}`}
            onClick={onToggleMic}
            aria-label={micOn ? "Mute microphone" : "Talk to Jarvis"}
            title={micOn ? "Listening. Click to mute" : "Talk to Jarvis about this document"}
          >
            <svg viewBox="0 0 24 24">
              <path d="M12 14a3 3 0 0 0 3-3V5a3 3 0 1 0-6 0v6a3 3 0 0 0 3 3zm5-3a5 5 0 0 1-10 0H5a7 7 0 0 0 6 6.92V21h2v-3.08A7 7 0 0 0 19 11h-2z" />
            </svg>
          </button>
        )}
        <PanelControls expanded={expanded} onToggleExpand={onToggleExpand} onClose={onClose} />
      </header>

      <div className="doc-toolbar">
        {view !== "preview" && (
          <div className="fmts" role="toolbar" aria-label="Formatting">
            {TOOLS.map((t) => (
              <button
                key={t.fmt}
                className={`fmt fmt-${t.fmt}`}
                title={t.title}
                aria-label={t.title}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => format(t.fmt)}
                disabled={locked}
              >
                {t.label}
              </button>
            ))}
          </div>
        )}
        <div className="doc-right">
          {source && (
            <button className="mini" onClick={saveToSource} disabled={!text.trim() || locked} title={source.path}>
              Save to {source.name}
            </button>
          )}
          <div className="seg" role="group" aria-label="Layout">
            {VIEWS.map((v) => (
              <button key={v.id} className={view === v.id ? "on" : ""} onClick={() => setView(v.id)}>
                {v.label}
              </button>
            ))}
          </div>
          <button className="mini" onClick={copy} disabled={!text.trim()}>
            Copy
          </button>
          <button className="mini" onClick={exportWord} disabled={!text.trim() || locked}>
            Word
          </button>
          <button className="mini" onClick={exportPdf} disabled={!text.trim() || locked}>
            PDF
          </button>
          <button className="mini" onClick={() => invoke("reveal_task", { id: doc.id }).catch((e) => setError(String(e)))}>
            Show file
          </button>
        </div>
      </div>

      {source && (
        <p className="doc-source-note">
          Opened from <b title={source.path}>{source.name}</b>.
          {source.ext === "docx"
            ? " Saving back keeps headings, lists, tables, bold, italics and links. Other Word formatting, such as fonts, colours and pictures, isn't kept."
            : ""}
        </p>
      )}

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

      <div className={`doc-body ${view}`}>
        {view !== "preview" && (
          <textarea
            ref={editorRef}
            className="doc-source"
            value={text}
            onChange={(e) => {
              setText(e.target.value);
              changed();
            }}
            onKeyDown={onKeyDown}
            onFocus={() => (lastFocus.current = "source")}
            onScroll={syncScroll}
            readOnly={locked}
            spellCheck
            placeholder={"Start typing in Markdown, or tell Jarvis below what to write.\n\n## A section\n\nSome **bold** text and a list:\n\n- first\n- second"}
            aria-label="Document source (Markdown)"
          />
        )}
        {view !== "write" && (
          <div
            ref={previewRef}
            className={`doc-preview markdown ${job ? "streaming" : ""}`}
            onClick={onPreviewClick}
            onScroll={(e) => trackFollow(e.currentTarget)}
          >
            <div
              ref={setRich}
              className="rich"
              contentEditable={!locked}
              suppressContentEditableWarning
              spellCheck
              role="textbox"
              aria-multiline="true"
              aria-label="Document"
              data-placeholder={job ? "Codex is writing this document. It appears here section by section as it's saved." : "Start writing here, or tell Codex below what to write."}
              onInput={onRichInput}
              onKeyDown={onRichKey}
              onFocus={() => (lastFocus.current = "rich")}
              onBlur={() => commitRich() && changed()}
            />
          </div>
        )}
      </div>

      <footer className="doc-foot">
        <form className="ask" onSubmit={submitAsk}>
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M12 2l1.9 5.6L19.5 9.5 13.9 11.4 12 17l-1.9-5.6L4.5 9.5l5.6-1.9zM19 14l.9 2.6 2.6.9-2.6.9L19 21l-.9-2.6-2.6-.9 2.6-.9z" />
          </svg>
          <input
            value={ask}
            onChange={(e) => setAsk(e.target.value)}
            placeholder={job ? "Codex is working on this document…" : text.trim() ? "Ask Codex to change this document…" : "Tell Codex what to write…"}
            aria-label="Ask Jarvis to write or edit"
            disabled={locked}
          />
          {job ? (
            <button type="button" className="mini" onClick={stopJob}>
              Stop
            </button>
          ) : (
            <button type="submit" className="btn primary" disabled={!ask.trim()}>
              {text.trim() ? "Edit" : "Write"}
            </button>
          )}
        </form>
        <span className="muted small">
          {count} word{count === 1 ? "" : "s"} · {Math.max(1, Math.round(count / 220))} min read
        </span>
      </footer>
    </section>
  );
}
