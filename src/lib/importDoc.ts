// Turn a file the user opened into Markdown for the editor. Everything runs inside the app, so it
// behaves the same on macOS and Windows. The Word and HTML converters load only when needed.

/** File types that can be opened as documents. */
export const IMPORT_TYPES = ["docx", "md", "markdown", "txt", "html", "htm"];

export const isImportable = (path: string) => IMPORT_TYPES.includes(path.split(".").pop()?.toLowerCase() ?? "");

const fromBase64 = (b64: string) => Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));

/** UTF-8 text → base64, in chunks so large documents don't overflow the call stack. */
export function textToBase64(text: string) {
  const bytes = new TextEncoder().encode(text);
  let bin = "";
  for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(bin);
}

async function makeTurndown() {
  const [{ default: TurndownService }, { gfm }] = await Promise.all([import("turndown"), import("turndown-plugin-gfm")]);
  const td = new TurndownService({ headingStyle: "atx", bulletListMarker: "-", codeBlockStyle: "fenced", emDelimiter: "*" });
  td.use(gfm);
  td.remove(["script", "style", "title"]);
  // A Markdown table row has to fit on one line, but Word cells often hold several paragraphs.
  // Keep each cell on one line, with its paragraph breaks as <br>. Added after gfm, so it wins.
  td.addRule("one-line-cells", {
    filter: ["th", "td"],
    replacement: (content, node) => {
      const cell = content.trim().replace(/\n{2,}/g, "<br>").replace(/\n/g, " ").replace(/\|/g, "\\|");
      const first = (node as HTMLElement).parentNode?.firstChild === node;
      return `${first ? "| " : " "}${cell} |`;
    },
  });
  return td;
}

/** Turndown pads list markers ("-   item"); tidy them to "- item". */
const tidy = (markdown: string) => `${markdown.replace(/^(\s*)([-*+]|\d+\.)\s{2,}(?=\S)/gm, "$1$2 ").trim()}\n`;

async function htmlToMarkdown(html: string): Promise<{ markdown: string; images: number }> {
  const td = await makeTurndown();
  // Embedded pictures would turn into huge blocks of text, so they're left out and counted.
  let images = 0;
  td.addRule("no-images", { filter: "img", replacement: () => (images++, "") });
  return { markdown: tidy(td.turndown(html)), images };
}

/** A ready-to-use HTML → Markdown converter, for editing the formatted document directly. */
export async function loadHtmlToMarkdown(): Promise<(html: string) => string> {
  const td = await makeTurndown();
  return (html) => (html.replace(/<br\s*\/?>|&nbsp;|\s/g, "") ? tidy(td.turndown(html)) : "");
}

export interface Imported {
  markdown: string;
  /** Things the user should know about the conversion. */
  notes: string[];
}

export async function toMarkdown(ext: string, dataBase64: string): Promise<Imported> {
  const bytes = fromBase64(dataBase64);
  const text = () => new TextDecoder().decode(bytes);
  switch (ext) {
    case "md":
    case "markdown":
    case "txt":
      return { markdown: text(), notes: [] };
    case "html":
    case "htm": {
      const { markdown, images } = await htmlToMarkdown(text());
      return { markdown, notes: images ? [`${images} image${images > 1 ? "s were" : " was"} left out.`] : [] };
    }
    case "docx": {
      const mammoth = (await import("mammoth")).default;
      const result = await mammoth.convertToHtml(
        { arrayBuffer: bytes.buffer as ArrayBuffer },
        // Word's Title and Subtitle styles become headings instead of plain lines.
        { styleMap: ["p[style-name='Title'] => h1:fresh", "p[style-name='Subtitle'] => h2:fresh"] },
      );
      const { markdown, images } = await htmlToMarkdown(result.value);
      const notes = images ? [`${images} image${images > 1 ? "s were" : " was"} left out.`] : [];
      return { markdown, notes };
    }
    default:
      throw new Error(`Jarvis can't open .${ext} files.`);
  }
}
