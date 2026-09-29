// Markdown → a standalone, print-ready HTML page. The backend prints it to PDF with a headless
// Chromium browser, which lays out every script properly (Nepali included) on macOS and Windows.

import DOMPurify from "dompurify";
import { marked } from "marked";

const PRINT_CSS = `
@page { size: A4; margin: 20mm 18mm; }
html { -webkit-print-color-adjust: exact; print-color-adjust: exact; }
body { margin: 0; color: #1f2329; font: 11pt/1.55 -apple-system, "Segoe UI", "Helvetica Neue", Arial, "Noto Sans", "Noto Sans Devanagari", "Nirmala UI", sans-serif; }
h1.doc-title { font-size: 22pt; line-height: 1.2; margin: 0 0 16pt; }
h1 { font-size: 18pt; margin: 20pt 0 8pt; }
h2 { font-size: 14pt; margin: 18pt 0 6pt; }
h3 { font-size: 12pt; margin: 14pt 0 4pt; }
h1, h2, h3, h4 { break-after: avoid; page-break-after: avoid; }
p, li { orphans: 3; widows: 3; }
table { border-collapse: collapse; width: 100%; margin: 10pt 0; font-size: 10pt; }
th, td { border: 1px solid #cfd6de; padding: 5pt 7pt; text-align: left; vertical-align: top; }
th { background: #f2f4f7; }
tr, img, pre, blockquote { break-inside: avoid; }
code { font: 9.5pt Menlo, Consolas, monospace; background: #f2f4f7; padding: 0 3pt; border-radius: 3pt; }
pre { background: #f6f8fa; padding: 8pt 10pt; border-radius: 4pt; white-space: pre-wrap; }
pre code { background: none; padding: 0; }
blockquote { margin: 10pt 0; padding: 2pt 12pt; border-left: 3px solid #0a66c2; color: #5f6b76; }
a { color: #0a66c2; text-decoration: none; }
img { max-width: 100%; }
hr { border: 0; border-top: 1px solid #dfe3e8; margin: 16pt 0; }
`;

const escape = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);

/** The page to print: the title as a heading (unless the Markdown starts with its own) and the body. */
export function printableHtml(title: string, markdown: string, includeTitle = true): string {
  const body = DOMPurify.sanitize(marked.parse(markdown, { async: false }) as string);
  const heading = includeTitle && title.trim() && !/^\s*#\s/.test(markdown) ? `<h1 class="doc-title">${escape(title.trim())}</h1>` : "";
  return `<!doctype html><html><head><meta charset="utf-8"><title>${escape(title)}</title><style>${PRINT_CSS}</style></head><body>${heading}${body}</body></html>`;
}
