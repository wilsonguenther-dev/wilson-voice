# Pixel-Art Craft & Standards — for Yappy, the hand-coded chick companion

> **What this is:** a self-sufficient craft dossier for making our animated pixel chick
> read like *real* pixel art, with the **talking mouth/beak** as the priority fix. It is
> written to be coded from directly. Every technique is tied to a real, cited source and,
> where it matters, to the exact place in our current code that needs to change.
>
> **Our current build (the thing being critiqued):**
> `docs/prototypes/yappy.html` renders the chick to a **64×58** offscreen buffer
> (`PW=64, PH=58`) and scales it up with `imageSmoothingEnabled=false` and CSS
> `image-rendering:pixelated` — so the pipeline is already correct "true pixel art."
> The mouth is driven by `desktop/src/pill/mouth.ts` (`MouthDriver`), which already turns
> live mic RMS into a smoothed `0..1` jaw-open with gate/gamma/gain and asymmetric
> attack/release. **The renderer's job is only to turn that number into a good mouth.**
>
> All links verified 2026-07-23.

---

## 0. TL;DR — the diagnosis and the fix, up front

**Why the mouth looks awkward right now.** In `yappy.html` the beak is drawn as two
*disconnected horizontal bars* with **transparent space between them**:

```js
// current beak (yappy.html ~line 145)
const by=cy+6, bo=Math.round(P.beak*4);
octx.fillStyle=C.beak;  rct(cx-1,by,3,1); rct(cx,by+1,1,1);      // upper: a 3px bar + 1 point
if(bo>1){ octx.fillStyle=C.tongue; rct(cx,by+1+Math.floor(bo/2),1,1); } // 1 tongue pixel floating in the gap
octx.fillStyle=C.beakD; rct(cx-1,by+1+bo,3,1); rct(cx,by+2+bo,1,1); // lower: a 3px bar that just slides down
```

When it opens, the lower bar slides down by up to 4px and the gap between the two bars is
**filled with the yellow body color**, not a mouth interior. So an open mouth reads as
"two orange hyphens with a floating pink dot," never as a beak. Three separate problems:

1. **No dark interior.** An open mouth must reveal a dark cavity. Leaving the body color
   showing through is the #1 reason it looks wrong.
2. **No hinge.** Both mandibles are loose bars; there's no fixed corner they pivot around,
   so it doesn't read as a *jaw opening*. Real beaks: the lower mandible drops around a
   fixed hinge while the upper stays put.
3. **No seam / no separation from the body.** The orange beak sits on a yellow body with
   no darker edge, so even *closed* it reads mushy.

**The fix (detailed in §4):** make the beak a small solid **wedge/diamond** at rest, split
it along a fixed **hinge seam**, drop only the lower mandible, and **fill the reveal with a
dark interior color** plus one tongue pixel. Drive it with **3 discrete mouth frames**
(closed / ajar / wide) quantized from `MouthDriver`'s `0..1` — *not* continuous sub-pixel
scaling, which just strobes single pixels. This is exactly how Animal Crossing, Undertale
and Kirby "talk": a small set of mouth shapes flapped in sync, not phoneme lip-sync.

Secondary craft fixes (§1–§3): add a **selective outline** in a dark warm brown (never
pure black); tighten the **palette into hue-shifted ramps**; and hand-clean the
procedural-ellipse silhouette so curves don't have jaggies. Run/idle specs in §5–§6.

---

## 1. Pixel-art hygiene / the actual craft rules

The fundamentals every serious pixel artist follows. Canonical teachers and their pages:

- **Pedro Medeiros / Saint11** (Celeste, TowerFall) — the compact tutorial series:
  [saint11.art/pixel-art-tutorials](https://saint11.art/blog/pixel-art-tutorials/) and the
  article index [saint11.art/pixel_articles](https://saint11.art/pixel_articles/).
- **Brandon James Greer (BJG)** — limited-palette + fundamentals, excellent for beginners:
  [youtube.com/@BJGpixel](https://www.youtube.com/@BJGpixel).
- **MortMort** — beginner-friendly fundamentals (now at Mojang):
  [Lospec author page](https://lospec.com/pixel-art-tutorials/author/mortmort).
- **AdamCYounis** — the "Pixel Art Class" series, the deepest theory library:
  [youtube.com/@AdamCYounis](https://www.youtube.com/@AdamCYounis).
- **Slynyrd (Raymond Schlitter)** — the "Pixelblog" series, best written reference:
  [slynyrd.com/blog](https://www.slynyrd.com/blog).
- **Derek Yu (Spelunky)** — the classic "BASICS" + "Common Mistakes" tutorial:
  [derekyu.com/makegames/pixelart.html](https://www.derekyu.com/makegames/pixelart.html).
- **Pixel Parmesan** — the definitive anti-aliasing deep-dive:
  [Anti-Aliasing Fundamentals for Pixel Artists](https://pixelparmesan.com/blog/anti-aliasing-fundamentals-for-pixel-artists).
- **Pixel Logic — A Guide to Pixel Art** (Michael Azzi), referenced across Lospec.

### 1.1 Clean lines — the "pixel-perfect line" rule

- A clean pixel line moves in **consistent, even steps** — the same run-length on every
  step (e.g. 2,2,2… or 3,3,3…). A single step that breaks the pattern is a **jaggy**: "an
  unintentional break in an otherwise smooth line or curve"
  ([Jellytempo glossary](https://jellytempo.com/pixel-art-terms-definitions-a-beginner-pixel-artists-glossary/);
  [Pixel Art Wiki: Jaggies](https://pixelart.fandom.com/wiki/Jaggies)).
- **No doubles.** A "double" is where a 1px line thickens to 2px as it turns a corner —
  it makes the line look blocky and inconsistent. On a clean 1px line every pixel touches
  its neighbours only at the corner, never side-by-side then stacked
  ([Dual Core Studio – Lines/Curves/Jaggies, Lospec #16](https://lospec.com/pixel-art-tutorials/how-to-pixel-art-tutorials-16-lines-curves-jaggies-by-dual-core-studio)).
- **No orphan / stray pixels.** A lone pixel disconnected from the form (or a single pixel
  poking out of a clean edge) reads as noise. Slynyrd's animation rule is blunt: *"a single
  orphan or misplaced pixel in a looping animation is obvious"*
  ([Pixelblog 8](https://www.slynyrd.com/blog/2018/8/19/pixelblog-8-intro-to-animation)).

**Applies to us:** our silhouette is generated by a math ellipse fill
(`ell()` in `yappy.html`, `Math.sqrt(1 - t*t)` per row). Procedural ellipses at r≈13
produce **inconsistent step lengths and single-pixel jaggies** on the curve — this is a
big part of why it reads "almost but not quite pixel art." Two options: (a) after the
procedural fill, **hand-clean the outline ring** so the curve steps are even; or (b) bake a
few hand-authored silhouette masks (closed/idle/run frames) and blit those instead of
re-deriving the circle every frame. Hand-authored beats procedural for cleanliness at this
size — every serious sprite is placed pixel by pixel.

### 1.2 Outlines — selective outline (sel-out), and never pure black

Three outline strategies ([Pixnote: Outlines & sel-out](https://pixnote.net/en/learn/outlines/);
[Ricky Han: selective outlining/AA](https://rickyhan.com/jekyll/update/2019/09/01/pixel-art-algorithm-selective-outlining-anti-aliasing.html)):

- **Hard outline** — a uniform dark line around the whole silhouette. Reads like a sticker
  / enamel charm. Great for a *cute* companion that must pop on a busy background (this is
  the Finch / Tamagotchi look).
- **No outline** — silhouette defined by color alone. Softer, but mushes on low-contrast
  backgrounds. **This is what we do now**, and on the greenish LCD pod it's part of why the
  chick lacks punch.
- **Selective outline ("sel-out")** — a *shaded* outline: darker on the shadow/away side,
  lighter or absent on the lit side. It's essentially outline-as-form; it conveys 3D and
  light direction and is the pro default for characters.

**Rule: never outline in pure black (`#000`).** Pure black outlines look harsh, flatten
form and read cheap. Use a **dark, saturated, warm-shifted** version of the body hue
instead. Our eye color `#3a2f36` is already a good warm near-black — reuse that family. A
good chick outline is a dark warm brown like `#6a3a1e` / `#733e39`, not black.

**Recommendation for us:** a **selective outline** — a 1px dark-warm ring around the whole
silhouette (so it pops on the LCD), darkest along the bottom/shadow arc and one step
lighter along the top/lit arc; interior forms (belly, wing seam) get sel-out separations
only on their shadow side, not all the way around (avoids over-lining a tiny sprite).

### 1.3 Anti-aliasing & hinting — by hand, sparingly

AA = placing a mid-tone pixel to smooth a staircase so it "counts as a half pixel when
zoomed out" ([Saint11 — Anti-Alias & Banding](https://saint11.art/pixel_art_articles/article5/)).
Rules:

- **Only AA steps longer than 1×1.** *Do not AA 45° lines or straight lines* — they're
  already clean (Saint11). The longer the step, the longer the AA strip: "a long step
  should have a proportionally long anti-alias halftone strip."
- **Match AA direction to the slope** — horizontal slope → horizontal AA; vertical slope →
  vertical AA (Saint11).
- **On tiny sprites, AA can read as noise — skip it.** Pixel Parmesan's first rule is to
  ask *"is this really necessary?"*; for ~16–40px sprites, AA on outer edges "might read as
  noise" and won't composite correctly over a changing background
  ([Pixel Parmesan](https://pixelparmesan.com/blog/anti-aliasing-fundamentals-for-pixel-artists)).
  Reserve AA for the **inside** of the sprite (interior curves, form) and leave the outer
  silhouette crisp — critical because our chick sits on a live-repainting pod.

### 1.4 Banding — the flatness trap

Banding = two color bands the same shape/length running parallel, which *reinforces* the
pixel grid and flattens the form (Saint11; Pixel Parmesan). It's also what causes **pillow
shading** (concentric bands with no light direction). Fixes: compress bands into smaller
areas, rotate the gradient direction, or vary the run-lengths so parallel edges don't line
up. **For us:** the "bottom shade band" clip fill in `drawChick` is a flat parallel band —
give it a light *direction* (shade the lower-right, leave the upper-left lit) instead of a
uniform horizontal strip.

---

## 2. Color / palette discipline

### 2.1 Hue-shifted ramps (the single biggest "pro vs amateur" tell)

From [Slynyrd Pixelblog 1 — Color Palettes](https://www.slynyrd.com/blog/2018/1/10/pixelblog-1-color-palettes):

- Build the body as a **ramp** of ~4–6 swatches, not ad-hoc picks.
- **Hue-shift as value changes.** Positive hue shift = "warming as they become brighter."
  Slynyrd uses up to **~20° of hue shift per swatch** within a ramp ("20 is about as high
  as I go"). Classic pixel-art convention: **shadows shift toward blue/purple, highlights
  shift toward yellow/warm** — never just darken/lighten the same hue (that's the flat,
  amateur look).
- **Brightness** climbs steadily across the ramp, steps getting *smaller* toward the bright
  end; "usually never starts at 0 unless I want black."
- **Saturation** should "peak around the middle and never fully go to 100 or 0" — very dark
  colors with very high saturation get "overly rich and weighty."

**Our current body ramp** (`light #ffe487 → body #ffd24a → dark #f0b028 → deep #d8951a`)
already warm-shifts toward orange as it darkens — good instinct. Tighten it to even value
steps and **add one darker rung for the outline** (e.g. `#a8600f`) so the sel-out isn't
black. For a *cute* look, keep the highlight pale and slightly desaturated (a creamy
`#fff2c4`, which you already have as `belly`).

### 2.2 Reference palettes for a warm cute chick (Lospec, hex verified)

These are professionally-tuned, harmonious palettes — pull the chick's ramp from one of
them rather than hand-mixing:

- **[Endesga 32](https://lospec.com/palette-list/endesga-32)** — *best match for a warm
  chick.* Body `#feae34` (gold) → `#fee761` (highlight); beak `#f77622`; cream belly
  `#ead4aa` / `#e8b796`; blush `#f6757a`; non-black outline `#733e39` (dark warm brown) or
  `#3e2731`. A complete, coherent chick lives entirely inside this palette.
- **[Sweetie 16](https://lospec.com/palette-list/sweetie-16)** — softer, "friendlier" 16:
  body `#ffcd75`, beak/accent `#ef7d57`, warm dark `#b13e53`, near-black `#1a1c2c`. Fewer
  colors = more restraint, very cute.
- **[Resurrect 64](https://lospec.com/palette-list/resurrect-64)** and
  **[AAP-64](https://lospec.com/palette-list/aap-64)** — bigger, if you later add scenes,
  props, or multiple pets and want a shared master palette.

**Rule of restraint:** cap the chick at **~6–8 colors total** (body ×3–4 + beak ×2 +
eye/outline + white + blush). More colors ≠ better; limited palettes are what make pixel
art read as pixel art (BJG's whole channel is built on this).

### 2.3 The mouth-interior color you're missing

Add **one dark warm color for the open-mouth cavity** — e.g. `#7a2e14` (dark red-brown) or
reuse the darkest beak rung. This is the color that fills the reveal when the beak opens
(see §4). Without it the mouth can't read.

---

## 3. Readability at small size (~40–64px tall) — baby schema in pixels

Cuteness is the **baby schema** (*Kindchenschema*): oversized head, **large, low-set
eyes**, round body, tiny nose/mouth, chubby proportions — a measurable reward response, not
taste (documented in your own `cute-character-animation-code.md`). Translating that to a
64px sprite:

- **Head is the show.** A chick should be ~55–65% head. Keep the body a round egg; the head
  dome and body can be one silhouette.
- **Eyes: big, round, dark, glossy, LOW, wide-set.** This is the highest-leverage cuteness
  lever at any resolution. Each eye ≈ 2–3px of dark mass with a **single 1px white
  catchlight**. Place them **below the vertical midline** of the head and spaced apart.
  One catchlight, not two competing ones. (Our eyes are `ell(eXc,ey,2.4,3)` at `ey=cy+1` —
  already low and big; keep, but give them a crisp 1px outline/lash so they don't blob.)
- **Mouth small and close under the eyes.** Baby schema wants a *tiny* mouth. Our beak at
  `cy+6` sits nicely just under the eyes — the position is right; it's the *shape* that's
  wrong (§4).
- **Silhouette-first.** Every legible sprite reads as a solid silhouette at 1× before any
  interior detail (Derek Yu; Slynyrd). **Squint / thumbnail test**: shrink the sprite to
  1× — if you can't tell it's a chirping chick from the black silhouette + eyes alone, the
  detail is doing work the silhouette should do.
- **Detail budget is tiny.** At 40–64px you get *maybe* 6–10 "features." Spend them on
  eyes, beak, wing, feet, one belly separation — nothing else. Cut sparkles/tongue unless
  they earn it.

---

## 4. MOUTH / speech animation — THE KEY ASK

### 4.1 How cute characters actually "talk" (it's not lip-sync)

Cute 2D characters almost never phoneme-lip-sync. They **flap between a tiny set of mouth
shapes**, driven by audio amplitude or by text scrolling:

- **Animal Crossing (Animalese):** the game reads each *letter* of the on-screen text and
  plays a tiny per-letter sound; the mouth is **texture-swapped** (open/closed) in sync —
  no geometry, just swapping a "mouth open" vs "mouth closed" image per letter
  ([Nookipedia: Animalese](https://nookipedia.com/wiki/Animalese);
  [Blender Artists: emulating AC face swaps](https://blenderartists.org/t/how-to-emulate-animal-crossings-facial-expressions-texture-swapping-the-mouth-eyes/1192051)).
- **Undertale / Deltarune:** overworld/talk sprites swap between a small number of mouth
  frames while the textbox types, with a blip SFX per character. It's a **2-ish frame mouth
  flap**, not lip-sync ([FmsDraws UT/DR spriting tutorial](https://fmsdraws.tumblr.com/post/181571525098/the-undertaledeltarune-spriting-tutorial-sort);
  [UT/DR Dialogue Compendium](https://inactivesnail.neocities.org/utdr-dialogue)).
- **Kirby:** a small library of discrete mouth states (closed dot, small "o", wide open) —
  each a *distinct shape*, swapped, never tweened.
- **Full-industry standard:** even AAA lip-sync collapses ~44 phonemes to **~8–12 discrete
  "visemes"** (Disney's 12 archetypal mouth shapes), one shape standing in for several
  sounds ([MoCap Online — Lip Sync guide](https://mocaponline.com/blogs/mocap-news/lip-sync-animation-guide)).
  The standard viseme set is: **closed (M/B/P), open (AH), rounded/O (OO/W), wide (EE),
  and neutral rest** — for a tiny companion, **3 of these is plenty**.

**Takeaway for us:** quantize `MouthDriver`'s `0..1` into **3 discrete mouth frames** and
flap between them. Do **not** continuously scale a 3px beak by a float — at this size that
just flickers single pixels frame-to-frame (a jaggy/banding strobe). Discrete frames are
both more correct *and* cheaper.

### 4.2 The "round moving mouth" the user referenced (Approach B)

The circle-that-opens-and-closes (Kirby / Deltarune blob / Pou) is a real, beloved pattern:
a **dark oval that grows** — a dot → small "o" → wide "o", with a 1px lighter inner rim.
It's the simplest possible talking mouth and extremely cute. **But for a chick it throws
away the beak**, which is the single most on-brand "yapping" signal you have ("Yap" *is*
talking; a beak *is* a mouth built for it). Recommendation: **don't replace the beak with a
round mouth — but let the *widest* beak state open so wide it approximates a round singing
"o,"** giving you the charm of the round mouth while keeping chick identity.

### 4.3 The beak done right (Approach A — RECOMMENDED)

Model a real beak: **the lower mandible drops around a fixed hinge; the upper stays put; a
dark cavity is revealed between them.** Three frames, in our 64×58 buffer coordinates
(`cx=32`, beak top row `by ≈ cy+5`). Legend: `U`=upper beak (light `#ff9f1c`),
`u`=upper rim (dark `#e07a12`), `L`=lower mandible (dark `#e07a12`), `I`=interior cavity
(dark warm `#7a2e14`), `T`=tongue pixel (`#ff5d7a`), `.`=transparent.

**Frame 0 — CLOSED** (a chunky rounded wedge; note it's a *solid* shape, with a seam):
```
by-1   . . U . .
by     . U U U .        upper mandible (light)
by+1   u u u u u        hinge seam (dark rim) — the two OUTER pixels are the pivot
by+2   . L L L .        lower mandible (dark)
by+3   . . L . .
```

**Frame 1 — AJAR** (lower half drops 1px around the fixed hinge; 1 row of cavity revealed):
```
by-1   . . U . .
by     . U U U .        upper mandible — UNCHANGED
by+1   u u u u u        hinge seam — UNCHANGED (outer pixels stay = the hinge)
by+2   . I I I .        INTERIOR cavity  ← the pixels that used to be transparent body
by+3   . L L L .        lower mandible, dropped 1px
by+4   . . L . .
```

**Frame 2 — WIDE** (lower drops 2px; 2 rows of cavity + tongue; reads like a singing "o"):
```
by-1   . . U . .
by     . U U U .        upper mandible — UNCHANGED
by+1   u u u u u        hinge seam — UNCHANGED
by+2   . I I I .        cavity
by+3   . I T I .        cavity + tongue pixel
by+4   . L L L .        lower mandible, dropped 2px
by+5   . . L . .
```

Why this fixes the awkwardness:
1. **The upper mandible and hinge never move** → the mouth pivots like a real jaw, instead
   of two bars sliding apart.
2. **The reveal is filled with `I` (dark cavity)**, never transparent body color → it reads
   as *inside a mouth*.
3. **The lower mandible keeps its full wedge shape** (not a lone 3px bar) → still a beak
   when open.
4. **The seam `u` row** (a darker rim between upper and lower even when closed) gives the
   closed beak internal definition and separates it from the yellow body.

Max open of 2px (not the current 4px) keeps the beak from splitting into a thin slit; a
5-wide beak that opens ~2px is the sweet spot for "cute yap," not "unhinged snake."

### 4.4 Driving it from MouthDriver (amplitude → 3 frames, no strobe)

`MouthDriver.push()` already returns a smoothed `open ∈ [0,1]` with fast attack / slow
release. Quantize with **hysteresis + a minimum hold** so a noisy RMS doesn't chatter
between frames:

```js
// thresholds with a dead-band (hysteresis) so it doesn't flicker on the boundary
function beakFrame(open, prevFrame){
  const up   = [0.18, 0.52];   // cross UP into frame 1 at .18, into frame 2 at .52
  const down = [0.12, 0.42];   // fall back only below these (hysteresis gap)
  let f = prevFrame;
  if (open >= up[1]) f = 2;
  else if (open >= up[0]) f = Math.max(f, 1) === 2 && open < down[1] ? 1 : (f>=1?Math.min(f,2):1);
  // simpler, robust version:
  if (open >= up[1]) f = 2;
  else if (open <= down[0]) f = 0;
  else if (f === 2 && open < down[1]) f = 1;
  else if (f === 0 && open > up[0]) f = 1;
  return f;
}
// + hold each frame at least ~2 render frames (≈33ms @60fps) before allowing another change
```

- **Min-hold ≈ 2 frames / ~40–60ms** = the Animal Crossing per-letter cadence; fast enough
  to feel like talking, slow enough not to strobe.
- **Idle when silent:** `MouthDriver` already emits a small breathing value; map anything
  below the closed threshold to **frame 0**, and let the whole-body breathe (§6) carry the
  life — a resting beak should be *shut*, not fluttering.
- **On paste/stop:** call `MouthDriver.close()` (already exists) → snap to frame 0 with a
  tiny satisfied "settle" squash.

### 4.5 Expression variants (cheap wins, same 3-frame rig)

- **Happy `^_^`** (already in code): pair the arced eyes with beak frame 1 held → a content
  "chirp."
- **Surprised / loud syllable:** beak frame 2 + eyes widened 1px → the "singing O."
- **Sleepy:** beak frame 0, eyes as a 1px line, `z z z` (already implemented).

---

## 5. Idle + walk + RUN cycles for a tiny sprite

The chick must **run across its world to the front** when activated, then settle into idle.

### 5.1 Run cycle (the activation dash)

A run is **not** a fast walk: the body leans forward, the stride is longer, and there's an
**airborne frame where neither foot touches the ground** — that airborne beat is what makes
it read as running vs walking
([Thomas Palef — run cycle](https://medium.com/@thomaspalef/how-to-make-a-run-cycle-in-pixel-art-e72fb9c0b812);
[Sprite-AI — animating pixel art](https://www.sprite-ai.art/guides/how-to-animate-pixel-art)).

Frame budget (Slynyrd Pixelblog 8): **3 frames = most economical, 4 = compressed, 6 = best
balance, 8 = full.** For a stubby 2-legged chick, **4 frames** is the sweet spot; 6 if you
want more flutter. Key poses (Slynyrd's three run pose-types):

1. **Stride/contact** — front leg reaching forward, back leg trailing, body **pitched
   forward**, wings back.
2. **Passing (airborne)** — legs tucked under the body, body at its **highest**, *both feet
   off the ground*, wings up mid-flap.
3. **Opposite stride/contact** — mirror of 1.
4. **Passing (airborne)** — mirror of 2 (or reuse 2 mirrored).

Because the legs are only 2–3px stubs, most of the "run" reads through the **whole-body
motion**, not the legs:
- **Forward pitch:** lean the silhouette ~1–2px into the run direction on stride frames.
- **Vertical bob:** the body rises 1–2px on passing/airborne, drops on contact.
- **Squash on contact, stretch on the push** (§6): 1px flatter on landing, 1px taller
  leaving the ground.
- **Wing flap + head bob:** wings up on airborne, down on contact; head bobs 1px opposite
  the body for weight.
- **Dust puff** (optional, 1–2px) behind the trailing foot on contact = big "juice" for 2px
  of art.

Timing: at run speed hold each frame **~80–100ms**. Fewer frames → longer holds; Slynyrd:
strong keyframes matter more than frame count, and *"too many frames can leach the energy
from your motion."*

### 5.2 Walk cycle (if you want a slower amble)

Standard **4-frame walk**: **contact → passing → contact → passing** — contact = foot
meets ground, passing = one foot passes the other, always with **at least one foot on the
ground** ([Slynyrd Pixelblog 50 — Human Walk Cycle](https://www.slynyrd.com/blog/2024/5/24/pixelblog-50-human-walk-cycle);
[Tukita — pixel walk cycle](https://tukitanotes.blogspot.com/2017/01/pixel-art-walking-cycle.html)).
For a chick this can be even simpler: a **2-frame waddle** (rock left / rock right with a
1px body tilt and alternating foot) reads perfectly at 64px and is very Tamagotchi. Note:
for very small sprites, up/down bob "can be too busy" — keep it to 1px.

### 5.3 Idle cadence (the resting personality)

Layer several low-frequency loops so idle never freezes (all already partly in `yappy.html`):
- **Breathe:** whole-body vertical squash of **1px** on a slow sine, ~1.2–2s period (§6).
- **Blink:** eyes → 1px line for **~80–120ms**, every **2–5s, randomized** (not on a fixed
  beat — randomness reads alive). 2–3 frame close/open.
- **Idle variation:** every few seconds pick one micro-action — a **peck** (head dips 2px,
  beak frame 1, 2 frames), a **look** (pupil shifts ±1px), a **wing shuffle**. Randomize so
  it feels reactive, not looped.
- **Sleepy after inactivity:** droop, eye-lines, `z z z` (already implemented).

---

## 6. Squash / anticipation without breaking the grid

Squash-and-stretch is the "juice," and pixel art does it in **1–3px** deformations while
**preserving volume** (get wider when you get shorter, narrower when you get taller) so the
chick never looks like it shrank
([Sprite-AI — 12 principles for pixel art](https://www.sprite-ai.art/guides/animation-principles);
[Agate Dragon — bouncing ball S&S](https://agatedragon.blog/2024/08/16/bouncing-ball-animation/);
[PixelJoint — Squash & Stretch](https://pixeljoint.com/pixelart/37036.htm)).

Concrete numbers for a ~14px-body character (Sprite-AI): anticipation frame → **1px
shorter + 1px wider**; launch/stretch frame → **1–2px taller + 1px narrower**. *"Two pixels
of difference. At game speed it reads perfectly."* Celeste is the reference: deformation is
**aggressive but lasts only 1–2 frames**.

Hop / activation-arrival sequence (4 beats):
1. **Anticipation:** squash — body **1px shorter, 1px wider**, dip down (wind-up).
2. **Launch:** stretch — **1–2px taller, 1px narrower**, leave the ground.
3. **Land:** squash again on impact (1px flatter), + dust puff.
4. **Settle:** ease back to neutral over ~2 frames (the "overshoot" that sells weight).

Grid discipline: **snap deformations to integer pixels before drawing.** We already run a
second-order spring (`SOD`) for `sx/sy` — its output is a *float*. Do `Math.round()` on the
squash amount before it hits the ellipse/blit so edges don't sub-pixel-jitter (right now
`rx=13+P.sx` feeds a fractional value into `ell()`, which rounds internally but
inconsistently — round once, up front). Keep the spring for the *timing feel*; quantize the
*rendered* value.

---

## 7. Ranked, concrete recommendation for OUR chick

**1. Fix the mouth first — beak hinge, not sliding bars (Approach A, §4.3).**
   - Replace the current 2-bar beak with the **3-frame wedge**: fixed upper + fixed hinge
     seam, lower mandible drops **≤2px**, and **fill the reveal with a dark cavity color
     `#7a2e14`** (this single change is 80% of the fix).
   - Drive it by **quantizing `MouthDriver.push()`'s `0..1` into 3 frames** with hysteresis
     + a ~40–60ms min-hold (§4.4). No continuous sub-pixel scaling.
   - Keep the beak identity; let **frame 2 open wide enough to read as a round singing "o"**
     — that captures the "circle moving mouth" the user likes *without* ditching the beak.

**2. Add a selective outline in dark warm brown, never black (§1.2).**
   - 1px sel-out ring around the silhouette (`#733e39`-ish), darkest on the bottom/shadow
     arc; interior separations (belly, wing) only on the shadow side. This is the biggest
     "reads as real pixel art + pops on the LCD" win after the mouth.

**3. Tighten the palette into hue-shifted ramps; cap at ~6–8 colors (§2).**
   - Pull the chick's ramp from **Endesga 32**: body `#feae34`→`#fee761`, beak
     `#f77622`/`#e07a12`, cavity `#7a2e14`, belly cream `#ead4aa`, blush `#f6757a`, outline
     `#733e39`, eye `#3e2731`, one white catchlight. Shadows warm-shift toward orange,
     highlight pale-cream. One catchlight per eye, not two.

**4. Clean the silhouette (§1.1).**
   - Hand-fix (or bake) the outline ring so the ellipse curve has **even step-lengths** and
     no orphan pixels/doubles. Give the "bottom shade band" a light *direction* instead of a
     flat parallel band (kills the banding/pillow look).

**5. Run-cycle spec for the activation dash (§5.1).**
   - **4-frame run**: stride → airborne(both-feet-off, body highest) → opposite stride →
     airborne. Sell it with **1–2px forward pitch, 1–2px vertical bob, 1px contact-squash /
     push-stretch, wing flap, and a 2px dust puff.** Hold ~80–100ms/frame. Then ease into
     the layered **idle** (breathe 1px + randomized blink every 2–5s + occasional peck).

**6. Juice, on the grid (§6).**
   - Hop = anticipation-squash → stretch-launch → land-squash → settle-overshoot, all in
     **1–2px, volume-preserved**. `Math.round()` the spring output before rendering.

### Top fundamentals links to actually follow (in order)
1. [Slynyrd — Pixelblog index](https://www.slynyrd.com/blog) (esp.
   [#1 Color](https://www.slynyrd.com/blog/2018/1/10/pixelblog-1-color-palettes),
   [#8 Animation](https://www.slynyrd.com/blog/2018/8/19/pixelblog-8-intro-to-animation),
   [#50 Walk Cycle](https://www.slynyrd.com/blog/2024/5/24/pixelblog-50-human-walk-cycle)) — the single best written reference.
2. [Saint11 — Pixel Art Tutorials](https://saint11.art/blog/pixel-art-tutorials/) +
   [Anti-Alias & Banding](https://saint11.art/pixel_art_articles/article5/).
3. [Pixel Parmesan — Anti-Aliasing Fundamentals](https://pixelparmesan.com/blog/anti-aliasing-fundamentals-for-pixel-artists).
4. [Derek Yu — Pixel Art BASICS](https://www.derekyu.com/makegames/pixelart.html) (silhouette-first + common mistakes).
5. [Pixnote — Outlines & sel-out](https://pixnote.net/en/learn/outlines/).
6. [Brandon James Greer (YouTube)](https://www.youtube.com/@BJGpixel) & [AdamCYounis (YouTube)](https://www.youtube.com/@AdamCYounis) for limited palettes + theory.
7. [Lospec palette list](https://lospec.com/palette-list) —
   [Endesga 32](https://lospec.com/palette-list/endesga-32),
   [Sweetie 16](https://lospec.com/palette-list/sweetie-16).

---

## Sources
- Pedro Medeiros / Saint11 — [Tutorials](https://saint11.art/blog/pixel-art-tutorials/), [Articles](https://saint11.art/pixel_articles/), [Anti-Alias & Banding](https://saint11.art/pixel_art_articles/article5/)
- Brandon James Greer — [YouTube @BJGpixel](https://www.youtube.com/@BJGpixel), [ArtStation](https://www.artstation.com/bjgpixel)
- MortMort — [Lospec author page](https://lospec.com/pixel-art-tutorials/author/mortmort)
- AdamCYounis — [YouTube](https://www.youtube.com/@AdamCYounis)
- Slynyrd — [Blog](https://www.slynyrd.com/blog), [#1 Color Palettes](https://www.slynyrd.com/blog/2018/1/10/pixelblog-1-color-palettes), [#8 Animation](https://www.slynyrd.com/blog/2018/8/19/pixelblog-8-intro-to-animation), [#50 Walk Cycle](https://www.slynyrd.com/blog/2024/5/24/pixelblog-50-human-walk-cycle)
- Derek Yu — [Pixel Art BASICS](https://www.derekyu.com/makegames/pixelart.html)
- Pixel Parmesan — [Anti-Aliasing Fundamentals](https://pixelparmesan.com/blog/anti-aliasing-fundamentals-for-pixel-artists)
- Pixnote — [Outlines & Selective Outline](https://pixnote.net/en/learn/outlines/)
- Ricky Han — [Selective outlining / AA algorithm](https://rickyhan.com/jekyll/update/2019/09/01/pixel-art-algorithm-selective-outlining-anti-aliasing.html)
- Lospec — [Lines/Curves/Jaggies #16](https://lospec.com/pixel-art-tutorials/how-to-pixel-art-tutorials-16-lines-curves-jaggies-by-dual-core-studio), [Palette list](https://lospec.com/palette-list), [Endesga 32](https://lospec.com/palette-list/endesga-32), [Sweetie 16](https://lospec.com/palette-list/sweetie-16), [Resurrect 64](https://lospec.com/palette-list/resurrect-64), [AAP-64](https://lospec.com/palette-list/aap-64)
- Jellytempo — [Pixel Art Terms Glossary](https://jellytempo.com/pixel-art-terms-definitions-a-beginner-pixel-artists-glossary/)
- Pixel Art Wiki — [Jaggies](https://pixelart.fandom.com/wiki/Jaggies)
- Nookipedia — [Animalese](https://nookipedia.com/wiki/Animalese); Blender Artists — [AC face swaps](https://blenderartists.org/t/how-to-emulate-animal-crossings-facial-expressions-texture-swapping-the-mouth-eyes/1192051)
- FmsDraws — [UT/DR spriting tutorial](https://fmsdraws.tumblr.com/post/181571525098/the-undertaledeltarune-spriting-tutorial-sort); [UT/DR Dialogue Compendium](https://inactivesnail.neocities.org/utdr-dialogue)
- MoCap Online — [Lip Sync / Visemes guide](https://mocaponline.com/blogs/mocap-news/lip-sync-animation-guide)
- Thomas Palef — [Run cycle in pixel art](https://medium.com/@thomaspalef/how-to-make-a-run-cycle-in-pixel-art-e72fb9c0b812); Tukita — [Walk cycle](https://tukitanotes.blogspot.com/2017/01/pixel-art-walking-cycle.html); Sprite-AI — [Animating pixel art](https://www.sprite-ai.art/guides/how-to-animate-pixel-art), [12 principles](https://www.sprite-ai.art/guides/animation-principles)
- Squash & Stretch — [PixelJoint](https://pixeljoint.com/pixelart/37036.htm); [Agate Dragon bouncing ball](https://agatedragon.blog/2024/08/16/bouncing-ball-animation/)

*Cross-refs in this repo: `docs/research/cute-character-animation-code.md` (baby schema + Canvas2D rig), `docs/research/cute-companion-design.md` (why a chick), `docs/prototypes/yappy.html` (current pixel chick), `desktop/src/pill/mouth.ts` (`MouthDriver`, the `0..1` the beak reads).*
