//! yap24-NT2 (RESIDUAL) — the guards the shipped meeting pipeline (#218) did not
//! have: a live recording never counts toward `max_yield` (R1), a crash-loop
//! guard with a panic-contained worker and a delayed start (R2),
//! `waiting_for_model` (R3) and `asr_model` on the row (R4).
//!
//! Stub decoders behind the shipped `ChunkAsr` seam, real SQLite files, real
//! WAVs on disk, the real worker thread. No model, no microphone.

mod support;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use support::{open_db, temp_dir};
use wilson_voice_lib::asr_engine::{TimedKind, TimedSpan, TimedTranscript};
use wilson_voice_lib::db::Database;
use wilson_voice_lib::meeting_asr::{
    ChunkAsr, ChunkConfig, EngineDemand, JsonProgressStore, MeetingAsr, MeetingAsrConfig,
    MemoryWindows, NoDemand,
};
use wilson_voice_lib::meeting_control::reconcile_stranded_meetings;
use wilson_voice_lib::meeting_pipeline::{
    self, attempt_marker_path, guarded_job, read_attempt_marker, requeue_waiting_for_model,
    unprogressed_crashes, AsrUnavailable, AttemptMarker, MeetingEngine, MeetingProgressEvent,
    PipelineDeps, CRASH_LOOP_MESSAGE, MAX_UNPROGRESSED_ATTEMPTS, QUEUE_START_DELAY,
    WAITING_FOR_MODEL_MESSAGE,
};
use wilson_voice_lib::meetings::{MeetingState, NewMeetingSegment};
use wilson_voice_lib::transcription::TranscriptionManager;

const RATE: u32 = 16_000;
const SECONDS: f64 = 6.0;

fn write_wav(path: &Path) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).expect("create wav");
    for i in 0..(SECONDS * RATE as f64) as usize {
        let t = i as f32 / RATE as f32;
        let v = (t * 220.0 * std::f32::consts::TAU).sin() * 0.3;
        w.write_sample((v * i16::MAX as f32) as i16).unwrap();
    }
    w.finalize().unwrap();
}

/// Words every 0.5 s across the window, window-relative, like the engine.
fn words_for(samples: &[f32], tag: usize) -> TimedTranscript {
    let len = samples.len() as f64 / RATE as f64;
    let mut words = Vec::new();
    let mut t = 0.05;
    let mut k = 0;
    while t + 0.3 < len {
        words.push(TimedSpan {
            start_seconds: t,
            end_seconds: t + 0.3,
            text: format!("w{tag}x{k}"),
        });
        t += 0.5;
        k += 1;
    }
    let text = words
        .iter()
        .map(|w| w.text.clone())
        .collect::<Vec<_>>()
        .join(" ");
    TimedTranscript {
        text,
        kind: TimedKind::Word,
        segments: Vec::new(),
        words,
    }
}

struct StubAsr {
    calls: Arc<AtomicUsize>,
    panic_on_first: bool,
}

impl ChunkAsr for StubAsr {
    fn transcribe_timed(&self, samples: &[f32]) -> Result<TimedTranscript, String> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if self.panic_on_first && n == 0 {
            panic!("simulated decoder panic");
        }
        Ok(words_for(samples, n))
    }
}

fn small_windows() -> MeetingAsrConfig {
    MeetingAsrConfig {
        chunk: ChunkConfig {
            target_seconds: 2.0,
            min_seconds: 1.5,
            max_seconds: 2.5,
            overlap_seconds: 0.5,
            min_silence_seconds: 0.2,
        },
        yield_poll: Duration::from_millis(1),
        ..MeetingAsrConfig::default()
    }
}

type Factory = Arc<dyn Fn() -> Result<MeetingEngine, AsrUnavailable> + Send + Sync>;

fn stub_factory(calls: Arc<AtomicUsize>, model: &'static str) -> Factory {
    Arc::new(move || {
        Ok(MeetingEngine::new(
            model,
            Box::new(StubAsr {
                calls: Arc::clone(&calls),
                panic_on_first: false,
            }),
        ))
    })
}

fn deps(db: Arc<Database>, dir: &Path, asr: Factory) -> PipelineDeps {
    PipelineDeps {
        db,
        progress_dir: dir.to_path_buf(),
        asr,
        demand: Arc::new(NoDemand),
        quit: Arc::new(|| false),
        sink: Arc::new(|_: &MeetingProgressEvent| {}),
        after_transcript: None,
        config: small_windows(),
        start_delay: Duration::ZERO,
    }
}

/// A stopped meeting with one WAV on disk, in `transcribing`.
fn stopped_meeting(db: &Database, dir: &Path, title: &str) -> (String, PathBuf) {
    let m = db.create_meeting(title, "manual").expect("create meeting");
    let wav = dir.join(format!("{}.wav", m.id));
    write_wav(&wav);
    db.finish_meeting(&m.id, SECONDS, Some(&wav))
        .expect("finish meeting");
    db.set_meeting_state(&m.id, MeetingState::Transcribing, None)
        .expect("transcribing");
    (m.id, wav)
}

fn state_of(db: &Database, id: &str) -> (String, Option<String>) {
    let m = db.get_meeting(id).unwrap().expect("row");
    (m.state, m.error)
}

// ── R1: a live recording never counts toward max_yield ──────────────────────

/// The decoder for R1: it records every call that happens while the dictation
/// path has the microphone open.
struct WatchingAsr {
    manager: TranscriptionManager,
    during_recording: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
    first_call: Arc<Mutex<Option<Instant>>>,
}

impl ChunkAsr for WatchingAsr {
    fn transcribe_timed(&self, samples: &[f32]) -> Result<TimedTranscript, String> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        self.first_call
            .lock()
            .unwrap()
            .get_or_insert_with(Instant::now);
        if self.manager.live_recording() {
            self.during_recording.fetch_add(1, Ordering::SeqCst);
        }
        Ok(words_for(samples, n))
    }
}

/// The 180 s hands-free take against the 120 s cap, scaled by 1000x and then
/// some: max_yield 60 ms, the take lasts 600 ms (10x the cap, where 180 s is
/// 1.5x). The demand is the SHIPPED one — `TranscriptionManager`, flipped by
/// `set_live_recording` exactly as lib.rs flips it with `AppState::recording`.
#[test]
fn a_long_live_dictation_never_sees_a_meeting_chunk_take_the_engine() {
    let manager = TranscriptionManager::new();
    let during = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let first_call = Arc::new(Mutex::new(None));
    let asr = WatchingAsr {
        manager: manager.clone(),
        during_recording: Arc::clone(&during),
        calls: Arc::clone(&calls),
        first_call: Arc::clone(&first_call),
    };
    let dir = temp_dir("nt2-r1");
    let audio = MemoryWindows::new(vec![0.05f32; (SECONDS * RATE as f64) as usize], RATE);
    let mut store = JsonProgressStore::new(&dir);
    let never = || false;
    let take = Duration::from_millis(600);

    manager.set_live_recording(true);
    assert!(
        EngineDemand::dictation_pending(&manager),
        "a live take is a demand"
    );
    let recorder = manager.clone();
    let started = Instant::now();
    let ends = std::thread::spawn(move || {
        std::thread::sleep(take);
        recorder.set_live_recording(false);
        Instant::now()
    });
    let out = MeetingAsr {
        audio: &audio,
        vad: None,
        asr: &asr,
        demand: &manager,
        store: &mut store,
        quit: &never,
        config: MeetingAsrConfig {
            max_yield: Duration::from_millis(60),
            ..small_windows()
        },
    }
    .run("nt2-r1")
    .expect("the meeting transcribes");
    let take_ended = ends.join().unwrap();

    assert_eq!(
        during.load(Ordering::SeqCst),
        0,
        "a meeting chunk took the engine during a live take"
    );
    let first = first_call.lock().unwrap().expect("chunks decoded");
    assert!(
        first >= take_ended,
        "the first meeting chunk started {:?} into a {:?} take",
        first - started,
        take
    );
    assert!(!out.interrupted);
    assert!(calls.load(Ordering::SeqCst) > 0);
}

/// The cap still exists for what it was for: a "busy, not recording" claim
/// that never clears is wedged, and meeting ASR takes the engine back.
struct StuckBusy;
impl EngineDemand for StuckBusy {
    fn dictation_pending(&self) -> bool {
        true
    }
}

#[test]
fn a_stuck_busy_not_recording_claim_is_still_capped_by_max_yield() {
    let calls = Arc::new(AtomicUsize::new(0));
    let asr = StubAsr {
        calls: Arc::clone(&calls),
        panic_on_first: false,
    };
    let dir = temp_dir("nt2-r1-stuck");
    let audio = MemoryWindows::new(vec![0.05f32; (SECONDS * RATE as f64) as usize], RATE);
    let mut store = JsonProgressStore::new(&dir);
    let never = || false;
    let started = Instant::now();
    let out = MeetingAsr {
        audio: &audio,
        vad: None,
        asr: &asr,
        demand: &StuckBusy,
        store: &mut store,
        quit: &never,
        config: MeetingAsrConfig {
            max_yield: Duration::from_millis(20),
            ..small_windows()
        },
    }
    .run("nt2-r1-stuck")
    .expect("the meeting transcribes");
    assert!(!out.interrupted);
    assert!(calls.load(Ordering::SeqCst) > 0);
    assert!(started.elapsed() < Duration::from_secs(10));
}

// ── R2: crash-loop guard, catch_unwind, delayed start ───────────────────────

#[test]
fn unprogressed_crash_count_is_pure_and_resets_on_progress() {
    assert_eq!(MAX_UNPROGRESSED_ATTEMPTS, 2);
    assert_eq!(unprogressed_crashes(None, 0.0), 0);
    let died = AttemptMarker {
        unprogressed_crashes: 0,
        progress_at_start: 12.0,
    };
    assert_eq!(unprogressed_crashes(Some(died), 12.0), 1);
    assert_eq!(
        unprogressed_crashes(Some(died), 40.0),
        0,
        "progress resets it"
    );
    let died_twice = AttemptMarker {
        unprogressed_crashes: 1,
        progress_at_start: 12.0,
    };
    assert_eq!(unprogressed_crashes(Some(died_twice), 12.0), 2);
}

/// Two launches that died mid-job without the ledger moving: the third start
/// fails the row with one honest sentence and never touches the engine. The
/// audio is kept.
#[test]
fn two_crashes_without_progress_fail_the_row_instead_of_retrying_forever() {
    let dir = temp_dir("nt2-r2-loop");
    let db = Arc::new(open_db(&dir));
    let (id, wav) = stopped_meeting(&db, &dir, "crash loop");
    let calls = Arc::new(AtomicUsize::new(0));
    let d = deps(
        Arc::clone(&db),
        &dir,
        stub_factory(Arc::clone(&calls), "stub"),
    );

    // Launch 1 died mid-job: the marker it wrote is still there.
    std::fs::write(
        attempt_marker_path(&dir, &id),
        serde_json::to_vec(&AttemptMarker {
            unprogressed_crashes: 0,
            progress_at_start: 0.0,
        })
        .unwrap(),
    )
    .unwrap();
    // Launch 2 starts: one unprogressed crash, so it runs — and (simulated)
    // dies too, leaving its own marker.
    assert_eq!(
        read_attempt_marker(&dir, &id).map(|m| m.unprogressed_crashes),
        Some(0)
    );
    std::fs::write(
        attempt_marker_path(&dir, &id),
        serde_json::to_vec(&AttemptMarker {
            unprogressed_crashes: 1,
            progress_at_start: 0.0,
        })
        .unwrap(),
    )
    .unwrap();

    // Launch 3.
    let state = guarded_job(&d, &id).expect("guarded job");
    assert_eq!(state, MeetingState::Failed);
    let (s, err) = state_of(&db, &id);
    assert_eq!(s, "failed");
    assert_eq!(err.as_deref(), Some(CRASH_LOOP_MESSAGE));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "no decode after a crash loop"
    );
    assert!(wav.exists(), "the audio is kept for a Retry");
    assert!(
        !attempt_marker_path(&dir, &id).exists(),
        "the marker is cleared"
    );
}

/// One unprogressed crash is retried, and a clean run clears the marker.
#[test]
fn one_crash_is_retried_and_a_clean_run_clears_the_marker() {
    let dir = temp_dir("nt2-r2-once");
    let db = Arc::new(open_db(&dir));
    let (id, _) = stopped_meeting(&db, &dir, "crashed once");
    let calls = Arc::new(AtomicUsize::new(0));
    let d = deps(
        Arc::clone(&db),
        &dir,
        stub_factory(Arc::clone(&calls), "stub"),
    );
    std::fs::write(
        attempt_marker_path(&dir, &id),
        serde_json::to_vec(&AttemptMarker {
            unprogressed_crashes: 0,
            progress_at_start: 0.0,
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(guarded_job(&d, &id).unwrap(), MeetingState::Complete);
    assert!(calls.load(Ordering::SeqCst) > 0);
    assert!(!attempt_marker_path(&dir, &id).exists());
    assert!(!db.list_meeting_segments(&id).unwrap().is_empty());
}

/// One panicking job fails ITS row; the FIFO worker stays alive for the next.
#[test]
fn a_panicking_job_fails_its_row_and_the_fifo_keeps_going() {
    let dir = temp_dir("nt2-r2-panic");
    let db = Arc::new(open_db(&dir));
    let (bad, _) = stopped_meeting(&db, &dir, "panics");
    let (good, _) = stopped_meeting(&db, &dir, "fine");
    let calls = Arc::new(AtomicUsize::new(0));
    let first = Arc::new(AtomicBool::new(true));
    let factory_calls = Arc::clone(&calls);
    let factory: Factory = Arc::new(move || {
        Ok(MeetingEngine::new(
            "stub",
            Box::new(StubAsr {
                calls: Arc::new(AtomicUsize::new(0)),
                panic_on_first: first.swap(false, Ordering::SeqCst),
            }) as Box<dyn ChunkAsr>,
        ))
        .inspect(|_| {
            factory_calls.fetch_add(1, Ordering::SeqCst);
        })
    });
    let pipeline = meeting_pipeline::spawn(deps(Arc::clone(&db), &dir, factory));
    pipeline.enqueue(&bad);
    pipeline.enqueue(&good);
    assert!(
        pipeline.wait_idle(Duration::from_secs(30)),
        "worker drained"
    );

    let (s, err) = state_of(&db, &bad);
    assert_eq!(s, "failed");
    assert!(err.unwrap_or_default().contains("internal error"));
    assert!(
        !attempt_marker_path(&dir, &bad).exists(),
        "a caught panic is not a process crash"
    );
    assert_eq!(state_of(&db, &good).0, "complete", "the worker survived");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn the_worker_waits_its_start_delay_before_the_first_job() {
    assert_eq!(QUEUE_START_DELAY, Duration::from_secs(30));
    let dir = temp_dir("nt2-r2-delay");
    let db = Arc::new(open_db(&dir));
    let (id, _) = stopped_meeting(&db, &dir, "delayed");
    let calls = Arc::new(AtomicUsize::new(0));
    let mut d = deps(
        Arc::clone(&db),
        &dir,
        stub_factory(Arc::clone(&calls), "stub"),
    );
    d.start_delay = Duration::from_millis(400);
    let started = Instant::now();
    let pipeline = meeting_pipeline::spawn(d);
    pipeline.enqueue(&id);
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "no decode inside the delay"
    );
    assert!(pipeline.wait_idle(Duration::from_secs(30)));
    assert!(started.elapsed() >= Duration::from_millis(400));
    assert_eq!(state_of(&db, &id).0, "complete");
}

// ── R3 + R4: waiting_for_model, asr_model ───────────────────────────────────

#[test]
fn a_missing_model_waits_keeps_the_audio_and_requeues_on_download() {
    let dir = temp_dir("nt2-r3");
    let db = Arc::new(open_db(&dir));
    let (id, wav) = stopped_meeting(&db, &dir, "no model yet");
    let present = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let (p, c) = (Arc::clone(&present), Arc::clone(&calls));
    let factory: Factory = Arc::new(move || {
        if !p.load(Ordering::SeqCst) {
            return Err(AsrUnavailable::ModelMissing {
                model_id: "pinned-parakeet".into(),
            });
        }
        Ok(MeetingEngine::new(
            "pinned-parakeet",
            Box::new(StubAsr {
                calls: Arc::clone(&c),
                panic_on_first: false,
            }),
        ))
    });
    let pipeline = meeting_pipeline::spawn(deps(Arc::clone(&db), &dir, factory));
    pipeline.enqueue(&id);
    assert!(pipeline.wait_idle(Duration::from_secs(30)));
    let (s, err) = state_of(&db, &id);
    assert_eq!(s, "waiting_for_model", "never a permanent failed row");
    assert_eq!(err.as_deref(), Some(WAITING_FOR_MODEL_MESSAGE));
    assert!(wav.exists(), "the audio is kept");
    assert_eq!(db.meeting_asr_model(&id).unwrap(), None);

    // Relaunch before the download: reconcile requeues it.
    let mut relaunch = Vec::new();
    let seen = Mutex::new(&mut relaunch);
    reconcile_stranded_meetings(&db, &|m| seen.lock().unwrap().push(m.to_string()));
    assert_eq!(relaunch, vec![id.clone()]);

    // The model-download-complete event.
    present.store(true, Ordering::SeqCst);
    let n = requeue_waiting_for_model(&db, &|m| pipeline.enqueue(m));
    assert_eq!(n, 1);
    assert!(pipeline.wait_idle(Duration::from_secs(30)));
    assert_eq!(state_of(&db, &id).0, "complete");
    assert!(!db.list_meeting_segments(&id).unwrap().is_empty());
    assert_eq!(
        db.meeting_asr_model(&id).unwrap().as_deref(),
        Some("pinned-parakeet"),
        "R4: the row records the model that transcribed it"
    );
    assert!(calls.load(Ordering::SeqCst) > 0);
}

#[test]
fn a_refusal_is_a_failed_row_with_its_sentence() {
    let dir = temp_dir("nt2-r3-refused");
    let db = Arc::new(open_db(&dir));
    let (id, _) = stopped_meeting(&db, &dir, "spanish");
    let factory: Factory = Arc::new(|| {
        Err(AsrUnavailable::Refused(
            "Meeting notes are English-only for now.".into(),
        ))
    });
    assert_eq!(
        guarded_job(&deps(Arc::clone(&db), &dir, factory), &id).unwrap(),
        MeetingState::Failed
    );
    let (s, err) = state_of(&db, &id);
    assert_eq!(s, "failed");
    assert_eq!(
        err.as_deref(),
        Some("Meeting notes are English-only for now.")
    );
}

/// R4: a transcript is one model's. Segments a different model left are
/// discarded, not spliced onto the pinned model's output.
#[test]
fn a_meeting_left_by_another_model_is_re_decoded_by_the_pinned_one() {
    let dir = temp_dir("nt2-r4");
    let db = Arc::new(open_db(&dir));
    let (id, _) = stopped_meeting(&db, &dir, "model switch");
    db.set_meeting_asr_model(&id, "whisper-tiny").unwrap();
    db.append_meeting_segments(
        &id,
        &[
            NewMeetingSegment::new(0.0, 1.0, "STALE WHISPER TEXT".to_string())
                .on_track(meeting_pipeline::MIC_TRACK),
        ],
    )
    .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let d = deps(
        Arc::clone(&db),
        &dir,
        stub_factory(Arc::clone(&calls), "parakeet"),
    );
    assert_eq!(guarded_job(&d, &id).unwrap(), MeetingState::Complete);
    let segments = db.list_meeting_segments(&id).unwrap();
    assert!(!segments.is_empty());
    assert!(
        segments.iter().all(|s| !s.text.contains("STALE")),
        "another model's text survived"
    );
    assert_eq!(
        db.meeting_asr_model(&id).unwrap().as_deref(),
        Some("parakeet")
    );
}

/// Source scan: the launch path pins the meeting model and spawns the worker
/// with the delay, and the demand is the shipped TranscriptionManager one.
#[test]
fn lib_rs_pins_the_model_spawns_the_pipeline_and_flips_live_recording() {
    let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("lib.rs");
    assert!(lib.contains("meeting_pipeline::spawn(deps)"));
    assert!(lib.contains("start_delay: meeting_pipeline::QUEUE_START_DELAY"));
    assert!(lib.contains("models::recommended_model()"));
    assert!(lib.contains("demand: Arc::new(state.transcription.clone())"));
    assert!(lib.contains("requeue_waiting_for_model"));
    assert_eq!(lib.matches("set_live_recording(").count(), 3);
}
