// Runs inside the page the visual director is reviewing. Returns plain facts about the layout,
// so the review rests on measurements as well as on what the screenshots show.
(() => {
  const vw = innerWidth, vh = innerHeight;
  const doc = document.documentElement;
  const sel = (el) => {
    if (!el || !el.tagName) return "";
    if (el.id) return `${el.tagName.toLowerCase()}#${el.id}`;
    const c = (typeof el.className === "string" ? el.className : "").trim().split(/\s+/).filter(Boolean).slice(0, 2).join(".");
    return el.tagName.toLowerCase() + (c ? "." + c : "");
  };
  const visible = (el) => {
    const cs = getComputedStyle(el), r = el.getBoundingClientRect();
    return cs.display !== "none" && cs.visibility !== "hidden" && r.width > 1 && r.height > 1;
  };
  const rect = (el) => {
    const r = el.getBoundingClientRect();
    return { x: Math.round(r.left), y: Math.round(r.top + scrollY), w: Math.round(r.width), h: Math.round(r.height) };
  };
  const all = [...document.body.querySelectorAll("*")].filter((e) => !["SCRIPT", "STYLE", "NOSCRIPT", "LINK", "META", "BR", "PATH", "CIRCLE", "RECT", "G", "DEFS", "LINE", "POLYGON", "POLYLINE", "ELLIPSE", "STOP", "LINEARGRADIENT", "RADIALGRADIENT", "CLIPPATH", "USE", "SYMBOL", "TSPAN"].includes(e.tagName.toUpperCase()));
  const hasText = (el) => [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim().length > 1);
  const out = { viewport: { w: vw, h: vh }, doc: { w: doc.scrollWidth, h: doc.scrollHeight } };

  // Sideways overflow: anything poking out of the viewport.
  out.horizontalOverflow = doc.scrollWidth > vw + 1;
  out.overflowing = all
    .filter((e) => visible(e) && getComputedStyle(e).position !== "fixed")
    .map((e) => ({ e, r: e.getBoundingClientRect() }))
    .filter(({ r }) => r.right > vw + 2 || r.left < -2)
    .filter(({ e }) => !e.closest("[aria-hidden=true], canvas, .marquee, [class*=marquee], [class*=ticker]"))
    .slice(0, 8)
    .map(({ e, r }) => ({ el: sel(e), left: Math.round(r.left), right: Math.round(r.right) }));

  // Text that is too small to read.
  out.smallText = all
    .filter((e) => hasText(e) && visible(e) && parseFloat(getComputedStyle(e).fontSize) < 12)
    .slice(0, 8)
    .map((e) => ({ el: sel(e), px: parseFloat(getComputedStyle(e).fontSize), text: e.textContent.trim().slice(0, 30) }));

  // Contrast of text against the nearest solid background.
  const lum = (c) => {
    const f = (v) => ((v /= 255) <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4));
    return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2]);
  };
  const parse = (s) => {
    const m = s.match(/rgba?\(([^)]+)\)/);
    if (!m) return null;
    const p = m[1].split(",").map((x) => parseFloat(x));
    return { c: p.slice(0, 3), a: p.length > 3 ? p[3] : 1 };
  };
  const bgOf = (el) => {
    for (let n = el; n; n = n.parentElement) {
      const c = parse(getComputedStyle(n).backgroundColor);
      if (c && c.a > 0.9) return c.c;
      if (getComputedStyle(n).backgroundImage !== "none" && n !== document.body && n !== doc) return null;
    }
    return [255, 255, 255];
  };
  out.lowContrast = [];
  for (const e of all) {
    if (out.lowContrast.length >= 8) break;
    if (!hasText(e) || !visible(e)) continue;
    const cs = getComputedStyle(e);
    const fg = parse(cs.color), bg = bgOf(e);
    if (!fg || !bg || parseFloat(cs.opacity) < 0.3) continue;
    const l1 = lum(fg.c), l2 = lum(bg);
    const ratio = (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05);
    const big = parseFloat(cs.fontSize) >= 24 || (parseFloat(cs.fontSize) >= 18.6 && parseInt(cs.fontWeight) >= 700);
    if (ratio < (big ? 3 : 4.5)) out.lowContrast.push({ el: sel(e), ratio: +ratio.toFixed(2), text: e.textContent.trim().slice(0, 30) });
  }

  // Content that is still invisible after scrolling (a reveal animation that never finished).
  out.stuckHidden = all
    .filter((e) => hasText(e) && !e.closest("[aria-hidden=true], header, nav, [role=dialog], dialog") && (parseFloat(getComputedStyle(e).opacity) < 0.05 || getComputedStyle(e).visibility === "hidden") && getComputedStyle(e).display !== "none")
    .slice(0, 8)
    .map((e) => ({ el: sel(e), text: e.textContent.trim().slice(0, 30) }));

  // Text colliding with other text or buttons.
  const blocks = all.filter((e) => /^(H[1-6]|P|A|BUTTON|LI|LABEL|SPAN)$/.test(e.tagName) && hasText(e) && visible(e) && !e.closest("[aria-hidden=true]"));
  out.overlaps = [];
  for (let i = 0; i < blocks.length && out.overlaps.length < 8; i++) {
    const a = blocks[i], ra = a.getBoundingClientRect();
    for (let j = i + 1; j < blocks.length; j++) {
      const b = blocks[j];
      if (a.contains(b) || b.contains(a)) continue;
      const rb = b.getBoundingClientRect();
      const w = Math.min(ra.right, rb.right) - Math.max(ra.left, rb.left), h = Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top);
      if (w > 4 && h > 4 && (w * h) / Math.min(ra.width * ra.height, rb.width * rb.height) > 0.2) {
        out.overlaps.push({ a: sel(a) + " “" + a.textContent.trim().slice(0, 18) + "”", b: sel(b) + " “" + b.textContent.trim().slice(0, 18) + "”", y: Math.round(Math.max(ra.top, rb.top) + scrollY) });
        break;
      }
    }
  }

  // Headlines: how many lines they take, and whether a lone word is left on the last one.
  const lines = (el) => {
    const range = document.createRange();
    range.selectNodeContents(el);
    const rs = [...range.getClientRects()].filter((r) => r.width > 2);
    const tops = [];
    rs.forEach((r) => { if (!tops.some((t) => Math.abs(t - r.top) < r.height * 0.5)) tops.push(r.top); });
    const lastTop = Math.max(...tops);
    const last = rs.filter((r) => Math.abs(r.top - lastTop) < r.height * 0.5);
    const lastW = last.reduce((s, r) => s + r.width, 0);
    return { n: tops.length, lastRatio: lastW / el.getBoundingClientRect().width };
  };
  out.headlines = [...document.querySelectorAll("h1,h2,h3")].filter(visible).slice(0, 14).map((h) => {
    const l = lines(h);
    return { el: sel(h), text: h.textContent.trim().slice(0, 40), lines: l.n, lastLine: +l.lastRatio.toFixed(2), px: Math.round(parseFloat(getComputedStyle(h).fontSize)), w: Math.round(h.getBoundingClientRect().width), h: Math.round(h.getBoundingClientRect().height) };
  });
  out.h1Count = document.querySelectorAll("h1").length;

  // Sections: height, padding, and how much of each is empty.
  out.sections = [...document.querySelectorAll("header,main > *,section,footer")].filter(visible).slice(0, 16).map((s) => {
    const cs = getComputedStyle(s), r = s.getBoundingClientRect();
    const kids = [...s.querySelectorAll("*")].filter((k) => visible(k) && (hasText(k) || /^(IMG|CANVAS|SVG|VIDEO|BUTTON|A)$/i.test(k.tagName)));
    let top = Infinity, bottom = -Infinity;
    kids.forEach((k) => { const kr = k.getBoundingClientRect(); top = Math.min(top, kr.top); bottom = Math.max(bottom, kr.bottom); });
    const used = kids.length ? Math.max(0, bottom - top) : 0;
    return { el: sel(s), y: Math.round(r.top + scrollY), h: Math.round(r.height), padTop: Math.round(parseFloat(cs.paddingTop)), padBottom: Math.round(parseFloat(cs.paddingBottom)), topGap: kids.length ? Math.round(top - r.top) : null, bottomGap: kids.length ? Math.round(r.bottom - bottom) : null, filled: r.height ? +(used / r.height).toFixed(2) : 0 };
  });

  // Left edges of section headings: a tidy page lines them up.
  out.headingLefts = [...new Set([...document.querySelectorAll("h2")].filter(visible).map((h) => Math.round(h.getBoundingClientRect().left)))];
  const cta = [...document.querySelectorAll("a,button")].filter(visible).find((e) => /btn|button|cta|primary/i.test(e.className) && e.getBoundingClientRect().top + scrollY > 60);
  out.firstCtaY = cta ? Math.round(cta.getBoundingClientRect().top + scrollY) : null;

  // Pictures.
  out.images = [...document.images].filter(visible).map((i) => ({ el: sel(i), broken: i.complete && i.naturalWidth === 0, noAlt: !i.hasAttribute("alt"), upscale: i.naturalWidth ? +(i.getBoundingClientRect().width / i.naturalWidth).toFixed(2) : null })).filter((i) => i.broken || i.noAlt || (i.upscale && i.upscale > 1.6)).slice(0, 8);
  out.canvases = [...document.querySelectorAll("canvas")].filter(visible).map((c) => ({ el: sel(c), ...rect(c), blank: (() => { try { const x = document.createElement("canvas"); x.width = 8; x.height = 8; const g = x.getContext("2d"); g.drawImage(c, 0, 0, 8, 8); return !g.getImageData(0, 0, 8, 8).data.some((v, i) => i % 4 === 3 && v > 0); } catch { return null; } })() }));

  // Tap targets (matter on phones).
  out.smallTargets = [...document.querySelectorAll("a,button,input,select,textarea,summary")].filter(visible).filter((e) => { const r = e.getBoundingClientRect(); return r.height < 32 || r.width < 32; }).filter((e) => !(e.tagName === "A" && e.closest("p,li") && e.parentElement.textContent.trim().length > e.textContent.trim().length + 4)).slice(0, 8).map((e) => ({ el: sel(e), w: Math.round(e.getBoundingClientRect().width), h: Math.round(e.getBoundingClientRect().height), text: e.textContent.trim().slice(0, 20) }));
  return out;
})()
