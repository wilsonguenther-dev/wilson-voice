//! Wilson Voice — production dictation desktop app
//!
//! Hotkeys: primary FN / FN+Control via CGEvent tap (ptt_macos).
//! Optional legacy ⌘⇧V via Carbon global-shortcut.
//! HUD: floating float.html pill, parked bottom-center (no cursor chase).

// YV93 — public because the timed decode (`transcribe_timed`) and the shipped
// model's `Capabilities()` are what the meeting chunker is built on, and both
// are probed from outside the crate (`tests/asr_capabilities_probe.rs`, the
// eval harness).
pub mod asr_engine;
pub mod channel;
mod cli;
mod command_mode;
// YV64: reads macOS' own crash reports + the panic hook's log lines back into
// `crash_events`. Local only — see the module docs for the privacy rules.
pub mod crash;
// YV94: public because the schema is now the thing under test. The migration
// ladder, the FTS5 sync triggers and the delete-cascade are claims about a real
// SQLite file, so `tests/db_migration_idempotent.rs`, `meeting_fts_search.rs`,
// `meeting_delete_cascade.rs` and `meeting_stats_rollup.rs` open one and check.
pub mod db;
// Public so the golden formatting corpus in `tests/fixtures/formatting/` can run
// the real pipeline from an integration test (YV59).
pub mod dictation;
// YV120 — the diarization eval metrics (DER / JER / enrollment-EER) and the
// `CosineDistance` / `CosineSimilarity` newtypes. In the LIBRARY rather than in
// a test binary for two reasons the module header spells out: yap23's shipped
// clustering API takes a `CosineDistance` (a newtype declared inside a test
// crate could not be that type, and a second declaration is the mixed-unit bug
// merged finding #20 is about), and four separate integration-test binaries
// score against these same functions. Pure — no sidecar, no model bytes, no
// audio — which is why it can land before any diarization code exists.
pub mod diarize_metrics;
// YV121 — the parent side of the `yap-diarize` sidecar: `SidecarPool`'s four
// policies (handshake, bounded lateness, one restart, idle unload) ported to a
// child that takes its models by request rather than on argv. Public because
// `tests/diarize_sidecar_pool.rs` drives the real state machine against a stub
// PROCESS, and because its clustering API is where `CosineDistance` stops being
// a convention and becomes a signature.
pub mod diarize;
// The JSONL contract with the `yap-diarize` sidecar. Compiled into BOTH
// binaries from this one file so the two ends cannot drift — and its own unit
// tests therefore run inside each of them.
pub mod diarize_protocol;
// YV95: public because OS-12's energy rule is now a function with a test.
// `hover_tick_ms` is the whole of fix (2) — a visible-but-untouched pill during
// a three-hour meeting polls at 1 Hz, not 13 Hz — and
// `tests/pill_idle_tick_during_recording.rs` drives it directly rather than
// trying to infer a sleep duration from the outside.
pub mod float_pill;
mod focus;
/// Y4-H — the formatting screen's copy, derived from the pipeline's own
/// `runs_*` predicates so it cannot drift from what the pipeline does.
pub mod formatting_options;
// YV73: the disk sweep + the memory telemetry line. Pure selection rules, so
// "what may I delete" is testable without a filesystem — see the module docs.
mod hygiene;
// YV92 — the input format-change state machine (AirPods mid-meeting) plus the
// CoreAudio listeners that make it prompt. Public because the state machine is
// the falsifiable half: `tests/input_format_change_handler.rs` drives it with a
// synthetic event sequence and no audio hardware at all.
pub mod input_format;
mod latency;
// YV93 — meeting ASR: the VAD-cut chunker, the seam merge, the preemptible +
// resumable chunk driver, and the English-only gate. Public because every one
// of those is a falsifiable claim with a test file of its own
// (`tests/meeting_*.rs`) and because the eval harness (`tests/meeting_eval.rs`)
// must score the SHIPPED chunk geometry and the SHIPPED merge, not a copy.
pub mod meeting_asr;
pub mod meeting_pipeline;
// YP2 — offline Ed25519 license verification, the 14-day trial, and the single
// gate that stands in front of a NEW dictation (and nothing else).
pub mod license;
// yap-lifecycle — Dock reopen, one Quit, bounded teardown, stale-instance probe.
pub mod lifecycle;
mod logging;
// YV91 — the meeting capture session: bounded memory, an RT-safe capture
// callback, host-time anchors, the idle-sleep power assertion and the
// dictation fan-out. Public so the acceptance tests
// (`tests/meeting_capture_*.rs`) drive the REAL session, not a copy of it.
pub mod meeting;
// YV99 — the 22-A error matrix as a const table plus the pure policy for the
// three rows this item owns (journal backpressure, sleep/wake, the 3 h cap).
// Public because the matrix's whole purpose is to be checked from the outside:
// `tests/matrix_coverage.rs` walks the table against the filesystem, and
// `matrix_row5/16/17_*.rs` drive the state machines with synthetic events.
pub mod meeting_matrix;
// YV94 — the Notetaker's row types, its migration-1 SQL, and the pure Markdown
// renderer. Public so the export's "round-trips readably" claim is a test
// (`tests/meeting_markdown_export.rs`) rather than an eyeball.
pub mod meetings;
// YV95 — the manual start/stop control plane (finding #6's "no way to start a
// meeting"), and the OS-12 instrumentation it writes into the meeting row.
// Public because "a user can start and stop a meeting" is the item's acceptance
// line, and `tests/meeting_manual_start_stop.rs` +
// `tests/meeting_diagnostics_row.rs` assert it against a synthetic capture
// engine instead of a stopwatch.
pub mod meeting_control;
pub mod meeting_energy;
// PERM-A — public so `tests/mic_auth_status.rs` can assert the
// AVAuthorizationStatus discriminants and the call-site sweep without a
// microphone, a grant or a window server.
pub mod mic_auth;
// YV93 — public so the English-only meeting gate can be asserted against the
// real bundled catalog from `tests/meeting_english_only_gate.rs`.
pub mod models;
// YV101 — the macOS 14.4 runtime gate for the system-audio process tap (plan
// finding OS-11). Public so `tests/syscapture_os_gate.rs` can drive the version
// table, and because every future tap entry point has to pass through it.
pub mod os_version_gate;
pub mod paste;
mod paste_tx;
pub mod perm_probe;
pub mod perm_truth;
pub mod permissions;
// YV61: the validated polish stage. Public for the same reason `dictation` is —
// the golden formatting corpus runs the real stage from an integration test.
pub mod polish;
// YV91 — the IOKit idle-sleep assertion. Public so the "never the display
// variant" rule is assertable from an integration test.
pub mod power;
// The JSONL contract with the `yap-polish` sidecar. Compiled into BOTH binaries
// from this one file (see the module docs) so the two ends cannot drift. Public
// since YV97: the summary path's request kinds, its sampler plan and its
// truncate-and-warn budget fitting all live here, and
// `tests/summarize_grammar_chain.rs` holds them to their contract.
pub mod polish_protocol;
// Public since Y1-A: the tap-disable classifier and the session's tap-health
// counters are asserted by `tests/tap_health.rs`, which cannot reach a private
// module — and a claim about a hotkey that silently stops listening is exactly
// the kind that must be testable without a window server.
#[cfg(target_os = "macos")]
pub mod ptt_macos;
#[cfg(target_os = "macos")]
use ptt_macos::StopOrigin;
/// Off macOS there is no fn tap; the origins still name the stop's caller.
#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
enum StopOrigin {
    Hotkey,
    OnScreen,
    MenuBar,
}
mod record;
/// Y3-D — the clip guard, re-exported so `tests/cancel_long_take.rs` can drive
/// the REAL keep-the-audio path (`keep_cancelled_take` takes a `&mut ClipWav`)
/// instead of a parallel reimplementation of it that would stop describing the
/// code the moment the two drifted. One type, not the whole `record` module:
/// capture internals stay private.
pub use record::ClipWav;
/// Y3-F — the declared maximum session length, its 80% warning, and the rule +
/// cut that enforce them. Re-exported for the same reason as the two above:
/// `record` stays crate-private and `tests/max_session.rs` drives exactly these.
pub use record::{
    cap_cut_index, max_session_samples, session_cap_tick, session_cap_warning, SessionCapAction,
    CUT_GRACE, MAX_SESSION, MAX_SESSION_SECONDS, SESSION_WARN_AT, SESSION_WARN_PERCENT,
    TARGET_RATE,
};
/// Y3 — the dictation capture consumer's test seam and its memory ceiling.
/// `record` itself stays crate-private; these two are what
/// `tests/dictation_capture_memory.rs` drives.
pub use record::{capture_probe, MAX_RESIDENT_CAPTURE_BYTES};
/// DB-B — the launch sweep of `recordings/` and the recovery read-back, so
/// `tests/long_take_kill_mid_decode.rs` replays the real startup order after a
/// real abort.
pub use record::{read_wav_16k_mono, sweep_stale_wavs};
// YV92 — rate conversion and the anti-alias filter that now sits in front of
// it. Public for the same reason `input_format` is: the ≥20 dB-at-10 kHz claim
// belongs in a test that can measure it (`tests/biquad_lowpass_response.rs`),
// and the eval harness compares the two decimators end to end.
pub mod resample;
// YV91 — the preallocated lock-free SPSC ring the capture callback writes into
// (OS-7). Public so the zero-allocation claim is a test rather than a comment.
pub mod rtring;
mod secure_input;
// YV95 — every global chord in one table, so "the meeting hotkey does not
// collide with dictation-hold" (finding #6) is `tests/tray_hotkey_no_collision.rs`
// reading the same constants `run()` registers, not an eyeball over three
// literals.
pub mod shortcuts;
// yap-p0-chord-deadlock — global shortcuts are routed in the plugin handler, acted on after it.
mod shortcut_defer;
// yap-p0-hotkeys-no-permission — the zero-TCC dictate chord, the one hotkey
// arbiter and the truthful hotkey status.
pub mod hotkeys;
pub mod hotkey_table;
// Fix round 2: the exclusive Carbon probe the dictate registrar runs first.
pub mod carbon_probe;
use hotkeys::{HotkeyEdge, HotkeySource};
/// Y4-G: `pub` so `tests/undo_restores_raw.rs` can drive `append_signature`
/// directly — the signature block is the one stage that copies bytes verbatim
/// after polish, so undo has to be asserted against the real function.
pub mod snippets;
// yap-format-structure: spoken lists -> written lists. Pure, std-only; also
// compiled into the yap-polish sidecar via #[path].
pub mod structure;
// YV97 — the meeting summarizer: token-based chunking, MAP-stage extraction
// under a per-chunk grammar, and the ported V1-V7 gate. Public because all five
// of its acceptance criteria are integration tests over these pure functions.
pub mod summarize;
// YV98 — the support bundle: build, redact, preview, compose. Deliberately NOT
// in `crash.rs`, whose no-network test must stay green (see the module docs).
pub mod support;
// YV91 made this public: finding #9's rule (a meeting NEVER mutes the Mac) is
// enforced by `sysaudio::mute_for_take`, and that gate is asserted from
// `tests/meeting_no_automute.rs` against a fake output device.
pub mod sysaudio;
// The system-audio tap and its watchdog, in one module because they are one
// subject: YV100 (22-B) is the CoreAudio process tap itself — global-exclude-
// self, feeding the 22-A RT ring verbatim — and YV104 / OS-4 is the zero-buffer
// ghost watchdog that decides when that tap has to be rebuilt, the 3-per-meeting
// budget, the rebuild log YV106 persists, and the "did this tap ever deliver a
// non-zero sample" discriminator that is the ONLY honest way to tell a TCC
// denial from Apple's all-zero-buffer bug. Public because everything that can be
// proved without a Mac, a 14.4 kernel or a TCC grant IS proved from outside, in
// `tests/syscapture_*.rs`: the aggregate-device composition dictionary, the
// setup/teardown state machine, the IOProc's zero-allocation body and its
// `extern "C-unwind"` panic boundary, and every watchdog decision.
pub mod syscapture;
// YV93 — public because the preemption contract (a dictation request takes the
// one warm engine back from meeting ASR at the next chunk boundary) is proved
// against the REAL manager in `tests/meeting_dictation_preempts_transcription.rs`
// and `tests/matrix_new_asr_chunk_timeout.rs` (error-matrix row `3b`).
/// Y3-C — the only thing allowed to state progress through a running decode.
pub mod transcribe_progress;
pub mod transcription;
// YV93 — public for `WarmVad::speech_spans`, the silence map the chunker cuts
// its boundaries on.
pub mod vad;
/// The words a packed log line is allowed to keep — Yap's own, and nothing
/// else. Public so `support_bundle_redaction` can assert the guarantee from
/// outside the module that relies on it.
pub mod vocab;
/// The Rust string-literal extractor `vocab`'s corpus is built from. Its own
/// file, because `build.rs` `include!`s it to run the extraction at build time
/// — one implementation, compiled on both sides of the wall.
pub mod vocab_extract;

/// Y0-D — the ONE place the app's state root is decided (`YAP_DATA_DIR`).
pub mod app_paths;
/// Y0-D — `--smoke`: the launch mode that registers no hotkey and never pastes.
pub mod smoke;

// YV91 finding #27: the two counters `tests/meeting_no_model_resident.rs` reads
// to prove a meeting capture never brings an ASR model resident and never
// defeats the idle sweepers. Re-exported (rather than making the whole module
// public) so the test surface is exactly these two numbers.
pub use transcription::{engine_touches, model_load_calls, IDLE_UNLOAD_AFTER};

use db::{
    CrashEvent, Database, DayCount, DictCandidate, DictEntry, FailedDictation, Insights,
    ScratchNote, Snippet, TranscriptEntry,
};
use parking_lot::Mutex as PLMutex;
use permissions::PermissionReport;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle, Emitter, Manager, State, WindowEvent, Wry,
};
// YV95: the chords themselves now live in `shortcuts.rs` (one table, one
// collision test), so this only needs the plugin's extension trait and the
// pressed/released discriminator.
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::UpdaterExt;

/// YV41: the container-level `serde(default)` (backed by the `Default` impl
/// below) means a field missing from settings.json falls back to its default
/// instead of failing the whole load — a partial store can never reset a user's
/// config. Individually *invalid* fields are handled by `salvage_settings`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// YV41 settings-schema marker for one-time forward migrations. Fresh
    /// installs stamp the current version; a store written before the field
    /// existed reads as version 0 (see `apply_settings_migrations`) and is
    /// migrated + rewritten once.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub language: String,
    pub auto_paste: bool,
    pub hotkey_label: String,
    /// Always show HUD; also auto-shows while recording even if false.
    pub show_floating_pill: bool,
    /// fn | fn_control | both — CGEvent FN PTT (not Carbon).
    #[serde(default = "default_ptt_binding")]
    pub ptt_binding: String,
    /// YV49 command mode: which EXTRA modifier, held with `ptt_binding`, makes a
    /// press edit the selection instead of typing. command (⌘, default) | option
    /// (⌥) | off. Independent of `ptt_binding` so plain dictation is unaffected.
    #[serde(default = "default_command_binding")]
    pub command_binding: String,
    /// Keep Carbon ⌘⇧V as secondary hold binding.
    #[serde(default = "default_true")]
    pub keep_cmd_shift_v: bool,
    /// yap-p0-hotkeys-no-permission — the user's rebind of the "Works
    /// everywhere" dictate chord (Code + modifiers only). `None` = the default
    /// `shortcuts::DICTATE_TOGGLE` (⌃⌥⌘Space).
    #[serde(default)]
    pub dictate_toggle: Option<hotkeys::ChordSetting>,
    /// focus-2026-10-19 — the Record meeting chord rebind. `None` = ⌃⌘M.
    #[serde(default)]
    pub meeting_chord: Option<hotkeys::ChordSetting>,
    /// focus-2026-10-19 — the Cancel take key rebind. `None` = Escape.
    #[serde(default)]
    pub cancel_chord: Option<hotkeys::ChordSetting>,
    /// Floating pill style: "classic" (obsidian capsule) | "yappy" (pixel pet).
    #[serde(default = "default_pill_style")]
    pub pill_style: String,
    /// Where the pill docks on screen (YV53): "bottom" (centred island, the
    /// default) | "left" | "right" (Wispr-style side dock, vertically centred
    /// and flush to that screen edge). Drives `float_pill::PillPosition` for the
    /// NSPanel origin and the pill's own edge alignment in the webview.
    #[serde(default = "default_pill_position")]
    pub pill_position: String,
    /// Companion tone (YV27): friendly | rude | rose (default "friendly").
    /// Drives Yappy's reactive lines (pill chatter + house mood). Kept
    /// independent of `pill_style` so either companion can be warm, sassy, or
    /// sweet. The pill/house read this live off the emitted settings.
    #[serde(default = "default_companion_tone")]
    pub companion_tone: String,
    /// Smart-dictation mode: auto | plain | list | email | code | notes.
    /// "auto" infers the mode from the frontmost app; any other value forces it.
    #[serde(default = "default_dictation_mode")]
    pub dictation_mode: String,
    /// Auto-Cleanup level (YV10): none | light | medium | high (default
    /// "medium" since Y4-A). Gates the cleanup pipeline — "none" is a raw
    /// passthrough; higher levels enable dictionary → backtrack → formatting →
    /// local-LLM polish (see `dictation::CleanupLevel` / `run_cleanup`).
    ///
    /// Y4-A raised the shipped value from "light" to "medium": "light" stops
    /// one stage short of `runs_format()`, so a fresh install did no spoken
    /// punctuation, no lists, no email shape — "formatting does not work
    /// whatsoever".
    #[serde(default = "default_cleanup_level")]
    pub cleanup_level: String,
    /// Y4-A provenance: true once the user has actually MOVED the Auto-Cleanup
    /// picker. Nothing else writes it. A default nobody chose may be migrated
    /// out from under the user (that is what v1 → v2 does to "light"); a value
    /// the user picked may not, and without this bit the two are
    /// indistinguishable on disk — `save_settings` persists the whole struct,
    /// so every store has a `cleanupLevel` key from the first save onward.
    #[serde(default)]
    pub cleanup_level_set_by_user: bool,
    /// Y4-A: set by the v1 → v2 migration so the app can say ONE quiet line
    /// about formatting now being on, and where to turn it off. Cleared by the
    /// UI when the line is dismissed. False on a fresh install — a first run
    /// has no behaviour change to explain.
    #[serde(default)]
    pub formatting_notice_pending: bool,
    /// Where snippet triggers may fire (YV48): `inline` (anywhere in the
    /// transcript, the default) or `utterance` (only when the trigger is the
    /// WHOLE utterance). See `snippets::SnippetScope`.
    #[serde(default = "default_snippet_scope")]
    pub snippet_scope: String,
    /// YV61 local-LLM polish: the catalog id of the installed polish model
    /// (`models::polish_models()`), or `""` — the default — for OFF. With no id
    /// (or no downloaded file) `polish::polish_llm` never spawns the sidecar and
    /// the cleanup pipeline is exactly what it is today.
    #[serde(default)]
    pub polish_model: String,
    /// Hard deadline for one polish pass, in ms (spec §2.3, default 1200). Past
    /// it the model's answer is dropped and the rules text is what pastes.
    #[serde(default = "default_polish_deadline_ms")]
    pub polish_deadline_ms: u64,
    /// The `style_<mode>` tone dial (spec §2.4/R14), keyed by dictation mode
    /// (`email`, `chat`, …) with `very casual | casual | default | formal` as
    /// values. A mode with no entry — the default for all of them — is
    /// `default`. A map, not one field per mode, so a new mode needs no schema
    /// change.
    #[serde(default)]
    pub polish_styles: std::collections::BTreeMap<String, String>,
    /// YV62 (R13) — the sign-off block appended to a take, e.g.
    /// `"Wilson — drivia.consulting"`. Empty by default. It is copied BYTE FOR
    /// BYTE by `snippets::append_signature` AFTER the polish stage, so no model
    /// can rewrite it and none can invent one (`polish::validate_polish` rejects
    /// an invented signature on the V3/V5 path).
    #[serde(default)]
    pub signature: String,
    /// When that block is appended: `off` (the default — never) | `cue` (only
    /// when the take ends with "sign it") | `auto` (a cue, or any email that
    /// closes on a sign-off line). See `snippets::SignatureMode`.
    #[serde(default = "default_signature_mode")]
    pub signature_mode: String,
    /// Denoise the captured clip with RNNoise before transcription (YV12).
    /// Suppresses steady background noise (fans, hum, keyboard) over the
    /// native-rate buffer before the 16 kHz downsample. Defaults on; the
    /// denoiser itself falls back to the raw audio on any degeneracy.
    #[serde(default = "default_true")]
    pub denoise: bool,
    /// Auto-mute the whole Mac's system output while dictating (YV28). On record
    /// start we snapshot + mute the default output device so nothing plays over
    /// the user; on stop / cancel / error / exit we restore the EXACT prior mute
    /// and volume. Defaults on; the restore is unconditional so the Mac is never
    /// left muted even if this is toggled off mid-take.
    #[serde(default = "default_true")]
    pub mute_while_dictating: bool,
    /// First-run onboarding completed (YV9). While false the UI shows the
    /// welcome → permissions → voice-calibration flow; set true on finish.
    #[serde(default)]
    pub onboarded: bool,
    /// Calibration phrase captured during onboarding, kept for later
    /// personalization (initial_prompt biasing / future voice adaptation).
    #[serde(default)]
    pub calibration_sample: Option<String>,
    /// Selected embedded (GGUF) ASR model — a `models::catalog()` repo id
    /// (YV31). Since YV34 deleted the Python sidecar this is THE model the
    /// dictation path runs on; there is no other ASR selector.
    #[serde(default = "default_native_model")]
    pub native_model: String,
    /// YV80 — load the ASR model at LAUNCH instead of on the first dictation.
    /// Defaults false: Yap idles without ~930 MB of GGUF resident, and the
    /// first take arms the engine while the user is already talking (see
    /// `arm_asr_engine`). True restores YV38's eager path for power users who
    /// would rather spend the memory all day than the one-time first-take wait.
    #[serde(default)]
    pub preload_model: bool,
    /// Launch Yap at login (YV42), applied through tauri-plugin-autostart's
    /// macOS LaunchAgent. Defaults OFF — nothing installs a login item behind
    /// the user's back; the toggle is applied immediately on save and re-applied
    /// at every startup so this setting, not a stale LaunchAgent, is the truth.
    #[serde(default)]
    pub autostart: bool,
    /// Look for a newer Yap on GitHub Releases (YV44). The check ONLY notifies:
    /// nothing is downloaded or installed until the user clicks "Install now"
    /// (`install_update`). Defaults on; off means Yap never contacts the
    /// release endpoint at all.
    #[serde(default = "default_true")]
    pub check_updates: bool,
    /// A version the user dismissed with "Skip this version" (YV44). That exact
    /// version is never offered again; any newer release still is.
    #[serde(default)]
    pub skipped_update_version: Option<String>,
    /// YV46 first-run gate for the pre-SQLite `history/transcripts.json` import
    /// (`Database::migrate_json_if_needed`). Backend-owned bookkeeping, not a
    /// user preference: it flips to true after the one import attempt so no
    /// later launch stats that path again. Every install created after the
    /// SQLite switch simply flips it on its first launch with nothing to read.
    /// `save_settings` re-asserts the stored value, so a UI save can never
    /// clear it and re-arm the migration.
    #[serde(default)]
    pub legacy_json_migrated: bool,
    /// yap-format-structure — "Turn spoken lists into bullets". On by default:
    /// enumerations become lists, "topic, verdict" lines become `Topic: verdict`
    /// bullets, formatted for the app the take is pasted into. Rules only —
    /// never routed through the polish model.
    #[serde(default = "default_true")]
    pub structure_enabled: bool,
    /// Per-app override of that format, keyed by bundle id
    /// (`com.apple.mail` -> `markdown` | `plain` | `off`). Empty by default: the
    /// built-in table in `structure::structure_format_for` decides. A BTreeMap
    /// so the store is written in a stable order.
    #[serde(default)]
    pub structure_app_overrides: std::collections::BTreeMap<String, String>,
}

/// Current settings-schema version (YV41). Bump this ONLY together with a new
/// arm in `apply_settings_migrations` that upgrades stores written at the
/// previous version.
const CURRENT_SETTINGS_SCHEMA_VERSION: u32 = 2;

fn default_schema_version() -> u32 {
    CURRENT_SETTINGS_SCHEMA_VERSION
}
fn default_ptt_binding() -> String {
    "fn_control".into()
}
fn default_command_binding() -> String {
    "command".into()
}
fn default_pill_style() -> String {
    "classic".into()
}
fn default_pill_position() -> String {
    "bottom".into()
}
fn default_companion_tone() -> String {
    "friendly".into()
}
fn default_dictation_mode() -> String {
    "auto".into()
}
fn default_cleanup_level() -> String {
    "medium".into()
}
fn default_snippet_scope() -> String {
    "inline".into()
}
fn default_polish_deadline_ms() -> u64 {
    polish::DEFAULT_POLISH_DEADLINE_MS
}
fn default_signature_mode() -> String {
    "off".into()
}
fn default_true() -> bool {
    true
}
/// Fresh installs (and settings written before YV31) select the catalog's
/// recommended model — one source of truth, no hardcoded id to drift.
fn default_native_model() -> String {
    models::recommended_model().id.clone()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SETTINGS_SCHEMA_VERSION,
            language: "en".into(),
            auto_paste: true,
            hotkey_label: "fn⌃".into(),
            // Always-on glass island rides all Spaces
            show_floating_pill: true,
            ptt_binding: "fn_control".into(),
            command_binding: "command".into(),
            keep_cmd_shift_v: false,
            dictate_toggle: None,
            meeting_chord: None,
            cancel_chord: None,
            pill_style: "classic".into(),
            pill_position: "bottom".into(),
            companion_tone: "friendly".into(),
            dictation_mode: "auto".into(),
            cleanup_level: "medium".into(),
            cleanup_level_set_by_user: false,
            formatting_notice_pending: false,
            snippet_scope: "inline".into(),
            polish_model: String::new(),
            polish_deadline_ms: polish::DEFAULT_POLISH_DEADLINE_MS,
            polish_styles: std::collections::BTreeMap::new(),
            signature: String::new(),
            signature_mode: "off".into(),
            denoise: true,
            mute_while_dictating: true,
            onboarded: false,
            calibration_sample: None,
            native_model: default_native_model(),
            preload_model: false,
            autostart: false,
            check_updates: true,
            skipped_update_version: None,
            legacy_json_migrated: false,
            structure_enabled: true,
            structure_app_overrides: std::collections::BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub recording: bool,
    pub busy: bool,
    pub last_error: Option<String>,
    pub message: String,
    pub accessibility: bool,
    /// yap-p0-hotkeys-no-permission — what each keyboard surface is REALLY
    /// doing: the Carbon dictate chord's register result and the fn tap's
    /// observed delivery. Replaces the old `true`-on-thread-spawn flag.
    #[serde(default)]
    pub hotkeys: hotkeys::HotkeysStatus,
    /// The degraded-mode line ("fn is off — <reason>. Use ⌃⌥⌘Space (works
    /// without permissions).") — `None` while fn is delivering.
    #[serde(default)]
    pub hotkey_degraded: Option<String>,
    /// The label of the registered dictate chord (default ⌃⌥⌘Space).
    #[serde(default)]
    pub dictate_toggle_label: String,
    #[serde(default)]
    pub hands_free: bool,
    /// YV33 — dictation can actually run right now: the selected embedded model
    /// is downloaded. The UI shows "Ready" only when this is true; otherwise it
    /// routes to the model step.
    #[serde(default)]
    pub model_ready: bool,
    /// YV43 — another app holds macOS Secure Input, so the fn PTT event tap is
    /// blind and the hotkey cannot fire. True means the UI must SAY so instead
    /// of continuing to advertise "hold fn⌃".
    #[serde(default)]
    pub secure_input_blocked: bool,
    /// The holder + workaround line for the banner. `None` when not blocked.
    #[serde(default)]
    pub secure_input_detail: Option<String>,
    /// YV80 — the ASR engine is being loaded right now. Since the load is lazy
    /// by default this is true for the first take of a session, between the
    /// user releasing their key and the decode starting, and the UI SAYS so
    /// (see [`ENGINE_PREPARING_MESSAGE`]) rather than showing a "transcribing"
    /// state for a model that is still coming off disk.
    #[serde(default)]
    pub engine_loading: bool,
    /// Y1-A — macOS switched the CGEvent tap off (slow callback, sleep/wake, or
    /// a user-input flood) and we switched it back on. `None` for a healthy
    /// session AND for the first re-arm, which is silent by design; `Some` from
    /// the second one, when a tap that keeps dying is a fault worth surfacing.
    #[serde(default)]
    pub tap_health_message: Option<String>,
    /// yap-hotkey-stop — the live take is in its "Still recording" state (a
    /// hold with no key-up for 90 s, or the tap was switched off mid-take). The
    /// pill swaps its phase line for `ptt_macos::STILL_RECORDING_MESSAGE`.
    #[serde(default)]
    pub still_recording: bool,
}

struct AppState {
    settings: PLMutex<AppSettings>,
    recording: PLMutex<bool>,
    busy: PLMutex<bool>,
    /// Hands-free latch (double-tap fn⌃); release keys but keep recording.
    hands_free: PLMutex<bool>,
    /// Y3-F — the in-flight take has already crossed [`record::SESSION_WARN_AT`]
    /// and the one calm ceiling notice has been said. Latched so the watchdog,
    /// which ticks at HUD cadence, says it ONCE; cleared when a take starts.
    session_cap_warned: PLMutex<bool>,
    /// YV49 command mode: the selection captured when THIS take began, read via
    /// AX at key-down. `Some` marks the in-flight take as a command (edit the
    /// selection) rather than dictation (type the transcript). Consumed by
    /// `stop_and_transcribe` and cleared by `cancel_recording`, so it can never
    /// leak into a later, ordinary take.
    command_selection: PLMutex<Option<String>>,
    recorder: PLMutex<Option<record::ActiveRecording>>,
    /// System-output state captured when we muted the Mac for a take (YV28).
    /// `Some` only while a recording has the output muted; restored + cleared on
    /// stop / cancel / error / exit so the Mac is never left muted.
    saved_audio: PLMutex<Option<sysaudio::OutputAudioState>>,
    db: Arc<Database>,
    last_error: PLMutex<Option<String>>,
    /// yap-p0-hotkeys-no-permission — the ONE start/stop decision for every
    /// hotkey edge, Carbon or fn tap (see `take_hotkey_edge`).
    hotkey_arbiter: PLMutex<hotkeys::HotkeyArbiter>,
    /// Menu-bar (tray) handles, set once during setup after the tray is built.
    /// Kept so state transitions can reflect into the dropdown + tooltip (YV26).
    tray: PLMutex<Option<TrayIcon<Wry>>>,
    /// The dynamic Start/Stop Dictation item (label follows `recording`).
    tray_dictation: PLMutex<Option<MenuItem<Wry>>>,
    /// The Hands-free check item (check follows the `hands_free` latch).
    tray_hands_free: PLMutex<Option<CheckMenuItem<Wry>>>,
    /// YV51 — the "Undo AI Edit" item; its enabled state follows `undo_available`.
    tray_paste_raw: PLMutex<Option<MenuItem<Wry>>>,
    /// YV95 — the "Record a meeting" / "Stop meeting" item. Its label follows
    /// the meeting controller, and it is DISABLED when no capture engine is
    /// installed, because a Record button that does nothing is worse than one
    /// that says why it cannot.
    tray_meeting: PLMutex<Option<MenuItem<Wry>>>,
    /// YV125 — the "Record a meeting as…" submenu (In person / Virtual / Not
    /// sure). Disabled while a meeting is recording, for the same reason the
    /// item above becomes "Stop meeting": there is nothing to pick a kind FOR.
    tray_meeting_kind: PLMutex<Option<Submenu<Wry>>>,
    /// Y2-E — the DISABLED header carrying the license state ("Trial — 5 days
    /// left" / "Licensed" / "Trial ended — dictation paused"). Its text comes
    /// from [`license::tray_line`], the same decision the pill renders, so the
    /// menu bar and the pill cannot disagree about the trial.
    tray_license: PLMutex<Option<MenuItem<Wry>>>,
    /// Y2-E — "Upgrade Yap — $29 once". Held so `sync_tray` can INSERT it into
    /// and REMOVE it from the menu rather than grey it out: muda has no hidden
    /// item, and a disabled upgrade line is still the wrong sentence sitting in
    /// front of someone who already paid.
    tray_upgrade: PLMutex<Option<MenuItem<Wry>>>,
    /// Y2-E — the tray's own `Menu`, kept because `TrayIcon` does not hand it
    /// back and the upgrade item above has to be inserted into something.
    tray_menu: PLMutex<Option<Menu<Wry>>>,
    /// Y2-E — is the upgrade item currently IN the menu? Edge-triggered so
    /// `sync_tray` mutates the menu only when the answer changes.
    tray_upgrade_shown: PLMutex<bool>,
    /// Y2-E — does the tray icon currently wear the urgent treatment? Also
    /// edge-triggered: re-setting an NSStatusItem's image on every status emit
    /// is work nobody asked for, and this runs on the dictation hot path.
    tray_urgent: PLMutex<bool>,
    /// YV95 — the one owner of "a meeting is recording". Built in `setup` (it
    /// needs an `AppHandle` for its status sink), so this is a `OnceLock` rather
    /// than a field: every entry point reads it through [`AppState::meeting`]
    /// and none of them can start a second one.
    meeting: std::sync::OnceLock<Arc<meeting_control::MeetingController>>,
    /// YV51 — does the newest take have a raw transcript that differs from the
    /// polished text that was pasted? Gates the "Undo AI Edit" tray item and
    /// ⌃⌘Z, so the action is only offered when there IS an AI edit to undo.
    /// Cached (seeded at startup, rewritten as each take lands) so `sync_tray`
    /// never touches the DB on a state transition.
    undo_available: PLMutex<bool>,
    /// YV43 — latest Secure Input watchdog snapshot. Written ONLY by the
    /// watchdog thread on an edge; read by every `build_status`, so the pill,
    /// the banner and the tray tooltip all agree.
    secure_input: PLMutex<secure_input::SecureInputStatus>,
    /// Warm Silero VAD (YV36) — loaded ONCE from disk during startup (see the
    /// background thread in `run()`) and reused by every dictation, instead of
    /// re-loading + re-failing an ONNX graph on each clip. `None` until the
    /// model asset is on disk (or forever, if the download/load fails), which
    /// is the explicit energy-VAD fallback.
    vad: PLMutex<Option<Arc<vad::WarmVad>>>,
    /// YV39 cancellation generation. Every cancel/interrupt bumps it; the
    /// transcribe worker captures it when the take stops and re-checks it right
    /// before the paste, so a dictation the user cancelled while ASR was still
    /// running can never land ⌘V in their app seconds later.
    paste_generation: AtomicU64,
    /// Y3-D — set while the user has asked THIS take to stop.
    ///
    /// `paste_generation` (YV39) already invalidates a stale paste, but it is a
    /// weaker statement than a cancel: a stale take is still copied to the
    /// clipboard, still written to history, and still decoded to completion. A
    /// cancel means none of those happen. Cleared when a take arms, so a cancel
    /// can never leak into the next one; read at every stage boundary of the
    /// worker by [`take_is_cancelled`], which is the same checkpoint a chunked
    /// decode loop calls once per chunk.
    take_cancelled: AtomicBool,
    /// YP2 licensing. Owns `license.json`, the corroborating trial rows in
    /// SQLite, and the cached revocation list. Read (never cached) by the ONE
    /// gate in front of a new dictation; nothing else in the app consults it.
    license: Arc<license::LicenseManager>,
    /// Warm embedded-ASR engine lifecycle (YV31). Owns the loaded GGUF model
    /// and its idle-unload watcher; backs the model-management commands. Since
    /// YV34 it is the app's ONLY transcriber (see `stop_and_transcribe`).
    transcription: transcription::TranscriptionManager,
    /// YV98 — the diagnostics bundle the user last previewed, held between the
    /// preview and the send. The preview shows the REAL redacted bytes, so the
    /// send has to write those same bytes; rebuilding at send time would make
    /// the preview a preview of a different file.
    support_bundle: PLMutex<Option<support::PreparedBundle>>,
}

/// The environment variable that relocates the app's ENTIRE state root (Y0-D).
///
/// Unset — the shipped default, `<Application Support>/WilsonVoice`, unchanged.
/// Set — every stateful path (history DB, settings, models, recordings,
/// recovery, meetings, logs) moves under it, so a second launch cannot read or
/// write the first one's dictation history. This is an OVERRIDE, not a rename:
/// the default directory name is frozen because renaming it orphans the SQLite
/// history of every existing install.
///
/// Resolved exactly once, by [`app_paths::paths`]. `--smoke` REFUSES to start
/// unless this names a directory outside the default root (see [`smoke`]).
pub const DATA_DIR_ENV: &str = "YAP_DATA_DIR";

/// The app's state root. Honours [`DATA_DIR_ENV`]; identical to the shipped
/// behaviour when that is unset.
pub fn data_dir() -> PathBuf {
    let p = app_paths::paths().root.clone();
    let _ = std::fs::create_dir_all(&p);
    let _ = std::fs::create_dir_all(p.join("recordings"));
    p
}

/// The user's home, where macOS keeps its crash reports (YV64). Falls back to
/// `.` for the same reason `data_dir` does — a missing home must not panic.
fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Where a failed take's WAV is parked so it can be retried (YV52).
///
/// Deliberately NOT the recordings dir: `record::sweep_stale_wavs` empties that
/// at every startup, which would destroy exactly the clips a recovery needs.
/// Everything in here is purged after `db::FAILED_TAKE_RETENTION_DAYS`.
pub fn recovery_dir() -> PathBuf {
    app_paths::paths().recovery.clone()
}

/// YV95 — where a meeting's audio is written.
///
/// Deliberately NOT `recordings/` (swept clean at every startup by
/// `record::sweep_stale_wavs`, which would delete a three-hour meeting the app
/// crashed out of) and not `recovery/` (purged on the failed-take schedule). A
/// meeting's WAV is owned by its row and by YV94's 7-day retention sweep, and
/// nothing else may decide it is garbage.
pub fn meetings_dir() -> PathBuf {
    let p = app_paths::paths().meetings.clone();
    let _ = std::fs::create_dir_all(&p);
    p
}

impl AppState {
    /// The meeting controller, once `setup` has built it. `None` only during the
    /// first moments of startup, and every caller treats that as "not yet".
    fn meeting(&self) -> Option<Arc<meeting_control::MeetingController>> {
        self.meeting.get().cloned()
    }
}

/// YV95 — the single status broadcast for a meeting: the pill and the main
/// window both listen to `meeting`, and both render the SAME `elapsedLabel`
/// string, so the clock cannot disagree with itself.
///
/// OS-12 fix (1) lives here: this is called once per second by the controller's
/// ticker, and the only thing it does per tick is emit. The tray label is
/// touched on the recording EDGE only — a menu mutation is a main-thread hop,
/// and 10,800 of them over a meeting is the exact cost this item exists to not
/// pay.
fn meeting_status_sink(app: AppHandle) -> meeting_control::StatusSink {
    static TRAY_SHOWS_RECORDING: AtomicBool = AtomicBool::new(false);
    Arc::new(move |status| {
        let _ = app.emit(meetings::MEETING_EVENT, status);
        // The pill's hover watch drops to 1 Hz while this is set (OS-12 fix 2).
        float_pill::set_meeting_recording(status.recording);
        if TRAY_SHOWS_RECORDING.swap(status.recording, Ordering::SeqCst) == status.recording {
            return;
        }
        let recording = status.recording;
        let app_main = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(state) = app_main.try_state::<Arc<AppState>>() {
                if let Some(item) = state.tray_meeting.lock().clone() {
                    let _ = item.set_text(if recording {
                        "Stop meeting"
                    } else {
                        MEETING_TRAY_START_LABEL
                    });
                }
                // YV125 — a kind is something you pick FOR a meeting you are
                // about to start. While one is running there is nothing to pick
                // it for, and a live submenu would read as "change this
                // meeting's kind", which it is not.
                if let Some(submenu) = state.tray_meeting_kind.lock().clone() {
                    let _ = submenu.set_enabled(!recording);
                }
            }
        });
    })
}

/// The tray item's idle label. A verb and a noun, so the item reads the same way
/// to somebody who has never heard of this feature — the acceptance line for
/// this item is a user who has not read a changelog.
const MEETING_TRAY_START_LABEL: &str = "Record a meeting";

/// YV95 — every entry point (tray item, ⌃⌘M, the pill's stop control, the
/// Meetings empty state) funnels through here, so all four do exactly one thing.
///
/// The pill is shown for the duration when the user has it enabled: an
/// always-visible recording indicator is the cheap, real trust feature YV96's
/// consent copy leans on, and the OS-12 fixes above are what make it affordable.
///
/// YV125 — `kind` is what the user said this meeting is, and it is only read on
/// a START: the tray's plain item and ⌃⌘M pass `Unknown` (the picker skipped),
/// the "Record a meeting as…" submenu and the Meetings tab pass what was
/// chosen. A press that STOPS a meeting ignores it, because the recording that
/// is ending was started under whatever it was started under.
fn toggle_meeting(
    app: &AppHandle,
    state: &Arc<AppState>,
    kind: meetings::MeetingKind,
) -> Result<meeting_control::MeetingStatus, String> {
    let controller = state.meeting().ok_or("Yap is still starting up")?;
    let was_recording = controller.is_recording();
    let status = controller.toggle_with_kind(&meetings_dir(), None, kind)?;
    if status.recording && !was_recording {
        if state.settings.lock().show_floating_pill {
            float_pill::show_for_recording(app);
        }
        notify(
            app,
            "Recording this meeting",
            // fix-249 advisory (e): the LIVE chord, not the literal ⌃⌘M.
            hotkey_table::meeting_started_body(&hotkey_table::active_meeting().1),
        );
    } else if !status.recording && was_recording {
        // Never yank the pill out from under a dictation that is still running.
        if !*state.recording.lock() {
            float_pill::after_recording(app, state.settings.lock().show_floating_pill);
        }
    }
    Ok(status)
}

/// YV95 — the same toggle, but never on the main thread.
///
/// The tray item's handler and the ⌃⌘M shortcut handler both run on the macOS
/// MAIN THREAD, and stopping a meeting is not cheap work: it wakes and joins the
/// 1 Hz ticker, finalizes YV91's journal into a playable wav, joins the capture
/// watchdog and writes three rows to SQLite. Doing that inline freezes the menu
/// bar — and the Stop control the user just pressed — for as long as it takes.
///
/// So the two main-thread entry points hand the work to a short-lived thread and
/// return immediately. Everything the toggle does to the UI already hops back to
/// the main thread on its own (`float_pill::show_for_recording`/`after_recording`
/// go through `dispatch_main`; the tray label is set from the status sink's own
/// `run_on_main_thread`), so nothing here needs a second hop of its own.
///
/// The controller is still the only thing that decides whether a toggle is a
/// start or a stop, and it makes that decision under its own lock — so two
/// impatient presses cannot become two meetings, they become a start and a stop.
fn toggle_meeting_off_thread(
    app: &AppHandle,
    state: &Arc<AppState>,
    source: &'static str,
    kind: meetings::MeetingKind,
) {
    let app = app.clone();
    let state = state.clone();
    std::thread::Builder::new()
        .name("yap-meeting-toggle".into())
        .spawn(move || {
            if let Err(e) = toggle_meeting(&app, &state, kind) {
                log::warn!("meeting toggle from {source}: {e}");
                notify(&app, "Yap", e);
            }
        })
        .ok();
}

/// Can a take actually be transcribed right now? YV34 leaves exactly one
/// answer: the selected embedded model is downloaded. False means "Model
/// needed", never "Ready".
fn model_ready(state: &AppState) -> bool {
    let native = state.settings.lock().native_model.clone();
    native_model_ready(&native).is_some()
}

/// YV80 — what a take says while the engine is still being loaded. Word for
/// word the line `ModelSetup`'s ribbon already shows during setup, because it
/// is the same fact ("your speech engine isn't ready yet, hold on") and the
/// user should not have to learn a second phrase for it.
pub const ENGINE_PREPARING_MESSAGE: &str = "Preparing your speech engine…";

/// The status-line policy, pure so the "Ready" honesty rule is testable without
/// an AppState. `model_ready` false outranks everything except live
/// recording/transcribing/errors: a fresh install with no model must say so.
///
/// YV43: `secure_input_blocked` outranks the model + accessibility lines for the
/// same honesty reason — while another app holds Secure Input the fn tap is
/// blind, so telling the user to "hold fn⌃" is telling them to do something that
/// physically cannot work.
///
/// The inputs are a named struct, not ten positional arguments: eight of them
/// are `bool`s, and a swapped pair at a call site compiled silently.
#[derive(Clone, Copy)]
struct StatusInputs<'a> {
    recording: bool,
    hands_free: bool,
    busy: bool,
    engine_loading: bool,
    last_error: Option<&'a str>,
    model_ready: bool,
    accessibility: bool,
    secure_input_blocked: bool,
    ptt: &'a str,
    session_cap_warned: bool,
}

/// The status line for `inputs` — see [`StatusInputs`] for the precedence.
fn status_message(inputs: &StatusInputs<'_>) -> String {
    let StatusInputs {
        recording,
        hands_free,
        busy,
        engine_loading,
        last_error,
        model_ready,
        accessibility,
        secure_input_blocked,
        ptt,
        session_cap_warned,
    } = *inputs;
    if recording && session_cap_warned {
        // Y3-F: past 80% of `record::MAX_SESSION` the live take's own line SAYS
        // what is coming, on the status event the pill already renders. It
        // outranks the ordinary recording copy because it is the same fact plus
        // the part the user does not know; it is set once by the latch, so this
        // is a state the line holds, never a notice that repeats.
        record::session_cap_warning()
    } else if recording && hands_free {
        format!("Hands-free… tap {ptt} to stop")
    } else if recording {
        format!("Recording… release {ptt} or click Stop")
    } else if busy && engine_loading {
        // YV80: the first take of a session waits on the model coming off disk
        // before a single sample is decoded. Saying "Transcribing" there would
        // be a lie about which multi-second thing is happening.
        ENGINE_PREPARING_MESSAGE.into()
    } else if busy {
        "Transcribing with local Whisper…".into()
    } else if let Some(err) = last_error {
        format!("Error: {err}")
    } else if secure_input_blocked {
        secure_input::BLOCKED_MESSAGE.into()
    } else if !model_ready {
        "Model needed — download a speech model to start".into()
    } else if !accessibility {
        format!("Ready — {ptt} (enable Accessibility)")
    } else {
        format!("Ready — hold {ptt} · double-tap hands-free")
    }
}

fn build_status(state: &AppState) -> AppStatus {
    let recording = *state.recording.lock();
    let busy = *state.busy.lock();
    let hands_free = *state.hands_free.lock();
    let last_error = state.last_error.lock().clone();
    let accessibility = permissions::is_accessibility_trusted();
    let ptt = state.settings.lock().hotkey_label.clone();
    let ready = model_ready(state);
    let secure = state.secure_input.lock().clone();
    let engine_loading = state.transcription.is_loading();
    // Y1-A: the tap-health line rides on the same `status` event the pill
    // already listens to, so PERM-C's denied-state surface renders it without a
    // second channel.
    #[cfg(target_os = "macos")]
    let tap_health_message = ptt_macos::tap_health_message(ptt_macos::tap_health());
    #[cfg(not(target_os = "macos"))]
    let tap_health_message: Option<String> = None;
    #[cfg(target_os = "macos")]
    let still_recording = recording && ptt_macos::still_recording_prompted();
    #[cfg(not(target_os = "macos"))]
    let still_recording = false;
    let message = status_message(&StatusInputs {
        recording,
        hands_free,
        busy,
        engine_loading,
        last_error: last_error.as_deref(),
        model_ready: ready,
        accessibility,
        secure_input_blocked: secure.blocked,
        ptt: &ptt,
        session_cap_warned: *state.session_cap_warned.lock(),
    });
    #[cfg(target_os = "macos")]
    let message = if still_recording {
        ptt_macos::STILL_RECORDING_MESSAGE.to_string()
    } else {
        message
    };
    // yap-p0-hotkeys-no-permission DO 4/5: truthful hotkey status + the
    // explicit degraded-mode line. Secure Input outranks the stored tap state.
    let mut hotkeys_now = hotkeys::status();
    hotkeys_now.fn_tap =
        hotkeys::effective_fn_tap(&hotkeys_now.fn_tap, secure.blocked, secure.culprit.as_deref());
    let (_, dictate_toggle_label) = hotkeys::active_dictate();
    let probe = OsPermissionProbe;
    let hotkey_degraded = hotkeys::degraded_line(
        &hotkeys_now.carbon,
        &hotkeys_now.fn_tap,
        hotkeys::PermissionProbe::tap_plausible(&probe),
        &dictate_toggle_label,
    );
    // Idle and otherwise "Ready": the main window's header status chip names
    // the degraded mode instead of advertising a fn hold that cannot fire (the
    // floating pill does not render `message`).
    let message = match (&hotkey_degraded, recording || busy || last_error.is_some()) {
        (Some(line), false) if ready => line.clone(),
        _ => message,
    };
    AppStatus {
        recording,
        busy,
        last_error,
        message,
        accessibility,
        hotkeys: hotkeys_now,
        hotkey_degraded,
        dictate_toggle_label,
        hands_free,
        model_ready: ready,
        secure_input_blocked: secure.blocked,
        secure_input_detail: secure.blocked.then(|| secure.detail()),
        engine_loading,
        tap_health_message,
        still_recording,
    }
}

fn emit_status(app: &AppHandle, state: &AppState) {
    let _ = app.emit("status", &build_status(state));
    sync_tray(app, state);
}

/// Reflect the live recording / busy / hands-free state into the menu-bar
/// dropdown (Start↔Stop Dictation label, Hands-free check) and the tray tooltip
/// (idle vs recording vs transcribing). Called from `emit_status`, so every
/// state transition already funneling through it also refreshes the toolbar —
/// keeping the menu-bar control in lockstep with the pill + hotkey (YV26).
///
/// muda menu / NSStatusItem mutations must run on the main thread on macOS, and
/// `emit_status` is sometimes invoked from the ASR worker thread, so the actual
/// updates are hopped onto the main thread with cheap Arc-backed handle clones.
fn sync_tray(app: &AppHandle, state: &AppState) {
    let recording = *state.recording.lock();
    let busy = *state.busy.lock();
    let hands_free = *state.hands_free.lock();
    let secure_input_blocked = state.secure_input.lock().blocked;
    let dictation = state.tray_dictation.lock().clone();
    let hf_item = state.tray_hands_free.lock().clone();
    let raw_item = state.tray_paste_raw.lock().clone();
    let undo_available = *state.undo_available.lock();
    let tray = state.tray.lock().clone();
    // Y2-E — the license state, from the SAME two functions the pill's
    // `pillLicense` mirrors. `LicenseManager::status()` re-derives entitlement
    // from the already-in-memory signed blob: no disk read, no network, no
    // cached "licensed" boolean to go stale. That is what makes it safe to ask
    // on the path `emit_status` already runs on every state transition.
    let license = state.license.status();
    let (license_line, urgent) = license::tray_line(&license);
    let offers_purchase = license::tray_offers_purchase(&license);
    let license_item = state.tray_license.lock().clone();
    let upgrade_item = state.tray_upgrade.lock().clone();
    let tray_menu = state.tray_menu.lock().clone();
    // Read the two edge flags, then commit the new values here rather than in
    // the closure below: the closure has to be 'static, so it cannot hold
    // `&AppState`. Committing early means a `run_on_main_thread` that never
    // runs leaves the flags describing the menu we asked for rather than the
    // one we have — a one-emit skew that the next transition corrects, which is
    // the cheaper of the two failure modes (the other is a permanent redundant
    // insert attempt on every emit).
    let upgrade_shown = *state.tray_upgrade_shown.lock();
    let was_urgent = *state.tray_urgent.lock();
    *state.tray_upgrade_shown.lock() = offers_purchase;
    *state.tray_urgent.lock() = urgent;
    if dictation.is_none() && hf_item.is_none() && tray.is_none() {
        return; // tray not built yet (early startup emit)
    }
    let _ = app.run_on_main_thread(move || {
        // YV51: "Undo AI Edit" is greyed out unless the newest take actually has
        // a raw that differs from what was pasted — an always-enabled item that
        // silently does nothing is worse than a disabled one.
        if let Some(item) = raw_item {
            let _ = item.set_enabled(undo_available);
        }
        if let Some(item) = dictation {
            let _ = item.set_text(if recording {
                "Stop Dictation"
            } else {
                "Start Dictation"
            });
        }
        if let Some(item) = hf_item {
            let _ = item.set_checked(hands_free);
        }
        // Y2-E — the header sentence. One `set_text`, every state, no branch
        // here: the branching happened in `license::tray_line`.
        if let Some(item) = license_item {
            let _ = item.set_text(&license_line);
        }
        // Y2-E — the upgrade line appears and disappears rather than greying
        // out. Position 1 is directly under the header and ABOVE the header's
        // separator, so the money line reads as part of the license block and
        // not as the first of the dictation controls.
        if let (Some(menu), Some(item)) = (tray_menu.as_ref(), upgrade_item.as_ref()) {
            if offers_purchase && !upgrade_shown {
                let _ = menu.insert(item, 1);
            } else if !offers_purchase && upgrade_shown {
                let _ = menu.remove(item);
            }
        }
        if let Some(tray) = tray {
            // YV43: the idle tooltip must not keep saying "hold fn" while
            // Secure Input has the tap blind — that is the exact instruction
            // that silently does nothing. Live states still win: they are
            // already-running takes, not an invitation to press the key.
            let tip = if recording && hands_free {
                "Yap — hands-free recording"
            } else if recording {
                "Yap — recording"
            } else if busy {
                "Yap — transcribing…"
            } else if secure_input_blocked {
                secure_input::BLOCKED_MESSAGE
            } else {
                "Yap — hold fn to dictate"
            };
            let _ = tray.set_tooltip(Some(tip));
            // Y2-E — the icon wears the urgent treatment on the last day and
            // past the trial, AND ONLY THEN (`license::tray_line`'s second
            // return value). A permanently decorated menu-bar icon is noise, so
            // this is an edge, not a state: nothing is re-set while the answer
            // has not changed.
            //
            // The urgent art is COLOURED, so macOS must not tint it — hence
            // `set_icon_as_template(false)` for it and `true` for the idle
            // silhouette. Same artwork, same 44x44 alpha mask, one recolour.
            if urgent != was_urgent {
                let bytes: &[u8] = if urgent {
                    include_bytes!("../icons/tray-urgent.png")
                } else {
                    include_bytes!("../icons/tray-template.png")
                };
                match tauri::image::Image::from_bytes(bytes) {
                    Ok(img) => {
                        let _ = tray.set_icon(Some(img));
                        let _ = tray.set_icon_as_template(!urgent);
                    }
                    Err(e) => log::warn!(
                        "tray icon ({}): {e}",
                        if urgent { "urgent" } else { "idle" }
                    ),
                }
            }
        }
    });
}

/// Y2-E — the ONE place the menu bar opens the upgrade path.
///
/// It deliberately does NOT launch the browser. `open_purchase_page` hands
/// `license::PAYMENT_LINK_URL` straight to `open(1)`, and a menu item that
/// silently throws up a Stripe tab is a worse surprise than one that shows you
/// the page carrying the price and the Buy button first. So this lands on
/// Settings -> License, which is exactly where the pill's own purchase action
/// lands (`ClassicPill.tsx` / `YappyPill.tsx` emit the same `settings-tab`).
///
/// Both events already have listeners in `desktop/src/appShell.ts` (`navigate`
/// and `settings-tab`) — this adds no new frontend surface, and when Y2-D's
/// dedicated purchase sheet lands there is a single function body to repoint.
fn reveal_purchase_prompt(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
    let _ = app.emit("navigate", "settings");
    let _ = app.emit("settings-tab", "license");
}

fn notify(app: &AppHandle, title: &str, body: impl Into<String>) {
    let _ = app
        .notification()
        .builder()
        .title(title)
        .body(body.into())
        .show();
}

/// Mute the Mac's whole system output for the duration of a take (YV28), gated
/// by the `mute_while_dictating` setting. Snapshots the prior mute + volume into
/// `state.saved_audio` so `restore_system_output` can put it back verbatim.
fn mute_system_output(state: &AppState) {
    if !state.settings.lock().mute_while_dictating {
        return;
    }
    // YV91 finding #9: a dictation taken WHILE a meeting is recording must not
    // mute the call the user is listening to. The context decides, not the
    // caller — see `meeting::auto_mute_allowed`.
    let context = meeting::take_context();
    if let Some(saved) = sysaudio::mute_for_take(context, &sysaudio::SystemOutput) {
        *state.saved_audio.lock() = Some(saved);
        log::info!("system output muted for dictation");
    }
}

/// Restore the system output to its pre-dictation state (YV28). Unconditional
/// by design — it always puts back whatever `mute_system_output` saved,
/// regardless of the current setting, so the Mac is NEVER left muted (covers
/// stop, cancel, every error path, and app exit). No-op if nothing was muted.
fn restore_system_output(state: &AppState) {
    let saved = state.saved_audio.lock().take();
    if let Some(saved) = saved {
        sysaudio::restore(saved);
        log::info!("system output restored after dictation");
    }
}

/// How long the exit teardown waits for an in-flight transcription to hand the
/// ASR engine back (YV70) before quitting with it still live.
const EXIT_DRAIN_TIMEOUT: Duration = Duration::from_millis(900);
/// How long the exit teardown then waits for that take's worker to land its
/// result — the transcript row, or the recoverable failed-dictation row (YV70).
const EXIT_TAKE_TIMEOUT: Duration = Duration::from_millis(600);
/// Poll interval for both waits above.
const EXIT_WAIT_POLL: Duration = Duration::from_millis(10);

/// Everything the app must do on its way out, in the ONE order that is safe.
/// Returns the steps it ran, newest last, so the caller can log the shutdown
/// it actually got (and so a test can assert the order).
///
/// YV69 — the order IS the fix. Yap SIGABRTed on a normal Cmd-Q whenever a
/// model was still resident: `-[NSApplication terminate:]` → `exit()` →
/// `__cxa_finalize_ranges` ran ggml's C++ STATIC destructor, which tried to
/// free the Metal device (`ggml_metal_rsets_free`) after the ObjC/Metal
/// environment had already begun unwinding, and ggml_abort()ed. Unloading the
/// engine FIRST frees that device while the Tauri/ObjC runtime is still alive,
/// so the static destructor has nothing left to tear down. The two pre-existing
/// steps keep their relative order behind it.
///
/// YV70 — that unload frees only what is IN the slot, so it did nothing during
/// a take (the engine is leased out) and the crash survived for exactly that
/// case. The first step now DRAINS the lease, and a second step waits for the
/// drained take to land, both bounded — see `teardown_for_exit_with`.
#[cfg(test)]
fn teardown_for_exit(state: &Arc<AppState>) -> Vec<&'static str> {
    teardown_for_exit_with(state, EXIT_DRAIN_TIMEOUT, EXIT_TAKE_TIMEOUT)
}

/// The teardown with its two waits as parameters — the seam the YV70 tests use
/// so a stuck-lease case is exercised in milliseconds instead of seconds.
#[cfg(test)]
fn teardown_for_exit_with(
    state: &Arc<AppState>,
    drain: Duration,
    take: Duration,
) -> Vec<&'static str> {
    let mut steps = Vec::new();
    exit_step_asr_drain(state, drain, &mut steps);
    exit_step_await_take(state, take, &mut steps);
    exit_step_restore_output(state, &mut steps);
    exit_step_db_checkpoint(state, &mut steps);
    steps
}

/// Step 1 — release the ASR Metal device while ObjC is still standing.
///
/// YV70: `unload()` alone frees NOTHING during a take — the engine is leased
/// out to the transcription thread and the slot is empty, so the device
/// survived into `exit()` and ggml's static destructor aborted on it exactly as
/// before YV69. The drain cancels the in-flight decode and waits (bounded) for
/// the lease to come home first.
fn exit_step_asr_drain(state: &AppState, drain: Duration, steps: &mut Vec<&'static str>) {
    // yap-meetings-fix — a meeting job stands down at its next window instead
    // of re-loading the engine this drain is about to take away.
    meeting_pipeline::request_quit();
    match state.transcription.drain_and_unload(drain) {
        transcription::DrainOutcome::Idle => {}
        transcription::DrainOutcome::Drained => steps.push("asr_drain"),
        transcription::DrainOutcome::TimedOut => steps.push("asr_drain_timeout"),
    }
    steps.push("asr_unload");
}

/// Step 2 — YV70: the drained take is still mid-flight in its worker thread,
/// between the decode and the row it writes. Give it a bounded moment to land,
/// so quitting can't make a take the user already spoke vanish.
fn exit_step_await_take(state: &AppState, take: Duration, steps: &mut Vec<&'static str>) {
    if *state.busy.lock() {
        if await_in_flight_take(state, take) {
            steps.push("await_take");
        } else {
            log::warn!(
                "exit: dictation still in flight after {}ms — quitting anyway",
                take.as_millis()
            );
            steps.push("await_take_timeout");
        }
    }
}

/// Step 3 — YV28 safety net: never leave the Mac muted if we exit mid-take.
fn exit_step_restore_output(state: &AppState, steps: &mut Vec<&'static str>) {
    restore_system_output(state);
    steps.push("restore_output");
}

/// Step 4 — checkpoint the WAL so it never grows unbounded and the .db isn't
/// left a deceptive 4 KB stub. Last, so every row an earlier step wrote is in it.
fn exit_step_db_checkpoint(state: &AppState, steps: &mut Vec<&'static str>) {
    state.db.checkpoint();
    steps.push("db_checkpoint");
}

/// yap-lifecycle — THE quit. Every quit path (Cmd-Q from the app menu, the
/// Dock's Quit, the tray's "Quit Yap" via `app.exit(0)`) ends in
/// `RunEvent::Exit`, and that arm calls this and nothing else. The order is
/// `lifecycle::quit_plan()`: the pill is ordered out FIRST (inline, on the
/// main thread the Exit arm runs on), then the meeting stop, the ASR drain, the
/// in-flight take, the output restore and the WAL checkpoint — each on a worker
/// thread with its own deadline inside `lifecycle::QUIT_BUDGET`. A step that
/// misses its deadline is logged and skipped; the process exits regardless.
/// The meeting finalize is the exception: its cost grows with the meeting, so
/// it runs to completion under its own `lifecycle::MEETING_FINALIZE_CEILING`,
/// outside the budget (a 700 ms share cut a 2-hour meeting off mid-spill).
fn quit_teardown(app: &AppHandle, state: &Arc<AppState>) -> lifecycle::TeardownReport {
    use lifecycle::QuitStep;
    let steps = lifecycle::quit_plan()
        .into_iter()
        .map(|step| {
            let run: Box<dyn FnOnce() + Send> = match step {
                QuitStep::PillOrderOut => {
                    let app = app.clone();
                    Box::new(move || float_pill::hide_float(&app))
                }
                QuitStep::MeetingStop => {
                    let state = state.clone();
                    Box::new(move || {
                        // YV95 — a live meeting is finalized, not abandoned.
                        if let Some(c) = state.meeting() {
                            c.stop_if_running("app quit");
                        }
                    })
                }
                QuitStep::AsrDrain => {
                    let state = state.clone();
                    Box::new(move || {
                        exit_step_asr_drain(&state, EXIT_DRAIN_TIMEOUT, &mut Vec::new())
                    })
                }
                QuitStep::AwaitTake => {
                    let state = state.clone();
                    Box::new(move || {
                        exit_step_await_take(&state, EXIT_TAKE_TIMEOUT, &mut Vec::new())
                    })
                }
                QuitStep::RestoreOutput => {
                    let state = state.clone();
                    Box::new(move || exit_step_restore_output(&state, &mut Vec::new()))
                }
                QuitStep::DbCheckpoint => {
                    let state = state.clone();
                    Box::new(move || exit_step_db_checkpoint(&state, &mut Vec::new()))
                }
            };
            // Deadline, thread and budget class all come from the plan: the
            // meeting finalize is budget-exempt (MEETING_FINALIZE_CEILING), so
            // a long meeting is finalized, never cut off mid-spill.
            step.bounded(run)
        })
        .collect();
    lifecycle::run_bounded(steps, lifecycle::QUIT_BUDGET)
}

/// Wait (bounded) for the dictation worker to finish the take it is on. `busy`
/// is cleared at the very end of that thread — after the transcript insert, or
/// after `keep_failed_take` has written the recoverable row — so this returning
/// `true` means the take is durable. `false` = the budget ran out.
fn await_in_flight_take(state: &AppState, wait: Duration) -> bool {
    let deadline = std::time::Instant::now() + wait;
    while *state.busy.lock() {
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(EXIT_WAIT_POLL);
    }
    true
}

/// Mic-level HUD cadence: the float pill's waveform redraws at 20 frames per
/// second, so the level thread emits one `audio_level` per frame. This is a
/// render rate, not a wait on anything — the take's end arrives as a signal
/// (see `record::StopSignal`), never by re-reading state on a timer.
const HUD_FPS: u64 = 20;
const HUD_FRAME: std::time::Duration = std::time::Duration::from_millis(1_000 / HUD_FPS);

/// YV80 — what LAUNCH does with the ASR engine, in one testable place.
///
/// YV38 loaded the GGUF at startup so the user's first take wouldn't pay for
/// it. That bought a few seconds once and cost ~930 MB of resident memory for
/// the whole session (the `memory rss_mb=` line measured 929 MB on an idle Yap
/// that had never dictated) — a bad trade for a menu-bar app that spends most
/// of its life waiting for a key. The default is now lazy: launch loads
/// NOTHING, and the first take's arm brings the engine up while the user is
/// already talking (`arm_asr_engine`). `preload_model` restores the eager path
/// for anyone who would rather hold the memory than wait once.
///
/// The Silero VAD above is deliberately NOT part of this: it is a few MB, it
/// runs before ASR on every take, and loading it late would cost latency on the
/// hot path for no memory worth having.
///
/// Returns whether a load was attempted, so a caller (and the tests) can see
/// which path launch took.
fn preload_engine_at_startup(
    manager: &transcription::TranscriptionManager,
    preload_model: bool,
    model: Option<(String, PathBuf)>,
) -> bool {
    if !preload_model {
        log::info!("startup: ASR engine load deferred to the first dictation (YV80 lazy default)");
        return false;
    }
    let Some((model_id, model_path)) = model else {
        log::info!("startup: no downloaded ASR model — engine preload skipped");
        return false;
    };
    match manager.load(&model_id, &model_path) {
        Ok(()) => {
            log::info!("startup: ASR engine preloaded ({model_id})");
            true
        }
        Err(e) => {
            log::warn!("startup: ASR engine preload failed ({e}) — the take path retries");
            false
        }
    }
}

/// YV80 — bring the ASR engine up for the take that is starting, if nothing is
/// resident yet.
///
/// This is where the model load lives now that launch no longer pays for it
/// (see `preload_engine_at_startup`). It is deliberately the ARM and not the
/// transcribe: the load then overlaps the seconds the user spends talking, so
/// on a normal take the engine is warm by the time there is anything to decode
/// and the one-time cost is mostly hidden.
///
/// Three things it must never do, all of them satisfied by handing the work to
/// the blocking pool AFTER capture is already running:
/// * delay or interrupt capture — the recorder (and its YV63 crash journal) is
///   live before this is called, so audio is being written while the model
///   loads, and every never-lose-audio guarantee is untouched;
/// * block the caller — this returns immediately, the load is `spawn_blocking`
///   exactly like YV38's startup preload was;
/// * duplicate work — `TranscriptionManager::load` is idempotent AND coalesced
///   (YV80), so the transcribe that follows waits for this load rather than
///   starting a second one, and a warm take costs two cheap reads.
///
/// A failure is only logged: `transcribe_native` calls `load` again and its
/// error is the one the user sees, on the path that already knows how to
/// report one.
fn arm_asr_engine(app: &AppHandle, state: &Arc<AppState>) {
    if state.transcription.is_loaded() || state.transcription.is_loading() {
        return; // warm (or already coming up) — nothing to do
    }
    let native = state.settings.lock().native_model.clone();
    let Some((model_id, model_path)) = native_model_ready(&native) else {
        return; // no model downloaded — the take path raises that, not this
    };
    let app = app.clone();
    let state = state.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let started = std::time::Instant::now();
        let outcome = state.transcription.load(&model_id, &model_path);
        let load_ms = started.elapsed().as_millis();
        match outcome {
            Ok(()) => log::info!("take arm: ASR engine ready ({model_id}) in {load_ms}ms"),
            Err(e) => log::warn!("take arm: ASR engine load failed ({e}) — the take path retries"),
        }
        // A take that outlasted the load is sitting on "Preparing your speech
        // engine…" (`build_status` reads the loading flag live) — this is the
        // emit that moves it on to the decode.
        emit_status(&app, &state);
    });
}

/// YP2 — the ONE gate. Every way to begin a new dictation (hotkey, hands-free,
/// tray, pill, the Home button, onboarding calibration) funnels into
/// `start_recording`, so this is the only place it has to live.
///
/// What it does NOT touch, on purpose and forever: history, search, export,
/// settings, the dictionary, snippets, scratchpad, model management, crash
/// reports, permissions. The trial ending stops Yap from taking NEW words; it
/// never takes back the ones already spoken. `tests/license_gate.rs` reads this
/// file and fails if the check ever appears anywhere else.
///
/// Returns true when dictation may proceed.
fn license_allows_new_dictation(app: &AppHandle, state: &AppState) -> bool {
    let status = state.license.status();
    if status.allows_new_dictation() {
        return true;
    }
    // Gentle: a sentence that says what still works and what turns it back on,
    // throttled so leaning on the hotkey is not a notification storm. The event
    // lets an on-screen surface say the same thing without a system alert.
    let _ = app.emit("license_required", &status);
    if state.license.should_announce_gate() {
        notify(app, "Yap", license::LICENSE_REQUIRED_MESSAGE);
    }
    log::info!("license: new dictation declined — trial over, no valid license");
    false
}

/// PERM-C — the mic gate's own notification throttle, the same shape as
/// `LicenseManager::should_announce_gate` (license.rs, 15s): a held or
/// repeatedly-tapped hotkey must refuse every press, but it must not become a
/// notification storm while it does.
const MIC_GATE_NOTICE_INTERVAL_SECS: u64 = 15;
static MIC_GATE_NOTICE: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
/// One `requestAccessForMediaType:` per NotDetermined session. macOS shows the
/// dialog exactly once and answers every later call from the stored decision,
/// so re-asking on every press is a no-op that makes the hotkey feel dead.
static MIC_ACCESS_REQUESTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn should_announce_mic_gate() -> bool {
    let Ok(mut last) = MIC_GATE_NOTICE.lock() else {
        return true;
    };
    let now = std::time::Instant::now();
    let quiet = last
        .map(|t| now.duration_since(t).as_secs() < MIC_GATE_NOTICE_INTERVAL_SECS)
        .unwrap_or(false);
    if quiet {
        return false;
    }
    *last = Some(now);
    true
}

/// What the pill is told when a press is refused for the microphone.
/// `prompting` is true only for the NotDetermined case, where the system dialog
/// is on its way and the honest pill state is "waiting", not "blocked".
#[derive(Clone, serde::Serialize)]
struct MicGateNotice {
    status: String,
    prompting: bool,
}

const MIC_REQUIRED_MESSAGE: &str =
    "Yap cannot hear you — macOS is blocking the microphone. Open Yap → Permissions to turn it \
     back on.";

/// PERM-C — the SECOND gate on the one dictation-start path, and the FIRST one
/// asked. Modelled on `license_allows_new_dictation` directly above: refuse a
/// new take, say why once, never touch a byte of existing data.
///
/// Why it exists: opening the capture stream is not a permission check. A denied
/// stream opens and delivers SILENCE, so before this gate the hotkey recorded a
/// perfect, empty take and the pill showed a normal, cheerful recording. The
/// only question macOS actually answers is
/// `+[AVCaptureDevice authorizationStatusForMediaType:]`, which is a cheap,
/// prompt-free read (`mic_auth::authorization_status`) — cheap enough to ask on
/// EVERY press, which is the point: a grant revoked in System Settings mid-session
/// is seen by the very next press.
///
/// Nothing here waits. This runs on the AppKit main thread, so the NotDetermined
/// branch fires the non-blocking `request_access` and RETURNS false; the answer
/// arrives as `microphone-status` and the pill either clears or paints blocked.
/// Blocking for the length of a human decision on the TCC dialog stops the run
/// loop — see the PERM-A notes in `mic_auth`.
///
/// Order matters: this is asked BEFORE the license. "Yap cannot hear you" is the
/// truer message than "buy a license", and a user with no mic grant must never be
/// shown a purchase prompt. `tests/mic_gate.rs` asserts that order in the source.
fn microphone_allows_new_dictation(app: &AppHandle, _state: &AppState) -> bool {
    let status = mic_auth::authorization_status();
    match mic_auth::gate_decision(status) {
        mic_auth::MicGate::Proceed => true,
        mic_auth::MicGate::Prompt => {
            let _ = app.emit(
                "mic_permission_required",
                MicGateNotice {
                    status: status.as_str().to_string(),
                    prompting: true,
                },
            );
            if !MIC_ACCESS_REQUESTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                let app_answer = app.clone();
                mic_auth::request_access(move |answered| {
                    log::info!("mic gate: TCC answered {}", answered.as_str());
                    // The same event the Permissions pane already listens to, so
                    // one reader owns the status everywhere in the app.
                    let _ = app_answer.emit("microphone-status", answered.as_str());
                });
            }
            log::info!(
                "mic gate: new dictation declined — microphone grant not determined; \
                 asked macOS once, this press does not wait for the answer"
            );
            false
        }
        mic_auth::MicGate::Refuse => {
            let _ = app.emit(
                "mic_permission_required",
                MicGateNotice {
                    status: status.as_str().to_string(),
                    prompting: false,
                },
            );
            if should_announce_mic_gate() {
                notify(app, "Yap", MIC_REQUIRED_MESSAGE);
            }
            log::info!(
                "mic gate: new dictation declined — microphone {} (macOS will not re-prompt)",
                status.as_str()
            );
            false
        }
    }
}

fn start_recording(app: &AppHandle, state: &Arc<AppState>) {
    if *state.recording.lock() || *state.busy.lock() {
        return;
    }
    // PERM-C — the microphone is asked FIRST. A refused press must produce a
    // visible pill state instead of a silent take, and a user with no mic grant
    // must never be shown a purchase prompt. Neither gate waits on anything.
    if !microphone_allows_new_dictation(app, state) {
        return;
    }
    if !license_allows_new_dictation(app, state) {
        return;
    }
    // Y3-D: a take ARMS with a clear cancel flag, so a cancel that arrived
    // during the previous take can never kill this one. Cleared here rather
    // than in the worker: the worker starts after the hold ends, and a cancel
    // fired in the gap between press and release must still be honoured.
    state.take_cancelled.store(false, Ordering::SeqCst);
    // ...and so does the manager-side latch that Y3-B's chunk loop reads, so
    // the two are armed together and never drift apart.
    state.transcription.begin_user_take();
    register_cancel_shortcut(app);
    let denoise = state.settings.lock().denoise;
    // YV35: anchor the press→capture_start span on the physical key-down when
    // this take came from the PTT hold (None for tray/button/hands-free starts).
    let pressed_at = ptt_macos::press_started_at();
    // YV63: the take's crash journal spills into the SAME recovery dir a failed
    // take is parked in, so a recovered clip lands inside the retry + 7-day
    // purge lifecycle that already exists.
    match record::start_recording(
        data_dir().join("recordings"),
        &recovery_dir(),
        denoise,
        pressed_at,
    ) {
        Ok(active) => {
            let level = active.level.clone();
            let stop = active.stop_signal();
            // Y3-F — the clock the ceiling watchdog measures against.
            let started = active.started_at();
            *state.recorder.lock() = Some(active);
            // Y3-F: a fresh take starts un-warned, so the ceiling notice belongs
            // to THIS take and never leaks in from the last one.
            *state.session_cap_warned.lock() = false;
            *state.recording.lock() = true;
            // YV95 — the pill's hover watch must keep its fast cadence for a
            // take even when a meeting is recording underneath it.
            float_pill::set_dictating(true);
            // An earlier take's Copy card must not hold the pill (or its
            // hitbox) through this one.
            float_pill::clear_copy_card_for_new_take(app);
            *state.last_error.lock() = None;
            // YV28: silence the whole Mac so nothing plays over the user while
            // they talk. Restored the instant recording stops (see below).
            mute_system_output(state);
            log::info!("recording started");
            // YV80: capture is LIVE at this line, so the (lazy) model load below
            // runs alongside the user's speech and can never cost a sample.
            arm_asr_engine(app, state);
            let _ = app.emit("recording", true);
            emit_status(app, state);
            // Wispr-style: glass island appears for the hold
            float_pill::show_for_recording(app);
            // Stream mic levels to the float HUD at HUD_FPS. YV38: the thread
            // parks on the take's stop signal between frames instead of sleeping
            // a fixed slice and re-reading a flag, so key-release ends the meter
            // immediately (it used to keep drawing for up to one frame after the
            // hold, and the zeroing frame landed that late too).
            let app_lv = app.clone();
            // Y3-F: the ceiling watchdog rides this thread rather than getting one
            // of its own. It already ticks at HUD cadence, it already holds the
            // take's stop signal, and it already dies with the take — so the
            // ceiling costs no thread, and a stop lands within one HUD frame of
            // `record::MAX_SESSION` instead of within a 60 s watchdog interval.
            let state_lv = state.clone();
            std::thread::spawn(move || {
                use std::sync::atomic::Ordering;
                loop {
                    let v = level.load(Ordering::Relaxed) as f64 / 1000.0;
                    let _ = app_lv.emit("audio_level", v);
                    let warned = *state_lv.session_cap_warned.lock();
                    match record::session_cap_tick(started.elapsed(), warned) {
                        record::SessionCapAction::Continue => {}
                        record::SessionCapAction::Warn => {
                            *state_lv.session_cap_warned.lock() = true;
                            log::info!(
                                "session ceiling: {}s in, warning once ({}s ceiling)",
                                record::SESSION_WARN_AT.as_secs(),
                                record::MAX_SESSION_SECONDS
                            );
                            emit_status(&app_lv, &state_lv);
                        }
                        record::SessionCapAction::Stop => {
                            log::info!(
                                "session ceiling: {}s reached — stopping cleanly and transcribing",
                                record::MAX_SESSION_SECONDS
                            );
                            // The SAME path the hotkey release takes: the take is
                            // finalized, cut at a VAD boundary if one is inside the
                            // grace window, and transcribed. It is never discarded
                            // and never surfaced as an error.
                            stop_and_transcribe(app_lv.clone(), state_lv.clone());
                            break;
                        }
                    }
                    if stop.wait_stopped(HUD_FRAME) {
                        break;
                    }
                }
                let _ = app_lv.emit("audio_level", 0.0);
            });
        }
        Err(e) => {
            log::error!("record start failed: {e}");
            *state.last_error.lock() = Some(e.clone());
            emit_status(app, state);
            notify(app, "Yap — Mic", e);
        }
    }
}

/// Y3-D — take Escape off the system for the duration of ONE take.
///
/// See [`shortcuts::CANCEL`] for why this is not registered at launch like the
/// other four: a bare Escape held for the life of the process would swallow
/// Escape in every other app. Registration failures are logged and nothing else
/// — the tray's Cancel item and the pill's own button still work, so a Mac where
/// something else owns Escape loses the shortcut, not the feature.
///
/// Idempotent: `register` on an already-registered shortcut is an error the
/// plugin reports, and a re-arm inside one take (hands-free re-latch) must not
/// look like a failure, so the already-registered case is logged at debug.
fn register_cancel_shortcut(app: &AppHandle) {
    // Y0-D: a `--smoke` launch registers NOTHING system-wide.
    if !smoke::global_hotkeys_allowed() {
        return;
    }
    // fix-249 advisory (c): re-arm the key this take already holds, if any.
    let sc = hotkey_table::cancel_to_register();
    let cancel_label = format!("{sc:?}");
    if app.global_shortcut().is_registered(sc) {
        log::debug!("{} cancel already registered", cancel_label);
        hotkey_table::note_cancel_registered(sc);
        return;
    }
    match app.global_shortcut().register(sc) {
        Ok(()) => {
            hotkey_table::note_cancel_registered(sc);
            log::info!("{} cancel registered for this take", cancel_label)
        }
        Err(e) => log::warn!("{} register failed: {e}", cancel_label),
    }
}

/// Give Escape back to the rest of the Mac. Called on EVERY exit of a take —
/// the normal end, the failure arms, and the cancel itself — because the one
/// path that forgets is the one that leaves Escape broken system-wide until Yap
/// quits.
fn unregister_cancel_shortcut(app: &AppHandle) {
    if !smoke::global_hotkeys_allowed() {
        return;
    }
    // fix-249 advisory (c): drop the key the take ARMED, not whatever the
    // setting says now — a mid-take rebind must not leave Esc registered.
    let sc = hotkey_table::cancel_to_release();
    let cancel_label = format!("{sc:?}");
    if !app.global_shortcut().is_registered(sc) {
        return;
    }
    match app.global_shortcut().unregister(sc) {
        Ok(()) => log::info!("{} cancel released", cancel_label),
        Err(e) => log::warn!("{} unregister failed: {e}", cancel_label),
    }
}

/// Y3-D — what a cancelled take writes on its recovery row.
///
/// A distinct reason from YV52's failures and YV67's rejections, because it is
/// a distinct thing: nothing went wrong. History shows it as a take the user
/// stopped, and Retry on it is "actually, transcribe that after all" rather
/// than "try again, it may work this time".
pub const CANCELLED_TAKE_REASON: &str = "cancelled";

/// The message the cancelled take's terminal event carries. Not an error: the
/// pill settles to idle on it rather than painting a failure.
pub const CANCELLED_TAKE_MESSAGE: &str = "Cancelled";

/// Y3-D — the take-level cancellation checkpoint.
///
/// Called at every stage boundary of the transcribe worker (capture handoff,
/// pre-decode, post-decode, immediately before paste) and, in a chunked decode,
/// once per chunk boundary. Cooperative on purpose: the alternative is killing
/// the engine, and the engine is in-process and SHARED with meetings, so
/// killing it to end one dictation would take a running meeting down with it.
fn take_is_cancelled(state: &AppState) -> bool {
    state.take_cancelled.load(Ordering::SeqCst)
}

/// Y3-D — park a cancelled take's audio under the recovery lifecycle.
///
/// The same move + row that YV52 gives a FAILED take, which is the point: a
/// cancel must never be the thing that destroys fifteen minutes of audio. It
/// lands in `recovery_dir()` and therefore inherits YV52/YV63's 7-day purge and
/// shows up in History with a Retry, so "actually, transcribe that" survives a
/// mis-pressed Escape. `dir` is a parameter so the whole path is drivable
/// against a temp dir from a test rather than the user's real recovery folder.
///
/// Best-effort like `keep_failed_take`: if the clip cannot be preserved the user
/// is exactly where a pre-Y3-D cancel left them, never worse, and never with a
/// row pointing at audio that is not there.
pub fn keep_cancelled_take(
    db: &Database,
    dir: &Path,
    clip: &mut record::ClipWav,
    speech_seconds: f64,
    source_app: Option<String>,
) -> Option<FailedDictation> {
    let path = match clip.keep_for_recovery(dir) {
        Ok(path) => path,
        Err(e) => {
            log::warn!("cancelled take not recoverable (clip could not be kept): {e}");
            return None;
        }
    };
    match db.record_failed_dictation(&path, speech_seconds, CANCELLED_TAKE_REASON, source_app) {
        Ok(row) => {
            log::info!(
                "Y3-D: cancelled take {} ({:.2}s) parked for retry",
                row.id,
                speech_seconds
            );
            Some(row)
        }
        Err(e) => {
            // No row means nothing can ever reach this wav — don't leak audio.
            log::warn!("cancelled-take row not written ({e}) — removing kept clip");
            let _ = std::fs::remove_file(&path);
            None
        }
    }
}

/// Discard in-flight take (FN interrupt / cancel) — no ASR.
fn cancel_recording(app: &AppHandle, state: &AppState) {
    // YV39: bump BEFORE the recording check. A cancel that arrives while a
    // previous take is still transcribing finds `recording` already false and
    // returns below — but that take must still be invalidated, or its ⌘V lands
    // seconds after the user cancelled.
    state.paste_generation.fetch_add(1, Ordering::SeqCst);
    if !*state.recording.lock() {
        return;
    }
    *state.recording.lock() = false;
    float_pill::set_dictating(false);
    *state.hands_free.lock() = false;
    // YV49: a cancelled command press must not leave its selection armed for
    // the next, ordinary take.
    *state.command_selection.lock() = None;
    // YV28: un-mute the Mac the moment the take ends (cancel path).
    restore_system_output(state);
    let _ = app.emit("recording", false);
    let mut recoverable = None;
    if let Some(active) = state.recorder.lock().take() {
        // Discarded take — no ASR, so skip the Silero isolation pass entirely.
        //
        // Y3-D: the clip is PARKED, not unlinked. Before this, the returned clip
        // guard dropped here and took the only copy of the audio with it, so a
        // mis-pressed cancel at minute fifteen of a take was fifteen minutes
        // gone with nothing in History to show for it. A take too short to have
        // a wav at all still comes back `Err` from `stop_recording`, and there
        // is correspondingly nothing to keep.
        match record::stop_recording(active, None) {
            Ok(mut rec) => {
                recoverable = keep_cancelled_take(
                    &state.db,
                    &recovery_dir(),
                    &mut rec.clip,
                    rec.speech_seconds,
                    None,
                );
            }
            Err(e) => log::info!("cancelled take had no clip to keep: {e}"),
        }
    }
    // NOT a hard `last_error`: a cancel is something the user did on purpose, so
    // the pill settles to idle rather than painting a failure (the item's "the
    // pill shows a cancelled state that settles to idle — not an error").
    *state.last_error.lock() = None;
    let _ = app.emit(
        TAKE_CANCELLED_EVENT,
        serde_json::json!({ "message": CANCELLED_TAKE_MESSAGE, "failed": recoverable }),
    );
    // The terminal marker every waiting view already listens for (YV79), so a
    // spinner clears on the cancel instead of running to its watchdog.
    let _ = app.emit(
        TAKE_DONE_EVENT,
        serde_json::json!({ "ok": false, "message": CANCELLED_TAKE_MESSAGE }),
    );
    emit_status(app, state);
    float_pill::after_recording(app, state.settings.lock().show_floating_pill);
    // The bare literal is load-bearing and must stay intact: `vocab`'s
    // redaction corpus is LITERALS ONLY, so a word survives a packed support
    // log only if Yap compiled that exact literal into itself. Folding the new
    // clip-kept detail into this string dropped "cancelled" from the corpus and
    // redacted it out of every shipped support bundle —
    // `tests/support_bundle_contents.rs:156` caught it. The detail gets its own
    // line instead.
    log::info!("recording cancelled");
    log::info!("cancelled take clip kept: {}", recoverable.is_some());
}

/// Y3-D — cancel the take that is running RIGHT NOW, at whatever stage it is at.
///
/// `cancel_recording` only ever answered half the question. It stops a
/// RECORDING; the moment the hold ended and decode began there was nothing left
/// to press, so a fifteen-minute take the user regretted the instant they let go
/// occupied the engine to completion. This is the other half, and it is one
/// entry point on purpose: the user pressing Escape does not know or care which
/// stage the take is in, and a UI that offers cancel only during the hold is the
/// bug.
///
/// Three things happen, in this order:
///
/// 1. The cooperative flag goes up. Every stage boundary of the worker reads it
///    (see [`take_is_cancelled`]), so a take between stages stops without the
///    engine being involved at all.
/// 2. If a take is still recording, [`cancel_recording`] ends it and PARKS its
///    clip.
/// 3. If a decode is in flight, the engine's own cancel hook is fired with
///    [`transcription::CANCELLED_BY_USER`], so the decode returns at the next
///    point it checks rather than running to completion.
///
/// What deliberately does NOT happen: the engine is not killed and not
/// unloaded. It is in-process and shared with meeting ASR, so tearing it down
/// to end one dictation would take a running meeting down with it.
///
/// Returns true when something was actually cancelled, so the caller can tell a
/// real cancel from an Escape pressed at an idle pill.
#[tauri::command]
fn cancel_transcription(app: AppHandle, state: State<'_, Arc<AppState>>) -> bool {
    cancel_take(&app, &state)
}

/// The body of [`cancel_transcription`], callable without a `State` extractor.
///
/// The Escape hotkey fires on the global-shortcut handler thread, which has no
/// `State<'_, _>` to hand it, and the tray item is in the same position — so the
/// command is a one-line wrapper over this and there is exactly ONE cancel
/// implementation rather than one per entry point.
fn cancel_take(app: &AppHandle, state: &Arc<AppState>) -> bool {
    let was_recording = *state.recording.lock();
    let was_busy = *state.busy.lock();
    if !was_recording && !was_busy {
        log::debug!("cancel requested with no take in flight — ignored");
        unregister_cancel_shortcut(app);
        return false;
    }
    state.take_cancelled.store(true, Ordering::SeqCst);
    if was_recording {
        cancel_recording(app, state);
    }
    // Fired even when the take was still recording: the hold may have ended
    // between the two lines above, and cancelling nothing is a no-op that
    // returns false, while missing the decode is a fifteen-minute wait.
    let stopped_decode = state.transcription.cancel_in_flight_for_user();
    unregister_cancel_shortcut(app);
    log::info!(
        "Y3-D cancel: recording={was_recording} busy={was_busy} decode_cancelled={stopped_decode}"
    );
    true
}

/// Panic-safety net for the transcribe worker (audit finding [8]). `busy` is set
/// true before the worker thread spawns and reset at the end of its normal path;
/// if the worker *panics* before that reset, `busy` stays true forever and
/// dictation is permanently soft-locked until restart. This guard force-resets
/// `busy`/`hands_free` on unwind. It is disarmed on the normal path, so it only
/// fires on an actual panic.
struct WorkerBusyGuard {
    state: Arc<AppState>,
    app: AppHandle,
    armed: bool,
}
impl Drop for WorkerBusyGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        *self.state.busy.lock() = false;
        *self.state.hands_free.lock() = false;
        log::error!(
            "transcribe worker unwound before cleanup — force-reset busy/hands_free (was soft-lock)"
        );
        emit_status(&self.app, &self.state);
    }
}

/// Emitted whenever a take fails to produce a transcript (YV32). Before this the
/// error path only logged + notified, so the UI had NOTHING to listen to: the
/// onboarding overlay clears its "Transcribing…" state on `transcript` alone and
/// spun forever on any ASR failure. Payload: `{ message }`.
pub const TRANSCRIPT_ERROR_EVENT: &str = "transcript_error";

/// Transient toast channel (the `.toast` flash in App.tsx). Carries the paste
/// result line and, since YV49, the command-mode outcomes — including the
/// "Didn't catch a command" rejection that leaves the selection untouched.
pub const PASTE_OUTCOME_EVENT: &str = "paste_outcome";

/// YV79 — "this take is over", whatever the outcome. Emitted LAST by every
/// terminal arm of the take-result match, after that arm's own `transcript` /
/// `transcript_error` payload has landed, so a view waiting on a take clears
/// its spinner on ONE event instead of having to subscribe to every outcome
/// channel. The gap this closes: a SOFT outcome (the no-speech gate a silent
/// mic hits — no permission, wrong input device, output-only headset) only
/// ever reached [`PASTE_OUTCOME_EVENT`], the toast channel the onboarding
/// overlay sits on top of and never hears, so first-run calibration spun on
/// "Transcribing…" until its watchdog. Payload: `{ ok, message }`, where
/// `message` is `null` on the success path and the reason otherwise.
pub const TAKE_DONE_EVENT: &str = "take_done";

/// YV74 — an auto-paste take that produced NO read receipt, so the transcript
/// never reached the target app. Payload: the transcript row's id, which the
/// toast turns into a "Copy again" action (the transcript may not be on the
/// clipboard any more if the user copied something themselves while we waited).
/// Emitted only alongside a `paste_outcome` that already says what went wrong.
pub const PASTE_FAILED_EVENT: &str = "paste_failed";

/// Y4-G — the LLM polish stage was enabled for this take and produced nothing.
/// The payload is the closed-set REASON tag (`deadline`, `no_model`,
/// `no_sidecar`, `v2_truncated`, …) — never transcript text.
///
/// This exists because both polish failure modes are invisible by
/// construction: a missed deadline falls back to the rules text and a rejected
/// rewrite is discarded, and for any take over ~150 words those are the NORMAL
/// outcomes. Without this a user who installs a 1.12 GB model and picks High
/// gets rules-only output on every real dictation with no indication the model
/// was ever consulted.
pub const POLISH_SKIPPED_EVENT: &str = "polish_skipped";

/// PERM-D — a take that captured DIGITAL SILENCE, and what to do about it.
/// Payload: `{ code, status, needsPermission, message, failed }`. `code` is the
/// machine-readable reason (`SILENT_CAPTURE_CODE` today, the only one), `status`
/// is the microphone grant as re-read at the END of the take, and `failed` is
/// the recovery row for the preserved clip (or null if it could not be kept).
///
/// Why a separate channel from `transcript_error`: this take never reached ASR,
/// so there is no transcription to have failed, and the ACTION differs — a
/// revoked grant needs the permission screen, not a four-second toast.
pub const TAKE_FAILED_EVENT: &str = "take_failed";

/// Y3-D — the take the user cancelled. Distinct from [`TAKE_FAILED_EVENT`] and
/// [`TRANSCRIPT_ERROR_EVENT`] because a cancel is not a failure: the pill
/// settles to idle on it, no red state, no notification. `failed` carries the
/// parked recovery row (or null) so History can offer "transcribe it after all"
/// on exactly this take.
pub const TAKE_CANCELLED_EVENT: &str = "take_cancelled";

/// PERM-D — the `code` on [`TAKE_FAILED_EVENT`] for an all-zero buffer.
pub const SILENT_CAPTURE_CODE: &str = "silent_capture";

/// PERM-D — what the user is told when the buffer is silent and the grant is
/// FINE. The mic is authorized and a device exists, so the remaining suspects
/// are the device selection and its mute switch.
const SILENT_CAPTURE_DEVICE_MESSAGE: &str =
    "That take recorded pure silence — check your input device, it may be muted.";

/// PERM-D — the same take, when the grant is the reason. The permission screen
/// is surfaced alongside this, so the sentence names the cause and not an
/// errand.
const SILENT_CAPTURE_PERMISSION_MESSAGE: &str =
    "That take recorded pure silence — macOS is not letting Yap hear the microphone.";

/// PERM-D — what a silent take writes on its recovery row. `'static`, like
/// every other reason on that row, so no transcript text can reach it.
const SILENT_CAPTURE_REASON: &str = "silent capture: input delivered digital silence";

/// Soft status for a take that produced nothing to paste — a fumbled tap or a
/// rejected hallucination loop. Normal, not an error.
const NO_SPEECH_MESSAGE: &str = "Didn't catch any speech — hold and speak";

/// YV67 — what a hallucination-gate rejection writes on its recovery row, so
/// History says WHY the take is there and Retry re-runs ASR on the same audio.
const GATE_REPETITION_REASON: &str = "gate: possible repetition loop";

/// YV67 — a take the microphone died in the middle of. Errors the take (so the
/// `Err` arm preserves the partial wav) rather than pasting a transcript that is
/// silently cut off wherever the device dropped out.
const DEVICE_FAILED_MESSAGE: &str =
    "Microphone disconnected mid-take — the partial audio was saved, press Retry.";

/// The selected embedded model when it is actually downloaded, as
/// `(catalog id, on-disk path)`. `None` means nothing has been fetched yet, so
/// no take can be transcribed at all (YV34 — there is no sidecar to fall back
/// to any more).
fn native_model_ready(native_model: &str) -> Option<(String, PathBuf)> {
    let model = models::catalog_model(native_model)?;
    if !models::is_downloaded(model) {
        return None;
    }
    models::model_path(model).map(|path| (model.id.clone(), path))
}

/// Transcribe a captured take with the embedded GGUF engine — since YV34 the
/// app's only ASR path: load the model into the warm manager (a no-op when it is
/// already resident) and run it over the take's samples. Both the load and the
/// transcription are already bounded + panic-contained by the manager, so this
/// can fail but can never hang.
///
/// YV37: `samples` are the 16 kHz mono floats the capture path already produced
/// in memory. The clip is no longer written, read back and re-decoded from disk
/// before ASR sees a single sample.
/// How many dictionary terms are considered for the decode bias (YV47). The
/// real ceiling is the prompt's own token window — this only stops us reading
/// the whole table on every take.
const BIAS_TERM_LIMIT: i64 = 64;

/// How many pending dictionary suggestions the UI shows at once (YV47).
const DICT_CANDIDATE_LIMIT: i64 = 12;

/// The error `transcribe_native` returns when a decode comes back blank. Named
/// so the DB-B resume can tell "the remaining audio was silence" apart from a
/// real engine failure without matching a string literal in two places.
const EMPTY_TRANSCRIPT: &str = "Empty transcript";

/// The error for a decode that produced no text. `EMPTY_TRANSCRIPT` means the
/// audio was SILENCE, and DB-B's resume (`finish_recovered_decode`) acts on
/// that: it keeps the recovered words as the whole take and the retry then
/// deletes the clip. A windowed decode that came back blank because windows
/// FAILED (engine error, per-chunk timeout, the take deadline) is not silence —
/// the failed window may hold the rest of what he said — so it gets its own
/// error, which every caller treats as a real failure that keeps the clip.
fn blank_take_error(take: &transcription::DictationTake) -> String {
    if take.chunks_failed == 0 {
        return EMPTY_TRANSCRIPT.into();
    }
    let reason = take
        .degraded_reason
        .as_deref()
        .unwrap_or("transcription failed");
    format!(
        "{} of {} window(s) failed and nothing else was voiced ({reason})",
        take.chunks_failed, take.chunks_total
    )
}

fn transcribe_native(
    manager: &transcription::TranscriptionManager,
    model_id: &str,
    model_path: &Path,
    samples: Vec<f32>,
    language: &str,
    bias_prompt: Option<String>,
    // Y3-C — who hears about each completed decode window. The dictation path
    // passes the pill's reporter; every headless caller passes `NoObserver`.
    observer: &mut dyn transcribe_progress::TakeObserver,
) -> Result<transcription::AsrOutput, String> {
    let started = std::time::Instant::now();
    manager.load(model_id, model_path)?;
    // YV40: load and decode are timed apart — the load is 0 on the warm path and
    // seconds on a cold one, so folding them together (as `asr_model` used to)
    // made the decode span unreadable on exactly the take that is slowest.
    // YV80: on the first take of a session this is the lazy load's bill, minus
    // whatever of it `arm_asr_engine` already paid off while the user was still
    // talking — so `asr_load_ms` in the latency line IS the penalty the user
    // felt, and it is 0 again on every take after it.
    let load_ms = started.elapsed().as_millis() as i64;
    // The "Language I speak" setting used to reach Whisper as the sidecar's
    // `--language`; it now rides the engine's own language hint (YV34) so the
    // picker keeps working. Blank = autodetect.
    let language = Some(language.trim())
        .filter(|l| !l.is_empty())
        .map(str::to_string);
    let decode_started = std::time::Instant::now();
    // Y3-B: the whole take, windowed when it is long enough to be near the
    // per-call wall. A short take is the exact same single `transcribe` call
    // this line used to be; a long one decodes in windows under a PER-CHUNK
    // budget and comes back partial rather than empty if a window dies.
    let take = manager.transcribe_take_observed(samples, language, bias_prompt, observer)?;
    let decode_ms = decode_started.elapsed().as_millis() as i64;
    if take.windowed {
        log::info!(
            "dictation decoded in {} window(s), {} failed, degraded={}",
            take.chunks_total,
            take.chunks_failed,
            take.degraded
        );
    }
    if take.text.trim().is_empty() {
        return Err(blank_take_error(&take));
    }
    Ok(transcription::AsrOutput {
        text: take.text,
        backend: "native".into(),
        seconds: started.elapsed().as_secs_f64(),
        load_ms,
        decode_ms,
        degraded: take.degraded,
        chunks: take.chunks_total,
        chunks_failed: take.chunks_failed,
        degraded_reason: take.degraded_reason,
    })
}

/// What a finished take produced (YV49).
///
/// Dictation inserts a transcript row and pastes it. A command-mode take does
/// neither: it edits the selection in place (or is rejected), and the spoken
/// instruction is NOT the user's text, so it never enters history. Both of the
/// pre-YV49 "nothing to paste" exits (no speech, hallucination loop) are the
/// same `Soft` shape, which is why the worker's `Ok(None)` became this enum.
enum TakeOutcome {
    /// Transcript row + paste result + release→clipboard latency + whether a
    /// ⌘V was actually attempted (YV74: only an *attempted* paste that never
    /// produced a read receipt raises the "Copy again" toast action — a
    /// deliberate copy-only take already left the text on the clipboard).
    ///
    /// The row is boxed: it is ~10x every other variant, and this value is
    /// moved through the worker's result on every take.
    Dictated(Box<TranscriptEntry>, paste::PasteOutcome, i64, bool),
    /// A transient toast, not an error: no speech, an applied command edit, or
    /// an unrecognised command. Nothing was captured worth keeping.
    Soft(String),
    /// YV67 — a gate threw the take away AFTER the wav was written. Still not a
    /// crash (no hard `last_error`), but the audio is real and the verdict can
    /// be wrong: a false positive here used to destroy a genuine dictation with
    /// no transcript row, no recovery row and no wav. This arm keeps the audio
    /// and writes the retryable row, so History offers Recover.
    ///
    /// `reason` is the short, non-transcript string stored on the recovery row —
    /// `'static` on purpose so a gate can never smuggle the spoken text into it.
    Rejected {
        message: String,
        reason: &'static str,
    },
    /// PERM-D — the take's buffer was EXACTLY zero for its whole length: a
    /// muted or unauthorized input, not quiet speech. Nothing is transcribed,
    /// nothing is pasted, no transcript row is written — and, like `Rejected`,
    /// the audio is KEPT under the recovery-dir lifecycle, because the clip is
    /// the evidence of what the device actually delivered.
    SilentCapture {
        /// The grant as re-read at the end of the take.
        status: mic_auth::MicAuth,
        /// `status` is not `Authorized`: surface the permission screen.
        needs_permission: bool,
    },
    /// Y3-D — the user cancelled this take mid-decode.
    ///
    /// Its own arm rather than a `Soft` or an `Err`: `Soft` pastes nothing but
    /// also keeps nothing (there is no audio worth saving behind a fumbled tap),
    /// and `Err` is a failure, which a deliberate cancel is not. This arm keeps
    /// the audio under the recovery lifecycle, writes NO transcript row, pastes
    /// NOTHING, and settles the pill to idle.
    Cancelled,
}

fn stop_and_transcribe(app: AppHandle, state: Arc<AppState>) {
    // Hands-free: ignore key-release; only stop on explicit tap / button
    if *state.hands_free.lock() {
        log::debug!("stop ignored — hands-free latch active");
        return;
    }
    if !*state.recording.lock() {
        return;
    }
    *state.recording.lock() = false;
    float_pill::set_dictating(false);
    // YV28: un-mute the Mac immediately on release — restore the exact prior
    // mute + volume BEFORE the (async) transcription so audio comes right back.
    restore_system_output(&state);
    let _ = app.emit("recording", false);

    let active = state.recorder.lock().take();
    let Some(active) = active else {
        return;
    };
    // yap-ptt-capture-tail (D4): the release→clipboard clock starts AT the stop
    // edge, so the context reads below are inside it.
    let t_release = std::time::Instant::now();
    // D4: end the capture NOW — before the context reads, which talk to the
    // user's app over AX and can stall for seconds (see
    // `record::end_at_stop_edge`). Off the main thread, so a wedged capture
    // worker can never freeze the UI; the worker below joins it.
    let (capture_stop, (source_app, source_bundle_id, cursor_context)) = record::end_at_stop_edge(
        move || record::end_capture(active),
        || {
            (
                // Capture focused app *before* we steal focus with notifications / main window.
                focus::frontmost_app_name(),
                // yap-format-structure: the paste target's bundle id picks the list
                // syntax (Markdown / plain bullets / off). Captured with
                // `source_app`, before Yap can take focus.
                focus::frontmost_bundle_id(),
                // YV50 context awareness — the text just before the caret while the
                // user's app is still the focused one. Bounded to
                // `focus::CONTEXT_CHAR_LIMIT` chars; `None` on AX denial or a secure
                // field. PRIVACY: this string only ever steers the formatting
                // decisions below — it is never logged, stored, or emitted (see the
                // `cursor_context_is_never_logged_or_persisted` test).
                focus::text_before_cursor(),
            )
        },
    );

    *state.busy.lock() = true;
    emit_status(&app, &state);

    let settings = state.settings.lock().clone();
    let db = state.db.clone();
    let app2 = app.clone();
    let state2 = state.clone();
    // YV49: `Some` means this take was a command press — the selection it is
    // about to edit was read at key-down. Taken (not read) so exactly one take
    // can ever consume it.
    let command_selection = state.command_selection.lock().take();
    // YV39: the generation this take belongs to. Re-checked immediately before
    // the paste — a cancel during ASR bumps it and the transcript is copied
    // only, never pasted.
    let generation = state.paste_generation.load(Ordering::SeqCst);

    std::thread::spawn(move || {
        // Panic-safety: disarmed after the normal cleanup below (see [8]).
        let mut busy_guard = WorkerBusyGuard {
            state: state2.clone(),
            app: app2.clone(),
            armed: true,
        };
        // YV52 dictation recovery — the take's clip guard, parked here as soon
        // as capture finishes so the `Err` arm below can PRESERVE the wav and
        // record a recoverable row instead of letting the guard unlink it. On
        // every other exit it is dropped at the end of this thread exactly as
        // before, so a successful take's audio still never outlives the take.
        let mut pending: Option<(record::ClipWav, f64, Option<String>)> = None;
        // `Ok(None)` = nothing to paste (no speech, or a rejected hallucination
        // loop): the caller shows a gentle soft status instead of a hard error.
        let result = (|| -> Result<TakeOutcome, String> {
            // YV36 voice isolation — borrow the WARM Silero VAD built once at
            // startup (gated behind the `denoise` setting). Nothing is loaded,
            // downloaded or analysed on the hot path; `None` (model still
            // downloading, load failed, or denoise off) → the energy-VAD path,
            // so an utterance is never lost.
            let iso_vad = if settings.denoise {
                state2.vad.lock().clone()
            } else {
                None
            };
            let ended = capture_stop
                .join()
                .map_err(|_| "the capture-stop thread panicked".to_string())??;
            let rec = record::finish_recording(ended, iso_vad.as_deref())?;
            // YV20/M3: `rec.clip` unlinks the history wav on ANY exit of this
            // closure (success, the gates below, or the ASR/DB `?` failures) —
            // never leaked on error. YV37: that wav is written on a background
            // thread from here on, so it is off the release→ASR path entirely.
            let t_dsp = t_release.elapsed().as_millis() as i64;
            log::info!(
                "samples ready: {} speech={:.2}s voiced={:.2}s hold_wall={:.2}s dsp_ms={}",
                rec.clip.path().display(),
                rec.speech_seconds,
                rec.voiced_seconds,
                rec.hold_wall_seconds,
                t_dsp
            );
            // YV52: hand the clip guard to the failure arm (see `pending`). The
            // wav still exists only for the life of this take on every path that
            // is not an outright failure.
            // DB-B — the take's id, read before the clip moves: the decode
            // persists its prefix under it, and the in-flight recovery link the
            // clip writer made carries the same stem.
            let take_id = rec.clip.take_id();
            pending = Some((rec.clip, rec.speech_seconds, source_app.clone()));
            // Y3-D checkpoint — the capture→decode boundary, placed immediately
            // after `pending` takes the clip so the cancel arm inherits YV52's
            // keep-the-audio guarantee instead of re-deriving it. Every later
            // checkpoint is the same two lines for the same reason.
            if take_is_cancelled(&state2) {
                return Ok(TakeOutcome::Cancelled);
            }
            // YV67 — the mic died mid-hold (unplugged, format change). Whatever
            // it captured before that is real audio and is already on disk, but
            // the take is TRUNCATED at an arbitrary word, so transcribing and
            // pasting it would type a half sentence the user cannot get back.
            // Fail it instead: the `Err` arm below preserves the wav and writes
            // the Retry row.
            //
            // The ORDER here is load-bearing. Raising this inside
            // `record::stop_recording` (or anywhere above the line before this
            // one) would run BEFORE the clip guard is handed to `pending`, so
            // `ClipWav::drop` would unlink the very audio we are trying to save —
            // which is the bug, not the fix.
            if rec.device_failed {
                return Err(DEVICE_FAILED_MESSAGE.into());
            }
            // PERM-D — SILENT-CAPTURE gate, and it is asked BEFORE the
            // no-speech gate on purpose. An all-zero buffer passes through
            // YV16 as "didn't catch any speech", which is a sentence about the
            // SPEAKER; the truth is that the input delivered nothing at all,
            // which is a sentence about the DEVICE or the grant. Answering the
            // second question with the first is what made a first-run take
            // with no microphone permission look like a user who mumbled.
            //
            // The grant is re-read HERE, at the end of the take, not reused
            // from the press: revoking it in System Settings mid-take is
            // exactly the case that produces this buffer.
            //
            // Exactly zero, never a threshold — see `dictation::is_silent_capture`.
            let mic_status_at_take_end = mic_auth::authorization_status();
            if let dictation::TakeAudioVerdict::Silent { needs_permission } =
                dictation::classify_take_audio(&rec.samples, mic_status_at_take_end)
            {
                let status = mic_status_at_take_end;
                log::warn!(
                    "silent-capture gate: peak=0 over {} samples, mic={} — not transcribing",
                    rec.samples.len(),
                    status.as_str()
                );
                return Ok(TakeOutcome::SilentCapture {
                    status,
                    needs_permission,
                });
            }
            // No-speech gate (YV16): a near-silent / sub-second tap has ~0 TRUE
            // voiced time. Skip ASR entirely so Whisper never hallucinates
            // repetitive garbage ("WPM-SERV-SERV…") on silence. YV29 STRENGTHENS
            // this: Silero VAD (`speech_present`) also rejects a clip it finds no
            // speech in — catching steady-noise cases the energy RMS gate scores
            // as voiced. `speech_present` is true whenever Silero is unavailable,
            // so a missing model never drops a real utterance.
            if !dictation::has_enough_speech(rec.voiced_seconds) || !rec.speech_present {
                log::info!(
                    "no-speech gate: voiced={:.2}s speech_present={} below threshold — skipping ASR",
                    rec.voiced_seconds,
                    rec.speech_present
                );
                return Ok(TakeOutcome::Soft(NO_SPEECH_MESSAGE.into()));
            }
            // YV34 — the embedded GGUF engine is the ONLY transcriber: it runs
            // in-process (no interpreter, no HuggingFace download) on the
            // selected native model. With nothing downloaded we fail fast with an
            // actionable message instead of pretending; the `transcript_error`
            // emit below is what unspins any UI waiting on this take.
            let Some((model_id, model_path)) = native_model_ready(&settings.native_model) else {
                return Err(
                    "No speech model installed — open Settings → Models and download one.".into(),
                );
            };
            // YV47 — bias the decoder with the auto-learning dictionary: starred
            // terms first, then usage-ranked, capped to the prompt window. A DB
            // hiccup here must never cost the take, so it degrades to unbiased.
            let bias_prompt = match db.bias_terms(BIAS_TERM_LIMIT) {
                Ok(terms) => asr_engine::build_bias_prompt(&terms),
                Err(e) => {
                    log::warn!("dictionary bias unavailable ({e}) — decoding unbiased");
                    None
                }
            };
            // Y4-C — the take's silences, taken BEFORE `rec.samples` is moved
            // into the engine. This is the paragraphing signal: measured at
            // capture, thrown away until now.
            let take_pauses = rec.pause_spans.clone();
            // Y3-C — the pill's only honest source of mid-decode progress.
            //
            // The observer is handed to the decode and driven from INSIDE
            // Y3-B's window loop: `planned` fires once, with the number of
            // windows the planner actually produced, and `chunk_done` fires
            // once per window that really finished, carrying that window's real
            // text. A take the planner did not split never calls `planned` at
            // all, so nothing is emitted and the pill stays on its
            // undifferentiated busy state — correct, because a single window
            // has no progress to report and a 1/1 flicker is worse than
            // silence. There is no timer here and nothing is interpolated
            // between completions.
            let sink = transcribe_progress::AppProgress(app2.clone());
            let mut progress = transcribe_progress::TakeProgress::new(&sink);
            // Y3-D checkpoint — pre-decode. The last chance to not start a
            // fifteen-minute decode at all.
            if take_is_cancelled(&state2) {
                return Ok(TakeOutcome::Cancelled);
            }
            // DB-B — the decode's prefix goes to `take_chunks` as it grows, so a
            // kill mid-decode leaves both the audio (the clip's in-flight link)
            // and the words already decoded. Dropping `persist` — explicitly
            // below, or on any early return — clears that chunk text: an
            // orderly end of the decode leaves nothing resumable, and only a
            // death skips the drop.
            let db_ref: &Database = &db;
            let mut persist = PersistTakeChunks::new(&mut progress, db_ref, take_id.as_deref());
            let asr = match transcribe_native(
                &state2.transcription,
                &model_id,
                &model_path,
                rec.samples,
                &settings.language,
                bias_prompt,
                &mut persist,
            ) {
                Ok(asr) => asr,
                // Y3-D — the engine stopped because the user asked, which is
                // not a decode failure. Compared against the CONSTANT, never a
                // literal: transcription.rs:74-77 documents exactly this bug.
                Err(e) if e == transcription::CANCELLED_BY_USER => {
                    return Ok(TakeOutcome::Cancelled);
                }
                Err(e) => return Err(e),
            };
            drop(persist);
            // Y3-D checkpoint — post-decode. A decode with no cancel hook runs
            // to completion however loudly it is asked to stop; this is what
            // stops its text reaching the clipboard anyway.
            if take_is_cancelled(&state2) {
                return Ok(TakeOutcome::Cancelled);
            }
            // Y3-B — a DEGRADED take is text the user keeps AND audio we refuse
            // to throw away: some window of it never decoded, so the recovery
            // row (YV52/YV66's Recover path) is what lets them retry the same
            // audio instead of re-speaking eleven minutes of it.
            if asr.degraded {
                let reason = asr
                    .degraded_reason
                    .clone()
                    .unwrap_or_else(|| "some of this take did not decode".to_string());
                match pending.take() {
                    Some((clip, speech_seconds, src)) => {
                        let kept = keep_failed_take(
                            &db,
                            &recovery_dir(),
                            clip,
                            speech_seconds,
                            src,
                            &reason,
                        );
                        log::warn!(
                            "degraded take ({}/{} windows failed) — clip preserved for retry: {}",
                            asr.chunks_failed,
                            asr.chunks,
                            kept.is_some()
                        );
                    }
                    None => log::warn!(
                        "degraded take ({}/{} windows failed) — no clip to preserve",
                        asr.chunks_failed,
                        asr.chunks
                    ),
                }
            }
            // The take is over: nothing may repaint the pill behind the
            // transcript that is about to land on it.
            progress.finish();
            // Raw ASR output — preserved verbatim so both raw and polished text are
            // stored on the transcript (Wispr Flow "Undo AI edit" / raw↔polished).
            let mut raw_text = asr.text;
            // Hallucination gate (YV16, made non-destructive by YV66): degenerate
            // Whisper repetition loops ("WPM-SERV-SERV-SERV…") must not reach the
            // clipboard/paste path — but a loop at the END of a good take is no
            // reason to destroy the good part. `degenerate_cutoff` says WHERE the
            // degeneration starts, so a long dictation that goes off the rails in
            // its last stretch keeps everything in front of the rails.
            match dictation::degenerate_cutoff(&raw_text) {
                Some(cutoff)
                    if raw_text[..cutoff].split_whitespace().count()
                        >= dictation::DEGEN_MIN_KEEP_TOKENS =>
                {
                    // YV20/M2: never write the transcript body to the log — count only.
                    let before = raw_text.chars().count();
                    raw_text.truncate(cutoff);
                    let kept = raw_text.chars().count();
                    log::info!(
                        "hallucination gate: truncated degenerate tail (kept {} chars, dropped {} chars)",
                        kept,
                        before - kept
                    );
                }
                Some(_) => {
                    // Degenerate from the first token — there is no prefix to
                    // keep. YV67: a REJECTION, not a soft nothing — the wav
                    // exists by now, and this gate is a heuristic that can be
                    // wrong (a legitimately repetitive dictation reads as a
                    // loop). Keeping the audio + a retryable row is the
                    // difference between "Yap ate my dictation" and one click of
                    // Recover.
                    log::info!(
                        "hallucination gate: rejected degenerate ASR output ({} chars)",
                        raw_text.chars().count()
                    );
                    return Ok(TakeOutcome::Rejected {
                        message: NO_SPEECH_MESSAGE.into(),
                        reason: GATE_REPETITION_REASON,
                    });
                }
                None => {}
            }
            // YV49 command mode — this take is an INSTRUCTION about the text the
            // user had selected when they pressed, not text to type. It branches
            // BEFORE the cleanup pipeline on purpose: list detection and polish
            // are for dictated prose and would reshape the instruction itself.
            //
            // The hard rule: an instruction outside the deterministic command
            // table NEVER edits the selection. We toast and leave the text
            // exactly as it was — guessing would silently mangle text the user
            // cannot get back.
            if let Some(selection) = command_selection {
                let Some(command) = command_mode::parse_command(&raw_text) else {
                    // YV20/M2 hygiene: count only, never log the spoken text.
                    log::info!(
                        "command mode: unrecognised instruction ({} chars) — selection untouched",
                        raw_text.chars().count()
                    );
                    return Ok(TakeOutcome::Soft(
                        command_mode::UNKNOWN_COMMAND_MESSAGE.into(),
                    ));
                };
                // Same cancellation guard as dictation (YV39): a command the user
                // cancelled during ASR must not edit their app seconds later.
                if state2.paste_generation.load(Ordering::SeqCst) != generation {
                    log::warn!("stale command (cancelled during transcription) — no edit");
                    return Ok(TakeOutcome::Soft("Command cancelled".into()));
                }
                // `Delete` is carried out as a real Delete keystroke rather than
                // pasting an empty string (a no-op in several apps), which also
                // leaves the user's clipboard alone.
                let outcome = if command == command_mode::Command::Delete {
                    paste::delete_selection(&app2, source_bundle_id.as_deref())
                } else {
                    let edited = command_mode::apply(&command, &selection);
                    paste::insert_dictated_take(&app2, &edited, true, source_bundle_id.as_deref())
                };
                log::info!(
                    "command mode: {} ({} selected chars) → {}",
                    command.label(),
                    selection.chars().count(),
                    outcome.message
                );
                return Ok(TakeOutcome::Soft(if outcome.pasted {
                    command.label()
                } else {
                    format!("{} — {}", command.label(), outcome.message)
                }));
            }

            // Smart dictation (YV5): resolve the effective mode — a user-picked fixed mode
            // wins, otherwise it's inferred from the focused app. YV50 adds the
            // pre-caret context as a HINT that only fills the gap left when the
            // app alone resolved nothing (an email greeting in a browser tab).
            let dictation_mode = dictation::resolve_mode_with_context(
                &settings.dictation_mode,
                source_app.as_deref().unwrap_or_default(),
                cursor_context.as_deref(),
            );
            // YV10 cleanup pipeline: apply_dictionary → backtrack → format_dictation →
            // local-LLM polish (guarded stub), gated by the Auto-Cleanup level. Every
            // stage is guarded so a non-empty transcript can never become empty (falls
            // back to its input on any error/empty result — "never lose text").
            let cleanup_level = dictation::CleanupLevel::from_setting(&settings.cleanup_level);
            // YV61 stage 4: the validated polish path, bound to THIS take's mode
            // and the tone dial for it. It fails closed — a missing model, a
            // missed deadline, a crashed sidecar or a rewrite that fails
            // `validate_polish` all return `None`, and `run_cleanup` keeps the
            // rules text byte for byte.
            let polish_config = polish::PolishConfig::from_settings(&settings, dictation_mode);
            let t_cleanup = std::time::Instant::now();
            // yap-format-structure: list syntax for THIS paste target, from the
            // Structure switch and the per-app override list. A user-picked
            // Plain mode means "exactly what I said" — no structure either.
            let structure_format = dictation::structure_format_for_take(
                settings.structure_enabled && settings.dictation_mode.trim() != "plain",
                &settings.structure_app_overrides,
                source_bundle_id.as_deref().unwrap_or_default(),
                dictation_mode,
            );
            let (text, formatting_trace) = dictation::run_cleanup_structured_traced(
                &raw_text,
                cleanup_level,
                dictation_mode,
                structure_format,
                // YV62: the same tone dial the model's overlay gets, so R3's
                // trailing-period rule holds with the polish stage off.
                polish_config.style,
                // Y4-C: the pauses this speaker actually left.
                &take_pauses,
                |t| db.apply_dictionary(t).unwrap_or_else(|_| t.to_string()),
                // Y4-G: the Result twin, so a refusal arrives with its REASON
                // (`deadline` / `no_model` / `no_sidecar` / a validator tag)
                // instead of as an indistinguishable `None`. Both polish
                // failure modes are otherwise invisible by construction.
                |t| polish::polish_llm_result(t, dictation_mode, &polish_config),
            );
            // YV50: join the take onto the text already at the caret — lowercase
            // the lead word when the user is continuing a sentence, capitalise it
            // after . ? ! / a new line / in an empty field, and add the leading
            // space only when the character before the caret needs one. Purely
            // additive (casing + at most one space), so "never lose text" holds.
            // Y4-F: mode-aware. Every prose mode gets the R5 casing rule; `Code`
            // gets the spacing and nothing else, because a caret after `let x = 5. `
            // is not the end of a sentence and `foo` must not become `Foo`.
            let text = dictation::join_for_mode(&text, cursor_context.as_deref(), dictation_mode);
            // YV48 snippets: expand saved trigger phrases AFTER cleanup and
            // before the clipboard. Its own stage — never inside the dictionary
            // closure above, and never on the history command paths — so only a
            // live dictation can expand. A DB/matcher failure degrades to the
            // un-expanded text: the transcript always pastes.
            let text = match db.snippet_rules() {
                Ok(rules) if !rules.is_empty() => snippets::expand_snippets(
                    &text,
                    &rules,
                    snippets::SnippetScope::from_setting(&settings.snippet_scope),
                ),
                Ok(_) => text,
                Err(e) => {
                    log::warn!("snippets unavailable ({e}) — pasting text unexpanded");
                    text
                }
            };
            // YV62 signature (R13): the LAST stage, after the model and after the
            // snippet expansion, so the configured block reaches the pasteboard
            // byte for byte. Off by default — nothing is appended until the user
            // configures a signature AND turns the mode on.
            let text = snippets::append_signature(
                &text,
                &settings.signature,
                snippets::SignatureMode::from_setting(&settings.signature_mode),
                dictation_mode,
            );
            let cleanup_ms = t_cleanup.elapsed().as_millis() as i64;
            // Y4-G — one counter per skip reason in the local log. Only when
            // the stage was actually asked for, so a Light/Medium user never
            // sees a line about a model they never enabled.
            if cleanup_level == dictation::CleanupLevel::High {
                polish::log_polish_skip_counts();
            }
            log::info!(
                "cleanup-pipeline: level={:?} mode={:?} dictation_setting={} cleanup_level={}",
                cleanup_level,
                dictation_mode,
                settings.dictation_mode,
                settings.cleanup_level
            );
            // Always copy first (Wispr Flow: never lose text). The wrong-target
            // guard (YV21) inside insert_dictated_take re-checks the frontmost app
            // against `source_bundle_id` immediately before ⌘V and falls back to
            // clipboard-only if the user switched apps during the ASR delay.
            // YV39 cancellation guard: a stale take (cancelled while this one
            // was transcribing) is copied but NEVER pasted.
            // Y3-D checkpoint — the last boundary before anything reaches the
            // user's app. A cancel here pastes NOTHING: not even the
            // copy-to-clipboard a stale take still gets below, because a stale
            // take is one the user replaced and a cancelled one is one they
            // rejected.
            if take_is_cancelled(&state2) {
                return Ok(TakeOutcome::Cancelled);
            }
            let current_generation = state2.paste_generation.load(Ordering::SeqCst);
            if current_generation != generation {
                log::warn!(
                    "stale dictation (cancelled during transcription) — copy only, no paste"
                );
            }
            let want_paste = paste::should_paste(
                settings.auto_paste,
                generation,
                current_generation,
                focus::should_auto_paste(),
            );
            let t_paste = std::time::Instant::now();
            let outcome =
                paste::insert_dictated_take(&app2, &text, want_paste, source_bundle_id.as_deref());
            let paste_ms = t_paste.elapsed().as_millis() as i64;
            // North-star metric: release hotkey → text on clipboard
            let pipeline_ms = t_release.elapsed().as_millis() as i64;
            // YV40: the whole take as disjoint spans in one stable key=value
            // line (see `latency`) — press side, capture, DSP finalize, ASR load
            // vs decode, cleanup, paste — instead of the old cumulative mix.
            log::info!(
                "{}",
                latency::PipelineSpans {
                    hold_clipboard_ms: pipeline_ms,
                    press_capture_ms: rec.capture_start_ms,
                    capture_ms: (rec.hold_wall_seconds * 1000.0).round() as i64,
                    samples_ready_ms: t_dsp,
                    asr_load_ms: asr.load_ms,
                    asr_decode_ms: asr.decode_ms,
                    cleanup_ms,
                    paste_ms,
                    backend: &asr.backend,
                    pasted: outcome.pasted,
                }
                .summary_line()
            );
            // YV73: the memory/disk counterpart of the line above, one sample per
            // take, so a session's logs show growth take by take. Detached — the
            // take is pasted by now, but reading RSS forks `ps` and this thread
            // still has a transcript row to write.
            hygiene::log_snapshot_async(data_dir());
            // Store BOTH the polished text and the raw ASR transcript (YV10).
            let mut entry = db.insert_transcript_at(
                text,
                asr.backend,
                asr.seconds,
                rec.speech_seconds,
                pipeline_ms,
                source_app,
                chrono::Utc::now(),
                Some(raw_text),
            )?;
            // Y4-G — attribute this take's shape to the stages that made it.
            // AFTER the insert on purpose: the row is durable and the text is
            // already in the user's app, so a failure here costs the diff panel
            // its stage list and costs the dictation nothing.
            let stages = formatting_trace.stages_that_ran();
            if let Err(e) =
                db.set_take_formatting(&entry.id, &stages, formatting_trace.llm_skip_reason)
            {
                log::warn!("formatting trace not recorded: {e}");
            }
            entry.stages_that_ran = Some(stages);
            entry.polish_skip_reason = formatting_trace.llm_skip_reason.map(|r| r.to_string());
            // Hygiene: the history wav is unlinked by `rec.clip` on scope exit
            // (audio stays local only during the process, success or error alike).
            Ok(TakeOutcome::Dictated(
                Box::new(entry),
                outcome,
                pipeline_ms,
                want_paste,
            ))
        })();

        match result {
            Ok(TakeOutcome::Dictated(entry, outcome, pipeline_ms, attempted_paste)) => {
                if outcome.pasted {
                    *state2.last_error.lock() = None;
                } else if !outcome.copied {
                    *state2.last_error.lock() = Some(outcome.message.clone());
                } else if settings.auto_paste && !outcome.pasted {
                    // Soft warning — text is still on clipboard
                    *state2.last_error.lock() = Some(outcome.message.clone());
                } else {
                    *state2.last_error.lock() = None;
                }
                // YV51: this take is now the newest, so it decides whether
                // "Undo AI Edit" has anything to undo. The tray item picks the
                // new value up from the `emit_status` at the end of the take.
                *state2.undo_available.lock() =
                    dictation::undo_ai_edit_text(&entry.text, entry.raw_text.as_deref()).is_some();
                let _ = app2.emit("transcript", &entry);
                // Y4-G silent-skip signal — say ONCE, quietly, that the model
                // was enabled and produced nothing for this take. Its own event
                // so the pill can show it without reading the history row.
                if let Some(reason) = entry.polish_skip_reason.as_deref() {
                    let _ = app2.emit(POLISH_SKIPPED_EVENT, reason);
                }
                let _ = app2.emit(PASTE_OUTCOME_EVENT, &outcome.message);
                // YV74 — we tried to paste and got no receipt. Say so with an
                // action instead of a dead sentence: the toast offers "Copy
                // again" for this row, so the text is one click away even when
                // the clipboard has moved on.
                if attempted_paste && !outcome.pasted {
                    let _ = app2.emit(PASTE_FAILED_EVENT, &entry.id);
                }
                let _ = app2.emit(
                    "latency",
                    serde_json::json!({
                        "pipelineMs": pipeline_ms,
                        "asrSeconds": entry.asr_seconds,
                        "speechSeconds": entry.speech_seconds,
                    }),
                );
                // YV79 — terminal marker, last of this arm's emits so the
                // `transcript` above has already handed the text over before any
                // listener clears its busy state on this.
                let _ = app2.emit(
                    TAKE_DONE_EVENT,
                    serde_json::json!({ "ok": true, "message": serde_json::Value::Null }),
                );
                let preview = if entry.text.chars().count() > 100 {
                    let s: String = entry.text.chars().take(100).collect();
                    format!("{s}…")
                } else {
                    entry.text.clone()
                };
                notify(
                    &app2,
                    "Yap",
                    format!("{preview}\n({} · {}ms)", outcome.message, pipeline_ms),
                );
                log::info!(
                    "transcript ok words={} pasted={} pipeline_ms={}",
                    entry.word_count,
                    outcome.pasted,
                    pipeline_ms
                );
            }
            Ok(TakeOutcome::Cancelled) => {
                // Y3-D. Three assertions live in this arm, and each one is a
                // test in tests/cancel_long_take.rs:
                //   * NO transcript row — we never reach `insert_transcript_at`
                //     from here, so History gains a recovery row and nothing
                //     else.
                //   * NOTHING pasted — no `copy_and_maybe_paste`, not even the
                //     clipboard-only fallback a STALE take gets. A stale take is
                //     one the user superseded; a cancelled one is one they
                //     rejected.
                //   * The audio SURVIVES — parked under the same recovery-dir
                //     lifecycle (YV52/YV63, 7-day purge) a failed take gets, so
                //     "actually, transcribe that" is a Retry away. A cancel that
                //     destroys fifteen minutes of audio is a worse bug than the
                //     one this item fixes.
                // Not a hard `last_error`: the pill settles to idle, not to an
                // error state.
                *state2.last_error.lock() = None;
                let recoverable = pending.take().and_then(|(mut clip, speech_seconds, src)| {
                    keep_cancelled_take(&db, &recovery_dir(), &mut clip, speech_seconds, src)
                });
                let _ = app2.emit(
                    TAKE_CANCELLED_EVENT,
                    serde_json::json!({
                        "message": CANCELLED_TAKE_MESSAGE,
                        "failed": recoverable,
                    }),
                );
                // YV79 terminal marker, so any spinner clears on the cancel.
                let _ = app2.emit(
                    TAKE_DONE_EVENT,
                    serde_json::json!({ "ok": false, "message": CANCELLED_TAKE_MESSAGE }),
                );
                log::info!(
                    "take cancelled by the user (clip kept={})",
                    recoverable.is_some()
                );
            }
            Ok(TakeOutcome::Soft(msg)) => {
                // Nothing was typed: a fumbled tap or a command-mode outcome.
                // All NORMAL, not failures — surface a gentle soft status
                // (transient flash toast) and do NOT set a hard last_error, do
                // NOT insert a transcript.
                *state2.last_error.lock() = None;
                let _ = app2.emit(PASTE_OUTCOME_EVENT, &msg);
                // YV79 — the outcome onboarding used to miss entirely: nothing
                // was typed, and the toast above lives behind the overlay. The
                // reason rides along so the spinner is replaced by "Didn't catch
                // any speech — hold and speak" rather than a 90 s wait.
                let _ = app2.emit(
                    TAKE_DONE_EVENT,
                    serde_json::json!({ "ok": false, "message": &msg }),
                );
                log::info!("soft take outcome: {msg}");
            }
            Ok(TakeOutcome::Rejected { message, reason }) => {
                // YV67 — a gate threw away a take that HAD audio. Still not a
                // crash, so no hard `last_error`; but the wav is real and the
                // gate can be wrong, so it gets the same YV52 treatment a failed
                // take gets: keep the audio, write the retryable row. Before
                // this, `pending` was simply dropped here and `ClipWav::drop`
                // unlinked the only copy — a gate false positive was permanent
                // loss, with nothing in `failed_dictations` to show for it.
                *state2.last_error.lock() = None;
                let recoverable = pending.take().and_then(|(clip, speech_seconds, src)| {
                    keep_failed_take(&db, &recovery_dir(), clip, speech_seconds, src, reason)
                });
                log::info!(
                    "rejected take: {reason} (recoverable={})",
                    recoverable.is_some()
                );
                // Same payload shape as the `Err` arm, so History and the toast
                // offer Recover on exactly this take.
                let _ = app2.emit(
                    TRANSCRIPT_ERROR_EVENT,
                    serde_json::json!({ "message": &message, "failed": recoverable }),
                );
                // YV79 — terminal marker (see the `Soft` arm): the rejection
                // reason is what the waiting view shows instead of spinning.
                let _ = app2.emit(
                    TAKE_DONE_EVENT,
                    serde_json::json!({ "ok": false, "message": &message }),
                );
            }
            Ok(TakeOutcome::SilentCapture {
                status,
                needs_permission,
            }) => {
                // PERM-D — the take recorded nothing at all. Not a crash (no
                // hard `last_error`), and NOT the no-speech toast: the user did
                // nothing wrong, the input did.
                *state2.last_error.lock() = None;
                let message = if needs_permission {
                    SILENT_CAPTURE_PERMISSION_MESSAGE
                } else {
                    SILENT_CAPTURE_DEVICE_MESSAGE
                };
                // The clip is preserved under the SAME recovery-dir lifecycle a
                // rejected take gets (YV52/YV67). Deleting it would destroy the
                // one artefact that proves what the device delivered, and the
                // gate — like every gate — can be wrong.
                let recoverable = pending.take().and_then(|(clip, speech_seconds, src)| {
                    keep_failed_take(
                        &db,
                        &recovery_dir(),
                        clip,
                        speech_seconds,
                        src,
                        SILENT_CAPTURE_REASON,
                    )
                });
                // The permission screen, not a toast, when the grant is the
                // reason. This is the event PERM-C already wired to the pill and
                // the Permissions pane, so one emit paints both.
                if needs_permission {
                    let _ = app2.emit(
                        "mic_permission_required",
                        MicGateNotice {
                            status: status.as_str().to_string(),
                            prompting: false,
                        },
                    );
                }
                let _ = app2.emit(
                    TAKE_FAILED_EVENT,
                    serde_json::json!({
                        "code": SILENT_CAPTURE_CODE,
                        "status": status.as_str(),
                        "needsPermission": needs_permission,
                        "message": message,
                        "failed": recoverable,
                    }),
                );
                // Terminal marker (YV79), so onboarding's calibration spinner
                // clears with this reason instead of running to its watchdog.
                let _ = app2.emit(
                    TAKE_DONE_EVENT,
                    serde_json::json!({ "ok": false, "message": message }),
                );
                log::warn!(
                    "silent capture: mic={} needs_permission={} (recoverable={})",
                    status.as_str(),
                    needs_permission,
                    recoverable.is_some()
                );
            }
            Err(e) => {
                log::error!("pipeline failed: {e}");
                *state2.last_error.lock() = Some(e.clone());
                // YV52: a failed take is never lost. Keep the wav the pipeline
                // already wrote and record a recoverable row, so the user can
                // retry ASR on the same audio (the engine may since have
                // recovered, or the model finished downloading) instead of
                // re-speaking it. `None` = the clip could not be preserved, and
                // the toast/History simply have nothing to retry.
                let recoverable = pending.take().and_then(|(clip, speech_seconds, src)| {
                    keep_failed_take(&db, &recovery_dir(), clip, speech_seconds, src, &e)
                });
                // YV32: tell the UI, not just the log. Any view waiting on a
                // transcript (onboarding calibration, the pill) can now clear its
                // busy state and show the reason instead of spinning forever.
                // The recoverable row rides along so the error toast can offer
                // Retry on exactly this take.
                let _ = app2.emit(
                    TRANSCRIPT_ERROR_EVENT,
                    serde_json::json!({ "message": e.clone(), "failed": recoverable }),
                );
                // YV79 — terminal marker (see the `Soft` arm), emitted after the
                // error payload above so a listener that clears busy on this one
                // has the reason in hand already.
                let _ = app2.emit(
                    TAKE_DONE_EVENT,
                    serde_json::json!({ "ok": false, "message": &e }),
                );
                notify(&app2, "Yap — Failed", e);
            }
        }
        *state2.busy.lock() = false;
        *state2.hands_free.lock() = false;
        // Y3-D: the take is over on EVERY path that reaches here — success,
        // soft, rejection, silent capture, error and cancel alike — so Escape
        // goes back to the rest of the Mac here, once, instead of in six arms
        // where one would eventually be forgotten.
        unregister_cancel_shortcut(&app2);
        // Keep island on all Spaces if always-on; else hide after take
        float_pill::after_recording(&app2, state2.settings.lock().show_floating_pill);
        emit_status(&app2, &state2);
        // Reached the normal end — no soft-lock, disarm the panic guard.
        busy_guard.armed = false;
    });
}

/// YV52 — preserve a failed take: move its wav into the recovery dir and write
/// the row that makes it retryable. Best-effort by design — a take that cannot
/// be preserved (write failed, disk full, DB error) leaves the user exactly
/// where they were before this feature existed, never worse, and never with a
/// row pointing at audio that is not there.
///
/// YV67 also routes GATE REJECTIONS through here — `dir` is a parameter (always
/// `recovery_dir()` in the app) so the whole keep-the-audio path is drivable
/// against a temp dir in a test instead of the user's real recovery folder.
fn keep_failed_take(
    db: &Database,
    dir: &Path,
    mut clip: record::ClipWav,
    speech_seconds: f64,
    source_app: Option<String>,
    error: &str,
) -> Option<FailedDictation> {
    let path = match clip.keep_for_recovery(dir) {
        Ok(path) => path,
        Err(e) => {
            log::warn!("failed take not recoverable (clip could not be kept): {e}");
            return None;
        }
    };
    match db.record_failed_dictation(&path, speech_seconds, error, source_app) {
        Ok(row) => {
            log::info!(
                "YV52 recovery: kept failed take {} ({:.2}s) for retry",
                row.id,
                speech_seconds
            );
            Some(row)
        }
        Err(e) => {
            // No row means nothing can ever reach this wav — don't leak audio.
            log::warn!("failed-dictation row not written ({e}) — removing kept clip");
            let _ = std::fs::remove_file(&path);
            None
        }
    }
}

/// The error a crash-recovered take (YV63) carries in History — the user sees
/// why the row is there, and Retry runs ASR over the audio that survived.
const CRASH_RECOVERY_ERROR: &str = "recovered after crash";

/// YV63 — a dictation the app died in the MIDDLE of. Nothing used to survive
/// that: the wav is only written after a take ends, so a glitch mid-hold took
/// the words with it. The capture journal spills the frames to
/// `data_dir()/recovery/` as they arrive and leaves an in-progress marker behind
/// if the take never completes; this is the other half — at startup, every
/// orphaned marker is finalized into a real wav and given the SAME
/// failed-dictation row a failed transcription gets (YV52), so it shows up in
/// History with Retry instead of being gone.
///
/// Best-effort throughout, exactly like `keep_failed_take`: a take that cannot
/// be turned into a row has its wav removed rather than left as audio nothing
/// can reach. Returns how many takes were recovered.
fn recover_crashed_takes(db: &Database, dir: &Path) -> usize {
    adopt_recovered_takes(
        db,
        record::recover_orphaned_journals(dir),
        CRASH_RECOVERY_ERROR,
        "YV63 crash recovery",
    )
}

/// The error a take recovered from its Y3-A spill carries in History. Distinct
/// from [`CRASH_RECOVERY_ERROR`] because it is a distinct story: the take was
/// being DECODED when the app went away, so some of its words already exist.
const UNFINISHED_TAKE_ERROR: &str = "recovered mid-transcription — not finished";

/// DB-B — the producer `take_chunks` was missing: a [`TakeObserver`] that
/// forwards every progress call to the pill's observer unchanged and, as each
/// window extends the take's unbroken decoded prefix, upserts that prefix (its
/// seam-merged text and the sample its content ends at) as chunk 0 of the take.
///
/// One row per take, overwritten — not one row per window — because the
/// windows OVERLAP: concatenating per-window text would print every seam
/// phrase twice, and the merge that removes the overlap needs all the windows
/// at once. The stored prefix is already merged, so `take_partial_text` and
/// `take_resume_sample` read it back verbatim.
///
/// A DB error is logged and swallowed: it costs the resume (the take is still
/// offered as audio), never the decode in progress.
///
/// [`TakeObserver`]: transcribe_progress::TakeObserver
pub struct PersistTakeChunks<'a> {
    inner: &'a mut dyn transcribe_progress::TakeObserver,
    db: &'a Database,
    take_id: Option<&'a str>,
}

impl<'a> PersistTakeChunks<'a> {
    pub fn new(
        inner: &'a mut dyn transcribe_progress::TakeObserver,
        db: &'a Database,
        take_id: Option<&'a str>,
    ) -> Self {
        Self { inner, db, take_id }
    }
}

/// The decode is over by an orderly route (done, failed, cancelled): the take
/// is no longer resumable, so its chunk text goes. A kill, a crash or
/// `app.exit` never runs this, which is exactly what leaves the prefix behind.
impl Drop for PersistTakeChunks<'_> {
    fn drop(&mut self) {
        if let Some(id) = self.take_id {
            if let Err(e) = self.db.clear_take_chunks(id) {
                log::warn!(
                    "DB-B: chunk text for a finished take not cleared ({e}) — the retention sweep will"
                );
            }
        }
    }
}

impl transcribe_progress::TakeObserver for PersistTakeChunks<'_> {
    fn planned(&mut self, windows: usize) {
        self.inner.planned(windows);
    }
    fn chunk_done(&mut self, text: &str) {
        self.inner.chunk_done(text);
    }
    fn wants_prefix(&self) -> bool {
        self.take_id.is_some()
    }
    fn prefix_done(&mut self, window_index: usize, end_sample: usize, text: &str) {
        let Some(take_id) = self.take_id else {
            return;
        };
        let end = i64::try_from(end_sample).unwrap_or(i64::MAX);
        if let Err(e) = self.db.record_take_chunk(take_id, 0, text, end) {
            log::warn!("DB-B: prefix through window {window_index} not persisted ({e})");
        }
    }
}

/// Turn recovered audio into the failed-dictation rows History offers, carrying
/// each take's id so its persisted chunk text comes back with it.
///
/// Shared by both recovery routes on purpose: a journal orphan and a spill
/// orphan are the same thing to everything downstream (one row, one Retry, one
/// 7-day window), and the moment they are adopted by two different pieces of
/// code is the moment they start drifting apart.
fn adopt_recovered_takes(
    db: &Database,
    takes: Vec<record::RecoveredTake>,
    error: &str,
    tag: &str,
) -> usize {
    let mut recovered = 0;
    for take in takes {
        match db.record_failed_dictation_for_take(
            &take.wav_path,
            take.seconds,
            error,
            None,
            Some(take.take_id.clone()),
        ) {
            Ok(row) => {
                recovered += 1;
                // YV20/M2: the take's WORDS never reach a log line — only how
                // many characters of them survived.
                log::info!(
                    "{tag}: take {} ({:.2}s) rebuilt from disk, partial text {} chars",
                    row.id,
                    take.seconds,
                    row.partial_text.as_ref().map_or(0, |t| t.chars().count())
                );
            }
            Err(e) => {
                log::warn!("{tag}: row not written ({e}) — removing rebuilt clip");
                let _ = std::fs::remove_file(&take.wav_path);
                // The words are unreachable now that nothing points at them.
                let _ = db.clear_take_chunks(&take.take_id);
            }
        }
    }
    recovered
}

/// DB-B — the launch sweep for a LONG take the app died in the middle of.
///
/// While a take decodes, its finished clip is hard-linked into the recovery
/// dir as `<id>.decoding.wav` (`record::ClipWav::spawn`), and the decode's
/// unbroken prefix is upserted to `take_chunks` window by window
/// (`PersistTakeChunks`). Every orderly end of the take removes both; a kill
/// -9, a crash or `app.exit(0)` runs no `Drop`, so they survive — in a dir that
/// `record::sweep_stale_wavs` (which empties `recordings/` first) never
/// touches. Y3-A's `<id>.capture.wav`, moved here by an in-process capture
/// abort, is read back the same way.
///
/// This is what reads them back. In order, and the order matters:
///
///   1. **Purge first.** Expired takes and their chunk text go before anything
///      is offered, so the 7-day window bounds this directory whether or not
///      the user ever opens History, and a take recovered today always gets a
///      full window rather than inheriting a nearly-expired one.
///   2. **Journal orphans**, exactly as YV63 already did them.
///   3. **Spill / in-flight orphans** — the takes that were being decoded
///      (and Y3-A capture spills). Each becomes `<id>.wav` via a temp file and
///      a rename, and the orphan is deleted only once that wav is complete.
///
/// What it deliberately does NOT do:
///
///   * **It never decodes.** A cold launch that pins the GPU for four minutes
///     because the user quit mid-take yesterday is its own bug. The take is
///     OFFERED; the user presses "Finish transcribing" when they want it.
///   * **It never pastes.** It takes no `AppHandle`, so it structurally cannot:
///     pasting recovered words into whatever app happens to be focused minutes
///     or days later is the wrong behaviour, and it would drive straight
///     through YV21's paste-target guard.
///
/// Returns how many takes are now on offer.
pub fn recover_dictation(db: &Database, dir: &Path) -> usize {
    // (1) Retention first — see the doc comment.
    purge_expired_failed_takes(db);
    purge_expired_take_chunks(db);
    // (2) The YV63 route: a take that died while CAPTURING.
    let from_journals = recover_crashed_takes(db, dir);
    // (3) The DB-B route: a take that died while DECODING, whose spill the
    //     journal scan walks straight past.
    let from_spills = adopt_recovered_takes(
        db,
        record::recover_orphaned_spills(dir),
        UNFINISHED_TAKE_ERROR,
        "DB-B long-take recovery",
    );
    let offered = from_journals + from_spills;
    if offered > 0 {
        log::info!(
            "startup: {offered} dictation(s) offered for recovery ({from_journals} from a capture journal, {from_spills} unfinished mid-transcription) — none decoded, none pasted"
        );
    }
    offered
}

/// DB-B retention — chunk text lives on the SAME 7-day window as the audio it
/// came from. This sweeps rows whose take row is already gone (a take adopted
/// and then discarded, a row that failed to write); the rows that still have an
/// owner go in `purge_failed_dictations`' own transaction, with their WAV.
fn purge_expired_take_chunks(db: &Database) -> usize {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(db::FAILED_TAKE_RETENTION_DAYS);
    match db.purge_take_chunks(cutoff) {
        Ok(0) => 0,
        Ok(n) => {
            log::info!(
                "DB-B retention: purged partial text for {} take(s) older than {} days",
                n,
                db::FAILED_TAKE_RETENTION_DAYS
            );
            n
        }
        Err(e) => {
            log::warn!("take-chunk purge skipped: {e}");
            0
        }
    }
}

/// YV52 — expired recoverable takes are dropped with their audio. Called at
/// startup and before every list, so the 7-day window holds in a long-running
/// session too. Returns how many were purged.
fn purge_expired_failed_takes(db: &Database) -> usize {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(db::FAILED_TAKE_RETENTION_DAYS);
    match db.purge_failed_dictations(cutoff) {
        Ok(paths) => {
            for path in &paths {
                let _ = std::fs::remove_file(path);
            }
            if !paths.is_empty() {
                log::info!(
                    "YV52 retention: purged {} failed take(s) older than {} days",
                    paths.len(),
                    db::FAILED_TAKE_RETENTION_DAYS
                );
            }
            paths.len()
        }
        Err(e) => {
            log::warn!("failed-take purge skipped: {e}");
            0
        }
    }
}

/// YV94 retention (finding #28) — meeting AUDIO older than the window is
/// deleted; the transcript is kept forever.
///
/// Time-based, deliberately NOT delete-after-summarize: there is no summarize
/// stage yet (YV97), so delete-after-summarize would mean either "the audio is
/// never deleted" or "the transcript is the only artifact and the room can never
/// be re-heard". Flip the default once YV97 has shipped and proven itself.
fn purge_expired_meeting_audio(db: &Database) -> usize {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(meetings::AUDIO_RETENTION_DAYS);
    match db.purge_meeting_audio(cutoff) {
        Ok(paths) => {
            for path in &paths {
                let _ = std::fs::remove_file(path);
            }
            if !paths.is_empty() {
                log::info!(
                    "YV94 retention: purged audio for {} meeting(s) older than {} days",
                    paths.len(),
                    meetings::AUDIO_RETENTION_DAYS
                );
            }
            paths.len()
        }
        Err(e) => {
            log::warn!("meeting audio purge skipped: {e}");
            0
        }
    }
}

/// YV73 — one pass of the disk sweep, from the hygiene thread.
///
/// The YV52 retention purge runs FIRST, so the row list this reads afterwards
/// is already free of expired takes and the sweep only ever sees true orphans.
/// A row list that cannot be read yields `None`, which makes `hygiene::sweep`
/// skip the recovery directory entirely rather than delete audio it cannot
/// prove is unreachable.
fn run_disk_sweep(db: &Database) {
    purge_expired_failed_takes(db);
    // YV94 — same posture, one window later: expired meeting audio is dropped
    // before the orphan sweep runs, so the sweep never sees those WAVs as
    // "unreachable files I should think about".
    purge_expired_meeting_audio(db);
    let keep = match db.list_failed_dictations() {
        Ok(rows) => Some(rows.into_iter().map(|r| r.wav_path).collect::<Vec<_>>()),
        Err(e) => {
            log::warn!("hygiene: recovery sweep skipped, row list unreadable ({e})");
            None
        }
    };
    let outcome = hygiene::sweep(&data_dir(), keep.as_deref(), std::time::SystemTime::now());
    if outcome.removed() > 0 {
        log::info!(
            "YV73 hygiene: removed {} file(s) ({:.1} MB) — downloads={} recovery={} temp={} logs={}",
            outcome.removed(),
            outcome.bytes as f64 / (1024.0 * 1024.0),
            outcome.downloads,
            outcome.recovery,
            outcome.temp,
            outcome.logs
        );
    }
}

// --- Commands ---

/// Write settings.json (the single on-disk copy every settings writer goes
/// through — `save_settings` and the YV31 model commands).
fn persist_settings(settings: &AppSettings) -> Result<(), String> {
    write_settings_file(&data_dir().join("settings.json"), settings)
}

/// YV41 durable settings write, split from `persist_settings` so it is testable
/// against a temp dir. Two guarantees:
///
/// 1. **Merge forward.** The frontend round-trips the whole `AppSettings`
///    object on every save, which silently drops any key this build does not
///    know about (e.g. one written by a newer version before a downgrade). Keys
///    already on disk that the serialized struct does not carry are re-added
///    here, so a save can never delete another build's settings.
/// 2. **Atomic.** The bytes land in a sibling `.tmp` file that is fsynced and
///    then renamed over the target, so a crash or power loss mid-write leaves
///    either the old file or the new one — never the truncated file that the
///    loader would have to quarantine.
pub fn write_settings_file(path: &Path, settings: &AppSettings) -> Result<(), String> {
    let mut value = serde_json::to_value(settings).map_err(|e| e.to_string())?;
    if let (Some(next), Some(prev)) = (
        value.as_object_mut(),
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .as_ref()
            .and_then(|v| v.as_object().cloned()),
    ) {
        for (key, stored) in prev {
            next.entry(key).or_insert(stored);
        }
    }
    let s = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;

    // Same directory as the target — rename is only atomic within a filesystem.
    let tmp = path.with_extension("json.tmp");
    let write_tmp = || -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(s.as_bytes())?;
        f.sync_all()
    };
    if let Err(e) = write_tmp() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("write {} failed: {e}", tmp.display()));
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("rename {} -> {} failed: {e}", tmp.display(), path.display())
    })
}

/// YV41 startup read of settings.json. The old `from_str(..).ok().unwrap_or_default()`
/// threw away EVERY setting — hotkey, model, companion tone, the onboarded flag —
/// on any single unparseable field. The rules now:
///
/// * missing file → defaults (fresh install, nothing to rewrite);
/// * whole-file garbage (not JSON, or not a JSON object) → defaults, with the
///   corrupt file kept aside as `settings.json.bak` (never silently deleted);
/// * one bad field → `salvage_settings` keeps every other stored field;
/// * an older schema → `apply_settings_migrations` upgrades it and rewrites the
///   file once, atomically.
pub fn load_settings(path: &Path) -> AppSettings {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return AppSettings::default();
    };
    let stored: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v @ serde_json::Value::Object(_)) => v,
        Ok(_) => {
            quarantine_settings(path, "settings.json is not a JSON object");
            return AppSettings::default();
        }
        Err(e) => {
            quarantine_settings(path, &format!("settings.json is not valid JSON ({e})"));
            return AppSettings::default();
        }
    };

    let mut settings: AppSettings = match serde_json::from_value(stored.clone()) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("settings.json did not parse as a whole ({e}); salvaging field by field");
            salvage_settings(&stored)
        }
    };

    if apply_settings_migrations(&mut settings, &stored) {
        match write_settings_file(path, &settings) {
            Ok(()) => log::info!(
                "settings migrated to schema v{CURRENT_SETTINGS_SCHEMA_VERSION} and rewritten"
            ),
            Err(e) => log::warn!("settings migration could not be persisted ({e})"),
        }
    }
    settings
}

/// Move an unusable settings.json aside instead of overwriting it — the user's
/// (or a support session's) only copy of what was on disk. Mirrors the DB's
/// corrupt-file quarantine in `Database::open`.
fn quarantine_settings(path: &Path, why: &str) {
    let bak = path.with_extension("json.bak");
    match std::fs::rename(path, &bak) {
        Ok(()) => log::error!(
            "{why}; kept the corrupt file at {} and starting from defaults",
            bak.display()
        ),
        Err(e) => log::error!("{why}; could not keep a .bak copy ({e}); starting from defaults"),
    }
}

/// Rebuild settings from a stored object that failed to deserialize as a whole.
/// Every stored field that is individually valid is kept; only the broken values
/// (a wrong type, or a value written by a newer build) fall back to their
/// default. One bad field can therefore never reset the rest of the config.
/// Ported from Handy's `salvage_settings` (their #1619).
fn salvage_settings(stored: &serde_json::Value) -> AppSettings {
    let Some(stored_map) = stored.as_object() else {
        log::warn!("stored settings are not a JSON object; falling back to defaults");
        return AppSettings::default();
    };

    let mut merged = serde_json::to_value(AppSettings::default())
        .expect("default settings serialize to a JSON object");

    for (key, value) in stored_map {
        let previous = merged
            .as_object_mut()
            .expect("merged settings stay an object")
            .insert(key.clone(), value.clone());
        if serde_json::from_value::<AppSettings>(merged.clone()).is_err() {
            // Log the key only — values can carry dictated text or paths.
            log::warn!("dropping invalid settings field '{key}', keeping its default");
            let map = merged
                .as_object_mut()
                .expect("merged settings stay an object");
            match previous {
                Some(previous) => map.insert(key.clone(), previous),
                None => map.remove(key),
            };
        }
    }

    serde_json::from_value(merged).unwrap_or_else(|e| {
        log::warn!("failed to reassemble salvaged settings ({e}); falling back to defaults");
        AppSettings::default()
    })
}

/// One-time forward migrations for stores written by an older schema. Returns
/// true when `settings` was changed and the file should be rewritten. Reads the
/// version off the RAW stored value: a store written before the field existed
/// has no `schemaVersion` key and is version 0, even though serde has already
/// defaulted the struct field to the current version.
///
/// Must stay idempotent — it runs on every launch until the rewrite lands.
pub fn apply_settings_migrations(settings: &mut AppSettings, stored: &serde_json::Value) -> bool {
    let stored_version = stored
        .get("schemaVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    if stored_version >= CURRENT_SETTINGS_SCHEMA_VERSION {
        return false;
    }

    // v0 → v1: a pre-YV41 store can name a model the bundled catalog does not
    // have — the retired Python-sidecar ids (YV34), or a newer catalog's model
    // after a downgrade. An unknown id can never be downloaded or loaded, so the
    // app would sit at "Model needed" forever with no way back; point it at the
    // catalog's recommendation once.
    //
    // GUARDED on `stored_version < 1` (Y4-A). Until this item there was only one
    // schema version, so "not current" and "v0" were the same thing and the arm
    // needed no guard. Raising CURRENT to 2 separates them: without this guard a
    // v1 store re-enters a v0 → v1 arm, and a user who deliberately selected a
    // model a later catalog stopped listing would have it silently reset to the
    // recommendation on the next launch.
    if stored_version < 1 && models::catalog_model(&settings.native_model).is_none() {
        log::warn!(
            "settings migration v{stored_version}→v{CURRENT_SETTINGS_SCHEMA_VERSION}: \
             model '{}' is not in the bundled catalog; selecting the recommended model",
            settings.native_model
        );
        settings.native_model = default_native_model();
    }

    // v1 → v2 (Y4-A): "light" never reaches `runs_format()`, so every store
    // carrying it dictates with no spoken punctuation, no lists and no email
    // shape. It is rewritten to "medium" UNCONDITIONALLY, because a v1 store
    // cannot tell a chosen "light" from the shipped one: `save_settings`
    // persists the whole `AppSettings` struct, so `cleanupLevel` is present in
    // every store the moment onboarding saves, and v1 records no provenance
    // anywhere. From v2 on, `cleanup_level_set_by_user` records it, and a v2
    // store never re-enters this function at all (the early return above).
    //
    // Only "light" moves. "none" is verbatim mode — a real user need — and
    // "high" is already past the gate.
    if stored_version < 2 && settings.cleanup_level.trim().eq_ignore_ascii_case("light") {
        log::info!(
            "settings migration v{stored_version}→v{CURRENT_SETTINGS_SCHEMA_VERSION}: \
             Auto-Cleanup \"light\" never reached the formatting stage; raising it to \"medium\""
        );
        settings.cleanup_level = "medium".into();
        settings.formatting_notice_pending = true;
    }

    settings.schema_version = CURRENT_SETTINGS_SCHEMA_VERSION;
    true
}

/// yap-p0-insertion-bridge-fallback — the Copy card the pill should show (a
/// pill webview that mounts after `insert-copy-card` was emitted reads it here).
#[tauri::command]
fn pending_copy_card() -> Option<paste::CopyCard> {
    float_pill::pending_copy_card()
}

/// The pill's Copy card was dismissed: the pill goes back to following
/// `show_floating_pill`.
#[tauri::command]
fn dismiss_copy_card(app: AppHandle, state: State<'_, Arc<AppState>>) {
    let show = state.settings.lock().show_floating_pill;
    // A take recording or still processing keeps its pill, whatever the setting.
    let in_flight = *state.recording.lock() || *state.busy.lock();
    float_pill::release_copy_card(&app, show, in_flight);
}

#[tauri::command]
fn get_settings(state: State<'_, Arc<AppState>>) -> AppSettings {
    state.settings.lock().clone()
}

/// Y4-H — the formatting screen asks the backend what the levels MEAN instead
/// of carrying its own copy of the answer. Every string here is built from
/// `dictation::CleanupLevel`'s `runs_*` predicates and `polish::POLISH_SPEEDS`,
/// so a pipeline change updates the screen on the same commit.
#[tauri::command]
fn formatting_options() -> formatting_options::FormattingOptions {
    formatting_options::formatting_options()
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    settings: AppSettings,
) -> Result<(), String> {
    let mut next = settings;
    // YV46: the legacy-migration flag is backend bookkeeping the UI never
    // carries — take it from the live settings so a save can't re-arm the
    // one-shot migration on the next launch.
    {
        let live = state.settings.lock();
        next.legacy_json_migrated = live.legacy_json_migrated;
        // Y4-A provenance: this is the command the Auto-Cleanup picker calls,
        // and a save that MOVES the level is the only evidence that a human
        // chose it. Once true it stays true — a later save of some unrelated
        // toggle must not un-choose it.
        next.cleanup_level_set_by_user =
            live.cleanup_level_set_by_user || next.cleanup_level != live.cleanup_level;
    }
    // Keep label in sync with binding
    next.hotkey_label = match next.ptt_binding.as_str() {
        "fn" => "fn".into(),
        "both" | "fn_or_fn_control" => "fn / fn⌃".into(),
        _ => "fn⌃".into(),
    };
    let dictate_changed = state.settings.lock().dictate_toggle != next.dictate_toggle;
    if dictate_changed {
        // Validate BEFORE persisting: a refused rebind is an error the
        // Settings capture shows, never a silently dead chord.
        if let Some(c) = next.dictate_toggle.as_ref() {
            hotkeys::chord_from_setting(c)?;
            // fix-249 BLOCKING 2: the dictate chord is system-wide too.
            if let Some(why) = hotkey_table::reserved_chord_reason(c, false) {
                return Err(why);
            }
        }
    }
    // fix-249 advisory (d): the three rebindable chords' NEW values must all
    // differ — refused here, before anything is registered or saved.
    {
        // `resolve` = the chord each action would actually hold (an invalid
        // stored value falls back to its default); a CHANGED invalid value is
        // refused with its own reason by `chord_for_action` below.
        let (dc, dl) = dictate_toggle_chord(&next);
        let (mc, ml) = hotkey_table::resolve(
            hotkey_table::BindingAction::RecordMeeting,
            next.meeting_chord.as_ref(),
        );
        let (cc, cl) =
            hotkey_table::resolve(hotkey_table::BindingAction::Cancel, next.cancel_chord.as_ref());
        hotkey_table::check_distinct(&[
            (hotkey_table::BindingAction::DictateToggle, dc, dl),
            (hotkey_table::BindingAction::RecordMeeting, mc, ml),
            (hotkey_table::BindingAction::Cancel, cc, cl),
        ])?;
    }
    // fix-249 advisory (b): VALIDATE everything first (nothing mutated yet),
    // PERSIST, then APPLY the chords; an apply that fails rolls back every
    // chord this save already swapped AND the persisted file, so the live
    // chords always match the stored settings.
    let (meeting_changed, cancel_changed) = {
        let live = state.settings.lock();
        (live.meeting_chord != next.meeting_chord, live.cancel_chord != next.cancel_chord)
    };
    let new_meeting = if meeting_changed {
        Some(match next.meeting_chord.as_ref() {
            Some(c) => hotkey_table::chord_for_action(hotkey_table::BindingAction::RecordMeeting, c)?,
            None => hotkey_table::resolve(hotkey_table::BindingAction::RecordMeeting, None),
        })
    } else {
        None
    };
    let new_cancel = if cancel_changed {
        Some(match next.cancel_chord.as_ref() {
            Some(c) => hotkey_table::chord_for_action(hotkey_table::BindingAction::Cancel, c)?,
            None => hotkey_table::resolve(hotkey_table::BindingAction::Cancel, None),
        })
    } else {
        None
    };
    if new_cancel.is_some() && *state.recording.lock() {
        return Err("finish the current take, then change the cancel key".into());
    }
    let prev = state.settings.lock().clone();
    persist_settings(&next)?;
    *state.settings.lock() = next.clone();
    let rollback = |why: String, dictate_applied: bool, meeting_applied: Option<(tauri_plugin_global_shortcut::Shortcut, String)>| -> String {
        if let Some((old_chord, old_label)) = meeting_applied {
            let new_chord = hotkey_table::active_meeting().0;
            let gs = app.global_shortcut();
            if let Err(e) = hotkey_table::swap_chord(
                new_chord,
                old_chord,
                &|s| gs.is_registered(s),
                &mut |s| gs.register(s).map_err(|e| e.to_string()),
                &mut |s| gs.unregister(s).map_err(|e| e.to_string()),
            ) {
                log::warn!("record-meeting rollback to {old_label} failed: {e}");
            }
            hotkey_table::set_active_meeting((old_chord, old_label));
        }
        if dictate_applied {
            if let Err(e) = rebind_dictate_toggle(&app, &prev) {
                log::warn!("dictate chord rollback failed: {e}");
            }
        }
        if let Err(e) = persist_settings(&prev) {
            log::warn!("settings rollback persist failed: {e}");
        }
        *state.settings.lock() = prev.clone();
        why
    };
    // 1. dictate chord — new registered BEFORE the old is dropped.
    let mut dictate_applied = false;
    if dictate_changed && smoke::global_hotkeys_allowed() {
        if let Err(e) = rebind_dictate_toggle(&app, &next) {
            return Err(rollback(e, false, None));
        }
        dictate_applied = true;
    }
    // 2. Record meeting — atomic swap; runs on the command thread, never
    // inside the shortcut plugin's handler (yap-p0-chord-deadlock).
    let mut meeting_applied = None;
    if let Some((new_chord, new_label)) = new_meeting {
        let old = hotkey_table::active_meeting();
        if smoke::global_hotkeys_allowed() {
            let gs = app.global_shortcut();
            let r = hotkey_table::swap_chord(
                old.0,
                new_chord,
                &|s| gs.is_registered(s),
                &mut |s| gs.register(s).map_err(|e| e.to_string()),
                &mut |s| gs.unregister(s).map_err(|e| e.to_string()),
            );
            if let Err(e) = r {
                return Err(rollback(
                    format!("{new_label} could not be registered ({e}) — the old chord stays"),
                    dictate_applied,
                    None,
                ));
            }
            hotkey_table::set_meeting_status(hotkeys::CarbonStatus::Registered);
        }
        log::info!("record-meeting chord → {new_label}");
        hotkey_table::set_active_meeting((new_chord, new_label));
        meeting_applied = Some(old);
    }
    // 3. Cancel — registered per take. fix-249 advisory (c): the recording
    // lock is HELD across the check and the set, so a take cannot start in
    // between; and the release drops the key a take actually armed
    // (`hotkey_table::cancel_to_release`), so a rebind can never leave Esc
    // registered while idle.
    if let Some(v) = new_cancel {
        let recording = state.recording.lock();
        if *recording {
            drop(recording);
            return Err(rollback(
                "finish the current take, then change the cancel key".into(),
                dictate_applied,
                meeting_applied,
            ));
        }
        log::info!("cancel key → {}", v.1);
        hotkey_table::set_active_cancel(v);
        drop(recording);
    }
    // fix-249 advisory (e): the tray item shows the live Record meeting chord.
    if meeting_applied.is_some() {
        if let Some(item) = state.tray_meeting.lock().as_ref() {
            if let Err(e) = item.set_accelerator(Some(hotkey_table::meeting_accelerator(
                next.meeting_chord.as_ref(),
            ))) {
                log::warn!("tray meeting accelerator update failed: {e}");
            }
        }
    }
    // Tell the floating pill to re-read (e.g. switch classic ↔ yappy live, or
    // re-align itself to a new dock edge).
    let _ = app.emit("settings", &next);
    // YV53: the dock edge moves the NSPanel itself, so it must land before the
    // show below re-parks — and `reposition` covers the still-recording case
    // where the pill stays up without a show call.
    float_pill::set_position(float_pill::PillPosition::from_settings(&next.pill_position));
    float_pill::reposition(&app);
    if next.show_floating_pill {
        float_pill::show_float(&app)?;
    } else if !*state.recording.lock() {
        float_pill::hide_float(&app);
    }
    #[cfg(target_os = "macos")]
    {
        ptt_macos::set_binding(ptt_macos::PttBinding::from_settings(&next.ptt_binding));
        ptt_macos::set_command_binding(ptt_macos::CommandBinding::from_settings(
            &next.command_binding,
        ));
    }
    // YV42: the login item follows the toggle immediately, not on next launch.
    apply_autostart(&app, next.autostart);
    Ok(())
}

/// YV65 — the pill webview reports the VISIBLE capsule's rect (window-logical
/// points). The float window is much larger than the capsule and stays
/// click-through; only this rect turns the cursor on, so the pill is grabbable
/// without the transparent margin swallowing anything.
#[tauri::command]
fn pill_set_hitbox(x: f64, y: f64, w: f64, h: f64) {
    float_pill::set_hitbox(x, y, w, h);
}

/// YV65 — pointer-down on the capsule: latch the panel origin and suspend the
/// re-park loop for the duration of the gesture.
#[tauri::command]
fn pill_drag_start(app: AppHandle) {
    float_pill::drag_start(&app);
}

/// YV65 — live move. `dx`/`dy` are the cursor's total travel since
/// `pill_drag_start`, in logical points.
#[tauri::command]
fn pill_drag_move(app: AppHandle, dx: f64, dy: f64) {
    float_pill::drag_move(&app, dx, dy);
}

/// YV65 — pointer-up: snap to the nearest dock and PERSIST it, so a drag is the
/// same durable change the Settings picker makes (same store, same "settings"
/// event, so the picker and the pill both follow). `snap` is false for a press
/// that never became a drag — the pill just re-parks where it was.
#[tauri::command]
fn pill_drag_end(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    x: f64,
    y: f64,
    snap: bool,
) -> Result<(), String> {
    let Some(pos) = float_pill::drag_end(&app, (x, y), snap) else {
        return Ok(());
    };
    let next = {
        let mut settings = state.settings.lock();
        if settings.pill_position == pos.as_settings() {
            return Ok(()); // dropped back on the dock it already had
        }
        settings.pill_position = pos.as_settings().to_string();
        settings.clone()
    };
    persist_settings(&next)?;
    let _ = app.emit("settings", &next);
    log::info!("pill dragged → {} dock", next.pill_position);
    Ok(())
}

#[tauri::command]
fn get_history(
    state: State<'_, Arc<AppState>>,
    query: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<TranscriptEntry>, String> {
    state.db.list_transcripts(limit.unwrap_or(200), query)
}

#[tauri::command]
fn clear_history(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.db.clear_transcripts()
}

#[tauri::command]
fn delete_entry(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state.db.delete_transcript(&id)
}

#[tauri::command]
fn get_status(state: State<'_, Arc<AppState>>) -> AppStatus {
    build_status(&state)
}

#[tauri::command]
fn get_permissions(state: State<'_, Arc<AppState>>) -> PermissionReport {
    // YV33/YV34: the ASR row reports the engine that actually runs — a
    // downloaded embedded model. Nothing here spawns a process, so the 1.2 s
    // onboarding poll is now free (it used to exec the CLT python shim and pop
    // a developer-tools dialog on every tick).
    let native = state.settings.lock().native_model.clone();
    permissions::report(false, native_model_ready(&native).is_some())
}

#[tauri::command]
fn request_accessibility() -> bool {
    permissions::request_accessibility_prompt()
}

/// yap-p0-perm-repair — one-click Repair for a stale or denied grant:
/// `tccutil reset <Service> <Yap's own bundle id>` (Accessibility also resets
/// PostEvent), the system prompt, the exact pane, then a re-probe every 1 s for
/// 120 s that publishes `permission_repair` on each phase change. `async` so the
/// tccutil exec never runs on the main thread.
#[tauri::command]
async fn repair_permission(
    app: AppHandle,
    service: String,
) -> Result<perm_probe::RepairOutcome, String> {
    let svc = perm_probe::Service::parse(&service)
        .ok_or_else(|| format!("unknown permission service: {service}"))?;
    let outcome = perm_probe::repair(svc);
    if outcome.ok {
        let _ = std::thread::Builder::new()
            .name("perm-repair-reprobe".into())
            .spawn(move || {
                perm_probe::reprobe(
                    svc,
                    || perm_probe::probe_service(svc),
                    std::thread::sleep,
                    |phase| {
                        log::info!("perm-repair: {:?} -> {:?}", svc, phase);
                        let _ = app.emit(
                            perm_probe::REPAIR_EVENT,
                            perm_probe::RepairEvent::new(svc, phase),
                        );
                        if phase != perm_probe::RepairPhase::Waiting {
                            permissions::poke(permissions::WatchReason::Repair);
                        }
                    },
                );
            });
    }
    Ok(outcome)
}

/// yap-p0-perm-repair — Input Monitoring only takes effect after a quit and
/// reopen; the Repair flow's Relaunch button lands here.
#[tauri::command]
fn relaunch_app(app: AppHandle) {
    app.restart()
}

/// Ask macOS for the microphone — and RETURN, without waiting for the answer.
///
/// PERM-A. `async` so Tauri runs it off the main thread, and non-blocking even
/// there: the system dialog is a human decision, and the completion handler
/// arrives on an arbitrary dispatch queue. The return value is the status *at
/// the moment of asking* (`"not_determined"` when the dialog is about to
/// appear); the real outcome is emitted as `microphone-status` when TCC answers.
#[tauri::command(async)]
fn request_microphone(app: AppHandle) -> String {
    let before = mic_auth::authorization_status();
    mic_auth::request_access(move |status| {
        log::info!("mic auth: TCC answered {}", status.as_str());
        let _ = app.emit("microphone-status", status.as_str());
    });
    before.as_str().to_string()
}

/// The authoritative microphone status, as a string the UI branches on.
/// Pure: reading never prompts, so the permissions pane may poll it.
#[tauri::command]
fn microphone_status() -> String {
    mic_auth::authorization_status().as_str().to_string()
}

#[tauri::command]
fn get_insights(state: State<'_, Arc<AppState>>) -> Result<Insights, String> {
    state.db.insights()
}

/// Contiguous, zero-filled day-by-day words/sessions series (oldest first) for the
/// last `days` days. Feeds the Insights daily bar chart + the activity heatmap.
#[tauri::command]
fn daily_series(state: State<'_, Arc<AppState>>, days: i64) -> Result<Vec<DayCount>, String> {
    state.db.daily_series(days)
}

/// Contiguous, zero-filled month-by-month rollup (oldest first, "YYYY-MM") for the
/// last `months` months. Feeds the Insights monthly bar chart / long-range views.
#[tauri::command]
fn monthly_series(state: State<'_, Arc<AppState>>, months: i64) -> Result<Vec<DayCount>, String> {
    state.db.monthly_series(months)
}

#[tauri::command]
fn list_dictionary(state: State<'_, Arc<AppState>>) -> Result<Vec<DictEntry>, String> {
    state.db.list_dictionary()
}

#[tauri::command]
fn add_dictionary_term(
    state: State<'_, Arc<AppState>>,
    term: String,
    preferred: Option<String>,
) -> Result<DictEntry, String> {
    state.db.add_dictionary_term(term, preferred)
}

#[tauri::command]
fn update_dictionary_term(
    state: State<'_, Arc<AppState>>,
    id: String,
    term: String,
    preferred: Option<String>,
) -> Result<(), String> {
    state.db.update_dictionary_term(&id, term, preferred)
}

/// YV47 — pin a term to "always bias": it heads the ranking, survives the
/// harvest purge, and is the last thing dropped from the decoder prompt.
#[tauri::command]
fn set_dictionary_starred(
    state: State<'_, Arc<AppState>>,
    id: String,
    starred: bool,
) -> Result<(), String> {
    state.db.set_dictionary_starred(&id, starred)
}

#[tauri::command]
fn delete_dictionary_term(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state.db.delete_dictionary_term(&id)
}

/// YV47 — apply a "Fix transcription" edit: rewrite the stored transcript and
/// mine the words the user changed into dictionary candidates.
#[tauri::command]
fn correct_transcript(
    state: State<'_, Arc<AppState>>,
    id: String,
    text: String,
) -> Result<usize, String> {
    state.db.record_correction(&id, &text).map(|c| c.len())
}

/// YV52 — takes whose transcription failed and whose audio is still on disk.
/// Expired ones are purged (audio included) before the list is built, so the
/// UI can never offer a retry against a clip retention already removed.
#[tauri::command]
fn list_failed_dictations(state: State<'_, Arc<AppState>>) -> Result<Vec<FailedDictation>, String> {
    purge_expired_failed_takes(&state.db);
    state.db.list_failed_dictations()
}

/// DB-B — what a recovered take decodes on "Finish transcribing", and the
/// transcript that comes out of it. This is the whole resume decision of
/// `retry_failed_dictation`, lifted out so a test can drive it with a decoder
/// it controls; the command calls it with `transcribe_native`.
///
/// A resume only happens when BOTH halves of it are known: non-empty recovered
/// words AND a positive sample offset they end at. Anything else — the offset
/// could not be read (busy or poisoned DB), it reads 0, or there are no words —
/// decodes the whole clip with NO prefix. That is the only safe fallback:
/// keeping the prefix while decoding from 0 would put every recovered word in
/// front of a re-decode that contains them again, and the retry deletes the
/// only audio straight after, so a doubled transcript could never be redone.
///
/// When the remainder IS decoded and comes back blank (`EMPTY_TRANSCRIPT`: the
/// take died on its last windows while the user paused, or the tail is too
/// short to say anything), the recovered words are the whole take. Treating
/// that as a failure would make the take unfinishable — every press would fail
/// the same way until the retention sweep deleted the words with the audio.
fn finish_recovered_decode(
    partial_text: Option<String>,
    resume_sample: Result<i64, String>,
    samples: &[f32],
    decode: impl FnOnce(Vec<f32>) -> Result<transcription::AsrOutput, String>,
) -> Result<(String, transcription::AsrOutput), String> {
    let prefix = partial_text
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string);
    let resume_at = match (&prefix, resume_sample) {
        (Some(_), Ok(n)) if n > 0 => Some(n as usize),
        (Some(_), Ok(_)) => {
            log::warn!("recovered take has words but no resume point — decoding it whole");
            None
        }
        (Some(_), Err(e)) => {
            log::warn!("recovered take's resume point unreadable ({e}) — decoding it whole");
            None
        }
        (None, _) => None,
    };
    let (prefix, start) = match resume_at {
        Some(n) => (prefix, n),
        None => (None, 0),
    };
    let remaining: Vec<f32> = samples
        .get(start..)
        .map(<[f32]>::to_vec)
        .unwrap_or_default();
    // A take whose chunks covered the whole clip has nothing left to decode —
    // the persisted words ARE the take. Skip the engine rather than handing it
    // an empty buffer; the caller still runs the gate and the cleanup.
    let asr = if remaining.is_empty() {
        recovered_only_output()
    } else {
        match decode(remaining) {
            Err(e) if prefix.is_some() && e == EMPTY_TRANSCRIPT => recovered_only_output(),
            other => other?,
        }
    };
    let raw_text = match prefix {
        Some(prefix) => {
            let tail = asr.text.trim();
            if tail.is_empty() {
                prefix
            } else {
                format!("{prefix} {tail}")
            }
        }
        None => asr.text.clone(),
    };
    Ok((raw_text, asr))
}

/// The `AsrOutput` of a resume whose words all came off disk: nothing was
/// decoded, so nothing is attributed to the engine and nothing degraded.
fn recovered_only_output() -> transcription::AsrOutput {
    transcription::AsrOutput {
        text: String::new(),
        backend: "recovered".to_string(),
        seconds: 0.0,
        load_ms: 0,
        decode_ms: 0,
        degraded: false,
        chunks: 0,
        chunks_failed: 0,
        degraded_reason: None,
    }
}

/// What one retry decode means for a failed row. Decided in ONE place,
/// [`retry_outcome`]; every retry path goes through it.
enum RetryOutcome {
    /// The take is complete: the caller converts the row and deletes the clip.
    Convert(String, transcription::AsrOutput),
    /// The row and the clip stay. `error` becomes the row's reason and
    /// `attempt_text` its saved words ("Transcribed so far").
    Keep {
        error: String,
        attempt_text: Option<String>,
    },
}

fn word_count(t: Option<&str>) -> usize {
    t.map_or(0, |t| t.split_whitespace().count())
}

/// Words compared ignoring case and surrounding punctuation ("World," = "world").
fn norm_words(t: &str) -> Vec<String> {
    t.split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// True when `new` EXTENDS `old`: it starts with all of `old`'s words and adds more.
fn extends(new: &str, old: &str) -> bool {
    let (new, old) = (norm_words(new), norm_words(old));
    new.len() > old.len() && new.starts_with(&old)
}

/// Saved text as the retry sees it: cut at a repetition loop
/// (`dictation::degenerate_cutoff`), whatever is left before it. The words
/// recovered from the live decode were never gated, and a loop in them must
/// neither inflate the "not shorter" baseline nor be converted or kept.
fn gated_words(t: &str) -> String {
    match dictation::degenerate_cutoff(t) {
        Some(cutoff) => t[..cutoff].trim().to_string(),
        None => t.trim().to_string(),
    }
}

/// THE ROW'S SAVED WORDS, as the invariant, the extend-only rule and "Keep
/// what was transcribed" all see them: the GATED saved attempt text and the
/// GATED recovered words (`partial_text`, on every row that has them — also
/// when a retry fell back to decoding the whole clip), whichever extends the
/// other, else the longer. Keep therefore never saves less than the recovered
/// words.
fn saved_words(row: &db::FailedDictation) -> Option<String> {
    let gate = |t: &Option<String>| t.as_deref().map(gated_words).filter(|t| !t.is_empty());
    match (gate(&row.attempt_text), gate(&row.partial_text)) {
        (Some(attempt), Some(recovered)) => {
            if extends(&recovered, &attempt)
                || (!extends(&attempt, &recovered)
                    && word_count(Some(&recovered)) > word_count(Some(&attempt)))
            {
                Some(recovered)
            } else {
                Some(attempt)
            }
        }
        (attempt, recovered) => attempt.or(recovered),
    }
}

/// THE RETRY INVARIANT (PR #216). A retry deletes the recovery clip on its own
/// only when ALL hold:
///   (a) this decode had ZERO failed windows (engine error, per-window timeout,
///       the take deadline, or an error before any window decoded),
///   (b) the hallucination gate cut NOTHING (`gate_cut` is false): not in the
///       recovered words, not in the remainder, not across the seam. A cut
///       means audio whose words are not in the text — the retry resumes after
///       the last recovered chunk, so speech under a looped chunk is never
///       re-decoded — and that counts exactly like a failed window, and
///   (c) its text is NOT SHORTER, in words, than the row's saved words
///       ([`saved_words`]: the gated attempt text or gated recovered words).
/// Anything else keeps the row and the clip; only the user's confirmed "Keep
/// what was transcribed" or "Discard" deletes them.
///
/// `decoded` is ALWAYS gated text: [`decode_failed_take`] gates the recovered
/// words and the decoded remainder SEPARATELY, then the joined text, so (b)
/// counts the words that would really be converted and a loop in the recovered
/// words cannot throw away a clean remainder; whether any of those gates cut
/// anything arrives as `gate_cut`.
///
/// The saved words follow [`next_attempt_text`]: a zero-failure decode that
/// is SHORTER than the saved words replaces them only when the stored attempt
/// text is itself degenerate (`dictation::degenerate_cutoff`, a repetition
/// loop); otherwise, and for any decode with a failed window, they may only be
/// extended, never shrunk.
fn retry_outcome(
    row: &db::FailedDictation,
    decoded: Result<(String, transcription::AsrOutput), String>,
    gate_cut: bool,
) -> RetryOutcome {
    let saved = saved_words(row);
    let saved_count = word_count(saved.as_deref());
    let (text, asr) = match decoded {
        Ok(done) => done,
        // Nothing new decoded; the saved words (at least the gated recovered
        // words) are the words so far.
        Err(error) => {
            return RetryOutcome::Keep {
                attempt_text: saved,
                error,
            }
        }
    };
    let failed = asr.chunks_failed > 0 || gate_cut;
    let count = word_count(Some(&text));
    if !failed && count >= saved_count {
        return RetryOutcome::Convert(text, asr);
    }
    // A clean, shorter decode may replace the saved words ONLY when they are a
    // repetition loop. Fewer words after a model switch, or a blank remainder
    // (`recovered_only_output`), is no evidence against words an earlier
    // attempt got: those stay, and so does the clip.
    let saved_is_degenerate = row
        .attempt_text
        .as_deref()
        .is_some_and(|t| dictation::degenerate_cutoff(t).is_some());
    let authoritative = !failed && !asr.text.trim().is_empty() && saved_is_degenerate;
    let error = if gate_cut {
        "Part of this take repeated itself and could not be transcribed. Nothing was deleted — keep what was transcribed, or discard the take.".to_string()
    } else if failed {
        format!(
            "Part of this take didn't transcribe ({} of {} sections failed). The audio is kept — try again, or keep what was transcribed.",
            asr.chunks_failed,
            asr.chunks.max(asr.chunks_failed)
        )
    } else {
        format!(
            "This try got fewer words than before ({count} vs {saved_count}). Nothing was deleted — keep what was transcribed, or discard the take."
        )
    };
    RetryOutcome::Keep {
        attempt_text: next_attempt_text(saved.as_deref(), Some(text.trim()), authoritative),
        error,
    }
}

/// The row's saved words after an attempt that did not convert: an
/// authoritative decode (zero failures, replacing a degenerate saved text)
/// replaces them; any other attempt only EXTENDS them ([`extends`]: case and
/// punctuation ignored) and otherwise leaves them as they were.
fn next_attempt_text(
    saved: Option<&str>,
    candidate: Option<&str>,
    authoritative: bool,
) -> Option<String> {
    let Some(candidate) = candidate.filter(|c| !c.trim().is_empty()) else {
        return saved.map(str::to_string);
    };
    let Some(saved) = saved.filter(|s| !s.trim().is_empty()) else {
        return Some(candidate.to_string());
    };
    if authoritative || extends(candidate, saved) {
        Some(candidate.to_string())
    } else {
        Some(saved.to_string())
    }
}

/// YV66 — the retry path's hallucination gate, the same one the live path
/// runs. It matters MORE here: a successful retry unlinks the recovery wav, so
/// a degenerate tail that slips through is unrecoverable — there is no audio
/// left to try again with — and a good prefix thrown away is gone for the same
/// reason. Runs on the COMBINED text (recovered words + decoded remainder)
/// before [`retry_outcome`], and on the kept words of "Keep what was
/// transcribed".
///
/// `Ok(true)`: a degenerate tail was cut; `Ok(false)`: nothing to cut; `Err`:
/// degenerate from the first token, nothing to keep.
fn retry_gate(raw_text: &mut String) -> Result<bool, String> {
    match dictation::degenerate_cutoff(raw_text) {
        Some(cutoff)
            if raw_text[..cutoff].split_whitespace().count()
                >= dictation::DEGEN_MIN_KEEP_TOKENS =>
        {
            // YV20/M2: count only, never log the transcript body.
            let before = raw_text.chars().count();
            raw_text.truncate(cutoff);
            let kept = raw_text.chars().count();
            log::info!(
                "retry hallucination gate: truncated degenerate tail (kept {} chars, dropped {} chars)",
                kept,
                before - kept
            );
            Ok(true)
        }
        Some(_) => {
            // Degenerate from the first token — nothing to keep, so the row and
            // its wav stay put for another attempt.
            // YV20/M2: count only, never log the transcript body.
            log::info!(
                "retry hallucination gate: rejected degenerate output ({} chars)",
                raw_text.chars().count()
            );
            Err("Yap couldn't make sense of that clip — try dictating it again".into())
        }
        None => Ok(false),
    }
}

/// DB-B / YV52 — the decode half of a retry, lifted out of the command so a
/// test can drive it with a real windowed decode and a real database. `Ok`
/// only when [`retry_outcome`] says the take is complete; the caller then
/// converts the row and deletes the clip ([`convert_and_drop_clip`]). On `Err`
/// the row and the clip stay, with the row's reason and saved words updated.
fn decode_failed_take(
    db: &db::Database,
    row: &db::FailedDictation,
    partial_text: Option<String>,
    resume_sample: Result<i64, String>,
    samples: &[f32],
    decode: impl FnOnce(Vec<f32>) -> Result<transcription::AsrOutput, String>,
) -> Result<(String, transcription::AsrOutput), String> {
    // The recovered words and the remainder are gated SEPARATELY, then joined
    // (rr1 re-review of 2b09b0d: one gate over the joined text cut at a loop
    // inside the recovered words and threw the whole clean remainder away).
    // The joined text is gated once more for a loop across the seam. All of
    // it runs BEFORE the invariant judges the text, and ANY cut — or a gate
    // rejecting a part outright — is reported as `gate_cut` (rr1 re-review of
    // 8bf5225: the resume starts after the looped recovered chunk, so the
    // speech under the loop is in no text, and converting deleted it).
    let gate_cut = std::cell::Cell::new(false);
    let gate = |text: &mut String| match retry_gate(text) {
        Ok(cut) => gate_cut.set(gate_cut.get() || cut),
        Err(_) => {
            text.clear();
            gate_cut.set(true);
        }
    };
    let partial_text = partial_text.map(|t| {
        let gated = gated_words(&t);
        if gated != t.trim() {
            gate_cut.set(true);
        }
        gated
    });
    let decoded = finish_recovered_decode(partial_text, resume_sample, samples, |rest| {
        decode(rest).map(|mut asr| {
            gate(&mut asr.text);
            asr
        })
    })
    .map(|(mut text, asr)| {
        gate(&mut text);
        (text, asr)
    });
    match retry_outcome(row, decoded, gate_cut.get()) {
        RetryOutcome::Convert(text, asr) => Ok((text, asr)),
        RetryOutcome::Keep {
            error,
            attempt_text,
        } => {
            log::warn!("retry kept the clip and the row: {error}");
            let _ = db.note_failed_retry(&row.id, &error, attempt_text.as_deref());
            Err(error)
        }
    }
}

/// YV52 — the success half of a retry: the row converts into a history entry,
/// then the clip and the take's chunk text go. The ONLY place a retry deletes
/// a recovery wav, so a test can prove a failed decode never reaches it.
fn convert_and_drop_clip(
    db: &db::Database,
    row: &db::FailedDictation,
    wav: &Path,
    text: String,
    asr: transcription::AsrOutput,
    raw_text: Option<String>,
) -> Result<TranscriptEntry, String> {
    let entry = db.convert_failed_dictation(&row.id, text, asr.backend, asr.seconds, raw_text)?;
    // The take's audio now follows the normal lifecycle: it is gone. Its
    // partial text goes with it — the words live in `transcripts` now, and a
    // `take_chunks` row nothing points at is transcript text that would sit in
    // the DB until the retention sweep found it.
    if let Some(take_id) = row.take_id.as_deref() {
        let _ = db.clear_take_chunks(take_id);
    }
    let _ = std::fs::remove_file(wav);
    Ok(entry)
}

/// YV52 — re-run ASR on a failed take's preserved clip. The common cause of the
/// original failure is transient (no model downloaded yet, an engine that has
/// since reloaded), so this is a real second chance rather than a replay.
///
/// On success the row CONVERTS: the transcript lands in history under the
/// moment it was spoken, the recovery wav is unlinked, and the text is put on
/// the clipboard — it is never auto-pasted, because the user is in Yap now, not
/// in the app they dictated into.
///
/// `keep_partial` (PR #216 re-review, "Keep what was transcribed"): the user
/// chose to accept the words a failed attempt DID get (`attempt_text`) instead
/// of trying again. No decode runs; the row converts on those words and the
/// clip is deleted — the UI confirms that loss first. Without it, a window that
/// fails the same way on every try would leave only Discard.
#[tauri::command]
fn retry_failed_dictation(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
    keep_partial: Option<bool>,
) -> Result<TranscriptEntry, String> {
    let keep_partial = keep_partial.unwrap_or(false);
    // One closure, so every early `return Err` below lands in the label update.
    let result = (|| -> Result<TranscriptEntry, String> {
        let row = state
            .db
            .get_failed_dictation(&id)?
            .ok_or_else(|| "That recovery clip is no longer in the list".to_string())?;
        let wav = PathBuf::from(&row.wav_path);
        if !wav.exists() && !keep_partial {
            // The audio is gone (purged, or cleaned up outside the app) — drop the
            // row rather than leaving a Retry button that can only ever fail.
            // ("Keep what was transcribed" needs no audio, so it still converts.)
            let _ = state.db.delete_failed_dictation(&id);
            return Err("The audio for this take is no longer on disk".into());
        }
        let settings = state.settings.lock().clone();
        let (raw_text, asr) = if keep_partial {
            // The same saved words the invariant judged: gated, and never less
            // than the gated recovered words.
            let mut kept = saved_words(&row)
                .ok_or_else(|| "Nothing from this take has been transcribed yet".to_string())?;
            retry_gate(&mut kept)?;
            (kept, recovered_only_output())
        } else {
            let samples = record::read_wav_16k_mono(&wav)?;
            // DB-B — "Finish transcribing" is a RESUME, not a replay. A take
            // recovered mid-decode already has words in `take_chunks` and an
            // `end_sample` saying how far into its audio they go, so decode the
            // REMAINDER and keep what exists (`finish_recovered_decode`, called by
            // `decode_failed_take`).
            let Some((model_id, model_path)) = native_model_ready(&settings.native_model) else {
                return Err(
                    "No speech model installed — open Settings → Models and download one.".into(),
                );
            };
            let bias_prompt = match state.db.bias_terms(BIAS_TERM_LIMIT) {
                Ok(terms) => asr_engine::build_bias_prompt(&terms),
                Err(e) => {
                    log::warn!("dictionary bias unavailable ({e}) — retry decoding unbiased");
                    None
                }
            };
            // The recovered words and the freshly decoded remainder are ONE
            // transcript from here down. Everything after this line — the
            // hallucination gate, the dictionary, the cleanup pipeline, the polish
            // pass — sees exactly what it would have seen had the take never been
            // interrupted. A decode with ANY failed window comes back `Err` here,
            // so the row and the clip are never converted or deleted for it.
            let (partial_text, resume_sample) = match row.take_id.as_deref() {
                Some(take_id) => (
                    row.partial_text.clone(),
                    state.db.take_resume_sample(take_id),
                ),
                None => (None, Ok(0)),
            };
            decode_failed_take(
                &state.db,
                &row,
                partial_text,
                resume_sample,
                &samples,
                |remaining| {
                    transcribe_native(
                        &state.transcription,
                        &model_id,
                        &model_path,
                        remaining,
                        &settings.language,
                        bias_prompt,
                        // A retry or recovery from History has no live pill to fill:
                        // the user is looking at the history row, not at a recording
                        // indicator.
                        &mut transcribe_progress::NoObserver,
                    )
                },
            )?
        };
        if raw_text.trim().is_empty() {
            return Err("There is nothing left to transcribe in this take".into());
        }
        // Same cleanup pipeline as a live take, minus the caret context: the retry
        // happens in Yap, so there is no cursor to join onto.
        let mode = dictation::resolve_mode(
            &settings.dictation_mode,
            row.source_app.as_deref().unwrap_or_default(),
        );
        let polish_config = polish::PolishConfig::from_settings(&settings, mode);
        // A retry has no live paste target (the row stores the app's title, not
        // its bundle id), so it gets the conservative unknown-target format.
        let structure_format = dictation::structure_format_for_take(
            settings.structure_enabled && settings.dictation_mode.trim() != "plain",
            &settings.structure_app_overrides,
            "",
            mode,
        );
        let (text, _) = dictation::run_cleanup_structured_traced(
            &raw_text,
            dictation::CleanupLevel::from_setting(&settings.cleanup_level),
            mode,
            structure_format,
            polish_config.style,
            &[],
            |t| {
                state
                    .db
                    .apply_dictionary(t)
                    .unwrap_or_else(|_| t.to_string())
            },
            |t| polish::polish_llm(t, mode, &polish_config).ok_or("unknown"),
        );
        let entry =
            convert_and_drop_clip(&state.db, &row, &wav, text.clone(), asr, Some(raw_text))?;
        let outcome = paste::copy_and_maybe_paste(&app, &text, false, None);
        log::info!(
            "YV52 recovery: retry succeeded words={} ({})",
            entry.word_count,
            outcome.message
        );
        let _ = app.emit("transcript", &entry);
        Ok(entry)
    })();
    if let Err(e) = &result {
        // The row's reason is the CURRENT failure, not the original one (a no-op
        // when the row was converted or dropped).
        let _ = state.db.note_failed_retry(&id, e, None);
    }
    result
}

/// YV52 — throw a failed take away: the row and its audio both go.
#[tauri::command]
fn discard_failed_dictation(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    if let Some(path) = state.db.delete_failed_dictation(&id)? {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

/// YV47 — pending "Yap noticed: …" dictionary suggestions.
#[tauri::command]
fn list_dict_candidates(state: State<'_, Arc<AppState>>) -> Result<Vec<DictCandidate>, String> {
    state.db.list_dict_candidates(DICT_CANDIDATE_LIMIT)
}

#[tauri::command]
fn promote_dict_candidate(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<DictEntry, String> {
    state.db.promote_dict_candidate(&id)
}

#[tauri::command]
fn dismiss_dict_candidate(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state.db.dismiss_dict_candidate(&id)
}

/// YV48 — saved `trigger phrase → expansion` snippets (Settings → Snippets).
#[tauri::command]
fn list_snippets(state: State<'_, Arc<AppState>>) -> Result<Vec<Snippet>, String> {
    state.db.list_snippets()
}

#[tauri::command]
fn add_snippet(
    state: State<'_, Arc<AppState>>,
    trigger: String,
    expansion: String,
) -> Result<Snippet, String> {
    state.db.add_snippet(trigger, expansion)
}

#[tauri::command]
fn update_snippet(
    state: State<'_, Arc<AppState>>,
    id: String,
    trigger: String,
    expansion: String,
) -> Result<(), String> {
    state.db.update_snippet(&id, trigger, expansion)
}

#[tauri::command]
fn set_snippet_enabled(
    state: State<'_, Arc<AppState>>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    state.db.set_snippet_enabled(&id, enabled)
}

#[tauri::command]
fn delete_snippet(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state.db.delete_snippet(&id)
}

#[tauri::command]
fn list_scratch(state: State<'_, Arc<AppState>>) -> Result<Vec<ScratchNote>, String> {
    state.db.list_scratch()
}

#[tauri::command]
fn save_scratch(
    state: State<'_, Arc<AppState>>,
    id: Option<String>,
    title: String,
    body: String,
) -> Result<ScratchNote, String> {
    state.db.save_scratch(id, title, body)
}

#[tauri::command]
fn delete_scratch(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state.db.delete_scratch(&id)
}

#[tauri::command]
fn copy_entry(app: AppHandle, text: String) -> Result<(), String> {
    paste::copy_text(&app, &text)
}

/// Re-paste the most recent transcript — backs the tray "Paste Last Transcript"
/// item and the global ⌃⌘V shortcut (Wispr-parity). Reads the newest row
/// (`list_transcripts` is `ORDER BY created_at DESC`) and pastes it into the
/// current frontmost app. Like `paste_entry`, the CGEvent ⌘V hop BLOCKS the
/// main thread, so the paste MUST run off-main via `spawn_blocking` — calling
/// it directly from a main-thread menu/shortcut callback would deadlock. This
/// only copies+pastes; it does NOT insert a new history row (no duplicate).
fn paste_last_transcript(app: &AppHandle, state: &AppState) {
    let text = match state.db.list_transcripts(1, None) {
        Ok(entries) => match entries.into_iter().next() {
            Some(e) => e.text,
            None => {
                log::info!("paste last transcript: history is empty");
                return;
            }
        },
        Err(err) => {
            log::warn!("paste last transcript: db read failed: {err}");
            return;
        }
    };
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Explicit re-paste: the current frontmost app is the intended target,
        // so pass `None` (the source-app guard does not apply here).
        let o = paste::copy_and_maybe_paste(&app, &text, true, None);
        if !o.copied {
            log::warn!("paste last transcript failed: {}", o.message);
        }
    });
}

/// YV51 "Undo AI edit" — re-paste the RAW take over the polished one the user
/// just got. Backs the tray "Undo AI Edit" item and the global ⌃⌘Z shortcut.
///
/// Reads the newest row and hands the pair to [`dictation::undo_ai_edit_text`],
/// which is the single source of truth for "is there an edit to undo?" (the same
/// rule the tray enabled-state and the history button use). A `None` verdict is
/// a logged no-op — never a paste — so this can't blank the user's text when
/// Auto-Cleanup is off or the row predates the raw column. Like
/// `paste_last_transcript` the ⌘V hop blocks the main thread, so the paste MUST
/// run off-main via `spawn_blocking`, and no new history row is inserted.
fn paste_raw_last_transcript(app: &AppHandle, state: &AppState) {
    let entry = match state.db.list_transcripts(1, None) {
        Ok(entries) => entries.into_iter().next(),
        Err(err) => {
            log::warn!("undo ai edit: db read failed: {err}");
            return;
        }
    };
    let Some(entry) = entry else {
        log::info!("undo ai edit: history is empty");
        return;
    };
    let Some(raw) = dictation::undo_ai_edit_text(&entry.text, entry.raw_text.as_deref()) else {
        log::info!("undo ai edit: raw matches the pasted text — nothing to undo");
        return;
    };
    let raw = raw.to_string();
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Explicit re-paste: the current frontmost app is the intended target,
        // so pass `None` (the YV21 source-app guard does not apply here).
        let o = paste::copy_and_maybe_paste(&app, &raw, true, None);
        if !o.copied {
            log::warn!("undo ai edit failed: {}", o.message);
        }
    });
}

/// Y4-G — the user's local-only verdict on one take (1 up, -1 down, `None` to
/// clear). Stored in SQLite and used for NOTHING today.
///
/// It costs one nullable column now and it is the only way a future rules
/// change can be scored against real dissatisfaction rather than a guess. It is
/// never transmitted: this command writes one row and returns, there is no
/// network call on the path, and this app ships no analytics SDK.
#[tauri::command]
fn set_take_feedback(
    state: tauri::State<'_, AppState>,
    id: String,
    feedback: Option<i64>,
) -> Result<(), String> {
    state.db.set_take_feedback(&id, feedback)
}

#[tauri::command]
async fn paste_entry(app: AppHandle, text: String) -> Result<String, String> {
    // MUST be async → runs OFF the main thread. copy_and_maybe_paste hops to the
    // main thread for the CGEvent ⌘V and blocks waiting on it; a *sync* command
    // runs ON main and would deadlock waiting for a closure queued to that same
    // thread (3s freeze, then "paste timed out"). spawn_blocking keeps the async
    // runtime unblocked while the short main-thread hop completes.
    tauri::async_runtime::spawn_blocking(move || {
        // Explicit user re-paste of a stored entry: the current frontmost app IS
        // the intended target, so pass `None` — the YV21 source-app guard does
        // not apply here.
        let o = paste::copy_and_maybe_paste(&app, &text, true, None);
        if o.copied {
            Ok(o.message)
        } else {
            Err(o.message)
        }
    })
    .await
    .map_err(|e| format!("paste task join failed: {e}"))?
}

#[tauri::command]
fn open_data_dir() -> Result<(), String> {
    std::process::Command::new("open")
        .arg(data_dir())
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Export diagnostics (YV7): reveal the rotating logs folder (`data_dir/logs/`)
/// so users can attach yap.log to a support/bug report. Reuses the open pattern.
///
/// YV64: the export now also drops `crash-summary.txt` next to the logs, so a
/// report carries WHAT crashed rather than only the log tail. Writing it is
/// best-effort — a failed write still opens the folder, which is the action the
/// user asked for. It is written to the local logs folder and nowhere else.
#[tauri::command]
fn open_logs_dir(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let dir = logging::logs_dir(&data_dir());
    match state.db.list_crash_events(db::CRASH_EVENT_LIMIT) {
        Ok(events) => {
            let summary = crash::summary_text(&events);
            if let Err(e) = std::fs::write(dir.join("crash-summary.txt"), summary) {
                log::warn!("crash summary not written: {e}");
            }
        }
        Err(e) => log::warn!("crash summary skipped: {e}"),
    }
    std::process::Command::new("open")
        .arg(dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// YV64 — the recorded crashes, newest first, for Settings → Privacy &
/// Diagnostics → Stability.
#[tauri::command]
fn list_crash_events(state: State<'_, Arc<AppState>>) -> Result<Vec<CrashEvent>, String> {
    state.db.list_crash_events(db::CRASH_EVENT_LIMIT)
}

/// YV64 — the user has seen the list; stop treating those crashes as news.
#[tauri::command]
fn acknowledge_crash_events(state: State<'_, Arc<AppState>>) -> Result<usize, String> {
    state.db.acknowledge_crash_events()
}

/// YV64 — drop Yap's stored crash history.
#[tauri::command]
fn clear_crash_events(state: State<'_, Arc<AppState>>) -> Result<usize, String> {
    state.db.clear_crash_events()
}

// --- YV98: the crash-report button ---------------------------------------
//
// Two commands, in a deliberate order. `preview_support_bundle` BUILDS the
// bundle (redacting as it goes), caches it, and returns the real contents for
// the sheet. `send_support_bundle` writes those cached bytes and tries the
// compose window, falling back to reveal + clipboard. Nothing here opens a
// socket — see `support.rs`'s module docs and the test that enforces it.

/// Build the bundle and hand back exactly what is inside it.
///
/// Async on purpose: it must NOT run on the main thread, because the compose
/// probe below (`canPerformWithItems:`) is AppKit and has to be dispatched TO
/// the main thread with a result coming back. A sync command would already be
/// there and would deadlock on its own answer.
#[tauri::command]
async fn preview_support_bundle(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<support::BundlePreview, String> {
    let version = env!("CARGO_PKG_VERSION");
    let home = home_dir();
    let os = support::os_version();

    let crash_summary = match state.db.list_crash_events(db::CRASH_EVENT_LIMIT) {
        Ok(rows) => crash::summary_text(&rows),
        // A DB that will not read is itself a fact worth shipping.
        Err(e) => format!("crash summary unavailable: {e}\n"),
    };
    let native = state.settings.lock().native_model.clone();
    let permissions = support::permissions_block(&permissions::report(
        false,
        native_model_ready(&native).is_some(),
    ));

    let prepared = support::prepare(
        version,
        support::BundleInputs {
            crash_summary,
            logs: support::read_logs(&logging::logs_dir(&data_dir())),
            environment: support::environment_block(&os, std::env::consts::ARCH),
            permissions,
            models: support::models_block(&native),
            username: support::username_from_home(&home),
            generated_at: chrono::Utc::now(),
        },
    );

    // Ask AppKit whether a compose window is even achievable, so the sheet can
    // say which of the two paths the button will take BEFORE it is pressed.
    let mail_available = on_main_thread(&app, {
        let subject = support::subject_line(version, "");
        move || support::compose_email(None, &subject, "", true)
    })
    .unwrap_or(false);

    let preview = prepared.preview(mail_available);
    *state.support_bundle.lock() = Some(prepared);
    Ok(preview)
}

/// Write the previewed bundle to the Desktop, then compose — or reveal.
#[tauri::command]
async fn send_support_bundle(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<support::SendOutcome, String> {
    let prepared = state
        .support_bundle
        .lock()
        .clone()
        .ok_or_else(|| "Nothing to send — preview the report first.".to_string())?;

    let path = support::desktop_dir(&home_dir()).join(&prepared.file_name);
    support::write_zip(&path, &prepared.entries, prepared.generated_at)
        .map_err(|e| format!("Could not write the diagnostics file: {e}"))?;
    log::info!(
        "support bundle written ({} entries)",
        prepared.entries.len()
    );

    let version = env!("CARGO_PKG_VERSION");
    let headline = state
        .db
        .list_crash_events(db::CRASH_EVENT_LIMIT)
        .ok()
        .and_then(|rows| rows.first().map(|r| r.signature.clone()))
        .unwrap_or_default();
    let subject = support::subject_line(version, &headline);
    let body = support::body_text(version, &support::os_version(), &headline);

    // Compose AND the clipboard half of the fallback are AppKit, so the whole
    // decision happens on the main thread and only the answer comes back.
    let composed = on_main_thread(&app, {
        let path = path.clone();
        move || support::compose_email(Some(&path), &subject, &body, false)
    })
    .unwrap_or(false);

    if composed {
        return Ok(support::compose_outcome(&path));
    }
    let path_for_fallback = path.clone();
    Ok(
        on_main_thread(&app, move || support::fallback_outcome(&path_for_fallback)).unwrap_or_else(
            || support::SendOutcome {
                method: "reveal".into(),
                path: path.to_string_lossy().into_owned(),
                recipient: support::SUPPORT_EMAIL.into(),
                message: format!(
                    "The file is on your Desktop. Attach it to an email to {}.",
                    support::SUPPORT_EMAIL
                ),
            },
        ),
    )
}

/// Run `f` on the main thread and wait for its answer.
///
/// Returns `None` if the event loop never got to it — a main thread that is
/// wedged must not hang a support request forever, and the caller's fallback is
/// the same fallback a missing mail client gets.
fn on_main_thread<T: Send + 'static>(
    app: &tauri::AppHandle,
    f: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    if app
        .run_on_main_thread(move || {
            let _ = tx.send(f());
        })
        .is_err()
    {
        return None;
    }
    rx.recv_timeout(std::time::Duration::from_secs(10)).ok()
}

// --- YV94: Meetings ------------------------------------------------------
//
// Read/list/export/delete only. Nothing here STARTS a meeting: capture is
// YV91's and the entry points are YV95's, so this item ships the surface that
// makes a recorded meeting findable, readable, exportable and deletable — and
// nothing it cannot honestly do yet.

/// Default page size for the Meetings list. Meetings are far rarer than
/// dictations (hence 200 there), and each row carries a segment COUNT subquery.
const MEETING_LIST_LIMIT: i64 = 100;

/// Meetings newest-first. `query` searches segment text through FTS5 and the
/// title through LIKE.
#[tauri::command]
fn list_meetings(
    state: State<'_, Arc<AppState>>,
    limit: Option<i64>,
    query: Option<String>,
) -> Result<Vec<meetings::Meeting>, String> {
    state
        .db
        .list_meetings(limit.unwrap_or(MEETING_LIST_LIMIT).clamp(1, 500), query)
}

/// One meeting plus its transcript, in wall-clock order.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingDetail {
    pub meeting: meetings::Meeting,
    pub segments: Vec<meetings::MeetingSegment>,
    /// Whether the WAV named by the row is still on disk. The row can point at
    /// a file the hygiene sweep removed, and a UI that promises audio it cannot
    /// produce is worse than one that says the audio expired.
    pub audio_on_disk: bool,
}

#[tauri::command]
fn get_meeting(state: State<'_, Arc<AppState>>, id: String) -> Result<MeetingDetail, String> {
    let meeting = state
        .db
        .get_meeting(&id)?
        .ok_or_else(|| "that meeting is no longer here".to_string())?;
    let segments = state.db.list_meeting_segments(&id)?;
    let audio_on_disk = meeting
        .mic_wav_path
        .as_deref()
        .map(|p| Path::new(p).is_file())
        .unwrap_or(false);
    Ok(MeetingDetail {
        meeting,
        segments,
        audio_on_disk,
    })
}

#[tauri::command]
fn rename_meeting(
    state: State<'_, Arc<AppState>>,
    id: String,
    title: String,
) -> Result<(), String> {
    state.db.rename_meeting(&id, &title)
}

/// YV96 — should the one-time meeting-capture notice be shown?
///
/// Read once on mount and again after every close. Cheap enough to be a plain
/// SELECT on a one-row table; deliberately NOT cached in `AppState`, because the
/// only thing that can change it is the command below.
#[tauri::command]
fn meeting_consent(state: State<'_, Arc<AppState>>) -> meetings::MeetingConsent {
    state.db.meeting_consent()
}

/// YV96 — the notice has been shown and closed. Idempotent; first close wins.
///
/// This command **does not** start, stop, or authorise a recording, and nothing
/// on the capture path calls it: O1 closed as a nudge, so the notice rides along
/// beside the recording rather than standing in front of it.
#[tauri::command]
fn acknowledge_meeting_consent(
    state: State<'_, Arc<AppState>>,
) -> Result<meetings::MeetingConsent, String> {
    state.db.acknowledge_meeting_consent()
}

/// YV102 — what Yap currently believes about system-audio permission.
///
/// Read on mount and after the setup step runs. Separate key, separate command,
/// separate answer from [`meeting_consent`]: one is a legal notice, the other is
/// a macOS permission, and the only thing they have in common is the table they
/// live in.
#[tauri::command]
fn system_audio_setup(state: State<'_, Arc<AppState>>) -> meetings::SystemAudioSetup {
    state.db.system_audio_setup()
}

/// YV102 — **the "Set up meeting recording" step.** Provokes the TCC alert on
/// purpose, from Settings, with the explanation already on screen.
///
/// There is no permission-request API for system audio: creating and starting a
/// process tap IS the request (OS-10, quoting AudioCap's README). So this runs a
/// real 200 ms tap and throws the audio away. What it buys is the *timing* — the
/// alert arrives while the user is reading a sentence about why, instead of at
/// T-0 of their first Zoom join, where a dismissal is terminal because TCC never
/// asks twice.
///
/// `async` + `spawn_blocking` for the same reason every other blocking command
/// in this file is: the 200 ms dwell is a real sleep on a real CoreAudio device,
/// and the webview's invoke must not be what waits on it.
///
/// **Never returns `LooksDenied`.** 200 ms is far below
/// `syscapture::DENIAL_GRACE`, so a quiet Mac cannot be turned into a denial
/// verdict by this path — that verdict is only ever earned by a real session.
#[tauri::command]
async fn run_system_audio_setup(
    state: State<'_, Arc<AppState>>,
) -> Result<meetings::SystemAudioSetup, String> {
    let db = Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || {
        let verdict = prewarm_system_audio();
        db.record_system_audio_setup(verdict)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The platform half of [`run_system_audio_setup`], split out so the command is
/// one shape on every target.
///
/// The macOS 14.4 gate is checked HERE rather than trusted from the UI: the
/// affordance is disabled below 14.4 (YV101), and a disabled affordance is a
/// courtesy, not an enforcement.
#[cfg(target_os = "macos")]
fn prewarm_system_audio() -> meetings::SetupVerdict {
    use os_version_gate::{system_audio_gate_now, SystemAudioGate};
    if !matches!(system_audio_gate_now(), SystemAudioGate::Available) {
        return meetings::SetupVerdict::Unavailable;
    }
    let report = syscapture::imp::prewarm_system_audio_permission();
    match (report.opened, report.verdict) {
        (false, _) => {
            if let Some(error) = &report.error {
                log::warn!("system-audio pre-warm failed: {error}");
            }
            meetings::SetupVerdict::Failed
        }
        (true, syscapture::SystemAudioPermission::Granted) => {
            // PERM-E: the ONLY legitimate input to the system-audio grant is an
            // observation from a tap that already ran. This is that call site.
            permissions::note_audio_capture_observation(
                permissions::AudioCaptureObservation::Delivered,
            );
            permissions::poke(permissions::WatchReason::Refusal);
            meetings::SetupVerdict::Granted
        }
        // Opened, nothing audible in 200 ms. The alert has been shown; the
        // answer is genuinely not knowable yet, and saying so is the item.
        (true, _) => meetings::SetupVerdict::Ran,
    }
}

#[cfg(not(target_os = "macos"))]
fn prewarm_system_audio() -> meetings::SetupVerdict {
    meetings::SetupVerdict::Unavailable
}

/// YV97 — summarize one recorded meeting, locally.
///
/// MUST be `async` + `spawn_blocking`, same as [`paste_entry`]. A *sync*
/// `#[tauri::command]` resolves to `ExecutionContext::Blocking` and runs inline
/// ON the main thread — and this body is minutes of work, not milliseconds: up
/// to 30s waiting for the sidecar's model to load, one `count_tokens` round-trip
/// per transcript line (~1,700 at YV91's 3h cap), a MAP pass per chunk at up to
/// 60s each, then REDUCE folds at 90s each. On main that is a multi-minute
/// freeze of the tray, the global hotkey and the pill — dictation starvation,
/// which is exactly what the separate sidecar exists to prevent. `spawn_blocking`
/// puts it on a blocking-pool thread where the claim below is actually true.
///
/// The summarizer spawns its OWN sidecar for the job (see
/// [`summarize::SidecarSession`]) rather than borrowing the warm dictation one,
/// so a summary running in the background cannot stall a take.
///
/// The meeting's state is moved to `summarizing` for the duration and back to
/// what it was on the way out, so a crash mid-summary leaves a row that says
/// what was happening rather than one that lies about being complete.
///
/// A summarize that produces nothing WRITES nothing. Re-summarizing a meeting
/// whose model run came back empty — a dead sidecar, a wedged model, a
/// grammar-legal but contentless answer — returns an error and leaves the
/// existing `meetings.summary` exactly as it was; see the `is_empty` refusal
/// below and `tests/summarize_empty_is_never_stored.rs`.
#[tauri::command]
async fn summarize_meeting(state: State<'_, Arc<AppState>>, id: String) -> Result<String, String> {
    // The managed handle is cloned OUT of the borrow before the hop: the
    // closure must be `'static`, and `Arc<AppState>` is what makes that free.
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || summarize_meeting_blocking(&state, id))
        .await
        .map_err(|e| format!("summary task join failed: {e}"))?
}

/// yap-meetings-fix (D) — the app handle the pipeline's progress events go out
/// through. Set once in `setup`; before that there is no window to tell.
static MEETING_APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

/// yap-meetings-fix (D) — after a transcript lands, summarize it when a local
/// summary model is installed. "No model" is a state, not an error: the
/// transcript stands and the Meetings view says "Add a summary model in
/// Settings".
fn auto_summarize_meeting(state: &AppState, id: &str) {
    let model = state.settings.lock().polish_model.trim().to_string();
    if model.is_empty() || summarize::SidecarSession::for_installed_model(&model).is_err() {
        log::info!("summary skipped: no model (meeting {id})");
        return;
    }
    match summarize_meeting_blocking(state, id.to_string()) {
        Ok(_) => log::info!("summary: meeting {id} summarized automatically"),
        Err(e) => log::warn!("summary: automatic summary of meeting {id} failed: {e}"),
    }
    if let (Some(app), Ok(Some(m))) = (MEETING_APP.get(), state.db.get_meeting(id)) {
        let _ = app.emit(
            meeting_pipeline::MEETING_PROGRESS_EVENT,
            meeting_pipeline::MeetingProgressEvent {
                id: id.to_string(),
                state: m.state.clone(),
                percent: 100,
                processed_through_seconds: m.processed_through_seconds,
                duration_seconds: m.duration_seconds,
                error: m.error.clone(),
            },
        );
    }
}

/// yap-meetings-fix — Retry on a failed (or partial) meeting: forget what was
/// decoded and run the whole pipeline again over the audio that is still kept.
#[tauri::command]
fn retry_meeting(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    meeting_pipeline::reset_for_retry(&state.db, &meetings_dir(), &id)?;
    if !meeting_pipeline::enqueue(&id) {
        return Err("the meeting transcription worker is not running — restart Yap".to_string());
    }
    Ok(())
}

/// The body of [`summarize_meeting`], off the main thread.
fn summarize_meeting_blocking(state: &AppState, id: String) -> Result<String, String> {
    let meeting = state
        .db
        .get_meeting(&id)?
        .ok_or_else(|| "that meeting is no longer here".to_string())?;
    let segments = state.db.list_meeting_segments(&id)?;
    if segments.is_empty() {
        return Err("that meeting has no transcript to summarize".to_string());
    }
    let model = state.settings.lock().polish_model.trim().to_string();
    let session = summarize::SidecarSession::for_installed_model(&model)
        .map_err(|_| "no local summary model is installed".to_string())?;
    let client = summarize::SidecarSummaryClient::new(session);

    let previous =
        meetings::MeetingState::parse(&meeting.state).unwrap_or(meetings::MeetingState::Complete);
    let _ = state
        .db
        .set_meeting_state(&id, meetings::MeetingState::Summarizing, None);
    // YV133 — the summary is attributed with the SAME labelling the transcript
    // is rendered with: `TrackSpeakers::for_meeting` makes the two calls
    // `meetings::render_transcript` makes, off this meeting's stored `kind` and
    // its own segments, so the speaker beside an action item and the speaker on
    // the transcript row it cites cannot come apart. On the clustering branch
    // this source declines and the summary is exactly what YV97 produced.
    let speakers = summarize::TrackSpeakers::for_meeting(meeting.kind(), &segments);
    let summary = summarize::summarize_segments_with(&segments, &client, &speakers);
    let _ = state.db.set_meeting_state(&id, previous, None);

    let summary = summary.map_err(|e| match e {
        // Said in the user's words rather than as a tag, because this is the one
        // failure whose whole point is that NOTHING changed — a toast that only
        // said "summary failed" would leave the reader wondering what happened to
        // the summary they already had.
        summarize::SummaryError::Empty => {
            "the model had nothing to say about this meeting — the existing summary was kept"
                .to_string()
        }
        other => format!("summary failed: {}", other.tag()),
    })?;
    // Belt and braces on the ONE write of `meetings.summary`. `summarize_segments`
    // already refuses to return an empty summary, but the refusal that protects
    // the user's data belongs next to the destructive act as well: a later caller
    // that builds a `MeetingSummary` some other way (a resumed job, a partial
    // re-summarize) must not be able to overwrite a good summary with nothing
    // just by skipping the pipeline's guard.
    if summary.is_empty() {
        log::warn!("summary: refusing to overwrite meetings.summary with an empty result");
        return Err(
            "the model had nothing to say about this meeting — the existing summary was kept"
                .to_string(),
        );
    }
    state
        .db
        .set_meeting_summary(&id, &summary.markdown, Some(&summary.model))?;
    log::info!(
        "summary: meeting summarized over {} chunk(s), {} action(s), {} dropped item(s), truncated={}",
        summary.chunks,
        summary.actions.len(),
        summary.dropped_items,
        summary.truncated
    );
    Ok(summary.markdown)
}

/// Delete a meeting: rows, FTS entries, and the WAV.
///
/// `secure_delete = ON` means the cascade physically overwrites the freed pages
/// rather than unlinking them, so this is real work on a 3-hour meeting. It runs
/// off the UI thread for free — Tauri dispatches a NON-async command onto a
/// worker thread, and this is deliberately not `async fn`, so the webview keeps
/// painting while the pages are scrubbed.
#[tauri::command]
fn delete_meeting(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state.db.delete_meeting_with_audio(&id)
}

/// Export one meeting as Markdown into Application Support, alongside the
/// transcript export (YV77). Same temp-file-then-rename discipline: a reader
/// never sees a half-written file under the final name.
///
/// Markdown only — PDF was cut for v1 (finding #33): webview print pagination
/// over a 3-hour transcript is a real time sink for a feature nobody has asked
/// for, and a .md file opens in every editor, note app and chat window.
#[tauri::command]
fn export_meeting_markdown(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<ExportResult, String> {
    use std::io::Write;

    let (meeting, markdown) = state.db.meeting_markdown(&id)?;
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stem = meetings::export_file_stem(&meeting.title, meeting.started_at);
    let path = dir.join(format!("{stem}.md"));
    let tmp = dir.join(format!(".{stem}.md.part"));

    let file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut w = std::io::BufWriter::new(file);
    if let Err(e) = w
        .write_all(markdown.as_bytes())
        .and_then(|()| w.flush())
        .map_err(|e| e.to_string())
    {
        drop(w);
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    drop(w);
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(ExportResult {
        path: path.display().to_string(),
        count: meeting.segment_count as usize,
    })
}

// ── YV95 · starting and stopping one ──────────────────────────────────────────
//
// Three commands, one behaviour: `toggle_meeting` is what the UI actually calls
// (the pill's stop control and the empty state's button are the same action seen
// from two states), and the explicit start/stop pair exists so a caller that
// knows which one it wants cannot get the other by racing the toggle.

/// The current meeting status. Called on mount by both windows so a freshly
/// opened Yap never has to wait a second for the first tick to know.
#[tauri::command]
fn meeting_status(state: State<'_, Arc<AppState>>) -> meeting_control::MeetingStatus {
    match state.meeting() {
        Some(c) => c.status(),
        None => meeting_control::MeetingController::new(
            state.db.clone(),
            Arc::new(|_: &meeting_control::MeetingStatus| {}),
        )
        .status(),
    }
}

/// Start or stop the meeting — the one action behind all four entry points.
///
/// `kind` (YV125) is the webview's answer to the start-of-meeting picker, and
/// it is OPTIONAL on the wire: a caller that sends nothing has skipped the
/// question, which is `unknown` — the same thing ⌃⌘M does. An unrecognised
/// string resolves to `unknown` too (`MeetingKind::parse`), so a UI that gets
/// ahead of this build cannot start a meeting on a branch this build does not
/// have.
#[tauri::command]
fn toggle_meeting_recording(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    kind: Option<String>,
) -> Result<meeting_control::MeetingStatus, String> {
    let st = state.inner().clone();
    let kind = kind
        .as_deref()
        .map(meetings::MeetingKind::parse)
        .unwrap_or(meetings::MeetingKind::Unknown);
    toggle_meeting(&app, &st, kind)
}

/// The three choices the start-of-meeting picker offers, so the webview renders
/// the SAME list the tray submenu does rather than a second hand-written copy
/// that can drift from it (YV125).
#[tauri::command]
fn meeting_kind_choices() -> Vec<meeting_control::KindChoice> {
    meeting_control::KIND_PICKER.to_vec()
}

/// Start a meeting. Errors (no capture engine, one already running) come back as
/// a string the UI shows verbatim — this is the button a first-time user
/// presses, and a silent failure here is the whole of finding #6 all over again.
#[tauri::command]
fn start_meeting(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    title: Option<String>,
    kind: Option<String>,
) -> Result<meeting_control::MeetingStatus, String> {
    let controller = state.meeting().ok_or("Yap is still starting up")?;
    if controller.is_recording() {
        return Ok(controller.status());
    }
    let st = state.inner().clone();
    let kind = kind
        .as_deref()
        .map(meetings::MeetingKind::parse)
        .unwrap_or(meetings::MeetingKind::Unknown);
    controller.start_with_kind(&meetings_dir(), title, kind)?;
    let status = controller.status();
    if st.settings.lock().show_floating_pill {
        float_pill::show_for_recording(&app);
    }
    Ok(status)
}

/// Stop the running meeting. Idempotent: stopping a meeting that already stopped
/// is the state the user asked for, not an error to show them.
#[tauri::command]
fn stop_meeting(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<meeting_control::MeetingStatus, String> {
    let controller = state.meeting().ok_or("Yap is still starting up")?;
    if !controller.is_recording() {
        return Ok(controller.status());
    }
    controller.stop("user stopped")?;
    if !*state.recording.lock() {
        float_pill::after_recording(&app, state.settings.lock().show_floating_pill);
    }
    Ok(controller.status())
}

/// Where a transcript export landed and how many rows it holds (YV77). The
/// count reaches the UI so "Exported" is a claim the user can check against
/// their own history instead of an unverifiable path.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: String,
    pub count: usize,
}

/// Export transcript history to Application Support for backup / future LoRA corpus.
///
/// YV77 — streams EVERY row, oldest first, as JSON Lines (one compact object per
/// line: appendable and streamable, which is what a corpus reader wants). The
/// bytes go to a temp file first and are `rename`d onto the final name only
/// after a successful flush, so an interrupted or failed export can never leave
/// a truncated file masquerading as a finished backup.
#[tauri::command]
fn export_history(state: State<'_, Arc<AppState>>) -> Result<ExportResult, String> {
    use std::io::Write;

    let dir = data_dir();
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let path = dir.join(format!("export-transcripts-{stamp}.jsonl"));
    let tmp = dir.join(format!(".export-transcripts-{stamp}.jsonl.part"));

    let file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut w = std::io::BufWriter::new(file);
    let count = match state.db.write_transcripts_jsonl(&mut w).and_then(|n| {
        w.flush().map_err(|e| e.to_string())?;
        Ok(n)
    }) {
        Ok(n) => n,
        Err(e) => {
            drop(w);
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    };
    drop(w);
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(ExportResult {
        path: path.display().to_string(),
        count,
    })
}

#[tauri::command]
fn open_privacy_settings(pane: String) -> Result<(), String> {
    permissions::open_privacy_pane(&pane)
}

/// A command failure the frontend can BRANCH on rather than string-match. Every
/// existing command answers `Result<_, String>` and the UI toasts the string;
/// the license gate needs more than that — "trial over" is a different screen
/// from "the mic is busy" — so it carries a stable `code` alongside the
/// sentence a person reads.
#[derive(Debug, Clone, Serialize)]
pub struct CommandError {
    /// Stable machine code, e.g. `license_required`.
    pub code: String,
    pub message: String,
}

impl CommandError {
    fn license_required() -> Self {
        Self {
            code: license::LICENSE_REQUIRED_CODE.to_string(),
            message: license::LICENSE_REQUIRED_MESSAGE.to_string(),
        }
    }
}

/// Start/stop a dictation from a UI surface.
///
/// The STOP half is never gated: a take already in flight always gets to finish
/// and be saved, whatever the license says. Only the START half can be refused,
/// and it is refused with a typed `license_required` rather than a silent
/// no-op — a button that does nothing is the worst possible way to tell
/// somebody their trial ended.
#[tauri::command]
fn manual_toggle(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), CommandError> {
    // Same helper the hotkey path uses, so the event + the (throttled) toast
    // fire however the user reached this — including the floating pill, whose
    // click handler has nowhere to put a returned error.
    if !*state.recording.lock() && !license_allows_new_dictation(&app, state.inner()) {
        return Err(CommandError::license_required());
    }
    manual_toggle_inner(app, state);
    Ok(())
}

fn manual_toggle_inner(app: AppHandle, state: State<'_, Arc<AppState>>) {
    if *state.recording.lock() {
        // A manual stop (Home button / pill / sidebar) always exits hands-free —
        // clear BOTH latches: the app-side flag (or stop_and_transcribe's guard
        // makes every on-screen Stop a no-op while locked) AND the PTT tap-side
        // latch (or the next fn gesture is swallowed resetting it).
        stop_take(&app, state.inner(), StopOrigin::OnScreen);
    } else {
        start_recording(&app, state.inner());
    }
}

/// yap-p0-hotkeys-no-permission DO 2 — THE dispatcher for every hotkey edge.
/// The Carbon dictate chord (and the legacy ⌘⇧V) and the fn tap's
/// `PttEvent::Start` / `PttEvent::Stop` all land here; `hotkeys::HotkeyArbiter`
/// decides start / stop / ignore, so the two sources can never double-start
/// and there is no second start/stop path.
fn take_hotkey_edge(
    app: &AppHandle,
    state: &Arc<AppState>,
    edge: HotkeyEdge,
    source: HotkeySource,
) {
    let live = *state.recording.lock();
    let action = state
        .hotkey_arbiter
        .lock()
        .edge(edge, source, live, hotkeys::now_ms());
    log::info!("hotkey edge {edge:?} from {source:?} -> {action:?}");
    match action {
        hotkeys::TakeAction::Start => start_from_hotkey(app, state, source),
        hotkeys::TakeAction::Stop => stop_take(app, state, StopOrigin::Hotkey),
        hotkeys::TakeAction::Ignore => {}
    }
}

/// The start half of `take_hotkey_edge`. YV49: an fn press that carried the
/// command modifier EDITS the current selection instead of typing — read at
/// press time, because by the time ASR returns focus and selection may both
/// have moved. No selection → nothing to edit, so the take never starts.
fn start_from_hotkey(app: &AppHandle, state: &Arc<AppState>, source: HotkeySource) {
    #[cfg(target_os = "macos")]
    if source == HotkeySource::FnTap && ptt_macos::command_mode_active() {
        match focus::selected_text() {
            Some(selection) => {
                *state.command_selection.lock() = Some(selection);
                start_recording(app, state);
            }
            None => {
                *state.command_selection.lock() = None;
                log::info!("command mode: no selection — take not started");
                let _ = app.emit(PASTE_OUTCOME_EVENT, command_mode::NO_SELECTION_MESSAGE);
            }
        }
        return;
    }
    let _ = source;
    *state.command_selection.lock() = None;
    start_recording(app, state);
}

/// focus-2026-10-19 — the live hotkey table (every action, its key, whether
/// it works now, and the key that performs it right now).
fn current_binding_rows(state: &AppState) -> Vec<hotkey_table::BindingRow> {
    let secure = state.secure_input.lock().clone();
    let hk = hotkeys::status();
    let fn_tap = hotkeys::effective_fn_tap(&hk.fn_tap, secure.blocked, secure.culprit.as_deref());
    let settings = state.settings.lock().clone();
    let im = matches!(perm_probe::listen_event_hid(), permissions::GrantState::Authorized);
    let inputs = hotkey_table::TableInputs {
        fn_tap,
        dictate_carbon: hk.carbon,
        meeting_carbon: hotkey_table::meeting_status(),
        accessibility: perm_probe::accessibility_trusted(),
        input_monitoring: im,
        listen_event_row: hotkey_table::tcc_listen_event_row(&perm_truth::running_identity().bundle_id),
        hold_label: settings.hotkey_label.clone(),
        toggle_label: hotkeys::active_dictate().1,
        cancel_label: hotkey_table::active_cancel().1,
        meeting_label: hotkey_table::active_meeting().1,
    };
    hotkey_table::binding_table(&inputs)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HotkeyBindingsReport {
    rows: Vec<hotkey_table::BindingRow>,
    live_dictate_key: Option<String>,
    dump: String,
}

#[tauri::command]
fn hotkey_bindings(state: State<'_, Arc<AppState>>) -> HotkeyBindingsReport {
    let rows = current_binding_rows(&state);
    HotkeyBindingsReport {
        live_dictate_key: hotkey_table::live_dictate_key(&rows),
        dump: hotkey_table::debug_dump(&rows),
        rows,
    }
}

/// Swap the registered dictate chord for the Settings rebind, live, through
/// `hotkeys::rebind`: the new chord is registered FIRST and the old one is
/// dropped only after that succeeded. A chord another app holds returns Err
/// (shown by the Settings capture) and leaves the old chord registered and
/// active.
fn rebind_dictate_toggle(app: &AppHandle, settings: &AppSettings) -> Result<(), String> {
    let (chord, label) = dictate_toggle_chord(settings);
    // Fix round 2: `rebind_and_activate` swaps the chord AND re-emits `status`
    // when the active chord changed, so every surface naming the chord
    // (Settings, Permissions, Insights, the home banner) drops the old label.
    hotkeys::rebind_and_activate(
        &mut TauriCarbonRegistrar { app },
        chord,
        label,
        &mut AppHotkeySink,
    )
}

/// The registered dictate chord: the Settings rebind when it is valid, else
/// `shortcuts::DICTATE_TOGGLE`.
fn dictate_toggle_chord(settings: &AppSettings) -> (tauri_plugin_global_shortcut::Shortcut, String) {
    if let Some(c) = settings.dictate_toggle.as_ref() {
        match hotkeys::chord_from_setting(c) {
            Ok(v) => return v,
            Err(e) => log::warn!("dictate toggle rebind ignored ({e}) — using the default"),
        }
    }
    (
        shortcuts::DICTATE_TOGGLE.shortcut(),
        shortcuts::DICTATE_TOGGLE.label.to_string(),
    )
}

/// The real Carbon registrar: the global-shortcut plugin (RegisterEventHotKey).
/// Every DICTATE_TOGGLE register result goes to `hotkeys::set_carbon_status`
/// through `AppHotkeySink`.
struct TauriCarbonRegistrar<'a> {
    app: &'a AppHandle,
}

impl hotkeys::CarbonRegistrar for TauriCarbonRegistrar<'_> {
    fn register(&mut self, chord: tauri_plugin_global_shortcut::Shortcut) -> Result<(), String> {
        // Fix round 2: the plugin's RegisterEventHotKey is NON-exclusive and
        // returns noErr for a chord another app or macOS already holds, so ask
        // first with kEventHotKeyExclusive (on the main thread, where the
        // plugin registers too; tauri runs this inline when already on it).
        let (tx, rx) = std::sync::mpsc::channel();
        self.app
            .run_on_main_thread(move || {
                let _ = tx.send(carbon_probe::probe_chord(&chord));
            })
            .map_err(|e| e.to_string())?;
        if let Err(e) = rx.recv().map_err(|e| e.to_string())? {
            log::warn!("dictate-toggle exclusive probe refused the chord: {e}");
            return Err(e);
        }
        let r = if chord == shortcuts::DICTATE_TOGGLE.shortcut() {
            self.app.global_shortcut().register(shortcuts::DICTATE_TOGGLE.shortcut())
        } else {
            self.app.global_shortcut().register(chord)
        };
        match &r {
            Ok(()) => log::info!("{} dictate-toggle registered (carbon)", hotkeys::active_dictate().1),
            Err(e) => log::warn!("{} dictate-toggle register failed: {e}", hotkeys::active_dictate().1),
        }
        r.map_err(|e| e.to_string())
    }
    fn unregister(&mut self, chord: tauri_plugin_global_shortcut::Shortcut) -> Result<(), String> {
        self.app.global_shortcut().unregister(chord).map_err(|e| e.to_string())
    }
    fn is_registered(&self, chord: tauri_plugin_global_shortcut::Shortcut) -> bool {
        self.app.global_shortcut().is_registered(chord)
    }
}

/// The real TCC probe (informational: it never gates the tap).
struct OsPermissionProbe;

impl hotkeys::PermissionProbe for OsPermissionProbe {
    // yap-p0-perm-repair: the degraded line reads the same live probe.
    fn accessibility(&self) -> bool {
        perm_probe::accessibility_trusted()
    }
    fn input_monitoring(&self) -> bool {
        matches!(
            permissions::input_monitoring_state(
                perm_probe::listen_event_hid(),
                perm_probe::accessibility_trusted(),
                matches!(hotkeys::status().fn_tap, hotkeys::FnTapStatus::Delivering),
            ),
            permissions::GrantState::Authorized
        )
    }
}

/// The real tap source: `ptt_macos::start`, whose tap thread reports its own
/// install result through `set_fn_tap_status`.
struct FnTapSource<'a> {
    app: &'a AppHandle,
    state: &'a Arc<AppState>,
}

impl hotkeys::TapEventSource for FnTapSource<'_> {
fn install(&mut self) -> Result<hotkeys::TapAttempt, String> {
    #[cfg(target_os = "macos")]
    {
        start_fn_tap(self.app, self.state);
        Ok(hotkeys::TapAttempt::Dispatched)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (self.app, self.state);
        Err("fn tap is macOS-only".into())
    }
}
}

/// Writes arming results into the process-wide hotkey status.
struct AppHotkeySink;

impl hotkeys::StatusSink for AppHotkeySink {
fn carbon(&mut self, s: hotkeys::CarbonStatus) {
    hotkeys::set_carbon_status(s);
}
fn fn_tap(&mut self, s: hotkeys::FnTapStatus) {
    hotkeys::set_fn_tap_status(s);
}
fn current_fn_tap(&self) -> hotkeys::FnTapStatus {
    hotkeys::status().fn_tap
}
}

/// Start (or retry) the fn⌃ hybrid tap. Safe to call again: `ptt_macos::start`
/// only rebinds while a tap is running, and re-spawns after a failed install.
#[cfg(target_os = "macos")]
fn start_fn_tap(app: &AppHandle, state: &Arc<AppState>) {
    let binding = ptt_macos::PttBinding::from_settings(
        &state.settings.lock().ptt_binding,
    );
    let command_binding = ptt_macos::CommandBinding::from_settings(
        &state.settings.lock().command_binding,
    );
    let st = state.clone();
    let h = app.clone();
    ptt_macos::start(
        binding,
        command_binding,
        Arc::new(move |ev| {
            let st = st.clone();
            let h = h.clone();
            if let Err(e) = h.clone().run_on_main_thread(move || match ev {
                ptt_macos::PttEvent::Start => {
                    take_hotkey_edge(&h, &st, HotkeyEdge::Press, HotkeySource::FnTap);
                }
                ptt_macos::PttEvent::Stop => {
                    take_hotkey_edge(&h, &st, HotkeyEdge::Release, HotkeySource::FnTap);
                }
                ptt_macos::PttEvent::StillRecording => {
                        // yap-hotkey-stop DO 4: the status event carries
                        // `still_recording`; the pill swaps its line.
                        emit_status(&h, &st);
                        float_pill::show_for_recording(&h);
                        log::warn!(
                            "take still recording — pill asks for fn or stop"
                        );
                    }
                    ptt_macos::PttEvent::Interrupted => {
                        *st.hands_free.lock() = false;
                        cancel_recording(&h, &st);
                    }
                    ptt_macos::PttEvent::HandsFreeOn => {
                        *st.hands_free.lock() = true;
                        emit_status(&h, &st);
                        float_pill::show_for_recording(&h);
                        log::info!("hands-free ON");
                    }
                    ptt_macos::PttEvent::HandsFreeOff => {
                        *st.hands_free.lock() = false;
                        emit_status(&h, &st);
                        log::info!("hands-free OFF");
                    }
                }) {
                    log::error!("PTT main-thread hop failed: {e}");
                }
            }),
        );
        log::info!(
            "PTT hybrid started ({} · command mode {})",
            binding.label(),
            command_binding.label()
        );
}

/// yap-hotkey-stop DO 5 — THE stop for a live dictation. The hotkey (fn
/// key-up / fn tap), the pill's stop control (`manual_toggle`) and the menu
/// bar's Stop + Hands-free items all end the take HERE, so all of them clear
/// both latches: the app-side `hands_free` (or `stop_and_transcribe`'s guard
/// turns the stop into a no-op) and the PTT machine's own latch via
/// `ptt_macos::end_hands_free` (or the next fn press is swallowed resetting it).
/// tests/hotkey_stop.rs holds every call site to this function.
/// yap-p0-chord-deadlock — the ACT half of every global shortcut, run on the
/// main thread AFTER the plugin's handler returned (`shortcut_defer`), so the
/// plugin's shortcut lock is free and `register_cancel_shortcut` /
/// `unregister_cancel_shortcut` (and anything else calling `global_shortcut()`)
/// cannot re-enter it.
fn run_shortcut_job(app: &AppHandle, state: &Arc<AppState>, job: shortcut_defer::ShortcutJob) {
    use shortcut_defer::ShortcutJob as J;
    match job {
        // yap-p0-hotkeys-no-permission — the zero-TCC dictate chord (default
        // ⌃⌥⌘Space, rebindable) and the legacy ⌘⇧V: press and release both go
        // to the ONE dispatcher.
        J::Take(edge) => take_hotkey_edge(app, state, edge, HotkeySource::Carbon),
        // YV95 ⌃⌘M — a TOGGLE on key-down, not a hold. The hotkey never asks:
        // the picker is skipped (`unknown`).
        J::MeetingToggle => {
            log::info!("⌃⌘M pressed — toggle meeting");
            toggle_meeting_off_thread(app, state, "⌃⌘M", meetings::MeetingKind::Unknown);
        }
        // Y3-D esc — registered only WHILE a take is in flight. Off the main
        // thread: the cancel joins the recorder's stop.
        J::Cancel => {
            log::info!("esc pressed — cancel the take in flight");
            let (st, h) = (state.clone(), app.clone());
            std::thread::spawn(move || {
                cancel_take(&h, &st);
            });
        }
        // ⌃⌘V — Paste Last Transcript (Wispr-parity, always on).
        J::PasteLast => {
            log::info!("⌃⌘V pressed — paste last transcript");
            paste_last_transcript(app, state);
        }
        // YV51 ⌃⌘Z — Undo AI edit: re-paste the raw take over the polished one.
        J::UndoAiEdit => {
            log::info!("⌃⌘Z pressed — undo ai edit (paste raw)");
            paste_raw_last_transcript(app, state);
        }
    }
}

fn stop_take(app: &AppHandle, state: &Arc<AppState>, origin: StopOrigin) {
    *state.hands_free.lock() = false;
    #[cfg(target_os = "macos")]
    ptt_macos::stop_take_with(origin, || stop_and_transcribe(app.clone(), state.clone()));
    #[cfg(not(target_os = "macos"))]
    {
        let _ = origin;
        stop_and_transcribe(app.clone(), state.clone());
    }
}

/// Bring the main window forward — THE presenter for every activation path:
/// `RunEvent::Reopen` (Dock click, Finder double-click, `open -a Yap`), the
/// YV42 single-instance handler, the tray's "Open Yap"/"Settings" and the
/// `show_main` command. The decision is `lifecycle::activation_action`; this
/// only reads the window's state and carries the answer out. Main thread only.
fn present_main_window(app: &AppHandle, source: lifecycle::ActivationSource) {
    let Some(w) = app.get_webview_window("main") else {
        log::warn!("activation {source:?}: no main window to present");
        return;
    };
    let state = lifecycle::MainWindowState {
        exists: true,
        visible: w.is_visible().unwrap_or(false),
        minimized: w.is_minimized().unwrap_or(false),
        focused: w.is_focused().unwrap_or(false),
    };
    match lifecycle::activation_action(source, state) {
        lifecycle::WindowAction::Nothing => {}
        lifecycle::WindowAction::Present { unminimize } => {
            if unminimize {
                let _ = w.unminimize();
            }
            let _ = w.show();
            let _ = w.set_focus();
            log::info!("activation {source:?}: presented main window ({state:?})");
        }
    }
}

/// Say a stale handoff out loud. The Tauri app (and its notification plugin)
/// never starts in a launch that hands off, so this goes through
/// `osascript display notification` — non-modal, nothing to dismiss.
fn notify_stale_instance(msg: &str) {
    let script = format!(
        "display notification \"{}\" with title \"Yap\"",
        msg.replace('\\', "").replace('"', "'")
    );
    let _ = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .status();
}

/// The bundle identifier of THIS build's channel, for the stale-instance probe
/// that runs before the Tauri config is loaded. It is the channel profile's
/// identifier (production `com.wilsonguenther.wilson-voice`, Yap Test
/// `com.wilsonguenther.wilson-voice.test`), which `run()` already requires to
/// equal `config.identifier` (`channel::check_bundle_identity`) — the same id
/// the single-instance plugin derives its socket from. A hard-coded production
/// id here made Yap Test probe production's socket and exit 0 whenever Yap ran.
pub fn instance_identifier() -> &'static str {
    channel::current().identifier
}

/// The red X's one-time hint, claimed by a MOUNTED `HideHint` (on mount and on
/// each nudge). The marker is written here and nowhere else, so a hide during
/// onboarding — when nothing can show the note — does not use it up.
#[tauri::command]
fn claim_hide_hint() -> bool {
    lifecycle::HIDE_HINT.claim(&data_dir())
}

#[tauri::command]
fn show_main(app: AppHandle) {
    present_main_window(&app, lifecycle::ActivationSource::TrayOpen);
}

// ─── YP2 licensing commands ──────────────────────────────────────────

/// Current entitlement. Recomputed from the stored signature on every call —
/// there is no cached answer to go stale.
#[tauri::command]
fn license_status(state: State<'_, Arc<AppState>>) -> license::LicenseStatus {
    state.license.status()
}

/// Paste a license key. Verified BEFORE it is written, so a bad paste never
/// lands on disk. Activation is entirely local (yap-lic-00): the key is checked
/// against the compiled-in public key and no network call is made.
#[tauri::command]
fn activate_license(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    key: String,
) -> Result<license::LicenseStatus, CommandError> {
    match state.license.activate(&key) {
        Ok(status) => {
            let _ = app.emit("license", &status);
            Ok(status)
        }
        Err(e) => Err(CommandError {
            code: e.code().to_string(),
            message: e.message().to_string(),
        }),
    }
}

/// YP3 — open Stripe's hosted checkout in the user's browser.
///
/// Takes **no argument on purpose**. The destination is
/// `license::PAYMENT_LINK_URL`, a compile-time constant, handed to `open(1)` as
/// one argv element — never a shell string, and never a URL the webview
/// supplied. A command that accepted a URL would be a "open anything on this
/// Mac" primitive reachable from any script that reaches the frontend.
#[tauri::command]
fn open_purchase_page() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(license::PAYMENT_LINK_URL)
            .spawn()
            .map(|_| ())
            .map_err(|e| {
                format!(
                    "Could not open your browser ({e}). The link is {}",
                    license::PAYMENT_LINK_URL
                )
            })
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(format!(
            "Open {} in your browser to buy Yap.",
            license::PAYMENT_LINK_URL
        ))
    }
}

/// Remove the license from this Mac (moving a seat). The trial bookkeeping is
/// deliberately untouched — this is not a second fortnight.
#[tauri::command]
fn deactivate_license(app: AppHandle, state: State<'_, Arc<AppState>>) -> license::LicenseStatus {
    state.license.deactivate();
    let status = state.license.status();
    let _ = app.emit("license", &status);
    status
}

/// Push the `autostart` setting into the OS (YV42) — a macOS LaunchAgent via
/// tauri-plugin-autostart. Called at startup and on every save so the setting,
/// not a LaunchAgent left behind by an older install or a moved .app, is the
/// source of truth. A login item that cannot be written is logged, never
/// surfaced as a settings error: the rest of the save already landed.
fn apply_autostart(app: &AppHandle, enabled: bool) {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    // Only touch the plist when it actually disagrees — this runs on every
    // launch and on every unrelated settings save.
    match manager.is_enabled() {
        Ok(current) if current == enabled => return,
        Err(e) => log::warn!("could not read the launch-at-login state ({e}); applying anyway"),
        _ => {}
    }
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    match result {
        Ok(()) => log::info!(
            "launch at login {}",
            if enabled { "enabled" } else { "disabled" }
        ),
        Err(e) => log::warn!(
            "could not {} launch at login: {e}",
            if enabled { "enable" } else { "disable" }
        ),
    }
}

// --- Embedded ASR model management (YV31) ---

/// One bundled-catalog model as the model manager sees it: catalog metadata
/// merged with on-disk (downloaded) and selection state.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub architecture: String,
    /// Default-quant file that gets downloaded for this model.
    pub filename: String,
    pub quant: String,
    pub size_bytes: u64,
    pub downloaded: bool,
    pub selected: bool,
    pub recommended: bool,
    pub recommended_rank: Option<u32>,
}

/// The bundled catalog merged with downloaded + selected state. Offline: the
/// catalog is compiled into the binary and "downloaded" is a file-system check.
#[tauri::command]
async fn list_models(state: State<'_, Arc<AppState>>) -> Result<Vec<ModelEntry>, String> {
    let selected = state.settings.lock().native_model.clone();
    Ok(models::catalog()
        .models
        .iter()
        .filter_map(|m| {
            let file = m.default_file()?;
            Some(ModelEntry {
                id: m.id.clone(),
                name: m.name.clone(),
                description: m.description.clone(),
                architecture: m.architecture.clone(),
                filename: file.filename.clone(),
                quant: file.quant.clone(),
                size_bytes: file.size_bytes,
                downloaded: models::is_downloaded(m),
                selected: m.id == selected,
                recommended: m.recommended,
                recommended_rank: m.recommended_rank,
            })
        })
        .collect())
}

/// Download a catalog model's default-quant file (resumable, sha256-verified),
/// emitting `model_download_progress` events. Returns the on-disk path.
#[tauri::command]
async fn download_model(app: AppHandle, id: String) -> Result<String, String> {
    let path = models::download_model(&app, &id).await?;
    log::info!("model '{id}' downloaded to {}", path.display());
    Ok(path.to_string_lossy().into_owned())
}

/// Select the embedded ASR model. Persists `native_model` and drops a warm
/// engine of a DIFFERENT model so the next use loads the new selection.
#[tauri::command]
async fn select_model(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), String> {
    let model =
        models::catalog_model(&id).ok_or_else(|| format!("unknown catalog model '{id}'"))?;
    let next = {
        let mut settings = state.settings.lock();
        settings.native_model = model.id.clone();
        settings.clone()
    };
    persist_settings(&next)?;
    let _ = app.emit("settings", &next);
    if state.transcription.loaded_model().as_deref() != Some(model.id.as_str()) {
        state.transcription.unload();
    }
    log::info!("embedded ASR model selected: {}", model.id);
    Ok(())
}

/// One polish model as `list_polish_models` reports it (camelCase on the wire).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolishModelEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub parameters: String,
    pub license: String,
    pub size_bytes: u64,
    /// Present at its full catalog size — i.e. sha256-verified at download time.
    pub downloaded: bool,
    /// This id is the live `polish_model` setting.
    pub selected: bool,
}

/// The polish models a user may install (SEC-C).
///
/// This is the FIRST surface that ever offered them: `download_polish_model_with`
/// has existed since YV60 and nothing in the frontend could reach it, so the
/// whole LLM stage was dead code on every installed copy. Deliberately narrower
/// than the catalog — see `models::OFFERED_POLISH_RANK` for why the 0.5B fast
/// tier is withheld.
#[tauri::command]
async fn list_polish_models(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<PolishModelEntry>, String> {
    let selected = state.settings.lock().polish_model.trim().to_string();
    Ok(models::offered_polish_models()
        .into_iter()
        .map(|m| PolishModelEntry {
            id: m.id.clone(),
            name: m.name.clone(),
            description: m.description.clone(),
            parameters: m.parameters.clone(),
            license: m.license.clone(),
            size_bytes: m.file.size_bytes,
            downloaded: models::is_polish_downloaded(m),
            selected: m.id == selected,
        })
        .collect())
}

/// Install a polish model: free-space precondition, then the resumable,
/// sha256-verified download, then — and ONLY then — `polish_model` becomes
/// non-empty.
///
/// Optional by construction. Nothing calls this on the first-run path; the user
/// presses a button in Settings → Advanced, and until they do the stage no-ops
/// exactly as it does today.
#[tauri::command]
async fn download_polish_model(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<String, String> {
    let model = models::polish_model(&id).ok_or_else(|| format!("unknown polish model '{id}'"))?;
    if model.recommended_rank != Some(models::OFFERED_POLISH_RANK) {
        return Err(format!(
            "polish model '{id}' is in the catalog but is not offered for install"
        ));
    }

    // Free space BEFORE the first byte. A 1.1 GB fetch that dies at 98% on a
    // full volume leaves a `.partial` and a user with no idea why.
    let dir = models::models_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    models::check_polish_free_space(model, meeting::free_disk_bytes(&dir))?;

    let path = models::download_polish_model(&app, &id).await?;

    // The gate. `verified_polish_selection` is the only producer of a non-empty
    // `polish_model` — a download that "succeeded" but left a short file never
    // becomes a setting.
    let selection = models::verified_polish_selection(&id, &path)?;
    let next = {
        let mut settings = state.settings.lock();
        settings.polish_model = selection;
        settings.clone()
    };
    persist_settings(&next)?;
    let _ = app.emit("settings", &next);
    log::info!(
        "polish model '{id}' installed to {} and selected",
        path.display()
    );
    Ok(path.to_string_lossy().into_owned())
}

/// Turn the polish stage back off: `polish_model` returns to `""`, which is the
/// documented OFF state (lib.rs `polish_model`, `polish::build_request`). The
/// weights stay on disk — deleting a gigabyte the user may re-enable in a minute
/// is a separate decision they did not make here.
#[tauri::command]
async fn clear_polish_model(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let next = {
        let mut settings = state.settings.lock();
        settings.polish_model = String::new();
        settings.clone()
    };
    persist_settings(&next)?;
    let _ = app.emit("settings", &next);
    log::info!("polish model cleared — the LLM polish stage is off");
    Ok(())
}

/// Delete a downloaded model's file (and any interrupted `.partial`). Unloads
/// it first if it is the warm one, so the bytes aren't in use.
#[tauri::command]
async fn delete_model(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    let model =
        models::catalog_model(&id).ok_or_else(|| format!("unknown catalog model '{id}'"))?;
    if state.transcription.loaded_model().as_deref() == Some(model.id.as_str()) {
        state.transcription.unload();
    }
    models::delete_downloaded(model)?;
    log::info!("model '{}' deleted from disk", model.id);
    Ok(())
}

/// Whether the Notetaker can run at all, and if not, the one sentence the empty
/// state shows (YV93, plan finding #38).
///
/// 22-A is English end to end — Parakeet EN with `lang_detect:false` and no
/// meeting-language path — while the app's "Language I speak" picker implies
/// otherwise. A user who set that picker to Spanish and recorded a lecture would
/// get a page of plausible English nonsense, which is worse than an error
/// because it looks like output. The surface that consumes this lands with the
/// Meetings UI (YV95); the decision itself is here so it is made in one place.
///
/// YV101 — `available`/`message` answer "can the Notetaker record at all?"
/// (mic-only, the macOS 12 floor). `systemAudioAvailable`/`systemAudioMessage`
/// answer the separate question "can it also record the other end of the call?",
/// which needs macOS 14.4 (plan finding OS-11).
///
/// Two fields rather than one because the honest state on a macOS 13 Mac is
/// *both* at once: meeting recording works, the system-audio control is
/// visible and disabled with a sentence. Collapsing them into one boolean is
/// how "system audio needs 14.4" turns into "meetings don't work on your Mac".
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotetakerStatus {
    pub available: bool,
    pub message: Option<String>,
    pub system_audio_available: bool,
    pub system_audio_message: Option<String>,
}

impl NotetakerStatus {
    /// The payload, computed from an OS version rather than from *this* Mac's.
    ///
    /// The command below is a `State` read plus this call, so the decision the
    /// Notetaker's Settings step renders is reachable from a test on any
    /// machine — which is what lets matrix row `12b` be published as `Test`
    /// rather than as a policy nothing checks. It is one declaration, not a
    /// second copy: `notetaker_status` has no arithmetic of its own left.
    ///
    /// The two questions stay separate on purpose. `available`/`message` answer
    /// "can the Notetaker record at all?" (mic-only, the macOS 12 floor);
    /// `system_audio_*` answer "can it also record the other end of the call?"
    /// (macOS 14.4, plan finding OS-11). On a macOS 13 Mac the honest state is
    /// both at once — meetings record, the system-audio control is visible and
    /// disabled with a sentence — and collapsing them is how "system audio
    /// needs 14.4" becomes "meetings do not work on your Mac".
    pub fn for_os(model_id: &str, language: &str, os: os_version_gate::OsVersion) -> Self {
        let mic_only = meeting_asr::meeting_availability_for(
            meeting_asr::MeetingCapture::MicOnly,
            Some(model_id),
            Some(language),
            os,
        );
        let system_audio = meeting_asr::meeting_availability_for(
            meeting_asr::MeetingCapture::MicPlusSystemAudio,
            Some(model_id),
            Some(language),
            os,
        );
        Self {
            available: mic_only.is_ok(),
            message: mic_only.err().map(|blocked| blocked.message()),
            system_audio_available: system_audio.is_ok(),
            system_audio_message: system_audio.err().map(|blocked| blocked.message()),
        }
    }
}

#[tauri::command]
async fn notetaker_status(state: State<'_, Arc<AppState>>) -> Result<NotetakerStatus, String> {
    let (model_id, language) = {
        let settings = state.settings.lock();
        (settings.native_model.clone(), settings.language.clone())
    };
    Ok(NotetakerStatus::for_os(
        &model_id,
        &language,
        os_version_gate::OsVersion::current(),
    ))
}

/// Warm-engine lifecycle snapshot: what is resident, whether a load or a
/// transcription is in flight, and how close the idle watcher is to unloading.
#[tauri::command]
async fn engine_status(
    state: State<'_, Arc<AppState>>,
) -> Result<transcription::EngineStatus, String> {
    Ok(state.transcription.status())
}

/// A newer Yap release the user has not dismissed (YV44). Returned by
/// `check_for_update` and rendered as the "Update available" prompt; the
/// release bytes are NOT touched until the user asks for them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
}

/// Has the user pressed "Skip this version" on exactly this release (YV44)?
/// Pure so the skip rule is testable: only that one version is suppressed, so a
/// later release is always offered again.
fn update_is_skipped(version: &str, skipped: Option<&str>) -> bool {
    skipped.is_some_and(|s| s == version)
}

/// Every updater endpoint this build will try, in the order the plugin tries
/// them, read straight out of the compiled `tauri.conf.json` so it can never
/// drift from what the plugin actually contacts (UPD-A).
fn updater_endpoints(app: &AppHandle) -> Vec<String> {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("endpoints"))
        .and_then(|e| e.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// UPD-A: when the check fails on EVERY endpoint, the manual "Check for
/// updates" button has to say *which* host it could not reach. The endpoints
/// have moved twice already (GitHub release assets died when the repo went
/// private; the DMG then moved to the site host), and a bare "couldn't check
/// for updates" over a dead URL is indistinguishable from being offline — that
/// ambiguity is the whole reason this item exists. No key material is ever in
/// an endpoint URL, so this is safe to surface and to log.
fn describe_update_failure(err: &str, endpoints: &[String]) -> String {
    if endpoints.is_empty() {
        return format!("{err} (no update endpoint is configured in this build)");
    }
    format!("{err} — tried {}", endpoints.join(", "))
}

/// UPD-A: pick the NEWEST offer across every endpoint, never the first one that
/// answered. The plugin's own endpoint loop stops at the first 2xx manifest that
/// parses and never compares versions across endpoints, so a hand-staged primary
/// left on an older release would hide a newer release the fallback already
/// serves — every install would read "already on the latest Yap" until someone
/// re-staged the primary. Pure and generic so the rule is testable without a
/// network: `offers` is `(announced version, payload)`. An unparseable version
/// never beats a parseable one; ties keep the earlier (higher-priority) endpoint.
fn pick_newest_offer<T>(offers: Vec<(String, T)>) -> Option<T> {
    let mut best: Option<(Option<semver::Version>, T)> = None;
    for (version, payload) in offers {
        let parsed = semver::Version::parse(version.trim_start_matches('v')).ok();
        let better = match &best {
            None => true,
            Some((Some(b), _)) => parsed.as_ref().is_some_and(|p| p > b),
            Some((None, _)) => parsed.is_some(),
        };
        if better {
            best = Some((parsed, payload));
        }
    }
    best.map(|(_, payload)| payload)
}

/// Fold every endpoint's answer into the one the user sees (UPD-A). Any offer
/// wins (newest across endpoints, see `pick_newest_offer`); otherwise an
/// endpoint that answered "you are current" wins over one that failed, so a
/// primary that serves junk (an SPA catch-all answering 200 text/html, a 5xx)
/// can never switch off the fallback the way the plugin's `res.json()?` early
/// return would. Only when NO endpoint gave a usable answer and at least one
/// failed for a reason other than "no manifest" is it an error, and the error
/// names each endpoint with its own failure.
fn fold_endpoint_answers<T>(
    answers: Vec<(String, EndpointAnswer<T>)>,
) -> Result<Option<T>, String> {
    let mut offers = Vec::new();
    let mut up_to_date = false;
    let mut failures = Vec::new();
    for (endpoint, answer) in answers {
        match answer {
            EndpointAnswer::Offer(version, payload) => offers.push((version, payload)),
            EndpointAnswer::UpToDate => up_to_date = true,
            EndpointAnswer::NoManifest => {}
            EndpointAnswer::Failed(e) => failures.push(format!("{endpoint}: {e}")),
        }
    }
    if let Some(best) = pick_newest_offer(offers) {
        return Ok(Some(best));
    }
    if up_to_date || failures.is_empty() {
        return Ok(None);
    }
    Err(failures.join("; "))
}

/// What ONE endpoint said, on its own (UPD-A). Generic over the offer payload
/// so the fold is pure and testable without constructing a plugin `Update`.
enum EndpointAnswer<T> {
    Offer(String, T),
    UpToDate,
    NoManifest,
    Failed(String),
}

/// Ask EVERY configured endpoint on its own and return the newest release any
/// of them offers (UPD-A). Both `check_for_update` and `install_update` go
/// through here, so the bytes installed always come from the manifest that won
/// the check — never from a different endpoint's manifest.
async fn check_every_endpoint(
    app: &AppHandle,
) -> Result<Option<tauri_plugin_updater::Update>, String> {
    let endpoints = updater_endpoints(app);
    if endpoints.is_empty() {
        return Err(describe_update_failure("updater unavailable", &endpoints));
    }
    let mut answers = Vec::with_capacity(endpoints.len());
    for endpoint in endpoints {
        let answer = match tauri::Url::parse(&endpoint) {
            Err(e) => EndpointAnswer::Failed(format!("not a URL ({e})")),
            Ok(url) => match app
                .updater_builder()
                .endpoints(vec![url])
                .and_then(|b| b.build())
            {
                Err(e) => EndpointAnswer::Failed(e.to_string()),
                Ok(updater) => match updater.check().await {
                    Ok(Some(update)) => EndpointAnswer::Offer(update.version.clone(), update),
                    Ok(None) => EndpointAnswer::UpToDate,
                    Err(tauri_plugin_updater::Error::ReleaseNotFound) => EndpointAnswer::NoManifest,
                    Err(e) => EndpointAnswer::Failed(e.to_string()),
                },
            },
        };
        if let EndpointAnswer::Failed(e) = &answer {
            log::debug!("update endpoint {endpoint} failed: {e}");
        }
        answers.push((endpoint, answer));
    }
    fold_endpoint_answers(answers)
}

/// Ask the release endpoint whether a newer Yap exists — and ONLY ask (YV44).
/// Nothing downloads, nothing installs, nothing relaunches here; the answer is
/// handed to the UI, which prompts. Returns `None` when updates are turned off,
/// when there is nothing newer, when the user skipped this exact version, or
/// when no manifest is published yet (every endpoint answers 404 for
/// `latest.json` until the first release is staged — that is the NORMAL state,
/// so it is DEBUG, not the launch-time ERROR the plugin logs on its own). A
/// genuine failure (offline, malformed manifest) is returned as an error —
/// naming the endpoints it tried, see `describe_update_failure` — so the manual
/// "Check for updates" button can say so; the launch-time check ignores it.
#[tauri::command]
async fn check_for_update(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<UpdateInfo>, String> {
    let (enabled, skipped) = {
        let s = state.settings.lock();
        (s.check_updates, s.skipped_update_version.clone())
    };
    if !channel::current().updater_enabled {
        log::info!(
            "update check skipped: updater DISABLED in the {} channel",
            channel::CURRENT.name()
        );
        return Ok(None);
    }
    if !enabled {
        log::debug!("update check off (checkUpdates=false) — not contacting the endpoint");
        return Ok(None);
    }
    if app.updater().is_err() {
        log::debug!("updater unavailable in this build");
        return Ok(None);
    }
    match check_every_endpoint(&app).await {
        Ok(Some(update)) => {
            if update_is_skipped(&update.version, skipped.as_deref()) {
                log::debug!(
                    "update {} available but skipped by the user",
                    update.version
                );
                return Ok(None);
            }
            log::info!(
                "update available: {} (running {})",
                update.version,
                update.current_version
            );
            Ok(Some(UpdateInfo {
                version: update.version.clone(),
                current_version: update.current_version.clone(),
                notes: update.body.clone(),
            }))
        }
        Ok(None) => {
            log::debug!("update check: already on the latest Yap (or no manifest published yet)");
            Ok(None)
        }
        Err(e) => {
            let detail = describe_update_failure(&e, &updater_endpoints(&app));
            log::warn!("update check failed: {detail}");
            Err(detail)
        }
    }
}

/// Download + install a release the user just approved (YV44). This is the ONLY
/// path in Yap that calls `download_and_install`, and the only caller is the
/// "Install now" button in the update prompt — no launch-time install, ever.
/// Re-checks first so the bytes installed are the release the prompt named
/// (signature verification against `plugins.updater.pubkey` happens inside the
/// plugin). The new bundle applies on the next launch; we do not restart the
/// app out from under a dictation.
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<String, String> {
    if !channel::current().updater_enabled {
        return Err(format!(
            "updates are disabled in the {} channel — rebuild with scripts/test-build.sh",
            channel::CURRENT.name()
        ));
    }
    app.updater().map_err(|e| e.to_string())?;
    // UPD-A: the same every-endpoint, newest-wins check the prompt came from,
    // so the tarball downloaded is the one named by the manifest that won.
    let update = check_every_endpoint(&app)
        .await?
        .ok_or_else(|| "No update available anymore — you're on the latest Yap.".to_string())?;
    let version = update.version.clone();
    log::info!("installing Yap {version} (user-approved)");
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| {
            log::error!("update install failed: {e}");
            e.to_string()
        })?;
    log::info!("Yap {version} installed — applies on relaunch");
    Ok(version)
}

/// Open the transcript DB without panicking at launch. A transient failure
/// (another instance briefly holding the lock, disk momentarily full) is retried
/// once after a short backoff; if it still fails we degrade to an in-memory DB so
/// the app stays usable this session — history won't persist, but the process
/// never crashes on startup. (Genuine corruption is already self-healed inside
/// `Database::open` via quarantine.)
fn open_db_graceful(db_path: std::path::PathBuf) -> Database {
    match Database::open(db_path.clone()) {
        Ok(db) => db,
        Err(e) => {
            log::error!("DB open failed ({e}); retrying once after backoff");
            std::thread::sleep(std::time::Duration::from_millis(500));
            match Database::open(db_path) {
                Ok(db) => {
                    log::info!("DB opened on retry");
                    db
                }
                Err(e2) => {
                    log::error!(
                        "DB open failed again ({e2}); running with an IN-MEMORY DB — \
                         transcripts will NOT persist this session"
                    );
                    Database::open_in_memory().unwrap_or_else(|e3| {
                        // Opening an in-memory SQLite DB does not realistically fail;
                        // if it does, the sqlite runtime itself is unusable. Exit
                        // gracefully with a logged reason instead of panicking.
                        log::error!("in-memory DB fallback also failed ({e3}); exiting");
                        std::process::exit(1);
                    })
                }
            }
        }
    }
}

/// First name of the macOS account (`id -F` full name), for UI greetings.
/// Empty string when unavailable — the UI must not assume a name exists.
#[tauri::command]
fn user_display_name() -> String {
    std::process::Command::new("id")
        .arg("-F")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .and_then(|full| full.split_whitespace().next().map(str::to_string))
        .unwrap_or_default()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // TC-1 (review BLOCKING 1): before ANYTHING touches the data dir — the CLI
    // mode, the smoke preflight, the logger — refuse to start when the bundle
    // id compiled into the Tauri context is not the one this binary's channel
    // names. Fails closed: a mismatched pair would read/write the wrong state
    // root (or ship a production id with the updater off).
    let context = tauri::generate_context!();
    if let Err(why) =
        channel::check_bundle_identity(channel::CURRENT, &context.config().identifier)
    {
        eprintln!("{why}");
        std::process::exit(78); // EX_CONFIG
    }

    // YV32 headless mode (`--transcribe-file <wav>`): transcribe with the
    // embedded engine, print the text, exit. Handled FIRST so no window, tray,
    // hotkey tap, or app logger is ever created for a one-shot CLI run.
    if let Some(code) = cli::handle_args() {
        std::process::exit(code);
    }

    // Y0-D `--smoke`: a launch an automated agent may make. It registers no
    // global hotkey and synthesizes no paste, and it REFUSES TO START unless
    // `YAP_DATA_DIR` points outside the default state root — a smoke run that
    // silently used the default root would be reading and writing the real
    // dictation history, which is the whole failure this mode prevents. Decided
    // before the logger is initialised, because the logger itself writes under
    // the data dir.
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match smoke::preflight(&argv, std::env::var(DATA_DIR_ENV).ok().as_deref()) {
        Err(refusal) => {
            eprintln!("{}", refusal.message());
            std::process::exit(refusal.exit_code());
        }
        Ok(true) => smoke::arm(),
        Ok(false) => {}
    }
    // yap-p0-release-smoke: `--release-smoke` registers the hotkeys, probes every
    // service, makes ONE insertion into TextEdit and quits — under the same
    // scratch-root rule as `--smoke`.
    match smoke::release_preflight(&argv, std::env::var(DATA_DIR_ENV).ok().as_deref()) {
        Err(refusal) => {
            eprintln!("{}", refusal.message());
            std::process::exit(refusal.exit_code());
        }
        Ok(true) => smoke::arm_release(),
        Ok(false) => {}
    }

    // YV7: structured rotating file logging under data_dir()/logs/ (yap.log) +
    // a panic hook — keeps the console output and mirrors it to disk for support.
    logging::init(&data_dir());
    // TC-1: name the channel first, so a log proves which app wrote it.
    log::info!("{}", channel::startup_line());

    // yap-lifecycle DO 5 — the stale-instance probe. The single-instance
    // plugin's handoff is a one-way write followed by a silent exit(0), so a
    // running Yap whose main thread is wedged swallowed every relaunch. Hand
    // off here first and wait (bounded) for the running app's ack.
    match lifecycle::handoff_to_running(
        &lifecycle::handoff_socket(instance_identifier()),
        &lifecycle::handoff_ack_file(instance_identifier()),
        &std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        &std::env::args().collect::<Vec<_>>(),
        lifecycle::HANDOFF_ACK_TIMEOUT,
    ) {
        lifecycle::HandoffVerdict::NoRunningInstance => {}
        lifecycle::HandoffVerdict::Handed { ack_ms } => {
            log::info!("second launch: running Yap presented its window (ack {ack_ms}ms) — exiting");
            std::process::exit(0);
        }
        lifecycle::HandoffVerdict::Stale { waited_ms } => {
            let msg = lifecycle::stale_message(waited_ms);
            log::warn!("second launch: stale instance — {msg}");
            eprintln!("{msg}");
            notify_stale_instance(&msg);
            std::process::exit(1);
        }
        lifecycle::HandoffVerdict::Unknown(e) => {
            log::warn!("second launch: handoff probe inconclusive ({e}) — launching normally");
        }
    }

    // YV20/M3: sweep any stale voice WAVs left by a prior crash / hard-kill (the
    // happy path deletes its own clip, but a SIGKILL mid-transcribe can't). No
    // recording is in flight at startup, so every *.wav here is safe to remove.
    let swept = record::sweep_stale_wavs(&data_dir().join("recordings"));
    if swept > 0 {
        log::info!("startup: swept {swept} stale voice wav(s) from recordings dir");
    }

    let db_path = data_dir().join("wilson_voice.db");
    let db = open_db_graceful(db_path);

    // YV41: never reset a whole config over one bad field — salvage what parses,
    // quarantine a wholly corrupt file, migrate older schemas forward.
    let settings_path = data_dir().join("settings.json");
    let mut settings: AppSettings = load_settings(&settings_path);

    // YV46: the pre-SQLite `history/transcripts.json` import is a FIRST-RUN
    // step, not a per-launch one. It used to stat that path on every startup
    // for a file no install made after the SQLite switch can have; now the one
    // attempt is recorded in settings and never repeated (the import itself
    // also renames the file aside, so this is belt-and-braces, not the only
    // guard). Persisting the flag is best-effort: a failed write just means the
    // next launch re-checks a file that is no longer there.
    if !settings.legacy_json_migrated {
        let legacy = data_dir().join("history").join("transcripts.json");
        match db.migrate_json_if_needed(legacy) {
            Ok(n) if n > 0 => log::info!("migrated {n} legacy transcripts into SQLite"),
            Err(e) => log::warn!("legacy migrate: {e}"),
            _ => {}
        }
        settings.legacy_json_migrated = true;
        if let Err(e) = write_settings_file(&settings_path, &settings) {
            log::warn!("could not record the legacy-migration flag ({e})");
        }
    }
    // DB-B: one sweep for everything the previous run left on disk. It purges
    // on the 7-day window FIRST, then rebuilds both kinds of orphan — a take
    // that died while capturing (YV63's marker + spill) and a long take that
    // died while DECODING (Y3-A's spill, plus whatever chunk text it had already
    // persisted) — into rows the user can finish. It decodes nothing and pastes
    // nothing; see its doc comment for why both of those are deliberate.
    let rebuilt = recover_dictation(&db, &recovery_dir());
    if rebuilt > 0 {
        log::info!("startup: recovered {rebuilt} dictation(s) the app died in the middle of");
    }
    // YV64: the app has been crashing on real machines without ever knowing it.
    // Read macOS' own reports for our processes, plus the panic hook's own log
    // lines, into `crash_events` — the UI raises ONE toast if any of them is
    // news. Local only: nothing here is ever uploaded (see `crash`'s docs).
    let fresh_crashes = crash::ingest(&db, &home_dir(), &logging::logs_dir(&data_dir()));
    if fresh_crashes > 0 {
        log::warn!("startup: {fresh_crashes} crash(es) from previous session(s) recorded");
    }

    let db = Arc::new(db);

    // YP2: licensing state comes up BEFORE the app state that will hold it, so
    // the trial's first write happens once, on a DB that is already open.
    // yap-lic-00: launch init is local-only — `NoNetwork` is the transport, and
    // `launch_license_init_makes_no_network_call` pins that it is never used.
    let license_manager = Arc::new(license::launch_init(
        &data_dir(),
        db.clone() as Arc<dyn license::LicenseStore>,
        &license::NoNetwork,
    ));
    log::info!(
        "license: {}",
        match license_manager.status().entitlement {
            license::Entitlement::Licensed { ref plan, .. } => format!("licensed ({plan})"),
            license::Entitlement::Trial { days_left, .. } => format!("trial, {days_left}d left"),
            license::Entitlement::LicenseRequired { ref reason } =>
                format!("new dictation gated ({reason})"),
        }
    );

    let state = Arc::new(AppState {
        settings: PLMutex::new(settings),
        recording: PLMutex::new(false),
        busy: PLMutex::new(false),
        hands_free: PLMutex::new(false),
        session_cap_warned: PLMutex::new(false),
        command_selection: PLMutex::new(None),
        recorder: PLMutex::new(None),
        saved_audio: PLMutex::new(None),
        db,
        last_error: PLMutex::new(None),
        hotkey_arbiter: PLMutex::new(hotkeys::HotkeyArbiter::new()),
        tray: PLMutex::new(None),
        tray_dictation: PLMutex::new(None),
        tray_hands_free: PLMutex::new(None),
        tray_paste_raw: PLMutex::new(None),
        tray_meeting: PLMutex::new(None),
        tray_meeting_kind: PLMutex::new(None),
        tray_license: PLMutex::new(None),
        tray_upgrade: PLMutex::new(None),
        tray_menu: PLMutex::new(None),
        tray_upgrade_shown: PLMutex::new(false),
        tray_urgent: PLMutex::new(false),
        meeting: std::sync::OnceLock::new(),
        undo_available: PLMutex::new(false),
        secure_input: PLMutex::new(secure_input::SecureInputStatus::default()),
        vad: PLMutex::new(None),
        paste_generation: AtomicU64::new(0),
        take_cancelled: AtomicBool::new(false),
        license: license_manager,
        transcription: transcription::TranscriptionManager::new(),
        support_bundle: PLMutex::new(None),
    });

    // yap-meetings-fix (A, B) — the meeting transcription pipeline: ONE worker
    // thread, the shipped MeetingAsr driver over the one warm engine, and the
    // shipped EngineDemand so dictation always wins. Then reconcile every row a
    // previous run stranded mid-pipeline — BEFORE the first hygiene sweep, so
    // retention never sees a row whose fate is not decided yet.
    {
        let asr_state = state.clone();
        let sum_state = state.clone();
        let deps = meeting_pipeline::PipelineDeps {
            db: state.db.clone(),
            progress_dir: meetings_dir(),
            asr: Arc::new(move || {
                let (native, language) = {
                    let s = asr_state.settings.lock();
                    (s.native_model.clone(), s.language.clone())
                };
                let (model_id, model_path) = native_model_ready(&native).ok_or_else(|| {
                    "No speech model is downloaded yet — get one in Settings, then press Retry."
                        .to_string()
                })?;
                meeting_asr::meeting_availability(Some(&model_id), Some(&language))
                    .map_err(|e| e.message())?;
                Ok(Box::new(meeting_asr::WarmEngineChunkAsr::new(
                    asr_state.transcription.clone(),
                    model_id,
                    model_path,
                    Some("en".to_string()),
                    None,
                )) as Box<dyn meeting_asr::ChunkAsr>)
            }),
            demand: Arc::new(state.transcription.clone()),
            quit: Arc::new(|| false),
            sink: Arc::new(|event: &meeting_pipeline::MeetingProgressEvent| {
                if let Some(app) = MEETING_APP.get() {
                    let _ = app.emit(meeting_pipeline::MEETING_PROGRESS_EVENT, event);
                }
            }),
            after_transcript: Some(Arc::new(move |id: &str| {
                auto_summarize_meeting(&sum_state, id)
            })),
            config: meeting_asr::MeetingAsrConfig::default(),
        };
        meeting_pipeline::install(meeting_pipeline::spawn(deps));
        let reconciled = meeting_control::reconcile_stranded_meetings(&state.db, &|id| {
            meeting_pipeline::enqueue(id);
        });
        if !reconciled.is_empty() {
            log::info!("reconcile: {} stranded meeting(s) handled", reconciled.len());
        }
    }

    // YV73: memory + disk hygiene, entirely off the dictation path — its own
    // thread, nothing shared with capture, and the first sweep deliberately
    // delayed past the startup I/O burst (stale-wav sweep, retention purge,
    // crash ingest, ASR preload). It then ticks the memory telemetry line every
    // ten minutes and re-sweeps the disk once a day, for as long as the app
    // runs. Everything it touches is a file WE wrote in our own data dir.
    {
        let db = state.db.clone();
        std::thread::Builder::new()
            .name("wv-hygiene".into())
            .spawn(move || {
                // YV80: the FIRST memory sample is taken right away rather than
                // after the sweep delay, because it is the baseline the lazy
                // load is measured against — this line and the one the first
                // dictation emits (`hygiene::log_snapshot_async`) bracket the
                // model, so the rss_mb delta between them IS the engine.
                log::info!("{}", hygiene::collect(&data_dir()).summary_line());
                std::thread::sleep(hygiene::STARTUP_SWEEP_DELAY);
                run_disk_sweep(&db);
                let mut since_sweep = Duration::ZERO;
                loop {
                    log::info!("{}", hygiene::collect(&data_dir()).summary_line());
                    // YV81: the standby line — how many recurring polls are
                    // ticking, whether the pill is animating, and whether a
                    // polish process is still resident — plus the idle unload
                    // of that process. Both ride THIS tick rather than a timer
                    // of their own, which is the point of an energy pass.
                    polish::sweep_idle_sidecar();
                    log::info!("{}", hygiene::collect_energy().summary_line());
                    std::thread::sleep(hygiene::TELEMETRY_INTERVAL);
                    since_sweep += hygiene::TELEMETRY_INTERVAL;
                    if since_sweep >= hygiene::SWEEP_INTERVAL {
                        since_sweep = Duration::ZERO;
                        run_disk_sweep(&db);
                    }
                }
            })
            .ok();
    }

    // YV36: fetch the Silero v4 VAD model ONCE in the background (never
    // HuggingFace — a direct URL, sha256-checked, cached under Application
    // Support, then OFFLINE forever), then build the WARM VAD once and park it
    // in the app state. Both steps are off the hot path so they never delay a
    // dictation, and a failure at either step simply leaves `state.vad` as
    // `None` → the capture pipeline stays on the energy VAD.
    {
        let state = state.clone();
        std::thread::spawn(move || {
            let Some(path) = vad::ensure_model(&data_dir()) else {
                log::warn!("startup: no silero VAD model — energy VAD only");
                return;
            };
            match vad::WarmVad::load(&path) {
                Ok(warm) => {
                    *state.vad.lock() = Some(Arc::new(warm));
                    log::info!("startup: warm silero VAD loaded from {}", path.display());
                }
                Err(e) => log::warn!("startup: silero VAD load failed, energy VAD only: {e}"),
            }
        });
    }

    // YV80: what launch does with the ASR engine is now a setting, and its
    // default is LAZY (see `preload_engine_at_startup`). Still on the blocking
    // pool rather than the main thread, because the eager path it can take is
    // the same multi-second blocking load YV38 introduced.
    {
        let state = state.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let settings = state.settings.lock().clone();
            preload_engine_at_startup(
                &state.transcription,
                settings.preload_model,
                native_model_ready(&settings.native_model),
            );
        });
    }

    tauri::Builder::default()
        // YV42 single instance — MUST stay the FIRST plugin (Handy's order): a
        // second `open -a Yap` is caught here and handed to the running app
        // before any other plugin in this chain has taken a lock the duplicate
        // would grab too. Two live processes would fight over the CGEvent FN
        // tap, the ⌃⌘V global shortcut, the pasteboard transaction and the
        // SQLite WAL. The headless `--transcribe-file` run never reaches this
        // builder (cli::handle_args exits above), so a CLI transcription still
        // works while the app is open.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            log::info!("second instance launched — focusing the running app");
            // The callback runs on the async runtime; AppKit wants the main
            // thread. The ack is written only AFTER the window was presented,
            // so a launch waiting on it (lifecycle::handoff_to_running) learns
            // that the main thread is alive, not merely the socket.
            let h = app.clone();
            let _ = app.run_on_main_thread(move || {
                present_main_window(&h, lifecycle::ActivationSource::SecondInstance);
                lifecycle::write_handoff_ack(&lifecycle::handoff_ack_file(instance_identifier()));
            });
        }))
        // YV42 launch at login. The LaunchAgent variant is the macOS-supported
        // one (no deprecated login-item AppleScript); the `autostart` setting is
        // applied in setup() below and on every save, so it stays authoritative.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        // Auto-update: checks the GitHub Releases updater manifest (see
        // tauri.conf.json plugins.updater) and installs signed DMG updates.
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Real NSPanel support for Dictate island (macOS)
        .plugin(tauri_nspanel::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler({
                    // yap-p0-chord-deadlock — the plugin calls this with its
                    // shortcut table LOCKED, and every `global_shortcut()` call
                    // takes that lock, so the handler only ROUTES: the job runs
                    // on the main thread after the handler returned (see
                    // `shortcut_defer` and `run_shortcut_job`).
                    let on_shortcut = shortcut_defer::handler(
                        {
                            let state = state.clone();
                            move |app: AppHandle, job| {
                                let (h, st) = (app.clone(), state.clone());
                                if let Err(e) =
                                    app.run_on_main_thread(move || run_shortcut_job(&h, &st, job))
                                {
                                    log::warn!("shortcut {job:?} dropped: main thread unavailable ({e})");
                                }
                            }
                        },
                        {
                            let state = state.clone();
                            move || state.settings.lock().keep_cmd_shift_v
                        },
                        || hotkeys::active_dictate().0,
                    );
                    move |app, shortcut, event| {
                        on_shortcut(app.clone(), shortcut, event.state == ShortcutState::Pressed)
                    }
                })
                .build(),
        )
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            hotkey_bindings,
            // Y3-D — the pill's Cancel button and the Escape hotkey both land here.
            cancel_transcription,
            user_display_name,
            get_settings,
            formatting_options,
            save_settings,
            pill_set_hitbox,
            pending_copy_card,
            dismiss_copy_card,
            pill_drag_start,
            pill_drag_move,
            pill_drag_end,
            get_history,
            clear_history,
            delete_entry,
            get_status,
            get_permissions,
            request_accessibility,
            repair_permission,
            relaunch_app,
            request_microphone,
            microphone_status,
            get_insights,
            daily_series,
            monthly_series,
            list_dictionary,
            add_dictionary_term,
            update_dictionary_term,
            set_dictionary_starred,
            delete_dictionary_term,
            correct_transcript,
            list_failed_dictations,
            retry_failed_dictation,
            discard_failed_dictation,
            list_dict_candidates,
            promote_dict_candidate,
            dismiss_dict_candidate,
            list_snippets,
            add_snippet,
            update_snippet,
            set_snippet_enabled,
            delete_snippet,
            list_scratch,
            save_scratch,
            delete_scratch,
            copy_entry,
            paste_entry,
            set_take_feedback,
            open_data_dir,
            open_logs_dir,
            list_crash_events,
            acknowledge_crash_events,
            clear_crash_events,
            preview_support_bundle,
            send_support_bundle,
            // YV94 — Meetings. Read, export and delete only; starting one is
            // YV95's surface.
            list_meetings,
            summarize_meeting,
            retry_meeting,
            get_meeting,
            rename_meeting,
            delete_meeting,
            export_meeting_markdown,
            // YV96 — the one-time capture notice. Two reads and one write on a
            // single `settings_kv` row; nothing here can block a recording.
            meeting_consent,
            acknowledge_meeting_consent,
            system_audio_setup,
            run_system_audio_setup,
            // YV95 — the entry point finding #6 says the phase cannot merge
            // without.
            meeting_status,
            toggle_meeting_recording,
            meeting_kind_choices,
            start_meeting,
            stop_meeting,
            export_history,
            open_privacy_settings,
            manual_toggle,
            show_main,
            claim_hide_hint,
            list_models,
            download_model,
            list_polish_models,
            download_polish_model,
            clear_polish_model,
            select_model,
            delete_model,
            engine_status,
            notetaker_status,
            check_for_update,
            install_update,
            license_status,
            activate_license,
            deactivate_license,
            open_purchase_page
        ])
        .setup(move |app| {
            // Lightweight setup only — no hotkey register, no second window

            // YV95 — build the meeting controller first: the tray item below
            // reads `capture_available()` for its enabled state, and every entry
            // point resolves through `state.meeting()`.
            //
            // The capture engine itself is NOT installed here. It is YV91's
            // `meeting::MeetingSession` (PR #108 — RT-safe ring, host-time
            // anchors, bounded journal, power assertion, dictation fan-out), and
            // wiring it is one line at this exact spot, over a thin adapter
            // implementing `meeting_control::CaptureEngine` in terms of
            // `MeetingSession::start(SessionConfig)`:
            //
            //     meeting_control::install_capture_engine(Arc::new(<adapter>));
            //
            // Until that line exists the control plane is honest about it: the
            // tray item is disabled, the Meetings empty state says so in words,
            // and no code path pretends a recording is happening. Two capture
            // implementations in one app would be a far worse defect than a
            // button that tells the truth about what is missing.
            //
            // That honesty is time-boxed, not permanent: finding #6 is a phase
            // merge gate, and four disabled controls do not close it.
            // `tests/capture_engine_is_installed.rs` fails the build the moment
            // `src/meeting.rs` exists without the line above, so whichever of
            // #108 / #112 merges second cannot leave `main` with a dead Record
            // button.
            // YV91 (#108) is merged, so this is no longer a comment asking
            // someone to do it later: `meeting::MeetingSession` exists and the
            // Record button is wired to it. `tests/capture_engine_is_installed.rs`
            // is what keeps this line here.
            meeting_control::install_capture_engine(Arc::new(
                // YV110 — the engine holds the database because a meeting asks
                // it at T-0 whether this Mac attaches the system-audio track,
                // and writes back what the meeting actually heard on the way out.
                meeting_control::SessionEngine::new(state.db.clone()),
            ));
            let _ = MEETING_APP.set(app.handle().clone());
            let _ = state.meeting.set(Arc::new(meeting_control::MeetingController::new(
                state.db.clone(),
                meeting_status_sink(app.handle().clone()),
            )));

            // Regular app so Yap shows in the Dock, Launchpad, and Applications — it
            // has a real main window (and a social layer coming), not just a menubar
            // agent. The floating Dictate pill STILL hovers over other apps' fullscreen
            // Spaces: that comes from the NSPanel's collection behavior
            // (canJoinAllSpaces + fullScreenAuxiliary + nonactivating, see float_pill.rs),
            // which works regardless of activation policy — not from being .accessory.
            // LSUIElement is false in Info.plist to match.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Regular);

            // YV42: re-assert launch-at-login from the persisted setting, so the
            // toggle stays authoritative even if the .app moved (which strands
            // the old LaunchAgent) or an older install left one behind.
            apply_autostart(app.handle(), state.settings.lock().autostart);

            if let Some(win) = app.get_webview_window("main") {
                if smoke::release_armed() {
                    // yap-p0-release-smoke: the run inserts into TextEdit, so
                    // Yap must never take the foreground or key focus from it
                    // (tauri.conf.json creates "main" visible + focused).
                    let _ = win.hide();
                } else {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
                let w = win.clone();
                win.on_window_event(move |e| match e {
                    WindowEvent::CloseRequested { api, .. } => {
                        // The red X hides: Yap is a menu-bar utility and keeps
                        // listening for the hotkey. The FIRST hide ever makes the
                        // one-time hint pending and nudges the webview; the hint
                        // is consumed only when a mounted HideHint claims it
                        // (claim_hide_hint), so a hide during onboarding — when
                        // HideHint is not mounted — leaves it for the main view.
                        api.prevent_close();
                        let _ = w.hide();
                        if lifecycle::HIDE_HINT.note_hide(&data_dir()) {
                            let _ = w.emit(lifecycle::HIDE_HINT_EVENT, ());
                        }
                    }
                    // PERM-E: coming back to the window is the moment a grant
                    // flipped in System Settings becomes worth re-reading. The
                    // watcher is parked until this fires, so it costs one read,
                    // not a timer.
                    WindowEvent::Focused(true) => {
                        permissions::poke(permissions::WatchReason::Focus);
                    }
                    _ => {}
                });
            }

            // yap-perm-truth — the ONE app-activation observer, registered
            // beside the Focus poke above. Cmd-Tabbing back from System
            // Settings activates Yap whether or not the main window takes key,
            // and that is the moment a toggled grant is worth re-reading.
            permissions::observe_app_activation(|| {
                permissions::poke(permissions::WatchReason::Activate);
            });
            // The running binary's signature and the duplicate-bundle scan,
            // computed once, off the main thread, before the banner needs them.
            perm_truth::prime();
            // yap-p0-perm-repair: persist the last-run signature EVERY launch.
            perm_probe::prime();

            // PERM-E — the ONE permission watcher. It re-reads all four grants
            // on launch, on window focus, on wake (Y1-B's publisher, wired
            // below, calls `permissions::on_sleep_wake`; no second sleep/wake
            // observer is registered here) and on every hotkey/paste refusal,
            // and emits `permission_changed` only when something actually
            // changed. While nothing is denied it parks on a condvar with no
            // timer at all.
            {
                let read_state = state.clone();
                let emit_app = app.handle().clone();
                let tap_state = state.clone();
                permissions::start_watch(
                    move || {
                        let native = read_state.settings.lock().native_model.clone();
                        permissions::GrantSnapshot::from_report(&permissions::report(
                            false,
                            native_model_ready(&native).is_some(),
                        ))
                    },
                    move |snapshot| {
                        // yap-p0-hotkeys-no-permission DO 3: every permission
                        // edge re-attempts a tap that failed to install — a
                        // grant arriving later arms fn without a restart.
                        if smoke::global_hotkeys_allowed() {
                            let h = emit_app.clone();
                            let st = tap_state.clone();
                            let _ = emit_app.run_on_main_thread(move || {
                                hotkeys::attempt_tap(
                                    &OsPermissionProbe,
                                    &mut FnTapSource { app: &h, state: &st },
                                    &mut AppHotkeySink,
                                );
                            });
                        }
                        if let Err(e) =
                            emit_app.emit(permissions::PERMISSION_CHANGED_EVENT, snapshot)
                        {
                            log::warn!("permission_changed emit: {e}");
                        }
                    },
                );
            }

            // Y1-B — the ONE sleep/wake observer (`power.rs`), fanned out to its
            // three consumers. One publisher, three subscribers; nothing else in
            // `src/` registers NSWorkspace sleep/wake. Unregistered in
            // `RunEvent::Exit` below — an observer that outlives the app is a
            // crash on the next lid close.
            {
                let hub = Arc::new(power::SleepWakeHub::new());

                // Dictation (Y6-D): a take in flight when the lid closes is
                // parked through the cancel path — Y3-D keeps its clip
                // recoverable in History — so nothing pastes into whatever is
                // frontmost after wake, and nothing is lost either.
                let dict_app = app.handle().clone();
                let dict_state = state.clone();
                hub.subscribe(power::CONSUMER_DICTATION, move |event| {
                    let in_flight = *dict_state.recording.lock();
                    if power::dictation_should_park_take(event, in_flight) {
                        log::info!("sleep mid-take — parking the dictation take");
                        // Same path as a PTT interrupt; it settles the pill.
                        cancel_recording(&dict_app, &dict_state);
                        // fix-239: the take armed Escape as a GLOBAL shortcut
                        // at start, and `cancel_recording` does not release it
                        // (only `cancel_take` and the transcribe worker do). A
                        // sleep-cancel spawns no worker, so without this line
                        // the first Escape after wake — in any app — is eaten.
                        unregister_cancel_shortcut(&dict_app);
                    }
                });

                // Meetings (matrix row 16): the controller's truth drives
                // `SleepPolicy`; will-sleep finalizes the journal and stops the
                // meeting with the `paused_by_sleep` reason before the machine
                // goes down.
                let meet_app = app.handle().clone();
                let meet_state = state.clone();
                let policy = std::sync::Mutex::new(meeting_matrix::SleepPolicy::new());
                hub.subscribe(power::CONSUMER_MEETINGS, move |event| {
                    let Some(controller) = meet_state.meeting() else {
                        return;
                    };
                    // fix-239: the whole meeting-side behaviour lives in
                    // `MeetingController::on_sleep_wake`, which the row 16 test
                    // drives against a real controller; this closure only logs
                    // and settles the pill.
                    let settle_pill = || {
                        if !*meet_state.recording.lock() {
                            float_pill::after_recording(
                                &meet_app,
                                meet_state.settings.lock().show_floating_pill,
                            );
                        }
                    };
                    let mut policy = policy.lock().unwrap_or_else(|p| p.into_inner());
                    match controller.on_sleep_wake(&mut policy, event) {
                        meeting_control::SleepStep::Finalized => {
                            log::info!("meeting finalized on {event:?} (paused_by_sleep)");
                            settle_pill();
                        }
                        meeting_control::SleepStep::StopFailed(e) => {
                            log::warn!("meeting stop on {event:?}: {e}");
                            settle_pill();
                        }
                        meeting_control::SleepStep::ResumeOffered => {
                            // Row 16b: the offer has no UI yet, so it is logged.
                            log::info!("woke after a sleep-paused meeting — resume is a new segment");
                        }
                        meeting_control::SleepStep::Nothing => {}
                    }
                });

                // PERM-E: re-read every grant on wake.
                hub.subscribe(power::CONSUMER_PERMISSIONS, |event| {
                    permissions::on_sleep_wake(event);
                });

                power::install_sleep_wake_observer(hub);
            }

            // Menu-bar dropdown (Wispr parity): a full control surface so Yap is
            // driveable from the toolbar as well as the pill + hotkey (YV26).
            // `toggle` label + `hands_free` check are refreshed live by sync_tray.
            let recording_now = *state.recording.lock();
            // YV51: seed the "Undo AI Edit" guard from the newest stored take so
            // the item is correctly enabled/disabled on launch, not only after
            // the first dictation of the session.
            *state.undo_available.lock() = state
                .db
                .list_transcripts(1, None)
                .ok()
                .and_then(|e| e.into_iter().next())
                .map(|e| dictation::undo_ai_edit_text(&e.text, e.raw_text.as_deref()).is_some())
                .unwrap_or(false);
            let dictation_i = MenuItem::with_id(
                app,
                "toggle",
                if recording_now {
                    "Stop Dictation"
                } else {
                    "Start Dictation"
                },
                true,
                None::<&str>,
            )?;
            let hands_free_i = CheckMenuItem::with_id(
                app,
                "hands_free",
                "Hands-free",
                true,
                *state.hands_free.lock(),
                None::<&str>,
            )?;
            // "Paste Last Transcript" (⌃⌘V) — re-injects the newest transcript,
            // the marquee Wispr-parity action. The accelerator is also registered
            // globally below so it fires anywhere, not just when the menu is open.
            let paste_last_i = MenuItem::with_id(
                app,
                "paste_last",
                "Paste Last Transcript",
                true,
                Some("Ctrl+Cmd+V"),
            )?;
            // YV51 "Undo AI Edit" (⌃⌘Z) — re-pastes the RAW take over the
            // polished one. Starts disabled and is enabled by `sync_tray` only
            // when the newest take's raw actually differs from what was pasted.
            let paste_raw_i = MenuItem::with_id(
                app,
                "paste_raw",
                "Undo AI Edit (Paste Raw)",
                *state.undo_available.lock(),
                Some("Ctrl+Cmd+Z"),
            )?;
            // YV95 — the meeting entry point, one item above the dictation
            // controls' separator so it reads as its own mode rather than as a
            // variant of dictation. DISABLED (with the reason in the label) when
            // no capture engine is installed: finding #6 is about a feature
            // nobody can reach, and a button that silently does nothing is the
            // same defect wearing a nicer hat.
            let meeting_available = meeting_control::capture_available();
            let meeting_i = MenuItem::with_id(
                app,
                "meeting_toggle",
                if meeting_available {
                    MEETING_TRAY_START_LABEL.to_string()
                } else {
                    format!("{MEETING_TRAY_START_LABEL} (unavailable)")
                },
                meeting_available,
                // fix-249 advisory (e): the rebound chord, not the literal ⌃⌘M.
                Some(hotkey_table::meeting_accelerator(
                    state.settings.lock().meeting_chord.as_ref(),
                )),
            )?;
            // YV125 — the kind picker, as the qualified variant of the item
            // directly above it (the "Open" / "Open With ▸" shape macOS menus
            // already use). The plain item and ⌃⌘M start a meeting with the
            // question SKIPPED, which is a supported answer (`unknown`); this
            // submenu is for the user who wants to answer it. It is a hint that
            // improves the diarization target, never a gate — nothing here can
            // stop a recording from starting.
            let kind_items = meeting_control::KIND_PICKER
                .iter()
                .map(|choice| {
                    MenuItem::with_id(app, choice.menu_id, choice.label, true, None::<&str>)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let meeting_kind_i = Submenu::with_id_and_items(
                app,
                "meeting_kind",
                "Record a meeting as…",
                // No meeting can be running at setup, so this matches the plain
                // item's enabled state; the status sink flips both on the
                // recording edge.
                meeting_available,
                &kind_items
                    .iter()
                    .map(|i| i as &dyn tauri::menu::IsMenuItem<Wry>)
                    .collect::<Vec<_>>(),
            )?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let sep_meeting = PredefinedMenuItem::separator(app)?;
            // Y2-E — the license header, seeded from the same decision
            // `sync_tray` refreshes it with, so the menu bar is already correct
            // the first time it is opened rather than after the first state
            // transition. DISABLED because it is a label, not an action.
            let (license_line_now, _) = license::tray_line(&state.license.status());
            let license_i =
                MenuItem::with_id(app, "license_header", license_line_now, false, None::<&str>)?;
            let sep_license = PredefinedMenuItem::separator(app)?;
            // Y2-E — the upgrade line. Built unconditionally so `sync_tray`
            // owns a handle to it, but NOT placed in the menu below: it is
            // inserted only while `license::tray_offers_purchase` is true.
            let upgrade_i = MenuItem::with_id(
                app,
                "upgrade",
                license::UPGRADE_TRAY_LABEL,
                true,
                None::<&str>,
            )?;
            let show_i = MenuItem::with_id(app, "show", "Open Yap", true, None::<&str>)?;
            let settings_i = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
            let shortcuts_i =
                MenuItem::with_id(app, "shortcuts", "Keyboard Shortcuts…", true, None::<&str>)?;
            let sep3 = PredefinedMenuItem::separator(app)?;
            // Feedback/report affordance (Wispr's "Share feedback" / "Talk to
            // support") — opens the repo's new-issue page in the browser.
            let feedback_i =
                MenuItem::with_id(app, "feedback", "Send Feedback…", true, None::<&str>)?;
            let sep4 = PredefinedMenuItem::separator(app)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit Yap", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &license_i,
                    &sep_license,
                    &dictation_i,
                    &paste_last_i,
                    &paste_raw_i,
                    &sep_meeting,
                    &meeting_i,
                    &meeting_kind_i,
                    &sep1,
                    &hands_free_i,
                    &sep2,
                    &show_i,
                    &settings_i,
                    &shortcuts_i,
                    &sep3,
                    &feedback_i,
                    &sep4,
                    &quit_i,
                ],
            )?;
            // Keep handles so state transitions can reflect into the dropdown.
            *state.tray_dictation.lock() = Some(dictation_i);
            *state.tray_hands_free.lock() = Some(hands_free_i);
            *state.tray_paste_raw.lock() = Some(paste_raw_i);
            *state.tray_meeting.lock() = Some(meeting_i);
            *state.tray_meeting_kind.lock() = Some(meeting_kind_i);
            *state.tray_license.lock() = Some(license_i);
            *state.tray_upgrade.lock() = Some(upgrade_i);
            *state.tray_menu.lock() = Some(menu.clone());
            // Menu-bar TEMPLATE icon: a monochrome Yappy silhouette. `icon_as_template`
            // lets macOS tint it for the light/dark menu bar — a full-color app icon
            // crammed into the status bar looks wrong (that was the "terrible toolbar").
            let icon = tauri::image::Image::from_bytes(include_bytes!(
                "../icons/tray-template.png"
            ))
            .map_err(|e| format!("tray template icon: {e}"))?;

            let tray = TrayIconBuilder::new()
                .icon(icon)
                .icon_as_template(true)
                .menu(&menu)
                // Left-click opens the dropdown (Wispr behavior). Previously a
                // left-click only toggled the main window and the menu hid on
                // right-click, so it read as "no dropdown". The window is now
                // reached via the "Open Yap" item.
                .show_menu_on_left_click(true)
                .tooltip("Yap — hold fn to dictate")
                .on_menu_event({
                    let state = state.clone();
                    move |app, event| match event.id.as_ref() {
                        "quit" => {
                            // yap-lifecycle — the SAME quit as Cmd-Q and the
                            // Dock's Quit: app.exit(0) reaches RunEvent::Exit,
                            // whose quit_teardown orders the pill out first and
                            // then stops a live meeting (YV95) and checkpoints
                            // the WAL, every step bounded.
                            app.exit(0);
                        }
                        "show" => {
                            present_main_window(app, lifecycle::ActivationSource::TrayOpen);
                        }
                        "settings" => {
                            // Open the app AND jump it to the Settings view.
                            present_main_window(app, lifecycle::ActivationSource::TrayOpen);
                            let _ = app.emit("navigate", "settings");
                        }
                        "paste_last" => {
                            paste_last_transcript(app, &state);
                        }
                        // Y2-E — the upgrade path, from the menu bar. The item
                        // is only ever IN the menu when
                        // `license::tray_offers_purchase` said so, so this arm
                        // cannot fire for a licensed Mac or a stored-key holder.
                        "upgrade" => {
                            reveal_purchase_prompt(app);
                        }
                        // YV95 — start/stop a meeting from the menu bar. The
                        // picker is skipped on this path: `unknown`.
                        "meeting_toggle" => {
                            toggle_meeting_off_thread(
                                app,
                                &state,
                                "the tray item",
                                meetings::MeetingKind::Unknown,
                            );
                        }
                        // YV125 — the same start, with the kind the user chose.
                        id if meeting_control::kind_for_menu_id(id).is_some() => {
                            let kind = meeting_control::kind_for_menu_id(id)
                                .expect("the guard above matched this id");
                            toggle_meeting_off_thread(app, &state, "the tray kind picker", kind);
                        }
                        "paste_raw" => {
                            paste_raw_last_transcript(app, &state);
                        }
                        "shortcuts" => {
                            // Open Settings and hint the Shortcut sub-tab.
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                            let _ = app.emit("navigate", "settings");
                            let _ = app.emit("settings-tab", "shortcut");
                        }
                        "feedback" => {
                            // Open the repo's new-issue page in the default browser.
                            let _ = std::process::Command::new("open")
                                .arg("https://github.com/wilsonguenther-dev/wilson-voice/issues/new")
                                .spawn();
                        }
                        "hands_free" => {
                            if *state.hands_free.lock() {
                                // Turning hands-free OFF finalizes the take, mirroring
                                // the physical "tap-to-end-hands-free" path — clear
                                // both the app-side and PTT tap-side latches, then
                                // transcribe what was captured.
                                stop_take(app, &state, StopOrigin::MenuBar);
                            } else {
                                // Turning hands-free ON: ensure a take is running,
                                // then latch it so key-release / stop won't finalize.
                                if !*state.recording.lock() && !*state.busy.lock() {
                                    start_recording(app, &state);
                                }
                                if *state.recording.lock() {
                                    *state.hands_free.lock() = true;
                                }
                                emit_status(app, &state);
                            }
                        }
                        "toggle" => {
                            if *state.recording.lock() {
                                // Tray Stop also exits hands-free (see manual_toggle) —
                                // clear both the app-side and PTT tap-side latches.
                                stop_take(app, &state, StopOrigin::MenuBar);
                            } else {
                                start_recording(app, &state);
                            }
                        }
                        _ => {}
                    }
                })
                .build(app)?;
            // Retain the tray so its tooltip can be updated on state changes and
            // so it isn't dropped when this setup scope ends.
            *state.tray.lock() = Some(tray);

            emit_status(app.handle(), &state);

            // Glass island: always-on + rides all Spaces (re-park loop). The
            // dock edge (YV53) is applied BEFORE the first show so the pill
            // never appears bottom-centre and then jumps to the chosen edge.
            float_pill::set_position(float_pill::PillPosition::from_settings(
                &state.settings.lock().pill_position,
            ));
            if state.settings.lock().show_floating_pill {
                if let Err(e) = float_pill::show_float(app.handle()) {
                    log::warn!("float pill: {e}");
                }
                float_pill::start_space_keeper(app.handle().clone());
            } else {
                float_pill::hide_float(app.handle());
            }
            // YV65 drag-to-dock: the capsule is only cursor-interactive while
            // the pointer is on it. Started unconditionally — the pill still
            // appears for a dictation when the always-on setting is off, and the
            // watch is inert while the panel is hidden.
            float_pill::start_hover_watch(app.handle().clone());

            if !permissions::is_accessibility_trusted() {
                log::info!(
                    "Accessibility not trusted (AXIsProcessTrusted = false) — fn⌃ needs it. Enable {} in Privacy → Accessibility",
                    perm_truth::product_name()
                );
                // PERM-E: a hotkey that cannot arm IS a refusal. Tell the
                // watcher so the health row is right before the user tries.
                permissions::poke(permissions::WatchReason::Refusal);
            }

            // The fn⌃ hybrid tap (hold PTT / double-tap hands-free) is no longer
            // started here: yap-p0-hotkeys-no-permission arms it together with
            // the Carbon dictate chord in the registration block below
            // (`hotkeys::arm_at_launch`), and re-attempts it on every
            // permission-watch edge.

            // yap-p0-hotkeys-no-permission: either hotkey half changing re-emits
            // `status`, off the caller's thread (the tap callback is one caller).
            {
                let st = state.clone();
                let h = app.handle().clone();
                hotkeys::on_status_change(move || {
                    let st = st.clone();
                    let h = h.clone();
                    std::thread::spawn(move || emit_status(&h, &st));
                });
            }

            // YV43: watch for the one condition that kills the tap above without
            // any error — another app enabling macOS Secure Input. The watchdog
            // only calls back on an edge, so this is a state write + one status
            // emit per episode, not per poll. There is no Carbon fallback to
            // wire: RegisterEventHotKey cannot express the fn modifier that
            // every Yap binding uses, so the honest status IS the remedy.
            {
                let st = state.clone();
                let h = app.handle().clone();
                secure_input::start(move |snapshot| {
                    *st.secure_input.lock() = snapshot;
                    emit_status(&h, &st);
                });
            }

            // Global ⌃⌘V (Paste Last Transcript, always on) + optional ⌘⇧V
            // dictation toggle (off by default).
            // YV38: no fixed startup delay. `run_on_main_thread` queues this on
            // the event loop, so registration happens the moment the loop is
            // running — which is exactly the event the old 600 ms sleep was
            // guessing at, except the shortcuts are live from launch instead of
            // dead for the first half-second.
            {
                let h = app.handle().clone();
                let st = state.clone();
                let _ = app.handle().run_on_main_thread(move || {
                    // Y0-D: a `--smoke` launch registers NOTHING system-wide, so
                    // two lanes plus Wilson's own install cannot fight over the
                    // same four accelerators.
                    if !smoke::global_hotkeys_allowed() {
                        log::warn!("{}", smoke::HOTKEY_REFUSAL);
                        emit_status(&h, &st);
                        return;
                    }
                    // yap-p0-hotkeys-no-permission DO 1/3: the dictate chord is
                    // registered UNCONDITIONALLY (Carbon: no TCC grant needed)
                    // and the fn tap is ALWAYS attempted, never gated on the
                    // probe. Both report their real result.
                    {
                        let (chord, label) = dictate_toggle_chord(&st.settings.lock());
                        hotkeys::set_active_dictate(chord, label);
                        hotkeys::arm_at_launch(
                            chord,
                            &mut TauriCarbonRegistrar { app: &h },
                            &OsPermissionProbe,
                            &mut FnTapSource { app: &h, state: &st },
                            &mut AppHotkeySink,
                        );
                    }
                    // yap-p0-release-smoke: the run starts only once the
                    // launch-time registration above has reported.
                    if smoke::release_armed() {
                        smoke::spawn_release_smoke(h.clone());
                    }
                    // ⌃⌘V — Paste Last Transcript, registered unconditionally so
                    // the tray item's accelerator fires system-wide (Wispr-parity).
                    let paste_sc = shortcuts::PASTE_LAST.shortcut();
                    match h.global_shortcut().register(paste_sc) {
                        Ok(()) => log::info!("⌃⌘V paste-last registered"),
                        Err(e) => log::warn!("⌃⌘V register failed: {e}"),
                    }
                    // YV95 ⌃⌘M — the meeting toggle (finding #6). Registered
                    // unconditionally so the tray item's accelerator fires
                    // system-wide, exactly like the two above; a failure here is
                    // logged and the tray item still works.
                    // focus-2026-10-19: the chord is the Settings rebind when
                    // there is one, and its register result is the Record
                    // meeting row's status in the hotkey table.
                    {
                        let settings = st.settings.lock().clone();
                        hotkey_table::set_active_cancel(hotkey_table::resolve(
                            hotkey_table::BindingAction::Cancel,
                            settings.cancel_chord.as_ref(),
                        ));
                        hotkey_table::set_active_meeting(hotkey_table::resolve(
                            hotkey_table::BindingAction::RecordMeeting,
                            settings.meeting_chord.as_ref(),
                        ));
                    }
                    let (meeting_chord, meeting_label) = hotkey_table::active_meeting();
                    let meeting_result = h.global_shortcut().register(meeting_chord);
                    match &meeting_result {
                        Ok(()) => log::info!("{meeting_label} meeting-toggle registered"),
                        Err(e) => log::warn!("{meeting_label} register failed: {e}"),
                    }
                    hotkey_table::set_meeting_status(hotkeys::CarbonStatus::from_result(&meeting_result));
                    {
                        // The binding table at launch, once the tap thread has
                        // answered — the first line to read in a hotkey report.
                        let h2 = h.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_secs(3));
                            let st2 = h2.state::<Arc<AppState>>();
                            log::info!("hotkey table at launch:\n{}", hotkey_table::debug_dump(&current_binding_rows(&st2)));
                        });
                    }
                    // YV51 ⌃⌘Z — Undo AI edit (paste the raw take). Also
                    // registered unconditionally so the tray item's accelerator
                    // fires system-wide. ⌃⌘Z is free on macOS (undo is ⌘Z and
                    // redo is ⇧⌘Z), so it does not shadow an app shortcut; if
                    // another app has claimed it, registration fails loudly here
                    // and the tray item still works.
                    let undo_sc = shortcuts::UNDO_AI_EDIT.shortcut();
                    match h.global_shortcut().register(undo_sc) {
                        Ok(()) => log::info!("⌃⌘Z undo-ai-edit registered"),
                        Err(e) => log::warn!("⌃⌘Z register failed: {e}"),
                    }
                    if !st.settings.lock().keep_cmd_shift_v {
                        log::info!("⌘⇧V secondary disabled");
                        emit_status(&h, &st);
                        return;
                    }
                    let sc = shortcuts::DICTATION_TOGGLE_LEGACY.shortcut();
                    match h.global_shortcut().register(sc) {
                        Ok(()) => {
                            log::info!("secondary ⌘⇧V registered");
                            emit_status(&h, &st);
                        }
                        Err(e) => {
                            log::warn!("⌘⇧V register failed: {e}");
                            emit_status(&h, &st);
                        }
                    }
                });
            }

            log::info!(
                "Yap v{} — NSPanel Dictate island (tauri-nspanel)",
                env!("CARGO_PKG_VERSION")
            );
            Ok(())
        })
        .build(context)
        .expect("error while building Wilson Voice")
        .run(|app_handle, event| match event {
            // yap-lifecycle — a Dock click / Finder double-click / `open -a
            // Yap` on a running Yap. The arm only dispatches; the decision is
            // lifecycle::activation_action (which deliberately ignores
            // has_visible_windows: the pill counts as one).
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen {
                has_visible_windows,
                ..
            } => {
                present_main_window(
                    app_handle,
                    lifecycle::ActivationSource::DockReopen {
                        has_visible_windows,
                    },
                );
            }
            // The ONE quit teardown for Cmd-Q, the Dock's Quit and the tray's
            // Quit Yap (YV69 order behind the pill; every step bounded).
            tauri::RunEvent::Exit => {
                // Y1-B: remove the sleep/wake observer before anything it calls
                // into is torn down — first, ahead of the quit teardown.
                if power::unregister_sleep_wake_observer() {
                    log::info!("exit: sleep/wake observer unregistered");
                }
                if let Some(state) = app_handle.try_state::<Arc<AppState>>() {
                    let report = quit_teardown(app_handle, &state);
                    log::info!("{}", report.log_line());
                }
            }
            _ => {}
        });
}

#[cfg(test)]
mod tests {
    use super::{
        apply_settings_migrations, load_settings, preload_engine_at_startup, salvage_settings,
        status_message, update_is_skipped, write_settings_file, AppSettings, StatusInputs,
        CURRENT_SETTINGS_SCHEMA_VERSION, ENGINE_PREPARING_MESSAGE,
    };

    /// UPD-A — a failed check names the endpoints it tried. Without this the
    /// toast reads "Couldn't check for updates: error sending request", which
    /// looks identical whether the user is offline or the manifest URL is dead.
    #[test]
    fn update_failure_names_the_endpoints_it_tried() {
        let endpoints = vec![
            "https://yap-lemon.vercel.app/updates/latest.json".to_string(),
            "https://github.com/wilsonguenther-dev/wilson-voice/releases/latest/download/latest.json"
                .to_string(),
        ];
        let msg = super::describe_update_failure("error sending request", &endpoints);
        assert!(msg.contains("error sending request"), "{msg}");
        assert!(
            msg.contains("yap-lemon.vercel.app/updates/latest.json"),
            "{msg}"
        );
        assert!(msg.contains("github.com"), "{msg}");
        // Order is the order the plugin tries them in.
        assert!(
            msg.find("yap-lemon").unwrap() < msg.find("github.com").unwrap(),
            "{msg}"
        );
    }

    /// UPD-A — a stale primary must not shadow a newer fallback. The plugin's
    /// own loop would return the FIRST parseable manifest (0.9.0 here).
    #[test]
    fn newest_offer_wins_over_endpoint_order() {
        let offers = vec![
            ("0.9.0".to_string(), "vercel"),
            ("0.10.0".to_string(), "github"),
        ];
        assert_eq!(super::pick_newest_offer(offers), Some("github"));
        let tie = vec![
            ("0.9.0".to_string(), "vercel"),
            ("0.9.0".to_string(), "github"),
        ];
        assert_eq!(super::pick_newest_offer(tie), Some("vercel"));
        let junk = vec![
            ("not-a-version".to_string(), "a"),
            ("0.1.0".to_string(), "b"),
        ];
        assert_eq!(super::pick_newest_offer(junk), Some("b"));
    }

    /// UPD-A — a primary that FAILS (non-JSON 200 from an SPA catch-all, a
    /// 5xx) must never switch off the fallback: the plugin's `res.json()?`
    /// would return that error without ever asking the next endpoint.
    #[test]
    fn a_failing_primary_never_hides_the_fallback() {
        use super::EndpointAnswer as A;
        let got = super::fold_endpoint_answers(vec![
            (
                "p".to_string(),
                A::Failed("error decoding response body".into()),
            ),
            ("g".to_string(), A::Offer("0.10.0".to_string(), "github")),
        ]);
        assert_eq!(got, Ok(Some("github")));
        let got = super::fold_endpoint_answers::<&str>(vec![
            (
                "p".to_string(),
                A::Failed("error decoding response body".into()),
            ),
            ("g".to_string(), A::UpToDate),
        ]);
        assert_eq!(got, Ok(None));
        // Nothing published anywhere is the normal pre-release state, not an error.
        let got = super::fold_endpoint_answers::<&str>(vec![
            ("p".to_string(), A::NoManifest),
            ("g".to_string(), A::NoManifest),
        ]);
        assert_eq!(got, Ok(None));
        // Only when nothing usable came back is it an error, naming each failure.
        let got = super::fold_endpoint_answers::<&str>(vec![
            ("p".to_string(), A::NoManifest),
            ("g".to_string(), A::Failed("offline".into())),
        ]);
        assert_eq!(got, Err("g: offline".to_string()));
    }

    #[test]
    fn update_failure_with_no_endpoints_says_so() {
        let msg = super::describe_update_failure("boom", &[]);
        assert!(msg.contains("no update endpoint is configured"), "{msg}");
    }

    /// A fresh per-test directory (tests run in parallel — never share one).
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("yv41-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The defaults as they land on disk, as a mutable JSON object.
    fn default_settings_json() -> serde_json::Value {
        serde_json::to_value(AppSettings::default()).unwrap()
    }

    // --- Context awareness v1 (YV50) privacy ------------------------------
    //
    // The pre-caret context is the most sensitive thing Yap touches: whatever
    // the user already typed in their editor, mail draft or chat. The product
    // rule is that it exists ONLY inside the dictation call stack — these two
    // tests are the enforcement, one over the shipped source, one over a real
    // history row written through the real DB.

    /// No `log::` statement anywhere in the pipeline may interpolate the
    /// context, and the two modules that touch it must not log at all.
    #[test]
    fn cursor_context_is_never_logged_or_persisted() {
        const LIB: &str = include_str!("lib.rs");

        // 1. The binding exists and is only ever consumed by the two pure
        //    formatting calls — never by a log, an event emit, or the DB write.
        let mut uses = 0;
        for (i, line) in LIB.lines().enumerate() {
            if !line.contains("cursor_context") || line.trim_start().starts_with("//") {
                continue;
            }
            uses += 1;
            for banned in [
                "log::",
                "println!",
                "eprintln!",
                ".emit(",
                "insert_transcript",
                "notify(",
            ] {
                assert!(
                    !line.contains(banned),
                    "line {}: pre-caret context reaches `{banned}`: {line}",
                    i + 1
                );
            }
        }
        assert!(uses >= 3, "YV50 context binding missing from the pipeline");

        // 2. Same check against whole (multi-line) log macro invocations, which
        //    a line-by-line scan would miss.
        for block in log_macro_blocks(LIB) {
            assert!(
                !block.contains("cursor_context"),
                "pre-caret context interpolated into a log statement: {block}"
            );
        }

        // 3. The AX reader and the pure formatter never log anything at all.
        for (name, src) in [
            ("focus.rs", include_str!("focus.rs")),
            ("dictation.rs", include_str!("dictation.rs")),
        ] {
            assert!(
                !src.contains("log::"),
                "{name} must stay log-free — it handles the raw context string"
            );
        }
    }

    /// Every `log::` macro invocation in `src`, from `log::` to the `;` that
    /// closes it (semicolons inside the argument list / string literals sit at
    /// paren depth > 0 and are skipped).
    fn log_macro_blocks(src: &str) -> Vec<&str> {
        let mut blocks = Vec::new();
        let bytes = src.as_bytes();
        let mut from = 0;
        while let Some(rel) = src[from..].find("log::") {
            let start = from + rel;
            // Skip mentions that are not invocations: comments and the string
            // literals this very audit uses.
            let line_start = src[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let prefix = src[line_start..start].trim_start();
            if prefix.starts_with("//") || prefix.ends_with('"') {
                from = start + 5;
                continue;
            }
            let mut depth = 0i32;
            let mut end = start;
            for (i, &b) in bytes[start..].iter().enumerate() {
                match b {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    b';' if depth <= 0 => {
                        end = start + i;
                        break;
                    }
                    _ => {}
                }
            }
            if end > start {
                blocks.push(&src[start..end]);
            }
            from = start + 5;
        }
        assert!(
            blocks.len() > 10,
            "log scan found nothing — audit is broken"
        );
        blocks
    }

    /// A dictation whose formatting was steered by the surrounding text stores
    /// the DICTATED words only: no fragment of the context reaches the row, the
    /// raw column, or the JSON the history UI receives.
    #[test]
    fn context_never_reaches_the_history_row() {
        // What the user had already typed (would be a privacy leak if stored).
        let context = "Dear Wilson, thanks for the invoice — the wire went out on";
        let dictated = "Friday afternoon";

        // The context steers the take: mid-sentence ⇒ joining space + lowercase.
        let text = crate::dictation::join_with_context(dictated, Some(context));
        assert_eq!(text, " friday afternoon");

        let dir = temp_dir("yv50-context-privacy");
        let db = crate::db::Database::open(dir.join("wilson_voice.db")).unwrap();
        let entry = db
            .insert_transcript_at(
                text,
                "native".into(),
                1.0,
                2.0,
                100,
                Some("Mail".into()),
                chrono::Utc::now(),
                Some(dictated.into()),
            )
            .unwrap();

        // Check the returned row, the row read back out of SQLite, and the JSON
        // shape the frontend gets — no context word may appear in any of them.
        let listed = db.list_transcripts(10, None).unwrap();
        assert_eq!(listed.len(), 1);
        let surfaces = [
            serde_json::to_string(&entry).unwrap(),
            serde_json::to_string(&listed).unwrap(),
        ];
        for surface in &surfaces {
            for word in context.split_whitespace().filter(|w| w.len() > 4) {
                assert!(
                    !surface.contains(word),
                    "context word {word:?} leaked into history: {surface}"
                );
            }
            // The dictated words themselves are of course still there.
            assert!(surface.contains("friday afternoon"), "{surface}");
        }

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // YV33: "Ready" is a claim about being able to transcribe. With no usable
    // model the status line must say a model is needed — the old rule ("a file
    // exists at the resolved interpreter path") reported Ready on every fresh
    // Mac, where that path was the non-functional Command Line Tools shim.
    #[test]
    fn status_says_model_needed_until_a_model_is_ready() {
        let no_model = status_message(&StatusInputs {
            recording: false,
            hands_free: false,
            busy: false,
            engine_loading: false,
            last_error: None,
            model_ready: false,
            accessibility: true,
            secure_input_blocked: false,
            ptt: "fn⌃",
            session_cap_warned: false,
        });
        assert!(no_model.contains("Model needed"), "{no_model}");
        assert!(!no_model.contains("Ready"), "{no_model}");

        let ready = status_message(&StatusInputs {
            recording: false,
            hands_free: false,
            busy: false,
            engine_loading: false,
            last_error: None,
            model_ready: true,
            accessibility: true,
            secure_input_blocked: false,
            ptt: "fn⌃",
            session_cap_warned: false,
        });
        assert!(ready.starts_with("Ready — hold fn⌃"), "{ready}");

        // Accessibility is a paste-only concern: still Ready, with a nudge.
        let no_ax = status_message(&StatusInputs {
            recording: false,
            hands_free: false,
            busy: false,
            engine_loading: false,
            last_error: None,
            model_ready: true,
            accessibility: false,
            secure_input_blocked: false,
            ptt: "fn⌃",
            session_cap_warned: false,
        });
        assert!(no_ax.starts_with("Ready —"), "{no_ax}");
        assert!(no_ax.contains("Accessibility"), "{no_ax}");

        // Live states and hard errors still outrank the model gate.
        assert!(status_message(&StatusInputs {
            recording: true,
            hands_free: false,
            busy: false,
            engine_loading: false,
            last_error: None,
            model_ready: false,
            accessibility: true,
            secure_input_blocked: false,
            ptt: "fn⌃",
            session_cap_warned: false,
        })
        .contains("Recording"));
        assert!(status_message(&StatusInputs {
            recording: true,
            hands_free: true,
            busy: false,
            engine_loading: false,
            last_error: None,
            model_ready: false,
            accessibility: true,
            secure_input_blocked: false,
            ptt: "fn⌃",
            session_cap_warned: false,
        })
        .contains("Hands-free"));
        assert!(status_message(&StatusInputs {
            recording: false,
            hands_free: false,
            busy: true,
            engine_loading: false,
            last_error: None,
            model_ready: false,
            accessibility: true,
            secure_input_blocked: false,
            ptt: "fn⌃",
            session_cap_warned: false,
        })
        .contains("Transcribing"));
        let err = status_message(&StatusInputs {
            recording: false,
            hands_free: false,
            busy: false,
            engine_loading: false,
            last_error: Some("boom"),
            model_ready: true,
            accessibility: true,
            secure_input_blocked: false,
            ptt: "fn⌃",
            session_cap_warned: false,
        });
        assert_eq!(err, "Error: boom");
    }

    // YV43: while another app holds Secure Input the fn tap is blind, so the
    // idle line must never keep advertising the key — it must name the cause.
    #[test]
    fn status_reports_secure_input_instead_of_advertising_the_dead_hotkey() {
        let blocked = status_message(&StatusInputs {
            recording: false,
            hands_free: false,
            busy: false,
            engine_loading: false,
            last_error: None,
            model_ready: true,
            accessibility: true,
            secure_input_blocked: true,
            ptt: "fn⌃",
            session_cap_warned: false,
        });
        assert_eq!(blocked, crate::secure_input::BLOCKED_MESSAGE);
        assert!(!blocked.contains("Ready"), "{blocked}");

        // It also outranks the model gate — pointing a user at a download does
        // not fix a keyboard they cannot reach the app with.
        let blocked_no_model = status_message(&StatusInputs {
            recording: false,
            hands_free: false,
            busy: false,
            engine_loading: false,
            last_error: None,
            model_ready: false,
            accessibility: true,
            secure_input_blocked: true,
            ptt: "fn⌃",
            session_cap_warned: false,
        });
        assert_eq!(blocked_no_model, crate::secure_input::BLOCKED_MESSAGE);

        // But a live take and a real error still win: those describe what just
        // happened, not an instruction the user cannot follow.
        assert!(status_message(&StatusInputs {
            recording: true,
            hands_free: false,
            busy: false,
            engine_loading: false,
            last_error: None,
            model_ready: true,
            accessibility: true,
            secure_input_blocked: true,
            ptt: "fn⌃",
            session_cap_warned: false,
        })
        .contains("Recording"));
        assert_eq!(
            status_message(&StatusInputs {
                recording: false,
                hands_free: false,
                busy: false,
                engine_loading: false,
                last_error: Some("boom"),
                model_ready: true,
                accessibility: true,
                secure_input_blocked: true,
                ptt: "fn⌃",
                session_cap_warned: false,
            }),
            "Error: boom"
        );

        // Cleared → straight back to the normal Ready line.
        assert!(status_message(&StatusInputs {
            recording: false,
            hands_free: false,
            busy: false,
            engine_loading: false,
            last_error: None,
            model_ready: true,
            accessibility: true,
            secure_input_blocked: false,
            ptt: "fn⌃",
            session_cap_warned: false,
        })
        .starts_with("Ready — hold fn⌃"));
    }

    // --- YV80 lazy ASR load ------------------------------------------------

    /// Stands in for a loaded GGUF session — the startup decision is about
    /// WHETHER an engine is built, never about what it decodes.
    struct StubEngine;
    impl crate::transcription::Transcriber for StubEngine {
        fn transcribe(
            &mut self,
            _samples: &[f32],
            _language: Option<&str>,
            _bias_prompt: Option<&str>,
        ) -> Result<String, String> {
            Ok("stub".into())
        }
    }

    /// A manager whose loader counts the engines it builds, plus a real (empty)
    /// model file on disk so the manager's "is it downloaded?" gate passes —
    /// the whole launch path exercised without a 700 MB GGUF anywhere near it.
    fn stub_manager(
        tag: &str,
        loads: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) -> (
        crate::transcription::TranscriptionManager,
        (String, std::path::PathBuf),
    ) {
        let dir = temp_dir(tag);
        let path = dir.join("stub.gguf");
        std::fs::write(&path, b"stub gguf").unwrap();
        let manager = crate::transcription::TranscriptionManager::with_loader(
            std::sync::Arc::new(move |_p: &std::path::Path| {
                loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(Box::new(StubEngine) as Box<dyn crate::transcription::Transcriber>)
            }),
            std::time::Duration::from_secs(600),
            std::time::Duration::from_secs(600),
            std::time::Duration::from_secs(5),
        );
        (manager, ("stub/model".to_string(), path))
    }

    /// YV80: Yap used to load the GGUF at launch and sit on ~930 MB of resident
    /// memory it might never use. The default is now lazy — launch must leave
    /// the manager UNLOADED, and must not have called the loader at all.
    #[test]
    fn engine_not_loaded_at_startup_by_default() {
        // The setting that decides it, at its shipped default.
        assert!(
            !AppSettings::default().preload_model,
            "the lazy path is the default; preload is opt-in"
        );

        let loads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (manager, model) = stub_manager("yv80-lazy", loads.clone());
        let attempted =
            preload_engine_at_startup(&manager, AppSettings::default().preload_model, Some(model));

        assert!(
            !attempted,
            "launch must not attempt a load on the lazy path"
        );
        assert!(!manager.is_loaded(), "nothing may be resident after launch");
        assert!(!manager.is_loading());
        assert_eq!(
            loads.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the loader must never run at startup by default"
        );
    }

    /// The escape hatch: `preload_model` puts YV38's eager launch back, model
    /// resident before anyone presses a key.
    #[test]
    fn preload_setting_restores_eager_path() {
        let loads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (manager, model) = stub_manager("yv80-eager", loads.clone());
        let settings = AppSettings {
            preload_model: true,
            ..AppSettings::default()
        };

        let attempted =
            preload_engine_at_startup(&manager, settings.preload_model, Some(model.clone()));

        assert!(attempted, "the eager path must attempt the load");
        assert!(manager.is_loaded(), "the engine must be resident at launch");
        assert_eq!(manager.loaded_model().as_deref(), Some(model.0.as_str()));
        assert_eq!(loads.load(std::sync::atomic::Ordering::SeqCst), 1);

        // And the setting round-trips through the store like any other.
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(json["preloadModel"], serde_json::json!(true));
        // Settings written before the field existed read as the lazy default
        // rather than failing the load.
        let legacy: AppSettings = serde_json::from_str(r#"{"language":"en"}"#).unwrap();
        assert!(!legacy.preload_model);
    }

    /// YV80: the first take waits on the model, and the status line has to SAY
    /// which multi-second thing is happening — "Transcribing" while nothing has
    /// been decoded yet is the dead air this replaces.
    #[test]
    fn busy_says_preparing_while_the_engine_is_still_loading() {
        let loading = status_message(&StatusInputs {
            recording: false,
            hands_free: false,
            busy: true,
            engine_loading: true,
            last_error: None,
            model_ready: true,
            accessibility: true,
            secure_input_blocked: false,
            ptt: "fn⌃",
            session_cap_warned: false,
        });
        assert_eq!(loading, ENGINE_PREPARING_MESSAGE);

        // Once it is resident the same busy take reads as a decode again.
        let decoding = status_message(&StatusInputs {
            recording: false,
            hands_free: false,
            busy: true,
            engine_loading: false,
            last_error: None,
            model_ready: true,
            accessibility: true,
            secure_input_blocked: false,
            ptt: "fn⌃",
            session_cap_warned: false,
        });
        assert!(decoding.contains("Transcribing"), "{decoding}");

        // A load that overlaps the hold is invisible: the user is talking, and
        // the recording line is still the true one.
        let recording = status_message(&StatusInputs {
            recording: true,
            hands_free: false,
            busy: false,
            engine_loading: true,
            last_error: None,
            model_ready: true,
            accessibility: true,
            secure_input_blocked: false,
            ptt: "fn⌃",
            session_cap_warned: false,
        });
        assert!(recording.contains("Recording"), "{recording}");
    }

    // YV9: onboarding gate must default to false so a fresh install shows the
    // first-run flow, and older settings.json (written before the field existed)
    // must deserialize with onboarded=false via serde default — never panic.
    #[test]
    fn onboarded_defaults_false_and_is_backward_compatible() {
        assert!(!AppSettings::default().onboarded);
        assert!(AppSettings::default().calibration_sample.is_none());

        // Legacy settings JSON with no `onboarded` / `calibrationSample` keys.
        // YV34: it also still carries the retired sidecar keys (`model`,
        // `speedProfile`), which must be ignored rather than fail the parse —
        // every existing install has them on disk.
        let legacy = r#"{
            "model": "whisper-large-v3-turbo",
            "speedProfile": "balanced",
            "language": "en",
            "autoPaste": true,
            "hotkeyLabel": "fn⌃",
            "showFloatingPill": true
        }"#;
        let parsed: AppSettings = serde_json::from_str(legacy).expect("legacy parse");
        assert!(!parsed.onboarded);
        assert!(parsed.calibration_sample.is_none());
        // YV10 / Y4-A: cleanup_level defaults to "medium" for fresh installs and
        // for legacy JSON written before the field existed. It was "light" until
        // Y4-A, which stopped one stage short of `CleanupLevel::runs_format()` —
        // so a legacy store with no `cleanupLevel` key deserialised into a
        // configuration that never formatted anything. The serde default is the
        // only thing standing behind that store, which is why it is asserted on
        // both sides here.
        assert_eq!(AppSettings::default().cleanup_level, "medium");
        assert_eq!(parsed.cleanup_level, "medium");
        // YV12: denoise defaults ON for fresh installs and legacy JSON (serde default).
        assert!(AppSettings::default().denoise);
        assert!(parsed.denoise);
        // YV27: companion_tone defaults to "friendly" for fresh installs and
        // legacy JSON (written before the field existed) via serde default.
        assert_eq!(AppSettings::default().companion_tone, "friendly");
        assert_eq!(parsed.companion_tone, "friendly");

        // A finished onboarding round-trips (camelCase key on the wire).
        let done = AppSettings {
            onboarded: true,
            calibration_sample: Some("the quick brown fox".into()),
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&done).expect("serialize");
        assert!(json.contains("\"onboarded\":true"));
        assert!(json.contains("\"calibrationSample\":\"the quick brown fox\""));
        let back: AppSettings = serde_json::from_str(&json).expect("round-trip");
        assert!(back.onboarded);
        assert_eq!(
            back.calibration_sample.as_deref(),
            Some("the quick brown fox")
        );

        // YV27: a chosen companion tone round-trips on the camelCase wire key.
        let toned = AppSettings {
            companion_tone: "rude".into(),
            ..AppSettings::default()
        };
        let tjson = serde_json::to_string(&toned).expect("serialize tone");
        assert!(tjson.contains("\"companionTone\":\"rude\""));
        let tback: AppSettings = serde_json::from_str(&tjson).expect("tone round-trip");
        assert_eq!(tback.companion_tone, "rude");

        // YV31: nativeModel defaults to the catalog's recommended model for
        // fresh installs AND for legacy JSON written before the field existed,
        // and round-trips on its camelCase wire key.
        let recommended = crate::models::recommended_model().id.clone();
        assert_eq!(AppSettings::default().native_model, recommended);
        assert_eq!(parsed.native_model, recommended);
        let native = AppSettings {
            native_model: "handy-computer/whisper-tiny-gguf".into(),
            ..AppSettings::default()
        };
        let njson = serde_json::to_string(&native).expect("serialize native model");
        assert!(njson.contains("\"nativeModel\":\"handy-computer/whisper-tiny-gguf\""));
        let nback: AppSettings = serde_json::from_str(&njson).expect("native round-trip");
        assert_eq!(nback.native_model, "handy-computer/whisper-tiny-gguf");
    }

    // YV42: launch-at-login is OFF unless the user asked for it — a fresh
    // install and a store written before the field existed must both read
    // false, so no LaunchAgent is ever installed behind the user's back. When
    // it IS on it round-trips (and survives salvage) on its wire key.
    #[test]
    fn autostart_defaults_off_and_round_trips() {
        assert!(!AppSettings::default().autostart);

        let legacy = r#"{
            "language": "en",
            "autoPaste": true,
            "hotkeyLabel": "fn⌃",
            "showFloatingPill": true
        }"#;
        let parsed: AppSettings = serde_json::from_str(legacy).expect("legacy parse");
        assert!(
            !parsed.autostart,
            "a pre-YV42 store must not enable autostart"
        );

        let on = AppSettings {
            autostart: true,
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&on).expect("serialize autostart");
        assert!(json.contains("\"autostart\":true"));
        let back: AppSettings = serde_json::from_str(&json).expect("autostart round-trip");
        assert!(back.autostart);

        // A different field being corrupt must not silently turn it back off.
        let mut stored = serde_json::to_value(&on).expect("autostart settings to value");
        stored
            .as_object_mut()
            .unwrap()
            .insert("denoise".into(), serde_json::json!("sometimes"));
        assert!(salvage_settings(&stored).autostart);
    }

    // YV44: the update check is on by default (it only ever notifies), stays a
    // user-owned switch, and survives a store written before the field existed.
    #[test]
    fn check_updates_defaults_on_and_round_trips() {
        assert!(AppSettings::default().check_updates);
        assert!(AppSettings::default().skipped_update_version.is_none());

        let legacy = r#"{
            "language": "en",
            "autoPaste": true,
            "hotkeyLabel": "fn⌃",
            "showFloatingPill": true
        }"#;
        let parsed: AppSettings = serde_json::from_str(legacy).expect("legacy parse");
        assert!(
            parsed.check_updates,
            "a pre-YV44 store must still get update notifications"
        );

        let off = AppSettings {
            check_updates: false,
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&off).expect("serialize checkUpdates");
        assert!(json.contains("\"checkUpdates\":false"));
        let back: AppSettings = serde_json::from_str(&json).expect("checkUpdates round-trip");
        assert!(!back.check_updates, "turning updates off must persist");

        // A different field being corrupt must not silently re-enable it.
        let mut stored = serde_json::to_value(&off).expect("update settings to value");
        stored
            .as_object_mut()
            .unwrap()
            .insert("denoise".into(), serde_json::json!("sometimes"));
        assert!(!salvage_settings(&stored).check_updates);
    }

    // YV44: "Skip this version" suppresses exactly the version the user
    // dismissed — the next release is still offered.
    #[test]
    fn skipped_version_suppresses_only_that_version() {
        assert!(update_is_skipped("0.7.0", Some("0.7.0")));
        assert!(!update_is_skipped("0.7.1", Some("0.7.0")));
        assert!(!update_is_skipped("0.7.0", None));

        let skipped = AppSettings {
            skipped_update_version: Some("0.7.0".into()),
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&skipped).expect("serialize skippedUpdateVersion");
        assert!(json.contains("\"skippedUpdateVersion\":\"0.7.0\""));
        let back: AppSettings = serde_json::from_str(&json).expect("skip round-trip");
        assert_eq!(back.skipped_update_version.as_deref(), Some("0.7.0"));
    }

    // YV41: ONE unparseable field used to throw away the entire settings.json
    // (hotkey, model, tone, the onboarded flag). Salvage keeps every field that
    // is individually valid and defaults only the broken ones.
    #[test]
    fn salvage_keeps_every_valid_field_when_one_is_corrupt() {
        let mut stored = default_settings_json();
        let map = stored.as_object_mut().unwrap();
        map.insert("onboarded".into(), serde_json::json!(true));
        map.insert("pttBinding".into(), serde_json::json!("fn"));
        map.insert(
            "nativeModel".into(),
            serde_json::json!("handy-computer/whisper-tiny-gguf"),
        );
        map.insert(
            "calibrationSample".into(),
            serde_json::json!("the quick brown fox"),
        );
        // A value with the wrong type, and one a newer build could have written.
        map.insert("denoise".into(), serde_json::json!("sometimes"));
        map.insert("companionTone".into(), serde_json::json!(42));

        // Precondition: this is exactly the whole-store parse failure that used
        // to reset everything to defaults.
        assert!(serde_json::from_value::<AppSettings>(stored.clone()).is_err());

        let salvaged = salvage_settings(&stored);
        assert!(salvaged.onboarded, "the onboarded flag must survive");
        assert_eq!(salvaged.ptt_binding, "fn");
        assert_eq!(salvaged.native_model, "handy-computer/whisper-tiny-gguf");
        assert_eq!(
            salvaged.calibration_sample.as_deref(),
            Some("the quick brown fox")
        );
        // Only the two broken fields fall back.
        assert!(salvaged.denoise);
        assert_eq!(salvaged.companion_tone, "friendly");
    }

    // A key this build has never heard of (written by a newer version) is
    // ignored, not treated as corruption.
    #[test]
    fn salvage_tolerates_unknown_keys() {
        let mut stored = default_settings_json();
        let map = stored.as_object_mut().unwrap();
        map.insert(
            "fieldFromTheFuture".into(),
            serde_json::json!({ "nested": true }),
        );
        map.insert("language".into(), serde_json::json!("fr"));
        map.insert("cleanupLevel".into(), serde_json::json!(7));

        let salvaged = salvage_settings(&stored);
        assert_eq!(salvaged.language, "fr");
        // Y4-A: a field of the wrong type falls back to the SHIPPED default,
        // which is now "medium". Salvage runs on the struct, below the
        // migration, so this is the serde default and nothing else.
        assert_eq!(salvaged.cleanup_level, "medium");
    }

    // End-to-end through the file: one bad field, everything else survives, and
    // the good file is NOT quarantined.
    #[test]
    fn load_settings_salvages_one_bad_field_instead_of_resetting_everything() {
        let dir = temp_dir("salvage");
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{
                "schemaVersion": 1,
                "language": "en",
                "autoPaste": true,
                "hotkeyLabel": "fn",
                "showFloatingPill": true,
                "pttBinding": "fn",
                "onboarded": true,
                "nativeModel": "handy-computer/whisper-tiny-gguf",
                "cleanupLevel": ["high"]
            }"#,
        )
        .unwrap();

        let loaded = load_settings(&path);
        assert!(loaded.onboarded);
        assert_eq!(loaded.ptt_binding, "fn");
        assert_eq!(loaded.native_model, "handy-computer/whisper-tiny-gguf");
        // Y4-A: the bad field falls back to the shipped default, now "medium".
        // The v1 → v2 migration also runs on this file (it is stored at v1), and
        // it deliberately does NOT fire here: its arm rewrites the literal
        // "light", and salvage already produced "medium". Two mechanisms, one
        // value, no double-handling.
        assert_eq!(
            loaded.cleanup_level, "medium",
            "only the bad field defaults"
        );
        assert!(path.exists(), "a salvageable file is not quarantined");
        assert!(!dir.join("settings.json.bak").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // A wholly corrupt file (truncated by a crash mid-write, or not even an
    // object) falls back to defaults, but is KEPT as .bak — never deleted.
    #[test]
    fn load_settings_quarantines_a_corrupt_file_as_bak() {
        let dir = temp_dir("corrupt");
        let path = dir.join("settings.json");
        let bak = dir.join("settings.json.bak");

        for garbage in [r#"{"language": "en", "autoPas"#, r#""not an object""#] {
            let _ = std::fs::remove_file(&bak);
            std::fs::write(&path, garbage).unwrap();

            let loaded = load_settings(&path);
            assert_eq!(
                serde_json::to_value(&loaded).unwrap(),
                default_settings_json(),
                "corrupt store falls back to defaults"
            );
            assert!(!path.exists(), "the corrupt file is moved aside");
            assert_eq!(
                std::fs::read_to_string(&bak).unwrap(),
                garbage,
                "the corrupt bytes are kept verbatim for recovery"
            );
        }

        // Missing file (fresh install) is not an error and leaves no .bak.
        let _ = std::fs::remove_file(&bak);
        assert!(!load_settings(&path).onboarded);
        assert!(!bak.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // The write is atomic (temp file + rename in the same dir, no leftovers) and
    // merges unknown keys forward, so a save from a build that does not know a
    // field cannot delete it.
    #[test]
    fn settings_write_is_atomic_and_merges_unknown_keys_forward() {
        let dir = temp_dir("write");
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{"language": "de", "fieldFromTheFuture": {"nested": true}}"#,
        )
        .unwrap();

        let settings = AppSettings {
            language: "en".into(),
            onboarded: true,
            ..AppSettings::default()
        };
        write_settings_file(&path, &settings).expect("atomic write");

        let on_disk: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk["language"], serde_json::json!("en"));
        assert_eq!(on_disk["onboarded"], serde_json::json!(true));
        assert_eq!(
            on_disk["fieldFromTheFuture"],
            serde_json::json!({ "nested": true }),
            "a key this build does not know must survive the save"
        );
        assert!(
            !dir.join("settings.json.tmp").exists(),
            "the temp file is renamed into place, not left behind"
        );
        assert_eq!(load_settings(&path).language, "en");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // YV41 schema versioning: a store written before the field existed is
    // version 0, gets migrated + rewritten ONCE, and is left alone afterwards.
    #[test]
    fn schema_version_migration_runs_once_then_stops() {
        let dir = temp_dir("migrate");
        let path = dir.join("settings.json");
        // A pre-YV41 store naming the retired Python-sidecar model id.
        std::fs::write(
            &path,
            r#"{
                "language": "en",
                "autoPaste": true,
                "hotkeyLabel": "fn⌃",
                "showFloatingPill": true,
                "onboarded": true,
                "nativeModel": "whisper-large-v3-turbo"
            }"#,
        )
        .unwrap();

        let migrated = load_settings(&path);
        assert_eq!(migrated.schema_version, CURRENT_SETTINGS_SCHEMA_VERSION);
        assert!(migrated.onboarded, "migration keeps the user's config");
        assert_eq!(
            migrated.native_model,
            super::default_native_model(),
            "a model the bundled catalog does not have is reset once"
        );

        // The rewrite landed, so the next launch is a no-op.
        let stored: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            stored["schemaVersion"],
            serde_json::json!(CURRENT_SETTINGS_SCHEMA_VERSION)
        );
        let mut again: AppSettings = serde_json::from_value(stored.clone()).unwrap();
        assert!(
            !apply_settings_migrations(&mut again, &stored),
            "a current-schema store must not be rewritten on every read"
        );

        // A version-0 store whose model IS in the catalog keeps its selection —
        // the migration only touches what it cannot resolve.
        let keeps = serde_json::json!({
            "language": "en",
            "nativeModel": "handy-computer/whisper-tiny-gguf",
        });
        let mut settings: AppSettings = serde_json::from_value(keeps.clone()).unwrap();
        assert!(apply_settings_migrations(&mut settings, &keeps));
        assert_eq!(settings.native_model, "handy-computer/whisper-tiny-gguf");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // YV46: the legacy JSON import is armed exactly once. A store written
    // before the flag existed reads as "not yet migrated"; once the flag is
    // recorded it survives every later load and save, so no launch after the
    // first ever looks for history/transcripts.json again.
    #[test]
    fn legacy_json_migration_is_armed_once_then_recorded() {
        let dir = temp_dir("legacy-migrate");
        let path = dir.join("settings.json");
        std::fs::write(&path, r#"{"schemaVersion": 1, "onboarded": true}"#).unwrap();

        let mut settings = load_settings(&path);
        assert!(
            !settings.legacy_json_migrated,
            "a store from before the flag still gets its one import attempt"
        );

        settings.legacy_json_migrated = true;
        write_settings_file(&path, &settings).expect("record the flag");
        assert!(
            load_settings(&path).legacy_json_migrated,
            "the flag is durable, so the next launch skips the import"
        );

        // A later save (which carries the flag through from the live settings)
        // must not re-arm it.
        let reloaded = load_settings(&path);
        write_settings_file(&path, &reloaded).expect("ordinary save");
        assert!(load_settings(&path).legacy_json_migrated);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// YV63 — the crash half of the promise. A take the app died in the middle
    /// of leaves an in-progress marker and its spilled audio in the recovery
    /// dir; the startup scan must turn that into a real WAV and the SAME
    /// failed-dictation row (YV52) a failed transcription gets, so the words are
    /// one Retry away instead of gone.
    #[test]
    fn orphaned_marker_becomes_failed_dictation_row_on_startup() {
        let dir = temp_dir("yv63-crash-recovery");
        let db = crate::db::Database::open(dir.join("wilson_voice.db")).unwrap();
        let recovery = dir.join("recovery");

        // A take in flight: half a second already spilled, then the app dies.
        let journal = crate::record::CaptureJournal::start(&recovery).expect("journal opens");
        let spoken: Vec<f32> = (0..8_000)
            .map(|i| 0.4 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 16_000.0).sin())
            .collect();
        journal.append(&spoken);
        journal.abandon();

        assert_eq!(
            super::recover_crashed_takes(&db, &recovery),
            1,
            "the orphaned take must be recovered at startup"
        );

        let rows = db.list_failed_dictations().unwrap();
        assert_eq!(rows.len(), 1, "the recovered take gets a retryable row");
        assert_eq!(rows[0].error, super::CRASH_RECOVERY_ERROR);
        assert!((rows[0].speech_seconds - 0.5).abs() < 0.01);
        // The row points at a real, parseable WAV inside the recovery dir — the
        // same lifecycle (Retry, discard, 7-day purge) the YV52 clips live in.
        let wav = std::path::PathBuf::from(&rows[0].wav_path);
        assert!(
            wav.starts_with(&recovery),
            "recovered audio stays in {recovery:?}"
        );
        let samples = crate::record::read_wav_16k_mono(&wav).expect("recovered wav parses");
        assert_eq!(samples.len(), spoken.len());

        // Idempotent: the next launch must not re-recover the same take.
        assert_eq!(super::recover_crashed_takes(&db, &recovery), 0);
        assert_eq!(db.list_failed_dictations().unwrap().len(), 1);

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- YV67: no take that already wrote a WAV may vanish -----------------
    //
    // `failed_dictations` sat at 0 rows against 469 transcripts because YV52
    // recovery was wired ONLY to the `Err` arm. A gate rejection took the
    // `Ok(Soft)` arm, never took `pending`, and `ClipWav::drop` unlinked the
    // only copy of the audio — no transcript, no recovery row, nothing.

    /// What a finished take owes the audio it already wrote to disk.
    #[derive(Debug, PartialEq)]
    enum AudioDuty {
        /// It became a transcript row; the history wav is disposable.
        Transcribed,
        /// Nothing worth keeping was captured (fumbled tap, command edit).
        Discardable,
        /// A real wav exists and produced NO transcript — it must survive as a
        /// retryable row, or the user's words are gone for good.
        MustPersist,
    }

    /// Exhaustive on purpose (no `_` arm): a new `TakeOutcome` variant fails to
    /// COMPILE here until someone decides what happens to the wav it may have
    /// written. That compile error is the whole point of this helper.
    fn audio_duty(outcome: &super::TakeOutcome) -> AudioDuty {
        match outcome {
            super::TakeOutcome::Dictated(..) => AudioDuty::Transcribed,
            super::TakeOutcome::Soft(_) => AudioDuty::Discardable,
            super::TakeOutcome::Rejected { .. } => AudioDuty::MustPersist,
            // PERM-D — a silent take keeps its audio for the same reason a
            // rejected one does: the clip is the evidence, and the gate can be
            // wrong.
            super::TakeOutcome::SilentCapture { .. } => AudioDuty::MustPersist,
            // Y3-D — a cancelled take keeps its audio for the strongest reason
            // of the four: the user may have cancelled a fifteen-minute take by
            // accident, and there is no second copy anywhere.
            super::TakeOutcome::Cancelled => AudioDuty::MustPersist,
        }
    }

    /// A real 16 kHz WAV on disk plus the guard that owns it — the same shape
    /// the pipeline hands to `keep_failed_take`.
    fn take_wav(dir: &std::path::Path, name: &str) -> (std::path::PathBuf, crate::record::ClipWav) {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(format!("{name}.wav"));
        let spoken: Vec<f32> = (0..16_000)
            .map(|i| 0.4 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 16_000.0).sin())
            .collect();
        crate::record::write_wav_i16(&path, 16_000, &spoken).unwrap();
        (path.clone(), crate::record::ClipWav::adopt_existing(path))
    }

    #[test]
    fn every_take_outcome_that_wrote_a_wav_persists_something() {
        assert_eq!(
            audio_duty(&super::TakeOutcome::Soft(super::NO_SPEECH_MESSAGE.into())),
            AudioDuty::Discardable
        );
        let rejected = super::TakeOutcome::Rejected {
            message: super::NO_SPEECH_MESSAGE.into(),
            reason: super::GATE_REPETITION_REASON,
        };
        assert_eq!(audio_duty(&rejected), AudioDuty::MustPersist);

        // Drive the duty the gate arm now honours, against a real DB + wav.
        let super::TakeOutcome::Rejected { reason, .. } = rejected else {
            unreachable!("constructed as Rejected")
        };
        let dir = temp_dir("yv67-rejected-persists");
        let db = crate::db::Database::open(dir.join("wilson_voice.db")).unwrap();
        let recovery = dir.join("recovery");
        let (_original, clip) = take_wav(&dir.join("recordings"), "rejected");

        let row = super::keep_failed_take(&db, &recovery, clip, 1.0, Some("Notes".into()), reason)
            .expect("a rejected take must be recoverable");
        assert_eq!(row.error, super::GATE_REPETITION_REASON);

        let rows = db.list_failed_dictations().unwrap();
        assert_eq!(
            rows.len(),
            1,
            "the rejected take gets exactly one retry row"
        );
        let wav = std::path::PathBuf::from(&rows[0].wav_path);
        assert!(
            wav.starts_with(&recovery),
            "kept audio lives in {recovery:?}"
        );
        assert!(
            wav.exists(),
            "the retry row must point at audio that is there"
        );
        // …and it is NOT history: a gate rejection never becomes a transcript.
        assert!(
            db.list_transcripts(10, None).unwrap().is_empty(),
            "a rejected take must not enter history"
        );

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejected_take_keeps_wav_and_row() {
        let dir = temp_dir("yv67-keeps-wav");
        let db = crate::db::Database::open(dir.join("wilson_voice.db")).unwrap();
        let recovery = dir.join("recovery");
        let recordings = dir.join("recordings");
        let (original, clip) = take_wav(&recordings, "kept");

        // `keep_failed_take` takes the guard BY VALUE, so the `ClipWav` is
        // dropped inside this call. Everything asserted below is therefore
        // asserted AFTER the drop that used to delete the take.
        let row = super::keep_failed_take(
            &db,
            &recovery,
            clip,
            2.5,
            None,
            super::GATE_REPETITION_REASON,
        )
        .expect("the clip is preserved");

        let kept = std::path::PathBuf::from(&row.wav_path);
        assert!(
            kept.exists(),
            "dropping the guard must NOT unlink a kept recovery wav"
        );
        assert!(!original.exists(), "the clip was moved, not copied");
        assert_eq!(
            crate::record::read_wav_16k_mono(&kept).unwrap().len(),
            16_000,
            "the preserved audio is the whole take, and Retry can decode it"
        );
        assert_eq!(db.list_failed_dictations().unwrap()[0].id, row.id);

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- YV69: the exit teardown order ------------------------------------
    //
    // Quitting with a model resident used to SIGABRT: ggml's C++ static
    // destructor freed the Metal device from inside `exit()`, after ObjC had
    // already started unwinding. The fix is purely an ordering one, so the
    // test IS the order.

    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    /// Stand-in for a loaded GGUF session — never actually run here; it only
    /// has to occupy the manager's engine slot so `is_loaded()` is true.
    struct ExitStubEngine;

    impl crate::transcription::Transcriber for ExitStubEngine {
        fn transcribe(
            &mut self,
            _samples_16k_mono: &[f32],
            _language: Option<&str>,
            _bias_prompt: Option<&str>,
        ) -> Result<String, String> {
            Ok(String::new())
        }
    }

    /// YV70 — a decode that takes REAL time, so a take can be caught IN FLIGHT
    /// (engine leased out, slot empty) the way a Cmd-Q mid-transcription finds
    /// it. `cancellable` mirrors transcribe-cpp's cooperative cancel: the run
    /// polls the flag between slices and returns `Aborted` once it is set. With
    /// it off nothing can stop the decode — the stuck-lease case.
    struct SlowStubEngine {
        decode: Duration,
        cancel: Arc<AtomicBool>,
        cancellable: bool,
    }

    impl crate::transcription::Transcriber for SlowStubEngine {
        fn transcribe(
            &mut self,
            _samples_16k_mono: &[f32],
            _language: Option<&str>,
            _bias_prompt: Option<&str>,
        ) -> Result<String, String> {
            let deadline = Instant::now() + self.decode;
            while Instant::now() < deadline {
                if self.cancel.load(Ordering::SeqCst) {
                    return Err("operation aborted".into());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok("in-flight take".into())
        }

        fn cancel_handle(&self) -> Option<crate::transcription::CancelHandle> {
            if !self.cancellable {
                return None;
            }
            let flag = self.cancel.clone();
            Some(Arc::new(move || flag.store(true, Ordering::SeqCst)))
        }
    }

    /// A manager over one stub engine. Long idle timeout on purpose: the idle
    /// watcher must NOT be the thing that unloads in these tests — the teardown
    /// has to do it.
    fn exit_manager(
        make: impl Fn() -> Box<dyn crate::transcription::Transcriber> + Send + Sync + 'static,
    ) -> crate::transcription::TranscriptionManager {
        crate::transcription::TranscriptionManager::with_loader(
            Arc::new(move |_p: &std::path::Path| Ok(make())),
            Duration::from_secs(15 * 60),
            Duration::from_secs(60),
            Duration::from_secs(120),
        )
    }

    /// The app state the teardown runs against, with a real DB in `dir`.
    fn exit_state(
        dir: &std::path::Path,
        transcription: crate::transcription::TranscriptionManager,
    ) -> Arc<super::AppState> {
        let db = Arc::new(crate::db::Database::open(dir.join("wilson_voice.db")).unwrap());
        let license = Arc::new(crate::license::LicenseManager::new(
            dir,
            db.clone() as Arc<dyn crate::license::LicenseStore>,
        ));
        Arc::new(super::AppState {
            settings: super::PLMutex::new(AppSettings::default()),
            recording: super::PLMutex::new(false),
            busy: super::PLMutex::new(false),
            hands_free: super::PLMutex::new(false),
            session_cap_warned: super::PLMutex::new(false),
            command_selection: super::PLMutex::new(None),
            recorder: super::PLMutex::new(None),
            saved_audio: super::PLMutex::new(None),
            db,
            last_error: super::PLMutex::new(None),
            hotkey_arbiter: super::PLMutex::new(super::hotkeys::HotkeyArbiter::new()),
            tray: super::PLMutex::new(None),
            tray_dictation: super::PLMutex::new(None),
            tray_hands_free: super::PLMutex::new(None),
            tray_paste_raw: super::PLMutex::new(None),
            tray_meeting: super::PLMutex::new(None),
            tray_meeting_kind: super::PLMutex::new(None),
            tray_license: super::PLMutex::new(None),
            tray_upgrade: super::PLMutex::new(None),
            tray_menu: super::PLMutex::new(None),
            tray_upgrade_shown: super::PLMutex::new(false),
            tray_urgent: super::PLMutex::new(false),
            meeting: std::sync::OnceLock::new(),
            undo_available: super::PLMutex::new(false),
            secure_input: super::PLMutex::new(crate::secure_input::SecureInputStatus::default()),
            vad: super::PLMutex::new(None),
            paste_generation: std::sync::atomic::AtomicU64::new(0),
            take_cancelled: std::sync::atomic::AtomicBool::new(false),
            license,
            transcription,
            support_bundle: super::PLMutex::new(None),
        })
    }

    /// A real (empty) model file so the manager's "is it downloaded?" gate passes.
    fn stub_model_in(dir: &std::path::Path) -> std::path::PathBuf {
        let model = dir.join("stub.gguf");
        std::fs::write(&model, b"stub gguf").unwrap();
        model
    }

    /// Spin (bounded) until `cond` holds. Panics rather than hanging the suite.
    fn wait_until(what: &str, cond: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !cond() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn teardown_for_exit_unloads_asr_first() {
        let dir = temp_dir("yv69-teardown");
        let model = stub_model_in(&dir);

        let manager = exit_manager(|| Box::new(ExitStubEngine));
        manager.load("stub", &model).expect("stub model loads");
        assert!(manager.is_loaded(), "the take left a model resident");

        let state = exit_state(&dir, manager.clone());

        let steps = super::teardown_for_exit(&state);

        assert_eq!(
            steps,
            vec!["asr_unload", "restore_output", "db_checkpoint"],
            "the Metal device must be released BEFORE the process reaches exit()"
        );
        assert!(
            !manager.is_loaded(),
            "quitting must leave no ggml Metal device for the static destructor"
        );

        drop(state);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- YV70: quitting DURING a transcription ----------------------------
    //
    // YV69's unload only frees what is in the slot. During a take the engine is
    // leased OUT to the transcription thread, so the slot is empty, the unload
    // freed nothing, and the Metal device rode into `exit()` — the same SIGABRT
    // YV69 was supposed to end. The teardown now drains the lease first.

    /// The common path: an in-flight decode is cancelled, hands the engine back,
    /// and only THEN is the engine freed. Nothing is resident when this returns.
    #[test]
    fn teardown_waits_for_inflight_lease_then_unloads() {
        let dir = temp_dir("yv70-drain");
        let model = stub_model_in(&dir);

        // A 30 s decode: if the drain did not cancel + wait, this test would
        // either sail past a still-leased engine or block for half a minute.
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let manager = exit_manager(move || {
            Box::new(SlowStubEngine {
                decode: Duration::from_secs(30),
                cancel: flag.clone(),
                cancellable: true,
            })
        });
        manager.load("stub", &model).expect("stub model loads");
        let state = exit_state(&dir, manager.clone());

        let worker = {
            let m = manager.clone();
            std::thread::spawn(move || m.transcribe(vec![0.1; 16], None, None))
        };
        wait_until("the take to be in flight", || manager.status().transcribing);
        // This is the YV69 blind spot: resident, but NOT in the slot.
        assert!(manager.is_loaded(), "the engine is resident during a take");
        assert!(
            manager.loaded_model().is_none(),
            "the engine is leased out — an unload here frees nothing"
        );

        let started = Instant::now();
        let steps =
            super::teardown_for_exit_with(&state, Duration::from_secs(5), Duration::from_secs(1));

        assert_eq!(
            steps,
            vec!["asr_drain", "asr_unload", "restore_output", "db_checkpoint"],
            "the lease must be drained BEFORE the unload"
        );
        assert!(
            cancelled.load(Ordering::SeqCst),
            "the in-flight decode must be told to stop, not just waited on"
        );
        assert!(
            !manager.is_loaded(),
            "the returned engine must be unloaded, not put back for exit() to free"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the drain must end with the lease, not with the 30 s decode: {:?}",
            started.elapsed()
        );
        // The lease really did come home: the aborted decode is what ended it.
        assert!(worker.join().unwrap().is_err(), "the decode was aborted");

        drop(state);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The rare path: a decode that ignores cancellation. The quit must not
    /// hang on it — the wait is bounded, it is logged, and exit proceeds.
    #[test]
    fn teardown_times_out_on_stuck_lease_without_hanging() {
        let dir = temp_dir("yv70-stuck");
        let model = stub_model_in(&dir);

        let manager = exit_manager(|| {
            Box::new(SlowStubEngine {
                decode: Duration::from_secs(3),
                cancel: Arc::new(AtomicBool::new(false)),
                // No cancel hook: nothing can end this decode early.
                cancellable: false,
            })
        });
        manager.load("stub", &model).expect("stub model loads");
        let state = exit_state(&dir, manager.clone());

        let m = manager.clone();
        std::thread::spawn(move || {
            let _ = m.transcribe(vec![0.1; 16], None, None);
        });
        wait_until("the take to be in flight", || manager.status().transcribing);

        let started = Instant::now();
        let steps = super::teardown_for_exit_with(
            &state,
            Duration::from_millis(150),
            Duration::from_millis(50),
        );
        let elapsed = started.elapsed();

        assert_eq!(
            steps,
            vec![
                "asr_drain_timeout",
                "asr_unload",
                "restore_output",
                "db_checkpoint"
            ],
            "a stuck lease is reported, not hidden"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "Cmd-Q must not wait out a wedged decode, took {elapsed:?}"
        );
        assert!(
            elapsed >= Duration::from_millis(150),
            "…but it must actually have given the lease its budget, took {elapsed:?}"
        );

        drop(state);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The take the user already spoke must survive the quit: it either lands as
    /// a transcript, or it becomes the recoverable failed-dictation row History
    /// offers Retry on. What it may never do is silently vanish.
    ///
    /// The worker here mirrors the shape of the real one in `stop_and_transcribe`
    /// — `busy` true, transcribe, then EITHER `insert_transcript` OR the
    /// `record_failed_dictation` write `keep_failed_take` performs, and only
    /// then `busy` false — over the real database.
    #[test]
    fn quit_during_transcription_take_is_not_lost() {
        fn quit_mid_take(tag: &str, cancellable: bool) -> (usize, usize) {
            let dir = temp_dir(tag);
            let model = stub_model_in(&dir);
            let wav = dir.join("take.wav");
            std::fs::write(&wav, b"RIFF....WAVE").unwrap();

            let manager = exit_manager(move || {
                Box::new(SlowStubEngine {
                    decode: Duration::from_millis(400),
                    cancel: Arc::new(AtomicBool::new(false)),
                    cancellable,
                })
            });
            manager.load("stub", &model).expect("stub model loads");
            let state = exit_state(&dir, manager.clone());
            *state.busy.lock() = true;

            let worker = {
                let state = state.clone();
                let wav = wav.clone();
                std::thread::spawn(move || {
                    let result = state.transcription.transcribe(vec![0.1; 16], None, None);
                    // The real worker still has the gates, polish and the paste
                    // ahead of it here — the window this test is about.
                    std::thread::sleep(Duration::from_millis(100));
                    match result {
                        Ok(text) => {
                            state
                                .db
                                .insert_transcript(text, "stub".into(), 0.4, 1.0, 500, None)
                                .expect("the take lands in history");
                        }
                        Err(e) => {
                            state
                                .db
                                .record_failed_dictation(&wav, 1.0, &e, None)
                                .expect("the take stays recoverable");
                        }
                    }
                    *state.busy.lock() = false;
                })
            };
            wait_until("the take to be in flight", || manager.status().transcribing);

            // Cmd-Q, mid-transcription.
            let steps = super::teardown_for_exit_with(
                &state,
                Duration::from_secs(5),
                Duration::from_secs(5),
            );
            assert!(
                steps.contains(&"await_take"),
                "the teardown must wait for the in-flight take to land: {steps:?}"
            );
            assert!(
                !manager.is_loaded(),
                "and it must still leave no Metal device behind"
            );

            // Everything below is asserted at the moment the process would call
            // exit() — the worker gets nothing more after this point.
            let counts = (
                state.db.list_transcripts(10, None).unwrap().len(),
                state.db.list_failed_dictations().unwrap().len(),
            );
            worker.join().unwrap();
            drop(state);
            let _ = std::fs::remove_dir_all(&dir);
            counts
        }

        // Cancelled by the drain: the decode ends early, so the take cannot land
        // as text — it must survive as a retryable row instead.
        let (transcripts, failed) = quit_mid_take("yv70-lost-cancelled", true);
        assert_eq!(
            (transcripts, failed),
            (0, 1),
            "a cancelled take must become a recoverable failed dictation"
        );

        // Not cancellable: the decode finishes on its own inside the budget, so
        // the take lands as a real transcript.
        let (transcripts, failed) = quit_mid_take("yv70-lost-landed", false);
        assert_eq!(
            (transcripts, failed),
            (1, 0),
            "a take that finished must land in history, not be thrown away"
        );
    }

    // ── DB-B: the "Finish transcribing" resume, driven for real ──────────────
    // These call `finish_recovered_decode` — the function `retry_failed_dictation`
    // runs its resume through — with a decoder that behaves like
    // `transcribe_native`: one word per voiced 100 ms block, named by the
    // block's sample value, and `Err(EMPTY_TRANSCRIPT)` when nothing is voiced.

    const BLOCK: usize = 1600;

    /// `voiced` numbered blocks (sample value = block number, from 1), then
    /// `silent` blocks of zeros.
    fn db_b_clip(voiced: usize, silent: usize) -> Vec<f32> {
        let mut v = Vec::new();
        for b in 0..voiced {
            v.extend(std::iter::repeat_n((b + 1) as f32, BLOCK));
        }
        v.extend(std::iter::repeat_n(0.0f32, silent * BLOCK));
        v
    }

    fn db_b_fake_decode(samples: Vec<f32>) -> Result<crate::transcription::AsrOutput, String> {
        let words: Vec<String> = samples
            .chunks(BLOCK)
            .filter(|c| c[0] != 0.0)
            .map(|c| format!("w{}", c[0] as usize))
            .collect();
        if words.is_empty() {
            return Err(super::EMPTY_TRANSCRIPT.into());
        }
        Ok(crate::transcription::AsrOutput {
            text: words.join(" "),
            backend: "native".into(),
            seconds: 0.1,
            load_ms: 0,
            decode_ms: 1,
            degraded: false,
            chunks: 1,
            chunks_failed: 0,
            degraded_reason: None,
        })
    }

    /// BLOCKING (both reviewers): the take died on its trailing silence, so the
    /// remainder is non-empty but has no speech. Finish must land the recovered
    /// words, not fail with "Empty transcript" on every press until the 7-day
    /// sweep deletes them.
    #[test]
    fn db_b_resume_with_a_silent_remainder_keeps_the_recovered_words() {
        let clip = db_b_clip(20, 10);
        let prefix = (1..=20)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let seen = std::cell::Cell::new(None);
        let (text, asr) = super::finish_recovered_decode(
            Some(prefix.clone()),
            Ok((20 * BLOCK) as i64),
            &clip,
            |rest| {
                seen.set(Some(rest.len()));
                db_b_fake_decode(rest)
            },
        )
        .expect("a silent remainder must not make a recovered take unfinishable");
        assert_eq!(
            seen.get(),
            Some(10 * BLOCK),
            "the REMAINDER must be decoded"
        );
        assert_eq!(text, prefix);
        assert_eq!(asr.backend, "recovered");
    }

    /// BLOCKING (independent reviewer): the resume point cannot be read while
    /// the words can. Decoding from 0 with the prefix kept would double every
    /// recovered word and the retry then deletes the only audio.
    #[test]
    fn db_b_unreadable_resume_point_never_doubles_the_transcript() {
        let clip = db_b_clip(4, 0);
        let seen = std::cell::Cell::new(None);
        let (text, _) = super::finish_recovered_decode(
            Some("w1 w2".into()),
            Err("database is locked".into()),
            &clip,
            |rest| {
                seen.set(Some(rest.len()));
                db_b_fake_decode(rest)
            },
        )
        .expect("the whole clip is still decodable");
        assert_eq!(
            seen.get(),
            Some(4 * BLOCK),
            "falls back to a whole-clip decode"
        );
        assert_eq!(text, "w1 w2 w3 w4", "no recovered word may appear twice");
    }

    /// Same polarity for a resume point that reads 0 next to non-empty words.
    #[test]
    fn db_b_zero_resume_point_with_words_decodes_whole_without_prefix() {
        let clip = db_b_clip(3, 0);
        let (text, _) =
            super::finish_recovered_decode(Some("w1".into()), Ok(0), &clip, db_b_fake_decode)
                .unwrap();
        assert_eq!(text, "w1 w2 w3");
    }

    #[test]
    fn db_b_resume_mid_take_appends_only_the_remainder() {
        let clip = db_b_clip(4, 0);
        let (text, asr) = super::finish_recovered_decode(
            Some("w1 w2".into()),
            Ok((2 * BLOCK) as i64),
            &clip,
            db_b_fake_decode,
        )
        .unwrap();
        assert_eq!(text, "w1 w2 w3 w4");
        assert_eq!(asr.backend, "native");
    }

    /// Only a BLANK decode is an empty tail. A real engine failure still fails,
    /// so the row and its audio stay for another try.
    #[test]
    fn db_b_engine_failure_on_the_remainder_still_fails_closed() {
        let clip = db_b_clip(4, 0);
        let r = super::finish_recovered_decode(
            Some("w1 w2".into()),
            Ok((2 * BLOCK) as i64),
            &clip,
            |_| Err("transcription worker died unexpectedly".into()),
        );
        assert_eq!(
            r.err().as_deref(),
            Some("transcription worker died unexpectedly")
        );
    }

    /// A row with no recovered words keeps the old contract: a blank decode is
    /// an error, never an empty history entry.
    #[test]
    fn db_b_no_prefix_blank_decode_is_still_an_error() {
        let clip = db_b_clip(0, 5);
        let r = super::finish_recovered_decode(None, Ok(0), &clip, db_b_fake_decode);
        assert_eq!(r.err().as_deref(), Some(super::EMPTY_TRANSCRIPT));
    }

    #[test]
    fn db_b_words_covering_the_whole_clip_skip_the_engine() {
        let clip = db_b_clip(2, 0);
        let (text, asr) = super::finish_recovered_decode(
            Some("w1 w2".into()),
            Ok((2 * BLOCK) as i64),
            &clip,
            |_| panic!("nothing is left to decode"),
        )
        .unwrap();
        assert_eq!(text, "w1 w2");
        assert_eq!(asr.backend, "recovered");
    }

    /// Wiring: the command runs its resume through the helper above, and the
    /// helper's empty-tail arm matches the error `transcribe_native` emits.
    #[test]
    fn db_b_retry_command_resumes_through_the_tested_helper() {
        const LIB: &str = include_str!("lib.rs");
        let body = LIB
            .split_once("fn retry_failed_dictation(")
            .unwrap()
            .1
            .split_once("/// YV52 — throw a failed take away")
            .unwrap()
            .0;
        assert!(body.contains("decode_failed_take("));
        let helper = LIB
            .split_once("fn decode_failed_take(")
            .unwrap()
            .1
            .split_once("\n}\n")
            .unwrap()
            .0;
        assert!(helper.contains("finish_recovered_decode("));
        assert!(helper.contains("retry_outcome(row, decoded, gate_cut.get())"));
        for b in [body, helper] {
            assert!(!b.contains(".unwrap_or(0)"), "no fabricated resume point");
        }
        let native = LIB
            .split_once("fn transcribe_native(")
            .unwrap()
            .1
            .split_once("\n}\n")
            .unwrap()
            .0;
        assert!(native.contains("return Err(blank_take_error(&take));"));
    }

    /// PR #178 re-review: a remainder whose windows partly FAILED and partly
    /// decoded to silence comes back blank, but it is not a silent tail. It
    /// must not be reported as `EMPTY_TRANSCRIPT`, or the resume converts the
    /// row on the recovered words alone and the retry deletes the only copy of
    /// the failed window's audio.
    #[test]
    fn db_b_blank_decode_with_failed_windows_is_not_a_silent_tail() {
        let take = |chunks_failed: usize| crate::transcription::DictationTake {
            text: String::new(),
            degraded: chunks_failed > 0,
            windowed: true,
            chunks_total: 3,
            chunks_failed,
            degraded_reason: (chunks_failed > 0).then(|| "chunk decode timed out".to_string()),
        };
        // Pure silence is still the silent tail the resume may finish on.
        assert_eq!(super::blank_take_error(&take(0)), super::EMPTY_TRANSCRIPT);
        let err = super::blank_take_error(&take(1));
        assert_ne!(err, super::EMPTY_TRANSCRIPT);
        // And the resume fails closed on it: the row and the wav stay.
        let clip = db_b_clip(2, 30);
        let r = super::finish_recovered_decode(
            Some("w1 w2".into()),
            Ok((2 * BLOCK) as i64),
            &clip,
            |_| Err(err.clone()),
        );
        assert_eq!(r.err(), Some(err));
    }

    /// PR #216 — the retry invariant (`retry_outcome`) over attempt HISTORIES,
    /// each on a resume row (recovered "w1 w2" + remainder) and a whole-clip
    /// row, through the REAL windowed decode (`transcribe_native` →
    /// `transcribe_take_with_observed`, a stub engine scripted per window), a
    /// real database, and the same convert-and-delete tail the command runs.
    /// After every attempt: row kept or converted, wav kept or deleted, and the
    /// saved words.
    #[test]
    fn db_b_retry_invariant_over_attempt_histories() {
        use crate::transcription::{
            Transcriber, TranscriptionManager, DICTATION_CHUNK_SAMPLE_RATE,
            DICTATION_CHUNK_THRESHOLD_SECONDS,
        };
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex};
        use std::time::Duration;

        /// What window `i` of one attempt decodes to (`n` = windows planned).
        #[derive(Clone, Copy, PartialEq, Debug)]
        enum Script {
            /// Every window decodes: w3, w4, w5, …
            Clean,
            /// Every window decodes to nothing (a model switch, or silence).
            Blank,
            /// Window 0 → "w3"; every later window fails.
            Partial,
            /// Window 0 → "W3," and window 1 → "w4"; the rest fail.
            PartialPunct,
            /// Every window but the last decodes (w3 …); the last fails.
            MostlyOk,
            /// Window 1 → "w4"; every other window fails.
            OtherWindow,
            /// Window 0 loops ("w3" × 60); every later window fails.
            Loop,
            /// Clean, but window 0 is 14 real words then "the" × 60 and the
            /// rest are blank: 76 words raw, 16 after the gate.
            CleanLoop,
            /// Clean, but only window 0 → "w3"; the rest blank (model switch).
            CleanFew,
            /// Clean, window 0 → "x3", the rest blank: same count as Partial,
            /// different words.
            CleanOther,
            /// Window 0 is 14 real words then "the" × 60; the rest decode
            /// cleanly (w4 …). The gate cuts the remainder part-way.
            MidLoop,
            /// Window 0 is "the" × 60; the rest decode cleanly (w4 …).
            LoopFirst,
            /// Not a decode: an earlier build saved a repetition loop on the row.
            SeedLoop,
        }
        fn window(script: Script, i: usize, n: usize) -> Result<String, String> {
            let fail = || Err(format!("window {i} timed out"));
            match (script, i) {
                (Script::Clean, i) => Ok(format!("w{}", i + 3)),
                (Script::MostlyOk, i) if i + 1 < n => Ok(format!("w{}", i + 3)),
                (Script::Blank, _) => Ok(String::new()),
                (Script::Partial, 0) => Ok("w3".into()),
                (Script::PartialPunct, 0) => Ok("W3,".into()),
                (Script::PartialPunct, 1) => Ok("w4".into()),
                (Script::OtherWindow, 1) => Ok("w4".into()),
                (Script::Loop, 0) => Ok(vec!["w3"; 60].join(" ")),
                (Script::CleanLoop, 0) => {
                    let real: Vec<String> = (1..=14).map(|k| format!("r{k}")).collect();
                    Ok(format!("{} {}", real.join(" "), vec!["the"; 60].join(" ")))
                }
                (Script::CleanFew, 0) => Ok("w3".into()),
                (Script::CleanOther, 0) => Ok("x3".into()),
                (Script::MidLoop, 0) => {
                    let real: Vec<String> = (1..=14).map(|k| format!("r{k}")).collect();
                    Ok(format!("{} {}", real.join(" "), vec!["the"; 60].join(" ")))
                }
                (Script::LoopFirst, 0) => Ok(vec!["the"; 60].join(" ")),
                (Script::MidLoop | Script::LoopFirst, i) => Ok(format!("w{}", i + 3)),
                (Script::CleanLoop | Script::CleanFew | Script::CleanOther, _) => Ok(String::new()),
                _ => fail(),
            }
        }
        let windows = Arc::new(AtomicUsize::new(usize::MAX));
        struct Scripted(Arc<AtomicUsize>, Arc<Mutex<Script>>, Arc<AtomicUsize>);
        impl Transcriber for Scripted {
            fn transcribe(
                &mut self,
                _s: &[f32],
                _l: Option<&str>,
                _b: Option<&str>,
            ) -> Result<String, String> {
                let i = self.0.fetch_add(1, Ordering::SeqCst);
                window(*self.1.lock().unwrap(), i, self.2.load(Ordering::SeqCst))
            }
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let script = Arc::new(Mutex::new(Script::Clean));
        let manager = {
            let (calls, script, windows) = (calls.clone(), script.clone(), windows.clone());
            TranscriptionManager::with_loader(
                Arc::new(move |_p: &std::path::Path| {
                    Ok(
                        Box::new(Scripted(calls.clone(), script.clone(), windows.clone()))
                            as Box<dyn Transcriber>,
                    )
                }),
                Duration::from_secs(3600),
                Duration::from_secs(3600),
                Duration::from_secs(30),
            )
        };
        let dir = std::env::temp_dir().join(format!("db-b-inv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let model = dir.join("stub.bin");
        std::fs::write(&model, b"stub").unwrap();
        let rate = DICTATION_CHUNK_SAMPLE_RATE as usize;
        let resume = 10 * rate;
        let remainder = vec![0.1f32; (DICTATION_CHUNK_THRESHOLD_SECONDS as usize + 40) * rate];
        let resume_clip: Vec<f32> = std::iter::repeat_n(0.1f32, resume)
            .chain(remainder.iter().copied())
            .collect();
        let db = crate::db::Database::open_in_memory().unwrap();

        // One "Finish transcribing" exactly as the command runs it.
        let attempt = |id: &str, s: Script| {
            calls.store(0, Ordering::SeqCst);
            *script.lock().unwrap() = s;
            let row = db.get_failed_dictation(id).unwrap().expect("row");
            let resume_sample = match row.take_id.as_deref() {
                Some(t) => db.take_resume_sample(t),
                None => Ok(0),
            };
            let clip = if row.take_id.is_some() {
                &resume_clip
            } else {
                &remainder
            };
            let wav = std::path::PathBuf::from(&row.wav_path);
            super::decode_failed_take(
                &db,
                &row,
                row.partial_text.clone(),
                resume_sample,
                clip,
                |rest| {
                    super::transcribe_native(
                        &manager,
                        "stub",
                        &model,
                        rest,
                        "",
                        None,
                        &mut crate::transcribe_progress::NoObserver,
                    )
                },
            )
            .and_then(|(raw, asr)| {
                super::convert_and_drop_clip(&db, &row, &wav, raw.clone(), asr, Some(raw))
            })
        };

        // How many windows the remainder plans into (the same for both rows).
        let probe = db
            .record_failed_dictation_for_take(&dir.join("probe.wav"), 1.0, "x", None, None)
            .unwrap();
        let _ = attempt(&probe.id, Script::Blank);
        let n = calls.load(Ordering::SeqCst);
        assert!(
            (2..40).contains(&n),
            "the remainder must window (got {n} windows)"
        );
        let clean_tail: Vec<String> = (0..n).map(|i| format!("w{}", i + 3)).collect();
        let clean_tail = clean_tail.join(" ");
        windows.store(n, Ordering::SeqCst);
        let looped = vec!["w3"; 60].join(" ");
        let seq = |to: usize| -> String {
            (3..=to)
                .map(|k| format!("w{k}"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let mostly = seq(n + 1);
        let r14 = (1..=14)
            .map(|k| format!("r{k}"))
            .collect::<Vec<_>>()
            .join(" ");

        // Expected state after an attempt: None = converted (row gone, wav
        // deleted); Some(saved words) = row and wav kept with these words.
        type Want = Option<Option<String>>;
        let kept = |t: &str| -> Want { Some(Some(t.to_string())) };
        for resume_row in [true, false] {
            let p = |t: &str| {
                if resume_row {
                    format!("w1 w2 {t}")
                } else {
                    t.to_string()
                }
            };
            // Resume rows keep their recovered words when nothing else is saved.
            let recovered: Want = Some(resume_row.then(|| "w1 w2".to_string()));
            let histories: Vec<(&str, Vec<(Script, Want)>)> = vec![
                ("clean full decode", vec![(Script::Clean, None)]),
                (
                    "blank, 0 failures, nothing saved",
                    // Resume: the remainder was silence, the recovered words are
                    // the whole take. Whole clip: nothing to convert, kept.
                    vec![(Script::Blank, if resume_row { None } else { Some(None) })],
                ),
                (
                    "blank, 0 failures, longer text saved",
                    vec![
                        (Script::Partial, kept(&p("w3"))),
                        (Script::Blank, kept(&p("w3"))),
                        (Script::Blank, kept(&p("w3"))),
                    ],
                ),
                (
                    "partial with failures",
                    vec![(Script::Partial, kept(&p("w3")))],
                ),
                (
                    // The gate runs on a failed attempt too: a loop is never
                    // saved as "transcribed so far".
                    "looping attempt is gated, not saved",
                    vec![(Script::Loop, recovered.clone())],
                ),
                (
                    // rr1 on 9784adf: 76 raw words pass "not shorter" against 22
                    // saved, the gate cuts them to 16 — the invariant must see 16.
                    "looping clean decode the gate trims below saved",
                    vec![
                        (Script::MostlyOk, kept(&p(&mostly))),
                        (Script::CleanLoop, kept(&p(&mostly))),
                    ],
                ),
                (
                    // rr2 on 9784adf: fewer clean words (a model switch) never
                    // replace non-degenerate saved words, so no later press can
                    // convert on the shorter text.
                    "clean shorter decode, non-degenerate saved text",
                    vec![
                        (Script::MostlyOk, kept(&p(&mostly))),
                        (Script::CleanFew, kept(&p(&mostly))),
                        (Script::CleanFew, kept(&p(&mostly))),
                        (Script::Clean, None),
                    ],
                ),
                (
                    // A loop saved by an earlier build is gated before it is
                    // counted, so it cannot hold a clean decode back.
                    "clean after a saved loop converts",
                    vec![(Script::SeedLoop, kept(&p(&looped))), (Script::Clean, None)],
                ),
                (
                    // As designed, documented: an attempt with failed windows
                    // whose words do not EXTEND the saved ones leaves them as
                    // they were; a CLEAN decode with the same word count passes
                    // (b) and converts on its own words.
                    "same count, different words",
                    vec![
                        (Script::Partial, kept(&p("w3"))),
                        (Script::OtherWindow, kept(&p("w3"))),
                        (Script::CleanOther, None),
                    ],
                ),
                (
                    // rr1 advisory on 8bf5225: a remainder that STARTS with a
                    // loop is rejected by the gate; that is a cut, never silence.
                    "remainder begins with a loop",
                    vec![(Script::LoopFirst, recovered.clone())],
                ),
                (
                    // A gate cut part-way through the remainder: the clean words
                    // after the loop are in no text, so nothing auto-deletes.
                    "remainder loops part-way",
                    vec![(Script::MidLoop, kept(&p(&r14)))],
                ),
                (
                    // Extending ignores case and punctuation: "W3," extends "w3".
                    "punctuation does not block an extension",
                    vec![
                        (Script::Partial, kept(&p("w3"))),
                        (Script::PartialPunct, kept(&p("W3, w4"))),
                    ],
                ),
            ];
            for (name, steps) in histories {
                let case = format!(
                    "{name} / {}",
                    if resume_row {
                        "resume row"
                    } else {
                        "whole-clip row"
                    }
                );
                let take_id = resume_row.then(|| format!("take-{}", name.replace(' ', "-")));
                if let Some(t) = &take_id {
                    db.record_take_chunk(t, 0, "w1 w2", resume as i64).unwrap();
                }
                let wav = dir.join(format!("{}-{resume_row}.wav", name.replace(' ', "-")));
                std::fs::write(&wav, b"RIFF stand-in").unwrap();
                let row = db
                    .record_failed_dictation_for_take(&wav, 120.0, "Yap quit", None, take_id)
                    .unwrap();
                for (k, (s, want)) in steps.into_iter().enumerate() {
                    let ctx = format!("{case}, attempt {} ({s:?})", k + 1);
                    let r = if s == Script::SeedLoop {
                        let t = p(&looped);
                        db.note_failed_retry(&row.id, "earlier", Some(&t)).unwrap();
                        Err("earlier".to_string())
                    } else {
                        attempt(&row.id, s)
                    };
                    let now = db.get_failed_dictation(&row.id).unwrap();
                    match want {
                        None => {
                            assert!(r.is_ok(), "{ctx}: should convert, got {:?}", r.err());
                            assert!(now.is_none(), "{ctx}: row converted");
                            assert!(!wav.exists(), "{ctx}: clip deleted");
                        }
                        Some(saved) => {
                            let err = r.expect_err(&ctx);
                            let now = now.unwrap_or_else(|| panic!("{ctx}: row kept"));
                            assert!(wav.exists(), "{ctx}: clip kept");
                            assert_eq!(now.attempt_text, saved, "{ctx}: saved words");
                            assert_eq!(now.error, err, "{ctx}: the row shows this error");
                            assert_eq!(now.partial_text, row.partial_text, "{ctx}: resume state");
                        }
                    }
                }
            }
        }
        // rr1 on 2b09b0d — recovered words (`partial_text`) come from the live
        // decode and were never gated. (recovered words, their end sample —
        // 0 = the resume point is unusable and the retry decodes the whole
        // clip —, script, converted raw text, or the saved words Keep saves.)
        let real14 = (1..=14)
            .map(|k| format!("r{k}"))
            .collect::<Vec<_>>()
            .join(" ");
        let looped_recovered = format!("{real14} {}", vec!["the"; 60].join(" "));
        /// (name, recovered words, their end sample, script, converts to, Keep saves)
        type Special<'a> = (
            &'a str,
            &'a str,
            usize,
            Script,
            Option<String>,
            Option<String>,
        );
        let special: Vec<Special> = vec![
            (
                // rr1 on 8bf5225: the resume starts AFTER the looped chunk, so
                // the speech under the loop is never re-decoded — a gate cut,
                // so the row and clip stay; Keep saves gated 14 + remainder.
                "loop in recovered words + clean remainder",
                &looped_recovered,
                resume,
                Script::Clean,
                None,
                Some(format!("{real14} {clean_tail}")),
            ),
            (
                "loop in recovered words + blank remainder",
                &looped_recovered,
                resume,
                Script::Blank,
                None,
                Some(real14.clone()),
            ),
            (
                "loop in recovered words + failed remainder",
                &looped_recovered,
                resume,
                Script::Partial,
                None,
                Some(format!("{real14} w3")),
            ),
            (
                "whole-clip fallback, short clean decode",
                &real14,
                0,
                Script::CleanFew,
                None,
                Some(real14.clone()),
            ),
        ];
        for (name, recovered, end, s, converts_to, keeps) in special {
            let t = format!("special-{}", name.replace(' ', "-"));
            db.record_take_chunk(&t, 0, recovered, end as i64).unwrap();
            let wav = dir.join(format!("{t}.wav"));
            std::fs::write(&wav, b"RIFF stand-in").unwrap();
            let row = db
                .record_failed_dictation_for_take(&wav, 120.0, "Yap quit", None, Some(t))
                .unwrap();
            let r = attempt(&row.id, s);
            let now = db.get_failed_dictation(&row.id).unwrap();
            match converts_to {
                Some(raw) => {
                    let entry = r.expect(name);
                    assert_eq!(entry.raw_text.as_deref(), Some(raw.as_str()), "{name}");
                    assert!(now.is_none(), "{name}: row converted");
                    assert!(!wav.exists(), "{name}: clip deleted");
                }
                None => {
                    assert!(r.is_err(), "{name}: must keep the row");
                    let now = now.unwrap_or_else(|| panic!("{name}: row kept"));
                    assert!(wav.exists(), "{name}: clip kept");
                    assert_eq!(now.attempt_text, keeps, "{name}: saved words");
                    assert_eq!(super::saved_words(&now), keeps, "{name}: what Keep saves");
                    // A row a previous build left with a SHORTER attempt text
                    // that does not extend them: Keep saves the gated
                    // recovered words, never the shorter attempt.
                    db.note_failed_retry(&row.id, "older build", Some("w3"))
                        .unwrap();
                    let older = db.get_failed_dictation(&row.id).unwrap().unwrap();
                    assert_eq!(
                        super::saved_words(&older).as_deref(),
                        Some(real14.as_str()),
                        "{name}: Keep vs a short attempt"
                    );
                }
            }
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// YV66 — the retry path runs the SAME gate as the live path. It is the one
    /// caller that DELETES the recovery wav on success, so a degenerate tail
    /// reaching the clipboard whole would be unrecoverable: no audio left to
    /// retry against. A whole-take verdict is not enough here, because a take
    /// that is clean for 300 tokens and only then sticks is not degenerate from
    /// its first token — it must be CUT.
    #[test]
    fn retry_gate_truncates_degenerate_tail() {
        const LIB: &str = include_str!("lib.rs");
        let body = LIB
            .split_once("fn retry_failed_dictation(")
            .expect("the retry command must exist")
            .1
            .split_once("/// YV52 — throw a failed take away")
            .expect("the retry command must end where the next command begins")
            .0;
        let gate = LIB
            .split_once("fn retry_gate(")
            .expect("the retry gate must exist")
            .1
            .split_once("\n}\n")
            .unwrap()
            .0;
        assert!(body.contains("retry_gate(&mut kept)"), "Keep is gated too");
        assert!(
            gate.contains("dictation::degenerate_cutoff("),
            "the retry path must gate on the cutoff, not on a whole-take verdict"
        );
        assert!(
            !gate.contains("is_hallucinated_repetition"),
            "the whole-take verdict is false for a degenerate TAIL — it would ship the loop"
        );

        // 300 tokens of dictation, then the decoder sticks on "the": the
        // keep-branch of that gate is the one that has to fire.
        let prose = (0..300)
            .map(|i| format!("word{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let text = format!("{}{}", prose, " the".repeat(200));
        assert!(
            !crate::dictation::is_hallucinated_repetition(&text),
            "fixture must be one the whole-take verdict lets through"
        );
        let cutoff =
            crate::dictation::degenerate_cutoff(&text).expect("the degenerate tail must be found");
        assert!(
            text[..cutoff].split_whitespace().count() >= crate::dictation::DEGEN_MIN_KEEP_TOKENS,
            "the gate must truncate this take, not reject it whole"
        );
        assert_eq!(text[..cutoff].trim_end(), prose);
        let mut gated = text.clone();
        super::retry_gate(&mut gated).expect("truncated, not rejected");
        assert_eq!(gated.trim_end(), prose);
    }

    /// YV66 × YV67 — the two features meet inside ONE `match` arm, so this is
    /// the seam where either can silently undo the other. YV66 turns the live
    /// hallucination gate from a boolean into a cutoff; YV67 requires that the
    /// arm which still rejects a whole take returns `TakeOutcome::Rejected`, so
    /// `keep_failed_take` gets the wav and the user gets a Retry row. Returning
    /// `Soft` there compiles, passes every other test, and quietly restores the
    /// bug YV67 fixed: the audio is unlinked by `ClipWav::drop` and the take is
    /// gone. Pin the arm itself.
    #[test]
    fn live_gate_whole_take_rejection_keeps_its_audio() {
        const LIB: &str = include_str!("lib.rs");
        let gate = LIB
            .split_once("match dictation::degenerate_cutoff(&raw_text) {")
            .expect("the live gate must run the cutoff")
            .1
            .split_once("// YV49 command mode")
            .expect("the live gate ends where command mode begins")
            .0;
        assert!(
            gate.contains("return Ok(TakeOutcome::Rejected {")
                && gate.contains("reason: GATE_REPETITION_REASON,"),
            "a whole-take rejection must keep the wav as a retryable row (YV67)"
        );
        assert!(
            !gate.contains("TakeOutcome::Soft("),
            "a Soft outcome discards the wav — the take would be unrecoverable"
        );
        // And the arm it guards is reachable: a take that is degenerate from its
        // very first token still cuts at 0, which is what selects that arm.
        assert_eq!(
            crate::dictation::degenerate_cutoff(&" the".repeat(200)),
            Some(0)
        );
    }

    /// YV79 — the onboarding overlay clears "Transcribing…" on ONE event now
    /// ([`super::TAKE_DONE_EVENT`]), so a terminal arm that forgets to emit it
    /// is a dead spinner for the whole calibration watchdog. `Soft` WAS exactly
    /// that arm: a silent mic (permission denied, wrong input device) trips the
    /// no-speech gate, which emitted only the toast channel the overlay cannot
    /// hear. Nothing in the type system says an arm must announce itself, so
    /// pin every one of them against the source.
    #[test]
    fn every_take_outcome_emits_a_terminal_event() {
        const LIB: &str = include_str!("lib.rs");
        let arms = LIB
            .split_once("        match result {")
            .expect("the transcribe worker must match on its take result")
            .1
            .split_once("        *state2.busy.lock() = false;")
            .expect("the take-result match ends where the worker clears busy")
            .0;
        // In source order — each arm is the slice from its own head to the next.
        const HEADS: [&str; 4] = [
            "Ok(TakeOutcome::Dictated(",
            "Ok(TakeOutcome::Soft(",
            "Ok(TakeOutcome::Rejected {",
            "Err(e) => {",
        ];
        let mut cursor = 0usize;
        for (i, head) in HEADS.iter().enumerate() {
            let start = cursor
                + arms[cursor..].find(head).unwrap_or_else(|| {
                    panic!("the take-result match must still have a `{head}` arm")
                });
            let end = match HEADS.get(i + 1) {
                Some(next) => {
                    start
                        + arms[start..]
                            .find(next)
                            .unwrap_or_else(|| panic!("`{next}` must follow `{head}`"))
                }
                None => arms.len(),
            };
            assert!(
                arms[start..end].contains("TAKE_DONE_EVENT"),
                "the `{head}` arm must emit TAKE_DONE_EVENT — without it every view \
                 waiting on this outcome spins until its watchdog"
            );
            cursor = start + head.len();
        }
    }
}
