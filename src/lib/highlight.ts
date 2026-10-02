// A small syntax highlighter for the code view: HTML, CSS, JavaScript/TypeScript and JSON.
// It's regex-based on purpose (no dependency, works offline); it only has to make generated sites readable.

export type Tok = { cls: string; text: string };

const JS_KEYWORDS = /^(?:const|let|var|function|return|if|else|for|while|do|switch|case|break|continue|new|class|extends|import|export|from|default|async|await|try|catch|finally|throw|typeof|instanceof|in|of|this|super|static|get|set|yield|void|delete)$/;

function js(code: string): Tok[] {
  const out: Tok[] = [];
  const re = /(\/\/[^\n]*|\/\*[\s\S]*?\*\/)|("(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'|`(?:[^`\\]|\\[\s\S])*`)|(\b\d[\d_.]*(?:e[+-]?\d+)?\b)|([A-Za-z_$][\w$]*)(\s*\()?|([\s\S])/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(code))) {
    if (m[1]) out.push({ cls: "c", text: m[1] });
    else if (m[2]) out.push({ cls: "s", text: m[2] });
    else if (m[3]) out.push({ cls: "n", text: m[3] });
    else if (m[4]) {
      const w = m[4];
      if (JS_KEYWORDS.test(w)) out.push({ cls: "k", text: w });
      else if (/^(?:true|false|null|undefined|NaN|Infinity)$/.test(w)) out.push({ cls: "n", text: w });
      else if (m[5]) out.push({ cls: "f", text: w });
      else out.push({ cls: "", text: w });
      if (m[5]) out.push({ cls: "", text: m[5] });
    } else out.push({ cls: "", text: m[6] });
  }
  return out;
}

function css(code: string): Tok[] {
  const out: Tok[] = [];
  const re = /(\/\*[\s\S]*?\*\/)|("(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*')|(@[\w-]+)|(#[0-9a-fA-F]{3,8}\b)|(-?\d*\.?\d+(?:%|px|r?em|vh|vw|vmin|vmax|ch|s|ms|deg|fr)?\b)|(--[\w-]+|[a-zA-Z-]+)(?=\s*:)|([\s\S])/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(code))) {
    if (m[1]) out.push({ cls: "c", text: m[1] });
    else if (m[2]) out.push({ cls: "s", text: m[2] });
    else if (m[3]) out.push({ cls: "k", text: m[3] });
    else if (m[4] || m[5]) out.push({ cls: "n", text: m[4] ?? m[5] });
    else if (m[6]) out.push({ cls: "p", text: m[6] });
    else out.push({ cls: "", text: m[7] });
  }
  return out;
}

function json(code: string): Tok[] {
  const out: Tok[] = [];
  const re = /("(?:[^"\\\n]|\\.)*")(\s*:)?|(-?\d[\d.eE+-]*)|\b(true|false|null)\b|([\s\S])/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(code))) {
    if (m[1]) {
      out.push({ cls: m[2] ? "p" : "s", text: m[1] });
      if (m[2]) out.push({ cls: "", text: m[2] });
    } else if (m[3] || m[4]) out.push({ cls: "n", text: (m[3] ?? m[4])! });
    else out.push({ cls: "", text: m[5] });
  }
  return out;
}

function html(code: string): Tok[] {
  const out: Tok[] = [];
  const re = /(<!--[\s\S]*?-->)|(<!doctype[^>]*>)|(<\/?[A-Za-z][^>]*>)|([^<]+|<)/gi;
  let m: RegExpExecArray | null;
  while ((m = re.exec(code))) {
    if (m[1]) out.push({ cls: "c", text: m[1] });
    else if (m[2]) out.push({ cls: "k", text: m[2] });
    else if (m[3]) {
      const t = /^(<\/?)([\w:-]+)([\s\S]*?)(\/?>)$/.exec(m[3]);
      if (!t) {
        out.push({ cls: "", text: m[3] });
        continue;
      }
      out.push({ cls: "p0", text: t[1] }, { cls: "t", text: t[2] });
      const attrs = /(\s+)|([\w:@.-]+)(=)?("[^"]*"|'[^']*'|[^\s"'>]+)?|([\s\S])/g;
      let a: RegExpExecArray | null;
      while ((a = attrs.exec(t[3]))) {
        if (a[1]) out.push({ cls: "", text: a[1] });
        else if (a[2]) {
          out.push({ cls: "a", text: a[2] });
          if (a[3]) out.push({ cls: "", text: a[3] });
          if (a[4]) out.push({ cls: "s", text: a[4] });
        } else out.push({ cls: "", text: a[5] });
      }
      out.push({ cls: "p0", text: t[4] });
    } else out.push({ cls: "", text: m[4] });
  }
  return out;
}

export function languageOf(file: string): string {
  const ext = file.split(".").pop()?.toLowerCase() ?? "";
  if (["html", "htm", "svg", "xml"].includes(ext)) return "html";
  if (ext === "css") return "css";
  if (["js", "mjs", "cjs", "ts", "tsx", "jsx"].includes(ext)) return "js";
  if (["json", "webmanifest"].includes(ext)) return "json";
  return "text";
}

/** The file as lines, each a list of coloured pieces. A comment or string that runs over several lines is split per line. */
export function highlight(code: string, file: string): Tok[][] {
  const lang = languageOf(file);
  const toks = lang === "html" ? html(code) : lang === "css" ? css(code) : lang === "js" ? js(code) : lang === "json" ? json(code) : [{ cls: "", text: code }];
  const lines: Tok[][] = [[]];
  for (const t of toks) {
    const parts = t.text.split("\n");
    parts.forEach((p, i) => {
      if (i > 0) lines.push([]);
      if (p) lines[lines.length - 1].push({ cls: t.cls, text: p.replace(/\r$/, "") });
    });
  }
  return lines;
}
