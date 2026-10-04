// Builds src-tauri/resources/three.min.js: Three.js (plus OrbitControls) as one classic script that
// defines the global THREE. Generated websites load it from site/vendor/ with a plain <script> tag,
// so it works offline, from disk, and in Jarvis's preview. Run again after upgrading `three`:
//   node scripts/build-three.mjs
import { build } from "rolldown";
import { mkdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const tmp = join(root, "node_modules", ".cache", "three-entry");
mkdirSync(tmp, { recursive: true });
const entry = join(tmp, "entry.js");
writeFileSync(
  entry,
  `import * as T from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
globalThis.THREE = Object.assign({}, T, { OrbitControls });
`,
);
const out = join(root, "src-tauri", "resources");
await build({
  input: entry,
  cwd: root,
  platform: "browser",
  output: { dir: tmp, format: "iife", entryFileNames: "three.min.js", minify: true, banner: "/* Three.js (MIT, threejs.org) with OrbitControls, bundled by Jarvis. Global: THREE */" },
});
mkdirSync(out, { recursive: true });
const { copyFileSync } = await import("node:fs");
copyFileSync(join(tmp, "three.min.js"), join(out, "three.min.js"));
rmSync(tmp, { recursive: true, force: true });
console.log(`three.min.js: ${(statSync(join(out, "three.min.js")).size / 1024).toFixed(0)} KB`);
