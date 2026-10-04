---
name: "Video design"
description: "Plan and build a polished video as a HyperFrames composition: storyboard, scenes, type, colour, motion, captions, narration and a quality checklist. Use for any request to make a video, reel, promo, explainer or slideshow."
---

# Video design

You build videos as HTML. A HyperFrames composition is a web page whose elements carry timing in `data-*` attributes and whose motion lives on one paused GSAP timeline. Jarvis renders it frame by frame in a browser and encodes an MP4. Nothing in your page may depend on the real clock, the network or the user's mouse.

**Start from `references/video-skeleton.html`.** It is a complete, valid 12-second composition (three scenes with crossfades, a staggered list, captions). Copy its structure into the project's `index.html` and replace the content. Read `video.json` first: it gives the canvas size (`width`, `height`), the length (`seconds`) and the title. Put those numbers on the root element and design for that shape (a portrait 1080x1920 video is composed very differently from a 1920x1080 one).

After you finish, Jarvis runs `hyperframes check` on your work (structure, layout, motion, text contrast), sends you every problem it finds, and renders a draft. So write it clean the first time.

## 1. Plan before you build

Read the whole brief and every file in the folder (images in `assets/`, a `voiceover` or `script` if given). Write `STORYBOARD.md` first:

- **Goal and viewer**: who watches, where (social feed, a meeting, a website), and what they should do next.
- **Length**: the total from `video.json`. Scenes add up to it (within a second).
- **Scenes**, one line each: start, length, the one idea, the on-screen text (the exact words), the visual, the narration, and the assets it needs.
- **Narration script** if there is a voiceover: short sentences, about 2.5 words per second, so a 30-second video holds about 75 words.

Pace: one idea per scene, 3 to 6 seconds each, the hook in the first 2 seconds, a clear final scene with the one action to take. Hold each piece of text on screen at least 1.5 seconds, plus 0.4 seconds for every 5 words beyond the first 5.

## 2. The composition contract (the rules the checker enforces)

- One root `<div id="root" data-composition-id="main" data-start="0" data-duration="N" data-width="W" data-height="H">`, directly in `<body>`. Do not wrap it in a `<template>`. The root's `data-duration` is the video's length; set it to `video.json`'s `seconds`.
- Everything that appears and disappears is a **clip**: an element with `class="clip"`, `data-start` (seconds) and `data-duration`, and a `data-track-index`. The framework shows and hides clips by time. Never tween a clip's `visibility`, `display` or `autoAlpha`; fade a child or the clip's `opacity`.
- **One timeline**: `const tl = gsap.timeline({ paused: true })`, built completely, then `window.__timelines["main"] = tl` at the end. Never call `tl.play()`.
- Starting states go inside the tween: `tl.fromTo(el, { opacity: 0, y: 60 }, { opacity: 1, y: 0, duration: 0.8 }, atSeconds)`. Never put a CSS `transform` on an element you also animate with GSAP, and never centre with `transform: translate(-50%, -50%)` on something you animate; use flex or `inset: 0`.
- Animate `opacity`, `x`, `y`, `scale`, `rotation`. Do not tween `letterSpacing`, `fontSize`, `width` or `height` of text (they snap to whole pixels and stutter); scale instead.
- Deterministic only: no `Date.now()`, no `performance.now()`, no unseeded `Math.random()` (use a small seeded generator), no `fetch`, no `setTimeout`/`setInterval` for visuals. `repeat: -1` is allowed only under a finite root `data-duration`.
- No `<br>` in sentences; let text wrap inside a `max-width`. Every element that is scaled or rotated must be block-level and sized (not an inline `<span>`).
- Ids are unique across the page. Give sections readable ids (`s1`, `s1-title`).
- The root and the scenes use `width/height: 100%` or `inset: 0`; the canvas size comes from the root's `data-width`/`data-height`, so don't hardcode pixel sizes on `#root`.
- Warnings called `nested_structure_needs_subcomposition` are fine to ignore. Errors (`✗`) are not.

## 3. Assets: local files only

- Every image, video, audio and font is a file inside this folder (`assets/…`), referenced by a relative path. No `http://` URLs for media, no stock-photo links, no CDN scripts. The only script is `vendor/gsap.min.js`.
- Use the pictures you are given exactly as they are. A logo is never redrawn or recoloured. A photo or cut-out with a transparent background (PNG) is placed as an `<img>` with `alt`, a fixed `width`/`height` and `object-fit`.
- If the brief needs a picture you were not given, make it with CSS shapes, gradients, patterns or inline SVG, and say so in your report. Never leave an empty box.
- Fonts: use the system stacks from the skeleton (`ui-serif, Georgia, serif` for display, `system-ui, sans-serif` for text). A named font such as "Poppins" needs an `@font-face` that points to a file in `assets/fonts/`; without one the checker fails the page.
- **Audio**: a voiceover or music track is `<audio id="voiceover" src="assets/voiceover.mp3" data-start="0.2" data-duration="11.5" data-track-index="8" data-volume="1"></audio>`. Every `<audio>` needs an `id`, or the render is silent. Never add `crossorigin`, and never call `.play()`. Do not add an `<audio>` for a file that doesn't exist (not even inside an HTML comment: the checker reads comments too). Music under speech: `data-volume="0.2"` on the music element.
- **Video clips** (footage the user gave): `<video id="a-roll" class="clip" src="assets/clip.mp4" muted playsinline data-start="0" data-duration="5" data-track-index="0"></video>`. Keep timing on the video itself, not on a wrapper that also has `data-start`.

## 4. Design

- **Direction**: pick one that fits the subject and the tone words in the brief, and commit (editorial, warm and organic, modern minimal, bold and playful, luxury). Write your palette, the two fonts and the direction at the top of `STORYBOARD.md`.
- **Colour**: tokens on `:root` (`--bg`, `--surface`, `--text`, `--muted`, `--accent`). One accent. Text contrast at least 4.5:1 on its real background (the checker tests this). Never light-grey text on white.
- **Type for a screen across a room**: headlines 110 to 180px, supporting text 44 to 64px, nothing below 40px, in a 1920x1080 frame. For portrait 1080x1920 use 90 to 140px headlines and 48 to 60px text. Two typefaces at most. Headlines have `text-wrap: balance` and a `max-width`.
- **Safe area**: keep all text and logos at least 8% from every edge. For portrait/social formats keep the bottom 18% and top 10% clear of important text (apps draw buttons there). Captions sit in the lower third.
- **Layout**: grid or flex with generous gaps; one focal point per scene; align edges. Do not centre everything; mix a left text column with a large visual.
- **Motion**: entrances 0.5 to 0.9 seconds with `power2.out` / `power3.out`; stagger lists by 0.15 to 0.3 seconds; give long holds a slow push-in or drift (`scale` 1 to 1.05) so nothing sits dead still; move things the way they would move in the world. Crossfade scenes over 0.4 to 0.6 seconds by overlapping their clips. Vary the entrance style between scenes; don't use the same fade-up everywhere. Respect that every frame is rendered, so there is no cost to rich motion, but keep it purposeful.
- **Captions**: when there is narration, add captions: one phrase at a time (3 to 7 words), timed to the narration, as separate clips on their own track, high contrast on a pill or shadow (see the skeleton). Time each caption from the narration's real timing if a `narration.json` with sentence times is in the folder; otherwise estimate 2.5 words per second.
- **Ending**: the last 2 to 3 seconds hold the call to action (what to do, where to find it) and the logo, still enough to read.
- Avoid: a purple-to-blue gradient on every background, emoji as icons, walls of text, tiny grey text, five different animation styles at once, anything flashing faster than 3 times a second.

## 5. 3D scenes (only if the brief asks for 3D or it clearly serves the story)

If `vendor/three.min.js` exists in the folder, Three.js is available as a plain script (global `THREE`). Load it with `<script src="vendor/three.min.js"></script>` before your script; never use `import`, import maps or a CDN. Render from HyperFrames time, not the clock:

```html
<canvas id="three-layer" style="position:absolute;inset:0;width:100%;height:100%"></canvas>
<script>
  const renderer = new THREE.WebGLRenderer({ canvas: document.getElementById("three-layer"), alpha: true, antialias: true });
  renderer.setSize(1920, 1080, false); renderer.setPixelRatio(1);   // use the root's data-width / data-height
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera(35, 1920 / 1080, 0.1, 100);
  camera.position.set(0, 0, 6);
  /* …build meshes and lights once, synchronously… */
  function renderAt(t) { /* set rotation/position from t only */ renderer.render(scene, camera); }
  window.addEventListener("hf-seek", (e) => renderAt(e.detail.time));
  renderAt(window.__hfThreeTime || 0);
</script>
```

No `requestAnimationFrame`, no `OrbitControls` interaction, no downloaded models or textures (draw textures on a canvas). Always keep the root `data-duration`.

## 6. Before you finish

- [ ] `video.json`'s size and length are on the root; scenes add up to the length.
- [ ] Every clip has `data-start` and `data-duration`; nothing ends after the root's duration.
- [ ] Every file referenced exists in this folder; no remote URLs; every `<audio>` has an `id` and a real file.
- [ ] Text is large, inside the safe area, and readable (contrast).
- [ ] The hook is in the first 2 seconds, the last scene says what to do next.
- [ ] `STORYBOARD.md` lists the scenes, the palette and fonts, and what you invented.

Then write the report to the path you were given: `## What I made` (scenes and timing), `## Sample content` (anything invented), `## Left for you`. Your final spoken message is two or three plain sentences on what the video is and how long it is. Do not claim you rendered it: Jarvis does that.

*The composition rules here are distilled from HeyGen's open-source HyperFrames documentation (Apache License 2.0).*
