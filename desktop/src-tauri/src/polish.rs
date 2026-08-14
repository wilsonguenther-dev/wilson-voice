//! The local-LLM polish stage and its never-lose-text gate (YV61 — spec item
//! YV57, `docs/research/wispr-formatting-deep-dive.md` §2.5–2.6).
//!
//! The sidecar is UNTRUSTED output. Nothing it returns reaches the pasteboard
//! unvalidated: [`validate_polish`] is a pure function that accepts a rewrite or
//! rejects it, and a rejection is not an error — the caller simply keeps the
//! rules-stage text it already had. That is what makes "never lose text"
//! provable rather than hoped for.
//!
//! Three layers, deliberately separated so the risky ones are testable with
//! ZERO model bytes:
//!
//! ```text
//! polish_llm(text, mode, cfg)   // production: resolves the installed model,
//!   └─ polish_stage(…, client)  //   builds the request, and picks the sidecar
//!        ├─ build_request       //   V8 + the off/too-short/too-long gates
//!        └─ polish_with         //   deadline + panic guard around the client
//!             └─ validate_polish//   V1–V7 over the returned string
//! ```
//!
//! [`PolishClient`] is the seam: the tests inject a client that sleeps past the
//! deadline, one that panics, one that returns garbage, and assert the pipeline
//! output is byte-identical to the rules output in every case.
//!
//! YV75 added the second seam, one layer down: [`SidecarPool`] owns the warm
//! child, the readiness handshake and the restart budget, and takes its
//! `Command` from a launcher — so the tests drive the real state machine
//! against a stub PROCESS, with no model on disk. The handshake is the fix for
//! a stage that could never start: the sidecar's readiness line used to go to a
//! stderr the parent nulled, so a cold child (seconds of GGUF page-in) was
//! indistinguishable from a wedged one and every take inside that window burned
//! its whole deadline waiting for an answer that could not come.
//!
//! R5 (lead casing from the caret context) is NOT re-applied here on purpose:
//! `lib.rs` runs `dictation::join_with_context` AFTER `run_cleanup`, so the
//! context's casing decision lands after the model and the model cannot override
//! it (asserted by `polish_never_overrides_the_r5_lead_case`). Forcing the rules
//! text's own casing back on here would instead undo the capitalization fixes
//! the model is asked for.

use std::io::{BufRead, BufReader, Write};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;

use crate::dictation::{self, DictationMode, Style};
use crate::models;
use crate::polish_protocol::{max_out_for, parse_ready, parse_response_for, PolishRequest};

/// Hard deadline for one polish pass (§2.3). Past it the answer is dropped and
/// the rules text is what gets pasted.
pub const DEFAULT_POLISH_DEADLINE_MS: u64 = 1200;

/// Bounds on the configured deadline. Below the floor the stage could never
/// answer; above the ceiling it stops being a dictation app.
const MIN_POLISH_DEADLINE_MS: u64 = 100;
const MAX_POLISH_DEADLINE_MS: u64 = 5000;

/// Nothing to fix below this — a 3-word utterance costs a model round trip and
/// gains nothing (§2.3).
const MIN_POLISH_WORDS: usize = 4;
/// Long-form is rules-only: chunking is a later item, and a single pass over
/// 400+ words cannot hold the deadline (§2.3).
const MAX_POLISH_WORDS: usize = 400;

/// Content-word retention floor (§2.5 V3).
const RETENTION_FLOOR: f64 = 0.80;
/// Length band around the input, list markers and newlines excluded (§2.5 V2).
const MAX_LENGTH_RATIO: f64 = 2.5;
const MIN_LENGTH_RATIO: f64 = 0.45;
/// Non-ASCII ratio the output may add over the input before it reads as script
/// drift (§2.5 V7).
pub(crate) const MAX_NON_ASCII_DRIFT: f64 = 0.25;

/// Template / chat-scaffold markers that must never survive into the paste
/// (§2.5 V6).
pub(crate) const TEMPLATE_MARKERS: &[&str] = &["<|im_start|>", "<|im_end|>", "<think>", "```"];

/// Assistant openers (§2.5 V6). A typist does not announce itself; a rewrite
/// that starts this way answered the dictation instead of retyping it.
pub(crate) const ASSISTANT_PREAMBLES: &[&str] =
    &["sure,", "here is", "here's the", "i've ", "certainly"];

/// Filename of the bundled sidecar. Tauri strips the target triple from
/// `bundle.externalBin` when it stages the binary next to the app executable,
/// and a workspace `cargo build` puts it in the same directory in dev.
const SIDECAR_BIN: &str = "yap-polish";

/// How long a freshly spawned sidecar may take to announce readiness (YV75)
/// before it is declared failed.
///
/// This is a CEILING ON LATENESS, not a wait anybody pays: no dictation ever
/// blocks on it (see [`SidecarPool::rewrite`]). The number is derived rather
/// than guessed — the catalog's two polish GGUFs are 1.12 GB (1.5B) and 491 MB
/// (0.5B), and a cold start pays a full page-in of that file plus the Metal
/// upload before it can answer anything. 10s covers the large model on a cold
/// page cache with headroom; past it the child is not loading, it is stuck, and
/// saying so beats sitting in `starting` forever.
const SIDECAR_READY_BUDGET: Duration = Duration::from_secs(10);

/// YV81 — how long a warm sidecar may sit UNUSED before it is terminated.
///
/// The child is a resident llama.cpp process holding its whole GGUF (491 MB or
/// 1.12 GB, plus its Metal buffers) for the rest of the session, and a machine
/// left with Yap open all day pays that for takes that already happened. Ten
/// minutes is past any plausible pause inside one writing session, so the cost
/// of being wrong is one model load on the next take — the same load the first
/// take of the session pays, and never on the dictation path (YV75: a loading
/// child makes the take rules-only, it does not block it).
const SIDECAR_IDLE_UNLOAD: Duration = Duration::from_secs(10 * 60);

/// Respawns allowed per app session after the child dies (YV75). One: a single
/// death is usually transient (a paged-out model, a process killed under memory
/// pressure), a second is the model or the machine, and an unbounded respawn
/// would pay a process launch on every dictation forever.
const MAX_SIDECAR_RESTARTS: u64 = 1;

/// Cap on one logged stderr line from the child. Its stderr is diagnostics, not
/// text — but a runaway child must not be able to flood the rotating log.
const STDERR_LOG_CHARS: usize = 300;

/// Rejected rewrites since launch — `polish_rejected_total` (§2.5). A COUNT, and
/// the reason tag on the log line; neither string is ever logged.
static POLISH_REJECTED_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Monotonic request id. A response carrying an older one answers a dictation
/// the parent already gave up on and is discarded (`parse_response_for`).
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// The warm sidecar and its lifecycle, held for the app's lifetime. Built on
/// first use so the production launcher can be swapped for a stub process in
/// the tests (see [`SidecarPool`]).
fn pool() -> &'static SidecarPool {
    static POOL: OnceLock<SidecarPool> = OnceLock::new();
    POOL.get_or_init(|| SidecarPool::new(Box::new(staged_command), SIDECAR_READY_BUDGET))
}

/// Why a rewrite did not arrive. Every variant means the same thing to the
/// caller — keep the rules text — but they are distinguished so the reason tag
/// on the counter is honest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolishError {
    /// The client did not answer inside `deadline_ms`.
    Deadline,
    /// No sidecar binary, spawn failed, or the child died.
    Unavailable,
    /// The child answered something that is not a usable response line.
    Protocol,
}

/// The seam between the pipeline and the model. Implemented in production by
/// [`SidecarClient`]; implemented in tests by clients that sleep, panic, or
/// return garbage, so the failure paths are exercised with no model present.
pub trait PolishClient {
    /// Rewrite `req.text`. Any failure at all is an `Err` — a client must never
    /// invent text to satisfy this signature.
    fn rewrite(&self, req: &PolishRequest) -> Result<String, PolishError>;
}

/// Everything the polish stage needs beyond the text itself, resolved from
/// settings once per take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolishConfig {
    /// Installed polish model's catalog id. `""` = the stage is OFF, which is
    /// the default and the behaviour with no model downloaded.
    pub model: String,
    /// Hard deadline for one pass.
    pub deadline_ms: u64,
    /// Tone dial for the take's mode (YV62). The same value the rules stage runs
    /// R3 with, so the model is asked for the tone the rules already applied.
    pub style: Style,
}

impl Default for PolishConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
            deadline_ms: DEFAULT_POLISH_DEADLINE_MS,
            style: Style::Default,
        }
    }
}

impl PolishConfig {
    /// Read the stage's settings for one take: `polish_model`,
    /// `polish_deadline_ms`, and the `style_<mode>` dial for THIS mode.
    pub fn from_settings(settings: &crate::AppSettings, mode: DictationMode) -> Self {
        Self {
            model: settings.polish_model.trim().to_string(),
            deadline_ms: settings
                .polish_deadline_ms
                .clamp(MIN_POLISH_DEADLINE_MS, MAX_POLISH_DEADLINE_MS),
            style: style_for_mode(settings, mode),
        }
    }
}

/// The wire name of a mode (§2.4). `Code` has one so the refusals below can be
/// written against it — it never reaches a model.
pub fn mode_tag(mode: DictationMode) -> &'static str {
    match mode {
        DictationMode::Email => "email",
        DictationMode::Document => "document",
        DictationMode::Notes => "notes",
        DictationMode::Code => "code",
        DictationMode::Chat => "chat",
        DictationMode::Plain => "plain",
        DictationMode::List => "list",
    }
}

/// The `style_<mode>` tone dial: the position stored for this mode, or
/// [`Style::Default`] when nothing is set for it (or the stored value is not a
/// dial position). Public since YV62 because the RULES stage needs it too — R3's
/// trailing-period rule is dialled by the same setting as the model's overlay.
pub fn style_for_mode(settings: &crate::AppSettings, mode: DictationMode) -> Style {
    Style::from_setting(
        settings
            .polish_styles
            .get(mode_tag(mode))
            .map(String::as_str)
            .unwrap_or_default(),
    )
}

/// Shape one request, or decline the stage entirely.
///
/// `None` — the model is never asked — when the stage is off (`model == ""`),
/// the mode is `Code` (§2.5 V8: code never reaches a model at all), or the
/// utterance is outside the length band the latency budget is written for.
pub fn build_request(text: &str, mode: DictationMode, cfg: &PolishConfig) -> Option<PolishRequest> {
    if cfg.model.trim().is_empty() {
        return None;
    }
    // V8 — the gate, not a filter on the answer: `Code` never leaves the app.
    if mode == DictationMode::Code {
        return None;
    }
    let words = text.split_whitespace().count();
    if !(MIN_POLISH_WORDS..=MAX_POLISH_WORDS).contains(&words) {
        return None;
    }
    // The pushed session topic, once there is one, would go on the request's
    // `topic`. NEVER the AX cursor context — that steers decisions in-process
    // and is not sent anywhere.
    Some(PolishRequest::polish(
        NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed),
        mode_tag(mode),
        cfg.style.tag(),
        max_out_for(text),
        cfg.deadline_ms,
        text.to_string(),
    ))
}

/// The whole stage for one take against a given client: request shaping, then
/// the guarded, validated call. `None` ⇒ the caller keeps its rules text.
pub fn polish_stage(
    text: &str,
    mode: DictationMode,
    cfg: &PolishConfig,
    client: &dyn PolishClient,
) -> Option<String> {
    let req = build_request(text, mode, cfg)?;
    polish_with(text, req, client)
}

/// Call `client` under a hard deadline and a panic guard, then put its answer
/// through [`validate_polish`].
///
/// Every failure mode collapses to `None`: a refusal, a panic inside the client,
/// an answer that arrived too late, or one that fails the gate. The caller's
/// text is untouched in all of them.
pub fn polish_with(text: &str, req: PolishRequest, client: &dyn PolishClient) -> Option<String> {
    // V8, defence in depth: a caller that hand-builds a code request still gets
    // no model call. `build_request` already refuses; this is the second lock.
    if req.mode == mode_tag(DictationMode::Code) {
        return reject("v8_code_mode");
    }
    let started = Instant::now();
    let deadline = Duration::from_millis(req.deadline_ms.max(1));
    // A panicking client must not take the dictation down with it — the take is
    // already recorded and the rules text is ready to paste.
    let answered = std::panic::catch_unwind(AssertUnwindSafe(|| client.rewrite(&req)));
    let rewritten = match answered {
        Ok(Ok(text)) => text,
        Ok(Err(_)) => return reject("client_error"),
        Err(_) => return reject("client_panic"),
    };
    // The parent's own clock is the authority: a client that answers late (or
    // one that cannot be interrupted) still loses its answer here.
    if started.elapsed() > deadline {
        return reject("deadline");
    }
    validate_polish(text, &rewritten)
}

/// Accept the model's rewrite, or reject it (→ caller keeps the rules output).
///
/// Pure: no I/O, and neither string is logged — a rejection bumps
/// `polish_rejected_total` and logs the reason TAG only. The checks are §2.5
/// V1–V7 in order; V8 (`mode == Code`) is a gate before the call, in
/// [`build_request`] / [`polish_with`], because code must never be sent at all.
pub fn validate_polish(input: &str, output: &str) -> Option<String> {
    // V1 — never paste nothing.
    if output.trim().is_empty() {
        return reject("v1_empty");
    }
    // V2 — runaway repetition or truncation. List markers and newlines are
    // excluded from the compare so a prose→list reflow is not a length change.
    let (before, after) = (compare_len(input), compare_len(output));
    if before > 0 {
        let ratio = after as f64 / before as f64;
        if ratio > MAX_LENGTH_RATIO {
            return reject("v2_runaway");
        }
        if ratio < MIN_LENGTH_RATIO {
            return reject("v2_truncated");
        }
    }
    // V3 — the model dropped a clause.
    if retention(input, output) < RETENTION_FLOOR {
        return reject("v3_retention");
    }
    // V4 — invented numbers, times, prices. A model that manufactures its OWN
    // list markers trips this too, deliberately: list rendering belongs to the
    // rules stage (R7–R11), which puts the markers in the input the model sees.
    let known_digits = digit_runs(input);
    if digit_runs(output).iter().any(|d| !known_digits.contains(d)) {
        return reject("v4_invented_digits");
    }
    // V5 — invented contact details.
    let known_contacts = contact_tokens(input);
    if contact_tokens(output)
        .iter()
        .any(|c| !known_contacts.contains(c))
    {
        return reject("v5_invented_contact");
    }
    // V6 — template leak or assistant behaviour.
    if TEMPLATE_MARKERS.iter().any(|m| output.contains(m)) {
        return reject("v6_template_leak");
    }
    let opening = output.trim_start().to_lowercase();
    if ASSISTANT_PREAMBLES.iter().any(|p| opening.starts_with(p)) {
        return reject("v6_preamble");
    }
    // V7 — language / script drift.
    if non_ascii_ratio(output) - non_ascii_ratio(input) > MAX_NON_ASCII_DRIFT {
        return reject("v7_script_drift");
    }
    Some(output.trim().to_string())
}

/// Rejections so far (`polish_rejected_total`, §2.5).
pub fn rejected_total() -> u64 {
    POLISH_REJECTED_TOTAL.load(Ordering::Relaxed)
}

/// Count the rejection, log the TAG, and hand the caller its `None`. The
/// rejected text and the input are never logged — YV20/M2 hygiene.
fn reject(reason: &'static str) -> Option<String> {
    POLISH_REJECTED_TOTAL.fetch_add(1, Ordering::Relaxed);
    log::info!("polish rejected: {reason}");
    None
}

/// Length for the V2 compare: non-whitespace characters, with a leading list
/// marker dropped from every line. A rules-stage list and the same list reflowed
/// by the model must not read as a 30% length change.
fn compare_len(text: &str) -> usize {
    text.lines()
        .map(|line| {
            strip_list_marker(line.trim())
                .chars()
                .filter(|c| !c.is_whitespace())
                .count()
        })
        .sum()
}

/// Drop a leading `- ` / `* ` / `12. ` marker from one line.
fn strip_list_marker(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        return rest.trim_start();
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        if let Some(rest) = line[digits..].strip_prefix(". ") {
            return rest.trim_start();
        }
    }
    line
}

/// Content words for V3: case-folded tokens of ≥ 4 characters, minus the
/// vocabulary the cleanup pipeline is LICENSED to consume (fillers, discourse
/// particles, retraction markers — see `dictation`, which owns those lists).
pub(crate) fn content_words(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’'))
        .filter(|w| w.chars().count() >= 4)
        .map(str::to_lowercase)
        .filter(|w| !is_consumable(w))
        .collect()
}

/// A word the rewrite may drop without that counting as lost content.
fn is_consumable(word: &str) -> bool {
    let mut phrase_words = dictation::DISCOURSE_PARTICLES
        .iter()
        .chain(dictation::CORRECTION_MARKERS.iter())
        .flat_map(|p| p.split(' '));
    dictation::FILLER_TOKENS.contains(&word) || phrase_words.any(|w| w == word)
}

/// Share of the input's content words that survived, counted as a multiset so a
/// dropped repetition counts as dropped.
fn retention(input: &str, output: &str) -> f64 {
    let want = content_words(input);
    if want.is_empty() {
        return 1.0;
    }
    let mut have = content_words(output);
    let mut kept = 0usize;
    for word in &want {
        if let Some(at) = have.iter().position(|h| h == word) {
            have.remove(at);
            kept += 1;
        }
    }
    kept as f64 / want.len() as f64
}

/// Every run of digits in `text` ("3:30" → `["3", "30"]`).
pub(crate) fn digit_runs(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_digit())
        .filter(|run| !run.is_empty())
        .map(str::to_string)
        .collect()
}

/// Email addresses and URLs, case-folded and stripped of the punctuation a
/// sentence wraps them in.
pub(crate) fn contact_tokens(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|t| {
            t.trim_matches(|c: char| !(c.is_alphanumeric() || "@:/._-+~%#?=&".contains(c)))
                .trim_end_matches(['.', ',', ';', ':', '!', '?'])
                .to_lowercase()
        })
        .filter(|t| is_contact(t))
        .collect()
}

/// An `@`-address or a URL — the two things a model inventing "helpful" detail
/// reaches for first.
fn is_contact(token: &str) -> bool {
    token.contains('@')
        || token.starts_with("http://")
        || token.starts_with("https://")
        || token.starts_with("www.")
}

/// Share of characters outside ASCII — the V7 script-drift signal.
pub(crate) fn non_ascii_ratio(text: &str) -> f64 {
    let total = text.chars().count();
    if total == 0 {
        return 0.0;
    }
    text.chars().filter(|c| !c.is_ascii()).count() as f64 / total as f64
}

/// The production stage: the installed polish model, the bundled sidecar, and
/// the gate.
///
/// `None` — and therefore a rules-only paste — whenever the stage is off, the
/// selected model is not fully downloaded, the mode is `Code`, the utterance is
/// out of band, the sidecar cannot answer in time, or the answer fails
/// [`validate_polish`]. Absent a model this is exactly the no-op the pipeline
/// has always had.
pub fn polish_llm(text: &str, mode: DictationMode, cfg: &PolishConfig) -> Option<String> {
    let model = models::polish_model(cfg.model.trim())
        .filter(|m| models::is_polish_downloaded(m))
        .map(models::polish_model_path)?;
    polish_stage(text, mode, cfg, &SidecarClient::new(model))
}

/// The polish model this machine could actually run right now: the catalog's
/// best-ranked entry that is fully downloaded, if any. The stage itself uses the
/// user's `polish_model` setting — this exists so the ignored latency harness
/// (YV59) can measure the real thing without hard-coding a catalog id.
pub fn installed_model_id() -> Option<String> {
    models::polish_models()
        .iter()
        .filter(|m| models::is_polish_downloaded(m))
        .min_by_key(|m| m.recommended_rank.unwrap_or(u32::MAX))
        .map(|m| m.id.clone())
}

/// The sidecar's lifecycle, as Diagnostics reports it (YV75).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SidecarState {
    /// No sidecar process is running: the stage is off, no polish model is
    /// installed, the binary was never staged next to the app, or the child was
    /// killed and the next take will spawn a fresh one.
    NotInstalled,
    /// Spawned, model still loading — inside the readiness budget. Takes in
    /// this window are rules-only.
    Starting,
    /// The readiness line arrived with the model resident; requests are served.
    Ready,
    /// Given up on for this app session, with a reason.
    Failed,
}

/// The sidecar state plus why, for `engine_status` (YV75).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarStatus {
    pub state: SidecarState,
    /// A short tag — `no_binary`, `spawn_failed`, `ready_timeout`,
    /// `not_loaded`, `died`. NEVER text: nothing dictated can reach the UI or
    /// the log through this field.
    pub reason: Option<&'static str>,
}

impl SidecarState {
    /// A stable one-word tag for the energy telemetry line (YV81). Never text.
    pub fn tag(self) -> &'static str {
        match self {
            Self::NotInstalled => "unloaded",
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

impl SidecarStatus {
    const fn new(state: SidecarState, reason: Option<&'static str>) -> Self {
        Self { state, reason }
    }
}

/// The polish sidecar's state for `engine_status` / Diagnostics (YV75).
pub fn sidecar_status() -> SidecarStatus {
    pool().status()
}

/// YV81 — unload a warm sidecar that nobody has used for
/// [`SIDECAR_IDLE_UNLOAD`], returning whether a child was actually killed.
///
/// Called from the hygiene thread's existing ten-minute tick (`lib.rs`), so the
/// policy costs no timer of its own — which does mean a child outlives its last
/// use by 10–20 minutes rather than exactly 10. The next take spawns a fresh
/// one through the ordinary YV75 path: the slot is empty and the state is
/// `NotInstalled`, which is the same shape as "never spawned this session".
pub fn sweep_idle_sidecar() -> bool {
    pool().sweep_idle()
}

/// How the sidecar answered the handshake, as the reader thread saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Readiness {
    /// No readiness line yet — still loading, or wedged.
    Cold,
    /// Ready, with the model resident.
    Ready,
    /// Up, but explicitly WITHOUT a model. That is a failure, not readiness:
    /// this child can never rewrite anything.
    NoModel,
}

impl Readiness {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Ready,
            2 => Self::NoModel,
            _ => Self::Cold,
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            Self::Cold => 0,
            Self::Ready => 1,
            Self::NoModel => 2,
        }
    }
}

/// How a sidecar process is started. Production builds the staged binary's
/// command; the tests build a stub process, so the whole state machine —
/// handshake, no-op while cold, one restart then failed — is exercised with
/// zero model bytes on disk.
type Launcher = Box<dyn Fn(&Path) -> Result<Command, PolishError> + Send + Sync>;

/// The production launcher: the staged `yap-polish` next to the app executable.
fn staged_command(model: &Path) -> Result<Command, PolishError> {
    let mut command = Command::new(sidecar_binary().ok_or(PolishError::Unavailable)?);
    command.arg("--model").arg(model);
    Ok(command)
}

/// The warm sidecar slot and the policy around it (YV75).
///
/// The policy is the whole point of this type, and all of it is fail-open
/// toward the rules text:
///
/// * **the handshake is never waited on from the dictation path.** A child that
///   has not announced readiness makes THIS take a no-op — the rules-formatted
///   text passes straight through — and the take costs a channel-free bool read
///   rather than a deadline. Before YV75 the readiness line went to a stderr the
///   parent nulled, so a cold sidecar was indistinguishable from a wedged one
///   and every take inside the load window burned its deadline for nothing.
/// * **the budget bounds lateness, not the take.** Past
///   [`SIDECAR_READY_BUDGET`] from spawn, a still-silent child is killed and the
///   stage is `failed(ready_timeout)`.
/// * **one restart per session.** A child that dies is respawned once; the
///   second death is `failed(died)` and no further process is launched.
/// * **it does not idle forever (YV81).** A child nobody has used for
///   [`SIDECAR_IDLE_UNLOAD`] is terminated by [`SidecarPool::sweep_idle`]; the
///   next take respawns it through the path above, unchanged.
struct SidecarPool {
    slot: Mutex<Option<Sidecar>>,
    /// The model the child in (or last in) the slot was launched for. Kept
    /// after that child is gone, so a deliberate model change can clear a
    /// sticky failure the OLD model earned.
    launched: Mutex<Option<PathBuf>>,
    status: Mutex<SidecarStatus>,
    /// Respawns spent this session — bounded by [`MAX_SIDECAR_RESTARTS`].
    restarts: AtomicU64,
    launch: Launcher,
    ready_budget: Duration,
    /// When the warm child was last asked for anything (YV81). Stamped on every
    /// `rewrite`, which is the only way in.
    last_used: Mutex<Instant>,
    /// How long unused before [`sweep_idle`](Self::sweep_idle) kills the child.
    idle_unload: Duration,
    /// The clock the idle window is measured on — injected so
    /// `polish_sidecar_unloads_after_idle` can jump ten minutes instead of
    /// sleeping through them.
    now: Clock,
}

/// The pool's clock. `Instant::now` in production; a test hands it a base
/// instant plus an offset it controls.
type Clock = Box<dyn Fn() -> Instant + Send + Sync>;

impl SidecarPool {
    fn new(launch: Launcher, ready_budget: Duration) -> Self {
        Self::with_clock(
            launch,
            ready_budget,
            SIDECAR_IDLE_UNLOAD,
            Box::new(Instant::now),
        )
    }

    fn with_clock(
        launch: Launcher,
        ready_budget: Duration,
        idle_unload: Duration,
        now: Clock,
    ) -> Self {
        let started = now();
        Self {
            slot: Mutex::new(None),
            launched: Mutex::new(None),
            status: Mutex::new(SidecarStatus::new(SidecarState::NotInstalled, None)),
            restarts: AtomicU64::new(0),
            launch,
            ready_budget,
            last_used: Mutex::new(started),
            idle_unload,
            now,
        }
    }

    /// Terminate a warm child that has gone [`idle_unload`](Self#structfield.idle_unload)
    /// without a take. `true` when a process was actually killed.
    ///
    /// Deliberately NOT a failure: the state goes back to `NotInstalled` (the
    /// same state as "nothing spawned yet"), the restart budget is untouched,
    /// and the next take walks the ordinary spawn path.
    fn sweep_idle(&self) -> bool {
        let mut held = self.slot.lock();
        if held.is_none() {
            return false;
        }
        let idle = (self.now)().saturating_duration_since(*self.last_used.lock());
        if idle < self.idle_unload {
            return false;
        }
        if let Some(child) = held.take() {
            child.kill();
        }
        self.set(SidecarState::NotInstalled, None);
        log::info!(
            "polish sidecar unloaded after {}s unused — it reloads on the next take",
            idle.as_secs()
        );
        true
    }

    fn status(&self) -> SidecarStatus {
        *self.status.lock()
    }

    fn set(&self, state: SidecarState, reason: Option<&'static str>) {
        *self.status.lock() = SidecarStatus::new(state, reason);
    }

    /// Give up on the sidecar for the rest of this app session.
    fn fail(&self, reason: &'static str) {
        log::warn!("polish sidecar failed: {reason}");
        self.set(SidecarState::Failed, Some(reason));
    }

    /// The child is gone. One respawn is allowed per session; past that the
    /// stage is failed with a reason instead of relaunching a process that
    /// keeps dying.
    fn died(&self) {
        if self.restarts.fetch_add(1, Ordering::Relaxed) >= MAX_SIDECAR_RESTARTS {
            self.fail("died");
        } else {
            log::warn!("polish sidecar died — one restart left this session");
            self.set(SidecarState::NotInstalled, None);
        }
    }

    /// One rewrite against the warm child, spawning it if needed. Every `Err`
    /// here means the same thing to the caller: keep the rules text.
    fn rewrite(&self, model: &Path, req: &PolishRequest) -> Result<String, PolishError> {
        let mut held = self.slot.lock();
        // YV81 — the take that keeps the child warm. Stamped before any of the
        // work below so a request that fails still counts as use: an idle
        // unload is for a sidecar nobody is dictating at, not for a bad take.
        *self.last_used.lock() = (self.now)();
        // A model change (Settings) invalidates the warm child — and it is a
        // deliberate act, not a failure, so the restart budget AND any sticky
        // failure start over. Keyed on the pool's own record rather than the
        // slot, so picking a new model also revives a stage that gave up on the
        // old one.
        if self.launched.lock().as_deref() != Some(model) {
            if let Some(stale) = held.take() {
                stale.kill();
            }
            *self.launched.lock() = Some(model.to_path_buf());
            self.restarts.store(0, Ordering::Relaxed);
            self.set(SidecarState::NotInstalled, None);
        }
        if held.is_none() {
            // Failure is sticky: past the restart budget we stop paying a
            // process launch per dictation and stay honestly failed.
            if self.status().state == SidecarState::Failed {
                return Err(PolishError::Unavailable);
            }
            match Sidecar::spawn(self.launch.as_ref(), model) {
                Ok(fresh) => {
                    log::info!("polish sidecar starting");
                    self.set(SidecarState::Starting, None);
                    *held = Some(fresh);
                }
                Err(e) => {
                    self.fail("spawn_failed");
                    return Err(e);
                }
            }
        }
        let sidecar = held.as_mut().expect("spawned above");
        match sidecar.readiness() {
            Readiness::Ready => self.set(SidecarState::Ready, None),
            Readiness::NoModel => {
                held.take().expect("borrowed above").kill();
                self.fail("not_loaded");
                return Err(PolishError::Unavailable);
            }
            // THE no-op that replaces the livelock: no wait, no deadline, no
            // lost text — the rules output is what pastes for this take.
            Readiness::Cold => {
                let overdue = sidecar.started.elapsed() > self.ready_budget;
                if overdue {
                    held.take().expect("borrowed above").kill();
                    self.fail("ready_timeout");
                } else {
                    log::info!("polish skipped: sidecar is still loading its model");
                    self.set(SidecarState::Starting, None);
                }
                return Err(PolishError::Unavailable);
            }
        }
        match sidecar.exchange(req) {
            Ok(text) => Ok(text),
            Err(e) => {
                // A missed deadline leaves a decode running and a half-written
                // line in the pipe: kill it rather than read that line into the
                // NEXT dictation (§2.1 — the hard kill is why this is a process).
                if let Some(dead) = held.take() {
                    dead.kill();
                }
                // Only a child that is GONE counts against the restart budget —
                // a late or refused answer is the protocol working as designed.
                if e == PolishError::Unavailable {
                    self.died();
                } else {
                    self.set(SidecarState::NotInstalled, None);
                }
                Err(e)
            }
        }
    }
}

/// The real client: the bundled `yap-polish` process, spawned on first use and
/// held warm (§2.3 — the model load is off the dictation path). Any failure
/// kills the child; the next take spawns a fresh one, subject to the pool's
/// restart budget.
pub struct SidecarClient<'a> {
    model: PathBuf,
    pool: &'a SidecarPool,
}

impl SidecarClient<'static> {
    pub fn new(model: PathBuf) -> Self {
        Self {
            model,
            pool: pool(),
        }
    }
}

impl<'a> SidecarClient<'a> {
    /// A client over a caller-owned pool — how the tests drive the real state
    /// machine against a stub process instead of the app-wide sidecar.
    #[cfg(test)]
    fn over(pool: &'a SidecarPool, model: PathBuf) -> Self {
        Self { model, pool }
    }
}

impl PolishClient for SidecarClient<'_> {
    fn rewrite(&self, req: &PolishRequest) -> Result<String, PolishError> {
        self.pool.rewrite(&self.model, req)
    }
}

/// One warm sidecar process plus the reader threads that turn its stdout into
/// lines the parent can wait on with a timeout, and its stderr into log records.
struct Sidecar {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    /// The handshake, as the stdout reader saw it (YV75).
    ready: Arc<AtomicU8>,
    /// When this process was launched — the clock the readiness budget runs on.
    started: Instant,
}

impl Sidecar {
    fn spawn(
        launch: &(dyn Fn(&Path) -> Result<Command, PolishError> + Send + Sync),
        model: &Path,
    ) -> Result<Self, PolishError> {
        let mut child = launch(model)?
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // YV75: this stream used to be discarded outright, which is also
            // where the child's readiness line went before the handshake moved
            // onto stdout. Piped and drained below — and it MUST be drained: a
            // stderr nobody reads eventually fills its pipe and blocks the
            // child mid-write.
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| PolishError::Unavailable)?;
        let stdin = child.stdin.take().ok_or(PolishError::Unavailable)?;
        let stdout = child.stdout.take().ok_or(PolishError::Unavailable)?;
        let stderr = child.stderr.take().ok_or(PolishError::Unavailable)?;
        let (tx, lines) = mpsc::channel();
        let ready = Arc::new(AtomicU8::new(Readiness::Cold.as_u8()));
        let handshake = Arc::clone(&ready);
        // Reading a pipe cannot be given a timeout, so the read lives in a
        // thread and the parent waits on the channel instead. The thread ends
        // when the child's stdout closes.
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                // The handshake rides the SAME stream as the responses — that
                // is what makes it a protocol message rather than a log line.
                if let Some(hello) = parse_ready(&line) {
                    log::info!(
                        "polish sidecar ready: version={} model_loaded={}",
                        hello.version,
                        hello.model_loaded
                    );
                    let state = if hello.model_loaded {
                        Readiness::Ready
                    } else {
                        Readiness::NoModel
                    };
                    handshake.store(state.as_u8(), Ordering::Release);
                    continue;
                }
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        // The child's diagnostics, into our rotating log at DEBUG. They are
        // diagnostics only — the sidecar never writes dictated text to stderr —
        // and each line is capped so a runaway child cannot flood the log.
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                log::debug!(
                    "yap-polish: {}",
                    line.chars().take(STDERR_LOG_CHARS).collect::<String>()
                );
            }
        });
        Ok(Self {
            child,
            stdin,
            lines,
            ready,
            started: Instant::now(),
        })
    }

    /// The handshake so far. A plain atomic read: the dictation path asks this
    /// question on every take and must never block on the answer.
    fn readiness(&self) -> Readiness {
        Readiness::from_u8(self.ready.load(Ordering::Acquire))
    }

    /// One request, one answer, inside `deadline_ms`.
    fn exchange(&mut self, req: &PolishRequest) -> Result<String, PolishError> {
        let line = serde_json::to_string(req).map_err(|_| PolishError::Protocol)?;
        writeln!(self.stdin, "{line}").map_err(|_| PolishError::Unavailable)?;
        self.stdin.flush().map_err(|_| PolishError::Unavailable)?;
        let deadline = Instant::now() + Duration::from_millis(req.deadline_ms.max(1));
        loop {
            let left = deadline
                .checked_duration_since(Instant::now())
                .ok_or(PolishError::Deadline)?;
            match self.lines.recv_timeout(left) {
                // A line for an older id answers a dictation this process
                // stopped waiting for; pasting it would put one take's text
                // into another's target.
                Ok(line) => {
                    if let Some(response) = parse_response_for(&line, req.id) {
                        return response.into_text().ok_or(PolishError::Protocol);
                    }
                }
                Err(RecvTimeoutError::Timeout) => return Err(PolishError::Deadline),
                Err(RecvTimeoutError::Disconnected) => return Err(PolishError::Unavailable),
            }
        }
    }

    fn kill(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The staged sidecar binary, next to the app executable — where Tauri puts an
/// `externalBin` in the bundle, and where a workspace build puts it in dev.
/// `None` (⇒ the stage stays off) when it is not there.
fn sidecar_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let path = exe.parent()?.join(SIDECAR_BIN);
    path.is_file().then_some(path)
}

/// The never-lose-text gate. Every test here is selected by the spec's
/// acceptance filters (`cargo test validate_polish_ polish_fallback_ signature_`),
/// so one command covers the validator, the fallback behaviour it exists for, and
/// the YV62 signature stage that sits on the far side of it.
#[cfg(test)]
mod polish_fallback_tests {
    use super::*;
    use crate::dictation::{join_with_context, run_cleanup, CleanupLevel, LeadCase};
    use crate::snippets::{append_signature, expand_snippets, SignatureMode, SnippetScope};
    use std::sync::atomic::AtomicUsize;

    /// The lead-case decision an already-written string carries — how the R5
    /// ordering assertion reads the pipeline's output.
    fn lead_case_of(text: &str) -> LeadCase {
        match text.chars().find(|c| !c.is_whitespace()) {
            Some(c) if c.is_uppercase() => LeadCase::Capitalize,
            Some(c) if c.is_lowercase() => LeadCase::Lowercase,
            _ => LeadCase::Leave,
        }
    }

    /// A take that survives the rules stage as prose — long enough to clear the
    /// `MIN_POLISH_WORDS` gate, with a filler for the backtrack stage to eat.
    const RAW: &str = "um so we shipped the build and emailed the client";

    fn no_dict(text: &str) -> String {
        text.to_string()
    }

    /// The stage ON: a model id is configured, so `build_request` shapes a real
    /// request. No file is ever touched — the client is injected.
    fn on() -> PolishConfig {
        PolishConfig {
            model: "qwen2.5-0.5b-instruct-q4_k_m".to_string(),
            deadline_ms: DEFAULT_POLISH_DEADLINE_MS,
            style: Style::Default,
        }
    }

    /// The rules-stage output — what a rejected or missing rewrite must leave
    /// behind, byte for byte.
    fn rules_text(raw: &str, mode: DictationMode) -> String {
        run_cleanup(
            raw,
            CleanupLevel::High,
            mode,
            Style::Default,
            no_dict,
            |_| None,
        )
    }

    /// The full pipeline with `client` wired into stage 4.
    fn polished(
        raw: &str,
        mode: DictationMode,
        cfg: &PolishConfig,
        client: &dyn PolishClient,
    ) -> String {
        run_cleanup(raw, CleanupLevel::High, mode, cfg.style, no_dict, |t| {
            polish_stage(t, mode, cfg, client)
        })
    }

    /// Sleeps past any deadline worth having, then answers perfectly — the
    /// answer must still be dropped.
    struct SleepingClient;
    impl PolishClient for SleepingClient {
        fn rewrite(&self, req: &PolishRequest) -> Result<String, PolishError> {
            std::thread::sleep(Duration::from_millis(60));
            Ok(req.text.clone())
        }
    }

    /// A client that dies mid-rewrite. The unwind must not reach the take.
    struct PanickingClient;
    impl PolishClient for PanickingClient {
        fn rewrite(&self, _req: &PolishRequest) -> Result<String, PolishError> {
            panic!("polish client blew up");
        }
    }

    /// Returns a fixed string, and counts how many times it was asked.
    struct ScriptedClient {
        answer: &'static str,
        calls: AtomicUsize,
    }
    impl ScriptedClient {
        fn new(answer: &'static str) -> Self {
            Self {
                answer,
                calls: AtomicUsize::new(0),
            }
        }
        fn calls(&self) -> usize {
            self.calls.load(Ordering::Relaxed)
        }
    }
    impl PolishClient for ScriptedClient {
        fn rewrite(&self, _req: &PolishRequest) -> Result<String, PolishError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(self.answer.to_string())
        }
    }

    // --- V1–V7: the validator -------------------------------------------

    #[test]
    fn validate_polish_rejects_empty() {
        let input = "we shipped the build and emailed the client";
        assert_eq!(validate_polish(input, ""), None);
        assert_eq!(validate_polish(input, "   \n\t "), None);
    }

    #[test]
    fn validate_polish_rejects_runaway_and_truncation() {
        let input = "we shipped the build and emailed the client this afternoon";
        // Runaway repetition — the classic small-model loop.
        let runaway = [input; 4].join(" ");
        assert_eq!(validate_polish(input, &runaway), None);
        // Truncation — half a rewrite is worse than the rules text.
        assert_eq!(validate_polish(input, "we shipped"), None);
        // Re-flowing the rules stage's list onto its own lines is NOT a length
        // change: markers and newlines are excluded from the compare.
        let list_in = "1. buy milk 2. buy eggs 3. buy bread";
        assert_eq!(
            validate_polish(list_in, "1. Buy milk\n2. Buy eggs\n3. Buy bread").as_deref(),
            Some("1. Buy milk\n2. Buy eggs\n3. Buy bread")
        );
    }

    #[test]
    fn validate_polish_rejects_dropped_content_words() {
        let input =
            "please review the pricing proposal and forward it to the finance department before friday";
        // Same shape, four content words gone — retention 0.55.
        assert_eq!(
            validate_polish(
                input,
                "please review the pricing proposal and forward it to them soon"
            ),
            None
        );
        // Keeping them is a clean rewrite, punctuation and casing aside.
        assert!(validate_polish(
            input,
            "Please review the pricing proposal and forward it to the finance department before Friday."
        )
        .is_some());
    }

    #[test]
    fn validate_polish_rejects_invented_digits() {
        // The spec's case: a time the speaker never said.
        assert_eq!(validate_polish("call me at 3", "call me at 3:30"), None);
        // The digits that WERE spoken survive a normal rewrite.
        assert_eq!(
            validate_polish("call me at 3", "Call me at 3.").as_deref(),
            Some("Call me at 3.")
        );
    }

    #[test]
    fn validate_polish_rejects_invented_email_or_url() {
        let input = "email jordan the notes when the build is green";
        assert_eq!(
            validate_polish(
                input,
                "Email jordan@drivia.consulting the notes when the build is green."
            ),
            None
        );
        assert_eq!(
            validate_polish(
                input,
                "Email jordan the notes at https://drivia.consulting when the build is green."
            ),
            None
        );
        // An address the speaker DID dictate is not invented.
        let dictated = "email jordan@drivia.consulting the notes when the build is green";
        assert!(validate_polish(
            dictated,
            "Email jordan@drivia.consulting the notes when the build is green."
        )
        .is_some());
    }

    #[test]
    fn validate_polish_rejects_template_leak_and_preamble() {
        let input = "we shipped the build and emailed the client";
        for leaked in [
            "We shipped the build and emailed the client.<|im_end|>",
            "<think>the user wants a rewrite</think>We shipped the build and emailed the client.",
            "```\nWe shipped the build and emailed the client.\n```",
        ] {
            assert_eq!(validate_polish(input, leaked), None, "leak: {leaked}");
        }
        for assistant in [
            "Sure, here is the rewritten text: we shipped the build and emailed the client.",
            "Here is the rewrite: we shipped the build and emailed the client.",
            "Certainly! We shipped the build and emailed the client.",
        ] {
            assert_eq!(
                validate_polish(input, assistant),
                None,
                "preamble: {assistant}"
            );
        }
    }

    #[test]
    fn validate_polish_rejects_script_drift() {
        // V7 — every content word survives and the length is in band, so the
        // ONLY thing wrong is the script the model bolted on.
        let input = "we shipped the build and emailed the client";
        assert_eq!(
            validate_polish(
                input,
                "We shipped the build and emailed the client。我们已经通知了客户并且发布了构建"
            ),
            None
        );
    }

    #[test]
    fn validate_polish_accepts_a_clean_rewrite() {
        // Grammar, punctuation and casing fixed; every fact intact.
        assert_eq!(
            validate_polish(
                "so we shipped the build and emailed the client",
                "  We shipped the build and emailed the client.  "
            )
            .as_deref(),
            Some("We shipped the build and emailed the client.")
        );
    }

    // --- Fail-closed wiring ----------------------------------------------

    #[test]
    fn polish_fallback_on_deadline_returns_rules_text() {
        let cfg = PolishConfig {
            deadline_ms: 5,
            ..on()
        };
        let client = SleepingClient;
        // Byte for byte the rules output — the late answer is dropped whole.
        assert_eq!(
            polished(RAW, DictationMode::Notes, &cfg, &client),
            rules_text(RAW, DictationMode::Notes)
        );
    }

    #[test]
    fn polish_fallback_on_client_panic_returns_rules_text() {
        // The panic message on stderr is the client's; the take survives it.
        let client = PanickingClient;
        assert_eq!(
            polished(RAW, DictationMode::Notes, &on(), &client),
            rules_text(RAW, DictationMode::Notes)
        );
    }

    #[test]
    fn polish_fallback_on_garbage_output_returns_rules_text() {
        let expected = rules_text(RAW, DictationMode::Notes);
        for garbage in [
            "",
            "   ",
            "Sure, here is your text!",
            "<think>hmm</think>",
            "we shipped",
        ] {
            let client = ScriptedClient::new(garbage);
            assert_eq!(
                polished(RAW, DictationMode::Notes, &on(), &client),
                expected,
                "garbage: {garbage:?}"
            );
            assert_eq!(client.calls(), 1, "the client is asked exactly once");
        }
    }

    #[test]
    fn polish_never_runs_in_code_mode() {
        // V8 — no request is even shaped for `Code`…
        assert!(
            build_request("print the value and return it", DictationMode::Code, &on()).is_none()
        );
        // …and a hand-built code request never reaches the client either.
        let client = ScriptedClient::new("print(value)");
        let req = PolishRequest::polish(
            1,
            mode_tag(DictationMode::Code),
            "default",
            64,
            1200,
            "print the value and return it".to_string(),
        );
        assert_eq!(
            polish_with("print the value and return it", req, &client),
            None
        );
        assert_eq!(client.calls(), 0, "code mode never reaches a model");
        // End to end: the pipeline in `Code` pastes the raw take.
        let raw = "print open paren value close paren period";
        assert_eq!(
            polished(raw, DictationMode::Code, &on(), &client),
            rules_text(raw, DictationMode::Code)
        );
        assert_eq!(client.calls(), 0);
    }

    #[test]
    fn polish_uses_a_clean_client_rewrite() {
        // The stage is not a no-op in disguise: a rewrite that clears the gate
        // IS what the pipeline returns.
        let client = ScriptedClient::new("We shipped the build and emailed the client.");
        assert_eq!(
            polished(RAW, DictationMode::Notes, &on(), &client),
            "We shipped the build and emailed the client."
        );
        assert_eq!(client.calls(), 1);
    }

    #[test]
    fn polish_is_off_until_a_model_is_configured() {
        // The default settings ship the stage OFF (`polish_model == ""`), so a
        // client is never built and the pipeline is exactly what it was.
        let cfg = PolishConfig::default();
        assert!(build_request(RAW, DictationMode::Notes, &cfg).is_none());
        let client = ScriptedClient::new("We shipped the build and emailed the client.");
        assert_eq!(
            polished(RAW, DictationMode::Notes, &cfg, &client),
            rules_text(RAW, DictationMode::Notes)
        );
        assert_eq!(client.calls(), 0);
        // Same for utterances outside the band the latency budget covers.
        assert!(build_request("ship it", DictationMode::Notes, &on()).is_none());
        let long = "word ".repeat(MAX_POLISH_WORDS + 1);
        assert!(build_request(&long, DictationMode::Notes, &on()).is_none());
    }

    #[test]
    fn polish_never_overrides_the_r5_lead_case() {
        // R5: the caret context owns the first letter. `lib.rs` joins AFTER the
        // polish stage, so a model that capitalises mid-sentence loses.
        let client = ScriptedClient::new("We shipped the build and emailed the client.");
        let text = polished(RAW, DictationMode::Notes, &on(), &client);
        assert_eq!(lead_case_of(&text), LeadCase::Capitalize);
        let joined = join_with_context(&text, Some("I think"));
        assert_eq!(joined, " we shipped the build and emailed the client.");
        // …and an empty field still gets the capital.
        assert_eq!(
            join_with_context(&text, Some("")),
            "We shipped the build and emailed the client."
        );
    }

    #[test]
    fn polish_request_carries_the_mode_and_tone_dial() {
        let cfg = PolishConfig {
            style: Style::Formal,
            ..on()
        };
        let req = build_request(RAW, DictationMode::Email, &cfg).expect("the stage is on");
        assert_eq!(req.mode, "email");
        assert_eq!(req.style, "formal");
        assert_eq!(req.deadline_ms, DEFAULT_POLISH_DEADLINE_MS);
        assert_eq!(req.max_out, max_out_for(RAW));
        // The cursor context is never sent anywhere, not even locally.
        assert_eq!(req.topic, None);
        // Ids are monotonic, so a late answer to an earlier take is detectable.
        let next = build_request(RAW, DictationMode::Email, &cfg).expect("the stage is on");
        assert!(next.id > req.id);
    }

    #[test]
    fn polish_rejections_are_counted_without_logging_text() {
        // Monotone rather than exact: the counter is process-wide and the other
        // tests in this binary run in parallel.
        let before = rejected_total();
        assert_eq!(validate_polish("we shipped the build today", ""), None);
        assert!(rejected_total() > before);
    }

    // --- The readiness handshake (YV75) -----------------------------------

    /// A stub sidecar: `/bin/sh` running `script`. The pool cannot tell it from
    /// the real binary — it only ever sees a `Command`, a stdin and a stdout —
    /// so the handshake, the cold no-op and the restart budget are all proven
    /// with no GGUF, no llama.cpp and no staged binary.
    fn stub(script: &'static str) -> Launcher {
        Box::new(move |_model: &Path| {
            let mut command = Command::new("/bin/sh");
            command.arg("-c").arg(script);
            Ok(command)
        })
    }

    /// The same, counting launches — how "one restart, then failed" is proven.
    fn counted_stub(script: &'static str, launches: Arc<AtomicUsize>) -> Launcher {
        Box::new(move |_model: &Path| {
            launches.fetch_add(1, Ordering::Relaxed);
            let mut command = Command::new("/bin/sh");
            command.arg("-c").arg(script);
            Ok(command)
        })
    }

    /// Announces readiness, then answers every request as id 7.
    const READY_STUB: &str = concat!(
        r#"printf '{"type":"ready","version":"stub","model_loaded":true}\n'"#,
        "\n",
        "while read -r line; do\n",
        r#"  printf '{"id":7,"ok":true,"text":"We shipped the build and emailed the client."}\n'"#,
        "\ndone\n"
    );

    /// Never announces anything and never exits — a model still loading.
    const COLD_STUB: &str = "sleep 5\n";

    /// Announces readiness and immediately dies, so the request that follows
    /// finds a process that is gone.
    const DYING_STUB: &str = r#"printf '{"type":"ready","version":"stub","model_loaded":true}\n'"#;

    /// A path that does not exist: the stubs ignore `--model`, and a real file
    /// would only prove the test is reading from disk when it must not.
    fn stub_model() -> PathBuf {
        PathBuf::from("/nonexistent/polish-model.gguf")
    }

    /// The request the stubs answer, with the id they hard-code.
    fn stub_request() -> PolishRequest {
        PolishRequest::polish(
            7,
            "notes",
            "default",
            max_out_for(RAW),
            DEFAULT_POLISH_DEADLINE_MS,
            RAW.to_string(),
        )
    }

    /// Poll `f` until it answers, then give up. A test waits for a handshake
    /// that crosses a process boundary; it must never HANG waiting for one.
    fn poll_until<T>(mut f: impl FnMut() -> Option<T>) -> Option<T> {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Some(answered) = f() {
                return Some(answered);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        None
    }

    #[test]
    fn polish_ready_handshake_roundtrip() {
        let pool = SidecarPool::new(stub(READY_STUB), Duration::from_secs(10));
        let model = stub_model();
        // Nothing asked for yet, so nothing is running.
        assert_eq!(
            pool.status(),
            SidecarStatus::new(SidecarState::NotInstalled, None)
        );
        // The first take spawns the child, finds it cold, and SAYS so rather
        // than waiting on a pipe that cannot answer yet.
        assert_eq!(
            pool.rewrite(&model, &stub_request()),
            Err(PolishError::Unavailable)
        );
        assert_eq!(
            pool.status(),
            SidecarStatus::new(SidecarState::Starting, None)
        );
        // Then the readiness line lands on stdout — the same stream the
        // responses ride — and requests are served.
        let answer = poll_until(|| pool.rewrite(&model, &stub_request()).ok());
        assert_eq!(
            answer.as_deref(),
            Some("We shipped the build and emailed the client.")
        );
        assert_eq!(pool.status(), SidecarStatus::new(SidecarState::Ready, None));
        pool.slot.lock().take().expect("a warm child").kill();
    }

    #[test]
    fn polish_not_ready_passes_rules_text_through_immediately() {
        let pool = SidecarPool::new(stub(COLD_STUB), Duration::from_secs(10));
        let client = SidecarClient::over(&pool, stub_model());
        let started = Instant::now();
        let out = polished(RAW, DictationMode::Notes, &on(), &client);
        let took = started.elapsed();
        // Byte for byte the rules output: a loading model never blocks a take
        // and never loses one.
        assert_eq!(out, rules_text(RAW, DictationMode::Notes));
        // And nothing was waited out — not the readiness budget, not even the
        // per-take deadline. This is the assertion the livelock would fail.
        assert!(
            took < Duration::from_millis(500),
            "a cold sidecar cost the take {took:?}"
        );
        assert_eq!(
            pool.status(),
            SidecarStatus::new(SidecarState::Starting, None)
        );
        pool.slot.lock().take().expect("a spawned child").kill();
    }

    #[test]
    fn polish_dead_sidecar_restarts_once_then_fails_closed() {
        let launches = Arc::new(AtomicUsize::new(0));
        let pool = SidecarPool::new(
            counted_stub(DYING_STUB, Arc::clone(&launches)),
            Duration::from_secs(10),
        );
        let model = stub_model();
        // Take after take against a child that dies on arrival. Bounded, so a
        // pool that never gave up would fail this test rather than hang it.
        let failed = poll_until(|| {
            let _ = pool.rewrite(&model, &stub_request());
            let status = pool.status();
            (status.state == SidecarState::Failed).then_some(status)
        });
        assert_eq!(
            failed,
            Some(SidecarStatus::new(SidecarState::Failed, Some("died")))
        );
        assert_eq!(
            launches.load(Ordering::Relaxed),
            2,
            "one spawn plus exactly one restart, per app session"
        );
        // Sticky: a later take neither launches a third process nor blocks…
        let started = Instant::now();
        assert_eq!(
            pool.rewrite(&model, &stub_request()),
            Err(PolishError::Unavailable)
        );
        assert!(started.elapsed() < Duration::from_millis(200));
        assert_eq!(launches.load(Ordering::Relaxed), 2);
        // …and Diagnostics is told why, not just that.
        assert_eq!(pool.status().reason, Some("died"));
        // Picking a different model in Settings is not the failure the budget
        // counted, so it starts the stage over instead of staying dead.
        let _ = pool.rewrite(Path::new("/nonexistent/other-model.gguf"), &stub_request());
        assert_eq!(launches.load(Ordering::Relaxed), 3);
        let leftover = pool.slot.lock().take();
        if let Some(child) = leftover {
            child.kill();
        }
    }

    /// YV81 — a warm sidecar is a resident process holding a GGUF; unused, it is
    /// pure standby drain. The window is driven by an INJECTED clock, so this
    /// test jumps ten minutes instead of sleeping through them.
    #[test]
    fn polish_sidecar_unloads_after_idle() {
        let base = Instant::now();
        let offset = Arc::new(AtomicU64::new(0));
        let hand = Arc::clone(&offset);
        let idle_unload = Duration::from_secs(10 * 60);
        let pool = SidecarPool::with_clock(
            stub(READY_STUB),
            Duration::from_secs(10),
            idle_unload,
            Box::new(move || base + Duration::from_secs(hand.load(Ordering::Relaxed))),
        );
        let model = stub_model();
        // A real take, answered by a warm child.
        let answer = poll_until(|| pool.rewrite(&model, &stub_request()).ok());
        assert_eq!(
            answer.as_deref(),
            Some("We shipped the build and emailed the client.")
        );
        assert!(pool.slot.lock().is_some(), "the child is warm");

        // A minute later it is still warm: the pause between two takes in one
        // writing session must never cost a model reload.
        offset.store(60, Ordering::Relaxed);
        assert!(!pool.sweep_idle());
        assert!(pool.slot.lock().is_some());
        assert_eq!(pool.status(), SidecarStatus::new(SidecarState::Ready, None));

        // Ten minutes unused — the process goes, and the RAM with it.
        offset.store(10 * 60, Ordering::Relaxed);
        assert!(pool.sweep_idle(), "an idle sidecar is terminated");
        assert!(pool.slot.lock().is_none(), "no child is left resident");
        // NOT a failure: `NotInstalled` is the state a fresh session starts in.
        assert_eq!(
            pool.status(),
            SidecarStatus::new(SidecarState::NotInstalled, None)
        );
        // Sweeping again is a no-op — nothing to kill, nothing to log.
        assert!(!pool.sweep_idle());

        // And the next take brings it back through the ORDINARY YV75 spawn +
        // handshake path (this is the "verify, do not duplicate" half): the
        // unload spent none of the restart budget.
        let again = poll_until(|| pool.rewrite(&model, &stub_request()).ok());
        assert_eq!(
            again.as_deref(),
            Some("We shipped the build and emailed the client.")
        );
        assert_eq!(pool.status(), SidecarStatus::new(SidecarState::Ready, None));
        pool.slot.lock().take().expect("a warm child").kill();
    }

    // --- The signature, on the far side of the model (YV62, R13) ----------

    /// The configured block. Multi-line, with an address in it — a signature is
    /// exactly the shape of thing a model is most tempted to invent.
    const SIGNATURE: &str = "Wilson Guenther\nwilson@drivia.consulting";

    /// An email take that closes on a sign-off cue, so R13's shape rule renders
    /// the `Thanks,` line `signature_mode = auto` keys off.
    const EMAIL_RAW: &str =
        "hey Jordan the numbers are attached and I will send the deck tomorrow thanks";

    /// The production ordering, exactly as `lib.rs` wires it: cleanup (with the
    /// model in stage 4) → the R5 caret join → snippets → the signature LAST.
    fn pasted(
        raw: &str,
        mode: DictationMode,
        client: &dyn PolishClient,
        signature: &str,
        sig_mode: SignatureMode,
    ) -> String {
        let cfg = on();
        let text = polished(raw, mode, &cfg, client);
        let text = join_with_context(&text, None);
        let text = expand_snippets(&text, &[], SnippetScope::Inline);
        append_signature(&text, signature, sig_mode, mode)
    }

    /// Rewrites the take the way a model does — different casing, different
    /// punctuation — and records what it was actually given.
    struct ManglingClient {
        seen: Mutex<String>,
    }
    impl ManglingClient {
        fn new() -> Self {
            Self {
                seen: Mutex::new(String::new()),
            }
        }
    }
    impl PolishClient for ManglingClient {
        fn rewrite(&self, req: &PolishRequest) -> Result<String, PolishError> {
            *self.seen.lock() = req.text.clone();
            Ok(req.text.to_lowercase())
        }
    }

    #[test]
    fn signature_auto_appends_exact_bytes_after_llm() {
        let client = ManglingClient::new();
        let out = pasted(
            EMAIL_RAW,
            DictationMode::Email,
            &client,
            SIGNATURE,
            SignatureMode::Auto,
        );
        // The model DID rewrite the take — this is not a fallback in disguise.
        let mangled = client.seen.lock().clone();
        assert!(!mangled.is_empty(), "the model was never asked");
        assert_ne!(out, rules_text(EMAIL_RAW, DictationMode::Email));
        // The model never even saw the signature, so it had nothing to mangle.
        assert!(!mangled.contains("Guenther") && !mangled.contains('@'));
        // And the block that landed is the configured string, byte for byte,
        // once, at the end — under the sign-off line R13's shape rule rendered.
        assert!(
            out.ends_with(&format!("\n{SIGNATURE}")),
            "signature is not the tail of {out:?}"
        );
        assert_eq!(out.matches(SIGNATURE).count(), 1);
        assert!(out.to_lowercase().contains("thanks,"));
    }

    #[test]
    fn signature_never_invented_when_mode_off() {
        // The model helpfully signs the mail itself — a copy of the block it was
        // never given. That is a correctness bug, and the gate is what catches
        // it: an address the speaker never said fails V5, so the rules text is
        // what pastes.
        const INVENTED: &str = concat!(
            "Hey Jordan,\n\nThe numbers are attached and I will send the deck tomorrow.\n\n",
            "Thanks,\nWilson Guenther\nwilson@drivia.consulting"
        );
        assert!(INVENTED.ends_with(SIGNATURE), "the invention is the block");
        let client = ScriptedClient::new(INVENTED);
        let before = rejected_total();
        let out = pasted(
            EMAIL_RAW,
            DictationMode::Email,
            &client,
            SIGNATURE,
            SignatureMode::Off,
        );
        assert_eq!(client.calls(), 1, "the model was asked exactly once");
        assert!(rejected_total() > before, "the invented block was accepted");
        assert_eq!(out, rules_text(EMAIL_RAW, DictationMode::Email));
        assert!(!out.contains('@') && !out.contains("Guenther"));
        // …and with the stage OFF the configured block is not appended either,
        // even though this take ends on exactly the sign-off line `auto` wants.
        assert!(crate::dictation::ends_with_signoff_line(&out));
        assert!(!out.contains(SIGNATURE));
    }
}
