# Motion, 3D & Component Library Scout — the "Yap" origami companion + HUD rebuild

> **Target app:** `Yap` (formerly Wilson Voice). Stack: **Tauri 2** (Rust core + macOS
> system **WKWebView**) + **React 19 / TypeScript / Vite 7**. The always-on pill is a
> separate MPA entry rendered in a transparent, always-on-top **NSPanel**.
>
> **Scope of this file:** the broad *library scout* across animation runtimes, 3D/WebGL,
> tweening, generative/vector, origami tooling (recap), component stores, and charts —
> with a per-item verdict and two ranked stack recommendations at the end. The deep
> origami/fold **build plan** lives in `docs/research/origami-yapping-pill.md`; this file
> cross-references it rather than duplicating it.
>
> Every license + last-release fact below was verified this session against the GitHub
> license API, `gh` release/tag data, and the npm registry (`npm view`). Dates are
> `YYYY-MM-DD`. "Recency" = latest tagged release or last repo push as of **2026-07-22**.

---

## 0. Hard constraints these were judged against

| Constraint | What it means for a pick |
|---|---|
| **No runtime CDN (strict CSP)** | Everything must `npm install` + Vite-bundle, or ship as a static asset. Anything that fetches a scene/asset from a vendor host at runtime (Spline default, LottieFiles hosted `.lottie` URLs) is disqualified until proven fully local. |
| **Permissive license only** | ✅ MIT / BSD / Apache-2.0 / MPL-2.0 / ISC / Unlicense. ❌ **GPL/AGPL/LGPL** (copyleft — LGPL's relink obligation is unsatisfiable in a bundled/minified Tauri app). ⚠️ **Non-OSI "free" licenses** (GSAP "Standard", Commons Clause, Hover.dev EULA) — usable *inside* the product but **not freely redistributable as OSS source**; flagged individually. The product itself ships **source-available non-commercial (BSL 1.1 / PolyForm)**, so a copyleft *runtime* dep is fatal, but a GPL tool used **offline** whose numeric output we bake is fine (output data ≠ derivative of the tool). |
| **60 fps at low CPU, always-on** | The pill repaints on a transparent NSPanel that floats over everything. **A live WebGL context that stays alive at idle is the #1 battery/thermal risk.** Prefer: static frame at rest + rAF only while animating; Canvas2D / SVG / CSS / WAAPI over persistent WebGL; WASM only if the CSP cost is paid deliberately. |
| **Tauri/WKWebView WASM gotcha** | Instantiating WebAssembly (Rive, dotLottie) requires the Tauri CSP `script-src` to include **`'wasm-unsafe-eval'`**. Without it, the WASM runtime throws at load. Budget this before adopting any WASM lib. |

---

## 1. Declarative / vector animation runtimes

### Rive — `rive-app/rive-wasm` (npm `@rive-app/canvas`, `@rive-app/webgl2`, `@rive-app/react-canvas`)
- **Repo:** <https://github.com/rive-app/rive-wasm> · **Runtime license: MIT** (repo + npm `@rive-app/canvas` v**2.39.0**, `@rive-app/webgl2` v2.39.0 both MIT). Repo pushed **2026-07-23** — actively maintained.
- **What it gives us:** a `.riv` file authored in Rive's editor with **state machines** — you feed runtime *inputs* (`amplitude`, `wordCount`, a `state` enum) and the file drives a hand-designed mascot with **mesh deform, bones, blend states**. This is the strongest option for the **reactive personality / mouth / listening→thinking→done** choreography (it is *authored* animation, not a physics-exact crease fold).
- **Offline in Tauri:** ✅ Yes. The **editor is a proprietary SaaS** (free tier) but that only produces the file; the shipped `.riv` is a local static asset and the **runtime is MIT**. No CDN. **Caveat:** the canvas runtime is **WASM** → add `'wasm-unsafe-eval'` to the CSP (see §0). `.riv` files are tiny (KBs).
- **Perf on HUD:** Good. `@rive-app/canvas` renders to Canvas2D-ish via its own renderer (no persistent WebGL context needed — pick the **canvas** build, not webgl2, to avoid a live GL context on the panel). Runtime JS ~15 KB + WASM ~150–200 KB gzip (one-time load, then cheap frames). Pause the state machine at idle to zero out CPU.
- **Verdict:** **Adopt for the expressive/reactive mascot layer (v2 of the pill personality)** — the best fit for state-driven "yapping" mouth; not the crease-exact fold.

### Lottie — `lottie-web` (Airbnb) + dotLottie (`@lottiefiles/dotlottie-web`)
- **Repos:** <https://github.com/airbnb/lottie-web> (**MIT**, npm `lottie-web` v**5.13.0**, last push 2025-09-01) · <https://github.com/LottieFiles/dotlottie-web> (**MIT**, npm `@lottiefiles/dotlottie-web` v**0.78.2**, published 2026-07-22).
- **What it gives us:** plays a designer-made After-Effects/Bodymovin JSON (or compressed `.lottie`). Ideal for **eye-blink / idle-breathe / mouth-loop accents** and even a *fake* fold (AE animation, `direction: -1` to reverse) — but a Lottie "fold" is a **drawn** animation, **not** a real crease articulation, so it fails the "folds back exactly like real origami" test for the fold itself.
- **Offline in Tauri:** ✅ Bundle the `.json`/`.lottie` locally. `lottie-web` is pure JS (SVG/Canvas renderer), **no WASM, no CDN** — the simplest offline path. `@lottiefiles/dotlottie-web` uses a **WASM (rlottie) core** (needs `'wasm-unsafe-eval'`) — heavier; only worth it for `.lottie` compression, otherwise use plain `lottie-web`. **Do not** use LottieFiles' hosted-URL loading mode (that's a CDN fetch).
- **Perf on HUD:** `lottie-web` SVG renderer is fine for small loops; use the **canvas** renderer or `lottie_light` build to cut CPU. Full lib ~60 KB gzip. Stop the animation at idle.
- **Verdict:** **Use plain `lottie-web` for cheap looping accents (blink/breathe/mouth)**; skip dotLottie's WASM unless you need `.lottie` compression; never for the crease-exact fold.

### Spline — `@splinetool/runtime`
- **npm:** `@splinetool/runtime` v1.12.98 · repo <https://github.com/splinetool/spline>. **License: none declared** — the npm package has **no `license` field** and the repo returns no license (effectively **proprietary / all-rights-reserved**).
- **What it gives us:** in-browser 3D scenes authored in Spline's editor. Visually rich, but it is a **full three.js + physics runtime** and by default **loads the scene from `prod.spline.design` (a CDN)** unless you export `.splinecode` locally.
- **Offline in Tauri:** ❌ **Two disqualifiers:** (1) no permissive (or any) OSS license grant on the runtime; (2) default CDN scene loading. Even with a local `.splinecode` export, the missing license is fatal for a source-available repo.
- **Perf on HUD:** ❌ Heaviest of the three (persistent WebGL + physics), the exact always-on battery risk.
- **Verdict:** **Reject.** Unlicensed runtime + CDN-by-default + heavy. Rive covers the "designed reactive character" need with an MIT runtime.

---

## 2. 3D / WebGL — which is lightest for a tiny HUD?

> **Framing:** for an always-on transparent panel, the cheapest "3D-looking" effects
> (metaballs, liquid glass, paper shading) are best done **without a persistent WebGL
> context** — via an SVG gooey filter (§4) or Canvas2D baked frames (§5, origami file).
> Reach for WebGL only when you truly want a live raymarched/lit surface, and then pick
> the **smallest** wrapper and **pause `rAF` at idle**.

| Lib | Repo / npm | License | Approx gzip | Live GL context? | Fit for a tiny HUD |
|---|---|---|---|---|---|
| **three.js** | <https://github.com/mrdoob/three.js> · `three` v**0.185.1** (r185, 2026-07-01) | **MIT** | ~150–170 KB | Yes | Powerful but **heavy** for a 44px pill; only if you need real lighting/shadows on the mascot. |
| **react-three-fiber** | <https://github.com/pmndrs/react-three-fiber> · `@react-three/fiber` v**9.6.1** | **MIT** | ~18 KB + three | Yes (via three) | Ergonomic React bindings *on top of* three — inherits three's weight. |
| **drei** | <https://github.com/pmndrs/drei> · `@react-three/drei` v**10.7.7** | **MIT** | modular | Yes | Helpers (shaders, `MeshTransmissionMaterial` for glass) — import per-helper, it's large if you pull it all. |
| **ogl** | <https://github.com/oframe/ogl> · `ogl` v**1.0.11** | **Unlicense** (public-domain-equivalent, permissive ✅) | **~10 KB** | Yes | **Lightest real WebGL option.** Minimal, tree-shakeable; perfect for a single full-quad fragment shader (metaball/liquid). |
| **regl** | <https://github.com/regl-project/regl> · `regl` | **MIT** | ~28 KB | Yes | Functional WebGL wrapper; more than we need for one shader quad — ogl is smaller. |
| **raw GLSL shader** (hand-written WebGL, no lib) | — | your code (permissive) | ~1–3 KB | Yes | **Lightest possible** for a metaball/liquid-glass/paper fragment shader on one full-screen quad. Most control, most boilerplate. |

- **Lightest for a tiny HUD:** **raw GLSL on a single quad** (≈0 dep) or **ogl** (~10 KB) if you want a tidy wrapper. Both still open a live GL context — so **pause the render loop when the pill is at rest** and consider tearing down the context on hide.
- **Verdict:** **ogl (Unlicense) or raw GLSL** for any true shader effect (liquid glass / metaball / paper light). **three.js/r3f only if v-next wants lit 3D on the mascot** — overkill and battery-hostile for an always-on pill otherwise.

---

## 3. Motion / tween libraries

### motion / Framer Motion — `motion` (motion.dev)
- **Repo:** <https://github.com/motiondivision/motion> · npm `motion` v**12.42.2** · **MIT**. (The former `framer-motion` is now unified under `motion`; `motion.dev` is the site.)
- **What it gives us:** React-first declarative springs, layout animations, gesture + `AnimatePresence` — the natural tool for the **capsule expand/contract (Dynamic-Island feel)** and state transitions in a React MPA.
- **Offline/Tauri:** ✅ pure JS, bundles cleanly, no CDN, no WASM. **Perf:** use `LazyMotion` + `domAnimation` (~18 KB gzip) or `motion/mini` (~2.6 KB) instead of the full `motion` (~34 KB) to keep the pill lean; it can drive the compositor (transform/opacity) so 60 fps is easy.
- **Verdict:** **Primary choice for React state/spring micro-interactions on the pill.**

### Motion One — `@motionone/dom`
- **npm:** `@motionone/dom` v**10.18.0** · **MIT** (now folded into the Motion project).
- **What it gives us:** a tiny (~5 KB) vanilla animation lib built on the **native Web Animations API** — hands most work to WKWebView's compositor.
- **Offline/Tauri:** ✅ trivial. **Perf:** excellent (WAAPI-backed, off-main-thread where possible). **Verdict:** **Best featherweight option** if you don't want React's `motion` weight — or just use **native `element.animate()` (WAAPI, zero deps, `playbackRate = -1` to reverse exactly)**, which is already the recommendation in the origami file for reversible hand-rigged accents.

### anime.js v4 — `animejs`
- **Repo:** <https://github.com/juliangarnier/anime> · v**4.5.0** (2026-06-22) · **MIT** (v4 relicensed to MIT and rewritten as ESM/tree-shakeable).
- **What it gives us:** timeline-based tweening of DOM/SVG/JS values, staggering, spring/easing. Great for **choreographed multi-element sequences** (filling the "state gap" beats) without React.
- **Offline/Tauri:** ✅ ESM bundle, no CDN/WASM. **Perf:** good; drives transforms/opacity for compositor-friendly motion. ~9 KB gzip. **Verdict:** **Solid permissive timeline engine** — a good MIT alternative to GSAP's timeline for the thinking/pasting/done choreography.

### Theatre.js — `@theatre/core` / `@theatre/studio`
- **Repo:** <https://github.com/theatre-js/theatre> · **Apache-2.0** ✅ (last push 2024-08-14 — **maintenance has slowed**).
- **What it gives us:** a visual **keyframe editor** (`@theatre/studio`) + a small runtime (`@theatre/core`) — design complex sequences in a GUI, ship the JSON + core. Nice for authoring the exact fold/state timeline visually.
- **Offline/Tauri:** ✅ core bundles locally; `studio` is a dev-only tool. **Perf:** core is reasonable (~40–60 KB) but it's another runtime to carry. **Verdict:** **Optional authoring convenience** (Apache-2.0, safe) — worth it only if you want a GUI timeline; otherwise anime.js/WAAPI cover it lighter. Watch the slowing maintenance.

### GSAP — `gsap` ⚠️ **LICENSE CAVEAT**
- **Repo:** <https://github.com/greensock/GSAP> · npm `gsap` v**3.15.0** · **License: "GreenSock Standard 'no charge' license"** (`https://gsap.com/standard-license`) — **NOT an OSI/permissive license.**
- **Status (verified this session):** After Webflow's acquisition of GreenSock (Oct 2024), **as of April 2025 GSAP is 100% free — including commercial use and all formerly-paid Club plugins** (SplitText, MorphSVG, DrawSVG, ScrollTrigger, ScrollSmoother, Inertia). So it is **free to use, even in a commercial product**.
- **The caveat that still bites us:** it is **still a custom, non-OSI license**, not MIT/BSD/Apache. Its terms **prohibit using GSAP to build a competing animation tool/service**, and it is not the kind of license you can cleanly re-license/redistribute inside a **source-available OSS repo** the way you can MIT code. For a repo other people fork, shipping a non-standard-licensed dependency is friction, and there's an MIT-equivalent for everything we need (motion + anime.js + WAAPI).
- **Verdict:** **Avoid in the shipped OSS bundle on principle** (non-OSI license), even though it's now free. Use `motion` / `anime.js` / WAAPI instead. MorphSVG would only give a silhouette *morph*, not a real crease fold, anyway.

### popmotion — `popmotion`
- **Repo:** <https://github.com/popmotion/popmotion> · npm `popmotion` v**11.0.5** · **MIT** (last push 2024-03-12 — **effectively deprecated**, its authors folded it into Motion).
- **What it gives us:** low-level `animate`, `interpolate`, spring, path interpolation primitives (~5 KB).
- **Verdict:** **Skip as a dependency** — it's legacy; the same primitives live in maintained `motion`. Fine as a reference for `interpolate`.

---

## 4. Vector / paper / generative + SVG-filter tricks

### Paper.js — `paper`
- **Repo:** <https://github.com/paperjs/paper.js> · npm `paper` v**0.12.18** · **MIT** ✅ (GitHub reports NOASSERTION because of the LICENSE header format, but the package is MIT; last release v0.12.15 in 2021 — **quiet**).
- **Gives:** a full **vector scene graph** on Canvas (boolean ops, path math, hit-testing). Powerful for generative paper/geometry, but **~100 KB+ gzip and heavy** for a pill. **Verdict:** overkill for the HUD; use only if you need real vector boolean geometry. Two.js is lighter for simple generative shapes.

### Two.js — `two.js`
- **Repo:** <https://github.com/jonobr1/two.js> · npm `two.js` v**0.8.23** (v0.8.21 release 2025-10-02) · **MIT** ✅, actively maintained.
- **Gives:** a lightweight 2D drawing API with **SVG / Canvas / WebGL** renderers — good for stylized generative "paper" shapes and morph-y vector motion at ~30–40 KB. **Verdict:** **Good MIT pick** if you want a small vector-scene abstraction over Canvas for the mascot's flat art (lighter than Paper.js/p5).

### Rough.js — `roughjs`
- **Repo:** <https://github.com/rough-stuff/rough> · npm `roughjs` v**4.6.6** · **MIT** ✅ (last tag 2019 but stable/complete, ~9 KB).
- **Gives:** hand-drawn / sketchy rendering of shapes and paths. **Verdict:** a fun **stylistic accent** (sketchy paper look) if the brand wants it; tiny and permissive. Not core.

### p5.js — `p5` ⚠️ **COPYLEFT**
- **Repo:** <https://github.com/processing/p5.js> · npm `p5` v**2.3.1** (2026-07-21, very active) · **License: LGPL-2.1** ⚠️.
- **Gives:** the friendly creative-coding API (great for prototyping generative art). **But** LGPL is **copyleft**: bundling+minifying p5 into a Tauri app is effectively static linking, and LGPL's requirement to let users relink/replace the library is **impractical to satisfy in a bundled app** — a real risk for a source-available product.
- **Verdict:** **Do not bundle** (copyleft). Use **Two.js (MIT)** or **ogl/raw Canvas** for the same generative needs. p5 is fine only as a **local sketchpad for prototyping ideas**, never shipped.

### Shape morph — flubber / polymorph-js
- **flubber:** <https://github.com/veltman/flubber> · npm `flubber` v**0.4.2** · **MIT** ✅ (~20 KB, last push 2022). Best-guess interpolation between two arbitrary SVG shapes.
- **polymorph-js:** npm `polymorph-js` v**1.0.2** · **MIT** ✅ (~5 KB, lighter path morph).
- **Verdict:** **Great for the reactive-mouth / expand-contract silhouette accents**, but a morph **tweens outlines** and has no concept of creases — **it cannot express a real origami unfold** (that's the baked-keyframe path in the origami file). Use for mouth/blob accents only.

### SVG filter "gooey / metaball" trick (native, zero-dep) ⭐
- **Technique:** stack blobs, apply `feGaussianBlur` then a high-contrast `feColorMatrix` alpha ramp — nearby shapes **merge like liquid metaballs**. Pure SVG, **no library, no WebGL, no WASM**, supported in WKWebView.
- **Perf on HUD:** cheap relative to a live GL context; the blur is GPU-composited. Keep the filter region small (the pill bounds) to bound cost.
- **Verdict:** ⭐ **The cheapest, most Tauri-friendly way to get a liquid/metaball pill** — first choice for the "liquid" HUD look before reaching for WebGL. Pair with CSS `backdrop-filter` or native vibrancy for the "glass."

---

## 5. Origami-specific (recap — full plan in `origami-yapping-pill.md`)

| Tool | Repo / npm | License | Role |
|---|---|---|---|
| **Origami Simulator** (Amanda Ghassaei) | <https://github.com/amandaghassaei/OrigamiSimulator> | **MIT** ✅ (verified via license API; last push 2025-11-20) | **Offline bake engine.** Sweep `globals.creasePercent` 0→1 over a traditional crane CP, dump per-frame vertex positions → `crane.json`. Real rigid-fold physics computed **once at build time**. Do **not** run it live in the pill (three.js + GPU solver + jQuery UI = always-on battery risk). |
| **FOLD** (edemaine/fold) | <https://github.com/edemaine/fold> · npm `fold` | **MIT** ✅ (v0.12.0, 2023) | Canonical `.fold` JSON storage for crease patterns + baked frames. Loader = `JSON.parse` (no lib to ship). |
| **Rabbit Ear** (robbykraft/Origami) | <https://github.com/robbykraft/Origami> · npm `rabbit-ear` | **GPL-3.0** ⛔ | **Study/offline-tool ONLY — never bundled.** GPL would force Yap to GPL (fatal vs BSL/PolyForm). Even offline, prefer the MIT Origami Simulator so provenance stays clean. |

**Confirmed runtime path (from the origami file):** bake with Origami Simulator (MIT) →
store as FOLD (MIT) → play back baked keyframes with a **zero-dep Canvas2D `foldPlayer`**
that lerps one eased scalar `t`. Unfold `t:0→1`, fold `t:1→0` = **bit-exact reverse by
construction**, `rAF` only while animating, **no WebGL context** on the panel. This is the
only path that is *both* faithful to a real crease pattern *and* cheap enough for an
always-on HUD. (Named designer CPs like Lang's carry author copyright even inside an MIT
repo — ship only **traditional/public-domain** patterns or self-authored ones.)

---

## 6. Component stores / UI kits (copy-in motion components)

> These are mostly **copy-paste registries** (shadcn-style) — you copy a component's
> source into your repo rather than `npm install` a black box. That makes bundle cost
> a function of *what you copy*, and it makes **license per-snippet** matter.

| Kit | Repo | License | Glass / blob / aurora / liquid / audio-viz? | Verdict |
|---|---|---|---|---|
| **shadcn/ui** | <https://github.com/shadcn-ui/ui> (`shadcn` CLI v4.14.0, 2026-07-22) | **MIT** ✅ | Foundational primitives (Radix + Tailwind); no flashy motion by itself | **Adopt as the base layer** for Settings/Insights/Dictionary/Scratchpad. The design system foundation. |
| **Aceternity UI** | <https://ui.aceternity.com/> · org <https://github.com/aceternity> | **MIT** (53+ free components; **Pro** blocks are a separate paid license) | ✅✅ **Aurora Background, Background Beams, Wavy Background, Spotlight, Glowing Effect, Gooey/Meteors, Background Gradient** | **Best source of "aurora/liquid/glass" motion components** for the HUD look. Built on `motion` + Tailwind (both MIT). Copy the free MIT ones. |
| **Magic UI** | <https://github.com/magicuidesign/magicui> | **MIT** ✅ (active 2026-07-21) | ✅ Shimmer/border-beam, particles, animated gradient/aurora, marquee | **Adopt** — big library of MIT motion components; strong glass/beam/gradient set. |
| **ReactBits** | <https://github.com/DavidHDev/react-bits> | ⚠️ **MIT + Commons Clause** (the npm `react-bits` "MIT" field is **misleading** — the repo `LICENSE.md` adds Commons Clause) | ✅✅ Aurora, Liquid/Metaballs, "Silk", "Iridescence", "Threads", "Dot Grid", **audio/waveform-ish backgrounds** | **Great components, license caveat.** Commons Clause = **source-available, not OSI**: fine to copy a component **into your app** (even commercial), but you **may not redistribute the components themselves**. In a *published source-available repo* that line is fuzzy — treat copied ReactBits code as **use-in-app-only**, don't present it as MIT, and prefer an MIT equivalent (Aceternity/Magic UI) where one exists. |
| **Cult UI** | <https://github.com/nolly-studio/cult-ui> | **MIT** ✅ (active 2026-07-22) | ✅ Glass/neumorphic cards, texture/shader backgrounds, animated popovers | **Adopt** — MIT, nice "premium" glass/texture components. |
| **Kokonut UI** | <https://github.com/kokonut-labs/kokonutui> | **MIT** ✅ (active 2026-07-22) | ✅ Gradient/aurora buttons & cards, particle/beam effects | **Adopt** — MIT, good flashy accents. |
| **Park UI** | <https://github.com/cschroeter/park-ui> | **MIT** ✅ | Design-system primitives (Ark UI + Panda/Tailwind); not effect-heavy | Foundation alternative to shadcn; no special blob/aurora. Optional. |
| **tremor** | <https://github.com/tremorlabs/tremor> | **Apache-2.0** ✅ | Dashboard/**charts** + KPI blocks (not glass/aurora) | **Consider for the Insights page** KPI tiles + simple charts (React + Recharts under the hood). Apache-2.0 safe. |
| **motion-primitives** | <https://github.com/ibelick/motion-primitives> | **MIT** ✅ | ✅ Morphing dialog, text effects, **glow/spotlight**, animated backgrounds | **Adopt** — MIT, built on `motion`; clean primitives for the state-gap micro-motions. |
| **Hover.dev** | <https://www.hover.dev/> | ⚠️ **Proprietary EULA** (free tier + paid "Hover Pro"; **not** MIT/OSS) | ✅ Animated glass/gradient UI + templates | **Reference / not-for-OSS-redistribution.** License grants use in projects (incl. OSS *projects*) but **no repackaging/redistribution of the components** — same fuzziness as Commons Clause for a source-available repo. Prefer the MIT kits above. |

**Where the glass / blob / aurora / liquid / audio-viz components live:** **Aceternity UI**
and **ReactBits** are richest (aurora, liquid, metaballs, iridescence, waveform-y
backgrounds); **Magic UI**, **Cult UI**, **Kokonut UI**, **motion-primitives** add more
glass/beam/gradient accents. For **clean licensing**, lean on **Aceternity (free/MIT) +
Magic UI + Cult UI + Kokonut UI + motion-primitives (all MIT)**; use ReactBits/Hover.dev
**only as copy-in-for-your-app or visual reference**, never as redistributable OSS.

---

## 7. Charts (Insights page rebuild) — lightest + nicest

> The roadmap wants **15+ user-switchable chart types** (bar/line/donut/treemap/triangle,
> a **GitHub-style commit heatmap**, radial, area, sparkline…) fed from `daily_stats`.

| Lib | Repo / npm | License | Approx weight | Character | Verdict |
|---|---|---|---|---|---|
| **uPlot** | <https://github.com/leeoniya/uPlot> · `uplot` v**1.6.32** | **MIT** ✅ | **~20 KB gzip** (tiny, canvas) | Extremely fast time-series line/area/bar; minimal API, you style it | **Lightest + fastest** for the dense time-series (WPM-over-time, words-per-day). **Best "performance" pick**, esp. for long history + heatmap-adjacent series. Less "batteries-included" for exotic chart types. |
| **visx** | <https://github.com/airbnb/visx> · `@visx/*` v**4.0.0** (2026-06-11) | **MIT** ✅ | modular (import only what you use) | D3 math + React primitives — you compose SVG charts (incl. **heatmap**, treemap, radial, custom shapes) | **Best for the "15+ custom chart types" + GitHub heatmap** — you get exactly the marks you draw at a controlled bundle cost. Most flexible for a *custom, switchable* Insights board. Requires more build effort. |
| **Recharts** | <https://github.com/recharts/recharts> · `recharts` v**3.10.0** (2026-07-20) | **MIT** ✅ | **~100 KB+ gzip** (bundles d3 modules) | Declarative React components, batteries-included | **Fastest to ship** common charts (line/bar/area/donut/radial). Heavier; **tremor** wraps it for dashboards. Good default if dev-speed > bundle size. |
| **Nivo** | <https://github.com/plouc/nivo> · `@nivo/*` v**0.99.0** | **MIT** ✅ | heavy, per-chart packages (d3 + react-spring) | Gorgeous defaults, includes a ready **calendar/commit heatmap** (`@nivo/calendar`) | **Nicest-looking out of the box** and ships the **GitHub-style heatmap for free** — but the **heaviest**; import only the chart packages you use. |
| **Observable Plot** | <https://github.com/observablehq/plot> · `@observablehq/plot` v**0.6.17** | **ISC** ✅ | ~90 KB gzip (bundles much of d3) | Grammar-of-graphics; concise specs, great for exploratory/varied marks | **Elegant + concise** for many chart types from one API; not React-native (renders SVG you mount), heavier than uPlot. |

- **Lightest:** **uPlot** (~20 KB, canvas, fast). **Nicest zero-effort + free heatmap:** **Nivo**. **Most flexible for a custom switchable board:** **visx**.
- **Recommended combo for Yap Insights:** **uPlot** for the high-frequency time-series (words/day, WPM trend, sparklines) + **visx** for the bespoke/switchable marks and the **GitHub-style commit heatmap** (or drop in **`@nivo/calendar`** if you want the heatmap for free and accept its weight). Skip Recharts/Nivo-everywhere if bundle size matters; use **tremor (Apache-2.0)** only if you want pre-built KPI/dashboard blocks fast.

---

## Master license & recency table (verified 2026-07-22)

| Library | npm / repo | License | Latest (date) | Ship in bundle? |
|---|---|---|---|---|
| Rive runtime | `@rive-app/canvas` <https://github.com/rive-app/rive-wasm> | **MIT** | 2.39.0 (repo push 2026-07-23) | ✅ (WASM → CSP `'wasm-unsafe-eval'`) |
| lottie-web | `lottie-web` <https://github.com/airbnb/lottie-web> | **MIT** | 5.13.0 (2025-09) | ✅ (pure JS) |
| dotLottie | `@lottiefiles/dotlottie-web` | **MIT** | 0.78.2 (2026-07-22) | ✅ (WASM → CSP) |
| Spline runtime | `@splinetool/runtime` | **none / proprietary** ⛔ | 1.12.98 | ❌ (no license + CDN) |
| three.js | `three` <https://github.com/mrdoob/three.js> | **MIT** | r185 / 0.185.1 (2026-07-01) | ✅ (heavy) |
| react-three-fiber | `@react-three/fiber` | **MIT** | 9.6.1 (2026-04) | ✅ |
| drei | `@react-three/drei` | **MIT** | 10.7.7 (2025-11) | ✅ (modular) |
| ogl | `ogl` <https://github.com/oframe/ogl> | **Unlicense** ✅ | 1.0.11 | ✅ (~10 KB) |
| regl | `regl` <https://github.com/regl-project/regl> | **MIT** | (push 2026-06) | ✅ |
| motion / Framer Motion | `motion` <https://github.com/motiondivision/motion> | **MIT** | 12.42.2 (2026-07) | ✅ |
| Motion One | `@motionone/dom` | **MIT** | 10.18.0 | ✅ (~5 KB, WAAPI) |
| anime.js v4 | `animejs` <https://github.com/juliangarnier/anime> | **MIT** | 4.5.0 (2026-06) | ✅ |
| Theatre.js | `@theatre/core` <https://github.com/theatre-js/theatre> | **Apache-2.0** | (push 2024-08) | ✅ (core) |
| **GSAP** | `gsap` <https://github.com/greensock/GSAP> | ⚠️ **Std "no-charge" (non-OSI)** | 3.15.0 | free but **avoid in OSS bundle** |
| popmotion | `popmotion` | **MIT** (deprecated) | 11.0.5 (2024) | skip (legacy) |
| Paper.js | `paper` <https://github.com/paperjs/paper.js> | **MIT** | 0.12.18 | ✅ (heavy) |
| Two.js | `two.js` <https://github.com/jonobr1/two.js> | **MIT** | 0.8.23 | ✅ |
| Rough.js | `roughjs` <https://github.com/rough-stuff/rough> | **MIT** | 4.6.6 | ✅ (~9 KB) |
| **p5.js** | `p5` <https://github.com/processing/p5.js> | ⛔ **LGPL-2.1 (copyleft)** | 2.3.1 (2026-07) | ❌ don't bundle |
| flubber | `flubber` <https://github.com/veltman/flubber> | **MIT** | 0.4.2 | ✅ (accents) |
| polymorph-js | `polymorph-js` | **MIT** | 1.0.2 | ✅ (accents) |
| Origami Simulator | <https://github.com/amandaghassaei/OrigamiSimulator> | **MIT** | (push 2025-11) | **offline bake only** |
| FOLD | `fold` <https://github.com/edemaine/fold> | **MIT** | 0.12.0 | ✅ (`JSON.parse`) |
| **Rabbit Ear** | `rabbit-ear` <https://github.com/robbykraft/Origami> | ⛔ **GPL-3.0** | 0.9.4-alpha | ❌ never (offline study only) |
| shadcn/ui | <https://github.com/shadcn-ui/ui> | **MIT** | shadcn 4.14.0 | ✅ (copy-in) |
| Aceternity UI (free) | <https://github.com/aceternity> | **MIT** (Pro = paid) | — | ✅ (copy-in) |
| Magic UI | <https://github.com/magicuidesign/magicui> | **MIT** | (push 2026-07) | ✅ (copy-in) |
| **ReactBits** | <https://github.com/DavidHDev/react-bits> | ⚠️ **MIT + Commons Clause** | 1.0.5 | copy-in-for-app only |
| Cult UI | <https://github.com/nolly-studio/cult-ui> | **MIT** | (push 2026-07) | ✅ (copy-in) |
| Kokonut UI | <https://github.com/kokonut-labs/kokonutui> | **MIT** | (push 2026-07) | ✅ (copy-in) |
| Park UI | <https://github.com/cschroeter/park-ui> | **MIT** | 0.43.1 | ✅ (copy-in) |
| tremor | <https://github.com/tremorlabs/tremor> | **Apache-2.0** | (push 2025-10) | ✅ |
| motion-primitives | <https://github.com/ibelick/motion-primitives> | **MIT** | (push 2026-03) | ✅ (copy-in) |
| **Hover.dev** | <https://www.hover.dev/> | ⚠️ **Proprietary EULA** | — | reference / not OSS-redist |
| uPlot | `uplot` <https://github.com/leeoniya/uPlot> | **MIT** | 1.6.32 | ✅ (~20 KB) |
| visx | `@visx/*` <https://github.com/airbnb/visx> | **MIT** | 4.0.0 (2026-06) | ✅ (modular) |
| Recharts | `recharts` <https://github.com/recharts/recharts> | **MIT** | 3.10.0 (2026-07) | ✅ (heavy) |
| Nivo | `@nivo/*` <https://github.com/plouc/nivo> | **MIT** | 0.99.0 | ✅ (heavy) |
| Observable Plot | `@observablehq/plot` <https://github.com/observablehq/plot> | **ISC** | 0.6.17 | ✅ |

---

## Tauri 2 / WKWebView gotchas (apply to every pick above)

1. **WASM needs a CSP grant.** Rive and dotLottie instantiate WebAssembly; the Tauri
   `app > security > csp` must include **`script-src ... 'wasm-unsafe-eval'`** or they
   throw at load. Plain `lottie-web`, motion, anime.js, ogl, Canvas2D need no such grant.
2. **No CDN = bundle everything.** All the ✅ libs `npm install` + Vite-bundle into
   `dist`, which Tauri packages into the app — no network at runtime. The only violators
   are **Spline** (CDN scene by default) and **LottieFiles hosted-URL mode** (use local
   assets instead). Keep the strict CSP; don't add `connect-src` exceptions for these.
3. **A live WebGL context on a transparent always-on NSPanel is the main battery/thermal
   cost.** three.js / r3f / regl / ogl / Rive-webgl2 all keep a GL context alive. Prefer
   **Canvas2D / SVG-filter / CSS / WAAPI** for the always-on pill; if you must use WebGL,
   **pause `rAF` and consider destroying the context when the pill is idle/hidden.**
4. **"Liquid glass" — prefer native vibrancy over CSS `backdrop-filter` for the panel
   background.** CSS `backdrop-filter: blur()` works in WKWebView and blurs the desktop
   behind the transparent window (real vibrancy), but it **repaints on the GPU every
   frame the region changes** — costly always-on. Tauri's `window-vibrancy`
   (`NSVisualEffectView`) gives the macOS glass **natively and more efficiently**; layer
   the SVG-gooey blob + component art on top of it. Use CSS `backdrop-filter` only for
   small, static glass accents.
5. **Retina over-draw.** Size any `<canvas>` to the actual pill pixels × `devicePixelRatio`,
   not the oversized shadow-room window, or you pay 4× fill cost per frame.
6. **Copy-in kits (shadcn/Aceternity/Magic UI/etc.) add only what you copy** — no runtime
   dep, bundle cost is per-component, and each carries its own license (watch ReactBits'
   Commons Clause and Hover.dev's EULA).

---

## Ranked recommendation A — render a beautiful FOLDING ORIGAMI companion at 60 fps

*(This aligns with and extends `origami-yapping-pill.md`; the fold itself must be a real
crease articulation, so baked keyframes win over any morph/drawn animation.)*

1. **Fold geometry (offline bake): Origami Simulator — MIT.** Sweep `globals.creasePercent`
   0→1 over a traditional crane / bird-base CP; dump per-frame vertex positions.
   *(build-time only; not in the bundle.)*
2. **Crease-pattern storage: FOLD (`edemaine/fold`) — MIT.** `.fold` JSON; loader is
   `JSON.parse`. *(~0 bundle.)*
3. **Runtime fold player: zero-dep Canvas2D `foldPlayer`.** Lerp one eased scalar `t`
   across baked frames; unfold `0→1`, fold `1→0` = **bit-exact reverse**; `rAF` only while
   animating; **no WebGL context.** *(~1–2 KB of your code + ~30 KB gzip JSON/mascot.)*
4. **Expressive layer / reactive mouth (v2): Rive `@rive-app/canvas` — MIT.** Feed
   `amplitude`/`wordCount`/`state` into a state machine for the "chewing/thinking/yapping"
   personality on top of the fold. *(~15 KB JS + ~150–200 KB WASM; needs CSP
   `'wasm-unsafe-eval'`; pause at idle.)*
5. **State springs / expand-contract: native WAAPI or `@motionone/dom` (MIT) / `motion`
   (MIT).** Dynamic-Island capsule feel; `playbackRate = -1` reverses exactly. *(0–18 KB.)*
6. **Optional accents: `lottie-web` (MIT)** for blink/breathe loops; **flubber/polymorph
   (MIT)** for mouth-silhouette morphs. *(Accents only — never the fold.)*
- **Total added bundle for v1 (baked fold + WAAPI):** **≈ 30–50 KB gzip, zero new WebGL,
  ~0 idle CPU.** Adding Rive for v2 personality: **+~200 KB WASM** (one-time) and a CSP
  tweak. **Gotcha:** keep the pill on Canvas2D/WAAPI; only pay the Rive WASM/CSP cost when
  you actually ship the expressive mascot.

## Ranked recommendation B — premium redesigned "pill / HUD" (liquid glass / metaball / paper)

1. **Panel glass: native vibrancy via Tauri `window-vibrancy` (`NSVisualEffectView`).**
   The efficient macOS glass base — cheaper than CSS `backdrop-filter` always-on. *(Rust
   side, ~0 JS.)*
2. **Liquid / metaball merge: native SVG gooey filter (`feGaussianBlur` + `feColorMatrix`)
   — zero dep.** ⭐ First choice; no WebGL context. *(~0 bundle.)* Reach for **ogl
   (Unlicense, ~10 KB) or a raw GLSL quad** only if you want a true raymarched liquid-glass
   surface — and pause its `rAF` at idle.
3. **Motion system: `motion` (MIT)** for React springs/layout (capsule expand/contract),
   or **`@motionone/dom` / WAAPI** for the featherweight path; **anime.js v4 (MIT)** for
   choreographed state-gap timelines (thinking→pasting→done). *(2.6–34 KB gzip depending
   on build.)*
4. **Premium components (copy-in, MIT): Aceternity UI (Aurora/Beams/Spotlight/Glow) +
   Magic UI + Cult UI + Kokonut UI + motion-primitives**, on a **shadcn/ui (MIT)**
   foundation. Copy the specific glass/aurora/liquid pieces you need. *(Bundle = only what
   you copy.)* **Avoid** shipping ReactBits (Commons Clause) and Hover.dev (proprietary)
   as redistributed OSS — copy-in-for-app or reference only.
5. **Audio-reactive "mouth": keep the existing CSS-var waveform bars; optionally `siriwave`
   (MIT)** for a Siri-style sine wave, or a small Canvas FFT. *(Tiny.)*
6. **Vector/paper stylization (optional): Two.js (MIT)** for generative paper shapes,
   **Rough.js (MIT)** for a sketchy accent. **Not p5.js (LGPL).**
- **Total added bundle for the HUD look:** **≈ 20–50 KB gzip** (mostly `motion` + copied
  components; the glass/metaball are native/zero-dep). **Gotchas:** do the glass with
  native vibrancy (not always-on CSS blur); do metaballs with the SVG filter (not a
  persistent WebGL context); size canvases to DPR; keep `rAF` paused at rest.

---

### Bottom line
- **Fold companion:** MIT bake (Origami Simulator) → MIT storage (FOLD) → **zero-dep
  Canvas2D playback** for a bit-exact reversible fold, **Rive (MIT)** for the reactive
  personality when ready. No GPL, no persistent WebGL, ~30–50 KB.
- **HUD look:** **native vibrancy + SVG gooey filter + `motion` + copy-in MIT components
  (Aceternity/Magic UI/Cult UI/Kokonut/motion-primitives on shadcn)**. Charts: **uPlot +
  visx** (add `@nivo/calendar` if you want a free GitHub heatmap).
- **Hard "no" list:** **Spline** (unlicensed + CDN), **Rabbit Ear** (GPL-3.0), **p5.js**
  (LGPL), and **GSAP** (free but non-OSI — use motion/anime.js instead). **Flag list:**
  **ReactBits** (Commons Clause) and **Hover.dev** (proprietary) — copy-in-for-app only,
  not redistributable OSS.

---
*Verified this session (2026-07-22) via GitHub license API, `gh` release/tag data, and
`npm view` license/version. GSAP's "100% free since April 2025 (post-Webflow)" status and
its still-non-OSI Standard license confirmed via Webflow/CSS-Tricks announcements.
ReactBits' `LICENSE.md` (MIT + Commons Clause) and Spline runtime's missing license field
read directly. Bundle sizes are approximate gzip and will vary with tree-shaking.*
