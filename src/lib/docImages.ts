// Images in documents. Codex saves pictures next to document.md and links them relatively
// (`![Brain](ai-brain-3d.png)`). The web view can't load files from disk by path, so local
// images are read through the backend and shown as data URLs. The Markdown keeps the original
// file name: `data-md-src` remembers it, and it's put back before converting HTML to Markdown.

import { loadImage } from "../components/ImageThumb";

const REMOTE = /^(https?:|data:|blob:)/i;

/** A local image link resolved against the document's folder. */
export function resolveImagePath(dir: string, src: string): string {
  let p = src;
  try {
    p = decodeURI(src);
  } catch {
    /* keep as written */
  }
  p = p.replace(/^file:\/\//i, "");
  if (p.startsWith("/")) return p;
  const parts = `${dir.replace(/\/+$/, "")}/${p}`.split("/");
  const out: string[] = [];
  for (const part of parts) {
    if (part === "..") out.pop();
    else if (part !== "." && part !== "") out.push(part);
  }
  return `/${out.join("/")}`;
}

/** Load every local image inside `root` from disk. Safe to call again after re-rendering. */
export function hydrateImages(root: HTMLElement, dir: string) {
  for (const img of Array.from(root.querySelectorAll("img"))) {
    const original = img.getAttribute("data-md-src") ?? img.getAttribute("src");
    if (!original || REMOTE.test(original)) continue;
    img.setAttribute("data-md-src", original);
    if (img.src.startsWith("data:")) continue;
    img.removeAttribute("src");
    img.classList.add("doc-img-loading");
    loadImage(resolveImagePath(dir, original))
      .then((url) => {
        if (img.getAttribute("data-md-src") !== original) return;
        img.src = url;
        img.classList.remove("doc-img-loading");
      })
      .catch(() => {
        img.classList.remove("doc-img-loading");
        img.classList.add("doc-img-missing");
        img.title = `Image not found: ${original}`;
      });
  }
}

/** HTML with loaded images pointed back at their original file names, for turning into Markdown. */
export function restoreImageSources(html: string): string {
  if (!html.includes("data-md-src")) return html;
  const t = document.createElement("template");
  t.innerHTML = html;
  for (const img of Array.from(t.content.querySelectorAll("img[data-md-src]"))) {
    img.setAttribute("src", img.getAttribute("data-md-src")!);
    img.removeAttribute("data-md-src");
    img.classList.remove("doc-img-loading", "doc-img-missing");
    if (!img.getAttribute("class")) img.removeAttribute("class");
  }
  return t.innerHTML;
}

const MD_IMAGE = /!\[([^\]]*)\]\(\s*<?([^)\s>]+)>?(\s+"[^"]*")?\s*\)/g;

/** Local images a Markdown text links to, with their loaded data URLs (missing ones are left out). */
export async function loadMarkdownImages(markdown: string, dir: string): Promise<Map<string, string>> {
  const found = new Map<string, string>();
  const hrefs = new Set(Array.from(markdown.matchAll(MD_IMAGE), (m) => m[2]).filter((h) => !REMOTE.test(h)));
  await Promise.all(
    Array.from(hrefs, async (href) => {
      try {
        found.set(href, await loadImage(resolveImagePath(dir, href)));
      } catch {
        /* missing image: exports show its alt text instead */
      }
    }),
  );
  return found;
}

/** The Markdown with local images embedded as data URLs, for self-contained exports (PDF, HTML). */
export async function inlineMarkdownImages(markdown: string, dir: string): Promise<string> {
  const images = await loadMarkdownImages(markdown, dir);
  if (!images.size) return markdown;
  return markdown.replace(MD_IMAGE, (whole, alt: string, href: string, title = "") =>
    images.has(href) ? `![${alt}](${images.get(href)}${title})` : whole,
  );
}
