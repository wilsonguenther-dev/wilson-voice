//! Y4-E — long-form dictation is polished by CHUNKING, not skipped by a cliff.
//!
//! Before this suite, `polish.rs` refused any take over 400 words outright, so
//! the longest and most valuable dictations got the least formatting. The cliff
//! is gone; what replaces it is a paragraph-first chunker whose per-chunk word
//! budget is the MEASURED one from `docs/BUDGETS.md` (§`per_chunk_word_budget`
//! = 160), a validator that runs per chunk, and an aggregate gate over the
//! assembled document.
//!
//! Every test here drives the injectable `PolishClient` seam — a client that
//! sleeps past the deadline, one that panics, one that returns garbage, one
//! that overruns its token budget — EXCEPT the last, which drives the real
//! sidecar and skips with a NAMED reason when the weights or the binary are
//! absent. Fakes cannot see a model that misses its deadline; only the real one
//! can, which is why both kinds are here.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use wilson_voice_lib::dictation::{DictationMode, Style};
use wilson_voice_lib::polish::{
    build_chunk_request, installed_model_id, plan_polish_chunks, plan_polish_chunks_with,
    polish_llm_outcome, polish_stage, polish_stage_chunked, sidecar_status, take_deadline_ms,
    validate_aggregate, PolishClient, PolishConfig, PolishError, PolishOutcome, SidecarState,
    MAX_POLISH_CHUNKS, POLISH_CHUNK_WORDS,
};
use wilson_voice_lib::polish_protocol::PolishRequest;

/// The stage ON, with the shipped per-request deadline.
fn on() -> PolishConfig {
    PolishConfig {
        model: "qwen2.5-1.5b-instruct-q4_k_m".to_string(),
        deadline_ms: 1200,
        style: Style::Default,
    }
}

/// A take of `words` words laid out as paragraphs of `per_para` sentences, with
/// real sentence punctuation so the chunker has boundaries to find. Every word
/// is distinct enough that retention and duplication are both measurable.
fn take_of(words: usize, per_para: usize) -> String {
    let mut out = String::new();
    let mut made = 0usize;
    let mut sentence = 0usize;
    while made < words {
        let n = 10.min(words - made);
        let body: Vec<String> = (0..n).map(|i| format!("word{}", made + i)).collect();
        out.push_str(&body.join(" "));
        out.push('.');
        made += n;
        sentence += 1;
        if made >= words {
            break;
        }
        if sentence % per_para == 0 {
            out.push_str("\n\n");
        } else {
            out.push(' ');
        }
    }
    out
}

/// Sum of a chunk plan's bodies and separators — the invariant every seam claim
/// rests on.
fn rejoin(chunks: &[wilson_voice_lib::polish::PolishChunk]) -> String {
    chunks
        .iter()
        .map(|c| format!("{}{}", c.sep, c.body))
        .collect()
}

/// Uppercases the first letter of each chunk it is given and appends nothing —
/// a plausible, in-band rewrite that always passes the validator.
struct CasingClient {
    calls: AtomicUsize,
}

impl CasingClient {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
        }
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

impl PolishClient for CasingClient {
    fn rewrite(&self, req: &PolishRequest) -> Result<String, PolishError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let mut chars = req.text.chars();
        let first = chars.next().map(|c| c.to_uppercase().to_string());
        Ok(format!("{}{}", first.unwrap_or_default(), chars.as_str()))
    }
}

/// A client that behaves for every chunk except one, where it does something
/// specific and destructive.
struct FaultyClient {
    at: usize,
    fault: Fault,
    calls: AtomicUsize,
}

#[derive(Clone, Copy)]
enum Fault {
    /// Returns an assistant preamble — V6, rejected by the validator.
    Garbage,
    /// Panics inside the client.
    Panic,
    /// The sidecar's `max_out` overrun, which for `KIND_POLISH` is a hard error.
    Overrun,
    /// Answers, but far too late for the deadline.
    Slow,
}

impl FaultyClient {
    fn new(at: usize, fault: Fault) -> Self {
        Self {
            at,
            fault,
            calls: AtomicUsize::new(0),
        }
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

impl PolishClient for FaultyClient {
    fn rewrite(&self, req: &PolishRequest) -> Result<String, PolishError> {
        let n = self.calls.fetch_add(1, Ordering::Relaxed);
        if n == self.at {
            match self.fault {
                Fault::Garbage => return Ok("Sure, here is your text!".to_string()),
                Fault::Panic => panic!("the polish client blew up on chunk {n}"),
                Fault::Overrun => return Err(PolishError::Overrun),
                Fault::Slow => std::thread::sleep(Duration::from_millis(req.deadline_ms + 40)),
            }
        }
        let mut chars = req.text.chars();
        let first = chars.next().map(|c| c.to_uppercase().to_string());
        Ok(format!("{}{}", first.unwrap_or_default(), chars.as_str()))
    }
}

/// Always unavailable — the shape of a dead or never-spawned sidecar.
struct DeadSidecar {
    calls: AtomicUsize,
}

impl PolishClient for DeadSidecar {
    fn rewrite(&self, _req: &PolishRequest) -> Result<String, PolishError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Err(PolishError::Unavailable)
    }
}

/// Answers each chunk just inside the per-chunk deadline. A whole-take budget
/// of one chunk's deadline would lose every chunk after the first.
struct PacedClient {
    per_call: Duration,
    calls: AtomicUsize,
}

impl PolishClient for PacedClient {
    fn rewrite(&self, req: &PolishRequest) -> Result<String, PolishError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        std::thread::sleep(self.per_call);
        let mut chars = req.text.chars();
        let first = chars.next().map(|c| c.to_uppercase().to_string());
        Ok(format!("{}{}", first.unwrap_or_default(), chars.as_str()))
    }
}

fn polishable(outcome: &PolishOutcome) -> usize {
    outcome.chunks.len()
}

#[test]
fn a_two_thousand_word_take_is_polished_not_skipped() {
    let take = take_of(2000, 4);
    assert!(take.split_whitespace().count() >= 2000);
    // The old behaviour: anything past 400 words was declined outright and no
    // request was ever shaped. The chunk gate still refuses an OVERSIZED single
    // request — that is the second lock — which is why the take has to be
    // chunked to be polished at all.
    assert!(
        build_chunk_request(&take, DictationMode::Notes, &on(), None).is_none(),
        "one request for the whole take is still refused; chunking is the path"
    );

    let client = CasingClient::new();
    let outcome = polish_stage_chunked(&take, DictationMode::Notes, &on(), &client);

    assert!(
        polishable(&outcome) >= 12,
        "a 2000-word take at a {POLISH_CHUNK_WORDS}-word budget must be many chunks, got {}",
        polishable(&outcome)
    );
    assert_eq!(
        outcome.accepted_chunks(),
        polishable(&outcome),
        "every chunk was accepted by this client: {:?}",
        outcome.chunks
    );
    assert_eq!(client.calls(), polishable(&outcome));
    assert!(outcome.aggregate_reason.is_none());
    assert_ne!(
        outcome.text, take,
        "the take was rewritten, not passed through"
    );
    // And the take is still all there.
    assert_eq!(
        outcome.text.split_whitespace().count(),
        take.split_whitespace().count()
    );
    // `polish_stage` — what the pipeline calls — returns it rather than `None`.
    let client = CasingClient::new();
    assert!(
        polish_stage(&take, DictationMode::Notes, &on(), &client).is_some(),
        "the pipeline entry point must not decline a long take any more"
    );
}

#[test]
fn one_rejected_chunk_keeps_only_that_chunk_raw() {
    let take = take_of(1200, 3);
    let plan = plan_polish_chunks(&take);
    let bodies: Vec<String> = plan
        .iter()
        .filter(|c| !c.body.is_empty())
        .map(|c| c.body.clone())
        .collect();
    assert!(bodies.len() > 3);

    let client = FaultyClient::new(2, Fault::Garbage);
    let outcome = polish_stage_chunked(&take, DictationMode::Notes, &on(), &client);

    assert_eq!(outcome.reason_for(2), Some("validator"));
    assert_eq!(
        outcome.accepted_chunks(),
        bodies.len() - 1,
        "exactly one chunk lost its rewrite: {:?}",
        outcome.chunks
    );
    // The rejected chunk's RULES text is present verbatim…
    assert!(
        outcome.text.contains(&bodies[2]),
        "the rejected chunk must keep its rules text byte for byte"
    );
    // …and the assistant preamble never reached the document.
    assert!(!outcome.text.contains("Sure, here is"));
    // …while its neighbours were still rewritten.
    assert!(outcome.chunks[1].accepted && outcome.chunks[3].accepted);
}

#[test]
fn aggregate_retention_floor_holds_across_chunks() {
    let take = take_of(1600, 4);
    let client = CasingClient::new();
    let outcome = polish_stage_chunked(&take, DictationMode::Notes, &on(), &client);

    // The assembled document clears the whole-take gate, not just the per-chunk
    // ones: `polish_stage_chunked` runs `validate_aggregate` before it hands the
    // rewrite back, and would have kept the rules text whole if it had not.
    assert!(outcome.aggregate_reason.is_none());
    assert_eq!(
        validate_aggregate(&take, &outcome.text).as_deref(),
        Some(outcome.text.as_str()),
        "the aggregate gate accepts the document it produced"
    );

    // The gate is not decorative: the failure it catches is an ASSEMBLY that
    // lost or duplicated a chunk, which no per-chunk pass can see. A document
    // missing its second half is under the retention floor and is refused.
    let half: String = outcome
        .text
        .chars()
        .take(outcome.text.chars().count() / 2)
        .collect();
    assert_eq!(
        validate_aggregate(&take, &half),
        None,
        "an assembly that dropped half the take must fail the aggregate gate"
    );
    // …and so is one that emitted the document three times over. (TWICE is
    // ratio 2.0 and inside `MAX_LENGTH_RATIO` 2.5 — the band is a band, not a
    // duplicate detector, and saying otherwise here would be a test that lies
    // about what the gate does. The seam test is what pins single-copy output.)
    let doubled = format!("{} {}", outcome.text, outcome.text);
    assert_eq!(
        validate_aggregate(&take, &doubled).as_deref(),
        Some(doubled.as_str()),
        "2.0x is inside the band, and the aggregate gate is only the band"
    );
    let tripled = format!("{} {} {}", outcome.text, outcome.text, outcome.text);
    assert_eq!(
        validate_aggregate(&take, &tripled),
        None,
        "3.0x is past MAX_LENGTH_RATIO and is refused"
    );
}

#[test]
fn seam_does_not_duplicate_or_drop_a_sentence() {
    for take in [
        take_of(900, 3),
        take_of(420, 1),
        "one short paragraph with several words in it.".to_string(),
        format!("{}\n\n{}", take_of(300, 2), take_of(300, 5)),
        format!("  {}  \n\n", take_of(500, 2)),
    ] {
        let plan = plan_polish_chunks(&take);
        // Byte-exact: the plan IS the take, separators included.
        assert_eq!(rejoin(&plan), take, "chunk plan must be byte-exact");
        for chunk in &plan {
            assert!(
                chunk.words() <= POLISH_CHUNK_WORDS,
                "chunk over budget: {} words",
                chunk.words()
            );
        }
        // No sentence appears twice, and none disappears.
        let client = CasingClient::new();
        let outcome = polish_stage_chunked(&take, DictationMode::Notes, &on(), &client);
        let before: Vec<&str> = take.split_whitespace().collect();
        let after: Vec<&str> = outcome.text.split_whitespace().collect();
        assert_eq!(
            before.len(),
            after.len(),
            "a seam changed the word count of {take:?}"
        );
        for (b, a) in before.iter().zip(after.iter()) {
            assert!(
                a.eq_ignore_ascii_case(b),
                "word {b:?} became {a:?} across a seam"
            );
        }
        // The join whitespace survived: every paragraph break is still there.
        assert_eq!(
            take.matches("\n\n").count(),
            outcome.text.matches("\n\n").count(),
            "a paragraph break was dropped at a seam"
        );
        // Mid-sentence capitalisation: a chunk that does NOT start a sentence
        // must not have its first letter capitalised by the seam itself. The
        // plan cuts at sentence ends wherever a sentence end exists, so every
        // chunk here begins a sentence.
        for chunk in plan.iter().filter(|c| !c.body.is_empty()).skip(1) {
            let prev_end = take
                .find(&chunk.body)
                .and_then(|at| take[..at].trim_end().chars().last());
            assert!(
                matches!(prev_end, Some('.') | Some('!') | Some('?') | None),
                "a chunk started mid-sentence after {prev_end:?}"
            );
        }
    }
}

#[test]
fn deadline_scales_with_chunk_count() {
    let short = take_of(80, 4);
    let long = take_of(2000, 4);
    let cfg = on();

    assert_eq!(
        take_deadline_ms(&short, &cfg),
        cfg.deadline_ms,
        "a one-chunk take keeps the single-pass deadline"
    );
    let chunks = plan_polish_chunks(&long)
        .iter()
        .filter(|c| !c.body.is_empty())
        .count() as u64;
    assert!(chunks >= 12);
    assert_eq!(
        take_deadline_ms(&long, &cfg),
        chunks * cfg.deadline_ms,
        "the whole-take deadline is one chunk's deadline per chunk"
    );
    assert!(
        take_deadline_ms(&long, &cfg) > cfg.deadline_ms * 10,
        "a long take is not given 1200 ms total"
    );

    // And it is real, not just arithmetic: a client that spends most of the
    // per-chunk deadline on EVERY chunk still gets every chunk accepted. Under
    // one shared 1200 ms budget only the first could have survived.
    //
    // The two numbers below are a RATIO, not a duration, and the ratio is what
    // the assertion rests on. `polish_attempt` measures the deadline on the
    // parent's wall clock around `client.rewrite`, so anything that delays a
    // sleeping thread — a loaded CI runner, a cold page, another lane's cargo
    // build — is charged to the chunk. PER_CALL is therefore a QUARTER of the
    // per-chunk deadline: a chunk only misses if it is descheduled for longer
    // than it slept, three times over. An earlier revision of this test used
    // 60 ms against 120 ms and went red on CI (run 34940670588: 4 of 6 chunks
    // reported `deadline`) while passing locally every time. Two-times headroom
    // is not headroom.
    const PER_CHUNK_MS: u64 = 1_000;
    const PER_CALL_MS: u64 = PER_CHUNK_MS / 4;
    let cfg = PolishConfig {
        deadline_ms: PER_CHUNK_MS,
        ..on()
    };
    let take = take_of(900, 3);
    let planned = plan_polish_chunks(&take)
        .iter()
        .filter(|c| !c.body.is_empty())
        .count();
    // The liveness assertion below needs the take to outlive ONE chunk's
    // deadline on honest work alone, so the plan must be wide enough that
    // `planned * PER_CALL_MS` clears `PER_CHUNK_MS` without relying on jitter.
    assert!(
        (planned as u64) * PER_CALL_MS > PER_CHUNK_MS,
        "a {planned}-chunk plan at {PER_CALL_MS} ms/chunk cannot outlive a {PER_CHUNK_MS} ms slice"
    );
    let client = PacedClient {
        per_call: Duration::from_millis(PER_CALL_MS),
        calls: AtomicUsize::new(0),
    };
    let started = std::time::Instant::now();
    let outcome = polish_stage_chunked(&take, DictationMode::Notes, &cfg, &client);
    let spent = started.elapsed();
    assert_eq!(
        outcome.accepted_chunks(),
        planned,
        "each chunk gets its own slice of the deadline: {:?}",
        outcome.chunks
    );
    assert_eq!(
        client.calls.load(Ordering::Relaxed),
        planned,
        "every chunk was actually asked; the pass was not short-circuited"
    );
    assert!(
        spent > Duration::from_millis(cfg.deadline_ms),
        "the take legitimately outlived a single chunk's deadline ({spent:?})"
    );
}

#[test]
fn a_panicking_client_on_chunk_seven_still_returns_the_whole_document() {
    let take = take_of(2000, 4);
    let plan: Vec<String> = plan_polish_chunks(&take)
        .into_iter()
        .filter(|c| !c.body.is_empty())
        .map(|c| c.body)
        .collect();
    assert!(
        plan.len() > 7,
        "need at least eight chunks, got {}",
        plan.len()
    );

    let client = FaultyClient::new(7, Fault::Panic);
    let outcome = polish_stage_chunked(&take, DictationMode::Notes, &on(), &client);

    assert_eq!(outcome.reason_for(7), Some("client_panic"));
    assert_eq!(outcome.accepted_chunks(), plan.len() - 1);
    assert_eq!(
        client.calls(),
        plan.len(),
        "the panic did not stop the take"
    );
    // Nothing was lost: every word of the input is still in the output, and
    // chunk seven is present as its rules text.
    assert_eq!(
        outcome.text.split_whitespace().count(),
        take.split_whitespace().count()
    );
    assert!(outcome.text.contains(&plan[7]));
}

#[test]
fn a_max_out_chunk_keeps_its_rules_text_and_says_so() {
    let take = take_of(1200, 3);
    let plan: Vec<String> = plan_polish_chunks(&take)
        .into_iter()
        .filter(|c| !c.body.is_empty())
        .map(|c| c.body)
        .collect();

    let client = FaultyClient::new(3, Fault::Overrun);
    let outcome = polish_stage_chunked(&take, DictationMode::Notes, &on(), &client);

    // The reason is recorded BY NAME — `max_out` is not folded into "protocol".
    assert_eq!(outcome.reason_for(3), Some("max_out"));
    assert_eq!(outcome.accepted_chunks(), plan.len() - 1);
    assert!(
        outcome.text.contains(&plan[3]),
        "an overrun keeps that chunk's rules text instead of discarding the take"
    );
    assert_eq!(
        client.calls(),
        plan.len(),
        "an overrun stops one chunk only"
    );
    // The wire string maps to the variant the parent reports.
    assert_eq!(PolishError::from_wire_err("max_out"), PolishError::Overrun);
    assert_eq!(PolishError::Overrun.reason(), "max_out");
}

#[test]
fn a_slow_chunk_loses_only_its_own_rewrite() {
    let take = take_of(900, 3);
    let plan_len = plan_polish_chunks(&take)
        .iter()
        .filter(|c| !c.body.is_empty())
        .count();
    let cfg = PolishConfig {
        deadline_ms: 100,
        ..on()
    };
    let client = FaultyClient::new(1, Fault::Slow);
    let outcome = polish_stage_chunked(&take, DictationMode::Notes, &cfg, &client);
    assert_eq!(outcome.reason_for(1), Some("deadline"));
    assert_eq!(outcome.accepted_chunks(), plan_len - 1);
}

#[test]
fn a_twelve_chunk_take_does_not_spend_twelve_sidecar_restarts() {
    // YV75's restart budget is PER SESSION and is NOT reset per chunk — an
    // unbounded respawn is exactly what it exists to prevent. Chunking cannot
    // multiply the spend because the first `Unavailable` stops the loop: the
    // remaining chunks are recorded without being asked.
    let take = take_of(2000, 4);
    let planned = plan_polish_chunks(&take)
        .iter()
        .filter(|c| !c.body.is_empty())
        .count();
    assert!(planned >= 12);
    let client = DeadSidecar {
        calls: AtomicUsize::new(0),
    };
    let outcome = polish_stage_chunked(&take, DictationMode::Notes, &on(), &client);
    assert_eq!(
        client.calls.load(Ordering::Relaxed),
        1,
        "a dead sidecar costs ONE round trip for the whole take, not one per chunk"
    );
    assert_eq!(outcome.accepted_chunks(), 0);
    assert_eq!(
        outcome.text, take,
        "never-lose-text: the rules text is intact"
    );
    assert!(outcome
        .chunks
        .iter()
        .all(|c| c.reason == Some("no_sidecar")));
}

#[test]
fn short_take_path_is_byte_identical() {
    // Under MIN_POLISH_WORDS: still no round trip at all.
    let client = CasingClient::new();
    let tiny = "ship it now";
    let outcome = polish_stage_chunked(tiny, DictationMode::Notes, &on(), &client);
    assert_eq!(client.calls(), 0, "a 3-word take costs no round trip");
    assert_eq!(outcome.text, tiny);
    assert_eq!(outcome.accepted_chunks(), 0);
    assert_eq!(
        polish_stage(tiny, DictationMode::Notes, &on(), &client),
        None
    );

    // Stage off: no client call, no change.
    let off = PolishConfig::default();
    let one = "we shipped the build and emailed the client about the delay.";
    let client = CasingClient::new();
    assert_eq!(polish_stage(one, DictationMode::Notes, &off, &client), None);
    assert_eq!(client.calls(), 0);

    // Code never reaches a model, at any length.
    let client = CasingClient::new();
    assert_eq!(
        polish_stage(&take_of(900, 3), DictationMode::Code, &on(), &client),
        None
    );
    assert_eq!(client.calls(), 0);

    // A take inside the budget is ONE chunk and exactly the single-pass answer:
    // one call, and the output is the validated rewrite, byte for byte.
    let client = CasingClient::new();
    let plan = plan_polish_chunks(one);
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].sep, "");
    assert_eq!(plan[0].body, one);
    let got = polish_stage(one, DictationMode::Notes, &on(), &client).expect("polished");
    assert_eq!(client.calls(), 1);
    assert_eq!(
        got,
        "We shipped the build and emailed the client about the delay."
    );
}

#[test]
fn the_chunk_ceiling_keeps_text_it_will_not_polish() {
    // Past MAX_POLISH_CHUNKS the remaining chunks are carried, not dropped.
    let take = take_of(400, 2);
    let plan = plan_polish_chunks_with(&take, 4);
    assert!(plan.len() > MAX_POLISH_CHUNKS);
    assert_eq!(rejoin(&plan), take);
}

#[test]
fn skipped_with_a_named_reason_when_the_weights_are_absent() {
    // The ONE test here that drives the real sidecar. Fakes cannot see a model
    // that misses its deadline or overruns its token budget on real content —
    // `docs/BUDGETS.md` measured both — so this is the only test in the suite
    // that can falsify the envelope the chunk budget was derived from.
    let Some(model) = installed_model_id() else {
        println!(
            "polish_long_form: SKIPPED — no polish model is installed \
             (`installed_model_id()` is None; download one from Settings → Polish, \
             e.g. qwen2.5-1.5b-instruct-q4_k_m, to run this test)"
        );
        return;
    };
    let Some(sidecar) = staged_sidecar() else {
        println!(
            "polish_long_form: SKIPPED — the `yap-polish` sidecar binary is not staged \
             next to the test binary (looked beside {:?} and in its two parent \
             directories; build it with `cargo build -p yap-polish --release`)",
            std::env::current_exe().ok()
        );
        return;
    };
    println!("polish_long_form: running against {model} via {sidecar:?}");

    let cfg = PolishConfig {
        model,
        // The per-request ceiling, not the per-request default: a cold child
        // pays a GGUF page-in on its first chunk (BUDGETS.md: 7 765 ms cold).
        deadline_ms: 5000,
        style: Style::Default,
    };
    // YV75: a COLD child answers `unavailable` rather than blocking a take, so
    // the first request is a warm-up, not a measurement. Wait for the handshake
    // before asking anything of the model.
    let warm = "we shipped the build and emailed the client about the delay.";
    let started = std::time::Instant::now();
    let mut status = sidecar_status();
    while status.state != SidecarState::Ready
        && status.state != SidecarState::Failed
        && started.elapsed() < Duration::from_secs(25)
    {
        let _ = polish_llm_outcome(warm, DictationMode::Notes, &cfg);
        std::thread::sleep(Duration::from_millis(250));
        status = sidecar_status();
    }
    if status.state != SidecarState::Ready {
        // The reason is the sidecar's own tag, not a guess: `spawn_failed`,
        // `ready_timeout`, `not_loaded`, `no_binary`, `died`. The binary named
        // is whatever sits next to the TEST executable, which in a dev tree is
        // the workspace debug build rather than the staged release one.
        println!(
            "polish_long_form: SKIPPED — the `yap-polish` child never reached `ready` \
             (state={:?}, reason={:?}, waited {:?}, binary {sidecar:?}). This test needs a \
             sidecar that actually starts: `cargo build -p yap-polish --release` and stage \
             it beside the test executable to run it.",
            status.state,
            status.reason,
            started.elapsed()
        );
        return;
    }

    let take = take_of(2000, 4);
    let outcome = polish_llm_outcome(&take, DictationMode::Notes, &cfg)
        .expect("a model is installed, so the stage exists");
    println!(
        "polish_long_form: {} chunk(s), {} accepted, reasons={:?}",
        outcome.chunks.len(),
        outcome.accepted_chunks(),
        outcome
            .chunks
            .iter()
            .filter(|c| !c.accepted)
            .map(|c| (c.index, c.reason))
            .collect::<Vec<_>>()
    );
    // Never-lose-text holds against the REAL model, whatever it returned.
    assert!(!outcome.text.trim().is_empty());
    assert!(
        outcome.chunks.len() >= 12,
        "a 2000-word take must be chunked, not refused"
    );
    assert!(
        outcome.accepted_chunks() >= 1,
        "no chunk of a 2000-word take survived the real model: {:?}",
        outcome.chunks
    );
}

/// The staged `yap-polish` binary, looked for exactly where the app looks: next
/// to the running executable, and in its parent directories (cargo puts test
/// binaries in `target/<profile>/deps`).
fn staged_sidecar() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?.to_path_buf();
    for _ in 0..3 {
        let candidate = dir.join("yap-polish");
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = dir.parent()?.to_path_buf();
    }
    None
}
