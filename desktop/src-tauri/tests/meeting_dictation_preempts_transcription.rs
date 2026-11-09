//! YV93 — a dictation issued while a meeting is being transcribed completes in
//! under two seconds.
//!
//! Plan finding #2b: there is exactly ONE warm engine (`transcription.rs`
//! answers a second concurrent caller with `Err("no ASR model is loaded")`), so
//! post-processing a long meeting would hard-fail the sub-second dictation path
//! for the length of the meeting. The contract this file holds the code to:
//!
//! * meeting ASR yields the engine to a dictation **at the next chunk
//!   boundary** — never mid-chunk, because a cancelled decode is wasted Metal
//!   time and, per YV70, an abandoned engine;
//! * the dictation completes in **< 2 s** (the plan's own number);
//! * the meeting then **resumes and loses nothing** — a preemption that
//!   dropped the chunk it was in would pass a naive timing assertion.
//!
//! It runs against the REAL `TranscriptionManager` — the real lease, the real
//! slot, the real timeout — with only the native engine stubbed, because the
//! contention being tested is the manager's, not the model's.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use wilson_voice_lib::asr_engine::TimedTranscript;
use wilson_voice_lib::meeting_asr::{
    ChunkConfig, JsonProgressStore, MeetingAsr, MeetingAsrConfig, WarmEngineChunkAsr,
};
use wilson_voice_lib::transcription::{
    CancelHandle, Transcriber, TranscriptionManager, ENGINE_HANDBACK_WAIT,
    MAX_SANCTIONED_CHUNK_DECODE_SECONDS,
};

#[path = "support/meeting.rs"]
mod support;
use support::{expected_words, ramp_audio, window_bounds, RampDecoder};

/// A meeting chunk decode is SLOW (400 ms — a 30 s window on Metal is ~1 s, so
/// this is the same shape at test speed); a dictation is fast (50 ms).
const CHUNK_DECODE: Duration = Duration::from_millis(400);
/// …and this is the same test with the one variable that turned out to matter:
/// a chunk decode LONGER than a dictation is willing to wait for the engine.
///
/// 6.5 s is not a hypothetical — `ChunkConfig::validate` sanctions a full-width
/// chunk at up to `MAX_SANCTIONED_CHUNK_DECODE_SECONDS` (60 s), and the
/// conditions that produce a slow decode (thermal throttle, a cold Metal
/// warm-up) are the conditions a long meeting creates. Against the first cut of
/// YV93 this exact number turned "the dictation waits one chunk" into
/// `panicked: the dictation goes through: "no ASR model is loaded"`: the take
/// was not delayed, it was destroyed.
const SLOW_CHUNK_DECODE: Duration = Duration::from_millis(6_500);
const DICTATION_DECODE: Duration = Duration::from_millis(50);
/// The plan's own acceptance number for a dictation issued mid-meeting.
const DICTATION_BUDGET: Duration = Duration::from_secs(2);

/// yap-1108-timing-open-m-budgets — the budget, stated in chunk boundaries
/// instead of wall time. A dictation may wait out the chunk in flight when it
/// arrives (the "at the next boundary, never mid-chunk" contract) plus at most
/// ONE chunk that began while its claim was already up: the meeting checks the
/// claim at the boundary and only then leases, so a claim raised in that gap
/// lands on a chunk that has already started. Anything more is the meeting
/// ignoring the claim at a boundary.
const CHUNKS_STARTED_UNDER_A_CLAIM_ALLOWED: usize = 1;
/// Why the count is the same property as the plan's number, not a weaker one:
/// the worst case it admits — the chunk in flight, one raced chunk, then the
/// dictation's own decode — fits inside the 2 s budget with room to spare, and
/// it is decided by counting, so a starved runner cannot fail it.
const _: () = assert!(
    CHUNK_DECODE.as_millis() * (1 + CHUNKS_STARTED_UNDER_A_CLAIM_ALLOWED as u128)
        + DICTATION_DECODE.as_millis()
        < DICTATION_BUDGET.as_millis()
);

/// A HANG guard, never a measurement: how long the tests wait for a meeting to
/// reach the point they need before calling it stuck.
const PROGRESS_DEADLINE: Duration = Duration::from_secs(60);

fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let started = Instant::now();
    while !cond() {
        assert!(
            started.elapsed() < PROGRESS_DEADLINE,
            "timed out after {PROGRESS_DEADLINE:?} waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// yap-1108-timing-open-m-budgets — what the stub engine saw, for the
/// count-based preemption assertions.
#[derive(Default)]
struct Probe {
    /// Meeting chunk decodes that have BEGUN (a decode in flight is
    /// `started > chunks`).
    started: AtomicUsize,
    /// Chunk decodes that began while a dictation's interactive claim was
    /// already up — chunk boundaries the meeting passed without yielding.
    started_under_claim: AtomicUsize,
    /// The manager, so a decode can read the claim when it begins.
    /// `interactive_pending` is one atomic load: it takes no lock the
    /// manager could be holding around the decode.
    manager: OnceLock<TranscriptionManager>,
}

const MEETING_SECONDS: f64 = 600.0;

/// The stub engine. It tells the two callers apart the way the app does: a
/// dictation goes through `transcribe` (text only), a meeting chunk through
/// `transcribe_timed`.
struct StubEngine {
    chunks: Arc<AtomicUsize>,
    dictations: Arc<AtomicUsize>,
    probe: Arc<Probe>,
    /// The meeting half: one timestamped word per second of the window it was
    /// handed, read back off the ramp fixture.
    decoder: RampDecoder,
}

impl Transcriber for StubEngine {
    fn transcribe(
        &mut self,
        _samples: &[f32],
        _language: Option<&str>,
        _bias: Option<&str>,
    ) -> Result<String, String> {
        std::thread::sleep(DICTATION_DECODE);
        self.dictations.fetch_add(1, Ordering::AcqRel);
        Ok("the dictation went through".to_string())
    }

    fn transcribe_timed(
        &mut self,
        samples: &[f32],
        _language: Option<&str>,
        _bias: Option<&str>,
    ) -> Result<TimedTranscript, String> {
        let claimed = self
            .probe
            .manager
            .get()
            .is_some_and(|m| m.interactive_pending());
        if claimed {
            self.probe
                .started_under_claim
                .fetch_add(1, Ordering::AcqRel);
        }
        self.probe.started.fetch_add(1, Ordering::AcqRel);
        std::thread::sleep(CHUNK_DECODE);
        self.chunks.fetch_add(1, Ordering::AcqRel);
        let (start, end) = window_bounds(samples);
        Ok(self.decoder.transcript_for(start, end))
    }

    fn cancel_handle(&self) -> Option<CancelHandle> {
        None
    }
}

fn manager(
    chunks: Arc<AtomicUsize>,
    dictations: Arc<AtomicUsize>,
) -> (TranscriptionManager, PathBuf) {
    manager_probed(chunks, dictations, Arc::new(Probe::default()))
}

fn manager_probed(
    chunks: Arc<AtomicUsize>,
    dictations: Arc<AtomicUsize>,
    probe: Arc<Probe>,
) -> (TranscriptionManager, PathBuf) {
    let engine_probe = Arc::clone(&probe);
    // `load` insists the model file exists; any file will do, since the loader
    // below never opens it.
    let model_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let m = TranscriptionManager::with_loader(
        Arc::new(move |_p: &std::path::Path| {
            Ok(Box::new(StubEngine {
                chunks: chunks.clone(),
                dictations: dictations.clone(),
                probe: Arc::clone(&engine_probe),
                decoder: RampDecoder::new(true),
            }) as Box<dyn Transcriber>)
        }),
        Duration::from_secs(3600),
        Duration::from_secs(3600),
        Duration::from_secs(30),
    );
    // A test-only reference cycle (engine -> probe -> manager); the process
    // ends with the test binary.
    let _ = probe.manager.set(m.clone());
    (m, model_path)
}

#[test]
fn a_dictation_issued_mid_meeting_completes_in_under_two_seconds() {
    let chunks = Arc::new(AtomicUsize::new(0));
    let dictations = Arc::new(AtomicUsize::new(0));
    let probe = Arc::new(Probe::default());
    let (manager, model_path) =
        manager_probed(chunks.clone(), dictations.clone(), Arc::clone(&probe));
    manager.load("stub", &model_path).expect("stub loads");

    let dir = std::env::temp_dir().join(format!("yap-yv93-preempt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");

    let meeting_manager = manager.clone();
    let meeting_path = model_path.clone();
    let meeting_dir = dir.clone();
    let meeting = std::thread::spawn(move || {
        let audio = ramp_audio(MEETING_SECONDS);
        let asr = WarmEngineChunkAsr::new(
            meeting_manager.clone(),
            "stub",
            &meeting_path,
            Some("en".to_string()),
            None,
        );
        let mut store = JsonProgressStore::new(&meeting_dir);
        let never = || false;
        let mut job = MeetingAsr {
            audio: &audio,
            vad: None,
            asr: &asr,
            demand: &meeting_manager,
            store: &mut store,
            quit: &never,
            config: MeetingAsrConfig {
                chunk: ChunkConfig::default(),
                yield_poll: Duration::from_millis(2),
                ..MeetingAsrConfig::default()
            },
        };
        job.run("m-preempt").expect("the meeting transcribes")
    });

    // Let the meeting get properly under way — several chunks in, engine busy.
    // yap-1108-timing-open-m-budgets: waited on the chunk counters, not a
    // 50 ms sleep: two chunks done and a third decode already begun.
    wait_until("the meeting's third chunk decode to be in flight", || {
        chunks.load(Ordering::Acquire) >= 2 && probe.started.load(Ordering::Acquire) >= 3
    });

    let started = Instant::now();
    let dictated = manager
        .transcribe(vec![0.1f32; 16_000], None, None)
        .expect("the dictation goes through");
    let elapsed = started.elapsed();

    assert_eq!(dictated, "the dictation went through");
    // The budget, counted (replaces `elapsed < DICTATION_BUDGET`; see
    // CHUNKS_STARTED_UNDER_A_CLAIM_ALLOWED for why it is the same property).
    let under_claim = probe.started_under_claim.load(Ordering::Acquire);
    assert!(
        under_claim <= CHUNKS_STARTED_UNDER_A_CLAIM_ALLOWED,
        "the meeting began {under_claim} chunk decodes while the dictation's claim was up \
         (at most {CHUNKS_STARTED_UNDER_A_CLAIM_ALLOWED}) — it did not yield at the next \
         boundary, so the dictation queued behind the meeting (took {elapsed:?}, plan \
         budget {DICTATION_BUDGET:?})"
    );
    // It cannot have been instant either — it waited out the chunk that was in
    // flight, which is the "at the next boundary, not mid-chunk" half of the
    // contract.
    assert!(
        elapsed >= DICTATION_DECODE,
        "the dictation did not actually run: {elapsed:?}"
    );

    let out = meeting.join().expect("the meeting thread finishes");
    assert!(
        out.preempt_yields >= 1,
        "meeting ASR never stood down for the dictation"
    );
    assert_eq!(
        out.text,
        expected_words(MEETING_SECONDS as usize).join(" "),
        "the meeting lost or duplicated words around the preemption"
    );
    assert_eq!(out.chunks_failed, 0);
    assert_eq!(dictations.load(Ordering::Acquire), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The signal itself: raised while a dictation is waiting for or holding the
/// engine, and clear the moment it is done. Meeting ASR reads exactly this.
#[test]
fn the_interactive_claim_is_raised_for_a_dictation_and_dropped_after_it() {
    let chunks = Arc::new(AtomicUsize::new(0));
    let dictations = Arc::new(AtomicUsize::new(0));
    let (manager, model_path) = manager(chunks, dictations);
    manager.load("stub", &model_path).expect("stub loads");

    assert!(!manager.interactive_pending(), "idle to begin with");
    let m = manager.clone();
    let dictating = std::thread::spawn(move || m.transcribe(vec![0.2f32; 16_000], None, None));
    let mut saw_claim = false;
    for _ in 0..200 {
        if manager.interactive_pending() {
            saw_claim = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(saw_claim, "a dictation never raised the claim");
    dictating.join().expect("thread").expect("dictation");
    assert!(
        !manager.interactive_pending(),
        "the claim outlived the dictation — meeting ASR would yield forever"
    );
}

/// …and a meeting chunk does NOT raise it. A meeting that counted itself as a
/// waiter would stand down at every boundary, forever, for itself.
#[test]
fn a_meeting_chunk_does_not_claim_the_engine_interactively() {
    let chunks = Arc::new(AtomicUsize::new(0));
    let dictations = Arc::new(AtomicUsize::new(0));
    let (manager, model_path) = manager(chunks.clone(), dictations);
    manager.load("stub", &model_path).expect("stub loads");

    let m = manager.clone();
    let decoding =
        std::thread::spawn(move || m.transcribe_timed(vec![0.3f32; 16_000 * 5], None, None));
    // Watch the whole decode: the claim must never come up.
    let deadline = Instant::now() + CHUNK_DECODE;
    while Instant::now() < deadline {
        assert!(
            !manager.interactive_pending(),
            "a meeting chunk raised the dictation claim"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    decoding.join().expect("thread").expect("chunk decodes");
    assert_eq!(chunks.load(Ordering::Acquire), 1);
}

/// A stub whose chunk decode can be ENDED early, the way the real engine's YV70
/// cancel token ends a native run — and whose cancel flag is STICKY until
/// reset, like the real one, so a manager that forgot to reset it would wedge
/// here rather than in production.
struct CancellableStubEngine {
    chunks: Arc<AtomicUsize>,
    dictations: Arc<AtomicUsize>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    decoder: RampDecoder,
    /// yap-1108-timing-open-m-budgets — counters for the count-based checks.
    slow: Arc<SlowProbe>,
}

/// yap-1108-timing-open-m-budgets — what the cancellable stub saw.
#[derive(Default)]
struct SlowProbe {
    /// Chunk decode attempts begun (a cancelled attempt and its re-run both
    /// count). Attempt [`WEDGED_ATTEMPT`] is the one the dictation lands on.
    attempts: AtomicUsize,
    /// Chunk decodes ended by the cancel token.
    aborts: AtomicUsize,
    /// `chunks` / `aborts` as they stood when the dictation's own decode ran.
    chunks_at_dictation: AtomicUsize,
    aborts_at_dictation: AtomicUsize,
}

/// The attempt (0-based) the dictation is issued into: the meeting's SECOND
/// chunk. It ends ONLY when cancelled — or, as a hang guard, after the longest
/// decode the geometry sanctions, which the assertions then reject — so "the
/// dictation took the engine off it" is decided by the cancel, never by how
/// fast the runner ran a timed decode.
const WEDGED_ATTEMPT: usize = 1;

impl Transcriber for CancellableStubEngine {
    fn transcribe(
        &mut self,
        _samples: &[f32],
        _language: Option<&str>,
        _bias: Option<&str>,
    ) -> Result<String, String> {
        self.slow
            .chunks_at_dictation
            .store(self.chunks.load(Ordering::Acquire), Ordering::Release);
        self.slow
            .aborts_at_dictation
            .store(self.slow.aborts.load(Ordering::Acquire), Ordering::Release);
        std::thread::sleep(DICTATION_DECODE);
        self.dictations.fetch_add(1, Ordering::AcqRel);
        Ok("the dictation went through".to_string())
    }

    fn transcribe_timed(
        &mut self,
        samples: &[f32],
        _language: Option<&str>,
        _bias: Option<&str>,
    ) -> Result<TimedTranscript, String> {
        let attempt = self.slow.attempts.fetch_add(1, Ordering::AcqRel);
        let decode = if attempt == WEDGED_ATTEMPT {
            Duration::from_secs_f64(MAX_SANCTIONED_CHUNK_DECODE_SECONDS)
        } else {
            SLOW_CHUNK_DECODE
        };
        let deadline = Instant::now() + decode;
        while Instant::now() < deadline {
            if self.cancel.load(Ordering::Acquire) {
                self.slow.aborts.fetch_add(1, Ordering::AcqRel);
                return Err("aborted".to_string());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        self.chunks.fetch_add(1, Ordering::AcqRel);
        let (start, end) = window_bounds(samples);
        Ok(self.decoder.transcript_for(start, end))
    }

    fn cancel_handle(&self) -> Option<CancelHandle> {
        let flag = self.cancel.clone();
        Some(Arc::new(move || flag.store(true, Ordering::Release)))
    }

    fn reset_cancel(&mut self) {
        self.cancel.store(false, Ordering::Release);
    }
}

/// Three chunks — enough for a dictation to land in the middle of one and for
/// the meeting to carry on afterwards, short enough that the whole test is a
/// handful of slow decodes.
const SLOW_MEETING_SECONDS: f64 = 90.0;

/// The same acceptance criterion as the test above, run at the only chunk-decode
/// length that ever mattered: longer than the engine handback wait.
///
/// The contract is unchanged and the numbers are the plan's own — the dictation
/// completes in under two seconds and the meeting loses nothing — but the
/// mechanism has to be different, because at this decode length "wait for the
/// next chunk boundary" IS the failure. A dictation now takes the engine off the
/// chunk mid-decode; the chunk, which is re-decodable from disk, is the thing
/// thrown away and re-run.
#[test]
fn a_dictation_survives_a_chunk_decode_longer_than_the_engine_handback_wait() {
    assert!(
        SLOW_CHUNK_DECODE <= Duration::from_secs_f64(MAX_SANCTIONED_CHUNK_DECODE_SECONDS),
        "this test is only interesting if the geometry actually sanctions a decode \
         this long"
    );

    let chunks = Arc::new(AtomicUsize::new(0));
    let dictations = Arc::new(AtomicUsize::new(0));
    let slow = Arc::new(SlowProbe::default());
    let model_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let manager = {
        let chunks = chunks.clone();
        let dictations = dictations.clone();
        let slow = Arc::clone(&slow);
        TranscriptionManager::with_loader(
            Arc::new(move |_p: &std::path::Path| {
                Ok(Box::new(CancellableStubEngine {
                    chunks: chunks.clone(),
                    dictations: dictations.clone(),
                    cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                    decoder: RampDecoder::new(true),
                    slow: Arc::clone(&slow),
                }) as Box<dyn Transcriber>)
            }),
            Duration::from_secs(3600),
            Duration::from_secs(3600),
            Duration::from_secs(120),
        )
    };
    manager.load("stub", &model_path).expect("stub loads");

    let dir = std::env::temp_dir().join(format!("yap-yv93-preempt-slow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");

    let meeting_manager = manager.clone();
    let meeting_path = model_path.clone();
    let meeting_dir = dir.clone();
    let meeting = std::thread::spawn(move || {
        let audio = ramp_audio(SLOW_MEETING_SECONDS);
        let asr = WarmEngineChunkAsr::new(
            meeting_manager.clone(),
            "stub",
            &meeting_path,
            Some("en".to_string()),
            None,
        );
        let mut store = JsonProgressStore::new(&meeting_dir);
        let never = || false;
        let mut job = MeetingAsr {
            audio: &audio,
            vad: None,
            asr: &asr,
            demand: &meeting_manager,
            store: &mut store,
            quit: &never,
            config: MeetingAsrConfig {
                chunk: ChunkConfig::default(),
                yield_poll: Duration::from_millis(2),
                ..MeetingAsrConfig::default()
            },
        };
        job.run("m-preempt-slow").expect("the meeting transcribes")
    });

    // Into the SECOND chunk's decode — the engine is busy, and will stay busy
    // until something cancels it. yap-1108-timing-open-m-budgets: waited on
    // the attempt counter, not on a 200 ms sleep.
    wait_until("the meeting's second chunk decode to be in flight", || {
        chunks.load(Ordering::Acquire) >= 1
            && slow.attempts.load(Ordering::Acquire) > WEDGED_ATTEMPT
    });
    let chunks_when_issued = chunks.load(Ordering::Acquire);

    let started = Instant::now();
    let dictated = manager
        .transcribe(vec![0.1f32; 16_000], None, None)
        .expect("the dictation goes through");
    let elapsed = started.elapsed();

    assert_eq!(dictated, "the dictation went through");
    // The budget, counted (replaces `elapsed < DICTATION_BUDGET`, and is
    // stronger): when the dictation's decode ran, NO chunk had completed since
    // it was issued and the in-flight one had been cancelled. A dictation that
    // queued behind the chunk instead finds it completed (after up to
    // MAX_SANCTIONED_CHUNK_DECODE_SECONDS) and no abort — on any runner.
    let chunks_at_dictation = slow.chunks_at_dictation.load(Ordering::Acquire);
    let aborts_at_dictation = slow.aborts_at_dictation.load(Ordering::Acquire);
    assert_eq!(
        chunks_at_dictation, chunks_when_issued,
        "a chunk decode completed while the dictation waited (took {elapsed:?}, plan \
         budget {DICTATION_BUDGET:?}; chunk decode {SLOW_CHUNK_DECODE:?}, handback wait \
         {ENGINE_HANDBACK_WAIT:?}) — it queued behind the chunk"
    );
    assert!(
        aborts_at_dictation >= 1,
        "the dictation ran without the in-flight chunk ever being cancelled"
    );

    let out = meeting.join().expect("the meeting thread finishes");
    assert_eq!(
        out.text,
        expected_words(SLOW_MEETING_SECONDS as usize).join(" "),
        "the preempted chunk was not re-decoded — the meeting lost words"
    );
    assert_eq!(
        out.chunks_failed, 0,
        "a preempted chunk was written off as an ASR failure, which is a \
         permanent hole in the transcript"
    );
    assert!(
        out.preempted_decodes >= 1,
        "the dictation queued behind the chunk instead of taking the engine off it"
    );
    assert!(
        !out.interrupted,
        "the meeting stood down instead of finishing"
    );
    assert_eq!(dictations.load(Ordering::Acquire), 1);
    let _ = std::fs::remove_dir_all(&dir);
}
