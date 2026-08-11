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
use std::sync::Arc;
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

const MEETING_SECONDS: f64 = 600.0;

/// The stub engine. It tells the two callers apart the way the app does: a
/// dictation goes through `transcribe` (text only), a meeting chunk through
/// `transcribe_timed`.
struct StubEngine {
    chunks: Arc<AtomicUsize>,
    dictations: Arc<AtomicUsize>,
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
        std::thread::sleep(CHUNK_DECODE);
        self.chunks.fetch_add(1, Ordering::AcqRel);
        let (start, end) = window_bounds(samples);
        Ok(self.decoder.transcript_for(start, end))
    }

    fn cancel_handle(&self) -> Option<CancelHandle> {
        None
    }
}

fn manager(chunks: Arc<AtomicUsize>, dictations: Arc<AtomicUsize>) -> (TranscriptionManager, PathBuf) {
    // `load` insists the model file exists; any file will do, since the loader
    // below never opens it.
    let model_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let m = TranscriptionManager::with_loader(
        Arc::new(move |_p: &std::path::Path| {
            Ok(Box::new(StubEngine {
                chunks: chunks.clone(),
                dictations: dictations.clone(),
                decoder: RampDecoder::new(true),
            }) as Box<dyn Transcriber>)
        }),
        Duration::from_secs(3600),
        Duration::from_secs(3600),
        Duration::from_secs(30),
    );
    (m, model_path)
}

#[test]
fn a_dictation_issued_mid_meeting_completes_in_under_two_seconds() {
    let chunks = Arc::new(AtomicUsize::new(0));
    let dictations = Arc::new(AtomicUsize::new(0));
    let (manager, model_path) = manager(chunks.clone(), dictations.clone());
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
    while chunks.load(Ordering::Acquire) < 2 {
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(50));

    let started = Instant::now();
    let dictated = manager
        .transcribe(vec![0.1f32; 16_000], None, None)
        .expect("the dictation goes through");
    let elapsed = started.elapsed();

    assert_eq!(dictated, "the dictation went through");
    assert!(
        elapsed < DICTATION_BUDGET,
        "a dictation issued mid-meeting took {elapsed:?}, budget {DICTATION_BUDGET:?}"
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
}

impl Transcriber for CancellableStubEngine {
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
        let deadline = Instant::now() + SLOW_CHUNK_DECODE;
        while Instant::now() < deadline {
            if self.cancel.load(Ordering::Acquire) {
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
    let model_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let manager = {
        let chunks = chunks.clone();
        let dictations = dictations.clone();
        TranscriptionManager::with_loader(
            Arc::new(move |_p: &std::path::Path| {
                Ok(Box::new(CancellableStubEngine {
                    chunks: chunks.clone(),
                    dictations: dictations.clone(),
                    cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                    decoder: RampDecoder::new(true),
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

    // Into the SECOND chunk's decode — the engine is busy, and will be for
    // seconds.
    while chunks.load(Ordering::Acquire) < 1 {
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(200));

    let started = Instant::now();
    let dictated = manager
        .transcribe(vec![0.1f32; 16_000], None, None)
        .expect("the dictation goes through");
    let elapsed = started.elapsed();

    assert_eq!(dictated, "the dictation went through");
    assert!(
        elapsed < DICTATION_BUDGET,
        "a dictation issued mid-chunk took {elapsed:?}, budget {DICTATION_BUDGET:?} \
         (chunk decode {SLOW_CHUNK_DECODE:?}, handback wait {ENGINE_HANDBACK_WAIT:?})"
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
