//! Making a video from a brief, in stages. The code worker (Codex) only ever writes files: its sandbox
//! can't start a browser, a local server or the network. Jarvis does everything else between stages.
//!
//!   1. PLAN     the worker writes STORYBOARD.md and requests.json (what media it wants)
//!   2. APPROVE  the person reads the storyboard and approves it or asks for changes
//!   3. MEDIA    Jarvis fetches and makes what was asked for: catalog blocks, music, sound effects,
//!               photos, icons, logos, a Gemini voiceover, generated pictures, cut-outs
//!   4. COMPOSE  the worker writes index.html from the storyboard and the finished media
//!   5. CHECK    Jarvis runs HyperFrames' checker; the worker fixes what it finds (up to twice)
//!   6. DRAFT    Jarvis renders a draft MP4; a final render is a separate step
//!
//! Progress goes to the app as `video-stage` events: {folder, stage, message, data}.

use crate::preview;
use crate::settings;
use crate::tasks::{self, Status, Task};
use crate::videokit::{self, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::process::Command;
use tokio::sync::oneshot;

/// HyperFrames skills every video worker gets. The first is Jarvis's own and wins where they differ.
const CORE_SKILLS: [&str; 9] = [
    "video-design",
    "hyperframes",
    "hyperframes-core",
    "hyperframes-animation",
    "hyperframes-creative",
    "hyperframes-audio",
    "hyperframes-keyframes",
    "hyperframes-registry",
    "media-use",
];
/// Ready-made workflows for particular kinds of video; one is added to the core set.
/// ("slideshow" is left out: HyperFrames' slideshow builds a clickable deck, not a video.)
const WORKFLOWS: [&str; 8] = ["product-launch-video", "faceless-explainer", "music-to-video", "embedded-captions", "talking-head-recut", "pr-to-video", "motion-graphics", "general-video"];
const MAX_PLAN_ROUNDS: u32 = 3;
const MAX_FIX_ROUNDS: u32 = 2;
/// Rounds of: the art director watches the draft, the worker polishes, the draft is rendered again.
const MAX_REVIEW_ROUNDS: u32 = 3;

static RUNNING: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static STOPPED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static ANSWERS: Mutex<Option<HashMap<String, oneshot::Sender<Answer>>>> = Mutex::new(None);

#[derive(Debug)]
struct Answer {
    approve: bool,
    feedback: String,
}

fn with_set<R>(set: &Mutex<Option<HashSet<String>>>, f: impl FnOnce(&mut HashSet<String>) -> R) -> R {
    let mut g = set.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(HashSet::new))
}

fn stage(app: &AppHandle, folder: &str, stage: &str, message: impl Into<String>, data: Value) {
    let _ = app.emit("video-stage", json!({ "folder": folder, "stage": stage, "message": message.into(), "data": data }));
}

fn stopped(folder: &str) -> bool {
    with_set(&STOPPED, |s| s.contains(folder))
}

// ---------- requests the worker writes ----------

#[derive(Deserialize, Default, Debug)]
#[serde(default)]
pub struct Requests {
    /// Catalog blocks to install (transitions, overlays, charts, effects).
    pub blocks: Vec<String>,
    pub media: Vec<MediaRequest>,
    /// Pictures to generate. `sheet: true` asks for many objects on a plain background, cut out afterwards.
    pub images: Vec<ImageRequest>,
    pub voiceover: Option<VoiceRequest>,
}

#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct MediaRequest {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub intent: String,
}

#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct ImageRequest {
    pub id: String,
    pub prompt: String,
    pub sheet: bool,
}

#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct VoiceRequest {
    pub text: String,
    pub style: String,
    pub voice: String,
    pub language: String,
}

/// An id a worker may name a file after.
fn clean_id(id: &str) -> Option<String> {
    let id = id.trim();
    (!id.is_empty() && id.len() <= 40 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).then(|| id.to_string())
}

fn clean_block(name: &str) -> Option<String> {
    let n = name.trim();
    (!n.is_empty() && n.len() <= 60 && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')).then(|| n.to_string())
}

fn parse_requests(text: &str) -> Result<Requests, String> {
    let r: Requests = serde_json::from_str(text).map_err(|e| format!("requests.json isn't valid: {e}"))?;
    if r.blocks.len() > 20 || r.media.len() > 30 || r.images.len() > 8 {
        return Err("requests.json asks for too much: at most 20 blocks, 30 media items and 8 generated pictures.".into());
    }
    Ok(r)
}

// ---------- the worker stages ----------

fn skills_for(workflow: &str) -> Vec<String> {
    let mut list: Vec<String> = CORE_SKILLS.iter().map(|s| s.to_string()).collect();
    if WORKFLOWS.contains(&workflow) {
        list.push(workflow.to_string());
    }
    list
}

fn plan_request(title: &str, brief: &str, meta: &videokit::VideoMeta, workflow: &str, feedback: &str) -> String {
    let flow = if WORKFLOWS.contains(&workflow) { format!("\nThis is a \"{workflow}\" video: follow know-how/{workflow}/SKILL.md for how that kind of video is planned and paced.\n") } else { String::new() };
    let again = if feedback.trim().is_empty() { String::new() } else { format!("\nThe person read your storyboard and asked for changes. Update STORYBOARD.md and requests.json for them, keeping what they didn't mention:\n\"{}\"\n", feedback.trim()) };
    format!(
        "Stage 1 of 3: PLAN. Do not write index.html yet.\n\
         The video is called \"{title}\" ({format}, {w}x{h}, {secs} seconds; see video.json). What the person asked for:\n\"{brief}\"\n{flow}{again}\n\
         This is a motion piece, not a slideshow: read sections 1 and 4 of know-how/video-design/SKILL.md before you plan.\n\
         Write two files in this folder:\n\
         1. STORYBOARD.md: the goal and viewer, a `Motion: high` line (or `Motion: calm` only if the person asked for calm or minimal), the design direction (palette, two fonts) \
            the motion language (primary transition, accents, signature move), the spine (the one element that runs through and becomes each scene) and the spectacle beat (the one exaggerated moment, with its time), then each scene with its start time, length, the one idea, what the viewer sees, the frame (how the canvas is composed; no two neighbouring scenes alike), \
            the on-screen words if any (many scenes need none), the shot (a blueprint from know-how/hyperframes-animation/blueprints-index.md, or custom), its signature move, the camera move, the transition out, \
            the narration line, and which assets it uses. Scene lengths follow the narration and vary; they add up to {secs} seconds. Then the full narration script if there is a voiceover.\n\
         2. requests.json: everything Jarvis must fetch or make for you before you compose, as JSON:\n\
         {{\n  \"blocks\": [\"catalog-block-name\"],\n  \"media\": [ {{\"id\": \"music\", \"type\": \"bgm|sfx|image|icon|logo\", \"intent\": \"what you need, in plain words\"}} ],\n  \"images\": [ {{\"id\": \"sheet1\", \"prompt\": \"…\", \"sheet\": true}} ],\n  \"voiceover\": {{\"text\": \"the whole narration\", \"style\": \"warm, friendly\", \"language\": \"en\"}}\n}}\n\
         Leave out any key you don't need (a silent video has no voiceover). `blocks` are names from the HyperFrames catalog (see know-how/hyperframes-registry/SKILL.md and registry-index.json if present): \
         ask for overlays and self-contained moments (grain, light leaks, a chart, a logo outro), not for the transitions between your scenes, which you build yourself; \
         `sfx` intents can ask for whooshes and hits for your big transitions and slams; \
         `logo` intents are a domain such as example.com; `images` are for pictures that must be made (a product, an illustration, a sheet of separate objects on a plain background to be cut out). \
         Ask only for what the storyboard really uses, and keep each intent specific. Photos, music and sound effects come from HeyGen's catalog when the person has connected it.\n\
         Speech in files the person gave you (a talking video, an interview) is transcribed by Jarvis after you finish this stage, into assets/<file>.words.json with the exact time of every word. \
         So plan highlights and on-screen moments around that: do NOT wait for a transcript and do not ask for one in requests.json. \
         Don't plan captions or subtitles unless the person asked for them.",
        format = meta.format,
        w = meta.width,
        h = meta.height,
        secs = meta.seconds
    )
}

/// The video must run at least as long as its narration, plus a breath at each end, and not much longer:
/// a narration that ends well before the planned length would leave the last card held for many seconds,
/// so the video is cut to the narration plus about three seconds for the call to action.
/// Returns the (possibly changed) video and a sentence for the composer when it was changed.
fn fit_length(dir: &Path, meta: &videokit::VideoMeta, items: &[Item]) -> (videokit::VideoMeta, String) {
    let Some(voice) = items.iter().find(|i| i.ok && i.kind == "voiceover").and_then(|i| i.duration) else { return (meta.clone(), String::new()) };
    let longest = ((voice + 1.2).ceil() as u32).min(180);
    let shortest = ((voice + 3.0).ceil() as u32).clamp(5, 180);
    let need = if longest > meta.seconds {
        longest
    } else if meta.seconds > shortest + 1 {
        shortest
    } else {
        return (meta.clone(), String::new());
    };
    let mut longer = meta.clone();
    longer.seconds = need;
    if let Ok(json) = serde_json::to_string_pretty(&longer) {
        let _ = std::fs::write(dir.join("video.json"), json);
    }
    // The starting file still has the old length on its root element.
    if let Ok(html) = std::fs::read_to_string(dir.join("index.html")) {
        let old = format!("data-start=\"0\" data-duration=\"{}\" data-width", meta.seconds);
        if html.contains(&old) {
            let _ = std::fs::write(dir.join("index.html"), html.replacen(&old, &format!("data-start=\"0\" data-duration=\"{need}\" data-width"), 1));
        }
    }
    let (than, fit) = if need > meta.seconds { ("longer", "Stretch") } else { ("shorter", "Shrink") };
    let note = format!(
        "The finished narration runs {voice:.1} seconds, {than} than the {} seconds planned, so the video is now {need} seconds long (video.json and the root data-duration say so). \
         {fit} the scene timings in the storyboard to fit the narration, keeping their order and proportions, and keep each scene's key moments in step with the voice.",
        meta.seconds
    );
    (longer, note)
}

fn compose_request(meta: &videokit::VideoMeta, manifest_note: &str, length_note: &str) -> String {
    format!(
        "Stage 2 of 3: COMPOSE. The storyboard in STORYBOARD.md was approved. Write the video now: index.html (plus files under compositions/ if you use sub-compositions).\n\
         Canvas {w}x{h}, {secs} seconds (video.json). Start from know-how/video-design/references/video-skeleton.html.\n\
         Jarvis has fetched and made the media. assets/manifest.json lists every file with its id, kind, duration and credit; use those files exactly as they are, by relative path. \
         {manifest_note} {length_note}\n\
         If a requested item failed (ok: false), design without it instead of inventing a file. Catalog blocks you asked for are installed under compositions/ (see \"blocks\" in the manifest for each one's usage snippet).\n\
         If assets/voiceover.words.json exists, land each scene's key words and hits on their spoken times. Add no captions or subtitles unless the person asked for them. Mix any music quietly under the voice.\n\
         Design every scene on its own from what it shows (section 4a of know-how/video-design/SKILL.md): never one layout refilled scene after scene. \
         Make it move: follow section 4 (a camera in every scene, a living background, kinetic type, graphics that draw and count, real transitions). After you finish, Jarvis measures the motion and the scene layouts in your page and sends back a video that plays like a slideshow.\n\
         When you are done run ./hf lint and fix every error. Do not render.",
        w = meta.width,
        h = meta.height,
        secs = meta.seconds
    )
}

fn fix_request(problems: &[String]) -> String {
    format!(
        "Jarvis ran HyperFrames' checker on index.html and it found these problems. Fix every one at its cause, keep the design and timing otherwise as they are, then run ./hf lint:\n{}\n\
         Errors (marked ✗) must be fixed; ignore warnings called nested_structure_needs_subcomposition.\n\
         A contrast error far below the target (around 1.2 to 1.6:1) on text whose colours look fine almost always means something is covering that text at that second \
         (an overlay or box stretched over the frame, a transition panel, a scene that should already be hidden) or the text is caught mid-fade. \
         Find what is on top at the time it names and fix that; recolouring the text won't help.",
        problems.iter().map(|p| format!("- {p}")).collect::<Vec<_>>().join("\n")
    )
}

// ---------- the motion measure ----------

/// What a tween can animate that counts as movement.
const MOTION_PROPS: [&str; 20] = [
    "opacity", "autoAlpha", "x", "y", "xPercent", "yPercent", "scale", "scaleX", "scaleY", "rotation", "rotationX", "rotationY", "skewX", "skewY", "clipPath", "filter",
    "strokeDashoffset", "backgroundPosition", "color", "motionPath",
];
/// The expressive ones, beyond fading, sliding and scaling: what separates motion design from a slideshow.
const RICH_PROPS: [&str; 13] = ["xPercent", "yPercent", "rotation", "rotationX", "rotationY", "skewX", "skewY", "clipPath", "filter", "strokeDashoffset", "backgroundPosition", "color", "motionPath"];

#[derive(Debug, Default, PartialEq)]
struct Motion {
    tweens: usize,
    /// Distinct properties the tweens animate.
    props: Vec<String>,
    /// Tweens that bring something in from hidden (their start state has an opacity).
    entrances: usize,
    /// Entrances that are only a fade and a slide (opacity with x/y): the slideshow move.
    fade_slides: usize,
}

/// The text of the page's inline scripts.
fn inline_scripts(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(open) = rest.find("<script") {
        let after = &rest[open..];
        let Some(tag_end) = after.find('>') else { break };
        let body = &after[tag_end + 1..];
        let Some(close) = body.find("</script>") else { break };
        if !after[..tag_end].contains("src=") {
            out.push_str(&body[..close]);
            out.push('\n');
        }
        rest = &body[close..];
    }
    out
}

/// The argument text of a call whose "(" is at `open`: up to the matching ")", skipping strings.
fn call_args(src: &str, open: usize) -> &str {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut prev = ' ';
    for (i, c) in src[open..].char_indices() {
        match quote {
            Some(q) => {
                if c == q && prev != '\\' {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' | '`' => quote = Some(c),
                '(' | '{' | '[' => depth += 1,
                ')' | '}' | ']' => {
                    depth -= 1;
                    if depth == 0 {
                        return &src[open + 1..open + i];
                    }
                }
                _ => {}
            },
        }
        prev = c;
    }
    &src[open + 1..]
}

/// Keys of the object literals in `text` (at any depth): a name or quoted name right after `{` or `,`, followed by `:`.
fn object_keys(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut keys = vec![];
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '{' || c == ',' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            let quoted = j < chars.len() && (chars[j] == '"' || chars[j] == '\'');
            let start = if quoted { j + 1 } else { j };
            let mut k = start;
            while k < chars.len() && (chars[k].is_alphanumeric() || chars[k] == '_' || chars[k] == '$') {
                k += 1;
            }
            let mut end = k;
            if quoted && end < chars.len() && (chars[end] == '"' || chars[end] == '\'') {
                end += 1;
            }
            while end < chars.len() && chars[end].is_whitespace() {
                end += 1;
            }
            if k > start && end < chars.len() && chars[end] == ':' {
                keys.push(chars[start..k].iter().collect());
            }
        }
        i += 1;
    }
    keys
}

/// The first object literal in a tween's arguments (its start state for fromTo and from).
fn first_object(args: &str) -> &str {
    match args.find('{') {
        Some(open) => {
            let inner = call_args(args, open);
            let end = (open + 1 + inner.len() + 1).min(args.len());
            &args[open..end]
        }
        None => "",
    }
}

fn measure_motion(html: &str) -> Motion {
    let src = inline_scripts(html);
    let mut m = Motion::default();
    let mut props = std::collections::BTreeSet::new();
    for (pat, starts_hidden) in [(".fromTo(", true), (".from(", true), (".to(", false)] {
        let mut from = 0;
        while let Some(at) = src[from..].find(pat) {
            let open = from + at + pat.len() - 1;
            let args = call_args(&src, open);
            m.tweens += 1;
            for k in object_keys(args) {
                if MOTION_PROPS.contains(&k.as_str()) {
                    props.insert(k);
                }
            }
            if starts_hidden {
                let start = object_keys(first_object(args));
                if start.iter().any(|k| k == "opacity" || k == "autoAlpha") {
                    m.entrances += 1;
                    if start.iter().all(|k| ["opacity", "autoAlpha", "x", "y"].contains(&k.as_str())) {
                        m.fade_slides += 1;
                    }
                }
            }
            from = open + 1;
        }
    }
    m.props = props.into_iter().collect();
    m
}

/// A storyboard that asked for a calm video: a `Motion: calm` line, however it is formatted.
fn is_calm(board: &str) -> bool {
    board.lines().any(|l| {
        let l: String = l.to_lowercase().chars().filter(|c| !matches!(c, '*' | '_' | '`')).collect();
        let l = l.trim_start_matches(|c: char| c == '-' || c == '#' || c == '|' || c.is_whitespace());
        l.starts_with("motion: calm") || l.starts_with("motion level: calm")
    })
}

/// Why the composition still plays like a slideshow, in sentences for the worker. Empty when it moves enough.
fn motion_problems(html: &str, seconds: u32, calm: bool) -> Vec<String> {
    let m = measure_motion(html);
    let secs = seconds.max(1) as f64;
    let (per_second, min_props, min_rich) = if calm { (0.5, 4, 1) } else { (1.0, 6, 3) };
    let rich = m.props.iter().filter(|p| RICH_PROPS.contains(&p.as_str())).count();
    let mut out = vec![];
    if (m.tweens as f64) < per_second * secs {
        out.push(format!(
            "There are only {} tweens for a {seconds}-second video; aim for at least {}. Give every scene a moving camera, a living background and a new beat every second or so.",
            m.tweens,
            (per_second * secs * if calm { 1.0 } else { 1.5 }).ceil()
        ));
    }
    if m.props.len() < min_props || rich < min_rich {
        out.push(format!(
            "The timeline only animates {}. Use at least {min_props} different properties, {min_rich} or more of them beyond fades, slides and scale: rotation and skew on kinetic type, \
             clipPath and mask reveals (yPercent in an overflow-hidden wrapper), strokeDashoffset for lines that draw, filter for blur snaps.",
            if m.props.is_empty() { "nothing".to_string() } else { m.props.join(", ") }
        ));
    }
    if !calm && m.entrances >= 4 && m.fade_slides * 10 > m.entrances * 5 {
        out.push(format!(
            "{} of the {} entrances are the same fade-and-slide (opacity with x or y). Keep at most half that way; land the rest with slams (scale), mask rises (yPercent in an overflow-hidden wrapper), skews, clip-path wipes and letter cascades.",
            m.fade_slides, m.entrances
        ));
    }
    out
}

/// The page without the contents of its `<script>` and `<style>` blocks, so only markup is left.
fn markup_only(html: &str) -> String {
    let mut out = html.to_string();
    for tag in ["script", "style"] {
        let mut kept = String::with_capacity(out.len());
        let mut rest = out.as_str();
        while let Some(open) = rest.to_ascii_lowercase().find(&format!("<{tag}")) {
            kept.push_str(&rest[..open]);
            let close = format!("</{tag}>");
            match rest[open..].to_ascii_lowercase().find(&close) {
                Some(c) => rest = &rest[open + c + close.len()..],
                None => {
                    rest = "";
                    break;
                }
            }
        }
        kept.push_str(rest);
        out = kept;
    }
    out
}

/// Each opening tag in markup: (byte offset just past it, tag name, class attribute).
fn open_tags(markup: &str) -> Vec<(usize, usize, String, String)> {
    let mut out = vec![];
    let bytes = markup.as_bytes();
    let mut i = 0;
    while let Some(lt) = markup[i..].find('<') {
        let start = i + lt;
        let Some(gt) = markup[start..].find('>') else { break };
        let end = start + gt + 1;
        let tag = &markup[start + 1..end - 1];
        let name: String = tag.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>().to_ascii_lowercase();
        if !name.is_empty() && bytes.get(start + 1).is_some_and(|b| b.is_ascii_alphabetic()) {
            let class = ["class=\"", "class='"]
                .iter()
                .find_map(|p| tag.find(p).map(|at| (at + p.len(), p.chars().last().unwrap_or('"'))))
                .and_then(|(from, q)| tag[from..].find(q).map(|len| tag[from..from + len].to_string()))
                .unwrap_or_default();
            out.push((start, end, name, class));
        }
        i = end;
    }
    out
}

/// Each scene's structure: the tag and first class of the first four elements inside it. A scene is a
/// clip that is a `<section>` or has the class `scene`.
fn scene_signatures(html: &str) -> Vec<String> {
    let markup = markup_only(html);
    let tags = open_tags(&markup);
    let mut out = vec![];
    for (n, (_, end, name, class)) in tags.iter().enumerate() {
        let classes: Vec<&str> = class.split_whitespace().collect();
        if !classes.contains(&"clip") || !(name == "section" || classes.contains(&"scene")) {
            continue;
        }
        // Where this element closes: count same-named tags opening and closing after it.
        let mut depth = 1;
        let mut close = markup.len();
        let mut at = *end;
        while let Some(lt) = markup[at..].find('<') {
            let p = at + lt;
            let rest = markup[p + 1..].to_ascii_lowercase();
            if rest.starts_with(&format!("/{name}")) {
                depth -= 1;
                if depth == 0 {
                    close = p;
                    break;
                }
            } else if rest.starts_with(name.as_str()) && rest[name.len()..].starts_with(|c: char| c == ' ' || c == '>') {
                depth += 1;
            }
            at = p + 1;
        }
        // Repeated neighbours (a row of decorative dots) count once, so a shared backdrop isn't the whole signature.
        let mut inside: Vec<String> = vec![];
        for (_, _, tag, class) in tags[n + 1..].iter().take_while(|(start, ..)| *start < close) {
            let step = match class.split_whitespace().next() {
                Some(c) => format!("{tag}.{c}"),
                None => tag.clone(),
            };
            if inside.last() != Some(&step) {
                inside.push(step);
            }
            if inside.len() == 6 {
                break;
            }
        }
        out.push(inside.join(" > "));
    }
    out
}

/// Words glued to a span in the markup ("Pick<span>one" shows as "Pickone"), as the words that run together.
fn glued_words(html: &str) -> Vec<String> {
    let m = markup_only(html);
    let b = m.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric();
    let before = |i: usize| -> String { m[..i].chars().rev().take_while(|c| c.is_alphanumeric()).collect::<Vec<_>>().into_iter().rev().collect() };
    let after = |i: usize| -> String { m[i..].chars().take_while(|c| c.is_alphanumeric()).collect() };
    let mut out: Vec<String> = vec![];
    for (i, _) in m.match_indices("<span") {
        let Some(gt) = m[i..].find('>') else { continue };
        let inner = i + gt + 1;
        if i > 0 && word(b[i - 1]) && inner < b.len() && word(b[inner]) {
            out.push(format!("{}{}", before(i), after(inner)));
        }
    }
    for (i, _) in m.match_indices("</span>") {
        let next = i + "</span>".len();
        if i > 0 && word(b[i - 1]) && next < b.len() && word(b[next]) {
            out.push(format!("{}{}", before(i), after(next)));
        }
    }
    out.dedup();
    out.truncate(4);
    out
}

/// Scenes built by refilling one layout, and words glued together, in sentences for the worker. Empty when all is well.
fn layout_problems(html: &str) -> Vec<String> {
    let glued = glued_words(html);
    let mut out = if glued.is_empty() {
        vec![]
    } else {
        vec![format!(
            "Words are glued together where a span meets text, so they show without a space: {}. Keep a space outside each word span (Pick <span>one real task</span> this week) or give the spans a right margin.",
            glued.iter().map(|g| format!("\"{g}\"")).collect::<Vec<_>>().join(", ")
        )]
    };
    out.extend(same_layout_problems(html));
    out
}

fn same_layout_problems(html: &str) -> Vec<String> {
    let sigs = scene_signatures(html);
    if sigs.len() < 4 {
        return vec![];
    }
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for s in sigs.iter().filter(|s| !s.is_empty()) {
        *counts.entry(s.as_str()).or_default() += 1;
    }
    match counts.into_iter().max_by_key(|(_, n)| *n) {
        Some((sig, n)) if n * 2 > sigs.len() => vec![format!(
            "{n} of the {} scenes are built from the same layout ({sig}): one design refilled with new words, which is what makes it look like slides. \
             Design each scene on its own from what it shows (section 4a): a different frame every time, words only where they earn it, no eyebrow-headline-subline block repeated.",
            sigs.len()
        )],
        _ => vec![],
    }
}

fn rework_request(problems: &[String]) -> String {
    format!(
        "Jarvis looked at index.html and it still plays like a slideshow:\n{}\n\
         Rework it following sections 4 and 4a of know-how/video-design/SKILL.md (know-how/video-design/references/video-skeleton.html has working code for each technique): \
         every scene designed on its own with a different frame, a camera that moves in every scene, varied entrances, lines that draw and numbers that count where they fit, \
         and real transitions where the outgoing and incoming scenes move together. You may rebuild scenes from scratch. \
         Keep the storyboard's ideas, scene order, the timing against the voiceover and the media as they are. Then run ./hf lint and fix every error.",
        problems.iter().map(|p| format!("- {p}")).collect::<Vec<_>>().join("\n")
    )
}

/// Run one worker stage and wait for it.
async fn run_stage(app: &AppHandle, folder: &str, title: &str, request: String, workflow: &str, chat: &str) -> Result<Task, String> {
    let task = tasks::start_code_task(app.clone(), title.to_string(), request, folder.to_string(), None, Some(chat.to_string()), Some("video".into()), Some(skills_for(workflow)))?;
    stage(app, folder, "task", title.to_string(), json!({ "taskId": task.id }));
    loop {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        if stopped(folder) {
            let _ = tasks::cancel_task(tauri::Manager::state::<tasks::TaskStore>(app), task.id);
            return Err("Stopped.".into());
        }
        let Some(t) = tasks::get_task(app, task.id) else { return Err("The worker's task disappeared.".into()) };
        match t.status {
            Status::Running => continue,
            Status::Done => return Ok(t),
            Status::Cancelled => return Err("Stopped.".into()),
            Status::Failed => return Err(if t.error.is_empty() { "The worker stopped without finishing.".into() } else { t.error }),
        }
    }
}

// ---------- the media stage ----------

#[derive(Serialize, Clone, Default)]
#[serde(default)]
struct Item {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    ok: bool,
    file: String,
    duration: Option<f64>,
    note: String,
    credit: String,
}

/// Word timings spread over the narration by length, for captions when no transcription model is
/// installed: each word gets time in proportion to its letters, with a short pause after punctuation.
fn estimate_words(text: &str, total: f64) -> Vec<Value> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() || total <= 0.0 {
        return vec![];
    }
    let weight = |w: &str| w.chars().filter(|c| c.is_alphanumeric()).count().max(1) as f64 + if w.ends_with(['.', '!', '?']) { 3.0 } else if w.ends_with([',', ';', ':']) { 1.5 } else { 0.0 };
    let sum: f64 = words.iter().map(|w| weight(w)).sum();
    let mut t = 0.0f64;
    words
        .iter()
        .map(|w| {
            let len = total * weight(w) / sum;
            let spoken = len * if weight(w) > w.chars().filter(|c| c.is_alphanumeric()).count().max(1) as f64 { 0.8 } else { 1.0 };
            let v = json!({ "text": w, "start": (t * 100.0).round() / 100.0, "end": ((t + spoken) * 100.0).round() / 100.0 });
            t += len;
            v
        })
        .collect()
}

/// Heard words with the script's own spelling: when both have the same number of words, the script's text
/// (names spelled right) gets the heard timings; otherwise the heard words are used as they are.
fn align_words(script: &str, heard: Vec<Value>) -> Vec<Value> {
    let words: Vec<&str> = script.split_whitespace().collect();
    if words.len() != heard.len() {
        return heard;
    }
    words.iter().zip(heard).map(|(w, h)| json!({ "text": w, "start": h["start"], "end": h["end"] })).collect()
}

async fn hf_json(rt: &Runtime, dir: &Path, args: &[&str]) -> Result<Value, String> {
    let (ok, text) = videokit::run_hf_public(rt, dir, args).await?;
    let line = text.lines().rev().find(|l| l.trim_start().starts_with('{')).unwrap_or("");
    let v: Value = serde_json::from_str(line.trim()).map_err(|_| tasks::truncate(&text, 200))?;
    if ok && v["ok"] != json!(false) {
        Ok(v)
    } else {
        Err(v["error"].as_str().map(String::from).unwrap_or_else(|| tasks::truncate(&text, 200)))
    }
}

async fn resolve_media(rt: &Runtime, dir: &Path, m: &MediaRequest) -> Item {
    let mut item = Item { id: m.id.clone(), kind: m.kind.clone(), ..Default::default() };
    if !["bgm", "sfx", "image", "icon", "logo"].contains(&m.kind.as_str()) {
        item.note = format!("Jarvis can't fetch \"{}\".", m.kind);
        return item;
    }
    match hf_json(rt, dir, &["media-use", "resolve", "--type", &m.kind, "--intent", &m.intent, "--project", ".", "--json"]).await {
        Ok(v) => {
            let from = v["path"].as_str().unwrap_or("");
            let ext = Path::new(from).extension().and_then(|e| e.to_str()).unwrap_or("bin");
            let to = format!("assets/{}.{ext}", m.id);
            if !from.is_empty() && std::fs::copy(dir.join(from), dir.join(&to)).is_ok() {
                item.ok = true;
                item.file = to;
                item.duration = v["duration"].as_f64();
                item.credit = v["provenance"]["provider"].as_str().map(String::from).unwrap_or_default();
                item.note = v["description"].as_str().unwrap_or("").to_string();
            } else {
                item.note = "The file wasn't saved.".into();
            }
        }
        Err(e) => item.note = e,
    }
    item
}

/// A generated picture: a worker makes it with Codex's image generation, then it is copied into assets/.
async fn generate_image(app: &AppHandle, dir: &Path, meta: &videokit::VideoMeta, req: &ImageRequest, chat: &str) -> Vec<Item> {
    let mut item = Item { id: req.id.clone(), kind: "image".into(), ..Default::default() };
    let aspect = match meta.format.as_str() {
        "portrait" => "tall",
        "square" => "square",
        _ => "wide",
    };
    let prompt = if req.sheet {
        format!("{}\n\nLay out each object separately on one sheet, with generous empty space between every object so none touch or overlap, on a perfectly flat, plain pure white background with no shadows, no floor, no gradient and no text.", req.prompt.trim())
    } else {
        req.prompt.trim().to_string()
    };
    let started = tasks::start_image(app.clone(), format!("Picture for the video: {}", req.id), prompt, Some(1), Some(if req.sheet { "square".into() } else { aspect.into() }), None, None, Some(chat.to_string()));
    let task = match started {
        Ok(t) => t,
        Err(e) => {
            item.note = e;
            return vec![item];
        }
    };
    let done = loop {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        match tasks::get_task(app, task.id) {
            Some(t) if t.status == Status::Running => continue,
            Some(t) => break t,
            None => break task,
        }
    };
    let Some(src) = done.images.first().filter(|_| done.status == Status::Done) else {
        item.note = if done.error.is_empty() { "No picture was made.".into() } else { done.error };
        return vec![item];
    };
    let to = format!("assets/{}.png", req.id);
    if std::fs::copy(src, dir.join(&to)).is_err() {
        item.note = "The picture wasn't saved.".into();
        return vec![item];
    }
    item.ok = true;
    item.file = to.clone();
    item.credit = "generated".into();
    if !req.sheet {
        return vec![item];
    }
    // A sheet of separate objects on white: cut each one out into its own transparent picture.
    match crate::cutout::split_sheet(&dir.join(&to), &dir.join("assets"), &req.id) {
        Ok(parts) if !parts.is_empty() => {
            let mut out = vec![item];
            for (i, p) in parts.iter().enumerate() {
                out.push(Item { id: format!("{}-{}", req.id, i + 1), kind: "cutout".into(), ok: true, file: format!("assets/{p}"), credit: "generated, cut out".into(), note: format!("object {} of {}", i + 1, parts.len()), ..Default::default() });
            }
            out
        }
        Ok(_) => {
            item.note = "No separate objects were found on the sheet.".into();
            vec![item]
        }
        Err(e) => {
            item.note = e;
            vec![item]
        }
    }
}

/// A Kokoro voice for the requested delivery: a male voice when the style asks for one, a warm female voice otherwise.
fn kokoro_voice(style: &str) -> &'static str {
    let s = style.to_lowercase();
    if s.split(|c: char| !c.is_alphanumeric()).any(|w| w == "male" || w == "man" || w == "masculine") {
        "am_michael"
    } else {
        "af_heart"
    }
}

/// Why the audio engine made no voice: the reason it lists for the omitted line ("TTS failed — omitted (…)"), or
/// the last thing it printed.
fn engine_failure(stdout: &str, stderr: &str) -> String {
    let all = format!("{stdout}\n{stderr}");
    if let Some(line) = all.lines().find(|l| l.contains("omitted")) {
        let why = line.split_once("omitted (").map(|(_, r)| r.trim_end_matches(')')).unwrap_or(line.trim());
        // A JSON error from the provider: keep its message.
        let why = why.split("\"message\":\"").nth(1).and_then(|m| m.split('"').next()).unwrap_or(why);
        return tasks::truncate(why.trim(), 160);
    }
    tasks::truncate(stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no voice came back").trim(), 160)
}

/// Run HyperFrames' audio engine for one request. Returns the voice file (relative to `dir`), its length and its
/// metadata, or why it wasn't made.
async fn run_audio_engine(rt: &Runtime, dir: &Path, engine: &Path, request: &Value, gemini_key: Option<&str>) -> Result<(String, f64, Value), String> {
    let _ = std::fs::write(dir.join("audio_request.json"), request.to_string());
    let _ = std::fs::remove_file(dir.join("audio_meta.json"));
    let mut cmd = Command::new(rt.node_path());
    cmd.arg(engine).args(["--request", "audio_request.json", "--out", "audio_meta.json"]).current_dir(dir).env_clear().envs(rt.env_pairs());
    if let Some(key) = gemini_key {
        cmd.env("GEMINI_API_KEY", key);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    // Kokoro may fetch its model the first time it speaks.
    let out = match tokio::time::timeout(Duration::from_secs(600), cmd.output()).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return Err(format!("the audio engine didn't start: {e}")),
        Err(_) => return Err("it took too long".into()),
    };
    let _ = std::fs::remove_file(dir.join("audio_request.json"));
    let meta: Value = std::fs::read_to_string(dir.join("audio_meta.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
    let _ = std::fs::remove_file(dir.join("audio_meta.json"));
    let voice = meta["voices"][0].clone();
    match (voice["path"].as_str(), voice["duration_s"].as_f64()) {
        (Some(path), Some(secs)) => Ok((path.to_string(), secs, voice)),
        _ => Err(engine_failure(&String::from_utf8_lossy(&out.stdout), &String::from_utf8_lossy(&out.stderr))),
    }
}

async fn make_voiceover(app: &AppHandle, rt: &Runtime, dir: &Path, v: &VoiceRequest) -> Item {
    let mut item = Item { id: "voiceover".into(), kind: "voiceover".into(), ..Default::default() };
    let settings = settings::load(app);
    let mut fell_back = String::new();
    match crate::voices::speak(app, v.text.trim()).await {
        Some(Ok(sp)) => {
            let to = format!("assets/voiceover.{}", sp.ext);
            if std::fs::write(dir.join(&to), &sp.audio).is_err() {
                item.note = "The voiceover wasn't saved.".into();
                return item;
            }
            // The length is where the last word ends, plus the breath after it.
            let secs = sp.words.last().and_then(|w| w["end"].as_f64()).map(|e| e + 0.2).unwrap_or_else(|| (v.text.split_whitespace().count() as f64 * 0.4).max(1.0));
            let words = if sp.words.is_empty() { estimate_words(&v.text, secs) } else { sp.words };
            let _ = std::fs::write(dir.join("assets/voiceover.words.json"), serde_json::to_string_pretty(&json!({ "duration": secs, "estimated": false, "words": words })).unwrap_or_default());
            item.ok = true;
            item.file = to;
            item.duration = Some(secs);
            item.credit = "ElevenLabs speech".into();
            item.note = "Word timings are in assets/voiceover.words.json.".into();
            return item;
        }
        Some(Err(e)) => fell_back = format!("{e} Another voice was used instead. "),
        None => {}
    }
    let engine = rt.skills().join("media-use/audio/scripts/audio.mjs");
    if !engine.is_file() {
        item.note = "The audio engine isn't unpacked.".into();
        return item;
    }
    let key = settings.gemini_api_key.trim().to_string();
    let lang = if v.language.trim().is_empty() { "en" } else { v.language.trim() };
    let style = if v.style.trim().is_empty() { "warm, clear, unhurried" } else { v.style.trim() };
    // Gemini's voice first, then Gemini's lite voice (a separate daily quota), then HyperFrames' local Kokoro
    // voice when there is no key or Gemini can't speak (quota, outage). (provider, voice, TTS model)
    let mut attempts: Vec<(&str, String, Option<&str>)> = vec![];
    if !key.is_empty() {
        let voice = if !v.voice.trim().is_empty() { v.voice.trim() } else if settings.narration_gemini_voice.trim().is_empty() { "Kore" } else { settings.narration_gemini_voice.trim() };
        attempts.push(("gemini", voice.to_string(), None));
        attempts.push(("gemini", voice.to_string(), Some("gemini-3.8-flash-lite-tts")));
    }
    if lang.starts_with("en") {
        attempts.push(("kokoro", kokoro_voice(style).to_string(), None));
    }
    if attempts.is_empty() {
        item.note = "No voice is set up (no Gemini key and no ElevenLabs key), so there is no voiceover.".into();
        return item;
    }
    let mut failures: Vec<String> = vec![];
    let mut made: Option<(String, f64, Value, &str)> = None;
    for (provider, voice, model) in &attempts {
        let mut request = json!({
            "provider": provider,
            "voice": voice,
            "lang": lang,
            "style": style,
            "lines": [{ "id": "vo", "text": v.text.trim() }],
            "bgm": { "mode": "none" }
        });
        if let Some(model) = model {
            request["tts_model"] = json!(model);
        }
        match run_audio_engine(rt, dir, &engine, &request, (*provider == "gemini").then_some(key.as_str())).await {
            Ok((path, secs, voice)) => {
                made = Some((path, secs, voice, provider));
                break;
            }
            Err(e) => failures.push(format!("{}: {e}", model.unwrap_or(provider))),
        }
    }
    let Some((path, secs, voice, provider)) = made else {
        item.note = format!("The voiceover wasn't made ({}).", failures.join("; "));
        return item;
    };
    if provider == "kokoro" && !key.is_empty() {
        fell_back.push_str(&format!("Gemini's voice wasn't available ({}), so the local Kokoro voice was used. ", failures.join("; ")));
    }
    let path = path.as_str();
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("wav");
    let to = format!("assets/voiceover.{ext}");
    if std::fs::copy(dir.join(path), dir.join(&to)).is_err() {
        item.note = "The voiceover wasn't saved.".into();
        return item;
    }
    // Exact word times when the speech model is installed; otherwise spread over the length of the narration.
    let (words, estimated) = match voice["words"].as_array() {
        Some(w) if !w.is_empty() => (w.clone(), false),
        _ => match videokit::transcribe_words(rt, dir, &to).await {
            Some(heard) => (align_words(&v.text, heard), false),
            None => (estimate_words(&v.text, secs), true),
        },
    };
    let _ = std::fs::write(dir.join("assets/voiceover.words.json"), serde_json::to_string_pretty(&json!({ "duration": secs, "estimated": estimated, "words": words })).unwrap_or_default());
    item.ok = true;
    item.file = to;
    item.duration = Some(secs);
    item.credit = if provider == "kokoro" { "Kokoro speech (local)".into() } else { "Gemini speech".into() };
    item.note = format!("{fell_back}Word timings are in assets/voiceover.words.json.");
    item
}

/// Spoken files the person gave (talking video, interview audio…): the words with their times, so captions
/// and highlights can be placed where each word is really said. Installs the speech model first if needed.
async fn transcribe_given(app: &AppHandle, rt: &Runtime, dir: &Path, folder: &str) -> Vec<Item> {
    let ours = |name: &str| name.starts_with("voiceover") || name.starts_with("sfx") || name.starts_with("bgm");
    let mut found: Vec<String> = vec![];
    for entry in std::fs::read_dir(dir.join("assets")).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let ext = Path::new(&name).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let stem = Path::new(&name).file_stem().and_then(|e| e.to_str()).unwrap_or("").to_string();
        if ["mp4", "mov", "m4v", "webm", "mp3", "wav", "m4a", "aac", "ogg"].contains(&ext.as_str()) && !ours(&name) && !dir.join(format!("assets/{stem}.words.json")).is_file() {
            found.push(name);
        }
    }
    found.sort();
    if found.is_empty() {
        return vec![];
    }
    if !rt.captions_ready() {
        stage(app, folder, "media", "Installing the speech model so the words can be timed (once, about 700 MB)…", Value::Null);
        if let Err(e) = videokit::captions_install(app.clone()).await {
            return found.into_iter().map(|n| Item { id: format!("{n}-words"), kind: "transcript".into(), note: e.clone(), ..Default::default() }).collect();
        }
    }
    let rt = Runtime::for_app(app);
    let mut items = vec![];
    for name in found {
        stage(app, folder, "media", format!("Listening to {name}…"), Value::Null);
        let rel = format!("assets/{name}");
        let mut item = Item { id: format!("{name}-words"), kind: "transcript".into(), ..Default::default() };
        match videokit::transcribe_words(&rt, dir, &rel).await {
            Some(words) => {
                let out = format!("assets/{}.words.json", Path::new(&name).file_stem().and_then(|e| e.to_str()).unwrap_or("media"));
                let end = words.last().and_then(|w| w["end"].as_f64()).unwrap_or(0.0);
                let _ = std::fs::write(dir.join(&out), serde_json::to_string_pretty(&json!({ "source": rel, "duration": end, "estimated": false, "words": words })).unwrap_or_default());
                item.ok = true;
                item.file = out;
                item.note = format!("{} words with exact start and end times, spoken in {rel}. Use them for captions and highlights.", words.len());
            }
            None => item.note = format!("No speech was found in {rel} (or it couldn't be transcribed)."),
        }
        items.push(item);
    }
    items
}

/// Fetch and make everything in requests.json. Writes assets/manifest.json and returns the items.
async fn fulfill(app: &AppHandle, rt: &Runtime, dir: &Path, folder: &str, meta: &videokit::VideoMeta, reqs: &Requests, chat: &str) -> Vec<Item> {
    let _ = std::fs::create_dir_all(dir.join("assets"));
    let mut items: Vec<Item> = vec![];
    let mut blocks: Vec<Value> = vec![];
    items.extend(transcribe_given(app, rt, dir, folder).await);
    for name in reqs.blocks.iter().filter_map(|b| clean_block(b)) {
        stage(app, folder, "media", format!("Installing the “{name}” effect…"), Value::Null);
        match hf_json(rt, dir, &["add", &name, "--dir", ".", "--no-clipboard", "--json"]).await {
            Ok(v) => blocks.push(json!({ "name": name, "ok": true, "result": v })),
            Err(e) => blocks.push(json!({ "name": name, "ok": false, "error": e })),
        }
    }
    for m in &reqs.media {
        let Some(id) = clean_id(&m.id) else { continue };
        let m = MediaRequest { id, ..m.clone() };
        stage(app, folder, "media", format!("Finding {}: {}", m.kind, tasks::truncate(&m.intent, 60)), Value::Null);
        let item = resolve_media(rt, dir, &m).await;
        // A photo that the catalog couldn't give is made instead.
        if !item.ok && m.kind == "image" {
            stage(app, folder, "media", format!("Making a picture: {}", tasks::truncate(&m.intent, 60)), Value::Null);
            items.extend(generate_image(app, dir, meta, &ImageRequest { id: m.id.clone(), prompt: m.intent.clone(), sheet: false }, chat).await);
        } else {
            items.push(item);
        }
        if stopped(folder) {
            break;
        }
    }
    for im in &reqs.images {
        let Some(id) = clean_id(&im.id) else { continue };
        stage(app, folder, "media", format!("Making {}…", if im.sheet { "a sheet of objects" } else { "a picture" }), Value::Null);
        items.extend(generate_image(app, dir, meta, &ImageRequest { id, ..im.clone() }, chat).await);
        if stopped(folder) {
            break;
        }
    }
    if let Some(v) = reqs.voiceover.as_ref().filter(|v| !v.text.trim().is_empty()) {
        stage(app, folder, "media", "Recording the voiceover…", Value::Null);
        items.push(make_voiceover(app, rt, dir, v).await);
    }
    let manifest = json!({ "items": items, "blocks": blocks });
    let _ = std::fs::write(dir.join("assets/manifest.json"), serde_json::to_string_pretty(&manifest).unwrap_or_default());
    items
}

fn manifest_note(items: &[Item]) -> String {
    let ok = items.iter().filter(|i| i.ok).count();
    let failed: Vec<&str> = items.iter().filter(|i| !i.ok).map(|i| i.id.as_str()).collect();
    if failed.is_empty() {
        format!("All {ok} items are ready.")
    } else {
        format!("{ok} items are ready; these could not be made: {}.", failed.join(", "))
    }
}

// ---------- the whole thing ----------

pub struct Spec {
    pub folder: String,
    pub title: String,
    pub brief: String,
    pub workflow: String,
    pub chat: String,
    /// Skip the approval step (tests, and "just make it").
    pub auto_approve: bool,
}

async fn wait_for_answer(folder: &str) -> Answer {
    let (tx, rx) = oneshot::channel();
    ANSWERS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(folder.to_string(), tx);
    rx.await.unwrap_or(Answer { approve: false, feedback: String::new() })
}

/// The person's answer to a storyboard: approve it, or say what to change.
#[tauri::command]
pub fn video_answer(folder: String, approve: bool, feedback: Option<String>) -> Result<(), String> {
    let tx = ANSWERS.lock().unwrap_or_else(|e| e.into_inner()).as_mut().and_then(|m| m.remove(&folder));
    match tx {
        Some(tx) => tx.send(Answer { approve, feedback: feedback.unwrap_or_default() }).map_err(|_| "That storyboard isn't waiting any more.".to_string()),
        None => Err("There's no storyboard waiting for an answer.".into()),
    }
}

/// Stop a video that is being made.
#[tauri::command]
pub fn video_stop(folder: String) {
    with_set(&STOPPED, |s| s.insert(folder.clone()));
    if let Some(tx) = ANSWERS.lock().unwrap_or_else(|e| e.into_inner()).as_mut().and_then(|m| m.remove(&folder)) {
        let _ = tx.send(Answer { approve: false, feedback: String::new() });
    }
    // Only this video's render or check; another video being made carries on.
    videokit::video_cancel(Some(folder));
}

/// Make a video from a brief. The project folder is made first (video_new). Runs in the background.
#[tauri::command]
pub async fn video_make(app: AppHandle, folder: String, title: String, brief: String, workflow: Option<String>, chat_id: Option<String>, auto_approve: Option<bool>) -> Result<(), String> {
    let rt = Runtime::for_app(&app);
    if !rt.ready() {
        return Err("The video tools aren't set up yet. Open Set up and press Set up video.".into());
    }
    let (dir, _) = preview::project_dir(&app, &folder)?;
    if !dir.join("video.json").is_file() {
        return Err("That folder isn't a video project.".into());
    }
    if with_set(&RUNNING, |s| !s.insert(folder.clone())) {
        return Err("This video is already being made.".into());
    }
    with_set(&STOPPED, |s| s.remove(&folder));
    let spec = Spec { folder: folder.clone(), title, brief, workflow: workflow.unwrap_or_default(), chat: chat_id.unwrap_or_default(), auto_approve: auto_approve.unwrap_or(false) };
    tauri::async_runtime::spawn(async move {
        let result = produce(&app, &rt, &dir, &spec).await;
        with_set(&RUNNING, |s| s.remove(&spec.folder));
        match result {
            Ok(note) => stage(&app, &spec.folder, "done", note, Value::Null),
            Err(e) => stage(&app, &spec.folder, "error", e, Value::Null),
        }
    });
    Ok(())
}

async fn produce(app: &AppHandle, rt: &Runtime, dir: &Path, spec: &Spec) -> Result<String, String> {
    let folder = spec.folder.as_str();
    videokit::ensure_content(app).await?;
    let meta: videokit::VideoMeta = std::fs::read_to_string(dir.join("video.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).ok_or("video.json is missing.")?;
    // The catalog's names, so the worker can pick effects that exist.
    let _ = std::fs::copy(rt.content().join("registry/registry.json"), dir.join("registry-index.json"));

    // 1-2. Plan, and the person's approval.
    let mut feedback = String::new();
    let mut reqs = Requests::default();
    for round in 1..=MAX_PLAN_ROUNDS {
        stage(app, folder, "plan", if round == 1 { "Planning the video…".to_string() } else { "Updating the storyboard…".to_string() }, Value::Null);
        run_stage(app, folder, &format!("Storyboard: {}", spec.title), plan_request(&spec.title, &spec.brief, &meta, &spec.workflow, &feedback), &spec.workflow, &spec.chat).await?;
        let board = std::fs::read_to_string(dir.join("STORYBOARD.md")).map_err(|_| "The worker didn't write a storyboard.".to_string())?;
        reqs = match std::fs::read_to_string(dir.join("requests.json")) {
            Ok(t) => parse_requests(&t)?,
            Err(_) => Requests::default(),
        };
        if spec.auto_approve {
            break;
        }
        stage(app, folder, "storyboard", "The storyboard is ready. Read it and approve it, or say what to change.", json!({ "markdown": board, "round": round }));
        let answer = wait_for_answer(folder).await;
        if stopped(folder) {
            return Err("Stopped.".into());
        }
        if answer.approve {
            break;
        }
        if round == MAX_PLAN_ROUNDS {
            return Err("The storyboard still isn't right after three tries. Start again with a clearer brief.".into());
        }
        feedback = answer.feedback;
    }

    // 3. Media.
    stage(app, folder, "media", "Getting the media ready…", Value::Null);
    let items = fulfill(app, rt, dir, folder, &meta, &reqs, &spec.chat).await;
    if stopped(folder) {
        return Err("Stopped.".into());
    }
    let (meta, length_note) = fit_length(dir, &meta, &items);

    // 4. Compose.
    stage(app, folder, "compose", "Building the video…", Value::Null);
    run_stage(app, folder, &format!("Compose: {}", spec.title), compose_request(&meta, &manifest_note(&items), &length_note), &spec.workflow, &spec.chat).await?;

    // 4b. A composition that plays like a slideshow (too little motion, or one layout refilled) goes back once.
    let calm = std::fs::read_to_string(dir.join("STORYBOARD.md")).map(|b| is_calm(&b)).unwrap_or(false);
    let problems = std::fs::read_to_string(dir.join("index.html"))
        .map(|h| [motion_problems(&h, meta.seconds, calm), layout_problems(&h)].concat())
        .unwrap_or_default();
    if !problems.is_empty() && !stopped(folder) {
        stage(app, folder, "fix", "The video still looks like slides. Reworking it…", json!({ "problems": problems }));
        // A worker that stumbles here still leaves a video; the checker below catches anything it broke.
        if let Err(e) = run_stage(app, folder, "Rework the design", rework_request(&problems), &spec.workflow, &spec.chat).await {
            if stopped(folder) {
                return Err(e);
            }
        }
    }
    let gaps = media_gaps(&items);
    checked_draft(app, rt, dir, folder, &spec.workflow, &spec.chat).await.map(|note| format!("{note}{gaps}"))
}

/// What the person should know is missing from the finished video: a voiceover or music that couldn't be made.
fn media_gaps(items: &[Item]) -> String {
    let mut out = String::new();
    if let Some(v) = items.iter().find(|i| i.kind == "voiceover" && !i.ok) {
        out.push_str(&format!(" It has no voiceover: {}", tasks::truncate(v.note.trim(), 220)));
        if !out.ends_with('.') {
            out.push('.');
        }
    }
    if items.iter().any(|i| i.kind == "bgm" && !i.ok) && !items.iter().any(|i| i.kind == "bgm" && i.ok) {
        out.push_str(" It has no music: none could be found for it (music comes from HeyGen's catalog, so connect HeyGen in Set up if it isn't).");
    }
    out
}

/// Run HyperFrames' checker and have the worker fix what it finds, up to MAX_FIX_ROUNDS times. Returns the last report.
async fn check_and_fix(app: &AppHandle, rt: &Runtime, dir: &Path, folder: &str, workflow: &str, chat: &str, first: &str) -> Result<videokit::CheckReport, String> {
    let mut last = videokit::CheckReport { ok: false, problems: vec![], text: String::new() };
    for round in 0..=MAX_FIX_ROUNDS {
        stage(app, folder, "check", if round == 0 { first.to_string() } else { "Checking the fixes…".to_string() }, Value::Null);
        last = videokit::check_project_public(rt, dir).await?;
        if last.ok || round == MAX_FIX_ROUNDS || stopped(folder) {
            break;
        }
        stage(app, folder, "fix", format!("Fixing {} problem{}…", last.problems.len().max(1), if last.problems.len() == 1 { "" } else { "s" }), json!({ "problems": last.problems }));
        let problems = if last.problems.is_empty() { vec![tasks::truncate(&last.text, 400)] } else { last.problems.clone() };
        run_stage(app, folder, "Fix the video", fix_request(&problems), workflow, chat).await?;
    }
    Ok(last)
}

/// Put frames of the draft in .review/ (named by their time) for the worker to look at. Returns how many.
async fn leave_review_frames(rt: &Runtime, dir: &Path) -> usize {
    let secs = std::fs::read_to_string(dir.join("video.json")).ok().and_then(|t| serde_json::from_str::<videokit::VideoMeta>(&t).ok()).map(|m| m.seconds as f64).unwrap_or(0.0);
    if secs <= 0.0 {
        return 0;
    }
    let review = dir.join(".review");
    let _ = std::fs::remove_dir_all(&review);
    let _ = std::fs::create_dir_all(&review);
    let frames = crate::videoreview::frames(&rt.ffmpeg(), &dir.join("renders/draft.mp4"), secs, &std::env::temp_dir().join(format!("jarvis-shots-{}", std::process::id()))).await;
    frames.iter().filter(|f| std::fs::write(review.join(format!("at-{:05.1}s.jpg", f.at)), &f.jpeg).is_ok()).count()
}

fn polish_request(problems: &[String], frames: usize) -> String {
    let look = if frames > 0 {
        format!("The {frames} frames the art director looked at are attached to this message (and are in .review/, named by their second): look at them before you change anything, and check each fix against the frame it is about. ")
    } else {
        String::new()
    };
    format!(
        "Jarvis rendered a draft, watched it and looked at its frames. The art director found these problems:\n{}\n{look}\
         Fix each at its cause in index.html. You may re-stage or redesign a scene (bring the camera in close, make the subject fill the frame, make things happen in a still stretch) \
         as long as its idea, its words and its timing against the voiceover stay; never drop content. Follow know-how/video-design/SKILL.md. Keep everything else as it is, then run ./hf lint.",
        problems.iter().map(|p| format!("- {p}")).collect::<Vec<_>>().join("\n")
    )
}

/// Check the video, have the worker fix what the checker finds, then render a draft. Then the art director
/// watches the draft (still and blank stretches, and its frames), and the worker polishes what was found,
/// up to MAX_REVIEW_ROUNDS times. A polish that can't be made to pass the checker is undone.
async fn checked_draft(app: &AppHandle, rt: &Runtime, dir: &Path, folder: &str, workflow: &str, chat: &str) -> Result<String, String> {
    let last = check_and_fix(app, rt, dir, folder, workflow, chat, "Checking the video…").await?;
    if !last.ok {
        let why = last.problems.first().cloned().unwrap_or_else(|| "the checker could not run it".into());
        return Err(format!("The video still has a problem Jarvis couldn't fix: {why}"));
    }
    let mut done = render_draft(app, rt, dir, folder).await?;
    let (mut applied, mut left): (usize, Vec<String>) = (0, vec![]);
    let mut reviewed = false;
    // Why the art director couldn't look at the frames, if it couldn't (the automatic checks still ran).
    let mut eyes_closed: Option<String> = None;
    // Whether it looked at least once.
    let mut looked = false;
    for _ in 0..MAX_REVIEW_ROUNDS {
        if stopped(folder) {
            break;
        }
        let Ok((problems, unseen)) = visual_review(app, rt, dir, folder).await else { break };
        match unseen {
            Some(why) => {
                eyes_closed.get_or_insert(why);
            }
            None => looked = true,
        }
        reviewed = true;
        left = problems.clone();
        if problems.is_empty() {
            break;
        }
        stage(app, folder, "fix", format!("The art director found {} thing{} to fix…", problems.len(), if problems.len() == 1 { "" } else { "s" }), json!({ "problems": problems }));
        let passing = std::fs::read(dir.join("index.html")).ok();
        let shown = leave_review_frames(rt, dir).await;
        let polished = run_stage(app, folder, "Polish the video", polish_request(&problems, shown), workflow, chat).await;
        let _ = std::fs::remove_dir_all(dir.join(".review"));
        if let Err(e) = polished {
            if stopped(folder) {
                return Err(e);
            }
            break;
        }
        let report = check_and_fix(app, rt, dir, folder, workflow, chat, "Checking the polish…").await?;
        if !report.ok {
            // Back to the version that passed, which is the draft already rendered.
            if let Some(page) = passing {
                let _ = std::fs::write(dir.join("index.html"), page);
            }
            break;
        }
        done = render_draft(app, rt, dir, folder).await?;
        applied += problems.len();
        left.clear();
    }
    if !left.is_empty() {
        let _ = std::fs::write(dir.join("review-notes.md"), format!("# Left unfixed\n\n{}\n", left.join("\n")));
    } else {
        let _ = std::fs::remove_file(dir.join("review-notes.md"));
    }
    let review_note = match (reviewed, applied, left.len()) {
        (false, ..) => String::new(),
        (true, 0, 0) if !looked => " The automatic checks found nothing to fix.".into(),
        (true, 0, 0) => " The art director found nothing to fix.".into(),
        (true, a, 0) => format!(" The art director's {a} note{} {} applied.", if a == 1 { "" } else { "s" }, if a == 1 { "was" } else { "were" }),
        (true, _, l) => format!(" {l} of the art director's notes couldn't be applied; they are in review-notes.md."),
    };
    let eyes_note = eyes_closed.filter(|_| !looked).map(|why| format!(" The art director couldn't look at the frames ({}), so only the automatic checks ran.", tasks::truncate(&why, 120))).unwrap_or_default();
    Ok(format!("The draft is ready ({} KB, rendered in {}s).{review_note}{eyes_note}", done.size / 1024, done.seconds_taken))
}

async fn render_draft(app: &AppHandle, rt: &Runtime, dir: &Path, folder: &str) -> Result<videokit::RenderDone, String> {
    stage(app, folder, "render", "Rendering a draft…", Value::Null);
    let handle = app.clone();
    let key = folder.to_string();
    let render = videokit::render_public(rt, dir, "draft", move |percent, message| {
        let _ = handle.emit("video-render", json!({ "folder": key, "percent": percent, "message": message }));
    });
    tokio::pin!(render);
    // Stop works while it waits for another video's render too (a running render is killed by video_stop).
    loop {
        tokio::select! {
            done = &mut render => return done,
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                if stopped(folder) {
                    return Err("Stopped.".into());
                }
            }
        }
    }
}

/// Watch the draft for blank and still stretches and text off the frame (always), and show its frames to Gemini
/// (with a key). Returns what was found and, when Gemini couldn't look, why.
async fn visual_review(app: &AppHandle, rt: &Runtime, dir: &Path, folder: &str) -> Result<(Vec<String>, Option<String>), String> {
    stage(app, folder, "check", "The art director is looking at the draft…", Value::Null);
    let meta = videokit::video_info(app.clone(), folder.to_string()).map(|i| i.meta).map_err(|e| e.to_string())?;
    let video = dir.join("renders/draft.mp4");
    let secs = meta.seconds as f64;
    let calm = std::fs::read_to_string(dir.join("STORYBOARD.md")).map(|b| is_calm(&b)).unwrap_or(false);
    let mut problems = crate::videoreview::blank_problems(&crate::videoreview::blanks(&rt.ffmpeg(), &video).await);
    problems.extend(videokit::offcanvas_problems(rt, dir, meta.seconds).await);
    if !calm {
        problems.extend(crate::videoreview::still_problems(&crate::videoreview::stills(&rt.ffmpeg(), &video, secs).await, secs));
    }
    let key = settings::load(app).gemini_api_key;
    if key.trim().is_empty() {
        return Ok((problems, Some("there is no Gemini key".into())));
    }
    let frames = crate::videoreview::frames(&rt.ffmpeg(), &video, secs, &std::env::temp_dir().join(format!("jarvis-review-{}", std::process::id()))).await;
    match crate::videoreview::review(crate::videoreview::gemini_base(), key.trim(), &meta.title, &frames).await {
        Ok(found) => {
            problems.extend(found);
            Ok((problems, None))
        }
        Err(e) => Ok((problems, Some(e))),
    }
}

/// Change a finished video: the worker edits it, the checker looks, and a new draft is rendered.
#[tauri::command]
pub async fn video_edit(app: AppHandle, folder: String, instructions: String, region: Option<String>, chat_id: Option<String>) -> Result<(), String> {
    let rt = Runtime::for_app(&app);
    let (dir, _) = preview::project_dir(&app, &folder)?;
    if !dir.join("index.html").is_file() {
        return Err("This video has no composition yet.".into());
    }
    if instructions.trim().is_empty() {
        return Err("Say what to change.".into());
    }
    if with_set(&RUNNING, |s| !s.insert(folder.clone())) {
        return Err("This video is being worked on. Wait for it to finish, or stop it first.".into());
    }
    with_set(&STOPPED, |s| s.remove(&folder));
    let chat = chat_id.unwrap_or_default();
    let pointed = region.filter(|r| !r.trim().is_empty()).map(|r| format!("\nThe person pointed at this part of the picture: {}.", r.trim())).unwrap_or_default();
    let request = format!(
        "{}\n\nThis is a change to the existing video in this folder, not a new one: read STORYBOARD.md and index.html, make precise edits, and keep everything the change doesn't touch as it is. \
         Media can't be fetched for a change: use only the files already in assets/.{pointed} Run ./hf lint when you are done.",
        instructions.trim()
    );
    tauri::async_runtime::spawn(async move {
        let result = async {
            videokit::ensure_content(&app).await?;
            stage(&app, &folder, "compose", "Changing the video…", Value::Null);
            run_stage(&app, &folder, &format!("Change: {}", tasks::truncate(&instructions, 50)), request, "", &chat).await?;
            checked_draft(&app, &rt, &dir, &folder, "", &chat).await
        }
        .await;
        with_set(&RUNNING, |s| s.remove(&folder));
        match result {
            Ok(note) => stage(&app, &folder, "done", note, Value::Null),
            Err(e) => stage(&app, &folder, "error", e, Value::Null),
        }
    });
    Ok(())
}

/// Developer builds only: `JARVIS_E2E_VIDEO=<brief.json>` makes the app run the whole video flow by itself
/// at start-up (setting up the toolbox if needed, approving its own storyboard) and log every stage to
/// `JARVIS_E2E_LOG`. It exists to test the real app process, with real settings, Codex and tools.
#[cfg(debug_assertions)]
pub fn e2e_hook(app: AppHandle) {
    use std::io::Write;
    use tauri::Listener;
    let Some(spec_path) = std::env::var_os("JARVIS_E2E_VIDEO") else { return };
    let log_path = std::env::var_os("JARVIS_E2E_LOG").map(std::path::PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("jarvis-e2e-video.log"));
    let log = move |line: String| {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
            let _ = writeln!(f, "[{}] {}", chrono::Local::now().format("%H:%M:%S"), line);
        }
    };
    for name in ["video-setup", "video-stage", "video-render", "heygen-login"] {
        let log = log.clone();
        app.listen(name, move |e| log(format!("{name} {}", e.payload())));
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(12)).await;
        let spec: Value = match std::fs::read_to_string(&spec_path).ok().and_then(|t| serde_json::from_str(&t).ok()) {
            Some(v) => v,
            None => return log("E2E: couldn't read the brief file".into()),
        };
        // One brief, or {"runs": [brief, brief…]} made one after the other.
        let runs: Vec<Value> = spec["runs"].as_array().cloned().unwrap_or_else(|| vec![spec.clone()]);
        for run in runs {
            e2e_run(&app, &run, &log).await;
        }
        log("E2E: all runs finished".into());
    });
}

#[cfg(debug_assertions)]
fn clean_rel(p: &str) -> bool {
    !p.is_empty() && !p.starts_with('/') && !p.split('/').any(|c| c == "..")
}

#[cfg(debug_assertions)]
async fn e2e_run(app: &AppHandle, spec: &Value, log: &(impl Fn(String) + Clone + Send + Sync + 'static)) {
    use tauri::Listener;
    let text = |k: &str, d: &str| spec[k].as_str().unwrap_or(d).to_string();
    log(format!("E2E: starting \"{}\"", text("title", "Test video")));
    if !Runtime::for_app(app).ready() {
        log("E2E: setting up the video tools".into());
        if let Err(e) = videokit::video_setup(app.clone()).await {
            return log(format!("E2E: setup failed: {e}"));
        }
    }
    if spec["heygen_install"].as_bool() == Some(true) {
        match videokit::heygen_install(app.clone()).await {
            Ok(s) => log(format!("E2E: HeyGen installed={} signed_in={} ({})", s.installed, s.signed_in, s.message)),
            Err(e) => log(format!("E2E: HeyGen install failed: {e}")),
        }
    }
    let ws = match crate::workspaces::create_project(app.clone(), text("title", "Test video")) {
        Ok(w) => w,
        Err(e) => return log(format!("E2E: project failed: {e}")),
    };
    let folder = match crate::workspaces::project_new_site(app.clone(), ws.slug.clone(), text("title", "Test video")) {
        Ok(f) => f,
        Err(e) => return log(format!("E2E: folder failed: {e}")),
    };
    if let Err(e) = videokit::video_new(app.clone(), folder.clone(), text("title", "Test video"), Some(text("format", "landscape")), spec["seconds"].as_u64().map(|s| s as u32)) {
        return log(format!("E2E: video_new failed: {e}"));
    }
    log(format!("E2E: folder {folder}"));
    // Footage or audio the person "gave": copied into the folder before the plan.
    for f in spec["files"].as_array().into_iter().flatten() {
        let (from, to) = (f["from"].as_str().unwrap_or(""), f["to"].as_str().unwrap_or(""));
        let dest = Path::new(&folder).join(to);
        if clean_rel(to) {
            let _ = std::fs::create_dir_all(dest.parent().unwrap_or(Path::new(&folder)));
            log(format!("E2E: given file {to}: {:?}", std::fs::copy(from, &dest).map(|n| n)));
        }
    }
    // Resolves when this video reaches "done" (after the optional change) or "error".
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let change = spec["edit"].as_str().map(String::from);
    let (handle, edit_folder, log2) = (app.clone(), folder.clone(), log.clone());
    let asked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let id = app.listen("video-stage", move |e| {
        let v: Value = serde_json::from_str(e.payload()).unwrap_or(Value::Null);
        if v["folder"] != json!(edit_folder) {
            return;
        }
        match v["stage"].as_str() {
            Some("error") => {
                let _ = tx.send("error".into());
            }
            Some("done") => match &change {
                Some(change) if !asked.swap(true, std::sync::atomic::Ordering::SeqCst) => {
                    let (handle, folder, change, log) = (handle.clone(), edit_folder.clone(), change.clone(), log2.clone());
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(3)).await;
                        log(format!("E2E: asking for a change: {change}"));
                        if let Err(e) = video_edit(handle, folder, change, None, None).await {
                            log(format!("E2E: video_edit failed: {e}"));
                        }
                    });
                }
                _ => {
                    let _ = tx.send("done".into());
                }
            },
            _ => {}
        }
    });
    if let Err(e) = video_make(app.clone(), folder.clone(), text("title", "Test video"), text("brief", ""), Some(text("workflow", "")), None, Some(true)).await {
        log(format!("E2E: video_make failed: {e}"));
    }
    let mut rx = rx;
    let outcome = tokio::time::timeout(Duration::from_secs(40 * 60), rx.recv()).await;
    log(format!("E2E: \"{}\" ended: {:?}", text("title", "Test video"), outcome.ok().flatten()));
    app.unlisten(id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_read_and_kept_small() {
        let r = parse_requests(r#"{"blocks":["flash-through-white"],"media":[{"id":"music","type":"bgm","intent":"warm acoustic"}],"images":[{"id":"sheet1","prompt":"five breads","sheet":true}],"voiceover":{"text":"Hello.","style":"warm"}}"#).unwrap();
        assert_eq!((r.blocks.len(), r.media[0].kind.as_str(), r.images[0].sheet), (1, "bgm", true));
        assert_eq!(r.voiceover.unwrap().text, "Hello.");
        assert!(parse_requests("{}").unwrap().media.is_empty(), "every key is optional");
        assert!(parse_requests("not json").is_err());
        let many = format!("{{\"blocks\":[{}]}}", (0..25).map(|i| format!("\"b{i}\"")).collect::<Vec<_>>().join(","));
        assert!(parse_requests(&many).is_err());
    }

    #[test]
    fn names_from_a_worker_are_never_trusted_as_paths() {
        assert_eq!(clean_id("hero_1"), Some("hero_1".into()));
        assert_eq!(clean_id("../../etc/passwd"), None);
        assert_eq!(clean_id("a b"), None);
        assert_eq!(clean_id(""), None);
        assert_eq!(clean_block("flash-through-white"), Some("flash-through-white".into()));
        assert_eq!(clean_block("Flash; rm -rf /"), None);
        assert_eq!(clean_block("--force"), Some("--force".into()), "dashes are allowed in names");
    }

    #[test]
    fn caption_timings_follow_the_narration_length() {
        let w = estimate_words("Warm bread, every morning. Baked at four.", 6.0);
        assert_eq!(w.len(), 7);
        assert_eq!(w[0]["text"], "Warm");
        assert_eq!(w[0]["start"], 0.0);
        let last_end = w[6]["end"].as_f64().unwrap();
        assert!(last_end > 4.8 && last_end <= 6.0, "{last_end}");
        let starts: Vec<f64> = w.iter().map(|x| x["start"].as_f64().unwrap()).collect();
        assert!(starts.windows(2).all(|p| p[1] > p[0]), "words come in order");
        assert!(w[2]["start"].as_f64().unwrap() - w[1]["end"].as_f64().unwrap() > 0.05, "a pause follows a comma");
        assert!(estimate_words("", 5.0).is_empty() && estimate_words("hi", 0.0).is_empty());
    }

    #[test]
    fn heard_words_take_the_scripts_spelling_when_they_line_up() {
        let heard = vec![json!({"text": "Lacamari", "start": 0.3, "end": 0.9}), json!({"text": "brings", "start": 1.0, "end": 1.4})];
        let aligned = align_words("Lakhamari brings", heard.clone());
        assert_eq!((aligned[0]["text"].as_str(), aligned[0]["start"].as_f64()), (Some("Lakhamari"), Some(0.3)));
        assert_eq!(align_words("Lakhamari brings a little crunch", heard.clone()).len(), 2, "a different count keeps what was heard");
        assert_eq!(align_words("Lakhamari brings a little crunch", heard)[0]["text"], "Lacamari");
    }

    #[test]
    fn a_video_grows_to_fit_its_narration() {
        let dir = std::env::temp_dir().join(format!("jarvis-fit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let meta = videokit::VideoMeta { title: "t".into(), format: "portrait".into(), width: 1080, height: 1920, seconds: 20 };
        std::fs::write(dir.join("index.html"), "<div id=\"root\" data-composition-id=\"main\" data-start=\"0\" data-duration=\"20\" data-width=\"1080\">").unwrap();
        let voice = |secs: f64| Item { id: "voiceover".into(), kind: "voiceover".into(), ok: true, duration: Some(secs), ..Default::default() };
        let (same, note) = fit_length(&dir, &meta, &[voice(18.0)]);
        assert_eq!((same.seconds, note.as_str()), (20, ""), "a narration that fits changes nothing");
        assert_eq!(fit_length(&dir, &meta, &[]).0.seconds, 20, "no narration, no change");
        let (longer, note) = fit_length(&dir, &meta, &[voice(22.76)]);
        assert_eq!(longer.seconds, 24, "22.76s of speech plus a breath, rounded up");
        assert!(note.contains("22.8 seconds") && note.contains("24 seconds"));
        assert!(std::fs::read_to_string(dir.join("index.html")).unwrap().contains("data-duration=\"24\""));
        assert!(std::fs::read_to_string(dir.join("video.json")).unwrap().contains("\"seconds\": 24"));
        assert_eq!(fit_length(&dir, &meta, &[voice(400.0)]).0.seconds, 180, "never longer than three minutes");
        // A narration that ends long before the planned length: the video is cut to fit, with time for the ending.
        std::fs::write(dir.join("index.html"), "<div id=\"root\" data-composition-id=\"main\" data-start=\"0\" data-duration=\"60\" data-width=\"1920\">").unwrap();
        let sixty = videokit::VideoMeta { seconds: 60, ..meta.clone() };
        let (shorter, note) = fit_length(&dir, &sixty, &[voice(49.6)]);
        assert_eq!(shorter.seconds, 53, "49.6s of speech and three seconds to read the ending");
        assert!(note.contains("shorter than the 60 seconds") && note.contains("Shrink"));
        assert!(std::fs::read_to_string(dir.join("index.html")).unwrap().contains("data-duration=\"53\""));
        assert_eq!(fit_length(&dir, &sixty, &[voice(55.0)]).0.seconds, 58, "more than a second to spare is trimmed too");
        assert_eq!(fit_length(&dir, &sixty, &[voice(56.5)]).0.seconds, 60, "a second to spare is left alone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_worker_gets_the_core_skills_and_the_workflow_that_fits() {
        let s = skills_for("music-to-video");
        assert_eq!(s[0], "video-design");
        assert!(s.contains(&"media-use".to_string()) && s.contains(&"hyperframes-registry".to_string()) && s.last().unwrap() == "music-to-video");
        assert_eq!(skills_for("slideshow").len(), CORE_SKILLS.len(), "a deck isn't a video");
        assert_eq!(skills_for("rm -rf").len(), CORE_SKILLS.len(), "an unknown workflow adds nothing");
        let meta = videokit::VideoMeta { title: "t".into(), format: "portrait".into(), width: 1080, height: 1920, seconds: 30 };
        let plan = plan_request("Bakery reel", "A reel for a bakery", &meta, "product-launch-video", "");
        assert!(plan.contains("PLAN") && plan.contains("1080x1920") && plan.contains("30 seconds") && plan.contains("requests.json") && plan.contains("know-how/product-launch-video/SKILL.md"));
        assert!(!plan.contains("asked for changes"));
        assert!(plan_request("t", "b", &meta, "", "make it shorter").contains("make it shorter"));
        let compose = compose_request(&meta, "All 3 items are ready.", "");
        assert!(compose.contains("COMPOSE") && compose.contains("assets/manifest.json") && compose.contains("./hf lint") && compose.contains("Do not render"));
        assert!(fix_request(&["✗ gsap_x: bad".into()]).contains("- ✗ gsap_x: bad"));
        assert!(plan.contains("Motion: high") && plan.contains("blueprints-index.md"));
        assert!(compose.contains("measures the motion"));
    }

    /// A composition in the old style: every element fades up into place, scene after scene.
    fn slideshow(scenes: usize) -> String {
        let tweens: String = (0..scenes)
            .map(|i| {
                let at = i as f64 * 6.0;
                format!(
                    "tl.fromTo(\"#s{i}-kicker\", {{ opacity: 0, y: 24 }}, {{ opacity: 1, y: 0, duration: 0.6, ease: \"power2.out\" }}, {at});\n\
                     tl.fromTo(\"#s{i}-title\", {{ opacity: 0, y: 60 }}, {{ opacity: 1, y: 0, duration: 0.9 }}, {});\n\
                     tl.to(\"#s{i}-title\", {{ scale: 1.04, duration: 4, ease: \"none\" }}, {at});\n",
                    at + 0.3
                )
            })
            .collect();
        format!("<html><head><script src=\"vendor/gsap.min.js\"></script></head><body><div id=\"root\"></div><script>const tl = gsap.timeline({{ paused: true }});\n{tweens}window.__timelines[\"main\"] = tl;</script></body></html>")
    }

    #[test]
    fn the_skeleton_moves_enough_and_a_slideshow_does_not() {
        let skeleton = include_str!("../skills/video-design/references/video-skeleton.html");
        assert_eq!(motion_problems(skeleton, 12, false), Vec::<String>::new(), "the skeleton is the example the worker copies: {:?}", measure_motion(skeleton));
        let m = measure_motion(skeleton);
        assert!(m.props.iter().any(|p| p == "strokeDashoffset") && m.props.iter().any(|p| p == "clipPath") && m.props.iter().any(|p| p == "skewX"), "{:?}", m.props);

        let slides = slideshow(10);
        let m = measure_motion(&slides);
        assert_eq!((m.tweens, m.entrances, m.fade_slides), (30, 20, 20));
        assert_eq!(m.props, vec!["opacity", "scale", "y"]);
        let problems = motion_problems(&slides, 60, false);
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems[0].contains("only 30 tweens") && problems[1].contains("opacity, scale, y") && problems[2].contains("20 of the 20 entrances"));
        assert!(rework_request(&problems).contains("- There are only 30 tweens"));

        // Many plain properties (bars scaling, a nudge sideways) don't make up for nothing expressive.
        let busy = slides.replace("window.__timelines", "tl.fromTo(\"#bar\", { scaleX: 0, scaleY: 0, x: 0, rotation: 0 }, { scaleX: 1, scaleY: 1, x: 3, rotation: 2 }, 1);\nwindow.__timelines");
        assert_eq!(measure_motion(&busy).props.len(), 7);
        assert!(motion_problems(&busy, 60, false).iter().any(|p| p.contains("3 or more of them beyond fades")));

        // Calm videos are held to a gentler measure and may fade.
        assert_eq!(motion_problems(&slides, 60, true).len(), 1, "calm still needs more than three properties");
    }

    #[test]
    fn the_motion_measure_reads_only_tweens_in_inline_scripts() {
        assert_eq!(object_keys("{ opacity: 0, 'y': 4, \"clipPath\": \"inset(0)\", keyframes: [{ x: 1 }] }"), vec!["opacity", "y", "clipPath", "keyframes", "x"]);
        assert!(object_keys("{ v: on ? a : b }").iter().all(|k| k == "v"), "a ternary's branches aren't keys");
        let html = "<script src=\"a.js\">tl.to(\"#a\", { rotation: 4 })</script><script>tl.to(\"#b\", { skewX: 4, onUpdate: () => f({ x: 1 }) });</script>";
        let m = measure_motion(html);
        assert_eq!((m.tweens, m.props.clone()), (1, vec!["skewX".to_string(), "x".to_string()]), "a script with src is not read");
        assert_eq!(first_object("\"#a\", { opacity: 0, y: 2 }, { opacity: 1 }"), "{ opacity: 0, y: 2 }");
    }

    #[test]
    fn scenes_refilled_from_one_layout_are_sent_back() {
        let scene = |i: usize, visual: &str| {
            format!(
                "<section id=\"s{i}\" class=\"clip scene\" data-start=\"{i}\" data-duration=\"1\" data-track-index=\"1\"><div class=\"cam\"><div class=\"copy\"><p class=\"eyebrow\">0{i} / TECH</p><h2>Title {i}</h2></div>{visual}</div></section>"
            )
        };
        let page = |body: String| format!("<html><head><style>.cam{{}}</style></head><body><div id=\"root\">{body}</div><script>const s = '<section class=\"clip scene\">';</script></body></html>");
        let templated = page((0..6).map(|i| scene(i, "<div class=\"visual\"><img class=\"photo\"></div>")).collect());
        let sigs = scene_signatures(&templated);
        assert_eq!(sigs.len(), 6, "markup inside scripts is not a scene");
        assert_eq!(sigs[0], "div.cam > div.copy > p.eyebrow > h2 > div.visual > img.photo");
        let problems = layout_problems(&templated);
        assert!(problems.len() == 1 && problems[0].starts_with("6 of the 6 scenes are built from the same layout"), "{problems:?}");
        assert!(rework_request(&problems).contains("section 4a") || problems[0].contains("section 4a"));

        let varied = page(
            [
                "<section id=\"s0\" class=\"clip\" data-start=\"0\" data-duration=\"1\"><div class=\"word-wall\"><span class=\"w\">FAST</span></div></section>",
                "<section id=\"s1\" class=\"clip\" data-start=\"1\" data-duration=\"1\"><svg class=\"diagram\"><path class=\"link\"/></svg></section>",
                "<div id=\"s2\" class=\"clip scene\" data-start=\"2\" data-duration=\"1\"><img class=\"full-bleed\"><h2 class=\"over\">Moves</h2></div>",
                "<section id=\"s3\" class=\"clip\" data-start=\"3\" data-duration=\"1\"><div class=\"orbit\"><div class=\"core\"></div></div></section>",
                "<section id=\"s4\" class=\"clip\" data-start=\"4\" data-duration=\"1\"><div class=\"cam\"><div class=\"copy\"><p class=\"eyebrow\">x</p><h2>y</h2></div></div></section>",
            ]
            .concat(),
        );
        assert_eq!(scene_signatures(&varied).len(), 5);
        assert!(layout_problems(&varied).is_empty());
        assert!(layout_problems(include_str!("../skills/video-design/references/video-skeleton.html")).is_empty());
        // Nested sections count once each and close where they should.
        let nested = page((0..4).map(|i| format!("<section class=\"clip\" data-start=\"{i}\"><section class=\"inner{i}\"><b class=\"k\"></b></section></section>")).collect());
        assert_eq!(scene_signatures(&nested).len(), 4);
        // A backdrop shared by every scene doesn't make different scenes look alike.
        let backdrop = |inner: &str| format!("<i class=\"dot\"></i><i class=\"dot\"></i><i class=\"dot\"></i><div class=\"glow\"></div>{inner}");
        let decorated = page(
            ["<b class=\"giant\">A</b><i class=\"x\"></i>", "<svg class=\"map\"><path class=\"route\"/></svg>", "<img class=\"photo\"><h2 class=\"over\">c</h2>", "<div class=\"grid\"><div class=\"tile\"></div></div>"]
                .iter()
                .enumerate()
                .map(|(i, inner)| format!("<section class=\"clip\" data-start=\"{i}\">{}</section>", backdrop(inner)))
                .collect(),
        );
        assert!(layout_problems(&decorated).is_empty(), "{:?}", scene_signatures(&decorated));
    }

    #[test]
    fn words_glued_to_a_span_are_found() {
        let page = "<html><body><p>Pick<span class=\"a\">one real task</span>this week</p><p>Ask <span>inspect</span> improve</p>\
                    <div class=\"wordmark\"><span class=\"ch\">O</span><span class=\"ch\">r</span></div><script>x='a<span>b'</script></body></html>";
        assert_eq!(glued_words(page), vec!["Pickone".to_string(), "taskthis".to_string()]);
        let problems = layout_problems(page);
        assert!(problems.len() == 1 && problems[0].contains("\"Pickone\", \"taskthis\""));
        assert!(glued_words(include_str!("../skills/video-design/references/video-skeleton.html")).is_empty());
    }

    #[test]
    fn the_audio_engine_says_why_it_made_no_voice() {
        let out = "  total voice duration: 0s\n\nanomalies (non-fatal):\n  - line vo: TTS failed — omitted (Gemini TTS HTTP 429: {\"error\":{\"message\":\"Rate limit exceeded for model gemini-3.8-flash-tts (limit: 10 requests per day on Free Tier).\",\"code\":\"too_many_requests\"}})";
        assert_eq!(engine_failure(out, ""), "Rate limit exceeded for model gemini-3.8-flash-tts (limit: 10 requests per day on Free Tier).");
        assert_eq!(engine_failure("", "warming up\nkokoro: model download failed\n"), "kokoro: model download failed");
        assert_eq!(engine_failure("  - line vo: bad voice duration — omitted", ""), "- line vo: bad voice duration — omitted");
    }

    #[test]
    fn the_polish_round_points_at_the_frames_when_there_are_some() {
        let with = polish_request(&["At about 3.0s: the title is cut off.".into()], 12);
        assert!(with.contains("- At about 3.0s: the title is cut off.") && with.contains("The 12 frames the art director looked at are attached") && with.contains(".review/"));
        let without = polish_request(&["x".into()], 0);
        assert!(!without.contains("attached") && without.contains("./hf lint"));
    }

    #[test]
    fn missing_voice_and_music_are_told() {
        let item = |kind: &str, ok: bool, note: &str| Item { id: kind.into(), kind: kind.into(), ok, note: note.into(), ..Default::default() };
        assert_eq!(media_gaps(&[item("voiceover", true, ""), item("bgm", true, ""), item("sfx", false, "")]), "", "a missing sound effect isn't worth a mention");
        let gaps = media_gaps(&[item("voiceover", false, "The voiceover wasn't made (gemini: quota used up)"), item("bgm", false, "no provider")]);
        assert!(gaps.starts_with(" It has no voiceover: The voiceover wasn't made (gemini: quota used up). It has no music: none could be found for it (music comes from HeyGen"), "{gaps}");
        assert_eq!(kokoro_voice("Friendly male voice, upbeat"), "am_michael");
        assert_eq!(kokoro_voice("warm, clear, unhurried"), "af_heart");
        assert_eq!(kokoro_voice("a female narrator"), "af_heart", "\"female\" isn't \"male\"");
    }

    #[test]
    fn a_storyboard_can_ask_for_a_calm_video() {
        assert!(is_calm("# Plan\n\nMotion: calm\n"));
        assert!(is_calm("- **Motion:** calm, slow drift"));
        assert!(is_calm("**Motion level: Calm**"));
        assert!(!is_calm("Motion: high"));
        assert!(!is_calm("The camera moves with calm confidence."));
    }
}
