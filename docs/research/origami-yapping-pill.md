# Origami "Yapping Pill" — research (self-sufficient build dossier)

> Target app: **Yap** (formerly Wilson Voice). Stack: **Tauri 2** (Rust core + macOS
> system **WKWebView**) + **React 19 / TypeScript / Vite 7**. The pill is a separate
> MPA entry (`desktop/float.html` → `desktop/src/float-main.tsx` + `float.css`)
> rendered in a transparent always-on-top **NSPanel** (`src-tauri/src/float_pill.rs`,
> window 340×120 logical pt, pill ~44px tall). Waveform today is driven by a CSS var
> `--level` painted each rAF frame from Rust `audio_level` events.
>
> **Hard constraints for anything we adopt:**
> - **No CDN at runtime** — everything bundles locally (Vite import, or a static asset).
> - **License must be permissive** (MIT / BSD / Apache / MPL). The product license is
>   source-available **non-commercial** (BSL 1.1 / PolyForm). **GPL is fatal** — a GPL
>   runtime dep would force the whole app to GPL and is incompatible with BSL/PolyForm.
>   GPL code is only usable as an **offline build tool** whose *numeric output* (baked
>   geometry JSON) we ship — output data is not a derivative of the tool's source.
> - Must hit **60fps at low CPU** inside a tiny always-on-top transparent webview.
>
> This file is written so work can resume from it alone if the session hits a limit.

---

## TL;DR / decision up front

**Do NOT run a live rigid-origami physics solver in the pill.** It is the "correct"
motion but wrong for an always-on 340×120 panel (GPU context + per-frame relaxation =
battery/thermal). Instead:

1. **Bake once, offline:** run **Amanda Ghassaei's Origami Simulator** (MIT, three.js +
   WebGL GPU solver) on a *traditional* crane/bird-base crease pattern, sweep its
   `creasePercent` 0→1, and dump the folded **vertex positions at ~30–60 steps** to a
   small JSON file. (Rabbit Ear could also compute folds but it is **GPL-3.0** — only
   ever a build tool, never shipped.)
2. **Play back cheap, at runtime:** ship the baked JSON and interpolate a single
   scalar `t ∈ [0,1]` across the frames, drawing projected triangles with **Canvas2D**
   (no WebGL context, no three.js in the bundle). Unfold = `t: 0→1`; fold back =
   `t: 1→0` — the reverse is **bit-exact** because it is literally the same keyframe
   array played backwards. This is the only path that satisfies *"folds back the exact
   same way real origami does"* AND *60fps/low-CPU/always-on*.

Everything below is the evidence and the alternatives, per the brief.

---

## 1. Open-source origami / crease-pattern tooling that runs in a webview

### 1a. Amanda Ghassaei — Origami Simulator  ⭐ (the bake engine)
- Repo: <https://github.com/amandaghassaei/OrigamiSimulator>  ·  Live: <https://origamisimulator.org>  ·  Project page: <https://amandaghassaei.com/projects/origami_simulator/>
- **License: MIT** (verified via GitHub license API). Free to fork, embed, or run offline.
- **Tech:** `three.js` for the 3D scene + a **WebGL fragment-shader GPU solver**
  (`js/dynamic/GPUMath.js`, `js/dynamic/dynamicSolver.js`, `js/dynamic/GLBoilerplate.js`).
  It does *not* fold in sequential steps — it relaxes **every crease simultaneously**
  toward a target fold angle by iterating small displacements on a triangulated sheet.
  Extra deps in-repo: `svgpath`, `earcut`, `numeric.js`, `CCapture`, jQuery + jQuery UI.
- **Can it be driven 0→1 on a timeline? Yes.** The fold state is a single global:
  `globals.creasePercent` (0..1) with a setter `globals.setCreasePercent(percent)`
  and an auto-animate flag `globals.shouldAnimateFoldPercent` (see `js/globals.js`,
  lines ~49/106/109). The public UI is the "Fold Percent" slider (−100%..+100%; 0% flat,
  ±100% fully folded with opposite mountain/valley). So a script can ramp `creasePercent`
  and read geometry each step. **This is exactly what we exploit to bake.**
- **Imports:** SVG crease patterns and `.fold` files (`js/importer.js`). Ships a large
  example library under `assets/` — see §3 for which ones are license-safe.
- **Exports:** FOLD, STL, OBJ (`js/saveFOLD.js`, `js/saveSTL.js`) and video frames
  (`js/videoAnimator.js` via CCapture).
- **Caveat:** it is a **monolithic app, not a clean library** — no documented headless
  API. For baking we drive it in a headless browser (Playwright, available here via the
  `MCP_DOCKER` gateway) or a local `file://` load, and read the geometry out (details §5).
- **Why not run it live in the pill:** full three.js scene + jQuery UI + a GPU ping-pong
  solver iterating each frame, inside a transparent always-on-top panel, is heavy and
  hostile to battery. Great offline; wrong online.

### 1b. FOLD file format (edemaine/fold) — the interchange format
- Repo: <https://github.com/edemaine/fold>  ·  Spec: <https://github.com/edemaine/fold/blob/main/doc/spec.md>  ·  Viewer/API: <https://edemaine.github.io/fold/>  ·  npm: `fold`
- **License: MIT** (verified). Safe to ship the parser and any `.fold` data.
- **What it is:** FOLD = *Flexible Origami List Datastructure*, JSON describing a mesh:
  `vertices_coords`, `edges_vertices`, `edges_assignment` (M/V/B/F), `faces_vertices`,
  optional `faceOrders`, and folded-state coords. Because it is plain JSON, `JSON.parse`
  is the loader — no library required to read one.
- **What it does / doesn't do:** it is a **format + I/O + filters**, not a physics solver.
  The CLI `fold-convert --flat-fold -o B.fold A.fold` computes the *fully* flat-folded
  2D geometry of a crease pattern — but only the **end state**, not the intermediate
  partial-fold 3D angles an animation needs. So FOLD is our **canonical storage** for
  the crease pattern + our baked frames, but the *partial-angle timeline* must come from
  a solver (§1a) or a hand rig (§1e).
- **Integrate:** store each mascot's crease pattern as a `.fold` asset; parse with
  `JSON.parse`. Our baked keyframes can also be emitted as a FOLD "frames" array
  (`file_frames[]` with per-frame `vertices_coords`) so the format stays canonical.
- Forks worth knowing (same MIT format): `mayakraft/fold-file-format`, `amkraft/fold-file-format`.

### 1c. Rabbit Ear (robbykraft/Origami) — powerful, but GPL  ⚠️
- Repo: <https://github.com/robbykraft/Origami>  ·  Site: <https://rabbitear.org>  ·  npm: `rabbit-ear`
- **License: GPL-3.0** (verified via GitHub license API). **Do NOT ship it.** Bundling
  it would force Yap to GPL, which is incompatible with the chosen BSL 1.1 / PolyForm
  non-commercial license.
- **What it can do (why it's tempting):** full computational-origami library over the
  FOLD format — math lib, SVG + WebGL renderers, graph editing, and folded-form
  computation (`ear.graph.…` folded state from a flat CP). ES-module, UMD, and npm builds.
- **Only legitimate use here:** an **offline build tool**. Running a GPL program to
  *generate* geometry does not make the generated numbers GPL — the baked JSON is data,
  not a derivative of Rabbit Ear's source. So we *may* use it locally to compute folds,
  but we must not import it into the app or redistribute its code. Given Origami
  Simulator (MIT) already bakes what we need, **prefer the MIT tool and avoid the GPL
  entanglement entirely** unless we hit a wall.

### 1d. Tomohiro Tachi's simulators (Rigid Origami Simulator / Freeform / Origamizer)
- Software index: <https://origami.c.u-tokyo.ac.jp/~tachi/software/>  ·  Rigid-origami paper: <https://origami.c.u-tokyo.ac.jp/~tachi/cg/SimulationOfRigidOrigami_tachi_4OSME.pdf>
- These are the **academic origins** of rigid-origami simulation. They are **Java/native
  desktop apps** (not web, not a JS library you can bundle) and are **not** distributed
  under a clear permissive OSS license — treat as **reference/algorithms only**.
- Origami Simulator (§1a) already implements Tachi's *"Freeform Variations of Origami"*
  method for the web, so we get Tachi's math via the MIT codebase. Use the papers to
  understand rigidity/foldability; don't try to embed the Java tools.

### 1e. SVG / CSS fold approaches (hand-authored rig)
- **CSS 3D hinge fold — `paperfold` (mrflix):** <https://github.com/mrflix/paperfold> —
  **MIT.** Old (CSS3 `rotateX` + `transform-style: preserve-3d` folding of DOM strips).
  Not true origami, but the cleanest reference for a **pure-CSS hinge rig**: parent/child
  elements sharing an edge, each rotated about that edge; nesting reproduces accordion/
  book folds. Zero deps, trivial CPU, but authoring a crane this way is very laborious and
  won't match a real crease pattern.
- **`clip-path` polygon morph + 3D transforms:** animate `clip-path: polygon(...)` and
  layered `rotateX/rotateY` on stacked flaps. Cheap and GPU-composited, but you are
  *hand-faking* creases — acceptable for a stylized "beak opens" accent, not for a
  faithful unfold.
- **SVG path morphing (a *morph*, not a fold):**
  - **flubber** <https://github.com/veltman/flubber> — **MIT**, ~53KB. Best-guess
    interpolation between two arbitrary SVG shapes. Smooth, but it **tweens outlines**;
    it has no concept of creases, so it cannot express "the exact way an origami unfolds."
  - **polymorph-js** (npm `polymorph-js`) — **MIT**, lighter path morph.
  - **popmotion** `interpolate`/path morph (npm `popmotion`) — **MIT**.
  - **motion** (Framer Motion successor, npm `motion`) — **MIT**, React-friendly springs +
    path support. Good for micro-interactions and state springs (Dynamic-Island feel).
  - Verdict: morphing libs are great for the *reactive-mouth* accents and expand/contract
    springs, but **wrong for the fold** — a fold is a rigid crease articulation, not a
    silhouette tween.
- **GSAP + MorphSVG** — <https://gsap.com> — **AVOID.** License is the *"Standard 'no
  charge' license"* (`gsap.com/standard-license`), **not** an OSI/permissive license; it
  restricts building competing tooling and is a poor fit for a source-available repo. Use
  WAAPI/CSS instead. (MorphSVG would also only give a morph, not a real fold.)
- **Native Web Animations API (WAAPI):** built into WKWebView, **zero deps**, `element.
  animate()` with `playbackRate = -1` to reverse — ideal for orchestrating the pill's
  state springs and for reversing a hand rig exactly. (Polyfill `web-animations-js` is
  Apache-2.0 but unnecessary on modern WebKit.)

### 1f. Three.js / react-three-fiber — hinge-fold a flat net into a creature
- `three` — **MIT** (<https://threejs.org>); `@react-three/fiber` — **MIT**.
- **Approach:** build the flat net as faces; make each crease a **hinge** by parenting
  faces in a tree and rotating each child `Object3D` about the shared edge (a bone
  hierarchy). Ramp all hinge angles from a single `t` → rigid folding, reverse-exact by
  negating `t`. This is essentially a hand-built rigid-origami player.
- **Reference implementations:**
  - **georgiee/origami ("Origamizake")** <https://github.com/georgiee/origami> (demo
    <https://georgiee.github.io/origami/>) — **MIT.** A three.js origami *runtime* that
    folds a sheet by a sequence of fold instructions with crease visualization. The best
    starting point if we want *runtime* folding rather than baked playback.
  - **Codrops folding cardboard box** (three.js + GSAP) <https://tympanus.net/codrops/2022/12/13/how-to-code-an-on-scroll-folding-3d-cardboard-box-animation-with-three-js-and-gsap/> —
    tutorial for the parent/child hinge technique (swap GSAP for WAAPI/lerp to stay
    permissive).
- **Cost:** three.js adds ~150KB gzipped JS + a live WebGL context. For a *tiny* pill this
  is heavier than Canvas2D playback of baked frames, and a WebGL context that stays alive
  in an always-on-top panel is exactly the battery risk we want to avoid. Use r3f only if
  we later want live 3D lighting/shadows on the mascot; for v1, Canvas2D wins.

### 1g. Kirigami
- Origami Simulator handles kirigami (cuts) too — see `assets/Kirigami/*.svg` in its repo.
  Not needed for a crane/fox mascot; note it exists if a mascot needs slits.

---

## 2. Most practical path to "folds/unfolds exactly like real origami" at 60fps/low-CPU

Three candidate architectures, judged against: **fidelity** (is it a *real* crease
articulation?), **exact-reverse**, **runtime CPU/GPU**, **bundle size**, **authoring effort**.

| # | Approach | Fidelity of fold | Exact reverse | Runtime cost | Bundle | Authoring |
|---|----------|------------------|---------------|--------------|--------|-----------|
| (a) | **Live rigid-origami sim** (Origami Simulator solver in the pill) | Highest (true physics) | ~visually, not bit-exact | **High** (WebGL + per-frame relaxation, always-on) ❌ | Large (three.js+solver+jQuery) | Low |
| (b) | **Pre-baked keyframes** from a FOLD crease pattern (bake with §1a offline, play back) | **High** (frames come from the real solver) | **Exact** (reverse the array) ✅ | **Very low** (lerp + Canvas2D draw) ✅ | Tiny (JSON + ~0 lib) ✅ | Medium (one bake script/mascot) |
| (c) | **Hand-authored SVG/CSS 3D hinge rig** | Medium (looks folded, approximates creases) | Exact (reverse the transition) ✅ | Low ✅ | Tiny ✅ | **High** per mascot, and hard to match a real CP |

**Recommendation: (b) pre-baked keyframes.** Reasoning:

- It is the **only** option that is simultaneously *faithful to a real crease pattern*
  (the frames are produced by the actual rigid-origami solver) **and** cheap enough to run
  always-on. The heavy math happens **once at build time**, not on the user's battery.
- *"Folds back the exact same way"* becomes trivially true and provably exact: fold =
  advance `t` through frames `0→N`; unfold = walk `N→0`. Same geometry, reversed. No
  physics drift, no re-solve, no divergence between fold and unfold.
- **Runtime** is a scalar `t` eased over ~500–900ms, a per-frame linear interpolation of
  a few hundred vertex positions, a cheap painter's-order triangle draw in **Canvas2D**,
  and **the rAF loop only runs while animating** — at rest we draw one static folded frame
  and stop the loop (zero idle CPU). No WebGL context to keep alive in the panel.
- **Multiple mascots** (crane, fox, …) = just more baked JSON files, each ≤ a few hundred
  vertices × ~40 frames ≈ tens of KB. Swappable/collectible with no code change.

Use (c) only as a **fallback** for a v0 "good-enough beak/mouth open" if baking slips, and
(a) **only offline** as the bake engine.

---

## 3. Where to get crease patterns / nets for simple creatures (license-safe)

**Copyright nuance that matters:** a *traditional* origami model (the classic crane,
flapping bird, frog/waterbomb bases, boat, samurai hat, fox puppet) is **public-domain
folk knowledge** — its crease pattern is not copyrightable. But **named designs by living
authors are copyrighted even when the CP file sits in an MIT repo** — the repo's MIT
license covers the *code*, not an artist's design. So:

- ✅ **Safe (traditional / PD)** — from Origami Simulator `assets/` (repo is MIT; designs
  are traditional):
  - `assets/Origami/traditionalCrane.svg`, `assets/Origami/flat_crane.svg` — **the crane for v1.**
  - `assets/Origami/flappingBird.svg`, `assets/Origami/randlettflappingbird.svg` — bird/"creature".
  - `assets/Bases/birdBase.svg`, `assets/Bases/frogBase.svg`, `assets/Bases/waterbombBase.svg`,
    `assets/Bases/boatBase.svg`, `assets/Bases/squareBase.svg`, `assets/Bases/pinwheelBase.svg`
    — the classic bases; **frog base → fox/creature**, bird base → crane/other bird.
  - `assets/Pleating/*.fold` — box-pleat tessellations (abstract, PD).
- ❌ **Avoid (copyrighted authorship)** — also in that repo but NOT ours to redistribute
  as our mascots: `assets/Origami/langCardinal.svg`, `langKnlDragon.svg`, `langOrchid.svg`
  (Robert J. Lang), and any similarly attributed complex design. Use only for local
  experimentation, never ship.
- **FOLD example galleries** (MIT format, mostly traditional/abstract test models):
  edemaine/fold docs, and the Origami Simulator patterns above.
- **Polyhedra nets** (if a mascot is geometric): Wikimedia Commons has many **CC0/PD**
  nets (cube, dodecahedron, etc.) — public-domain mathematics.
- **Author our own** for anything custom: draw the crease pattern in Inkscape as an SVG
  with mountain/valley layered by stroke color (Origami Simulator's import convention:
  red = mountain, blue = valley, etc. — see its README), import → bake. This sidesteps
  all third-party-design questions and is the cleanest path for bespoke "Yap pets."
- **Rabbit Ear** ships example FOLD files too, but the repo is GPL — the *data* files are
  low-risk, but to keep the provenance clean, prefer the MIT sources or self-authored CPs.

For **v1**, use `traditionalCrane.svg` (or `birdBase.svg` for a simpler, faster bake).

---

## 4. Reactive "mouth" / listening→thinking→done references + OSS viz libs

**How the reference apps convey state (design language to borrow):**
- **superwhisper** (local Whisper Mac app) — minimalist orb/pill that **pulses** while
  listening; calm, privacy-forward. State via subtle scale/opacity breathing.
- **Wispr Flow** — bottom bar with a live **waveform** + streaming words; a word-count/
  competition bar (matches Yap's "yapping" + leaderboard direction). Motion = amplitude-
  reactive bars.
- **macOS/iOS Siri** — "listening" = animated **waveform / glowing orb**; iOS 18 = an
  **edge-of-screen glow**. "Thinking" = the orb churns; "done" = settle. The key trick is
  a *continuous* organic motion (sine-summed waves) so there's never a frozen frame.
- **Dynamic Island** — a capsule that **springs** between compact and expanded, morphing
  its content with spring physics. This is the exact metaphor for Yap's fold/unfold: a
  living capsule that changes shape by state. (3rd-party Mac clones exist — *DynamicLake*/
  *DynamicLake by AviorProd* — as visual reference only.)
- **Map to Yap's states** (fills the "gaps" Wilson flagged): `idle` (folded, breathing) →
  `listening` (unfold to mascot; mouth open; waveform = amplitude) → `transcribing/
  thinking` (mascot "chewing"/shimmer — never frozen) → `polishing` → `pasting` (a
  confirm beat) → `done` (satisfied settle) → **fold back to idle** (exact reverse).

**OSS waveform/mouth libraries (permissive):**
- **siriwave** (kopiro) <https://github.com/kopiro/siriwave> — **MIT.** Siri-style
  sine-summed waves on a `<canvas>`; `setAmplitude()` drives it. Drop-in for the
  "listening mouth." Tiny, no deps.
- **wavesurfer.js** (katspaugh) <https://github.com/katspaugh/wavesurfer.js> — **BSD-3.**
  Fuller waveform toolkit; heavier than we need for a pill but good if we want real FFT
  bars.
- **Lottie-web** (airbnb) <https://github.com/airbnb/lottie-web> — **MIT**, ~60–100KB.
  Plays a designer-made After Effects JSON. Could host a pre-baked "mouth/blink/idle"
  loop and even a **fake fold** (AE, reversed via `direction=-1`) — but a Lottie fold is a
  *drawn* animation, **not** a real crease articulation, so it fails the "exact origami"
  test for the fold itself. Fine for eyes/mouth accents.
- **Rive** (`rive-app/rive-wasm`, `rive-app/rive-react`) — **runtime MIT**, ~200KB gzip
  (incl. WASM). Its **state machine reacts to runtime inputs** — feed it `wordCount`,
  `amplitude`, `state` and it drives a hand-authored mascot with mesh deform. This is the
  strongest option for the **reactive personality/mouth** (not the physically-exact fold).
  The **editor is a proprietary SaaS** (free tier) but the *file* is exported to a local
  `.riv` we bundle, and the runtime is MIT — so it satisfies the no-CDN/permissive-runtime
  rule. Consider Rive for v2 "expressive mascot," Canvas2D baked frames for the v1 fold.
- **Home-rolled** (what the pill uses today): CSS-var-driven bars + a Canvas FFT. Cheapest;
  already in the codebase. Keep for the waveform; add the fold on top.

**Word-count-reactive "yapping" copy + curse filter (permissive):**
- The message tiers live in a config table keyed by word-count bucket × tone
  (Rude/Sassy, Friendly, Rose), picked deterministically per session with light rotation.
- Curse-filter toggle — swap or mask words with an OSS list/filter:
  - **leo-profanity** (npm) — **MIT**, has a clean-word list + `clean()`/`check()`.
  - **bad-words** (npm) — **MIT**.
  - **2Toad/Profanity** <https://github.com/2Toad/Profanity> — **MIT**, fast/typed.
  - **dsojevic/profanity-list** <https://github.com/dsojevic/profanity-list> — **MIT** word list.
  - Implementation: keep two copy variants per line (sassy-explicit vs sassy-clean); the
    filter toggle just selects the variant table — cleaner than runtime masking and avoids
    false positives on the reactive lines themselves.

---

## 5. Concrete minimal build plan — v1 "unfolding crane pill"

**Goal:** at rest the pill shows a **folded crane** (static, no rAF). On dictation start it
**unfolds** to the open crane over ~700ms; on stop it **folds back** over ~700ms (exact
reverse). Runs at 60fps, ~0 idle CPU, no WebGL context, fully bundled, MIT/permissive only.

### Step 0 — pick the pattern
Use `traditionalCrane.svg` (fidelity) or `birdBase.svg` (fewer faces = faster bake, smaller
JSON) from Origami Simulator's MIT `assets/`. Store a copy in
`desktop/src/mascots/<name>/crease.svg` (and optionally a `.fold` copy) for provenance.

### Step 1 — bake the fold timeline (offline, one-time, MIT tool only)
Drive Origami Simulator headlessly and dump geometry. Two ways:

- **Playwright (available via the `MCP_DOCKER` gateway):** load a local checkout of
  Origami Simulator, `importSVG` the crane, then loop:
  ```js
  // pseudo — runs INSIDE the Origami Simulator page context
  const N = 48, frames = [];
  for (let i = 0; i <= N; i++) {
    globals.setCreasePercent((i / N) * 100);   // 0 → 100 %
    // let the GPU solver relax a few ticks toward this target:
    for (let k = 0; k < 8; k++) model.step?.() ?? globals.threeView.render();
    frames.push(Array.from(model.getPositions())); // Float32 xyz per node
  }
  // also grab faces once:
  const faces = model.getFaces();  // vertex-index triples
  return { frames, faces };
  ```
  Exact accessor names live in `js/model.js` / `js/globals.js` / `js/dynamic/dynamicSolver.js`
  — read those to confirm (`getPositions`/`getNodePositions`, `getFaces`, and how the solver
  advances). If no clean getter exists, read the WebGL solver's position texture, or
  `saveFOLD` per step and parse the resulting `vertices_coords`.
- **Fallback:** use the in-app **Save FOLD** at several fold percentages by hand and parse
  those `.fold` files. Fewer frames, but zero scripting.

**Output:** `desktop/src/mascots/crane.json`:
```jsonc
{
  "name": "crane",
  "faces": [[0,1,2],[2,3,0], ...],        // static triangle indices
  "frameCount": 49,
  "frames": [ [x0,y0,z0, x1,y1,z1, ...],  // frame 0 = flat/unfolded
              ... ,
              [ ... ] ],                    // frame N = folded (rest pose)
  "restFrame": 48                          // which frame is "at rest" (folded)
}
```
Normalize/scale positions to fit the pill box at bake time so runtime does no fitting math.
Size estimate: 300 verts × 49 frames × 3 floats ≈ ~44k numbers ≈ ~150KB raw / ~30KB gzip.
Reduce with fewer frames (interpolation covers the gaps) or vertex decimation if needed.

### Step 2 — runtime player (Canvas2D, in the existing MPA pill)
Add a `<canvas className="mascot">` to `float-main.tsx` alongside the current waveform.
A tiny player module:
```ts
// desktop/src/mascot/foldPlayer.ts
export function makeFoldPlayer(canvas: HTMLCanvasElement, mascot: Mascot) {
  const ctx = canvas.getContext("2d")!;
  let raf = 0, t = 1, target = 1; // t=1 folded (rest), t=0 unfolded
  const F = mascot.frameCount - 1;

  function lerpFrame(t: number, out: Float32Array) {
    const f = t * F, i = Math.floor(f), a = f - i;
    const A = mascot.frames[i], B = mascot.frames[Math.min(i + 1, F)];
    for (let k = 0; k < out.length; k++) out[k] = A[k] + (B[k] - A[k]) * a;
  }
  const buf = new Float32Array(mascot.frames[0].length);

  function draw() {
    lerpFrame(t, buf);                    // interpolate positions
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    for (const [i0, i1, i2] of mascot.faces) {
      // orthographic project (x,y); z → shading. painter's order by avg z.
      // ctx.moveTo/lineTo the three projected verts, fill w/ z-based lightness.
    }
  }
  function tick() {
    t += (target - t) * 0.18;             // critically-ish damped ease
    draw();
    if (Math.abs(target - t) > 0.001) raf = requestAnimationFrame(tick);
    else { t = target; draw(); raf = 0; } // settle, STOP the loop (0 idle CPU)
  }
  return {
    unfold() { target = 0; if (!raf) raf = requestAnimationFrame(tick); },
    fold()   { target = 1; if (!raf) raf = requestAnimationFrame(tick); },
    dispose() { if (raf) cancelAnimationFrame(raf); },
  };
}
```
- Wire `unfold()` to the `recording` event turning true, `fold()` to it turning false —
  reuse the existing Tauri `listen("recording", …)` / `listen("status", …)` in
  `float-main.tsx`. The fold is the reverse of the unfold **by construction** (same `t`,
  opposite target), satisfying the "exact reverse" requirement with no extra work.
- Draw at `devicePixelRatio` (Retina) and size the canvas to the ~44px pill; painter's-
  order triangle fill with z-based lightness gives a convincing paper look with no WebGL.
- Keep the current amplitude waveform for the "mouth" during `listening`; the crane is the
  identity, the waveform is the voice.

### Step 3 — reactive "yapping" text + tone/curse filter
- On `done`, read `word_count` (already computed in Rust / stored on `transcripts`), bucket
  it, and show a line from `copy[tone][bucket]` with light rotation. Tone + curse toggle in
  Settings select the variant table (see §4). Keep copy in a config table for l10n.

### Step 4 — states / gap-fill
Extend the state machine to `idle → listening → transcribing(thinking) → pasting → done →
idle`, giving `transcribing` a non-frozen beat (mascot "chews": small idle wobble frames or
a shimmer over the folded shape), and a `done` settle before folding back. Reuse WAAPI for
the capsule expand/contract springs (Dynamic-Island feel), zero deps.

### CPU / perf risks (and mitigations)
- **Always-on WebGL context = battery/thermal** → *avoid it*: Canvas2D playback, no live
  three.js. (This is the #1 reason not to run the sim live.)
- **rAF running at idle** → stop the loop when `t` reaches target; at rest draw one static
  folded frame and cancel rAF. Never keep a 60fps loop alive for a static pill.
- **Transparent NSPanel compositing** on top of everything can be costly per repaint →
  only repaint during the ~700ms fold and during active waveform; idle = static.
- **Retina over-draw** in a 340×120 panel → size canvas to actual pill px × DPR, not the
  whole window; the window is oversized only for shadow room.
- **Baked JSON size** → 30–60 frames is plenty (lerp fills between); decimate the mesh if a
  complex crane is heavy. Prefer `birdBase` if the crane's face count bloats the file.
- **Bundle size** → Canvas2D path adds ~0 runtime deps; if we later adopt Rive for the
  expressive mascot, budget ~200KB and load its `.riv` locally (no CDN).
- **Bake reproducibility** → check the bake script + `crane.json` into the repo so the
  mascot can be regenerated; the GPL-tool concern doesn't apply because we ship data, and
  we're using the MIT tool anyway.

---

## Ranked recommended stack for v1

1. **Fold engine (offline bake): Amanda Ghassaei Origami Simulator — MIT.** Sweep
   `globals.creasePercent` 0→1 over a *traditional* crane/bird-base CP; dump per-step vertex
   positions to `crane.json`. Real rigid-origami motion, computed once, zero runtime cost.
   <https://github.com/amandaghassaei/OrigamiSimulator>
2. **Crease-pattern format & storage: FOLD (edemaine/fold) — MIT.** Canonical `.fold` for
   each mascot's CP and (optionally) the baked frames; loader is just `JSON.parse`.
   <https://github.com/edemaine/fold>
3. **Runtime fold player: Canvas2D + a ~40-line lerp/projection module — zero deps.** Play
   baked frames with a single eased `t`; unfold `0→1`, fold `1→0` = provably exact reverse;
   rAF only while animating. (Code sketch in §5.)
4. **State springs / expand-contract: native WAAPI (`element.animate`, `playbackRate=-1`)
   — zero deps.** Dynamic-Island feel; exact reversibility for hand-rigged accents.
5. **Listening "mouth" waveform: keep the current CSS-var bars; optionally siriwave (MIT)**
   for a Siri-style organic wave. <https://github.com/kopiro/siriwave>
6. **Reactive copy + curse filter: config table + leo-profanity / bad-words / 2Toad
   Profanity (all MIT)** — toggle selects a clean vs sassy variant table.
7. **v2 expressive mascot (optional): Rive runtime (rive-app, MIT, local `.riv`)** for
   word-count-reactive personality/mesh-deform on top of the baked fold. Editor is SaaS but
   the shipped file + runtime are permissive and CDN-free. <https://github.com/rive-app/rive-wasm>

**Explicitly rejected / caution:**
- **Rabbit Ear (robbykraft/Origami) — GPL-3.0:** never shipped; offline build-tool only (and
  even then, prefer the MIT Origami Simulator). <https://github.com/robbykraft/Origami>
- **GSAP / MorphSVG — "no charge" non-OSI license:** avoid in a source-available repo; use
  WAAPI/CSS. <https://gsap.com/standard-license>
- **Live rigid-origami sim in the pill / always-on three.js WebGL:** correct motion, wrong
  runtime — battery/thermal on an always-on-top panel. Bake instead.
- **flubber / polymorph / Lottie for the *fold itself*:** these are silhouette *morphs* or
  *drawn* animations, not crease articulations — they can't satisfy "exact origami unfold."
  Fine for mouth/eye accents, not the fold. (flubber MIT, polymorph-js MIT, lottie-web MIT.)

### License summary (verified this session)
| Library / asset | License | Ship at runtime? | Role |
|---|---|---|---|
| amandaghassaei/OrigamiSimulator | **MIT** | offline bake (or fork) | fold engine |
| edemaine/fold | **MIT** | ✅ | CP format + loader |
| three.js / @react-three/fiber | **MIT** | optional (v2) | 3D hinge fold |
| georgiee/origami (Origamizake) | **MIT** | reference / optional | runtime folding ref |
| mrflix/paperfold | **MIT** | reference | CSS hinge ref |
| veltman/flubber | **MIT** | accents only | shape morph |
| polymorph-js / popmotion / motion | **MIT** | accents / springs | morph + springs |
| kopiro/siriwave | **MIT** | ✅ | listening waveform |
| katspaugh/wavesurfer.js | **BSD-3** | optional | FFT waveform |
| airbnb/lottie-web | **MIT** | accents only | drawn loops |
| rive-app/rive-wasm, rive-react | **MIT (runtime)** | ✅ (local .riv) | v2 mascot |
| leo-profanity / bad-words / 2Toad Profanity / dsojevic/profanity-list | **MIT** | ✅ | curse filter |
| **robbykraft/Origami (Rabbit Ear)** | **GPL-3.0** ⚠️ | ❌ never | offline-only |
| **GSAP / MorphSVG** | **"no charge" (non-OSI)** ⚠️ | ❌ avoid | — |
| Origami Simulator `assets/…Lang…` designs | author copyright ⚠️ | ❌ | don't redistribute |
| traditional crane / bird/frog/waterbomb bases | public domain ✅ | ✅ | v1 mascots |

---
*Verified via GitHub license API + npm registry + repo source reads on 2026-07-22. Fold
percentage global (`globals.creasePercent` / `setCreasePercent` / `shouldAnimateFoldPercent`)
confirmed in Origami Simulator `js/globals.js`. Example crease-pattern paths confirmed in that
repo's `assets/` tree. `fold-convert --flat-fold` (end-state only) confirmed in edemaine/fold
README.*
