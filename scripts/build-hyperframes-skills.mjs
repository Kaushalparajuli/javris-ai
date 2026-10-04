// Builds src-tauri/resources/hyperframes-skills.tar.gz: HeyGen's HyperFrames agent skills (21 of them:
// the core set, media-use, and the ready-made workflows such as product-launch-video and slideshow) plus
// the component catalog index and the Apache-2.0 licence. Jarvis unpacks it into its private video
// toolbox and hands the relevant skills to the worker that writes a video.
// Usage:  node scripts/build-hyperframes-skills.mjs [tag]     (default tag matches the pinned version)
import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const tag = process.argv[2] ?? "v0.8.112";
const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const work = join(root, "node_modules", ".cache", "hyperframes-skills");
rmSync(work, { recursive: true, force: true });
mkdirSync(work, { recursive: true });

const archive = join(work, "repo.tgz");
console.log(`Downloading ${tag}…`);
execFileSync("curl", ["-fsSL", "-o", archive, `https://github.com/heygen-com/hyperframes/archive/refs/tags/${tag}.tar.gz`]);
const prefix = `hyperframes-${tag.replace(/^v/, "")}`;
execFileSync("tar", ["-xzf", archive, "-C", work, "--strip-components=1", `${prefix}/skills`, `${prefix}/registry/registry.json`, `${prefix}/registry/catalog-artifact`, `${prefix}/LICENSE`]);

const out = join(work, "pack", "hyperframes");
mkdirSync(join(out, "registry"), { recursive: true });
cpSync(join(work, "skills"), join(out, "skills"), { recursive: true });
cpSync(join(work, "registry", "registry.json"), join(out, "registry", "registry.json"));
mkdirSync(join(out, "registry", "catalog-artifact"));
for (const f of readdirSync(join(work, "registry", "catalog-artifact"))) {
  if (!f.endsWith(".bin")) cpSync(join(work, "registry", "catalog-artifact", f), join(out, "registry", "catalog-artifact", f)); // vectors are for search tools Jarvis doesn't run
}
if (existsSync(join(work, "LICENSE"))) cpSync(join(work, "LICENSE"), join(out, "LICENSE"));
writeFileSync(join(out, "SOURCE.txt"), `HeyGen HyperFrames ${tag} (Apache License 2.0), https://github.com/heygen-com/hyperframes\nskills/ and the registry index, unmodified.\n`);
// Test files and anything a worker can't use.
(function prune(dir) {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) prune(p);
    else if (/\.test\.(mjs|js|ts)$/.test(f)) rmSync(p);
  }
})(out);

const target = join(root, "src-tauri", "resources", "hyperframes-skills.tar.gz");
mkdirSync(dirname(target), { recursive: true });
execFileSync("tar", ["-czf", target, "-C", join(work, "pack"), "hyperframes"]);
console.log(`${target}: ${(statSync(target).size / 1048576).toFixed(1)} MB`);
rmSync(work, { recursive: true, force: true });
