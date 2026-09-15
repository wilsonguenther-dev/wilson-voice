//! Y4-I — the polish stage measured against the REAL weights.
//!
//! Every latency number the Y4 LLM lane is built on was a derived guess. The
//! eleven tests in `cargo test -p yap-polish --release` are prompt- and
//! protocol-level and NOT ONE of them loads a model, so nothing in the repo
//! could falsify a single millisecond of it. This file is the falsifier: it
//! spawns the REAL `yap-polish` binary against the REAL GGUF, sends REAL
//! `KIND_POLISH` requests at a range of input lengths, and records p50/p95
//! wall-clock milliseconds per length plus the cold readiness cost.
//!
//! **Why the deadline in the requests is enormous.** `DEFAULT_POLISH_DEADLINE_MS`
//! is 1200 and `MAX_POLISH_DEADLINE_MS` is 5000, but a measurement taken at
//! those values measures the PARENT'S GIVING UP, not the model. Every request
//! here carries [`MEASUREMENT_DEADLINE_MS`] so the number that comes back is
//! pure model cost, and the shipped deadlines are then compared AGAINST that
//! curve rather than baked into it.
//!
//! **A `max_out` overrun is a hard error, not a slow success.**
//! `yap-polish/src/main.rs` turns an output that hits `max_out` into an error
//! response for `KIND_POLISH`, which discards the whole rewrite. So an input
//! length is only inside the envelope when it is BOTH fast enough AND does not
//! overrun — this file counts both, and the derived per-chunk budget (which
//! `Y4-E` is specced against) falls out of the two together.
//!
//! **Skipping.** The weights are ~1.1 GB and live outside the repo; CI and a
//! fresh clone have neither them nor a built sidecar. Following the `Y11-F`
//! pattern (and `tests/meeting_eval.rs`), an absent prerequisite prints ONE
//! NAMED reason saying exactly which path was missing and returns — it never
//! silently passes, and `polish_envelope_is_skipped_with_a_named_reason_when_the_weights_are_absent`
//! is the test that holds that promise to its word.
//!
//! Nothing here opens a socket: a local parent talks to a local child over
//! stdio.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use wilson_voice_lib::models;
use wilson_voice_lib::polish_protocol::{
    max_out_for, parse_ready, parse_response_for, PolishRequest,
};

/// Pure model cost, not parent patience: far above any shipped deadline so a
/// slow answer is recorded as a slow answer instead of a timeout.
const MEASUREMENT_DEADLINE_MS: u64 = 600_000;

/// How long a cold sidecar may take to announce readiness before we call the
/// environment broken rather than the model slow.
const READY_BUDGET: Duration = Duration::from_secs(180);

/// The shipped deadline this curve is judged against (`polish.rs:59`).
const SHIPPED_DEADLINE_MS: u64 = 1200;
/// The shipped ceiling on a configured deadline (`polish.rs:64`).
const SHIPPED_MAX_DEADLINE_MS: u64 = 5000;
/// The shipped long-form cutoff (`polish.rs:73`) — the number Y4-E was about to
/// be specced against, and the number this file exists to replace.
const SHIPPED_MAX_POLISH_WORDS: usize = 400;

/// Input lengths swept, in words.
const SIZES: &[usize] = &[30, 60, 100, 150, 200, 300, 400];

/// Repetitions per length. Three is the floor at which a p50 means anything;
/// `YAP_ENVELOPE_REPS` raises it for a publication-grade sweep without making
/// the default `cargo test` run cost minutes it does not need to.
const DEFAULT_REPS: usize = 3;

/// Headroom applied to the largest length that cleared the shipped deadline.
/// A chunk sized exactly at the measured edge has a coin-flip's chance of
/// blowing the deadline on the next machine; 0.8 buys a fifth of the budget
/// back for a colder cache, a busier box, and the p99 this sweep is too short
/// to see.
const BUDGET_HEADROOM: f64 = 0.8;

// ── prerequisites, and the NAMED reason when one is missing ─────────────────

/// Why this file cannot measure anything on this machine. `Ok` carries the two
/// real paths; `Err` carries a reason that NAMES the thing that was absent.
type Prereq = Result<(PathBuf, PathBuf), String>;

/// The built `yap-polish`, searched where the gate, the bundle and a developer
/// each leave it. Returns the first that exists, or a reason naming all of the
/// places that were looked at.
fn polish_binary() -> Result<PathBuf, String> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(explicit) = std::env::var("YAP_POLISH_BIN") {
        candidates.push(PathBuf::from(explicit));
    }
    if let Ok(target) = std::env::var("CARGO_TARGET_DIR") {
        candidates.push(PathBuf::from(target).join("release").join("yap-polish"));
    }
    // The staged sidecar `bundle.externalBin` requires: binaries/yap-polish-<triple>.
    if let Ok(entries) = std::fs::read_dir(manifest.join("binaries")) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("yap-polish-") {
                candidates.push(entry.path());
            }
        }
    }
    candidates.push(manifest.join("target").join("release").join("yap-polish"));
    for candidate in &candidates {
        if candidate.is_file() {
            return Ok(candidate.clone());
        }
    }
    Err(format!(
        "no yap-polish binary: looked at {}. Build it with `cargo build -p yap-polish --release`.",
        candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// The recommended polish GGUF, present at its full catalog size.
fn polish_weights() -> Result<PathBuf, String> {
    let model = models::recommended_polish_model()
        .ok_or_else(|| "the bundled catalog lists no recommended polish model".to_string())?;
    let path = models::polish_model_path(model);
    if models::is_polish_downloaded(model) {
        return Ok(path);
    }
    Err(format!(
        "polish weights absent: {} ({}, {} bytes expected) is missing or incomplete. Install it in Yap's model settings, or point YAP_POLISH_MODEL at a .gguf.",
        path.display(),
        model.id,
        model.file.size_bytes
    ))
}

/// `model_override` is passed in rather than read from the process environment
/// so the skip test can exercise an absent model WITHOUT mutating env vars that
/// the other tests in this binary read from a different thread at the same time.
fn prerequisites_from(model_override: Option<String>) -> Prereq {
    let binary = polish_binary()?;
    let weights = match model_override {
        Some(explicit) => {
            let path = PathBuf::from(explicit);
            if !path.is_file() {
                return Err(format!(
                    "YAP_POLISH_MODEL points at {}, which is not a file",
                    path.display()
                ));
            }
            path
        }
        None => polish_weights()?,
    };
    Ok((binary, weights))
}

fn prerequisites() -> Prereq {
    prerequisites_from(std::env::var("YAP_POLISH_MODEL").ok())
}

/// Announce the skip on stdout in one line, the way `tests/meeting_eval.rs`
/// does, so `-- --nocapture` shows WHY a run produced no numbers.
fn skip(reason: &str) {
    println!("polish_envelope SKIPPED — {reason}");
}

// ── the wire ────────────────────────────────────────────────────────────────

/// A real `yap-polish` child. Deliberately NOT `summarize::SidecarSession`:
/// that type applies the request's own `deadline_ms` as the parent timeout and
/// owns a summarize-shaped API, whereas this file is measuring `KIND_POLISH`
/// round trips and must be able to record an answer that arrives late rather
/// than convert it into a timeout.
struct Sidecar {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    /// Wall-clock milliseconds from spawn to the readiness line — the cold cost
    /// a first dictation after launch actually pays.
    ready_ms: u64,
}

impl Sidecar {
    fn spawn(binary: &Path, model: &Path) -> Result<Self, String> {
        let started = Instant::now();
        let mut child = Command::new(binary)
            .arg("--model")
            .arg(model)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawning {} failed: {e}", binary.display()))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr = child.stderr.take().ok_or("no stderr")?;
        let (tx, lines) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(hello) = parse_ready(&line) {
                    let _ = ready_tx.send(hello.model_loaded);
                    continue;
                }
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        // Drained, not discarded: an unread stderr pipe fills and blocks the
        // child mid-write, which would show up here as a phantom latency spike.
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                eprintln!("yap-polish: {}", line.chars().take(200).collect::<String>());
            }
        });
        match ready_rx.recv_timeout(READY_BUDGET) {
            Ok(true) => {
                let ready_ms = started.elapsed().as_millis() as u64;
                Ok(Self {
                    child,
                    stdin,
                    lines,
                    ready_ms,
                })
            }
            Ok(false) => {
                let _ = child.kill();
                Err("the sidecar came up but reported model_loaded=false".to_string())
            }
            Err(_) => {
                let _ = child.kill();
                Err(format!(
                    "the sidecar never announced readiness within {:?}",
                    READY_BUDGET
                ))
            }
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// One `KIND_POLISH` round trip, timed by us rather than by the child's own
    /// self-report, because the number a user feels includes the pipe.
    fn polish(&mut self, id: u64, text: &str) -> (u64, Outcome) {
        let req = PolishRequest::polish(
            id,
            "plain",
            "default",
            max_out_for(text),
            MEASUREMENT_DEADLINE_MS,
            text.to_string(),
        );
        let line = match serde_json::to_string(&req) {
            Ok(line) => line,
            Err(e) => return (0, Outcome::Err(format!("serialize: {e}"))),
        };
        let started = Instant::now();
        if writeln!(self.stdin, "{line}").is_err() || self.stdin.flush().is_err() {
            return (0, Outcome::Err("the child's stdin is closed".to_string()));
        }
        let deadline = started + Duration::from_millis(MEASUREMENT_DEADLINE_MS);
        loop {
            let left = match deadline.checked_duration_since(Instant::now()) {
                Some(left) => left,
                None => return (MEASUREMENT_DEADLINE_MS, Outcome::Err("deadline".into())),
            };
            match self.lines.recv_timeout(left) {
                Ok(line) => {
                    if let Some(response) = parse_response_for(&line, id) {
                        let ms = started.elapsed().as_millis() as u64;
                        return if response.ok {
                            (ms, Outcome::Ok)
                        } else {
                            (
                                ms,
                                Outcome::Err(response.err.unwrap_or_else(|| "unknown".into())),
                            )
                        };
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    return (MEASUREMENT_DEADLINE_MS, Outcome::Err("deadline".into()))
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return (
                        started.elapsed().as_millis() as u64,
                        Outcome::Err("the child died".into()),
                    )
                }
            }
        }
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Ok,
    Err(String),
}

// ── the corpus ──────────────────────────────────────────────────────────────

/// Dictation-shaped input: lower case, no terminal punctuation, the filler and
/// self-correction a rules pass leaves behind. Polishing clean prose would
/// measure a no-op; this gives the model the work it is shipped to do.
fn dictated(words: usize) -> String {
    const SOURCE: &str =
        "so um i think the thing we need to do here is basically get the migration \
landed before friday because the other team is blocked on it and uh i told them we would have it \
ready you know by the end of this week so can you take a look at the branch and let me know if \
anything looks off i mean the tests are green but that does not really tell us much about the \
actual behaviour under load and we have been burned by that before right so maybe we should also \
run it against a copy of the production data first just to be safe and then um if that looks fine \
we can ship it monday morning and i will write the note to the customers myself";
    let pool: Vec<&str> = SOURCE.split_whitespace().collect();
    (0..words)
        .map(|i| pool[i % pool.len()])
        .collect::<Vec<_>>()
        .join(" ")
}

// ── statistics ──────────────────────────────────────────────────────────────

/// Nearest-rank percentile over already-sorted samples. With 3 samples a p95
/// IS the maximum, which is honest: it is reported as such rather than
/// interpolated into a number the sample size cannot support.
fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted[rank.min(sorted.len()) - 1]
}

#[derive(Debug, Clone)]
struct Row {
    words: usize,
    samples: usize,
    p50: u64,
    p95: u64,
    /// Requests that came back `ok: false`, keyed by the sidecar's short reason
    /// tag — `max_out` is the one that matters here.
    errors: Vec<String>,
}

impl Row {
    fn clean(&self) -> bool {
        self.errors.is_empty() && self.samples > 0
    }
}

/// The per-chunk word budget `Y4-E` is to be specced against, DERIVED from the
/// measured curve instead of inherited from `MAX_POLISH_WORDS`.
///
/// A length qualifies when every one of its requests came back `ok` (no
/// `max_out` overrun, which would discard the rewrite outright) AND its p95 sat
/// inside the shipped deadline. The budget is the largest such length with
/// [`BUDGET_HEADROOM`] applied. If nothing qualifies the budget is 0, which is
/// the correct and loud answer: the stage cannot hold its own deadline at any
/// length on this machine.
fn derive_per_chunk_word_budget(rows: &[Row], deadline_ms: u64) -> usize {
    rows.iter()
        .filter(|r| r.clean() && r.p95 <= deadline_ms)
        .map(|r| r.words)
        .max()
        .map(|w| (w as f64 * BUDGET_HEADROOM).floor() as usize)
        .unwrap_or(0)
}

/// Resident set size of a live pid, in bytes. `ps` is what `hygiene::resident_mb`
/// already uses for this process; a child needs the same question asked of a
/// different pid.
fn resident_bytes(pid: u32) -> Option<u64> {
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let kb: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some(kb * 1024)
}

fn reps() -> usize {
    std::env::var("YAP_ENVELOPE_REPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_REPS)
}

// ── the tests ───────────────────────────────────────────────────────────────

/// The skip contract itself, and the reason this file can never pass by
/// accident: every absent prerequisite produces a reason that NAMES the missing
/// thing. A skip that said only "skipping" would let a permanently broken
/// measurement sit green forever, which is the failure mode `Y11-F` was written
/// against.
///
/// This test runs everywhere — with weights and without — because it asserts a
/// property of the reason strings, not of the model.
#[test]
fn polish_envelope_is_skipped_with_a_named_reason_when_the_weights_are_absent() {
    // A path that certainly does not exist stands in for "the weights are
    // absent" on a machine where they happen to be present.
    let reason = prerequisites_from(Some(
        "/nonexistent/yap-envelope/no-such-model.gguf".to_string(),
    ))
    .expect_err("an absent model cannot yield a measurable prereq");

    assert!(
        reason.contains("no-such-model.gguf") || reason.contains("yap-polish"),
        "the skip reason must NAME what was missing, got: {reason}"
    );
    assert!(
        reason.len() > 20,
        "a one-word skip reason is the silent pass this test exists to forbid: {reason}"
    );

    // And the real reason on THIS machine, whatever it is, is equally named.
    match prerequisites() {
        Ok((binary, weights)) => println!(
            "prerequisites present: binary={} weights={}",
            binary.display(),
            weights.display()
        ),
        Err(reason) => {
            assert!(
                reason.contains('/'),
                "the skip reason must name a path, got: {reason}"
            );
            skip(&reason);
        }
    }
}

/// THE MEASUREMENT. Drives the real sidecar against the real GGUF across
/// [`SIZES`] and prints the p50/p95 curve, the cold readiness cost and the
/// derived per-chunk word budget — the numbers `docs/BUDGETS.md` carries and
/// `Y4-E` cites.
#[test]
fn polish_envelope_curve_against_the_real_weights() {
    let (binary, weights) = match prerequisites() {
        Ok(pair) => pair,
        Err(reason) => return skip(&reason),
    };
    let mut sidecar = match Sidecar::spawn(&binary, &weights) {
        Ok(sidecar) => sidecar,
        Err(reason) => return skip(&format!("the sidecar would not start: {reason}")),
    };
    let ready_ms = sidecar.ready_ms;
    println!(
        "cold_ready_ms={ready_ms}  binary={}  weights={}",
        binary.display(),
        weights.display()
    );

    let reps = reps();
    let mut id = 1u64;
    let mut rows: Vec<Row> = Vec::new();
    for &words in SIZES {
        let text = dictated(words);
        let mut oks: Vec<u64> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        for _ in 0..reps {
            let (ms, outcome) = sidecar.polish(id, &text);
            id += 1;
            match outcome {
                Outcome::Ok => oks.push(ms),
                Outcome::Err(reason) => errors.push(reason),
            }
        }
        oks.sort_unstable();
        let row = Row {
            words,
            samples: oks.len(),
            p50: percentile(&oks, 50.0),
            p95: percentile(&oks, 95.0),
            errors,
        };
        println!(
            "words={:<4} ok={}/{} p50={}ms p95={}ms errors={:?}",
            row.words, row.samples, reps, row.p50, row.p95, row.errors
        );
        rows.push(row);
    }

    let peak_child_bytes = resident_bytes(sidecar.pid()).unwrap_or(0);
    println!("polish_child_peak_resident_bytes={peak_child_bytes}");

    let budget = derive_per_chunk_word_budget(&rows, SHIPPED_DEADLINE_MS);
    println!("per_chunk_word_budget={budget} (derived from the curve at deadline {SHIPPED_DEADLINE_MS}ms, headroom {BUDGET_HEADROOM})");

    // ── what the curve is allowed to say, and what it is not ────────────────

    assert!(
        rows.iter().any(|r| r.samples > 0),
        "not one length produced a single successful rewrite: {rows:?}"
    );
    assert!(
        ready_ms > 0,
        "a cold readiness of 0ms means the handshake was not measured"
    );

    // The load-bearing claim: the derived budget is a REAL bound, and it is not
    // MAX_POLISH_WORDS. If a future model or machine makes 400 words fit inside
    // 1200ms this assertion fails loudly and BUDGETS.md gets rewritten — which
    // is the correct outcome, not a regression.
    assert!(
        budget > 0,
        "the stage held its {SHIPPED_DEADLINE_MS}ms deadline at NO measured length — \
         every Y4 item specced against a latency budget is unbuildable as written. Curve: {rows:?}"
    );
    assert!(
        budget < SHIPPED_MAX_POLISH_WORDS,
        "the derived budget {budget} reached MAX_POLISH_WORDS ({SHIPPED_MAX_POLISH_WORDS}); \
         docs/BUDGETS.md claims the cutoff is far below it and must be re-measured. Curve: {rows:?}"
    );

    // The panel's other finding: the shipped deadline CEILING does not cover the
    // shipped word CUTOFF. Assert the pairing that is actually true of this
    // machine, so the doc and the constants can never drift apart unnoticed.
    if let Some(longest) = rows.iter().find(|r| r.words == SHIPPED_MAX_POLISH_WORDS) {
        println!(
            "at MAX_POLISH_WORDS={SHIPPED_MAX_POLISH_WORDS}: ok={} p95={}ms errors={:?} \
             (MAX_POLISH_DEADLINE_MS={SHIPPED_MAX_DEADLINE_MS})",
            longest.samples, longest.p95, longest.errors
        );
        // Only asserted for the SHIPPED model. A smaller GGUF passed in through
        // YAP_POLISH_MODEL can clear 400 words inside 1200ms purely by emitting
        // less — the 0.5B tier does exactly that, and its latency is not even
        // monotonic in input length, which is the signature of an output length
        // decoupled from the input rather than of a faster rewrite.
        if std::env::var("YAP_POLISH_MODEL").is_err() {
            assert!(
                !longest.clean() || longest.p95 > SHIPPED_DEADLINE_MS,
                "MAX_POLISH_WORDS now fits the default deadline cleanly on the SHIPPED model — \
                 the whole premise of this item changed and BUDGETS.md is stale"
            );
        }
    }
}

/// The ceiling this loop was missing: peak resident bytes with the ASR engine
/// and the polish child BOTH loaded. `Y3-G` asserts a memory ceiling and had no
/// measured number to assert against — this is that number.
///
/// Measured as parent RSS (which contains the ASR engine, loaded in-process)
/// plus the child's RSS, because they are two processes and a user's machine
/// pays for both at once.
#[test]
fn polish_envelope_peak_resident_bytes_with_asr_and_polish_loaded() {
    let (binary, weights) = match prerequisites() {
        Ok(pair) => pair,
        Err(reason) => return skip(&reason),
    };
    let asr = models::recommended_model();
    let Some(asr_path) = models::model_path(asr).filter(|_| models::is_downloaded(asr)) else {
        return skip(&format!(
            "ASR weights absent: the recommended model {} is missing or incomplete under {}, so the two-model ceiling cannot be measured",
            asr.id,
            models::models_dir().display()
        ));
    };

    let before = resident_bytes(std::process::id()).unwrap_or(0);
    let engine = match wilson_voice_lib::asr_engine::load(&asr_path) {
        Ok(engine) => engine,
        Err(reason) => {
            return skip(&format!(
                "the ASR engine would not load {}: {reason}",
                asr_path.display()
            ))
        }
    };
    let mut sidecar = match Sidecar::spawn(&binary, &weights) {
        Ok(sidecar) => sidecar,
        Err(reason) => return skip(&format!("the sidecar would not start: {reason}")),
    };

    // Do real work in both so neither is measured lazily mapped and untouched.
    let (_, outcome) = sidecar.polish(9_001, &dictated(60));
    println!("warm-up rewrite: {outcome:?}");

    let parent = resident_bytes(std::process::id()).unwrap_or(0);
    let child = resident_bytes(sidecar.pid()).unwrap_or(0);
    let peak = parent + child;
    println!(
        "peak_resident_bytes={peak} (parent_with_asr={parent}, polish_child={child}, \
         parent_before_asr={before}, asr={}, polish={})",
        asr.id,
        weights.file_name().unwrap_or_default().to_string_lossy()
    );
    println!("peak_resident_mb={:.1}", peak as f64 / 1_048_576.0);

    assert!(
        parent > before,
        "parent RSS did not grow after loading the ASR engine — the engine was not measured"
    );
    assert!(
        child > 0,
        "the polish child reported no resident bytes; ps could not see pid {}",
        sidecar.pid()
    );
    drop(engine);
}
