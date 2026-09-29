// Markdown → Word (.docx), built entirely in the app so export works the same on every OS.
// The docx library is loaded only when someone exports, keeping it out of the startup bundle.

import type { FileChild, ParagraphChild } from "docx";
import { marked, type Token, type Tokens } from "marked";

interface RunStyle {
  bold?: boolean;
  italics?: boolean;
  strike?: boolean;
  code?: boolean;
  link?: boolean;
}

const ENTITIES: Record<string, string> = { "&amp;": "&", "&lt;": "<", "&gt;": ">", "&quot;": '"', "&#39;": "'" };
const decode = (s: string) => s.replace(/&(?:amp|lt|gt|quot|#39);/g, (m) => ENTITIES[m] ?? m);

/** A Word document for this title and Markdown, as base64 ready to hand to the backend. */
export async function markdownToDocx(title: string, markdown: string, includeTitle = true): Promise<string> {
  const d = await import("docx");
  const GREY = { type: d.ShadingType.CLEAR, fill: "F2F2F2", color: "auto" };
  const HEADINGS = [
    d.HeadingLevel.HEADING_1,
    d.HeadingLevel.HEADING_2,
    d.HeadingLevel.HEADING_3,
    d.HeadingLevel.HEADING_4,
    d.HeadingLevel.HEADING_5,
    d.HeadingLevel.HEADING_6,
  ];

  const run = (text: string, st: RunStyle) =>
    new d.TextRun({
      text: decode(text),
      bold: st.bold,
      italics: st.italics,
      strike: st.strike,
      font: st.code ? "Consolas" : undefined,
      style: st.link ? "Hyperlink" : undefined,
    });

  const inline = (tokens: Token[] | undefined, st: RunStyle = {}): ParagraphChild[] => {
    const out: ParagraphChild[] = [];
    for (const t of tokens ?? []) {
      switch (t.type) {
        case "strong":
          out.push(...inline((t as Tokens.Strong).tokens, { ...st, bold: true }));
          break;
        case "em":
          out.push(...inline((t as Tokens.Em).tokens, { ...st, italics: true }));
          break;
        case "del":
          out.push(...inline((t as Tokens.Del).tokens, { ...st, strike: true }));
          break;
        case "codespan":
          out.push(run((t as Tokens.Codespan).text, { ...st, code: true }));
          break;
        case "br":
          out.push(new d.TextRun({ break: 1 }));
          break;
        case "link": {
          const link = t as Tokens.Link;
          out.push(new d.ExternalHyperlink({ link: link.href, children: inline(link.tokens, { ...st, link: true }) }));
          break;
        }
        case "image": {
          const img = t as Tokens.Image;
          out.push(run(`[image: ${img.text || img.href}]`, st));
          break;
        }
        case "html":
          out.push(run((t as Tokens.HTML).text.replace(/<[^>]+>/g, ""), st));
          break;
        default: {
          const g = t as Tokens.Generic;
          if (Array.isArray(g.tokens) && g.tokens.length) out.push(...inline(g.tokens, st));
          else if (typeof g.text === "string") out.push(run(g.text, st));
        }
      }
    }
    return out;
  };

  // Each list gets its own numbering instance so ordered lists restart at 1.
  let listInstance = 0;
  const list = (l: Tokens.List, level: number): FileChild[] => {
    const instance = ++listInstance;
    const depth = Math.min(level, 5);
    const out: FileChild[] = [];
    for (const item of l.items) {
      const nested: Tokens.List[] = [];
      const body: Token[] = [];
      for (const c of item.tokens) {
        if (c.type === "list") nested.push(c as Tokens.List);
        else if ((c.type === "text" || c.type === "paragraph") && (c as Tokens.Text).tokens) body.push(...(c as Tokens.Text).tokens!);
        else if (c.type !== "checkbox") body.push(c);
      }
      const box = item.task ? [run(item.checked ? "☑ " : "☐ ", {})] : [];
      out.push(
        new d.Paragraph({
          children: [...box, ...inline(body)],
          ...(l.ordered ? { numbering: { reference: "ordered", level: depth, instance } } : { bullet: { level: depth } }),
        }),
      );
      for (const n of nested) out.push(...list(n, level + 1));
    }
    return out;
  };

  const table = (t: Tokens.Table) =>
    new d.Table({
      width: { size: 100, type: d.WidthType.PERCENTAGE },
      rows: [
        new d.TableRow({
          tableHeader: true,
          children: t.header.map((c) => new d.TableCell({ shading: GREY, children: [new d.Paragraph({ children: inline(c.tokens, { bold: true }) })] })),
        }),
        ...t.rows.map(
          (r) => new d.TableRow({ children: r.map((c) => new d.TableCell({ children: [new d.Paragraph({ children: inline(c.tokens) })] })) }),
        ),
      ],
    });

  const blocks = (tokens: Token[], quote = false): FileChild[] => {
    const out: FileChild[] = [];
    for (const t of tokens) {
      switch (t.type) {
        case "heading": {
          const h = t as Tokens.Heading;
          out.push(new d.Paragraph({ heading: HEADINGS[Math.min(h.depth, 6) - 1], children: inline(h.tokens) }));
          break;
        }
        case "paragraph":
          out.push(
            new d.Paragraph({
              children: inline((t as Tokens.Paragraph).tokens, quote ? { italics: true } : {}),
              indent: quote ? { left: 720 } : undefined,
              spacing: { after: 120 },
            }),
          );
          break;
        case "text":
          out.push(new d.Paragraph({ children: inline((t as Tokens.Text).tokens ?? [t]) }));
          break;
        case "list":
          out.push(...list(t as Tokens.List, 0));
          break;
        case "blockquote":
          out.push(...blocks((t as Tokens.Blockquote).tokens, true));
          break;
        case "code":
          for (const line of (t as Tokens.Code).text.split("\n")) {
            out.push(new d.Paragraph({ shading: GREY, spacing: { after: 0 }, children: [run(line || " ", { code: true })] }));
          }
          break;
        case "table":
          out.push(table(t as Tokens.Table));
          break;
        case "hr":
          out.push(new d.Paragraph({ border: { bottom: { style: d.BorderStyle.SINGLE, size: 6, color: "BFBFBF", space: 1 } } }));
          break;
        case "html": {
          const text = (t as Tokens.HTML).text.replace(/<[^>]+>/g, "").trim();
          if (text) out.push(new d.Paragraph({ children: [run(text, {})] }));
          break;
        }
        default:
          break; // blank space between blocks
      }
    }
    return out;
  };

  const tokens = marked.lexer(markdown);
  const first = tokens.find((t) => t.type !== "space");
  const hasOwnTitle = first?.type === "heading" && (first as Tokens.Heading).depth === 1;
  const children: FileChild[] = [];
  if (includeTitle && title.trim() && !hasOwnTitle) children.push(new d.Paragraph({ heading: d.HeadingLevel.TITLE, children: [new d.TextRun(title.trim())] }));
  children.push(...blocks(tokens));

  const doc = new d.Document({
    title: title.trim() || "Document",
    creator: "Jarvis",
    styles: { default: { document: { run: { font: "Calibri", size: 22 } } } },
    numbering: {
      config: [
        {
          reference: "ordered",
          levels: [0, 1, 2, 3, 4, 5].map((level) => ({
            level,
            format: d.LevelFormat.DECIMAL,
            text: `%${level + 1}.`,
            alignment: d.AlignmentType.START,
            style: { paragraph: { indent: { left: 720 * (level + 1), hanging: 360 } } },
          })),
        },
      ],
    },
    sections: [{ children }],
  });
  return d.Packer.toBase64String(doc);
}

/** A file name that's valid on macOS and Windows alike. */
export function safeFileName(title: string, ext: string) {
  const base = title
    .replace(/[\\/:*?"<>|\u0000-\u001f]+/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .replace(/[. ]+$/, "")
    .slice(0, 80);
  return `${base || "document"}.${ext}`;
}
