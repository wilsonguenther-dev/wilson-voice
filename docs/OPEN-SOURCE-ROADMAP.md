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
- **Verify History / Scratchpad / Dictionary delete + copy actually work** end to
  end (all IPC + imports + file structure).

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
  rollups (daily/weekly/monthly/yearly). NEXT INCREMENT: add `daily_series(days)`
  + month/week rollup queries off `daily_stats`; feeds the heatmap + charts.
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
