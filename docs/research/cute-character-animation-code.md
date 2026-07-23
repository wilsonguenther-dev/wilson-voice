# Hand-Coding an Adorable Animated Pet in a WebKit Webview (Canvas2D)

Research brief for the Yap companion (`wilson-voice`). Target: a tiny always-on-top
Tauri/WKWebView window (~120–140 px), 60 fps, low CPU, **no animation library** — every
system hand-coded and iterable. This document is self-sufficient: math, code sketches,
real citations, and a buildable ~120-line skeleton at the end.

Context from our own codebase: we already own a `MouthDriver`
(`desktop/src/pill/mouth.ts`) that turns live mic RMS into a smoothed 0..1 jaw-open with
gate/gamma/gain and asymmetric attack/release. The renderer only needs to turn that number
into a **cute mouth shape**. The origami crane was rejected — the goal is an adorable
*digital pet* (Tamagotchi / desktop-buddy energy), so this brief is written for a soft
procedural creature, not a folded object.

---

## 0. Why this is a "cute" problem before it is a code problem

The single highest-leverage fact: cuteness is a measurable perceptual response to the
**baby schema** (*Kindchenschema*), the cluster of infantile features Konrad Lorenz
identified — oversized head, **large low-set eyes**, round body, small nose/mouth, chubby
proportions. It is not aesthetic taste; baby schema demonstrably activates the nucleus
accumbens reward system and drives caretaking motivation
([Glocker et al., 2009, PNAS](https://www.pnas.org/doi/10.1073/pnas.0811620106);
[Glocker et al., Ethology 2009](https://onlinelibrary.wiley.com/doi/abs/10.1111/j.1439-0310.2008.01603.x)).
Stephen Jay Gould's "Mickey Mouse Meets Konrad Lorenz" documents Disney literally
regressing Mickey toward the baby schema (bigger head, bigger eyes) as he became more
beloved.

Design consequences we bake into every system below:
- **Eyes are ~1/3 of the face, sit low, and are round.** This alone carries most of the
  perceived cuteness (see §3).
- **Head/body big, limbs tiny.** Round silhouettes read as safe/soft.
- **Small mouth and nose.** The mouth animates a lot but stays small.
- **Slow, gentle idle motion.** Baby-like = unhurried, never jittery.

Everything else — springs, squash, juice — exists to make those proportions feel *alive*.

---

## 1. Procedural idle life (the "always alive, never busy" layer)

An idle pet must never freeze (dead) and never fidget (annoying). The trick is a **budget
of independent low-amplitude channels** summed together, each on its own timescale. Because
they're incommensurate sines/timers, they never repeat visibly, so the creature reads as
organic. Reference for the underlying trig/oscillation model: Daniel Shiffman,
[*The Nature of Code*, Ch.3 "Oscillation"](https://natureofcode.com/oscillation/).

### 1a. Breathing — vertical sine scale (volume-preserved)
A slow sine on vertical scale, with horizontal scale compensating so the creature "inflates"
rather than stretches. Period ~3–4 s, amplitude ±2–4 %.

```js
const breath = Math.sin(t * 1.9) * 0.03;   // ~3.3 s period, ±3%
let sy = 1 + breath;                        // taller on inhale
let sx = 1 / sy;                            // area-preserving (ellipse area = π·a·b)
```
Keep amplitude tiny — at 120 px, 3 % is ~3.6 px and already clearly "breathing".

### 1b. Blink — random-cadence eyelid close over ~120 ms
Humans blink every ~2–10 s; a blink lasts ~100–150 ms, closing *faster* than it opens.
Model it as a scheduled event driving an eyelid-coverage value 0..1, with an asymmetric
close/open curve. Occasionally fire a **double blink** for character.

```js
let nextBlink = 1.5, blinkT = -1, lid = 0;   // lid: 0 open .. 1 closed
function updateBlink(dt) {
  if (blinkT < 0 && t > nextBlink) blinkT = 0;      // trigger
  if (blinkT >= 0) {
    blinkT += dt;
    const CLOSE = 0.05, OPEN = 0.09;                // close fast, open slower (~140ms total)
    lid = blinkT < CLOSE
        ? blinkT / CLOSE                            // 0→1 closing
        : Math.max(0, 1 - (blinkT - CLOSE) / OPEN); // 1→0 opening
    if (blinkT > CLOSE + OPEN) {                     // done
      blinkT = -1; lid = 0;
      nextBlink = t + 2 + Math.random() * 4;        // 2–6 s to next
      if (Math.random() < 0.15) nextBlink = t + 0.18; // occasional double-blink
    }
  }
}
```
Render: multiply eye height by `(1 - lid)` (a squishing lid), *not* an eyelid rectangle —
squishing the whole eye reads cuter and is 3 lines.

### 1c. Micro weight-shifts — organic horizontal drift
A pure sine looks mechanical. Sum 2–3 incommensurate sines (irrational-ish frequency
ratios) to fake Perlin-noise drift for free:

```js
const sway = Math.sin(t * 0.7) * 2.0        // ±2 px slow
           + Math.sin(t * 0.31) * 1.4       // ±1.4 px slower, different period
           + Math.sin(t * 1.13) * 0.6;      // tiny fast wobble
```
Apply to the body's x, and let the head lag slightly (spring the head-x toward body-x, §2)
for secondary motion — the "follow-through" principle from Disney's 12 (see §2 refs).

### 1d. Occasional look-around — pupil saccades
Every few seconds, pick a new pupil target (a random point inside the eye, or the cursor,
or a point of "attention"). Real eyes **saccade** — snap fast (~40–60 ms) then hold — so
spring the pupil toward the target with a stiff, slightly underdamped spring (§2). Between
saccades, add micro-drift so pupils aren't statue-still.

```js
let look = {x: 0, y: 0}, lookTarget = {x: 0, y: 0}, nextLook = 2;
function updateLook(dt) {
  if (t > nextLook) {                       // choose a new place to look
    lookTarget.x = (Math.random() * 2 - 1) * 3;
    lookTarget.y = (Math.random() * 2 - 1) * 2;
    nextLook = t + 1.5 + Math.random() * 3;
  }
  spring(lookX, lookTarget.x, 8, 0.5, dt);  // fast snap = saccade (see §2)
  spring(lookY, lookTarget.y, 8, 0.5, dt);
}
```

### Layering rule (how to keep it alive but not busy)
Give each channel an **amplitude budget** and keep the *idle* total small. A practical
hierarchy:

| Channel        | Timescale | Amplitude | Character                     |
|----------------|-----------|-----------|-------------------------------|
| Breathing      | 3–4 s     | ±3 % scale| constant, cannot be turned off|
| Weight sway    | 5–10 s    | ±2–4 px   | slow, organic                 |
| Blink          | 2–6 s     | event     | punctuation, ~120 ms          |
| Look/saccade   | 2–5 s     | ±3 px     | attention, mostly held        |

Reactions (hop, surprise, sparkle) are **transient and larger** — they briefly break the
idle budget, then decay back. The felt result: a calm creature that "wakes up" on events.
Never run two large motions at once during idle.

---

## 2. Squash & stretch + bounce (the spring is the whole engine)

**Squash & stretch** is Disney principle #1 — deform to convey weight and elasticity while
**preserving volume**: an ellipse's area is `π·a·b`, so if you scale height by `s`, scale
width by `1/s` and the creature keeps its mass
([Thomas & Johnston, *The Illusion of Life*](https://www.nyfa.edu/student-resources/12-principles-of-animation/);
[squash & stretch explainer](https://www.deedeestudio.net/en/post/principles-animation-squash-stretch)).
- **Anticipation** (principle #2): wind *down/back* before a jump.
- **Landing squash**: on impact, flatten (`scaleY < 1`), then spring back with overshoot.

### The spring step (standard stiffness/damping integrator)
Model position as a **damped harmonic oscillator**: `a = -k·(x - target) - c·v`. Integrate
with **semi-implicit (symplectic) Euler** — update velocity *first*, then position. This is
the integrator commercial physics engines use because it stays stable where explicit Euler
injects energy and explodes
([Gaffer On Games, "Integration Basics"](https://gafferongames.com/post/integration_basics/)).

Parameterize by **frequency (Hz)** and **damping ratio ζ** instead of raw k/c — far more
intuitive to tune:

```js
// s = {x, v}; target = goal; f = Hz (bounciness); zeta = damping ratio; dt = seconds
function spring(s, target, f, zeta, dt) {
  const w = 2 * Math.PI * f;       // angular frequency
  const k = w * w;                 // stiffness (mass = 1)
  const c = 2 * zeta * w;          // damping
  const a = -k * (s.x - target) - c * s.v;
  s.v += a * dt;                   // velocity FIRST → symplectic, stable
  s.x += s.v * dt;                 // then position
}
```
- **ζ < 1** underdamped → bouncy **overshoot** (use ζ ≈ 0.25–0.4 for a springy hop).
- **ζ = 1** critically damped → fastest settle, **no** overshoot.
- **ζ > 1** overdamped → sluggish, no bounce.

Stability caveat: semi-implicit Euler is stable for reasonable `k·dt`, but if you crank
frequency very high **sub-step** the integration (run it 2–4× per frame with `dt/n`) or use
a closed-form solution. Ryan Juckett derives the exact analytic damped-spring update
(coefficients cached per timestep — no per-frame drift, unconditionally stable):
[ryanjuckett.com/damped-springs](https://www.ryanjuckett.com/damped-springs/).

### Anticipation + overshoot in one model (t3ssel8r "second-order dynamics")
For a jump that *anticipates* (dips before launching) and *overshoots* on landing without
hand-scripting keyframes, use the second-order system from t3ssel8r's
[*Giving Personality to Procedural Animations using Math*](https://www.youtube.com/watch?v=KPoeNZZ6H4s)
([text mirror](https://github.com/SalvatoreScalia/Giving-Personality-to-Procedural-Animations-using-Math)).
Three intuitive params: **f** frequency, **ζ** damping, **r** initial response.

```js
// f=freq, z=damping, r=response (r>1 overshoot, r<0 ANTICIPATION/wind-up before moving)
function SecondOrder(f, z, r, x0) {
  const w = 2 * Math.PI * f;
  this.k1 = z / (Math.PI * f);
  this.k2 = 1 / (w * w);
  this.k3 = r * z / w;
  this.xp = x0; this.y = x0; this.yd = 0;
  this.update = (x, dt) => {
    const xd = (x - this.xp) / dt; this.xp = x;                 // input velocity (finite diff)
    const k2s = Math.max(this.k2, dt*dt/2 + dt*this.k1/2, dt*this.k1); // clamp = stable
    this.y  += dt * this.yd;
    this.yd += dt * (x + this.k3*xd - this.y - this.k1*this.yd) / k2s;
    return this.y;
  };
}
```
Set **r < 0** on the jump's scaleY channel and the creature crouches *before* it leaps —
anticipation for free, no keyframes.

### Practical hop recipe
```js
const hopY   = {x: 0, v: 0};   // vertical offset
const squash = {x: 1, v: 0};   // scaleY
function jump() { hopY.v = -260; squash.x = 1.22; squash.v = 0; }  // launch: up + stretch
// each frame:
spring(hopY,   0, 3.0, 0.30, dt);   // gravity-less spring back to ground, bouncy
spring(squash, 1, 6.0, 0.35, dt);   // scaleY springs to 1 with overshoot
if (hopY.x > -3 && hopY.v > 30 && squash.x > 0.98) { squash.x = 0.80; squash.v = 0; } // land squash
// render: scaleY = squash.x, scaleX = 1/squash.x  (volume preserved)
```

---

## 3. Expressive eyes (the cuteness workhorse)

If you build only one thing well, build the eyes. Big round eyes + a bright **catchlight**
(specular highlight) + pupils that **track attention** produce most of the "it's alive and
adorable" reaction, because they hit the baby schema directly (§0). Rules:

**Shape & placement.** Eyes large (each ~⅓ of head width), **round**, set **low** on the
face (lower = younger = cuter), with generous spacing. Whites (sclera) optional — a big
dark pupil on a lighter iris on a white already reads cute; for a simpler pet, big glossy
black eyes + catchlight is enough.

**Catchlight (the magic pixel).** A single small white highlight, plus an optional smaller
secondary highlight, placed consistently relative to the light (e.g., upper-left). This is
what makes eyes read as wet/glossy/alive. Moving it slightly with the pupil sells 3D.

**Pupil tracking.** Move the pupil toward a target (cursor, or the current "attention"
point) but **clamp excursion** so the pupil never leaves the eye:
```js
const dx = mouse.x - eyeCX, dy = mouse.y - eyeCY;
const d = Math.hypot(dx, dy) || 1, m = Math.min(MAXOFFSET, d) / d;
const px = dx * m, py = dy * m;   // clamped pupil offset
```
Spring `px,py` toward that (§2) so the gaze glides and saccades instead of snapping rigidly.

**Emotion via eye shape** (cheap, high payoff — swap the eye path per mood):
- **Happy** → eyes become **upward arcs** (`‿ ‿` inverted, i.e., a `^`-shaped closed curve) —
  the classic content/smiling-eyes look.
- **Surprised** → eyes go **wide** (scale up) with a **small** pupil (contrast = alarm).
- **Sleepy** → **half-lidded**: drop the top lid ~40–60 % (`eyeHeight *= 0.5`, offset down).
- **Blink** → drive the same eye-height squish from §1b to 0.
- **Skeptical/curious** → asymmetric lids (one eye more open) — instant personality.

Because eye shape is just a path with a couple of parameters (openness, arc, pupil size),
you can crossfade emotions by lerping those numbers — no sprite swaps.

---

## 4. Mouth driven by audio (make our `MouthDriver` output *cute*)

We already have the hard part: `MouthDriver.push(rms, dt)` returns a smoothed **0..1
jaw-open** with gating, gamma lift, asymmetric attack/release (snap open, ease closed) and
idle breathing (`desktop/src/pill/mouth.ts`). The renderer's job is only **mouth shape**.
Wire it to the Tauri `audio_level` event:

```js
import { MouthDriver } from './pill/mouth';
const mouth = new MouthDriver();
let openTarget = 0;
listen('audio_level', e => { openTarget = e.payload.rms; });   // Tauri event → raw RMS
// each frame:
const open = mouth.push(openTarget, dt);   // 0..1, already smoothed & breathing
```

**Cute-mouth design principles:**
- **Small and low.** A cute mouth is a *small* feature near the bottom of the face; it opens
  a modest amount even at `open = 1`. Never a big gaping hole.
- **Rounded, not angular.** Use `quadraticCurveTo` for soft lips or a small filled ellipse.
- **Smile baseline.** Even at `open = 0`, give it a gentle upward curve so the resting face
  is happy, not neutral.
- **Width barely changes.** Real cute talking = jaw drops, corners stay put. Map `open`
  mostly to *height*, with a tiny width breathe.
- **Accents = charisma.** A small pink tongue that peeks at high open, or a tiny blush that
  fades in while "yapping," adds enormous character for ~4 lines.

Three shape options (pick one; the ellipse is simplest and cutest):

```js
// (A) round "o" mouth — cutest, 3 lines. open ∈ 0..1
const mh = 2 + open * 8, mw = 6 + open * 2;      // height grows, width barely
ctx.fillStyle = '#c0466a';
ctx.beginPath(); ctx.ellipse(0, 16, mw, mh, 0, 0, Math.PI * 2); ctx.fill();
// tongue peek at high open:
if (open > 0.5) { ctx.fillStyle = '#ff8fae';
  ctx.beginPath(); ctx.ellipse(0, 16 + mh * 0.4, mw * 0.6, mh * 0.4, 0, 0, Math.PI * 2); ctx.fill(); }

// (B) soft-lip "beak" — two quad curves hinged at the corners, smile baseline
const mo = open * 9;
ctx.beginPath();
ctx.moveTo(-7, 16);
ctx.quadraticCurveTo(0, 16 + mo, 7, 16);   // lower lip drops as it opens
ctx.quadraticCurveTo(0, 14,      -7, 16);  // upper lip = slight smile
ctx.fillStyle = '#e06a8b'; ctx.fill();
```

Because `MouthDriver` already does asymmetric attack/release, the mouth "snaps" on each
syllable and eases shut between words — the single biggest cue that it's *speaking your
words*, not lip-flapping randomly. Do **not** add more smoothing in the renderer; you'd blur
the syllables it worked to preserve.

---

## 5. Rendering approaches — three options, one recommendation

| Approach | What it is | Pros @120px | Cons | Verdict |
|---|---|---|---|---|
| **(a) Procedural vector** | Arcs/paths + `createRadialGradient` for soft volume, animated by canvas transforms | Resolution-independent; *every* param (squash, blink, mouth, gaze) is continuous & audio-reactive; **zero assets**; tiny code; trivial to iterate | Hard to get painterly/illustrated detail; you are the artist-in-code | ✅ **Best for a parametric, audio-reactive pet** |
| **(b) Sprite sheet** | Pre-drawn frames via `drawImage` | Highest art quality; cheap blit; artist-friendly | **Discrete** states — continuous audio mouth needs a separate layer or many frames; squash/gaze still need transforms; asset pipeline | Great later, with an artist, for detailed hero art |
| **(c) 2D skeletal rig** | Bones/mesh deform (Spine, Live2D, DragonBones, or hand-rolled) | Smooth deformation, reusable clips | Heavy tooling + a **runtime library** (violates "no library"); overkill at 120 px; hand-rolling bones is *more* code than vector | ❌ Overkill here |

**On (c):** [Spine](http://esotericsoftware.com/) (bone-based) and
[Live2D Cubism](https://www.live2d.com/) (illustration-mesh deform, the VTuber standard) are
the pro tools for smooth 2D deformation, but both ship runtimes you'd import, and at a 120 px
desktop pet their power is wasted. A hand-rolled bone chain is feasible but is strictly more
code than the vector approach for the same on-screen result.

**Recommendation: pure procedural vector.** Radial gradients give soft, volumetric shading
that reads "cute plush" at small size; canvas `scale`/`translate` give squash, breathe, and
bounce for free; and the mouth/eyes are just parameterized paths, so the mic and cursor drive
them continuously with no frame budget. Keep sprite-sheet/pixel-art as an *optional texture
layer* you can add inside the vector silhouette later (clip to the body path, `drawImage` a
pattern) without re-architecting.

**Performance notes (always-on-top, low CPU):**
- One small canvas backed at `devicePixelRatio` (cap DPR at 2). `clearRect` + redraw each
  frame is fine at 140 px.
- Drive with `requestAnimationFrame` (auto-pauses when the window is hidden/occluded).
- **Idle throttle:** when silent *and* no cursor nearby, drop to ~30 fps (skip every other
  rAF) — halves CPU while keeping breathing/blink alive. Full 60 fps only while talking or
  interacting.
- Tauri/WKWebView: transparent window via `transparent: true` in `tauri.conf.json` + a
  transparent CSS `body`; Canvas2D + rAF are fully supported in WKWebView.

---

## 6. Pixel-art in canvas (if/when you want a retro Tamagotchi look)

Canvas2D can render crisp pixel art, but you must fight the default bilinear smoothing.

**Crisp integer scaling.**
```js
ctx.imageSmoothingEnabled = false;   // MUST set (resets on canvas resize!)
```
([MDN: `imageSmoothingEnabled`](https://developer.mozilla.org/en-US/docs/Web/API/CanvasRenderingContext2D/imageSmoothingEnabled);
[MDN: "Crisp pixel art look"](https://developer.mozilla.org/en-US/docs/Games/Techniques/Crisp_pixel_art_look)).
Discipline that keeps pixels sharp:
- Author at **native sprite resolution** (e.g., 32×32 or 48×48) on an offscreen canvas, then
  blit up by an **integer** factor (2×, 3×, 4×) — never fractional, or you get shimmering
  uneven pixels. At 120 px display, a 40×40 sprite at 3× fits perfectly.
- **Snap draw positions to whole device pixels** (`Math.round`) so sub-pixel motion doesn't
  reintroduce blur.
- Belt-and-suspenders in CSS: `image-rendering: pixelated;` on the canvas element.
- devicePixelRatio: pick a zoom that divides the DPR-backed size evenly, or render the pixel
  layer separately at integer zoom and composite.

**Palette discipline.** Limit to ~8–16 colors; build 3–4-step **ramps** (dark→light) per
material and shade by ramp index, not free color. A tight palette is what makes pixel art
read as "designed" instead of noisy.

**How many frames for cute idle/blink** (frame-based, if not procedural):
- Idle breathe: **2–4** frames (or skip frames and bob procedurally in canvas).
- Blink: **2–3** frames (open → half → closed), held ~2–3 frames closed.
- A full "idle + occasional blink" cycle is comfortably **6–8** frames. Cuteness comes from
  *timing* (hold the closed blink a beat) more than frame count.

**Authoring tools:**
[Aseprite](https://www.aseprite.org/) — the standard ($20; onion-skinning, tag-based
animations, exports sprite sheet PNG + JSON), [source on GitHub](https://github.com/aseprite/aseprite).
[Piskel](https://piskelapp.com/) — free, browser-based, exports sheets/GIF; great for
prototyping. **LibreSprite** — free Aseprite fork.

---

## 7. Game-feel "juice" (the delight layer)

"Juice" = maximum output feedback for minimal input — the difference between a flat pet and
one that feels *alive and responsive*. Canonical references:
[**"Juice it or lose it"** — Martin Jonasson & Petri Purho](https://www.youtube.com/watch?v=Fy0aCDmgnxg)
(the definitive talk; same breakout game, before/after juice);
[**"The Art of Screenshake"** — Jan Willem Nijman / Vlambeer](https://www.youtube.com/watch?v=AJdEqssNZ-U)
(30 tricks that turn a bland prototype into a satisfying one);
[**Game Feel** — Steve Swink](https://www.routledge.com/Game-Feel-A-Game-Designers-Guide-to-Virtual-Sensation/Swink/p/book/9780123743282)
(the theory book); [**Nicky Case**](https://ncase.me/) (playful, hand-tuned interactive
motion — study how tiny reactions carry personality).

Juice techniques worth stealing for the pet, ranked by payoff-per-line:

1. **Landing squash + overshoot** (already have it via §2) — the #1 juice move.
2. **Sparkle particles on "done"** (task complete / good event). Spawn N particles with
   velocity, gravity, fade, and a twinkle (scale pulses):
   ```js
   const parts = [];
   function sparkle(x, y, n = 10) {
     for (let i = 0; i < n; i++) parts.push({
       x, y, a: Math.random() * 6.28, s: 40 + Math.random() * 60,
       vx: 0, vy: 0, life: 0, ttl: 0.6 + Math.random() * 0.4 });
   }
   function updateParts(dt) {
     for (const p of parts) {
       p.vx = Math.cos(p.a) * p.s; p.vy = Math.sin(p.a) * p.s + p.life * 120; // gravity
       p.x += p.vx * dt; p.y += p.vy * dt; p.life += dt;
     }
     for (let i = parts.length - 1; i >= 0; i--) if (parts[i].life > parts[i].ttl) parts.splice(i, 1);
   }
   // draw: alpha = 1 - life/ttl; radius twinkles with Math.sin(life*30)
   ```
3. **Color pop / flash.** On a positive event, briefly lerp the body brighter or nudge hue,
   then decay over ~150 ms (an ease-out). Cheap dopamine.
4. **Screen-space bob & tilt.** A gentle whole-body vertical sine + a tiny rotation on
   reactions (a happy "wiggle") — but for a *desktop* pet keep amplitude small and never do
   literal screenshake (annoying in a persistent window). Save "shake" for a squash-wiggle.
5. **Timing curves.** Arrivals ease-**out** (fast→slow, feels like it "lands"); pops use
   **back**/**elastic** (overshoot). Reference the 30 Penner curves:
   [easings.net](https://easings.net/) and [Robert Penner's originals](https://robertpenner.com/easing/).
6. **Hit-stop / sleep frames** (Vlambeer): freeze the whole animation for ~2–4 frames on a
   big beat, then resume — makes impacts feel heavier. Use *very* sparingly for a pet.

Golden rule from all four references: **stack many tiny reactions**. No single effect is the
juice; the *cascade* of squash + particle + color pop + sound on one event is what delights.

---

## Ranked implementation recommendation for OUR pet

**Rendering:** pure **procedural vector** on one small Canvas2D, backed at `min(devicePixelRatio, 2)`,
driven by a `dt`-based `requestAnimationFrame` loop, in the existing Tauri/WKWebView float
window. No library, no assets to start. (Add a pixel-art texture layer later only if we want
a retro skin — it slots inside the vector silhouette.)

**Build the procedural systems in this order** (each is independently shippable and visibly
improves the pet):

1. **Spring integrator** (§2) — the foundation everything reactive uses. ~6 lines.
2. **Breathing** (§1a) — constant life; the pet is never dead. ~2 lines.
3. **Blink** (§1b) — highest cuteness-per-line; schedule + eye-height squish. ~10 lines.
4. **Squash & stretch on hop/land** (§2) — spring scaleY, volume-preserve scaleX. The juice
   backbone. ~8 lines.
5. **Audio mouth** (§4) — wire our existing `MouthDriver` output to a cute round/beak mouth
   path. The feature that makes it *ours* (yaps your words). ~6 lines.
6. **Pupil look/cursor tracking + catchlight** (§1d, §3) — attention + gloss; big perceived-
   intelligence jump. ~10 lines.
7. **Juice: landing squash you already have + sparkle-on-done + color pop** (§7). ~20 lines.

Emotion eye-shapes (§3) and idle throttle (§5) come after the core loop feels good.

---

## Minimal buildable Canvas2D skeleton (~120 lines, self-contained)

A cute, soft-blob creature that **breathes, blinks, tracks the cursor, hops with squash &
stretch on click, and opens an audio-driven mouth**. Runs standalone in any browser; the two
plug points for our app are marked. Drop into an HTML file with `<canvas id="pet"></canvas>`
and a transparent body, or adapt into `float-main.tsx`.

```js
// cute-pet.js — hand-coded procedural pet. No libraries. ~115 lines.
const cv = document.getElementById('pet');
const ctx = cv.getContext('2d');
const DPR = Math.min(window.devicePixelRatio || 1, 2);
const W = 140, H = 150;
cv.width = W * DPR; cv.height = H * DPR;
cv.style.width = W + 'px'; cv.style.height = H + 'px';
ctx.setTransform(DPR, 0, 0, DPR, 0, 0);

// --- spring: semi-implicit Euler damped oscillator (freq Hz, damping ratio) ---
function spring(s, target, f, zeta, dt) {
  const w = 2 * Math.PI * f, k = w * w, c = 2 * zeta * w;
  s.v += (-k * (s.x - target) - c * s.v) * dt;   // velocity first = stable
  s.x += s.v * dt;
}
const hopY   = { x: 0, v: 0 };   // vertical hop offset
const squash = { x: 1, v: 0 };   // scaleY (1 = rest)
const lookX  = { x: 0, v: 0 };
const lookY  = { x: 0, v: 0 };

// --- timers / input ---
let t = 0, last = 0, nextBlink = 1.5, blinkT = -1, lid = 0;
let nextLook = 2, lookTgt = { x: 0, y: 0 };
const mouse = { x: W / 2, y: 60 }, inside = { v: false };
let mouthOpen = 0;                              // PLUG 1: set from MouthDriver.push(rms, dt)
window.setMouth = v => { mouthOpen = v; };      //   e.g. listen('audio_level', e => setMouth(mouth.push(e.payload.rms, dt)))

addEventListener('pointermove', e => {
  const r = cv.getBoundingClientRect();
  mouse.x = e.clientX - r.left; mouse.y = e.clientY - r.top; inside.v = true;
});
addEventListener('pointerdown', () => { hopY.v = -260; squash.x = 1.22; squash.v = 0; }); // hop!

function frame(now) {
  const dt = Math.min(0.033, (now - (last || now)) / 1000); last = now; t += dt;

  const breath = Math.sin(t * 1.9) * 0.03;                       // §1a breathing
  const sway = Math.sin(t * 0.7) * 2 + Math.sin(t * 0.31) * 1.4; // §1c weight shift

  spring(hopY, 0, 3.0, 0.30, dt);                                // §2 bouncy return
  spring(squash, 1, 6.0, 0.35, dt);
  if (hopY.x > -3 && hopY.v > 30 && squash.x > 0.98) { squash.x = 0.80; squash.v = 0; } // land squash

  if (blinkT < 0 && t > nextBlink) blinkT = 0;                   // §1b blink
  if (blinkT >= 0) {
    blinkT += dt; const C = 0.05, O = 0.09;
    lid = blinkT < C ? blinkT / C : Math.max(0, 1 - (blinkT - C) / O);
    if (blinkT > C + O) { blinkT = -1; lid = 0; nextBlink = t + 2 + Math.random() * 4;
      if (Math.random() < 0.15) nextBlink = t + 0.18; }
  }

  if (t > nextLook) {                                            // §1d saccade target
    lookTgt.x = (Math.random() * 2 - 1) * 3; lookTgt.y = (Math.random() * 2 - 1) * 2;
    nextLook = t + 1.5 + Math.random() * 3;
  }
  const tgtX = inside.v ? Math.max(-3, Math.min(3, (mouse.x - W / 2) * 0.08)) : lookTgt.x;
  const tgtY = inside.v ? Math.max(-2, Math.min(2, (mouse.y - 70) * 0.06))   : lookTgt.y;
  spring(lookX, tgtX, 8, 0.5, dt); spring(lookY, tgtY, 8, 0.5, dt);

  draw(breath, sway); requestAnimationFrame(frame);
}
requestAnimationFrame(frame);

function draw(breath, sway) {
  ctx.clearRect(0, 0, W, H);
  const cx = W / 2 + sway, cy = 92 + hopY.x * 0.06;
  const sy = squash.x * (1 + breath), sx = (1 / squash.x) * (1 - breath * 0.5); // volume preserve
  ctx.save(); ctx.translate(cx, cy); ctx.scale(sx, sy);

  const R = 42;                                                  // §0 round soft body
  const g = ctx.createRadialGradient(-12, -16, 4, 0, 0, R * 1.25);
  g.addColorStop(0, '#a6ecff'); g.addColorStop(1, '#3aa6d8');
  ctx.fillStyle = g; ctx.beginPath(); ctx.ellipse(0, 0, R, R * 1.02, 0, 0, 7); ctx.fill();

  const EX = 15, EY = 0, ER = 12;                                // §3 big low eyes
  for (const side of [-1, 1]) {
    ctx.fillStyle = '#fff';
    ctx.beginPath(); ctx.ellipse(side * EX, EY, ER, ER * (1 - 0.92 * lid), 0, 0, 7); ctx.fill();
    if (lid < 0.6) {
      const px = side * EX + lookX.x, py = EY + lookY.x;         // pupil tracks (springed)
      ctx.fillStyle = '#16303c';
      ctx.beginPath(); ctx.arc(px, py, 6, 0, 7); ctx.fill();
      ctx.fillStyle = 'rgba(255,255,255,.95)';                   // catchlight
      ctx.beginPath(); ctx.arc(px - 2.2, py - 2.6, 2.1, 0, 7); ctx.fill();
    }
  }

  const mh = 2 + mouthOpen * 8, mw = 6 + mouthOpen * 2;          // §4 cute audio mouth
  ctx.fillStyle = '#c0466a';
  ctx.beginPath(); ctx.ellipse(0, 18, mw, mh, 0, 0, 7); ctx.fill();
  if (mouthOpen > 0.5) { ctx.fillStyle = '#ff8fae';             // tongue peek
    ctx.beginPath(); ctx.ellipse(0, 18 + mh * 0.4, mw * 0.6, mh * 0.4, 0, 0, 7); ctx.fill(); }

  ctx.restore();
}
```

Plug points for `wilson-voice`:
- **PLUG 1 (audio):** replace `window.setMouth` wiring with our `MouthDriver` —
  `import { MouthDriver } from './pill/mouth'`, then in the Tauri `audio_level` listener call
  `setMouth(mouth.push(e.payload.rms, dt))`. The driver already smooths + breathes, so the
  renderer stays dumb.
- **Hop trigger:** wire `pointerdown` to real events (wake, task-done) instead of clicks;
  fire `sparkle()` (§7) alongside for the juice cascade.
- **Emotion:** swap the eye-height/arc and mouth-curve parameters per mood (§3) to express
  listening / thinking / done.

Everything here is a handful of numbers you can iterate live — which is exactly the point:
hand-coded, tunable, no library.

---

## Sources

- Baby schema / cuteness science: [Glocker et al., PNAS 2009](https://www.pnas.org/doi/10.1073/pnas.0811620106) · [Glocker et al., Ethology 2009](https://onlinelibrary.wiley.com/doi/abs/10.1111/j.1439-0310.2008.01603.x) · [Lorenz "baby schema" review, Royal Society 2024](https://www.ncbi.nlm.nih.gov/pmc/articles/PMC11285920/)
- Disney 12 principles / squash & stretch: [NYFA overview](https://www.nyfa.edu/student-resources/12-principles-of-animation/) · [Thomas & Johnston context (Arlington Museum)](https://arlingtonmuseum.org/explore-more/the-twelve-principles-of-animation) · [Squash & stretch deep-dive](https://www.deedeestudio.net/en/post/principles-animation-squash-stretch)
- Springs / integration: [Ryan Juckett — Damped Springs](https://www.ryanjuckett.com/damped-springs/) · [Gaffer On Games — Integration Basics](https://gafferongames.com/post/integration_basics/) · [t3ssel8r — Giving Personality to Procedural Animations (video)](https://www.youtube.com/watch?v=KPoeNZZ6H4s) · [text mirror](https://github.com/SalvatoreScalia/Giving-Personality-to-Procedural-Animations-using-Math) · [Shiffman — Nature of Code, Oscillation](https://natureofcode.com/oscillation/)
- Easing: [easings.net](https://easings.net/) · [Robert Penner's Easing Functions](https://robertpenner.com/easing/)
- Game feel / juice: [Juice it or lose it — Jonasson & Purho (YouTube)](https://www.youtube.com/watch?v=Fy0aCDmgnxg) · [The Art of Screenshake — Vlambeer (YouTube)](https://www.youtube.com/watch?v=AJdEqssNZ-U) · [Game Feel — Steve Swink (Routledge)](https://www.routledge.com/Game-Feel-A-Game-Designers-Guide-to-Virtual-Sensation/Swink/p/book/9780123743282) · [Nicky Case — ncase.me](https://ncase.me/)
- Canvas / pixel art: [MDN — imageSmoothingEnabled](https://developer.mozilla.org/en-US/docs/Web/API/CanvasRenderingContext2D/imageSmoothingEnabled) · [MDN — Crisp pixel art look](https://developer.mozilla.org/en-US/docs/Games/Techniques/Crisp_pixel_art_look)
- Pixel-art tools: [Aseprite](https://www.aseprite.org/) · [Aseprite GitHub](https://github.com/aseprite/aseprite) · [Piskel](https://piskelapp.com/)
- 2D skeletal (for contrast): [Spine — Esoteric Software](http://esotericsoftware.com/) · [Live2D Cubism](https://www.live2d.com/)
