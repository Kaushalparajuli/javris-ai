---
name: "Website design"
description: "Design and build a polished, distinctive, accessible website from a brief: design direction, type, colour, layout, components, copy and a quality checklist. Use for any website, landing page or small web app."
---

# Website design

Build sites that look like a good designer made them for this one client, not like a template. Follow this in order. Open `references/tokens-and-base.css` and `references/page-skeleton.html` first: start from them instead of a blank file, and change them to fit the brief.

**Read `references/lessons.md` before you design, if it exists.** It lists what Jarvis's visual director found wrong in earlier sites (headlines wrapping to five lines, badges poking past the edge, dead zones in cards, low-contrast emphasis). Every rule in it was learned from a real mistake: don't repeat them. After you finish, the visual director will render your site at desktop, tablet and phone sizes, review it like a design director, and ask for fixes, so build it as if that review is coming.

## 1. Understand before you draw

Read the whole brief (every file in the folder, the markdown most of all). Then write five lines at the top of `DESIGN.md` in the project folder:

- **Who it's for** and what they want to do on the site.
- **The one action** the site should lead to (call, donate, book, buy, apply, contact).
- **Three tone words** (for example: warm, trustworthy, playful) taken from the brief, not invented.
- **Language**: if the brief is in Nepali or Hindi, the site's text is in that language (see Typography), unless the brief says otherwise.
- **Content inventory**: what real content exists (name, mission, services, numbers, testimonials, contacts, colours, logo) and what you must write yourself.

Keep `DESIGN.md` updated with the final colours, fonts and decisions, so the next person can continue.

## 2. Pick one design direction and commit

Choose a single direction that fits the tone words, and let every decision follow it:

- **Editorial**: big serif headings, generous whitespace, thin rules, restrained colour. For consultancies, writers, institutions.
- **Warm and organic**: soft off-white background, rounded shapes, earthy greens/golds, hand-drawn SVG accents. For food, wellness, NGOs, craft.
- **Modern minimal**: tight grid, one strong accent, crisp sans-serif, lots of air. For software, studios, finance.
- **Bold and playful**: large type, saturated colour blocks, chunky shapes, friendly motion. For kids, events, consumer brands.
- **Luxury**: dark or deep neutrals, fine type, metallic accent used sparingly, slow gentle motion. For hotels, jewellery, premium services.

If the brief names colours or a brand, use them as the starting point and build the rest of the system around them.

## 3. Colour

- A system of **one brand colour, one accent, and a neutral scale** (5 to 7 steps from near-white to near-black). Roughly 60% neutral, 30% brand, 10% accent.
- Define colours once as CSS variables (see the tokens file) and never hard-code a colour elsewhere.
- Body text contrast at least 4.5:1, large text 3:1. Check pairs on their real backgrounds. Never put light-grey text on white.
- Tint the neutrals slightly toward the brand colour (a warm or cool grey) so the page feels cohesive.
- Use colour to guide attention: the primary button is the most saturated thing on the screen.
- Provide dark mode with `prefers-color-scheme` only if it can be done well; otherwise skip it.

## 4. Typography

- At most **two font families**: one for headings, one for text (or a single family in two weights). Default to a good system stack so nothing needs the network; a Google Fonts `<link>` is fine when the design needs a distinct face, always with a fallback stack.
- Use a **fluid type scale** with `clamp()` (the tokens file has one). Body 16 to 18px, line-height 1.6 to 1.75; headings line-height 1.1 to 1.25 with slightly negative letter-spacing for large sizes.
- Keep text lines to about **60 to 72 characters** (`max-width: 65ch`).
- Make hierarchy obvious: a clear size and weight step between heading, subheading, body and caption. Use at most three weights.
- **Nepali and Hindi (Devanagari):** use `lang="ne"` or `lang="hi"` on `<html>`; font stack `"Noto Sans Devanagari", "Mukta", "Kohinoor Devanagari", system-ui, sans-serif` (for headings `"Noto Serif Devanagari"` is good); line-height 1.7 to 1.9 because the script has tall marks; no letter-spacing, no uppercase transforms, no italics; make headings a touch larger than Latin ones.

## 5. Space and layout

- Use an **8px spacing scale** only (4, 8, 12, 16, 24, 32, 48, 64, 96, 128). Section padding is generous: `clamp(64px, 10vw, 128px)` top and bottom.
- One **container** (`max-width: 1120px` to `1200px`, side padding `clamp(16px, 4vw, 32px)`), CSS Grid for layouts, Flexbox for rows of items.
- Don't centre everything. Mix a left-aligned text column with an image or shape on the other side; alternate section layouts to keep a rhythm; use full-bleed colour bands to separate sections.
- Group related things closely and separate groups clearly. Align edges. When in doubt, add more whitespace, not another border.
- Build **mobile first**. Breakpoints near 640, 960 and 1200px. Nothing may scroll sideways at 360px wide.

## 6. Structure of the page

Every page: skip link, `<header>` with logo and nav (collapses to a menu button on mobile), `<main>`, `<footer>` with contact, links and the year.

**Landing page / organisation / consultancy**, in this order, dropping what the brief doesn't support:
1. **Hero**: a headline that says what it is and who it helps in under 12 words, one supporting sentence, one primary button and one quiet secondary link, and a strong visual (SVG shape, pattern, or large type), not an empty box.
2. **Trust strip**: numbers, logos or a one-line proof.
3. **What we do**: three or four items maximum, with real descriptions. Don't make nine identical cards.
4. **How it works / approach**: three numbered steps.
5. **Mission / story**: short, human, specific.
6. **Proof**: testimonials with a name and role, results, or projects.
7. **FAQ** using `<details>`/`<summary>`.
8. **Final call to action**: repeat the one action.
9. **Footer**.

Other site types: **portfolio** (work grid first, short about, contact), **restaurant/cafe** (hero, menu highlights, hours and location, booking), **event** (date and place in the hero, schedule, speakers, register), **shop** (product grid, clear price and buy button, trust details).

Add more pages only when the brief has more (about, services, contact). Share one `css/styles.css` and `js/main.js`. Keep the nav identical on every page.

## 7. Components (build them properly)

- **Buttons**: 44px minimum height, clear padding, a visible `:hover`, `:active` and `:focus-visible` state, one primary style and one secondary style. Never grey text on a coloured button without checking contrast.
- **Cards**: use sparingly. A card is a container with one idea; don't box everything. Consistent radius and shadow from tokens.
- **Navigation**: sticky header with a subtle blur or solid background, the current page marked with `aria-current="page"`, a menu button with `aria-expanded` on mobile.
- **Forms**: a visible `<label>` for every field, large inputs, clear focus ring, helpful error text, a submit button that says what it does. A static site can't send mail: use a `mailto:` or a clearly marked placeholder action and say so in the report.
- **Accordions**: native `<details>`.
- **Testimonials**: quote, name, role; no star clip-art.
- **Footer**: organised in 2 to 4 columns, contact details first.

## 8. Images, icons, illustration

- Don't link to stock photo URLs; they break. Make the visuals yourself: **inline SVG** (shapes, blobs, patterns, simple line icons you draw with `viewBox="0 0 24 24"` and `stroke="currentColor"`), CSS gradients used with restraint, and typographic layouts.
- Never use emoji as icons. Use one icon style (stroke width and corner style) across the site.
- Every `<img>` has meaningful `alt` (empty `alt=""` for decoration), `width`/`height` or `aspect-ratio`, and `loading="lazy"` below the fold.
- If a real logo or photo exists in the folder, use it. If the brief needs a photo you don't have, make a tasteful SVG placeholder and mention it in the report.
- Add a favicon (an inline SVG data URI is enough) and Open Graph meta tags.

## 9. Motion

- Subtle and purposeful: 150 to 250ms transitions with `ease-out`, a gentle fade/slide-up when sections scroll into view (IntersectionObserver), hover lifts on cards and buttons.
- Wrap all animation in `@media (prefers-reduced-motion: no-preference)`. No auto-playing carousels, no parallax that fights scrolling, nothing that blocks reading.

## 10. Accessibility (not optional)

Semantic landmarks, one `<h1>`, headings in order, `lang` set, visible `:focus-visible` outlines, keyboard-operable menu, contrast as above, tap targets at least 44px, `alt` text, form labels, `aria-label` on icon-only buttons, no information carried by colour alone, `prefers-reduced-motion` respected.

## 11. Copy

- Write real, specific copy from the brief. Never leave "Lorem ipsum" or "Your text here". If the brief lacks something (a testimonial, a statistic), write believable sample content, make it obviously safe (no real person's name or fake statistics presented as fact), and list it in the report under "Sample content".
- Headlines say the benefit, not the category. Buttons start with a verb ("Book a visit", "Join the programme"). Cut filler words.
- Match the language and register of the brief.

## 12. Avoid the generic look

Don't ship: a purple-to-blue gradient on white, three identical icon-in-a-circle cards, emoji icons, everything centred, glassmorphism on every surface, thick coloured borders on every card, tiny grey text, stock-photo hero text clouds, five different shadows, more than two fonts, or a hero with no visual. If a section could belong to any company, rewrite it for this one.

## 13. 3D and motion scenes (only when the brief asks for 3D, or it clearly serves the subject)

- If `vendor/three.min.js` exists in the site folder, Three.js is available. Load it with a plain `<script src="vendor/three.min.js"></script>` before your own script. It defines the global `THREE`, including `THREE.OrbitControls`. Do not use `import`, import maps, modules or a CDN for it, and don't edit that file.
- Put the scene in its own `<canvas>` (a hero or a section background). Keep all text and navigation in normal HTML on top, so the page is complete without the scene.
- Build visuals from code: geometry, procedural textures drawn on a canvas, particles, lights. No model or texture downloads.
- Size the renderer to its container, handle `resize`, cap the pixel ratio at 2. Use `requestAnimationFrame`, pause when the tab is hidden or the canvas is off screen, and keep it light enough for a laptop and a phone (thousands of particles, not hundreds of thousands).
- Respect `prefers-reduced-motion`: draw one still frame instead of animating. Wrap renderer creation in try/catch and show a styled static fallback if WebGL is unavailable.
- `aria-hidden="true"` on a decorative canvas. Dragging to rotate must not block scrolling on a phone: use `touch-action: pan-y`, or enable the controls for desktop pointers only.

## 14. Self-review before you finish

Open every HTML file in your head (or by reading it) and check:
- [ ] Every link, image, stylesheet and script path resolves; no dead `#` links except intentional ones.
- [ ] Colours, fonts and spacing all come from the tokens; no stray hard-coded values.
- [ ] Looks right at 360px, 768px and 1280px: nothing overflows, text isn't tiny, buttons reachable.
- [ ] Headline, primary action and contact details are obvious within five seconds.
- [ ] Contrast, focus states, alt text, labels, one `<h1>`, `lang` set.
- [ ] No lorem ipsum, no placeholder boxes, no broken emoji icons.
- [ ] `DESIGN.md` lists the direction, palette, fonts and what was invented.
- [ ] The site opens by double-clicking `index.html` (relative paths only).
- [ ] If there is a 3D scene: it draws, resizes, pauses when hidden, and the page still works without it.

Finally, write the report as instructed: what you built, the sample content you invented, how to open it, and what's left for the user.
