---
name: "Video design"
description: "Plan and build a polished, highly animated video as a HyperFrames composition: storyboard, camera, kinetic type, transitions, motion graphics, narration and a quality checklist. Use for any request to make a video, reel, promo, explainer or slideshow."
---

# Video design

You build videos as HTML. A HyperFrames composition is a web page whose elements carry timing in `data-*` attributes and whose motion lives on one paused GSAP timeline. Jarvis renders it frame by frame in a browser and encodes an MP4. Nothing in your page may depend on the real clock, the network or the user's mouse.

**You are a motion designer, not a slide maker.** The video must feel like a produced motion-graphics piece: a camera that moves, words that land on the beat, shapes that draw themselves, numbers that count, scenes that hand off to each other through real transitions. A sequence of title cards that fade up and crossfade is a failure, however tidy it looks. Every frame is rendered, so rich motion costs you nothing.

**You decide what to make.** For every scene, ask: what is the most vivid way to *show* this idea? Then build exactly that, as its own piece. One scene might be a single word filling the frame, the next a diagram that builds itself, then a photo the camera flies into, a fake app being used, a map, a 3D object turning, a wall of tiles, numbers racing, a collage, a split screen. There is no house layout, and no scene needs a headline. Section 4a has the rules that keep this from collapsing back into slides.

**Technique reference: `references/video-skeleton.html`.** It is a complete, valid 12-second composition with working code for the motion system below: a living background, a camera that pushes, shakes and whip-pans across a wide world, kinetic type, a self-drawing line, a count-up ring, a panel wipe and a letter cascade. Lift its *techniques* and its file structure; never its layouts or content. Read `video.json` first: it gives the canvas size (`width`, `height`), the length (`seconds`) and the title. Put those numbers on the root element and design for that shape (a portrait 1080x1920 video is composed very differently from a 1920x1080 one).

After you finish, Jarvis measures the motion in your script, runs `hyperframes check` (structure, layout, motion, text contrast), sends you every problem it finds, and renders a draft. So write it clean and alive the first time.

## 1. Plan before you build

Read the whole brief and every file in the folder (images in `assets/`, a `voiceover` or `script` if given). Write `STORYBOARD.md` first:

- **Goal and viewer**: who watches, where (social feed, a meeting, a website), and what they should do next.
- **Motion level**: a line `Motion: high` (the default) or `Motion: calm` (only when the brief asks for calm, minimal, quiet, elegant-and-still, or a plain presentation). See section 5.
- **Look**: when the brief brings no brand of its own, start from one of HyperFrames' frame presets instead of inventing a palette: skim `know-how/hyperframes-creative/frame-presets/*/FRAME.md`, pick the one that fits the subject and tone, and follow it (its colours, type and layout feel), remixing the brand in when there is one. Name it in the storyboard.
- **Genre know-how**: if `know-how/faceless-explainer/references/` or `know-how/product-launch-video/references/` is in the folder, read its `story-design.md`, `visual-design.md`, `motion-language.md` and `cut-catalog.md` before planning: they hold that genre's story shapes, looks and cuts.
- **Design direction**: the palette, the two fonts, the direction, and the **motion language**: your primary transition, one or two accent transitions, your house eases, and the one signature move the whole piece is remembered by.
- **The spine**: one persistent element that runs through the whole video and becomes each next scene (a line that is the typed underline, then a chart baseline, then a map route, then the underline of the call to action; a product that travels through every scene; a shape that keeps transforming). It is what makes the piece one film instead of separate slides. Name it and say what it becomes in each scene.
- **The spectacle beat**: one exaggerated moment, timestamped, usually at the climax or the payoff, that earns the piece: the hero word slams in 1.35 to 1.5 times oversized with a three-frame shake and a chromatic split that resolves in 0.2 s; a counter lands and detonates into a shockwave or a seeded particle burst; the logo completes with a bloom flash and a light sweep; a ring finishes and flashes out past the frame. Exactly one per video (a 60-second piece may have a smaller one per act). Everything else stays disciplined so this lands.
- **Scenes**, one per row: start, length, the one idea, **what the viewer sees** (the picture you are making, in a sentence), the **frame** (how the canvas is composed: full-bleed photo, one giant word, centred object, diagram across the frame, split, grid, off-centre close-up…; no two neighbouring scenes alike), the on-screen words if any (exact words; many scenes need none, since the narration carries the words), the **shot** (a blueprint name from `know-how/hyperframes-animation/blueprints-index.md`, or "custom"), the **signature move**, the **camera** (push-in, pull-back, pan, orbit, drift, shake), the **transition out**, the narration line, and the assets it needs.
- **Narration script** if there is a voiceover: short sentences, about 2.5 words per second, so a 30-second video holds about 75 words.

Pace: the hook lands in the first 1.5 seconds: the first words (or the first unmistakable picture of the subject, with its name) are on screen by 1.0 s, never a silent photo for the first three seconds. Cut on the narration, not on a metronome: a scene lasts as long as its sentence, usually 2 to 5 seconds, and lengths vary (a 1.5-second punch next to a 5-second build). Inside a scene something new happens every 1 to 1.5 seconds (a word lands, a line draws, a number ticks, the camera moves on). The last 2 to 3 seconds hold the call to action, calm enough to read.

## 2. The composition contract (the rules the checker enforces)

- One root `<div id="root" data-composition-id="main" data-start="0" data-duration="N" data-width="W" data-height="H">`, directly in `<body>`. Do not wrap it in a `<template>`. The root's `data-duration` is the video's length; set it to `video.json`'s `seconds`.
- Everything that appears and disappears is a **clip**: an element with `class="clip"`, `data-start` (seconds) and `data-duration`, and a `data-track-index`. The framework shows and hides clips by time. Never tween a clip's `visibility`, `display` or `autoAlpha`; fade a child or the clip's `opacity`.
- Don't put layout on the `clip` class itself (`.clip { position: absolute; inset: 0 }`): every timed element is a clip, and a rule like that stretches overlays and badges over the whole frame. Size and place each scene or overlay through its own class or id.
- **Transition panels are clips.** A wipe, cover, flash or curtain that fills the frame exists only while it moves: give each one `class="clip"` with its own `data-start` and `data-duration` around its move (as the skeleton's `#wipe` does), or make its resting CSS off-screen. Never leave a full-frame panel in the page whose CSS covers the canvas and that is only moved away by a tween starting later: everything before that tween renders as a blank colour. Jarvis watches every draft for blank stretches.
- Clips may sit inside non-clip wrappers (a camera, a world). Mark only purely decorative things that are meant to move past their box (drifting light, a texture, a wipe panel, a gloss, one giant word deliberately cropped by the frame) with `data-layout-allow-overflow`. **Never mark a camera, a world or a scene**: the mark also hides every word inside it from the check that catches text running off the canvas. A camera moving past its box only gives harmless warnings.
- Words split into spans keep their spaces: `Pick <span>one real task</span> this week`, never `Pick<span>one real task</span>this week`, which renders as "Pickone real taskthis". The same goes for words built in JavaScript: join them with spaces or give each word span a right margin.
- **One timeline**: `const tl = gsap.timeline({ paused: true })`, built completely, then `window.__timelines["main"] = tl` at the end. Never call `tl.play()`.
- Starting states go inside the tween: `tl.fromTo(el, { opacity: 0, y: 60 }, { opacity: 1, y: 0, duration: 0.8 }, atSeconds)`. When a **second** `fromTo` targets the same element, give it `immediateRender: false` (or animate a wrapper instead), or seeking breaks. Never put a CSS `transform` on an element you also animate with GSAP, and never centre with `transform: translate(-50%, -50%)` on something you animate; use flex or `inset: 0`.
- Animate transforms and paint: `x`, `y`, `xPercent`, `yPercent`, `scale`, `rotation`, `skewX`, `opacity`, `clipPath`, `filter`, `strokeDashoffset`, `color`, `backgroundPosition`. Do not tween `width`, `height`, `top`, `left`, `letterSpacing` or `fontSize` (they snap to whole pixels and stutter); scale, mask or clip instead.
- Deterministic only: no `Date.now()`, no `performance.now()`, no unseeded `Math.random()` (use a small seeded generator, e.g. `let s = 7; const rand = () => ((s = (s * 16807) % 2147483647) / 2147483647);`), no `fetch`, no `setTimeout`/`setInterval`/`requestAnimationFrame` for visuals. `repeat: -1` is allowed only under a finite root `data-duration`; prefer a finite `repeat`.
- Text written by `onUpdate` (a counter) must be a pure function of the tween's value, so any frame can be rendered on its own.
- No `<br>` in sentences; let text wrap inside a `max-width`. Every element that is scaled or rotated must be block-level or `inline-block` and sized (not a plain inline `<span>`).
- Ids are unique across the page. Give sections readable ids (`s1`, `s1-title`).
- The root and the scenes use `width/height: 100%` or `inset: 0`; the canvas size comes from the root's `data-width`/`data-height`, so don't hardcode pixel sizes on `#root`.
- Warnings called `nested_structure_needs_subcomposition` are fine to ignore. Errors (`✗`) are not.

## 3. Assets: local files only

- Every image, video, audio and font is a file inside this folder (`assets/…`), referenced by a relative path. No `http://` URLs for media, no stock-photo links, no CDN scripts. The only script is `vendor/gsap.min.js` (and `vendor/three.min.js` for 3D).
- Use the pictures you are given exactly as they are. A logo is never redrawn or recoloured. A photo or cut-out with a transparent background (PNG) is placed as an `<img>` with `alt`, a fixed `width`/`height` and `object-fit`, and it should move: a slow push, a parallax drift, a mask reveal, a tilt.
- If the brief needs a picture you were not given, make it with CSS shapes, gradients, patterns or inline SVG (icons, diagrams, devices, charts, abstract scenes), and say so in your report. Inline SVG is your best friend: every path can draw itself and every shape can move. Never leave an empty box.
- Fonts: use system stacks (`ui-sans-serif, system-ui, sans-serif`, `ui-serif, Georgia, serif`, `ui-monospace, monospace`). A named font such as "Poppins" needs an `@font-face` that points to a file in `assets/fonts/`; without one the checker fails the page.
- **Audio**: a voiceover or music track is `<audio id="voiceover" src="assets/voiceover.mp3" data-start="0.2" data-duration="11.5" data-track-index="8" data-volume="1"></audio>`. Every `<audio>` needs an `id`, or the render is silent. Never add `crossorigin`, and never call `.play()`. Do not add an `<audio>` for a file that doesn't exist (not even inside an HTML comment: the checker reads comments too). Music under speech: `data-volume="0.2"` on the music element. Sound effects (whooshes on transitions, hits on slams) are separate `<audio>` clips placed on the exact second of the move.
- **Video clips** (footage the user gave): `<video id="a-roll" class="clip" src="assets/clip.mp4" muted playsinline data-start="0" data-duration="5" data-track-index="0"></video>`. Keep timing on the video itself, not on a wrapper that also has `data-start`.
- **Catalog items** (`requests.json` → `blocks`): ask for overlays and self-contained moments, not for the transitions between your scenes. Good asks: `grain-overlay`, `light-leak`, `organic-light-leak-overlay`, `editorial-flash-overlay` (texture and light over the whole piece), `data-chart`, `apple-money-count`, `bar-chart-race`, `code-typing`, `logo-outro` (a whole moment you then restyle). The catalog's transition items (`whip-pan`, `transitions-*`, `zoom-through-transition`) are standalone demos with their own placeholder content; read them for technique, then build the move yourself on your own scenes. Installed files may load GSAP or fonts from a CDN: point those at `vendor/gsap.min.js` and the system font stacks before you use them.

## 4. The motion system (Motion: high)

Read `know-how/hyperframes-animation/SKILL.md`, then for each scene the blueprint you named (`know-how/hyperframes-animation/blueprints/<id>.md`) and the rules it leans on (`rules-index.md`). For transitions read `know-how/hyperframes-animation/transitions/overview.md` and `catalog.md`. These are proven recipes; use them instead of inventing weaker versions.

### 4a. Every scene is its own design

The quickest way to make a slideshow is to design one scene and refill it: a label, a headline, a subline on the left, a picture on the right, nine times. Don't.

- **No scene template.** Don't build scenes from one shared layout (the same wrapper with an eyebrow, a title, a subtitle and a "visual" slot, filled in a loop or copied per scene). Write each scene's markup and CSS for what that scene shows. Shared pieces are fine for the background, a small brand mark and helper functions, never for the scene layout.
- **Change the frame every scene.** Rotate through very different compositions: full-bleed image or footage, one huge word or number filling the screen, a centred object with things orbiting it, a diagram or map spanning the whole canvas, a UI being used, a grid or wall that assembles, a split screen, an extreme close-up that pulls back, type set along a path or stacked across the frame. Text-left and picture-right may appear once at most.
- **Social videos read with the sound off.** Portrait and square videos autoplay muted in feeds: the hook, the offer and the call to action must be carried by designed on-screen words as well as the voice (not by captions). Landscape explainers can lean on the voice more.
- **Words are optional.** A scene with no on-screen text, just a picture doing something while the voice speaks, is often the strongest. When there are words, they are part of the picture (huge, layered with the image, moving with the camera), not a caption block in a corner. Don't number scenes ("01 / …") or put a kicker label on every scene.
- **Fill the frame.** The subject of a scene takes most of the canvas: a card, window or object at least two thirds of the frame width or height, or bigger and cropped by the edges. A small card floating in an empty background is the second slideshow tell. To show an app, an email or a spreadsheet, put the camera *inside* it: frame the one line, number or button that matters at readable size, then travel to the next.
- **Size headlines by their longest word.** The longest word must fit within 84% of the frame width at its largest scale (including any camera push): on a 1080px-wide portrait frame a heavy display face fits about 900 / (0.62 x letters) px, so a 10-letter word like ORIGINATED tops out near 145px. Break onto two lines before shrinking below the readable size.
- **Each picture appears once.** A generated photo or illustration is used in one scene, unless the storyboard calls it a deliberate callback with something new happening; returning to the same photo for seconds with nothing new reads as filler.
- **Every word stays inside the frame.** Readable text keeps at least 8% from every edge through the whole camera move, not only when the move ends; a push-in that crops a headline is a bug. Only one oversized decorative word may bleed off on purpose.
- **Design for H.264, not for a web page.** A 1px border or a 0.06-opacity shadow disappears after compression. At 1920x1080 use borders of 3 to 4px or none, shadows of at least 0.25 opacity with real offset, decorative layers at 0.15 opacity or more, and cards that contrast strongly with what is behind them (not white on cream with a hairline).
- **Letters never touch.** Tight tracking makes big type look designed until the glyphs collide: keep letter-spacing at -0.04em or looser on heavy weights, and leave clear space between a number and the label beside it ("+312%" and "YoY" must not overlap).
- **Every word on screen is readable.** At least 40px on a 1920x1080 canvas (36px at 1080 wide), including text inside mock-ups, cards, tables and code. If a mock-up needs smaller text to fit, it is too much mock-up: show less of it, closer.
- **Contrast between scenes.** Alternate the ground: a dark scene, a full-bleed colour scene, a light one, a photograph. Not the same white card on the same background every time. Change scale too: an extreme close-up, then a wide view.
- **No standing decoration.** A line, a dot or a motif that sits in every scene and does nothing is clutter. A recurring element must act: carry the eye into the next scene, draw the connection, become the next shape.
- **A label arrives with its value.** Never show placeholder labels waiting for their content (three cards reading "DAY" before the 3, 10 and 30 land): bring the value first or together with its label. The same goes for a box, pill or panel that holds words: it is never on screen empty. Its words arrive within 0.2 seconds of it, or the box is drawn around words already there.
- **Make the thing, not a sign for it.** If the narration says robots, show a robot arm move. If it says faster, race two bars. If it says connected, draw the connections. Build it from SVG, CSS, generated pictures and the media Jarvis fetched.
- **One world, many views** is welcome (a signal thread that runs through every scene, one palette, one camera language); one layout is not.

### 4a+. The cinematography contract (every video with three or more scenes)

HyperFrames' authors found that more animation did not stop their multi-scene films looking like slides; a cinematography contract did:

- **One continuous camera journey through one world**, with a dwell-and-sweep rhythm: the camera sweeps to a region, dwells 1.5 to 2.5 seconds on its hero moment while the world keeps resolving (counters tick, labels stamp, the spine keeps drawing), then accelerates away. The camera rests; the film never freezes.
- **Scenes change by arriving, not by cutting.** The next region is already visible at the edge of the frame before the camera reaches it; the previous one leaves by parallax, not by fading out. Hard cuts happen only at seams you planned and named in the storyboard.
- **No slideshow tells**: no cluster of elements that fades in centred, sits, and fades out; no card floating in the middle of an empty background; no scene that could be screenshotted as a slide.
- **Density control**: one focal element at display scale per moment, with three depth layers (background, content, one foreground) moving at different rates under the camera.

### 4b. Premium motion (HyperFrames' own rules)

Every move must finish the sentence "this moves because…": it directs attention, carries continuity, shows a change, or expresses the brand's character. Decoration that can't finish it is the mark of cheap motion.

1. **Nothing ever fully stops.** Every hold keeps an ambient idle: a 1 to 2% breathing scale, a slow drift, a shimmer, a status dot pulsing on a 1-second beat. A frozen frame is the biggest cheap-motion tell.
2. **The camera is an actor.** One continuous camera move per scene (a 4 to 8% push-in, a slow orbit, a parallax pan) that eases gently and never decays to a dead stop.
3. **Overlapping action.** No two elements share a start or an end time. Stagger arrivals at short, irregular offsets (around 0.09 s), starting the next while the last is still settling. Delay supporting elements, never the focal one. Stagger what is arriving, not what is already there.
4. **Compound properties only when they tell one story.** Move plus fade reads as "arriving". Rising while rotating while scaling makes conflicting claims; pick one. Ease `out` for entrances, `in` for exits and for impacts (slams, stamps), `inOut` for repositioning something already on screen.
5. **Anticipation before the big moves.** A slam, a whip-pan or a launch reads stronger with a small wind-up first: 2 to 4 frames moving the other way (a word dips 3% before it slams, the camera eases back 1% before it whips). Use it on the few big moves, not on everything.
6. **Overshoot and follow-through on physical things.** A card or panel can overshoot on `back.out` and settle, with its shadow resolving a frame later. **Never overshoot a number**: a counter that passes its value reports a false figure.
7. **Depth planes.** The far background moves at about 0.2 times the camera, the content at 1 times, and at most one large foreground element at several times (blurred, about 22px) that crosses in front of the content. Occlusion proves depth better than blur alone.
8. **Pace by genre, and vary the energy.** Showreel and promo beats run 1.1 to 2 seconds; explainers hold longer. Use dwell-and-sweep: the camera sweeps between moments, dwells 1.5 to 2.5 seconds on a hero moment while the world keeps ticking (counters, labels, pulses), then accelerates away. Quiet stretches make the loud ones land.
9. **Carry something across every cut.** A hero prop that stays on screen, a match cut, a persistent motif or line, a shared motion direction, the beat. No hard cuts except at the seams you planned.
10. **Handmade texture stays reproducible.** Jitter, wobble and stop-motion come from a seeded generator, quantised on the frame index (hold each position for exactly two frames), never `Math.random()`.

**Camera: nothing is ever dead still.**
- Every scene sits inside a camera wrapper that moves for the whole scene: a slow push-in (`scale` 1 → 1.06), a pull-back that settles, a lateral drift, or an orbit. Big moments get a punch-in or a 0.25-second shake on impact.
- Prefer one continuous **world**: lay scenes out on an oversized canvas and fly the camera between them (whip-pan with a brief blur, a dive into an element, a pan across stations). See the skeleton's `#world`, and the blueprints `camera-journey`, `spatial-pan-stations`, `zoom-out-workspace-reveal`.
- **The picture always covers the frame.** A full-bleed photo or scene that the camera pans or drifts must be scaled up enough to cover the move: the extra size on each side (scale minus 1, times half the frame) must be larger than the farthest the camera travels that way. A 3% push-in leaves about 29px of spare height on a 1920px frame, so a 65px pan shows a strip of empty background. When in doubt, scale 1.12 and pan less.
- The camera eases like a real one: `power2.inOut` / `expo.inOut` for moves, `none` or `sine.inOut` for drift. No springs on the camera.

**Depth and a living background.**
- One background clip spans the whole video: gradient light that drifts, a grid or pattern that slides, soft orbs, grain. It ties the scenes together and keeps every frame alive.
- Layer each scene: background, midground, foreground. Layers move at different speeds (parallax), so the frame has depth.

**Kinetic typography: the words are the animation.**
- Split headlines into words (`inline-block` spans) and give them **different** entrances in the same line: a slam (`scale` 1.8 → 1, `expo.out`), a rise out of a mask (`yPercent` 110 → 0 inside an `overflow: hidden` wrapper), a skewed slide, a tip-up with rotation, a letter cascade. Swap a word in place for contrast ("slow" → "instant").
- **Sync to the voice.** If `assets/voiceover.words.json` exists, every key word lands on its `start` time. Build a small lookup (`const at = (word) => …`) and position tweens from it rather than guessing.
- Emphasis after landing: a marker sweep behind a word, an underline that draws, a colour flip, a scale pulse, a gradient sweep through the letters.

**Graphics that build themselves.**
- SVG lines, arrows, outlines and diagrams draw on (`pathLength="1"`, `stroke-dasharray="1"`, tween `strokeDashoffset` 1 → 0).
- Numbers count up (a value proxy + `onUpdate`, `font-variant-numeric: tabular-nums`) while a ring, bar or gauge fills beside them.
- Lists and grids assemble from different directions with a tight stagger (whole group within about 0.5 s); icons pop with `back.out(2)`.
- Connectors grow between nodes, dots orbit, cursors click, fake UIs change state, particles drift (seeded). Show the idea happening; don't just name it.

**Transitions: the handoff is part of the story.**
- Every scene change is a real transition where the outgoing and incoming scenes move **at the same moment**: whip-pan, push, zoom-through (the camera dives into an element that becomes the next scene), clip-path or iris reveal, panel or block wipe, match cut (a shape carries over), flash, blur snap. Outgoing content stays fully visible until the transition starts; the transition is the exit.
- Pick one primary transition (most cuts) plus one or two accents, matched to the energy (overview.md); spend the boldest one on the climax. A plain crossfade appears at most once, and only to wind down.
- Put a whoosh or hit sound effect on the big transitions when you have one.

**Easing and timing.**
- Arrivals: `expo.out`, `power4.out` (0.35 to 0.7 s). Pops: `back.out(1.6–2.2)`. Fast exits: `power4.in` (0.25 to 0.4 s). Idles and drift: `sine.inOut`. Linear (`none`) only for camera drift, progress and scrolling.
- Vary rhythm: quick hits against slower builds. Do not give everything the same 0.6-second `power2.out` fade-up; that is exactly the slideshow look this skill exists to avoid.

**Density targets** (Jarvis measures these after you compose and sends the video back if it falls short):
- At least 1.5 tweens per second of video, across at least 6 different properties from section 2's list.
- At least one camera wrapper that moves in every scene, and a background that moves for the whole video.
- Fade-and-slide (`opacity` with `x`/`y` only) is at most half of all entrances; the rest use scale, rotation, skew, masks (`clipPath`, `yPercent` inside a mask), drawing (`strokeDashoffset`) or filters.
- At least two different transition styles in any video with three or more scenes.

## 5. Motion: calm

Only when the brief asks for it. Keep the same craft with less energy: slow camera drift and push-ins still run in every scene, transitions are blur crossfades, focus pulls, light leaks and slow wipes (0.6 to 1.0 s, `sine.inOut`), text enters by line with masks rather than slams, and there is more air between beats. Still no static frames and no stack of identical fade-ups. Jarvis relaxes its density measure for a storyboard that says `Motion: calm`.

## 6. Design

- **Direction**: pick one that fits the subject and the tone words in the brief, and commit (bold and kinetic, editorial, tech and luminous, warm and organic, luxury, playful). Write the palette, the two fonts and the motion language at the top of `STORYBOARD.md`.
- **Colour**: tokens on `:root` (`--bg`, `--surface`, `--text`, `--muted`, `--accent`, optionally `--accent-2`). One lead accent. Text contrast at least 4.5:1 on its real background (the checker tests this). Never light-grey text on white.
- **Type for a screen across a room**: headlines 120 to 200px (kinetic hero words can go bigger), supporting text 44 to 64px, nothing below 40px, in a 1920x1080 frame. For portrait 1080x1920 use 100 to 160px headlines and 48 to 60px text. Two typefaces at most; heavy weights carry kinetic type well. Headlines have `text-wrap: balance` and a `max-width`.
- **Readable while moving**: a phrase the viewer must read is fully settled and still-readable for at least 1.2 seconds (plus 0.3 s per 5 words beyond 5) before the next thing replaces it; motion continues around it (camera, background, accents), not through it.
- **Safe area**: keep all text and logos at least 8% from every edge. For portrait/social formats keep the bottom 18% and top 10% clear of important text (apps draw buttons there).
- **Layout**: one focal point per moment, and a different composition every scene (section 4a). Use the whole frame and its edges: things enter from and leave toward them, and big shapes can bleed off them.
- **No captions.** Don't add subtitles or a caption track, even with a voiceover, and even if a HyperFrames workflow you were given says to: the person finds them ugly. The voice carries the words; on-screen words are designed moments (section 4a). Add captions only when the brief explicitly asks for captions or subtitles, and then follow `know-how/hyperframes-registry` caption components rather than a dark box at the bottom.
- **Ending**: the end card fits wholly inside the safe area (a ring, logo or badge is never cropped by an edge) and holds for about 3 seconds, no more; the last 2 to 3 seconds hold the call to action (what to do, where to find it) and the logo; the camera and background keep breathing, but the text is still.
- Avoid: the same layout scene after scene, a label-headline-subline block in every scene, a purple-to-blue gradient on every background, emoji as icons, walls of text, tiny grey text, everything entering with the same fade-up, a plain crossfade between every scene, scenes of equal length cut on a metronome, anything flashing faster than 3 times a second.

## 7. 3D scenes (when the brief asks for 3D or it clearly serves the story)

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

No `requestAnimationFrame`, no `OrbitControls` interaction, no downloaded models or textures (draw textures on a canvas). Always keep the root `data-duration`. CSS 3D (`perspective` on a wrapper, `rotationY`/`rotationX` on cards) is a lighter way to get depth and works everywhere.

## 8. Before you finish

- [ ] `video.json`'s size and length are on the root; scenes add up to the length.
- [ ] Every clip has `data-start` and `data-duration`; nothing ends after the root's duration.
- [ ] Every file referenced exists in this folder; no remote URLs; every `<audio>` has an `id` and a real file.
- [ ] Text is large, inside the safe area, readable (contrast) and holds still long enough to read.
- [ ] The hook lands in the first 1.5 seconds; the last scene says what to do next.
- [ ] Every scene is designed on its own: no shared scene layout, a different frame each time, words only where they earn it.
- [ ] Motion: every scene has a moving camera, the background moves, entrances vary, every scene change is a real transition, key words land on the voice, and the density targets in section 4 are met (or the storyboard says `Motion: calm`).
- [ ] `STORYBOARD.md` lists the scenes with their shots, moves and transitions, the palette, fonts and motion language, and what you invented.

Then write the report to the path you were given: `## What I made` (scenes and timing), `## Sample content` (anything invented), `## Left for you`. Your final spoken message is two or three plain sentences on what the video is and how long it is. Do not claim you rendered it: Jarvis does that.

*The composition rules here are distilled from HeyGen's open-source HyperFrames documentation (Apache License 2.0).*
