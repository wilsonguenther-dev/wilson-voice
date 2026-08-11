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
use wilson_voice_lib::transcription::{CancelHandle, Transcriber, TranscriptionManager};

#[path = "support/meeting.rs"]
mod support;
use support::{expected_words, ramp_audio, window_bounds, RampDecoder};

/// A meeting chunk decode is SLOW (400 ms — a 30 s window on Metal is ~1 s, so
/// this is the same shape at test speed); a dictation is fast (50 ms).
const CHUNK_DECODE: Duration = Duration::from_millis(400);
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
