export const meta = {
  name: 'yap-cicd-loop',
  description: 'Autonomous CI/CD loop for the Yap app: branch → build → local-verify → PR → CI → adversarial review → fix → squash-merge on green+PASS. Leaves failing items as open PRs. High effort.',
  phases: [
    { title: 'Build', detail: 'implement one item + local verify + open PR' },
    { title: 'CI+Review', detail: 'watch CI + adversarial QA vs acceptance' },
    { title: 'Fix', detail: 'address CI/review issues, re-push' },
    { title: 'Merge', detail: 'squash-merge on green + PASS' },
  ],
}

const REPO = '/Users/wilsonguenther/Desktop/wilson-voice';
const CHANGELOG = 'docs/CHANGELOG-YAP.md';
const MAX_FIX_ROUNDS = 2;
const CI_TIMEOUT = '32m';   // macOS Tauri build + cargo test is slow

// Backlog — each item is tightly scoped with TESTABLE acceptance. Big items point
// at the in-repo authority (the prototype / roadmap) so the agent reads truth, not a guess.
const BATCHES = {
  yap: [
    { id: 'YV1', title: 'Port the polished pill into the live app (world-fill camera + props)',
      ref: 'docs/prototypes/yappy-pill.html',
      spec: 'Port the design in docs/prototypes/yappy-pill.html into desktop/src/pill/YappyPill.tsx (keep it driven by the REAL app events: get_status/status/recording/audio_level/transcript, MouthDriver from ./mouth, reactiveLine from ./tone). Bring over: (1) the PULL-BACK CAMERA — a drawImage source-rect zoom so at rest the capsule is zoomed on the face and on activity it pans back so the pixel WORLD (sky+grass) fills the ENTIRE capsule edge-to-edge (no black rectangle-in-a-box); (2) a SKY-BLUE capsule fill (not obsidian); (3) length-tier PROPS/personas keyed off the transcript wordCount: paragraph → notepad + pencil in the wing; a few paragraphs → a desk + a pixel typewriter + glasses (receptionist); essay → glasses + full filing; (4) tone-aware working chatter + final line (rude/friendly/rose) from a data table. Keep the rest capsule small (tiny face). Do NOT touch ClassicPill, the analytics, or unrelated files. Match surrounding TS style; keep tsc strict-clean.',
      accept: 'desktop/src/pill/YappyPill.tsx contains a pull-back camera via drawImage with a source-rect (grep -n "drawImage(os," must show 8+ numeric args), a sky-blue capsule (grep -in "c6ecff\\|8fd6ff\\|sky" YappyPill.tsx non-empty), tier props (grep -Ein "typewriter|desk|pencil|notepad|receptionist" YappyPill.tsx non-empty), and tone-aware lines (grep -n "rude" YappyPill.tsx non-empty). `cd desktop && npx tsc --noEmit` exits 0 and `cd desktop/src-tauri && cargo test` passes. CI green.' },

    { id: 'YV2', title: 'Rename user-facing "Wilson Voice" → "Yap" (KEEP bundle id + data dir → TCC persists)',
      ref: 'docs/OPEN-SOURCE-ROADMAP.md',
      spec: 'Rename all USER-FACING occurrences of "Wilson Voice" to "Yap": productName in desktop/src-tauri/tauri.conf.json, CFBundleDisplayName/CFBundleName in desktop/src-tauri/Info.plist, the tray menu / window titles, and UI copy strings in desktop/src/**. CRITICAL — do NOT change the bundle identifier "com.wilsonguenther.wilson-voice" (renaming it resets macOS TCC and loses the user\'s Mic/Accessibility grants) and do NOT change the data directory name "WilsonVoice" in data_dir() (renaming it orphans the user\'s SQLite history). Only user-visible text changes. Leave code comments/paths referencing WilsonVoice/wilson-voice as-is where they are identifiers.',
      accept: 'productName is "Yap" in tauri.conf.json; grep -n "com.wilsonguenther.wilson-voice" desktop/src-tauri/tauri.conf.json STILL matches (bundle id unchanged); grep -n "WilsonVoice" desktop/src-tauri/src/lib.rs still shows the data_dir path (unchanged); no USER-FACING "Wilson Voice" string remains in desktop/src/ (grep -rn "Wilson Voice" desktop/src returns nothing). `cd desktop && npx tsc --noEmit` exits 0, `cd desktop/src-tauri && cargo build` succeeds. CI green.' },

    { id: 'YV3', title: 'Smart dictation v1 — context→mode mapping + list formatting (Wispr parity)',
      ref: 'docs/OPEN-SOURCE-ROADMAP.md',
      spec: 'Add smart, context-aware dictation groundwork in Rust (the app already resolves the frontmost app via focus::frontmost_app_name → source_app). (1) Add a pure function that maps an app name / bundle to a dictation MODE: email (Gmail/Mail/Outlook/Superhuman), document (Google Docs/Word/Pages), notes (Notes/Bear/Obsidian), code (Terminal/iTerm/VS Code/Xcode/Warp), chat (Slack/Discord/Messages), else plain. (2) Add a pure text-formatting function that detects LIST intent (dictation with enumerator cues like "first, second, next" or clearly itemized fragments) and formats it as a bullet/numbered list, otherwise preserves paragraphs. Wire nothing risky into the live paste path yet if it needs UI — just the pure functions + a place to call them. Add #[test]s proving: the app→mode mapping for ~6 apps, and that a list-like input formats to a list while prose stays prose. Keep everything else untouched.',
      accept: 'A Rust module/functions exist for app→mode and list-formatting with unit tests (grep -rn "fn .*mode\\|fn format" desktop/src-tauri/src non-empty; new #[test] fns present). `cd desktop/src-tauri && cargo test` passes including the new tests. `cd desktop && npx tsc --noEmit` exits 0. CI green.' },
  ],

  // Batch 2 — smart-dictation groundwork (from docs/research/wispr-parity.md) + the live-reaction pill.
  yap2: [
    { id: 'YV4', title: 'Port the live-reaction pill into the live app',
      ref: 'docs/prototypes/yappy-pill.html',
      spec: 'Update desktop/src/pill/YappyPill.tsx to match the CURRENT docs/prototypes/yappy-pill.html: (1) LIVE reactions WHILE listening — escalate by how long the user has been talking (quick → notes → desk → essay), showing the matching prop + a tone-aware live line as they go, NOT only at done (this fixes "a long dictation is just flapping wings"); (2) a BIGGER open capsule + camera zoomed in (~80% of the buffer, so the chick + grass read larger); (3) slow drifting clouds. Keep the pull-back camera, sky-blue capsule, tier props (notepad/pencil, desk/typewriter, glasses), and the tone-aware done line, all driven by the real recording/audio_level/transcript events. Since actual word count is unknown until the transcript arrives, base the LIVE tier on elapsed listening time. Do NOT touch ClassicPill, analytics, or the pipeline.',
      accept: 'YappyPill.tsx has live listening escalation (grep -Ein "listenT|liveName|listen.*tier" YappyPill.tsx non-empty) and a bigger open capsule + zoom (the open capsule width lerp target is >= 230 and the camera view uses < full buffer width). `cd desktop && npx tsc --noEmit` exits 0; `cd desktop/src-tauri && cargo test` passes. CI green.' },

    { id: 'YV5', title: 'Wire smart-dictation into the pipeline + a Settings dictation-mode picker',
      ref: 'docs/research/wispr-parity.md',
      spec: 'The pure functions dictation::mode_for_app + dictation::format_dictation exist but are LOG-ONLY. Wire them into the transcription pipeline in lib.rs: after transcription, compute the mode from the frontmost app (mode_for_app(source_app)) — unless the user picked a fixed mode — and run format_dictation on the text BEFORE it is stored/pasted. Guard it so it can NEVER lose text (on any error, fall back to the raw transcript). Add a `dictation_mode` field to AppSettings (Rust + serde default "auto" + TS interface) with values auto|plain|list|email|code|notes, and a Settings picker (mirror the existing pill-style picker) that saves it. Add Rust #[test]s proving the wiring: a fixed mode overrides the detected one, and format_dictation is applied. Do not change the ASR/record code or the pill.',
      accept: 'format_dictation is invoked in the pipeline (grep -n "format_dictation" desktop/src-tauri/src/lib.rs non-empty); a dictation_mode setting exists in Rust (grep -n "dictation_mode" desktop/src-tauri/src/lib.rs) and TS (grep -n "dictationMode" desktop/src/App.tsx) with a Settings picker; new #[test]s present and `cd desktop/src-tauri && cargo test` passes; `cd desktop && npx tsc --noEmit` exits 0. CI green.' },

    { id: 'YV6', title: 'Backtrack v1 — rule-based filler + self-correction cleanup (on-device)',
      ref: 'docs/research/wispr-parity.md',
      spec: 'Add a conservative, PURE Rust "backtrack" cleanup in dictation.rs (a fn like clean_backtrack(text) -> String) that: (a) removes standalone filler tokens ("um","uh","er","erm","hmm", and standalone "like"/"you know"/"I mean" used as filler — only when clearly non-semantic); (b) applies spoken self-corrections via trigger phrases ("... actually ...", "scratch that", "no wait, ...", "I mean ...") using the Wispr example semantics ("coffee at 2 actually 3" -> "coffee at 3"); (c) NEVER deletes real content or empties the string (if the result would be empty or lose too much, return the input unchanged). This is rule-based groundwork for the later local-LLM pass — keep it safe + conservative. Call it in format_dictation (or the polish path) behind the same never-lose-text guard. Add #[test]s for filler removal, the "actually" restatement, and content preservation.',
      accept: 'dictation.rs has a backtrack/cleanup fn with #[test]s covering filler removal, self-correction ("actually"), and content preservation (grep -Ein "backtrack|filler|scratch that|actually" desktop/src-tauri/src/dictation.rs non-empty; new #[test] fns). `cd desktop/src-tauri && cargo test` passes; `cd desktop && npx tsc --noEmit` exits 0. CI green.' },
  ],

  // Batch 3 — ship-it groundwork (roadmap §8): error logging, distribution, onboarding, local-LLM architecture. All testable.
  yap3: [
    { id: 'YV7', title: 'Error logging + diagnostics (support visibility)',
      ref: 'docs/OPEN-SOURCE-ROADMAP.md',
      spec: 'Add structured, rotating FILE logging: initialize a logger that writes to a rotating log file under data_dir()/logs/ (e.g. yap.log), keep the existing console logging, capture Rust panics via std::panic::set_hook and write them to the log with a stack/message, and add a Settings "Export diagnostics" control that opens/reveals the logs folder (reuse the open_data_dir pattern). Do not change app behavior otherwise. Use a small, permissive crate if needed (e.g. a rolling file appender) or hand-roll a rotating writer.',
      accept: 'A panic hook + rotating file log under data_dir/logs exist (grep -Ein "set_hook|logs/|rolling|rotate" desktop/src-tauri/src/lib.rs non-empty); a Settings diagnostics/logs control exists (grep -in "diagnostic|export.*log|logs" desktop/src/App.tsx non-empty). `cd desktop/src-tauri && cargo build` succeeds and `cd desktop && npx tsc --noEmit` exits 0. CI green.' },

    { id: 'YV8', title: 'Distribution packaging — DMG + auto-updater + release workflow',
      ref: 'docs/OPEN-SOURCE-ROADMAP.md',
      spec: 'Make Yap a distributable download. (1) In desktop/src-tauri/tauri.conf.json add the "dmg" bundle target alongside "app". (2) Add tauri-plugin-updater (Cargo dep + .plugin(...) init + the JS package) with an updater config block (endpoints + pubkey placeholders + a clear TODO) so the app can auto-update. (3) Add .github/workflows/release.yml that, on a version tag (v*), builds on macos-14, runs the frontend build + cargo build --release, and uploads the produced .dmg + updater manifest as GitHub Release assets; include a notarization step GATED on APPLE_ID/APPLE_PASSWORD/APPLE_TEAM_ID secrets (skip gracefully if absent). DO NOT change the bundle identifier.',
      accept: 'tauri.conf.json bundle targets include "dmg" (grep -n "\\"dmg\\"" desktop/src-tauri/tauri.conf.json); tauri-plugin-updater is wired (grep -rin "updater" desktop/src-tauri/Cargo.toml desktop/src-tauri/src/lib.rs non-empty); .github/workflows/release.yml exists and is valid YAML (python3 -c "import yaml,sys;yaml.safe_load(open(\\".github/workflows/release.yml\\"))"); bundle id still com.wilsonguenther.wilson-voice. `cd desktop/src-tauri && cargo build` + `cd desktop && npx tsc --noEmit` clean. CI green.' },

    { id: 'YV9', title: 'First-run onboarding flow (permissions + voice calibration)',
      ref: 'docs/OPEN-SOURCE-ROADMAP.md',
      spec: 'Add a first-run onboarding React flow rendered over the app: Welcome → Permissions walkthrough (Mic + Accessibility using the existing request_microphone/request_accessibility/get_permissions commands, showing live grant status) → Voice calibration (record a short prompted phrase via the existing recording commands and keep it for later personalization) → Done. Gate on an `onboarded` boolean setting (Rust AppSettings + serde default false; TS interface). Show onboarding when !onboarded on launch; set onboarded=true on finish (saveSettings). Add a "Replay onboarding" button in Settings. Keep the main app working; do not touch the pill or pipeline.',
      accept: 'An onboarding flow/component exists (grep -rin "onboard" desktop/src non-empty) gated by an `onboarded` setting present in BOTH Rust (grep -n "onboarded" desktop/src-tauri/src/lib.rs) and TS (grep -n "onboarded" desktop/src/App.tsx), with a replay control. `cd desktop && npx tsc --noEmit` exits 0 and `cd desktop/src-tauri && cargo build` succeeds. CI green.' },

    { id: 'YV10', title: 'Local-LLM cleanup ARCHITECTURE (pipeline + levels + raw/polished storage)',
      ref: 'docs/research/wispr-parity.md',
      spec: 'Lay the architecture for the Wispr-style LLM cleanup pass WITHOUT downloading a model (model = a later runtime setup step). In Rust define a cleanup PIPELINE (a fn or trait) that runs ordered stages: apply_dictionary (exists) → backtrack cleanup (exists) → format_dictation (exists) → an LLM stage that is a guarded NO-OP STUB now (documented; falls back to the input on any error/timeout). Add a `cleanup_level` setting (none|light|medium|high, default "light") in AppSettings (Rust + serde default + TS) that gates which stages run (none = raw passthrough). Store BOTH raw and polished text on the transcript: add a `raw_text` column to the transcripts table (with a migration) and populate it. Add #[test]s: cleanup_level "none" returns the raw text unchanged; the pipeline runs stages in order; raw_text is stored. Never lose text — every stage guarded.',
      accept: 'A cleanup pipeline + `cleanup_level` setting exist (grep -Ein "cleanup_level|cleanup pipeline|run_cleanup|Polish" desktop/src-tauri/src non-empty; grep -n "cleanup_level" desktop/src-tauri/src/lib.rs and desktop/src/App.tsx); transcripts store raw text (grep -n "raw_text" desktop/src-tauri/src/db.rs); new #[test]s for level gating + raw storage. `cd desktop/src-tauri && cargo test` passes; `cd desktop && npx tsc --noEmit` exits 0. CI green.' },
  ],

  // Batch 4 — voice isolation (from docs/research/voice-isolation.md), loopable pure-Rust tiers.
  yap4: [
    { id: 'YV11', title: 'Signal hygiene — high-pass + normalize/AGC + edge-fade (fixes quiet voice)',
      ref: 'docs/research/voice-isolation.md',
      spec: 'In desktop/src-tauri/src/record.rs, add pure-Rust signal hygiene applied to the captured mono f32 samples BEFORE the 16kHz downsample (Tier 0 in the research): (a) a one-pole/biquad HIGH-PASS filter ~80Hz to kill rumble/hum; (b) RMS-based NORMALIZE / soft AGC toward a target level so quiet speech is boosted WITHOUT clipping (hard-limit to [-1,1]); (c) a short edge-fade (few ms) to de-click start/end. Make each a small PURE function taking &[f32] (+ sample rate) so they are unit-testable in isolation, then call them in the capture path guarded (never produce NaN/empty; on any degenerate input return the samples unchanged). Do NOT change device setup or the resampler; do NOT touch the pill/pipeline.',
      accept: 'record.rs has pure fns for high-pass, normalize/AGC, and edge-fade with #[test]s (grep -Ein "high.?pass|normalize|agc|edge.?fade|fade" desktop/src-tauri/src/record.rs non-empty; new #[test] fns). Tests prove: a very quiet (~-40 dBFS) sine normalizes toward the target within a few dB with NO sample exceeding 1.0; the high-pass attenuates a low-freq tone; hygiene never yields NaN/empty. `cd desktop/src-tauri && cargo test` passes; `cd desktop && npx tsc --noEmit` exits 0. CI green.' },

    { id: 'YV12', title: 'Denoise with nnnoiseless (RNNoise, pure-Rust) over the captured clip',
      ref: 'docs/research/voice-isolation.md',
      spec: 'Add on-device DENOISE using the `nnnoiseless` crate (RNNoise, BSD-3, pure Rust, no external model asset). Run it over the captured audio to suppress steady background noise (fans, hum, keyboard) BEFORE the 16kHz downsample, in a small guarded function (on any error or if it would destroy the signal, fall back to the input unchanged — never lose the utterance). Gate it behind a `denoise` bool setting (AppSettings Rust + serde default true + TS) so the user can turn it off. Add a #[test] proving denoise improves a noisy fixture (segmental SNR up on injected noise) while clean speech is largely preserved (loses <= ~1 dB), and that a fallback path returns input unchanged. Keep it conservative + pipeline-order after hygiene.',
      accept: '`nnnoiseless` is a dependency (grep -n "nnnoiseless" desktop/src-tauri/Cargo.toml) and used in record.rs (grep -in "nnnoiseless\\|denoise" desktop/src-tauri/src/record.rs non-empty) behind a `denoise` setting (grep -n "denoise" desktop/src-tauri/src/lib.rs and desktop/src/App.tsx). A #[test] proves SNR improves on a noisy fixture and the fallback returns input unchanged. `cd desktop/src-tauri && cargo test` passes; `cd desktop && npx tsc --noEmit` exits 0. CI green.' },
  ],

  // Batch 5 — UI overhaul (roadmap §3): de-jargon Settings, Insights charts, configurable hotkeys. Testable UI.
  yap5: [
    { id: 'YV13', title: 'Settings overhaul — plain-language, sectioned, key controls surfaced',
      ref: 'docs/OPEN-SOURCE-ROADMAP.md',
      spec: 'Redesign desktop/src the Settings screen for humans (NOT devs): group controls into clear labeled sections (e.g. Companion, Dictation, Shortcut, Model & Speed, Privacy & Diagnostics), rewrite developer-jargon copy into plain friendly language, and SURFACE the most-used controls prominently near the top: Pill style, Dictation mode, and Denoise. Keep EVERY existing setting wired and saving (do not drop functionality); match the app\'s existing component styles/classes. TypeScript-only + CSS; do not change Rust or the pipeline.',
      accept: 'Settings renders clear section headers and the Pill-style + Dictation-mode + Denoise controls appear (grep -Ein "Companion|Dictation|Shortcut|Diagnostics|Pill style|Dictation mode|Denoise" desktop/src/App.tsx shows the sections + controls); all pre-existing settings keys still referenced (no setting removed — grep the prior keys: model, language, autoPaste, pttBinding, showFloatingPill, pillStyle, dictationMode, cleanup_level/cleanupLevel, denoise, onboarded still present in App.tsx). `cd desktop && npx tsc --noEmit` exits 0; `cd desktop/src-tauri && cargo build` succeeds. CI green.' },

    { id: 'YV14', title: 'Insights charts — expose series via Tauri commands + a bar chart + activity heatmap',
      ref: 'docs/OPEN-SOURCE-ROADMAP.md',
      spec: 'Wire the analytics series into the Insights UI. (1) Add Tauri commands wrapping db.daily_series(days) and db.monthly_series(months) and register them in the invoke_handler. (2) In the Insights view render, from real data, a DAILY WORDS bar/line chart (last ~30 days) and a GitHub-style ACTIVITY HEATMAP (last ~365 days, color-scaled by words) — hand-rolled with inline SVG or a <canvas> (NO heavy chart dependency / no CDN). Graceful empty state when there is no data. Keep the existing Insights stats. TS + a small Rust command; add a #[test] is not required for the chart, but the new commands must compile.',
      accept: 'Tauri commands for daily_series and monthly_series exist and are registered (grep -n "daily_series\\|monthly_series" desktop/src-tauri/src/lib.rs shows command fns + generate_handler entries); the Insights view renders a chart + heatmap from them (grep -Ein "heatmap|daily_series|monthlySeries|<svg|canvas" desktop/src/App.tsx non-empty). `cd desktop/src-tauri && cargo build` + `cd desktop/src-tauri && cargo test` pass; `cd desktop && npx tsc --noEmit` exits 0. CI green.' },

    { id: 'YV15', title: 'Configurable hotkey — a key-capture UI to set the dictation shortcut',
      ref: 'docs/OPEN-SOURCE-ROADMAP.md',
      spec: 'Add a proper "Set shortcut" key-capture control in Settings for the push-to-talk binding, replacing the bare preset chips (keep the fn / fn⌃ presets as quick options). The control listens for the next key combo the user presses and shows it; validate it (require a modifier or one of the supported fn bindings; warn on obvious system conflicts). Wire it to the existing ptt_binding/hotkey settings + the existing binding machinery — do NOT rip out the working CGEvent fn/fn⌃ handling; if arbitrary-key rebinding needs Rust support beyond the current fn/fn_control/both set, keep those as the persisted values and treat the capture UI as choosing among supported bindings (+ a clearly-labeled custom option only if you can wire it safely end to end). Never break the current hold-to-talk.',
      accept: 'A key-capture / "set shortcut" component exists in Settings (grep -Ein "set shortcut|capture|keydown|record.*shortcut" desktop/src/App.tsx non-empty) and updates the ptt binding setting (grep -n "pttBinding\\|hotkeyLabel" desktop/src/App.tsx); the existing fn/fn_control binding still works (ptt_macos + set_binding untouched or extended, grep confirms fn_control still handled). `cd desktop && npx tsc --noEmit` exits 0; `cd desktop/src-tauri && cargo build` succeeds. CI green.' },
  ],

  // Batch 11 — FOUNDATION: kill the Python/MLX sidecar, embed the ASR engine (Handy architecture),
  // make Yap install clean on ANY Mac. Root-cause + port recipes: docs/research/foundation-audit-2026-07-31.json
  // (8-agent audit). Donor repo: /Users/wilsonguenther/oss/handy (cjpais/Handy v0.9.4 — read its real code, do not guess).
  yap11: [
    { id: 'YV30', title: 'Embedded ASR engine module — transcribe-cpp (GGUF+Metal) + model catalog + verified downloader',
      ref: 'docs/research/foundation-audit-2026-07-31.json',
      spec: 'Add the native ASR foundation as NEW Rust modules in desktop/src-tauri/src (no pipeline wiring yet). (1) Engine wrapper module around the same crate Handy uses for GGUF/Metal inference — read /Users/wilsonguenther/oss/handy/src-tauri/Cargo.toml and its engine code and copy the exact crate + version + feature flags (audit says transcribe-cpp 0.1.3 with a metal feature; VERIFY against Handy, do not guess). Expose: load(model_path) -> engine, transcribe(engine, samples_16k_mono) -> text. (2) models.rs: compiled-in catalog via include_str!("catalog.json") — copy Handy top-2 recommended GGUF entries PLUS a whisper tiny/smallest entry for tests, each with pinned revision, download URL(s) incl. mirror if Handy has one, byte size, and sha256. (3) Downloader: async download to data_dir()/models with resumable .partial file, retry w/ backoff, stall timeout, MANDATORY sha256 verify before rename into place, progress callback emitting a typed Tauri event model_download_progress {model_id, downloaded, total}. Unit tests (no network): catalog parses with all required fields; sha256 verifier rejects corrupted fixture bytes; downloader resume/rename path logic via temp dir. The transcribe-cpp/ggml native build MUST compile in CI (macOS runner).',
      accept: 'Cargo.toml gains the engine crate (grep -n "transcribe" desktop/src-tauri/Cargo.toml non-empty); catalog + downloader modules exist (grep -rn "catalog.json\\|sha256\\|\\.partial\\|model_download_progress" desktop/src-tauri/src non-empty); new #[test] fns for catalog/sha256/resume present and `cd desktop/src-tauri && cargo test` passes; `cd desktop && npx tsc --noEmit` exits 0. CI green.' },

    { id: 'YV31', title: 'TranscriptionManager — warm engine lifecycle + model Tauri commands',
      ref: 'docs/research/foundation-audit-2026-07-31.json',
      spec: 'Add a TranscriptionManager owning Arc<Mutex<Option<LoadedEngine>>> modeled on Handy (/Users/wilsonguenther/oss/handy/src-tauri — read its transcription/model manager modules): load selected model off-main via spawn_blocking with catch_unwind panic containment; keep warm; idle-unload watcher (unload after ~15 min unused); transcribe(samples) with a hard timeout that returns Err (never hangs). Async Tauri commands registered in invoke_handler: list_models (catalog merged with downloaded/selected state), download_model(id) (uses YV30 downloader, emits progress events), select_model(id) (persists a new native_model setting, default = the catalog recommended model), delete_model(id), engine_status. NO change to the live dictation path yet. Rust #[test]s: manager state transitions (unloaded -> loaded -> idle-unloaded) using a stub/small path without network.',
      accept: 'Manager + commands exist and are registered (grep -n "list_models\\|download_model\\|select_model\\|engine_status" desktop/src-tauri/src/lib.rs shows generate_handler entries); native_model setting present in Rust settings struct; new #[test]s pass via `cd desktop/src-tauri && cargo test`; `cd desktop && npx tsc --noEmit` exits 0. CI green.' },

    { id: 'YV32', title: 'Wire the pipeline to the native engine + transcript_error event + headless E2E gate',
      ref: 'docs/research/foundation-audit-2026-07-31.json',
      spec: 'Make the native engine the primary transcription path in lib.rs: if the selected native model is downloaded, transcribe via TranscriptionManager; else fall back to the existing python sidecar ONLY when its venv actually exists. On ANY transcription failure emit a new event "transcript_error" {message} (in addition to logging) so the UI can react — the audit proved the current error path emits nothing the UI listens to (lib.rs:641-645). Give EVERY remaining child-process wait/output on the ASR path a timeout (the one-shot fallback currently has none — asr.rs:399-401). Keep the never-lose-text guard. Add a headless CLI mode like Handy: `--transcribe-file <wav>` transcribes with the native engine (auto-downloading the smallest catalog model if absent) and prints the text to stdout then exits. Commit a tiny fixture: generate a short known-phrase wav locally via `say -o` + afconvert (16k mono, < 300 KB) under desktop/src-tauri/tests/fixtures/ with the phrase documented.',
      accept: 'grep -n "transcript_error" desktop/src-tauri/src/lib.rs non-empty and the native path precedes the sidecar fallback; no untimed .output()/.wait() remains on the ASR path (inspect asr.rs); fixture wav committed; LOCAL E2E GATE (the reviewer runs this): `cd desktop/src-tauri && cargo run --release -- --transcribe-file tests/fixtures/<name>.wav` prints text containing the documented phrase word (network allowed for the one-time tiny-model download). `cargo test` passes; `cd desktop && npx tsc --noEmit` exits 0. CI green.' },

    { id: 'YV33', title: 'Onboarding model-download step + kill every first-run soft-lock + honest status',
      ref: 'docs/research/foundation-audit-2026-07-31.json',
      spec: 'Fix the fresh-Mac infinite-loading class end-to-end in the UI. (1) Onboarding (desktop/src/Onboarding.tsx) gains a "Get your model" step BEFORE calibration: 1-2 recommended model cards from list_models, Download button with a live progress bar driven by model_download_progress events, Continue disabled until a model is downloaded + selected. (2) Calibration soft-lock (audit root cause): listen for "transcript_error", clear busy, show the error inline with a Retry button; ALSO add a client-side watchdog (~90s) that clears busy into a visible error state — busy must never be clearable only by success. (3) Remove the synchronous "Install local ASR" main-thread freeze: the native engine replaces it; if a legacy install control remains it must call an async command. (4) Status honesty: show "Ready" only when the native engine has a downloaded, loadable model (or the legacy venv is genuinely importable) — the current python_ok=/usr/bin/python3-shim lie must die; otherwise show a "Model needed" state that routes to the model step. (5) Backend errors surface as an in-app toast, never only a macOS notification. Do not touch the pill, analytics, or dictation cleanup.',
      accept: 'Onboarding has the model step + progress bar (grep -Ein "model_download_progress|progress" desktop/src/Onboarding.tsx non-empty) and a transcript_error listener + watchdog clearing busy (grep -Ein "transcript_error|watchdog|setTimeout" desktop/src/Onboarding.tsx non-empty); status-honesty logic replaces the python_ok-only check (grep -n "python_ok" desktop/src shows it no longer solely gates "Ready"); no UI invocation of a sync setup_asr_venv remains. `cd desktop && npx tsc --noEmit` exits 0; `cd desktop/src-tauri && cargo test` passes. CI green.' },

    { id: 'YV34', title: 'Kill the Python sidecar — native engine becomes the only ASR path',
      ref: 'docs/research/foundation-audit-2026-07-31.json',
      spec: 'Remove the Python/MLX sidecar entirely now that YV30-33 landed: delete the warm-daemon + one-shot python paths (asr.rs python spawning), asr_paths.rs venv/python resolution + the include_str! worker seeding + setup_asr_venv command, every python spawn in get_permissions/status polling (the CLT-shim dialog storm), and the python/ directory at repo root. The native TranscriptionManager is the ONLY transcription path; transcript_error still fires on failure. Update any remaining UI copy referencing "local ASR install"/venv. Keep dictation.rs cleanup, DSP, VAD, paste, analytics, pill untouched. Prove no regression via the YV32 headless gate.',
      accept: '`grep -rn "python3\\|mlx\\|venv\\|asr_worker" desktop/src-tauri/src` returns NOTHING; the python/ dir is deleted from the repo; setup_asr_venv is gone from generate_handler; the YV32 E2E gate still passes: `cd desktop/src-tauri && cargo run --release -- --transcribe-file tests/fixtures/<name>.wav` prints the documented phrase word. `cargo test` passes; `cd desktop && npx tsc --noEmit` exits 0. CI green.' },
  ],
};

let A = {};
try { A = typeof args === 'string' ? JSON.parse(args) : (args || {}); } catch (e) { A = {}; }
const runLabel = A.runLabel || 'yap';
const batch = Array.isArray(A.batch) && A.batch.length ? A.batch : (BATCHES[runLabel] || BATCHES.yap);

const GUARD = `HARD RULES: implement ONLY this one item — no scope creep, no unrelated refactors, no stubs/mock data, targeted minimal diffs matching surrounding style. Repo: ${REPO}. Trunk is main. Use the authenticated gh CLI (account wilsonguenther-dev). Never edit main directly. If you cannot make the LOCAL gate pass, STOP and report opened=false — do NOT open a broken PR.
SCREENSHOT RULE (Wilson standard): if the change affects anything USER-VISIBLE (desktop/src UI, the pill, onboarding, site/), capture screenshot(s) of the rendered result — for React views run the vite dev server and screenshot with Playwright (npx playwright screenshot or a tiny script); for site/ screenshot the built page. Save small PNGs under docs/pr-screenshots/<ITEM-ID>/ ON THE BRANCH, and embed them in the PR body via their raw.githubusercontent branch URL with one annotation line each ("what changed here"). Pure-Rust/no-UI changes skip this.`;

function buildPrompt(item) {
  const branch = 'fix/' + item.id.toLowerCase();
  return `You are a ruthless senior Rust + TypeScript engineer implementing ONE item in the Yap (wilson-voice) macOS Tauri app via a professional CI/CD PR flow.

ITEM ${item.id} — ${item.title}
SPEC: ${item.spec}
ACCEPTANCE (ALL must hold, these are testable): ${item.accept}
READ FIRST: ${REPO}/${item.ref}. Also read the files you'll change + their neighbours to match real structure/style.
${GUARD}

STEPS:
1. cd ${REPO} && git checkout main && git pull --ff-only origin main && git checkout -b ${branch}
2. Implement per spec. Targeted minimal diffs.
3. LOCAL VERIFY (hard gate): cd ${REPO}/desktop && npm ci && npx tsc --noEmit  (must exit 0); then cd ${REPO}/desktop/src-tauri && cargo test  (must pass). Also run the acceptance grep checks yourself and confirm they hold. If anything fails, fix until green or STOP with opened=false + why.
4. Append a one-line entry to ${REPO}/${CHANGELOG} (create it if missing).
5. cd ${REPO} && git add -A && git commit -m "feat(yap): ${item.id} <short summary>" && git push -u origin ${branch}
6. gh pr create --base main --head ${branch} --title "feat(yap): ${item.id} ${item.title}" --body "<what changed + how each acceptance criterion is met>". Capture the PR number.
Return JSON: {implemented, filesChanged[], localVerifyPassed, opened, branch:"${branch}", prNumber, prUrl, sha, notes}.`;
}

function reviewPrompt(item, build) {
  return `Independent adversarial reviewer + CI watcher for a Yap PR. Try HARD to FALSIFY that it meets acceptance. Do NOT spawn sub-agents.
ITEM ${item.id}. ACCEPTANCE: ${item.accept}. PR #${build && build.prNumber} (branch ${build && build.branch}) in ${REPO}.
1. cd ${REPO}. Wait for CI: gh pr checks ${build && build.prNumber} --watch --interval 25 (timeout ~${CI_TIMEOUT}). Record pass/fail per check.
2. gh pr diff ${build && build.prNumber}; open the changed files. RUN each acceptance grep/command yourself and confirm the exact expected output.
3. Falsify: is every criterion genuinely met (reason about a concrete failing input)? Any stubs/mock data? Scope creep (files unrelated to ${item.id})? For YV2 specifically: confirm the bundle id and data_dir were NOT changed.
Return JSON: {ciGreen, failingChecks[], verdict:"PASS"|"FAIL", scopeCreep, issues:[{severity,what,where,fix}]}. PASS only if acceptance is genuinely met AND CI is green. Never claim green if CI failed or never ran.`;
}

function fixPrompt(item, build, review) {
  const issues = JSON.stringify((review && review.issues) || [], null, 1);
  const ci = review && !review.ciGreen ? `CI is RED (${JSON.stringify(review.failingChecks || [])}) — fix the build/tests too.` : 'CI is green; address the review issues.';
  return `Fix PR #${build && build.prNumber} (branch ${build && build.branch}) for ${item.id} in ${REPO}. ${ci}
Fix ONLY these: ${issues}
${GUARD}
cd ${REPO} && git checkout ${build && build.branch} && git pull --ff-only. Implement → LOCAL VERIFY (cd desktop && npx tsc --noEmit; cd desktop/src-tauri && cargo test; re-run the acceptance greps) → append changelog → commit "fix(yap): ${item.id} address CI/review" → git push. Return the build JSON shape (same prNumber/branch).`;
}

function mergePrompt(item, build) {
  return `Merge the approved + green PR #${build && build.prNumber} (branch ${build && build.branch}) in ${REPO}. Final guard: run gh pr checks ${build && build.prNumber} and confirm ALL required checks pass. Then cd ${REPO} && gh pr merge ${build && build.prNumber} --squash --delete-branch && git checkout main && git pull --ff-only origin main. Return {merged, mergeSha, notes}.`;
}

const BUILD_SCHEMA = { type: 'object', additionalProperties: false, required: ['implemented', 'localVerifyPassed', 'opened', 'notes'], properties: { implemented: { type: 'boolean' }, filesChanged: { type: 'array', items: { type: 'string' } }, localVerifyPassed: { type: 'boolean' }, opened: { type: 'boolean' }, branch: { type: 'string' }, prNumber: { type: 'number' }, prUrl: { type: 'string' }, sha: { type: 'string' }, notes: { type: 'string' } } };
const REVIEW_SCHEMA = { type: 'object', additionalProperties: false, required: ['ciGreen', 'verdict', 'scopeCreep', 'issues'], properties: { ciGreen: { type: 'boolean' }, failingChecks: { type: 'array', items: { type: 'string' } }, verdict: { type: 'string', enum: ['PASS', 'FAIL'] }, scopeCreep: { type: 'boolean' }, issues: { type: 'array', items: { type: 'object', additionalProperties: false, required: ['severity', 'what', 'fix'], properties: { severity: { type: 'string', enum: ['critical', 'high', 'medium', 'low'] }, what: { type: 'string' }, where: { type: 'string' }, fix: { type: 'string' } } } } } };
const MERGE_SCHEMA = { type: 'object', additionalProperties: false, required: ['merged', 'notes'], properties: { merged: { type: 'boolean' }, mergeSha: { type: 'string' }, notes: { type: 'string' } } };

log(`Yap CI/CD loop [${runLabel}] — ${batch.length} items in ${REPO}`);
const results = [];
for (let i = 0; i < batch.length; i++) {
  const item = batch[i];
  phase('Build');
  log(`[${item.id}] BUILD (${i + 1}/${batch.length}): ${item.title}`);
  const build = await agent(buildPrompt(item), { label: `build:${item.id}`, phase: 'Build', schema: BUILD_SCHEMA, effort: 'high' });
  if (!build || !build.opened || !build.prNumber) { results.push({ id: item.id, status: 'no-pr', notes: build && build.notes }); log(`[${item.id}] no PR: ${build && build.notes}`); continue; }
  phase('CI+Review');
  let review = await agent(reviewPrompt(item, build), { label: `review:${item.id}`, phase: 'CI+Review', schema: REVIEW_SCHEMA, effort: 'high' });
  let round = 0;
  while (review && (!review.ciGreen || review.verdict === 'FAIL') && round < MAX_FIX_ROUNDS) {
    round++;
    phase('Fix');
    log(`[${item.id}] FIX round ${round}`);
    await agent(fixPrompt(item, build, review), { label: `fix:${item.id}:r${round}`, phase: 'Fix', schema: BUILD_SCHEMA, effort: 'high' });
    phase('CI+Review');
    review = await agent(reviewPrompt(item, build), { label: `review:${item.id}:r${round}`, phase: 'CI+Review', schema: REVIEW_SCHEMA, effort: 'high' });
  }
  const green = review && review.ciGreen && review.verdict === 'PASS' && !review.scopeCreep;
  let merged = null;
  if (green) { phase('Merge'); merged = await agent(mergePrompt(item, build), { label: `merge:${item.id}`, phase: 'Merge', schema: MERGE_SCHEMA }); }
  else log(`[${item.id}] NOT merged — left open for a human`);
  results.push({ id: item.id, prNumber: build.prNumber, prUrl: build.prUrl, ciGreen: review && review.ciGreen, verdict: review && review.verdict, fixRounds: round, merged: merged && merged.merged, status: merged && merged.merged ? 'merged' : 'open' });
  log(`[${item.id}] → ${merged && merged.merged ? 'MERGED' : 'LEFT OPEN'}`);
}
const mergedN = results.filter(r => r.status === 'merged').length;
log(`Yap CI/CD loop done: ${mergedN}/${batch.length} merged`);
return { loop: runLabel, merged: mergedN, total: batch.length, results };
