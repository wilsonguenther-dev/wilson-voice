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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::dictation::{self, DictationMode, Style};
use crate::models;
use crate::polish_protocol::{max_out_for, parse_response_for, PolishRequest};

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
const MAX_NON_ASCII_DRIFT: f64 = 0.25;

/// Template / chat-scaffold markers that must never survive into the paste
/// (§2.5 V6).
const TEMPLATE_MARKERS: &[&str] = &["<|im_start|>", "<|im_end|>", "<think>", "```"];

/// Assistant openers (§2.5 V6). A typist does not announce itself; a rewrite
/// that starts this way answered the dictation instead of retyping it.
const ASSISTANT_PREAMBLES: &[&str] = &["sure,", "here is", "here's the", "i've ", "certainly"];

/// Filename of the bundled sidecar. Tauri strips the target triple from
/// `bundle.externalBin` when it stages the binary next to the app executable,
/// and a workspace `cargo build` puts it in the same directory in dev.
const SIDECAR_BIN: &str = "yap-polish";

/// Rejected rewrites since launch — `polish_rejected_total` (§2.5). A COUNT, and
/// the reason tag on the log line; neither string is ever logged.
static POLISH_REJECTED_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Monotonic request id. A response carrying an older one answers a dictation
/// the parent already gave up on and is discarded (`parse_response_for`).
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// The warm sidecar, held for the app's lifetime and re-spawned after any
/// failure. `None` until the first request with a model installed.
static SIDECAR: Mutex<Option<Sidecar>> = Mutex::new(None);

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
    Some(PolishRequest {
        id: NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed),
        mode: mode_tag(mode).to_string(),
        style: cfg.style.tag().to_string(),
        max_out: max_out_for(text),
        deadline_ms: cfg.deadline_ms,
        text: text.to_string(),
        // The pushed session topic, once there is one. NEVER the AX cursor
        // context — that steers decisions in-process and is not sent anywhere.
        topic: None,
    })
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
fn content_words(text: &str) -> Vec<String> {
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
fn digit_runs(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_digit())
        .filter(|run| !run.is_empty())
        .map(str::to_string)
        .collect()
}

/// Email addresses and URLs, case-folded and stripped of the punctuation a
/// sentence wraps them in.
fn contact_tokens(text: &str) -> Vec<String> {
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
fn non_ascii_ratio(text: &str) -> f64 {
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

/// The real client: the bundled `yap-polish` process, spawned on first use and
/// held warm (§2.3 — the model load is off the dictation path). Any failure
/// kills the child; the next take spawns a fresh one.
pub struct SidecarClient {
    model: PathBuf,
}

impl SidecarClient {
    pub fn new(model: PathBuf) -> Self {
        Self { model }
    }
}

impl PolishClient for SidecarClient {
    fn rewrite(&self, req: &PolishRequest) -> Result<String, PolishError> {
        let mut held = SIDECAR.lock();
        // A model change (Settings) invalidates the warm child.
        if held.as_ref().is_some_and(|s| s.model != self.model) {
            held.take().expect("checked as_ref above").kill();
        }
        if held.is_none() {
            *held = Some(Sidecar::spawn(&self.model)?);
        }
        let sidecar = held.as_mut().expect("spawned above");
        match sidecar.exchange(req) {
            Ok(text) => Ok(text),
            Err(e) => {
                // A missed deadline leaves a decode running and a half-written
                // line in the pipe: kill it rather than read that line into the
                // NEXT dictation (§2.1 — the hard kill is why this is a process).
                if let Some(dead) = held.take() {
                    dead.kill();
                }
                Err(e)
            }
        }
    }
}

/// One warm sidecar process plus the reader thread that turns its stdout into
/// lines the parent can wait on with a timeout.
struct Sidecar {
    model: PathBuf,
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

impl Sidecar {
    fn spawn(model: &Path) -> Result<Self, PolishError> {
        let binary = sidecar_binary().ok_or(PolishError::Unavailable)?;
        let mut child = Command::new(binary)
            .arg("--model")
            .arg(model)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // The child's diagnostics are its own; stdout is the protocol.
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| PolishError::Unavailable)?;
        let stdin = child.stdin.take().ok_or(PolishError::Unavailable)?;
        let stdout = child.stdout.take().ok_or(PolishError::Unavailable)?;
        let (tx, lines) = mpsc::channel();
        // Reading a pipe cannot be given a timeout, so the read lives in a
        // thread and the parent waits on the channel instead. The thread ends
        // when the child's stdout closes.
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            model: model.to_path_buf(),
            child,
            stdin,
            lines,
        })
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
        let req = PolishRequest {
            id: 1,
            mode: mode_tag(DictationMode::Code).to_string(),
            style: "default".to_string(),
            max_out: 64,
            deadline_ms: 1200,
            text: "print the value and return it".to_string(),
            topic: None,
        };
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
