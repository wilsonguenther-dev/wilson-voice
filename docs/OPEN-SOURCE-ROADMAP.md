# Open-Source Roadmap — (rename from "Wilson Voice")

> Captured from Wilson's brief while shipping the pill fixes. This is the SSOT for
> the open-source rebuild; execute via the build→QA→push→review loop across
> sessions. Each item = its own branch + PR + review.

## 0. Naming + license + open-sourcing
- **Name = `Yap` (LOCKED by Wilson).** Rename everything: GitHub repo
  (`wilson-voice` → `yap`), bundle id (`com.wilsonguenther.wilson-voice`), product
  name, Info.plist, all UI strings, tray/app copy, and the ASR paths dir
  (`~/Library/Application Support/WilsonVoice` → `Yap`; migrate the existing db).
  Note: renaming the bundle id resets TCC — plan a one-time re-grant at the rename.
- **License = source-available, INTERNAL-USE / NON-COMMERCIAL (LOCKED).** Use
  **BSL 1.1** or **PolyForm Internal-Use** — NOT plain MIT (MIT permits commercial,
  which Wilson does not want). Wilson agreed.
- **README:** for open-source devs — how to download the Whisper model from
  **HuggingFace**, how to configure/swap **on-device STT models** (Whisper sizes,
  Parakeet, Apple SpeechAnalyzer) in Settings, install/permissions, no dev-jargon.

## 1. BUGS (verified from Wilson's real-device smoke — HIGH PRIORITY)
- **fn key race with the macOS emoji/Character Viewer** — holding fn pops the emoji
  picker. Wispr avoids this. Fixes: (a) default binding **fn+Ctrl** (not bare fn),
  (b) guide user to System Settings → Keyboard → "Press 🌐 to → Do Nothing",
  (c) investigate consuming the fn tap (currently LISTEN_ONLY) if we own it.
- **"www.tauri.com" gets typed** when holding fn — the app is leaking the Tauri
  framework's default URL/menu into output. Find the source: default Tauri app
  menu ("Learn More"→tauri.app), leftover template content in index.html, or a
  context-menu/clipboard path. This is embarrassing for an OSS release — must fix.
- ✅ **Verified: History / Scratchpad / Dictionary delete + copy work.** Audited the
  full path — every command is registered, frontend invokes match names+args, the
  clipboard plugin is initialized (`copy_entry` → clipboard-manager), and the toast
  renders the raw message (the old "Error: Copied to clipboard" mislabel is gone;
  it toasts "Copied"). Deletes update local state + refresh. Added a DB-layer proof
  test (`delete_and_clear_actually_remove_rows`) covering all three surfaces.

## 2. Analytics — make the MATH extremely accurate (Wilson emphasized)
- ✅ **DONE (branch `fix/analytics-accuracy`, commit 95e805c):** total words, avg
  WPM, day streak, words today are now audited + PROVEN by 11 tests.
  - WPM root-cause fix: `speech_seconds` was the full clip length (silence +
    pauses inflated the denominator). Now it's **real voiced time** via an
    energy VAD (`record.rs::voiced_seconds`: 20ms-frame RMS, adaptive noise
    floor, bridge <=300ms inter-word gaps, drop lead/trail + long pauses). WPM
    is speech-weighted (Σwords / Σvoiced-min) and excludes legacy speech=0 rows.
  - `insights()` reads the authoritative `daily_stats` rollup (rebuilt each
    insert + healed at startup) — removed the O(N) fresh-day full-scan fallback.
  - Test seam `insert_transcript_at` proves streak (consecutive/grace/gaps),
    words-today scoping, total-words, and WPM exclusion. `cargo test` 16/16.
  - NEXT here: expose per-day WPM safely (daily_stats stores `words` over ALL
    rows but `speech_ms` only over speech>0.05 rows — do NOT divide them directly;
    compute per-day WPM from a speech-filtered word sum when charts need it).
- **Track beyond 365 days** (currently 7-day window). Full history retention +
  rollups (daily/weekly/monthly/yearly).
  - ✅ `daily_series(days)` DONE (`db.rs`): contiguous, zero-filled, oldest-first
    day series off the authoritative `daily_stats` (single indexed range scan);
    retention already unlimited (nothing pruned) so any window works. Test proves
    contiguity + zero-fill + sums. Feeds the heatmap + bar/line charts.
  - ✅ `monthly_series(months)` DONE (contiguous, zero-filled, `YYYY-MM`). TODO:
    week/year views + expose daily/monthly via a Tauri command to the Insights UI.
- **15+ chart types**, user-switchable board: bar, line, **circle/donut**,
  rectangle/treemap, triangle, **GitHub-style commit heatmap**, radial, area,
  sparkline, etc. Let users change how the Insights board looks.
- Research OSS chart libs (Recharts, visx, uPlot, D3, Nivo) — pick a good one.

## 3. Full UI overhaul (use the design skills — Wilson asked)
- Cohesive design system across **Home, Permissions, Insights, Dictionary,
  Scratchpad, Settings**. Strip developer-jargon from Settings (user-friendly copy).
- Upgrade Dictionary UI+code and Scratchpad UI+code.
- **App icon overhaul** (soundbars squircle is a start; do a proper pass for OSS).
- **Configurable hotkeys in Settings** (Wilson: "the biggest thing") — a proper
  key-capture UI, multiple bindings per action, validated against system conflicts.

## 3b. 🐣 THE YAPPING PILL — origami mascot + reactive personality (Wilson's headline creative vision)

This is the signature identity of Yap. The pill stops being a passive capsule and
becomes a **living mouth / mascot** that reacts to how much you talk. Build it
end-to-end; research is in `docs/research/origami-yapping-pill.md` (fold math),
`docs/research/companion-and-pill-inspiration.md` (UX/direction), and
`docs/research/motion-and-components.md` (render tech).

### ▶ DECIDED DIRECTION — "Kami" (the capsule is scrapped)
From the inspiration research. A **folded-paper creature that lives at the notch**:
- **Peeks** at rest (opt-in, non-blocking, dismissible, purpose-first — anti-Clippy).
- **Unfolds one beat** when you talk; the **beak = the waveform mouth**, driven by
  the live mic level via `MouthDriver` (`desktop/src/pill/mouth.ts`, already built).
- **Chews** while transcribing (fills the dead gap), settles on paste, then **folds
  back the exact reverse** and yaps its word-count line (`tone.ts`) from a
  **Dynamic-Island-style speech bubble** right above it.
- **Pets = swappable `crease.json` packs** (Shimeji-style asset packs → community pets).
- **Position is a setting** — flick-to-corner / pick a spot (Wispr's #1 complaint is a
  fixed pill). Offer a **Pindrop-style HUD-shape picker** (notch / orb / bubble / pill).
- Goo/liquid-glass = later *themes*; "seed-hatch" = a later evolution. v0 fallback if
  the fold is too heavy = a pure-CSS "concertina" accordion.

### ▶ DECIDED RENDER TECH (both research agents converge)
- **Fold = baked Canvas2D, NOT live WebGL.** Bake the crane fold offline with
  **Origami Simulator (MIT)** → per-frame vertices → FOLD JSON → play back with the
  zero-dep `foldPlayer.ts` (~30–50KB, bit-exact reverse). A persistent WebGL context
  on an always-on panel is the main battery/thermal risk — avoid it.
- **Rive (MIT)** for the reactive-personality layer (expressions/blinks) *later* —
  needs `'wasm-unsafe-eval'` in the CSP.
- **HUD look:** native Tauri window **vibrancy** + an **SVG gooey/metaball filter**
  for the speech bubble + **`motion`** (MIT) for micro-interactions + copy-in MIT
  components (shadcn / Magic UI / Cult UI / motion-primitives). **Charts:** uPlot + visx.
- **License landmines — do NOT bundle:** Spline (proprietary + CDN), ReactBits
  (MIT **+ Commons Clause**), Hover.dev (proprietary), p5.js (LGPL → use Two.js),
  GSAP (free-but-non-OSI → use `motion`/anime.js v4/WAAPI), Rabbit Ear (GPL, offline
  tool only). Safe WebGL if ever needed: **ogl** (Unlicense, ~10KB) or three.js (MIT).

### Origami fold/unfold mascot
- At **rest** the pill is a small **folded origami** shape (a compact capsule/seed).
- When you start dictating it **UNFOLDS like real origami** into an animal/creature
  mascot (crane, fox, etc.), and when you finish it **folds back the exact same
  way** — the motion must be the *true reverse* of the fold.
- The fold/unfold has to follow a **real origami crease pattern**, not a fake
  morph: "unfold in the exact way that an origami of whatever animal unfolds in
  real life… it folds back the exact same way." This is the hard, must-nail part.
- **Multiple mascots** = the origami "pets." User can pick/collect creatures.
- **DECIDED v1 stack (research done + license-verified — `docs/research/origami-yapping-pill.md`):**
  **bake-once / play-back-cheap.**
  1. *Offline bake:* sweep Amanda Ghassaei's **Origami Simulator** (MIT, WebGL
     rigid-fold; drivable via `globals.creasePercent` / `setCreasePercent()`)
     0→1 over a **traditional crane** crease pattern → dump per-frame vertex
     positions to `crane.json` (~30KB gzip). Store as **FOLD** format
     (`edemaine/fold`, MIT; loader = `JSON.parse`).
  2. *Runtime:* a **zero-dep Canvas2D `foldPlayer`** interpolates ONE eased
     scalar `t` across baked frames. Unfold `t:0→1`, fold `t:1→0` → the reverse
     is **bit-exact by construction** (this is the ONLY faithful way to get
     "folds back the exact same way"). `rAF` runs only while animating → idle
     CPU ≈ 0. No WebGL context in the always-on panel.
- **License landmines (do NOT ship):** Rabbit Ear (`robbykraft/Origami`) is
  **GPL-3.0** — offline tool only, never bundled. **GSAP/MorphSVG** non-OSI.
  flubber/polymorph/Lottie = silhouette *morphs*, not crease folds (mouth/eye
  accents only). Named **Lang** crease patterns carry author copyright even in an
  MIT repo — use only traditional/public-domain CPs (Origami Simulator's MIT
  `assets/traditionalCrane.svg` etc.) or author custom in Inkscape.
- **Deps (all MIT):** siriwave (optional waveform), **Rive** `rive-app` (v2
  expressive mascot, local `.riv`, CDN-free), **leo-profanity**/`bad-words` (curse
  filter → clean-vs-sassy copy table).
- Must run 60fps, low CPU, in the WebKit webview, fully bundled (no CDN).

### Word-count-reactive "yapping" messages
> ✅ Engine DONE: `desktop/src/pill/tone.ts` — pure `reactiveLine(words, {tone,
> curseFilter}, nonce)`. Buckets tiny/short/medium/long/epic; tones rude/friendly/
> rose/off; independent curse filter (damn→dang); nonce rotates lines. tsc + runtime
> asserts pass. TODO: wire into the pill on transcription-done + a Settings picker.
- The pill/mouth shows **reactive text based on how many words** you dictated in a
  prompt. Tiered by word count, e.g.:
  - short → "got it, got it. okay, mapping it."
  - medium → "oh, you have a lot to say."
  - long → "damn, you're yapping a lot" / "you forgot how to type or something?"
  - very long → "oh my gosh bro… am I your only friend?"
- **Tone presets** (user-selectable in Settings, on/off): **Rude/Sassy** (with a
  **curse filter** toggle — "damn you yapping"), **Friendly**, **Rose** (sweet).
  Curse filter is independent so users can keep the sass but clean the language.
- All copy lives in a config table so it's editable/localizable; ship a good
  default set per tone. Reactive line picked deterministically per session by
  word-count bucket (+ light rotation so it's not repetitive).

### The mouth
- The pill "is supposed to look like a **mouth**"; the emoji/mascot **folds into
  it**. Listening = mouth open/animated with the waveform; done = satisfied.

## 3c. Fill EVERY state gap with animation (Wilson: "there's a bunch of gaps")
- The dead time **after the user stops talking and before text appears**
  (transcribing) needs real motion — not a frozen pill. Audit the whole state
  machine and give each transition an action:
  `idle → (press) → listening → (release) → transcribing/thinking → polishing →
  pasting → done → fold back to idle`. Today only listening/busy/done exist.
- Add "thinking" choreography for the transcribe gap (the origami could be
  mid-fold, or the mascot "chewing/processing"), a paste confirmation beat, and a
  graceful fold-back. 360-audit for every other missing micro-state (errors,
  permission-needed, model-loading, empty result).

## 3d. 360 code audit (senior full-stack pass)
- Audit **every file, function, and diff** to a senior full-stack standard —
  correctness, error handling, dead code, naming, UX. Loop: build → QA → push →
  adversarial review → fix. (Analytics math already went through this — apply the
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
- Copy/paste in Settings/Scratchpad under menu-bar (Accessory) mode — needs the
  `.regular` activation-policy toggle when the main window opens, or an Edit menu.
- Developer-ID + notarization so TCC grants survive distribution (they already
  survive local rebuilds via consistent re-signing).
- ASR: migrate hot path to Rust `transcribe-rs` + Parakeet-TDT; dictionary →
  Whisper `initial_prompt` biasing is wired (verify it fires from Rust).

## Sequencing suggestion
1. Bugs (§1) — fn binding + tauri.com leak + verify delete/copy. 2. Analytics math
(§2 first half). 3. Hotkey-config UI + Settings de-jargon (§3). 4. Insights charts
(§2 second half). 5. UI overhaul + icon (§3). 6. Rename + license + README (§0).
7. Team component (§4) last (hardest). Notarization when distributing.

## 6. Smart dictation — context-aware formatting (Wispr Flow parity) [NEW]
Wilson wants dictation as smart as Wispr Flow:
- **App/context awareness.** Detect the frontmost app + field and adapt.
  FOUNDATION EXISTS: `focus.rs::frontmost_app_name()` already resolves the app and
  `accepts_text` (focused element editability); each transcript stores `source_app`.
  Extend: map app -> mode (Gmail/Outlook = email; Google Docs/Word = document;
  Notes/Bear = notes; Terminal/iTerm/VS Code = code/plain; Slack/Discord = chat;
  Roblox/games = plain). Keyword the bundle id / app name.
- **Intelligent formatting** (the killer feature). In the ASR polish step
  (`python/asr_worker.py` + the Rust pipeline), format by content + context:
  lists -> bullet/numbered list; paragraphs -> prose; a grocery list in Notes ->
  a list; a formal email in Gmail -> greeting/sign-off/formal tone. Detect intent
  from cues ("first, second", "dear", "grocery list") + the app mode.
- **Modes** (auto + user override in Settings): Formal / Casual / List / Email /
  Code / Notes. Show the active mode on the pill.

## 7. HONEST STATUS (2026-07-23) — what is NOT done yet
Wilson noted the app "still says Wilson Voice + looks like the original." Correct.
Done this arc: analytics accuracy (WPM/streak/series, tested), the switchable
Classic<->Yappy pill (origami retired), the Yappy pixel companion design.
NOT done (still original): 
- **App RENAME "Wilson Voice" -> "Yap"** (§0). Bundle id, product name, Info.plist,
  tray/UI copy, data dir. NOTE: renaming the bundle id resets TCC -> one-time re-grant.
- **Full UI OVERHAUL** (§3): Home / Permissions / Insights / Dictionary / Scratchpad /
  Settings still look like the original app. Needs the whole design pass.
- **Smart formatting** (§6 above).
- **Port the polished pill** (world-fill camera + typewriter/receptionist personas +
  sky-blue capsule) from `docs/prototypes/yappy-pill.html` into `YappyPill.tsx`.

## 8. MASTER PLAN — Yap to a shippable product (Wilson handed me AI/ML/inference lead, 2026-07-23)
Big vision: a downloadable macOS (later Windows) desktop app, distributed via a link on the
Forge front-end platform (drag-and-drop install), auto-updating, with support/error logging,
Wispr-parity intelligence via LOCAL inference, later a ~$5/mo subscription. My model/inference
calls are below. Split into (A) autonomous CI/CD-loop items [testable, merge-on-green] and
(B) prototype-first / human-in-the-loop items [design or model-download heavy].

### Model & inference decisions (my call as the ML lead)
- **Cleanup LLM (the Wispr "second pass"): Qwen2.5-3B-Instruct, 4-bit, via MLX-LM.** Reasons:
  strongest instruction-following in the 3B class, Apache-2.0 (ship-safe), ~1.9GB 4-bit, runs in
  the SAME MLX venv/daemon we already use for Whisper, ~30-60 tok/s on Apple Silicon → sub-second
  cleanup on short dictations. **Latency fallback: Qwen2.5-1.5B-Instruct-4bit** (~1GB, faster) for
  older/8GB Macs, selectable in Settings. Gemma-2-2B is a viable alt but Qwen2.5 wins on control +
  size. NEVER block paste on the LLM — hard timeout (~600ms) → fall back to rule-based/raw.
- **ASR: keep MLX-Whisper** (large-v3-turbo default; small/base for speed). Already wired.
- **VAD / voice isolation: research pending** (agent launched) — candidates: Silero VAD (gate),
  RNNoise / DeepFilterNet (denoise), Apple's built-in **Voice Isolation** (AUVoiceIsolation /
  AVAudioApplication) — likely: Apple Voice Isolation + Silero VAD on-device, cheap + native.

### (A) CI/CD-loop items (testable → autonomous batches)
- **Error logging + diagnostics**: structured rotating log file in the data dir, a global panic/
  error hook, "Export diagnostics" in Settings. (Support/bug visibility.)
- **Distribution packaging**: add DMG bundle target + `tauri-plugin-updater` (auto-update) +
  a GitHub Actions **release** workflow that builds, signs, **notarizes**, and publishes the DMG +
  updater manifest on a tag. (Drag-and-drop download + auto-update to all users.)
- **Onboarding flow**: first-run React flow — welcome → permissions walkthrough → **voice-sample
  capture** (records a calibration phrase for later personalization) → done, gated by `onboarded`.
- **Local-LLM cleanup ARCHITECTURE**: a `Polish` pipeline trait — rule-based Backtrack (done) as
  default + a slot for the MLX-LLM cleanup behind a setting, graceful fallback; model loaded at
  runtime via a `setup` command (NOT in CI). Wire cleanup levels (None/Light/Medium/High) + store
  `raw_text` + `polished_text` + an "Undo AI edit" toggle (from the Wispr research).
- **UI (testable parts)**: Settings de-jargon + make Dictation-mode/Pill-style prominent;
  configurable-hotkeys capture UI; Insights charts wired to daily_series/monthly_series.

### (B) prototype-first / human-in-the-loop
- **Local-LLM cleanup pass (runtime)**: download Qwen2.5-3B-4bit into the MLX venv via a setup
  step; the actual model-in-the-loop cleanup (can't run in CI). Prototype prompt + guardrails,
  Wilson reviews output quality before it's on by default.
- **Voice isolation/denoise**: after research — integrate the chosen denoiser into record.rs.
- **UI visual overhaul**: Home/Insights/Settings redesign — prototype in artifacts, Wilson
  approves, then loop the buildable pieces.
- **Voice personalization / fine-tuning**: use captured samples + frequent-term dictionary to
  bias/adapt (initial_prompt biasing now; LoRA later). Future.
- **Windows build + $5/mo subscription + licensing**: later; needs the cross-platform + billing pass.
