# Open-Source Roadmap â (rename from "Wilson Voice")

> Captured from Wilson's brief while shipping the pill fixes. This is the SSOT for
> the open-source rebuild; execute via the buildâQAâpushâreview loop across
> sessions. Each item = its own branch + PR + review.

## 0. Naming + license + open-sourcing
- **Name = `Yap` (LOCKED by Wilson).** Rename everything: GitHub repo
  (`wilson-voice` â `yap`), bundle id (`com.wilsonguenther.wilson-voice`), product
  name, Info.plist, all UI strings, tray/app copy, and the ASR paths dir
  (`~/Library/Application Support/WilsonVoice` â `Yap`; migrate the existing db).
  Note: renaming the bundle id resets TCC â plan a one-time re-grant at the rename.
- **License = source-available, INTERNAL-USE / NON-COMMERCIAL (LOCKED).** Use
  **BSL 1.1** or **PolyForm Internal-Use** â NOT plain MIT (MIT permits commercial,
  which Wilson does not want). Wilson agreed.
- **README:** for open-source devs â how to download the Whisper model from
  **HuggingFace**, how to configure/swap **on-device STT models** (Whisper sizes,
  Parakeet, Apple SpeechAnalyzer) in Settings, install/permissions, no dev-jargon.

## 1. BUGS (verified from Wilson's real-device smoke â HIGH PRIORITY)
- **fn key race with the macOS emoji/Character Viewer** â holding fn pops the emoji
  picker. Wispr avoids this. Fixes: (a) default binding **fn+Ctrl** (not bare fn),
  (b) guide user to System Settings â Keyboard â "Press ð to â Do Nothing",
  (c) investigate consuming the fn tap (currently LISTEN_ONLY) if we own it.
- **"www.tauri.com" gets typed** when holding fn â the app is leaking the Tauri
  framework's default URL/menu into output. Find the source: default Tauri app
  menu ("Learn More"âtauri.app), leftover template content in index.html, or a
  context-menu/clipboard path. This is embarrassing for an OSS release â must fix.
- **Verify History / Scratchpad / Dictionary delete + copy actually work** end to
  end (all IPC + imports + file structure).

## 2. Analytics â make the MATH extremely accurate (Wilson emphasized)
- â **DONE (branch `fix/analytics-accuracy`, commit 95e805c):** total words, avg
  WPM, day streak, words today are now audited + PROVEN by 11 tests.
  - WPM root-cause fix: `speech_seconds` was the full clip length (silence +
    pauses inflated the denominator). Now it's **real voiced time** via an
    energy VAD (`record.rs::voiced_seconds`: 20ms-frame RMS, adaptive noise
    floor, bridge <=300ms inter-word gaps, drop lead/trail + long pauses). WPM
    is speech-weighted (Î£words / Î£voiced-min) and excludes legacy speech=0 rows.
  - `insights()` reads the authoritative `daily_stats` rollup (rebuilt each
    insert + healed at startup) â removed the O(N) fresh-day full-scan fallback.
  - Test seam `insert_transcript_at` proves streak (consecutive/grace/gaps),
    words-today scoping, total-words, and WPM exclusion. `cargo test` 16/16.
  - NEXT here: expose per-day WPM safely (daily_stats stores `words` over ALL
    rows but `speech_ms` only over speech>0.05 rows â do NOT divide them directly;
    compute per-day WPM from a speech-filtered word sum when charts need it).
- **Track beyond 365 days** (currently 7-day window). Full history retention +
  rollups (daily/weekly/monthly/yearly).
  - â `daily_series(days)` DONE (`db.rs`): contiguous, zero-filled, oldest-first
    day series off the authoritative `daily_stats` (single indexed range scan);
    retention already unlimited (nothing pruned) so any window works. Test proves
    contiguity + zero-fill + sums. Feeds the heatmap + bar/line charts.
  - ✅ monthly_series(months) DONE (contiguous, zero-filled, YYYY-MM). TODO: week/year views + expose daily/monthly via a Tauri command to the Insights UI.
- **15+ chart types**, user-switchable board: bar, line, **circle/donut**,
  rectangle/treemap, triangle, **GitHub-style commit heatmap**, radial, area,
  sparkline, etc. Let users change how the Insights board looks.
- Research OSS chart libs (Recharts, visx, uPlot, D3, Nivo) â pick a good one.

## 3. Full UI overhaul (use the design skills â Wilson asked)
- Cohesive design system across **Home, Permissions, Insights, Dictionary,
  Scratchpad, Settings**. Strip developer-jargon from Settings (user-friendly copy).
- Upgrade Dictionary UI+code and Scratchpad UI+code.
- **App icon overhaul** (soundbars squircle is a start; do a proper pass for OSS).
- **Configurable hotkeys in Settings** (Wilson: "the biggest thing") â a proper
  key-capture UI, multiple bindings per action, validated against system conflicts.

## 3b. ð£ THE YAPPING PILL â origami mascot + reactive personality (Wilson's headline creative vision)

This is the signature identity of Yap. The pill stops being a passive capsule and
becomes a **living mouth / mascot** that reacts to how much you talk. Build it
end-to-end; research is in `docs/research/origami-yapping-pill.md` (fold math),
`docs/research/companion-and-pill-inspiration.md` (UX/direction), and
`docs/research/motion-and-components.md` (render tech).

### â¶ DECIDED DIRECTION â "Kami" (the capsule is scrapped)
From the inspiration research. A **folded-paper creature that lives at the notch**:
- **Peeks** at rest (opt-in, non-blocking, dismissible, purpose-first â anti-Clippy).
- **Unfolds one beat** when you talk; the **beak = the waveform mouth**, driven by
  the live mic level via `MouthDriver` (`desktop/src/pill/mouth.ts`, already built).
- **Chews** while transcribing (fills the dead gap), settles on paste, then **folds
  back the exact reverse** and yaps its word-count line (`tone.ts`) from a
  **Dynamic-Island-style speech bubble** right above it.
- **Pets = swappable `crease.json` packs** (Shimeji-style asset packs â community pets).
- **Position is a setting** â flick-to-corner / pick a spot (Wispr's #1 complaint is a
  fixed pill). Offer a **Pindrop-style HUD-shape picker** (notch / orb / bubble / pill).
- Goo/liquid-glass = later *themes*; "seed-hatch" = a later evolution. v0 fallback if
  the fold is too heavy = a pure-CSS "concertina" accordion.

### â¶ DECIDED RENDER TECH (both research agents converge)
- **Fold = baked Canvas2D, NOT live WebGL.** Bake the crane fold offline with
  **Origami Simulator (MIT)** â per-frame vertices â FOLD JSON â play back with the
  zero-dep `foldPlayer.ts` (~30â50KB, bit-exact reverse). A persistent WebGL context
  on an always-on panel is the main battery/thermal risk â avoid it.
- **Rive (MIT)** for the reactive-personality layer (expressions/blinks) *later* â
  needs `'wasm-unsafe-eval'` in the CSP.
- **HUD look:** native Tauri window **vibrancy** + an **SVG gooey/metaball filter**
  for the speech bubble + **`motion`** (MIT) for micro-interactions + copy-in MIT
  components (shadcn / Magic UI / Cult UI / motion-primitives). **Charts:** uPlot + visx.
- **License landmines â do NOT bundle:** Spline (proprietary + CDN), ReactBits
  (MIT **+ Commons Clause**), Hover.dev (proprietary), p5.js (LGPL â use Two.js),
  GSAP (free-but-non-OSI â use `motion`/anime.js v4/WAAPI), Rabbit Ear (GPL, offline
  tool only). Safe WebGL if ever needed: **ogl** (Unlicense, ~10KB) or three.js (MIT).

### Origami fold/unfold mascot
- At **rest** the pill is a small **folded origami** shape (a compact capsule/seed).
- When you start dictating it **UNFOLDS like real origami** into an animal/creature
  mascot (crane, fox, etc.), and when you finish it **folds back the exact same
  way** â the motion must be the *true reverse* of the fold.
- The fold/unfold has to follow a **real origami crease pattern**, not a fake
  morph: "unfold in the exact way that an origami of whatever animal unfolds in
  real lifeâ¦ it folds back the exact same way." This is the hard, must-nail part.
- **Multiple mascots** = the origami "pets." User can pick/collect creatures.
- **DECIDED v1 stack (research done + license-verified â `docs/research/origami-yapping-pill.md`):**
  **bake-once / play-back-cheap.**
  1. *Offline bake:* sweep Amanda Ghassaei's **Origami Simulator** (MIT, WebGL
     rigid-fold; drivable via `globals.creasePercent` / `setCreasePercent()`)
     0â1 over a **traditional crane** crease pattern â dump per-frame vertex
     positions to `crane.json` (~30KB gzip). Store as **FOLD** format
     (`edemaine/fold`, MIT; loader = `JSON.parse`).
  2. *Runtime:* a **zero-dep Canvas2D `foldPlayer`** interpolates ONE eased
     scalar `t` across baked frames. Unfold `t:0â1`, fold `t:1â0` â the reverse
     is **bit-exact by construction** (this is the ONLY faithful way to get
     "folds back the exact same way"). `rAF` runs only while animating â idle
     CPU â 0. No WebGL context in the always-on panel.
- **License landmines (do NOT ship):** Rabbit Ear (`robbykraft/Origami`) is
  **GPL-3.0** â offline tool only, never bundled. **GSAP/MorphSVG** non-OSI.
  flubber/polymorph/Lottie = silhouette *morphs*, not crease folds (mouth/eye
  accents only). Named **Lang** crease patterns carry author copyright even in an
  MIT repo â use only traditional/public-domain CPs (Origami Simulator's MIT
  `assets/traditionalCrane.svg` etc.) or author custom in Inkscape.
- **Deps (all MIT):** siriwave (optional waveform), **Rive** `rive-app` (v2
  expressive mascot, local `.riv`, CDN-free), **leo-profanity**/`bad-words` (curse
  filter â clean-vs-sassy copy table).
- Must run 60fps, low CPU, in the WebKit webview, fully bundled (no CDN).

### Word-count-reactive "yapping" messages
> â Engine DONE: `desktop/src/pill/tone.ts` â pure `reactiveLine(words, {tone,
> curseFilter}, nonce)`. Buckets tiny/short/medium/long/epic; tones rude/friendly/
> rose/off; independent curse filter (damnâdang); nonce rotates lines. tsc + runtime
> asserts pass. TODO: wire into the pill on transcription-done + a Settings picker.
- The pill/mouth shows **reactive text based on how many words** you dictated in a
  prompt. Tiered by word count, e.g.:
  - short â "got it, got it. okay, mapping it."
  - medium â "oh, you have a lot to say."
  - long â "damn, you're yapping a lot" / "you forgot how to type or something?"
  - very long â "oh my gosh broâ¦ am I your only friend?"
- **Tone presets** (user-selectable in Settings, on/off): **Rude/Sassy** (with a
  **curse filter** toggle â "damn you yapping"), **Friendly**, **Rose** (sweet).
  Curse filter is independent so users can keep the sass but clean the language.
- All copy lives in a config table so it's editable/localizable; ship a good
  default set per tone. Reactive line picked deterministically per session by
  word-count bucket (+ light rotation so it's not repetitive).

### The mouth
- The pill "is supposed to look like a **mouth**"; the emoji/mascot **folds into
  it**. Listening = mouth open/animated with the waveform; done = satisfied.

## 3c. Fill EVERY state gap with animation (Wilson: "there's a bunch of gaps")
- The dead time **after the user stops talking and before text appears**
  (transcribing) needs real motion â not a frozen pill. Audit the whole state
  machine and give each transition an action:
  `idle â (press) â listening â (release) â transcribing/thinking â polishing â
  pasting â done â fold back to idle`. Today only listening/busy/done exist.
- Add "thinking" choreography for the transcribe gap (the origami could be
  mid-fold, or the mascot "chewing/processing"), a paste confirmation beat, and a
  graceful fold-back. 360-audit for every other missing micro-state (errors,
  permission-needed, model-loading, empty result).

## 3d. 360 code audit (senior full-stack pass)
- Audit **every file, function, and diff** to a senior full-stack standard â
  correctness, error handling, dead code, naming, UX. Loop: build â QA â push â
  adversarial review â fix. (Analytics math already went through this â apply the
  same rigor to pill/record/paste/lib/IPC and the whole frontend.)

## 4. Team / collaboration component (bigger feature)
- Devs on the same project can **see each other's sessions**, **share prompts**
  (what people are prompting/saying), and **compete** (word-count leaderboard, like
  the Wispr bar). Cross-app sharing.
- **Security:** encrypted, firewalled, protected. Research the right architecture
  (E2E encryption, self-hostable sync server, opt-in). This is the hardest piece.

## 5. Tooling / research
- Consider **Graphify** (`Graphify-Labs/graphify`, `/graphify`) to map the whole
  app into a knowledge graph so we can see it end-to-end and refactor the file
  structure deliberately.
- Audit dependencies: what OSS deps / npm packages / SDKs are we NOT using that we
  should be (charts, key-capture, crypto, STT engines)?

## Carry-over from prior review (still open)
- Copy/paste in Settings/Scratchpad under menu-bar (Accessory) mode â needs the
  `.regular` activation-policy toggle when the main window opens, or an Edit menu.
- Developer-ID + notarization so TCC grants survive distribution (they already
  survive local rebuilds via consistent re-signing).
- ASR: migrate hot path to Rust `transcribe-rs` + Parakeet-TDT; dictionary â
  Whisper `initial_prompt` biasing is wired (verify it fires from Rust).

## Sequencing suggestion
1. Bugs (Â§1) â fn binding + tauri.com leak + verify delete/copy. 2. Analytics math
(Â§2 first half). 3. Hotkey-config UI + Settings de-jargon (Â§3). 4. Insights charts
(Â§2 second half). 5. UI overhaul + icon (Â§3). 6. Rename + license + README (Â§0).
7. Team component (Â§4) last (hardest). Notarization when distributing.
