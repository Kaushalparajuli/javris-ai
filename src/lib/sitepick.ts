// "Point at a part": the page reports what was clicked; this words it for the person and for the worker.

export interface Picked {
  tag: string;
  text: string;
  path: string;
  section: string;
}

/** How the picked element reads in the chip above the ask bar. */
export function pickedLabel(p: Picked) {
  const what = p.text ? `“${p.text.length > 40 ? `${p.text.slice(0, 40)}…` : p.text}”` : `<${p.tag}>`;
  return p.section && p.section !== p.text ? `${what} in ${p.section}` : what;
}

/** The same thing for the worker, with enough detail to find it in the code. */
export function pickedForWorker(p: Picked) {
  const parts = [`a <${p.tag}> element`];
  if (p.text) parts.push(`with the text “${p.text}”`);
  if (p.section) parts.push(`in the section headed “${p.section}”`);
  if (p.path) parts.push(`(selector path: ${p.path})`);
  return parts.join(" ");
}
