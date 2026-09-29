// "Save as" for exports: asks where to put the file, starting in the folder used last time
// (or Documents the first time). Paths come from the OS dialog, so this works on every platform.

import { dirname, documentDir, join } from "@tauri-apps/api/path";
import { save } from "@tauri-apps/plugin-dialog";

const LAST_DIR = "jarvis.export.lastFolder";

export interface FileKind {
  /** Shown in the dialog, e.g. "Word document". */
  name: string;
  /** Without the dot, e.g. "docx". */
  ext: string;
}

/** The chosen path with the right extension, or null if the dialog was cancelled. */
export async function askSavePath(fileName: string, kind: FileKind): Promise<string | null> {
  let folder = "";
  try {
    folder = localStorage.getItem(LAST_DIR) ?? "";
  } catch {
    /* no remembered folder */
  }
  if (!folder) folder = await documentDir().catch(() => "");
  const chosen = await save({
    title: `Save ${kind.name}`,
    defaultPath: folder ? await join(folder, fileName) : fileName,
    filters: [{ name: kind.name, extensions: [kind.ext] }],
  });
  if (!chosen) return null;
  const path = chosen.toLowerCase().endsWith(`.${kind.ext}`) ? chosen : `${chosen}.${kind.ext}`;
  try {
    localStorage.setItem(LAST_DIR, await dirname(path));
  } catch {
    /* remembered next time instead */
  }
  return path;
}

/** Just the file name, for "Saved …" messages. */
export const fileNameOf = (path: string) => path.split(/[\\/]/).pop() ?? path;
