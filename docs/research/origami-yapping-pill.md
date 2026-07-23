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

---

## Geometric crease rendering (v2 research)

> **Why v2.** The first prototype folded a *hand-authored silhouette* — it "looked like a
> pill in modes," not origami, because it morphed an outline instead of articulating a
> crease pattern. v2's goal is **genuine geometric crease folding**: a visible crease
> pattern (mountain/valley lines) whose **flat facets rotate rigidly about their shared
> creases** and collapse into a 3D creature, projected to 2D — the paper look with *real
> crease structure*. The core primitive is **hinge-tree forward kinematics** (each face =
> parent's transform ∘ a rotation about the shared crease edge), which is hand-authorable
> in ~150 lines of Canvas2D and needs **zero third-party runtime code** (so licensing is
> moot for the engine itself). Everything below is the concrete math, the real repos, and
> a copy-pasteable sketch.

### v2.1 — Rigid-origami FOLDING engines in JS: which give a usable MIT playback path?

The key distinction is **flat-folded state** (final 2D geometry + which layer is on top —
what you need for a *painter's-order* stack) vs **partial-fold angles over time** (the
per-crease dihedral angles at each `t`, what an *animation* needs). Most tools only do the
former; only a solver or a hinge rig gives the latter.

| Repo | License | Live in webview? | Gives partial-fold **angles/animation**? | Role for us |
|---|---|---|---|---|
| **amandaghassaei/OrigamiSimulator** <https://github.com/amandaghassaei/OrigamiSimulator> | **MIT** | yes but heavy (three.js + GPU solver) | **Yes** — global `creasePercent` ramps every crease's dihedral | **Bake engine** (offline sweep, §v1) *or* reference for the fold-angle model |
| **georgiee/origami** ("Origamizake") <https://github.com/georgiee/origami> · demo <https://georgiee.github.io/origami/> | **MIT** | yes (three.js runtime) | **Yes-ish** — folds by *reflecting* half-planes across crease lines; "choose a model, **skip through steps with a slider**" = step playback | The only **MIT runtime-folding** JS project; study its reflect-across-crease op |
| **origamimagiro/flat-folder** (Jason Ku) <https://github.com/origamimagiro/flat-folder> | **MIT** (verified) | yes (pure JS browser app) | **No** — computes **flat-folded state + layer/overlap order only**; imports FOLD/SVG/**OPX/CP**, exports FOLD | **Offline** tool to get the correct **stacking order** of the flat pose (feeds painter's-sort) — not an animator |
| **edemaine/fold** <https://github.com/edemaine/fold> | **MIT** | yes (parser is `JSON.parse`) | **No** — format + I/O + `--flat-fold` **end state only** | Canonical CP storage + loader (§v1) |
| **robbykraft/Origami** (Rabbit Ear) <https://github.com/robbykraft/Origami> | **GPL-3.0** ⚠️ | — | Yes (full fold solver) | **Study only / offline** — never ship (GPL is fatal to BSL/PolyForm) |
| **Tachi** Freeform / Rigid Origami Sim <https://origami.c.u-tokyo.ac.jp/~tachi/software/> | Java, no clear OSS license | no | Yes (academic origin) | **Algorithms/reference only** — OrigamiSimulator already ports Tachi's method to MIT web |

**How OrigamiSimulator maps `creasePercent` → vertex positions (the model to copy).**
Confirmed by source read (`js/dynamic/dynamicSolver.js`) + Ghassaei's paper *"Fast,
Interactive Origami Simulation using GPU Computation"*:

1. On init, **every crease stores a fixed target dihedral** `creaseMeta[i*4+2] =
   crease.getTargetTheta()` — that's the crease's *fully-folded* angle, `±π` scaled by the
   material's fold limit, sign = mountain(−)/valley(+).
2. The global fold slider is pushed to the GPU as a uniform on **every** solver step:
   `gpuMath.setUniformForProgram("velocityCalc", "u_creasePercent", percent, "1f")` and
   likewise for `positionCalcVerlet`.
3. In the GLSL, the **instantaneous target for each crease is `u_creasePercent *
   targetTheta`**. A torsional (crease) spring applies a force proportional to
   `(currentθ − creasePercent·targetTheta)`, alongside **axial** (edge-length) and **face**
   constraints that keep facets rigid; node positions integrate by **Verlet**. It relaxes
   *all* creases simultaneously toward the scaled target — it does **not** fold in sequence.

The load-bearing takeaway for a hand rig: **`creasePercent` is one scalar `t` that
multiplies every crease's flat-fold angle.** That is *exactly* the single-DOF ramp a
hinge-tree uses — `φ_crease(t) = t · φ_crease^flat`. OrigamiSimulator just also *solves the
loop-closure/rigidity coupling numerically* so arbitrary patterns stay physical; a hinge
tree gets that coupling for free only when the face graph is a tree (see §v2.2).

**Verdict.** There is **no drop-in MIT library that plays back partial-fold angles of an
arbitrary crease pattern live and cheaply.** The usable MIT paths are: (a) **bake** frames
from OrigamiSimulator offline and lerp them (best fidelity for a complex creature — the v1
plan), or (b) **hand-author a hinge-tree** for the bases/mouth (real crease articulation,
no dep, trivial CPU — best for the pill's fold + mouth). Use **flat-folder** offline once
per model to lock the flat-pose **layer order** so the painter's sort never flickers.

### v2.2 — Kinematics you can author by hand in ~150 lines

#### The hinge-tree fold (the engine) — exact math

Represent the sheet as **rigid facets** joined at creases. Build a **spanning tree of the
face-adjacency graph**: pick a root face, and every other face has exactly one *parent* it
shares a crease edge with. One scalar `t ∈ [0,1]` (plus, optionally, an independent `mouth`
DOF) drives all fold angles.

- Material (flat) coordinates: each vertex `i` has `v_i = (x_i, y_i, 0)`.
- Root face `r`: world transform `M_r = I` (its folded coords = its flat coords).
- For a child face `f` sharing crease edge `(a, b)` with its parent `p`, with signed
  dihedral fold angle `φ_f(t)`:
  - Hinge axis in **material** coords: `k = normalize(v_b − v_a)`, point on axis `A = v_a`.
  - **Rotation about the line** `(A, k)` by `φ`, as an affine `H = (R, τ)` via **Rodrigues**:

    ```
    R = I·cosφ + sinφ·[k]×  + (1−cosφ)·k kᵀ        // 3×3
    τ = A − R·A                                     // so the line (A,k) is fixed
        [k]× = [[0,−kz, ky],[kz,0,−kx],[−ky,kx,0]]
    ```
  - **Compose with the parent** (both expressed in material coords, then folded together):
    `M_f = M_p ∘ H`, i.e. `M_f(v) = M_p( R·v + τ )`.
  - World position of any material vertex `v` on face `f`: `P = M_f · v`.

  Because `H` **fixes the crease line** `(a,b)` and `M_p` is applied to both faces'
  shared coordinates identically, the shared edge verts computed from the parent and from
  the child are **bit-identical** — the paper never tears. Fold angle `φ_f(t) = t ·
  φ_f^flat` (with `φ^flat = ±π·assignment`) ⇒ `t=0` is the flat crease pattern, `t=1` the
  collapsed creature; **unfold = run `t` backwards**, provably exact (same as v1).

- **Render:** apply a fixed view rotation `V` (small pitch+yaw so you see 3D), then
  **orthographic project** (`sx = cx + s·(V·P)x`, `sy = cy − s·(V·P)y`), keep `(V·P)z` for
  depth; **painter-sort faces by centroid depth** (far first); **shade** each facet by
  `lightness ∝ |n̂ · L̂|` where `n̂` is the facet normal (cross product of two edges) — this
  flat-per-facet shading is what *reads as folded paper*. Stroke the facet edges faintly =
  visible creases.

**The one honest caveat (loops).** A real interior vertex makes its surrounding faces form
a **cycle**, and a spanning tree cuts one crease of that cycle. The fold angles around a
loop are **not independent** — they're coupled by a rigidity/loop-closure constraint, so
naively setting every `φ = t·φ^flat` leaves a small gap at the cut crease for a general
pattern. Two clean ways to stay genuinely rigid by hand:
1. **Author tree-structured "folds"** (a fold *sequence* / a net with no closed interior
   vertex) — wings, neck, head, beak-jaws hinged off a body all form a tree, zero coupling.
   This is what the code sketch below does and what georgiee/origami's reflect-op does.
2. **Solve the coupling in closed form for a single vertex** (below) and drive the loop's
   creases from one driver angle — good for the symmetric bases.

#### Single-vertex fold (waterbomb / cootie-catcher / preliminary base) — the closed form

A single interior vertex is a **spherical linkage**: put the vertex at a sphere's center;
each sector angle becomes a spherical arc, each fold angle is the supplement of an interior
angle of the spherical polygon (Huffman). Governing facts:

- **Flat-foldability (Kawasaki–Justin):** alternate sector angles sum to π
  (`α₁+α₃+… = α₂+α₄+… = π`). **Maekawa–Justin:** `#mountain − #valley = ±2`.
- **A developable degree-4 vertex is a spherical 4R linkage ⇒ exactly 1 DOF.** Under the
  **Weierstrass / half-angle substitution `tᵢ = tan(ρᵢ/2)`** the fold-angle relations
  become *linear*: pick one crease as the **driver** and every other fold angle follows

  ```
  tan(ρ_j / 2) = c_j · tan(ρ_driver / 2)          // c_j fixed by the sector angles
  ```

  (Huffman's spherical-trig relations, simplified via Weierstrass; see
  [royalsociety, Parametric solutions to degree-4 vertices, 2023](https://royalsocietypublishing.org/rspa/article/479/2279/20230319/101163/Parametric-solutions-to-the-kinematics-of)
  and [arXiv 2408.08816, Lorentz transformation for degree-4 vertices](https://arxiv.org/abs/2408.08816)).
  Drive `ρ_driver = t·π`, compute the others from the constant ratios → a **genuinely
  rigid, loop-closing 1-DOF fold** you can hand-code.
- **Symmetric vertices are trivial.** The **preliminary base** and **waterbomb base** are
  the *same* crease pattern — a square with both diagonals **and** both book-fold midlines
  through the center (a degree-8 vertex) — differing **only in mountain/valley assignment**
  (preliminary: midlines valley / diagonals mountain → collapses to a square-with-flaps;
  waterbomb: inverted → collapses to a triangle/balloon). By symmetry the collapse is a
  **1-DOF path with all fold-angle magnitudes equal**, so you can drive it with one `t` and
  a single per-crease `±` sign. *This is exactly what OrigamiSimulator's `±creasePercent`
  does: −100% is the M/V-inverted collapse of the same sheet — preliminary ⇄ waterbomb.*
- **Bird base** = preliminary base + four **petal folds** (more faces, still 1-DOF for the
  symmetric collapse) → your "crane/bird" creature.
- **Cootie-catcher / fortune-teller = the MOUTH.** Once folded, it's four rigid quad flaps
  hinged along the center spine; **pinching opens/closes it as a genuine 1-DOF rigid
  motion** — no flat-fold solve needed, just one dihedral. Model it directly as a hinge with
  an *opening* angle and drive that angle from mic amplitude (§v2.5). This is the cleanest,
  most "origami" audio-reactive jaw available.

#### The five bases, as authoring recipes

| Base | Crease pattern (square) | Central vertex | Collapsed 3D form | Hinge-rig note |
|---|---|---|---|---|
| **Preliminary** | 2 diagonals + 2 midlines | degree-8 | small square + 4 flaps | 1-DOF symmetric; parent of bird/crane |
| **Waterbomb** | *same lines*, M/V inverted | degree-8 | triangle / balloon | drive with `−t` of preliminary |
| **Bird** | preliminary + 4 petal folds | — | 4 long points (crane body) | tree of petal faces off the preliminary |
| **Cootie-catcher** | 2 diagonals + 2 midlines, blintzed | — | 4 pockets → **opening mouth** | one *opening* dihedral = the jaw |
| **Cootie mouth (min)** | — | — | 2 jaw flaps sharing a spine edge | upper/lower beak on shared crease `[a,b]`, `±m` |

### v2.3 — Where to get crease patterns / nets (permissive / public-domain)

Copyright rule (unchanged, critical): **traditional models are public-domain folk
knowledge** (crane, flapping bird, preliminary/waterbomb/bird/frog bases, cootie catcher,
fox puppet, boat, samurai hat); **named modern designs by living authors are copyrighted
even when the `.cp`/SVG sits in an MIT repo** — the repo's license covers the code, not the
artist's design.

- ✅ **OrigamiSimulator `assets/`** (repo MIT; designs traditional): `Origami/traditionalCrane.svg`,
  `flappingBird.svg`; `Bases/birdBase.svg`, `frogBase.svg`, `waterbombBase.svg`,
  `squareBase.svg`, `pinwheelBase.svg`, `boatBase.svg`. (Crane, fox-from-frog-base, bird.)
- ✅ **flat-folder** imports/ships FOLD + **`.cp` (Oripa)** + OPX + SVG — a good place to grab
  or normalize **traditional** CP files and re-export clean FOLD (MIT tool, offline).
- ✅ **edemaine/fold** example galleries (MIT format; traditional/abstract test models).
- ✅ **`.cp` files from Oripa / public CP archives** — but *filter to traditional/PD designs
  only*; skip anything credited to a named designer (Lang, Kamiya, etc.).
- ✅ **Wikimedia Commons polyhedra/animal nets** — many **CC0/PD** (public-domain math) for
  geometric creatures (crab/dog built from boxes, frog from waterbomb).
- ✅ **Self-author in Inkscape** — draw the CP as an SVG, **red stroke = mountain, blue =
  valley** (OrigamiSimulator import convention), import → bake, *or* type the vertices
  straight into the hinge-rig's `flat[]` array. This sidesteps every third-party-design
  question and is the cleanest path for bespoke "Yap pets."
- ❌ **Do not ship** `assets/…Lang…` (Cardinal, KnlDragon, Orchid) or any named-designer CP —
  local experimentation only.

For **crane / fox / frog / crab / dog** specifically: crane + flapping bird ship as
traditional SVGs; **fox and frog** come from the **frog base**; **crab and dog** are easiest
as **box-pleat / geometric nets** (author your own or use CC0 nets) rather than hunting a
copyrighted CP.

### v2.4 — CSS 3D "origami" hinge technique (nested `preserve-3d`)

Real, permissive references for folding DOM in 3D by rotating nested elements about their
shared edges:

- **OriDomi** (dmotz) <https://github.com/dmotz/oridomi> · <http://oridomi.com/> — **MIT**,
  standalone (no hard deps). "Fold up DOM elements like paper." Slices a target element into
  panels and folds them with CSS 3D transforms; requires `transform`/`preserve-3d`. The most
  complete pure-CSS origami-fold library.
- **mrflix/paperfold** <https://github.com/mrflix/paperfold> — **MIT** (in v1 list). The
  cleanest minimal reference for the parent/child hinge (`rotateX` + `transform-style:
  preserve-3d`).
- **Codrops "3D Folding Layout Technique"** <https://tympanus.net/codrops/2020/01/14/3d-folding-technique/>
  — the canonical write-up. Wrapper gets `perspective` + `transform-style: preserve-3d`;
  each fold panel gets a **`transform-origin` on the shared edge** (`bottom center` /
  `top center`) and an opposite **`rotateX(±deg)`**; **`overflow:hidden`** hides the content
  outside each panel so the seam reads as one folded sheet. Known wart: **tiny sub-pixel line
  gaps between folds** in some browsers (mitigate by scaling parent up / child down).
- **davidwalsh.name/3d-transforms** + **/folding-animation** — nesting-coordinate-system
  explainers (a child's transform is relative to its already-rotated parent — the same
  hinge-tree composition as §v2.2, done by the CSS compositor).

**The core CSS pattern** (each crease is a nested div; the child's coordinate frame rides
its parent, so folding a parent folds all its children):

```html
<div class="scene" style="perspective:600px">
  <div class="face" style="transform-style:preserve-3d">
    <!-- child hinged along its TOP edge: -->
    <div class="face child"
         style="transform-origin:top center; transform:rotateX(var(--fold));
                transform-style:preserve-3d; backface-visibility:visible">
      …grandchild hinged on ITS edge…
    </div>
  </div>
</div>
```
Drive `--fold` (or `element.animate(...,{...})` with `playbackRate=-1` to reverse exactly).

**When CSS beats Canvas:** few facets; you want **crisp DOM/text/images on the facets**
(the pill's logo, a word count *on* the paper); GPU-composited transitions with ~zero JS;
reversibility via WAAPI. **When Canvas wins (our fold):** many facets, **per-facet normal
shading** (the paper look), **true depth painter-sort** across interleaving flaps, and one
scalar driving *dozens* of coupled creases — CSS can't z-sort arbitrary interpenetrating
panels or shade by normal, and per-facet `preserve-3d` nesting gets unwieldy past ~8 folds.
**Recommendation: Canvas2D hinge-tree for the creature fold; CSS `preserve-3d` only for a
simple 2–4 panel accent** (e.g. an unfolding "receipt" of the transcript) where real DOM
content must live on the paper.

### v2.5 — Audio-reactive "mouth" (mic RMS → jaw dihedral)

Yap already exposes a live mic level (`--level`, Rust `audio_level` events). To open/close an
origami beak or cootie-catcher mouth **in sync with speech**, map a **smoothed loudness
envelope** to a single **jaw dihedral** — an independent DOF `mouth` alongside the fold `t`.

1. **Get RMS** from a Web Audio `AnalyserNode` on the mic stream (or reuse the existing
   level; RMS is the honest loudness):
   ```js
   const buf = new Float32Array(analyser.fftSize);       // time-domain
   analyser.getFloatTimeDomainData(buf);
   let s = 0; for (const x of buf) s += x*x;
   const rms = Math.sqrt(s / buf.length);                // 0..~0.3 for speech
   ```
2. **Noise-gate + normalize** to `0..1` (subtract a floor so silence = fully-closed, and a
   gamma < 1 to make quiet speech visibly move the jaw):
   ```js
   const FLOOR = 0.02, REF = 0.18, GAMMA = 0.7;
   let level = Math.min(1, Math.max(0, (rms - FLOOR) / REF));
   level = Math.pow(level, GAMMA);
   ```
3. **Asymmetric attack/release smoothing** (fast open on an onset, slower close so the mouth
   doesn't chatter) — one-pole EMA with two rates:
   ```js
   const ATTACK = 0.6, RELEASE = 0.15;                   // per-frame @60fps
   env += (level - env) * (level > env ? ATTACK : RELEASE);
   ```
4. **Map env → jaw dihedral** (radians), with a tiny idle so it's *never a frozen frame*:
   ```js
   const JAW_MIN = 0.05, JAW_MAX = 0.6;                  // ~3°..34°
   const idle = 0.02 * Math.sin(performance.now()/650);  // subtle breathing
   const mouth = JAW_MIN + env * (JAW_MAX - JAW_MIN) + idle;
   ```
   Feed `mouth` to the beak faces' angle functions (upper jaw `+mouth`, lower jaw `−mouth`).
   During *listening* the rAF loop runs anyway (waveform), so the mouth is free; at idle,
   `env→0` and you can drop back to a static folded frame.

Optional polish: split `rms` into a low band (jaw amplitude) and a high band (a slight
"lip"/wing flutter) via two `AnalyserNode`s with different `smoothingTimeConstant`, but a
single RMS→jaw map already reads clearly as talking.

### v2.6 — Ranked: how to make it REALLY look like geometric origami in v2

1. **Render flat-shaded facets from a real crease pattern, hard creases visible.** The single
   biggest fidelity lever is **per-facet flat shading** (constant lightness per triangle from
   its normal) + **stroked crease edges** + **painter's depth sort**. This is what separates
   "folded paper" from "a morphing blob." Do this in **Canvas2D** (no WebGL context in the
   panel). *(§v2.2 render step.)*
2. **Drive the fold with real hinge kinematics, one scalar `t`.** Facets rotate about their
   actual shared creases (`M_f = M_p ∘ Rodrigues(edge, t·φ_flat)`), collapsing the flat CP
   into the creature. `t:0→1` fold, `t:1→0` unfold = exact reverse. *(§v2.2.)*
3. **Author the creature as a hinge tree (wings/neck/head/jaws off a body).** Tree structure
   = no loop-coupling, genuinely rigid by construction, and covers the bird/crane silhouette
   plus the mouth. Add the **cootie-catcher jaw** as an independent 1-DOF for the audio mouth.
   *(§v2.2 recipes + code sketch.)*
4. **Lock the flat-pose layer order once with flat-folder (MIT, offline).** For any pose where
   flaps interleave and centroid-z sort could flicker, precompute the true stacking order and
   store it as a per-face draw priority. *(§v2.1.)*
5. **For a complex creature that a hand tree can't reach, BAKE from OrigamiSimulator (MIT).**
   Sweep `creasePercent` 0→1 on a *traditional* CP, dump per-step vertex positions, lerp at
   runtime — same Canvas2D renderer, just keyframes instead of live kinematics. *(§v1.)*
6. **Audio mouth = RMS → jaw dihedral with attack/release EMA.** *(§v2.5.)*
7. **CSS `preserve-3d` only for a 2–4-panel DOM accent** (e.g. an unfolding transcript
   receipt), never for the creature fold. *(§v2.4.)*

**Explicitly still-rejected:** silhouette morphs (flubber/polymorph/Lottie) *for the fold*;
GSAP/MorphSVG (non-OSI license); Rabbit Ear (GPL) shipped at runtime; a live GPU solver
kept alive in the always-on panel.

### v2.7 — Minimal hinge-tree fold sketch (Canvas2D, adapt freely)

Real geometric folding: facets rotate about their shared creases via Rodrigues, one scalar
`t` collapses the flat crease pattern into a bird, `mouth` opens the beak from mic RMS.
Zero dependencies; drop the `flat`/`faces` arrays to swap creatures (or paste baked frames).

```js
// ---- tiny vec3 + affine {R:[9 row-major], t:[3]} ----
const sub=(a,b)=>[a[0]-b[0],a[1]-b[1],a[2]-b[2]];
const cross=(a,b)=>[a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]];
const dot=(a,b)=>a[0]*b[0]+a[1]*b[1]+a[2]*b[2];
const norm=a=>{const l=Math.hypot(a[0],a[1],a[2])||1;return [a[0]/l,a[1]/l,a[2]/l];};
const apply=(M,p)=>[ M.R[0]*p[0]+M.R[1]*p[1]+M.R[2]*p[2]+M.t[0],
                     M.R[3]*p[0]+M.R[4]*p[1]+M.R[5]*p[2]+M.t[1],
                     M.R[6]*p[0]+M.R[7]*p[1]+M.R[8]*p[2]+M.t[2] ];
function compose(M,N){ // (M∘N)(p) = M(N(p))
  const R=new Array(9);
  for(let r=0;r<3;r++)for(let c=0;c<3;c++)
    R[r*3+c]=M.R[r*3]*N.R[c]+M.R[r*3+1]*N.R[3+c]+M.R[r*3+2]*N.R[6+c];
  const t=apply(M, N.t);
  return {R,t};
}
const I={R:[1,0,0,0,1,0,0,0,1],t:[0,0,0]};
function rotAboutLine(A,k,ang){            // rotate space about line (point A, unit axis k)
  const c=Math.cos(ang), s=Math.sin(ang), C=1-c, [x,y,z]=k;
  const R=[ c+x*x*C,   x*y*C-z*s, x*z*C+y*s,
            y*x*C+z*s, c+y*y*C,   y*z*C-x*s,
            z*x*C-y*s, z*y*C+x*s, c+z*z*C ];
  const RA=[R[0]*A[0]+R[1]*A[1]+R[2]*A[2],
            R[3]*A[0]+R[4]*A[1]+R[5]*A[2],
            R[6]*A[0]+R[7]*A[1]+R[8]*A[2]];
  return {R, t:[A[0]-RA[0], A[1]-RA[1], A[2]-RA[2]]};   // fixes the whole line
}

// ---- the crease pattern (flat material coords) — replace with your CP ----
const flat=[
  [-0.35,-1.0],[0.35,-1.0],[0.35,1.0],[-0.35,1.0], // 0-3 body quad
  [-1.3,0.0],[1.3,0.0],                             // 4,5 wing tips
  [-0.35,1.8],[0.35,1.8],                           // 6,7 head top edge (mouth spine)
  [0.0,2.5],[0.0,2.5]                               // 8 upper-beak tip, 9 lower-beak tip
];
// faces: parents MUST precede children. hinge=[a,b] shared crease edge (indices into flat).
const faces=[
  {v:[0,1,2,3], parent:-1, hinge:null,  ang:()=>0},              // 0 body (root, in-plane)
  {v:[3,0,4],   parent:0,  hinge:[3,0], ang:t=>-t*1.15},         // 1 left wing (about left edge)
  {v:[2,5,1],   parent:0,  hinge:[2,1], ang:t=> t*1.15},         // 2 right wing (sign flipped: axis reversed)
  {v:[3,2,7,6], parent:0,  hinge:[3,2], ang:t=> t*0.8},          // 3 head (tilt forward off top edge)
  {v:[6,7,8],   parent:3,  hinge:[6,7], ang:(t,m)=> t*0.25+m},   // 4 upper beak
  {v:[6,7,9],   parent:3,  hinge:[6,7], ang:(t,m)=> t*0.25-m},   // 5 lower beak (opens with mouth)
];

function computeWorld(t, mouth){
  const M=new Array(faces.length), W=new Array(faces.length);
  for(let i=0;i<faces.length;i++){
    const f=faces[i];
    if(f.parent<0){ M[i]=I; }
    else{
      const A=[flat[f.hinge[0]][0],flat[f.hinge[0]][1],0];
      const B=[flat[f.hinge[1]][0],flat[f.hinge[1]][1],0];
      M[i]=compose(M[f.parent], rotAboutLine(A, norm(sub(B,A)), f.ang(t,mouth)));
    }
    W[i]=f.v.map(vi=>apply(M[i],[flat[vi][0],flat[vi][1],0]));
  }
  return W;
}

// ---- view + render ----
const VIEW=compose(rotAboutLine([0,0,0],[1,0,0],-0.55),  // pitch
                   rotAboutLine([0,0,0],[0,1,0], 0.35)); // yaw
const L=norm([0.3,0.6,1.0]);                              // light dir
function render(ctx, W, w, h){
  const S=Math.min(w,h)*0.22, CX=w/2, CY=h*0.62;
  const items=W.map(poly=>{
    const view=poly.map(p=>apply(VIEW,p));
    const scr=view.map(p=>[CX+S*p[0], CY-S*p[1]]);
    const z=view.reduce((s,p)=>s+p[2],0)/view.length;
    const n=norm(cross(sub(view[1],view[0]), sub(view[2],view[0])));
    return {scr, z, n};
  });
  items.sort((a,b)=>a.z-b.z);                             // far first (painter)
  ctx.clearRect(0,0,w,h);
  for(const it of items){
    const sh=Math.round((0.55+0.45*Math.abs(dot(it.n,L)))*100);
    ctx.beginPath();
    it.scr.forEach((p,j)=> j?ctx.lineTo(p[0],p[1]):ctx.moveTo(p[0],p[1]));
    ctx.closePath();
    ctx.fillStyle=`hsl(45 42% ${sh}%)`;                   // warm paper tone
    ctx.fill();
    ctx.strokeStyle='rgba(120,90,55,0.45)'; ctx.lineWidth=1; ctx.stroke(); // creases
  }
}

// ---- drive loop: t eases 0→1 on activate; mouth from mic RMS (see §v2.5) ----
let t=0, target=0, env=0, raf=0;
function frame(ctx, canvas, analyser, buf){
  // mouth envelope
  analyser.getFloatTimeDomainData(buf);
  let s=0; for(const x of buf) s+=x*x;
  let lv=Math.min(1,Math.max(0,(Math.sqrt(s/buf.length)-0.02)/0.18)); lv=Math.pow(lv,0.7);
  env += (lv-env)*(lv>env?0.6:0.15);
  const mouth=0.05 + env*0.55 + 0.02*Math.sin(performance.now()/650);
  // fold ease
  t += (target-t)*0.18;
  render(ctx, computeWorld(t, mouth), canvas.width, canvas.height);
  // keep animating while folding OR while listening (mouth moving); else stop (0 idle CPU)
  if(Math.abs(target-t)>0.001 || env>0.01) raf=requestAnimationFrame(()=>frame(ctx,canvas,analyser,buf));
  else raf=0;
}
// unfoldToCreature(): target=1;  foldFlat(): target=0;  (kick raf if !raf)
```

Notes for adaptation: (1) the wings show the **axis-direction sign gotcha** — face 2's hinge
is written `[2,1]` (not `[1,2]`) so its axis points opposite face 1's, letting the *same*
`±t` lift both wings symmetrically; flip the hinge order or the angle sign per side. (2)
`ang` returns the **dihedral fold angle** — `±π` targets give a full flat-fold; the smaller
constants here give a stylized rest pose. (3) To play **baked keyframes** instead of live
kinematics, skip `computeWorld` and feed `render` the lerped frame directly — same renderer.
(4) For a **single-vertex base** (preliminary/waterbomb/cootie) give every crease the *same*
`|t·π|` with alternating signs (symmetry ⇒ loops close); for a general degree-4 vertex use
the `tan(ρ/2)` ratios from §v2.2 to set each `ang`.

---
*v2 research verified 2026-07-22: flat-folder MIT + flat-state/layer-order-only + FOLD/SVG/OPX/CP
I/O (repo read); OrigamiSimulator `creasePercent → creasePercent·targetTheta` crease-force model
(`js/dynamic/dynamicSolver.js` + Ghassaei GPU-computation paper); georgiee/origami MIT reflect-op
runtime folding with step slider; OriDomi MIT; Codrops preserve-3d/transform-origin/overflow-hidden
technique + sub-pixel-gap caveat; degree-4 = spherical 4R linkage, 1-DOF, `tᵢ=tan(ρᵢ/2)` linear
relations (royalsociety RSPA 2023, arXiv 2408.08816); Kawasaki/Maekawa flat-foldability;
preliminary ⇄ waterbomb = same CP, inverted M/V.*
