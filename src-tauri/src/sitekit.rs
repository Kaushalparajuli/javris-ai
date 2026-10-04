//! Extras for the websites Jarvis builds in a project's site folder (see tasks.rs, preview.rs):
//!
//! - **Three.js**: for a 3D request, the site folder gets `vendor/three.min.js`, one classic script
//!   (global `THREE`, with `OrbitControls`) that works offline, from disk and in the preview.
//! - **Versions**: before the code worker changes a site, the site is copied to `.versions/NNN`, so
//!   "go back" is always possible. Dot-folders are never previewed, listed, zipped or copied.
//! - **Point at a part**: a small script the preview server adds to pages shown in the app. It lets the
//!   user click a part of the page, so a change can say exactly which part.
//! - **Change this part** and **zip export**.

use crate::preview;
use crate::routines;
use crate::tasks::{self, Status, Task};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tokio::process::Command;

/// Three.js (with OrbitControls) as one classic script that defines the global THREE.
/// Built by scripts/build-three.mjs from the `three` package.
const THREE_JS: &[u8] = include_bytes!("../resources/three.min.js");
const THREE_FILE: &str = "vendor/three.min.js";
const VERSIONS_DIR: &str = ".versions";
const VERSIONS_KEPT: usize = 15;
/// A site bigger than this isn't copied for versions (a folder of videos, say).
const MAX_SNAPSHOT: u64 = 150 * 1024 * 1024;

// ---------- Three.js ----------

/// Whether a request is for 3D, so Three.js should be put in the site.
pub(crate) fn wants_three(text: &str) -> bool {
    let t = text.to_lowercase();
    ["three.js", "threejs", "three js", "3d", "webgl"].iter().any(|k| t.contains(k))
}

/// Put Three.js in `site/vendor/` if the request (or a brief in the folder) asks for 3D.
/// True when the file is there afterwards, so later changes keep it available.
pub(crate) fn ensure_three(site: &Path, request: &str) -> bool {
    let file = site.join(THREE_FILE);
    if file.is_file() {
        return true;
    }
    let brief_wants = std::fs::read_dir(site)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        .take(4)
        .any(|e| std::fs::read(e.path()).map(|b| wants_three(&String::from_utf8_lossy(&b[..b.len().min(20_000)]))).unwrap_or(false));
    if !wants_three(request) && !brief_wants {
        return false;
    }
    std::fs::create_dir_all(site.join("vendor")).is_ok() && std::fs::write(&file, THREE_JS).is_ok()
}

/// Added to the code worker's prompt when the site has Three.js.
pub(crate) const THREE_RULE: &str = "\nThis site folder already contains vendor/three.min.js: Three.js as one classic script that defines the global THREE (including THREE.OrbitControls). \
Use it for any 3D, WebGL or particle scene instead of a CDN, an import or an import map, and load it with a plain <script src=\"vendor/three.min.js\"></script> before your own script. Don't edit that file. \
Keep all text and navigation in normal HTML so the page is complete without the scene.\n";

// ---------- versions ----------

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteVersion {
    pub n: u32,
    pub label: String,
    pub at: u64,
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<PathBuf>, depth: u32) {
    if depth > 8 {
        return;
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        if p.is_dir() {
            collect(root, &p, out, depth + 1);
        } else if p.strip_prefix(root).is_ok() {
            out.push(p);
        }
    }
}

/// A hash of the site's file names and contents, to tell whether anything changed.
fn fingerprint(dir: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut files = vec![];
    collect(dir, dir, &mut files, 0);
    files.sort();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for f in files {
        f.strip_prefix(dir).unwrap_or(&f).to_string_lossy().hash(&mut h);
        std::fs::read(&f).unwrap_or_default().hash(&mut h);
    }
    h.finish()
}

fn total_size(dir: &Path) -> u64 {
    let mut files = vec![];
    collect(dir, dir, &mut files, 0);
    files.iter().filter_map(|f| f.metadata().ok()).map(|m| m.len()).sum()
}

fn has_page(dir: &Path) -> bool {
    dir.join("index.html").is_file()
        || std::fs::read_dir(dir).into_iter().flatten().flatten().any(|e| e.path().extension().is_some_and(|x| x == "html" || x == "htm"))
}

fn versions_file(site: &Path) -> PathBuf {
    site.join(VERSIONS_DIR).join("versions.json")
}

fn load_versions(site: &Path) -> Vec<SiteVersion> {
    std::fs::read_to_string(versions_file(site)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn version_dir(site: &Path, n: u32) -> PathBuf {
    site.join(VERSIONS_DIR).join(format!("{n:03}"))
}

/// Keep a copy of the site as it is now, unless it is already the newest copy.
fn snapshot(site: &Path, label: &str) {
    if !has_page(site) || total_size(site) > MAX_SNAPSHOT {
        return;
    }
    let mut list = load_versions(site);
    if let Some(last) = list.last() {
        if fingerprint(&version_dir(site, last.n)) == fingerprint(site) {
            return;
        }
    }
    let n = list.last().map(|v| v.n).unwrap_or(0) + 1;
    if routines::copy_dir(site, &version_dir(site, n)).is_err() {
        let _ = std::fs::remove_dir_all(version_dir(site, n));
        return;
    }
    list.push(SiteVersion { n, label: tasks::truncate(label, 80), at: tasks::now_ms() });
    while list.len() > VERSIONS_KEPT {
        let old = list.remove(0);
        let _ = std::fs::remove_dir_all(version_dir(site, old.n));
    }
    if let Ok(json) = serde_json::to_string_pretty(&list) {
        let _ = std::fs::write(versions_file(site), json);
    }
}

/// Called when the code worker is about to change a site: keep how it is now.
pub(crate) fn snapshot_before(site: &Path, request: &str) {
    snapshot(site, &format!("Before: {}", request.split("\n\n").next().unwrap_or(request).trim()));
}

fn busy(app: &AppHandle, site: &Path) -> bool {
    let want = site.canonicalize().unwrap_or_else(|_| site.to_path_buf());
    tasks::snapshot(app).iter().any(|t: &Task| t.kind == "code" && t.status == Status::Running && Path::new(&t.project).canonicalize().is_ok_and(|p| p == want))
}

#[tauri::command]
pub fn site_versions(app: AppHandle, folder: String) -> Result<Vec<SiteVersion>, String> {
    let (dir, _) = preview::project_dir(&app, &folder)?;
    let mut list = load_versions(&dir);
    list.reverse();
    Ok(list)
}

/// Put an earlier version back. The current site is kept as a version first, so nothing is lost.
#[tauri::command]
pub fn restore_site_version(app: AppHandle, folder: String, n: u32) -> Result<(), String> {
    let (dir, _) = preview::project_dir(&app, &folder)?;
    if busy(&app, &dir) {
        return Err("Jarvis is still working on this site. Wait for it to finish first.".into());
    }
    let src = version_dir(&dir, n);
    if !load_versions(&dir).iter().any(|v| v.n == n) || !has_page(&src) {
        return Err("That version isn't there any more.".into());
    }
    snapshot(&dir, &format!("Before going back to version {n}"));
    for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let p = e.path();
        let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
    }
    routines::copy_dir(&src, &dir).map_err(|e| format!("Couldn't restore it: {e}"))
}

// ---------- changing a site, exporting it ----------

/// Have the code worker change the site in `folder`. `region` describes the part of the page the user
/// pointed at, if they did. One change at a time per site.
#[tauri::command]
pub fn site_change(app: AppHandle, folder: String, instructions: String, region: Option<String>, chat_id: Option<String>) -> Result<Task, String> {
    let (dir, _) = preview::project_dir(&app, &folder)?;
    if instructions.trim().is_empty() {
        return Err("Say what to change.".into());
    }
    if busy(&app, &dir) {
        return Err("Jarvis is still working on this site. Wait for it to finish, or stop it first.".into());
    }
    let region = region.unwrap_or_default();
    let pointed = if region.trim().is_empty() { String::new() } else { format!("\n\nThe user pointed at this part of the page: {}.", region.trim()) };
    let request = format!(
        "{}\n\nThis is a change to the existing site in this folder, not a new build: read the files, make precise edits, and keep everything the change doesn't touch exactly as it is. \
         Keep the design consistent: reuse the existing CSS variables, fonts and components. It must still work from 360px to 1600px wide.{pointed}",
        instructions.trim()
    );
    tasks::start_code_task(app, format!("Change: {}", tasks::truncate(&instructions, 50)), request, dir.display().to_string(), None, chat_id, Some("build".into()), Some(vec!["website-design".into()]))
}

/// Zip the site's files to the path chosen in the save dialog.
#[tauri::command]
pub async fn export_site(app: AppHandle, folder: String, path: String) -> Result<String, String> {
    let (dir, _) = preview::project_dir(&app, &folder)?;
    let target = tasks::export_target(&path, "zip")?;
    if !has_page(&dir) {
        return Err("The site has no page yet.".into());
    }
    let name = match tasks::slug(&dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()) {
        s if s.is_empty() => "website".to_string(),
        s => s,
    };
    let work = std::env::temp_dir().join(format!("jarvis-site-{}", tasks::now_ms()));
    let staged = work.join(&name);
    routines::copy_dir(&dir, &staged).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&target);
    let out = if cfg!(target_os = "macos") {
        Command::new("ditto").args(["-c", "-k", "--keepParent"]).arg(&staged).arg(&target).output().await
    } else if cfg!(windows) {
        Command::new("powershell")
            .args(["-NoProfile", "-Command", &format!("Compress-Archive -Path '{}' -DestinationPath '{}' -Force", staged.display(), target.display())])
            .output()
            .await
    } else {
        Command::new("zip").arg("-r").arg(&target).arg(&name).current_dir(&work).output().await
    };
    let _ = std::fs::remove_dir_all(&work);
    match out {
        Ok(o) if o.status.success() && target.is_file() => Ok(target.display().to_string()),
        Ok(o) => Err(format!("Couldn't make the zip: {}", tasks::truncate(&String::from_utf8_lossy(&o.stderr), 160))),
        Err(e) => Err(format!("Couldn't make the zip: {e}")),
    }
}

// ---------- point at a part ----------

/// Added to pages shown in the app (requested with ?jvpick=1). While the app switches it on, hovering
/// outlines what's under the pointer and a click reports what it is; nothing happens otherwise.
const PICK_SCRIPT: &str = r#"<script>(function(){
var picking=false,hl=null,CLS='__jv-hl';
var st=document.createElement('style');st.textContent='.'+CLS+'{outline:2px solid #5cc8ff !important;outline-offset:2px !important;cursor:crosshair !important}';
(document.head||document.documentElement).appendChild(st);
function describe(el){
 var text=(el.innerText||el.alt||el.getAttribute('aria-label')||'').replace(/\s+/g,' ').trim().slice(0,90);
 var path=[],n=el;
 for(var i=0;i<4&&n&&n!==document.body&&n.tagName;i++){
  var s=n.tagName.toLowerCase();
  if(n.id)s+='#'+n.id;else{var c=[].filter.call(n.classList||[],function(x){return x!==CLS});if(c.length)s+='.'+c[0];}
  path.unshift(s);n=n.parentElement;
 }
 var sec=el.closest('section,header,footer,nav,main,article');
 var h=sec&&sec.querySelector('h1,h2,h3');
 return{tag:el.tagName.toLowerCase(),text:text,path:path.join(' > '),section:h?h.innerText.replace(/\s+/g,' ').trim().slice(0,60):''};
}
function clear(){if(hl){hl.classList.remove(CLS);hl=null;}}
document.addEventListener('mouseover',function(e){if(!picking)return;clear();hl=e.target;if(hl.classList)hl.classList.add(CLS);},true);
document.addEventListener('click',function(e){
 if(!picking)return;
 e.preventDefault();e.stopPropagation();clear();
 parent.postMessage({type:'jv-picked',desc:describe(e.target)},'*');
},true);
window.addEventListener('message',function(e){
 if(e.data&&e.data.type==='jv-pick'){picking=!!e.data.on;if(!picking)clear();}
});
})();</script>"#;

/// The page with the pick script added just before `</body>` (or at the end).
pub(crate) fn with_pick_script(html: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(html);
    let at = text.to_ascii_lowercase().rfind("</body>").unwrap_or(text.len());
    let mut out = String::with_capacity(text.len() + PICK_SCRIPT.len());
    out.push_str(&text[..at]);
    out.push_str(PICK_SCRIPT);
    out.push_str(&text[at..]);
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_site(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jarvis-sitekit-{name}-{}", tasks::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn three_js_is_added_only_for_3d_requests_and_stays() {
        assert!(wants_three("a hero with an interactive 3D planet") && wants_three("use Three.js") && wants_three("WebGL background"));
        assert!(!wants_three("a bakery site with a menu and hours"));
        let site = temp_site("three");
        assert!(!ensure_three(&site, "a bakery site") && !site.join(THREE_FILE).exists());
        assert!(ensure_three(&site, "a 3D hero") && site.join(THREE_FILE).is_file());
        assert!(ensure_three(&site, "now change the colours"), "once there, later changes keep it");
        let other = temp_site("three-brief");
        std::fs::write(other.join("brief.md"), "# Site\nThe hero is an interactive 3D globe.").unwrap();
        assert!(ensure_three(&other, "build the site from the brief"), "a brief in the folder can ask for 3D");
        // The embedded file is the classic-script build: a global THREE with the renderer and controls.
        let js = std::str::from_utf8(THREE_JS).unwrap();
        assert!(js.contains("globalThis.THREE") && js.contains("WebGLRenderer") && js.contains("OrbitControls"));
        assert!(!js.trim_start().starts_with("import "), "no module syntax: it has to work in a plain <script>");
        let _ = std::fs::remove_dir_all(&site);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn changes_are_kept_as_versions_and_can_be_restored() {
        let site = temp_site("versions");
        snapshot_before(&site, "empty folders are not kept");
        assert!(load_versions(&site).is_empty());
        std::fs::write(site.join("index.html"), "one").unwrap();
        std::fs::create_dir_all(site.join("css")).unwrap();
        std::fs::write(site.join("css").join("a.css"), "a{}").unwrap();
        snapshot_before(&site, "make the header darker\n\nThis is a change to the existing site");
        snapshot_before(&site, "same files, no new version");
        std::fs::write(site.join("index.html"), "two").unwrap();
        snapshot_before(&site, "add a menu");
        let list = load_versions(&site);
        assert_eq!(list.iter().map(|v| v.n).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(list[0].label, "Before: make the header darker");
        assert_eq!(std::fs::read_to_string(version_dir(&site, 1).join("index.html")).unwrap(), "one");
        assert!(version_dir(&site, 1).join("css").join("a.css").is_file());
        assert!(!version_dir(&site, 2).join(VERSIONS_DIR).exists(), "versions never contain versions");
        assert_eq!(fingerprint(&site), fingerprint(&version_dir(&site, 2)), "same files, same fingerprint");
        for n in 0..20 {
            std::fs::write(site.join("index.html"), format!("v{n}")).unwrap();
            snapshot_before(&site, &format!("change {n}"));
        }
        assert_eq!(load_versions(&site).len(), VERSIONS_KEPT);
        assert!(!version_dir(&site, 1).exists(), "the oldest copies are removed");
        let _ = std::fs::remove_dir_all(&site);
    }

    #[test]
    fn the_pick_script_goes_before_the_closing_body_tag() {
        let out = String::from_utf8(with_pick_script(b"<html><body><h1>Hi</h1></BODY></html>")).unwrap();
        let (script, close) = (out.find("jv-picked").unwrap(), out.find("</BODY>").unwrap());
        assert!(out.starts_with("<html><body><h1>Hi</h1><script>") && script < close && out.ends_with("</BODY></html>"));
        assert!(String::from_utf8(with_pick_script(b"<p>no body tag</p>")).unwrap().ends_with("</script>"));
        assert!(String::from_utf8(with_pick_script("<p>नमस्ते</p></body>".as_bytes())).unwrap().contains("नमस्ते"));
    }
}
