//! yap-meetings-fix (A) — Stop produces a transcript.
//!
//! Drives the REAL `MeetingController::stop` (not the worker directly) with a
//! fake capture engine that lands a 6 s two-track WAV pair, the REAL pipeline
//! worker installed exactly the way lib.rs installs it, a real SQLite file, and
//! a stub decoder behind the shipped `ChunkAsr` seam. Before this item, stop
//! left the row in `transcribing` forever and no segment was ever written.

mod support;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use support::{open_db, temp_dir};
use wilson_voice_lib::asr_engine::{TimedKind, TimedSpan, TimedTranscript};
use wilson_voice_lib::db::Database;
use wilson_voice_lib::meeting_asr::{ChunkAsr, ChunkConfig, MeetingAsrConfig, NoDemand};
use wilson_voice_lib::meeting_control::{
    install_capture_engine, ActiveCapture, CaptureEngine, CaptureOutcome, MeetingController,
    MeetingStatus, StatusSink,
};
use wilson_voice_lib::meeting_pipeline::{
    self, run_meeting_job, MeetingProgressEvent, PipelineDeps, MIC_TRACK, SYSTEM_TRACK,
};
use wilson_voice_lib::meetings::{MeetingKind, MeetingState};

const RATE: u32 = 16_000;
const SECONDS: f64 = 6.0;

fn write_wav(path: &Path, hz: f32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).expect("create wav");
    let n = (SECONDS * RATE as f64) as usize;
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let v = (t * hz * std::f32::consts::TAU).sin() * 0.3;
        w.write_sample((v * i16::MAX as f32) as i16).unwrap();
    }
    w.finalize().unwrap();
}

/// Words every 0.5 s across whatever window it is handed — window-relative,
/// exactly as the engine reports them.
struct StubAsr {
    calls: Arc<AtomicUsize>,
}

impl ChunkAsr for StubAsr {
    fn transcribe_timed(&self, samples: &[f32]) -> Result<TimedTranscript, String> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        let len = samples.len() as f64 / RATE as f64;
        let mut words = Vec::new();
        let mut t = 0.05;
        let mut k = 0;
        while t + 0.3 < len {
            words.push(TimedSpan {
                start_seconds: t,
                end_seconds: t + 0.3,
                text: format!("w{n}x{k}"),
            });
            t += 0.5;
            k += 1;
        }
        let text = words.iter().map(|w| w.text.clone()).collect::<Vec<_>>().join(" ");
        Ok(TimedTranscript {
            text,
            kind: TimedKind::Word,
            segments: Vec::new(),
            words,
        })
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

fn deps(
    db: Arc<Database>,
    dir: &Path,
    calls: Arc<AtomicUsize>,
    quit_after: Option<usize>,
    events: Arc<Mutex<Vec<MeetingProgressEvent>>>,
) -> PipelineDeps {
    let quit_calls = Arc::clone(&calls);
    PipelineDeps {
        db,
        progress_dir: dir.to_path_buf(),
        asr: Arc::new(move || {
            Ok(Box::new(StubAsr {
                calls: Arc::clone(&calls),
            }) as Box<dyn ChunkAsr>)
        }),
        demand: Arc::new(NoDemand),
        quit: Arc::new(move || match quit_after {
            Some(n) => quit_calls.load(Ordering::SeqCst) >= n,
            None => false,
        }),
        sink: Arc::new(move |e: &MeetingProgressEvent| events.lock().unwrap().push(e.clone())),
        after_transcript: None,
        config: small_windows(),
    }
}

// ── the fake capture engine: lands whatever WAV pair the test parked ────────

static PARKED: OnceLock<Mutex<Option<(PathBuf, PathBuf)>>> = OnceLock::new();

struct FakeEngine;
struct FakeCapture {
    wavs: (PathBuf, PathBuf),
}

impl CaptureEngine for FakeEngine {
    fn start(&self, _dir: &Path, _kind: MeetingKind) -> Result<Box<dyn ActiveCapture>, String> {
        let wavs = PARKED
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap()
            .clone()
            .ok_or("no wav parked")?;
        Ok(Box::new(FakeCapture { wavs }))
    }
}

impl ActiveCapture for FakeCapture {
    fn seconds(&self) -> f64 {
        SECONDS
    }
    fn stop(self: Box<Self>) -> Result<CaptureOutcome, String> {
        Ok(CaptureOutcome {
            wav_path: Some(self.wavs.0.clone()),
            sys_wav_path: Some(self.wavs.1.clone()),
            tap_rebuilds: None,
            track_rates: Vec::new(),
            seconds: SECONDS,
            note: None,
            partial: false,
        })
    }
}

#[test]
fn stop_hands_the_capture_to_the_pipeline_and_the_row_completes_with_both_tracks() {
    let dir = temp_dir("meetings-fix-wired");
    let db = Arc::new(open_db(&dir));
    let mic = dir.join("m.t0.wav");
    let sys = dir.join("m.t1.wav");
    write_wav(&mic, 220.0);
    write_wav(&sys, 330.0);
    *PARKED.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some((mic, sys));
    install_capture_engine(Arc::new(FakeEngine));

    let calls = Arc::new(AtomicUsize::new(0));
    let events = Arc::new(Mutex::new(Vec::new()));
    let pipeline = meeting_pipeline::spawn(deps(
        Arc::clone(&db),
        &dir,
        Arc::clone(&calls),
        None,
        Arc::clone(&events),
    ));
    assert!(meeting_pipeline::install(pipeline.clone()), "first install wins");

    let sink: StatusSink = Arc::new(|_: &MeetingStatus| {});
    let controller = MeetingController::new(Arc::clone(&db), sink);
    let id = controller.start(&dir, Some("wired".into())).expect("start");
    let stopped = controller.stop("test stop").expect("stop");
    assert_eq!(stopped.state, MeetingState::Transcribing);

    assert!(pipeline.wait_idle(Duration::from_secs(20)), "pipeline drained");
    let m = db.get_meeting(&id).unwrap().expect("row");
    assert_eq!(m.state, "complete", "row: {m:?}");
    let segs = db.list_meeting_segments(&id).unwrap();
    let t0 = segs.iter().filter(|s| s.track == MIC_TRACK).count();
    let t1 = segs.iter().filter(|s| s.track == SYSTEM_TRACK).count();
    println!("segments: track0={t0} track1={t1} decoder calls={}", calls.load(Ordering::SeqCst));
    assert!(t0 > 0 && t1 > 0, "segments on BOTH tracks: t0={t0} t1={t1}");
    assert!(
        (m.processed_through_seconds - m.duration_seconds).abs() < 1e-6,
        "processed_through {} == duration {}",
        m.processed_through_seconds,
        m.duration_seconds
    );
    let ev = events.lock().unwrap();
    let percents: Vec<u8> = ev.iter().map(|e| e.percent).collect();
    assert!(percents.windows(2).all(|w| w[0] <= w[1]), "monotonic: {percents:?}");
    assert_eq!(ev.last().map(|e| e.state.as_str()), Some("complete"));
    assert_eq!(ev.last().map(|e| e.percent), Some(100));
}

#[test]
fn a_second_run_resumes_from_processed_through_without_duplicating_segments() {
    let dir = temp_dir("meetings-fix-resume");
    let db = Arc::new(open_db(&dir));
    let mic = dir.join("r.t0.wav");
    let sys = dir.join("r.t1.wav");
    write_wav(&mic, 220.0);
    write_wav(&sys, 330.0);
    let m = db.create_meeting("resume", "manual").unwrap();
    db.finish_meeting(&m.id, SECONDS, Some(&mic)).unwrap();
    db.set_meeting_sys_wav_path(&m.id, Some(&sys)).unwrap();
    db.set_meeting_state(&m.id, MeetingState::Transcribing, None).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));

    // Run 1 is interrupted after two windows (a quit).
    let first = Arc::new(AtomicUsize::new(0));
    let d1 = deps(Arc::clone(&db), &dir, Arc::clone(&first), Some(2), Arc::clone(&events));
    let state = run_meeting_job(&d1, &m.id).unwrap();
    assert_eq!(state, MeetingState::Transcribing, "interrupted job stays queued");
    let after_first = db.list_meeting_segments(&m.id).unwrap().len();
    let progress_first = db.get_meeting(&m.id).unwrap().unwrap().processed_through_seconds;
    assert_eq!(first.load(Ordering::SeqCst), 2);
    assert!(after_first > 0 && progress_first > 0.0);

    // Run 2 resumes: it decodes only what is left.
    let second = Arc::new(AtomicUsize::new(0));
    let d2 = deps(Arc::clone(&db), &dir, Arc::clone(&second), None, Arc::clone(&events));
    assert_eq!(run_meeting_job(&d2, &m.id).unwrap(), MeetingState::Complete);

    // A control meeting over the same audio, decoded in one go.
    let control = db.create_meeting("control", "manual").unwrap();
    db.finish_meeting(&control.id, SECONDS, Some(&mic)).unwrap();
    db.set_meeting_sys_wav_path(&control.id, Some(&sys)).unwrap();
    let full = Arc::new(AtomicUsize::new(0));
    let d3 = deps(Arc::clone(&db), &dir, Arc::clone(&full), None, events);
    assert_eq!(run_meeting_job(&d3, &control.id).unwrap(), MeetingState::Complete);

    let total = full.load(Ordering::SeqCst);
    let resumed = second.load(Ordering::SeqCst);
    println!("full={total} first=2 resumed={resumed}");
    assert_eq!(2 + resumed, total, "the resumed run decodes ONLY the windows left");
    let segs = db.list_meeting_segments(&m.id).unwrap();
    let control_segs = db.list_meeting_segments(&control.id).unwrap();
    assert_eq!(segs.len(), control_segs.len(), "no window written twice");
    let mut keys: Vec<(i64, i64)> = segs
        .iter()
        .map(|s| (s.track, (s.start_seconds * 1000.0) as i64))
        .collect();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), segs.len(), "no duplicate segment");
    let row = db.get_meeting(&m.id).unwrap().unwrap();
    assert_eq!(row.state, "complete");
    assert!((row.processed_through_seconds - row.duration_seconds).abs() < 1e-6);
}

#[test]
fn a_meeting_whose_audio_is_gone_fails_with_the_sentence_and_is_never_complete() {
    let dir = temp_dir("meetings-fix-gone");
    let db = Arc::new(open_db(&dir));
    let m = db.create_meeting("gone", "manual").unwrap();
    db.finish_meeting(&m.id, SECONDS, Some(&dir.join("missing.wav"))).unwrap();
    db.set_meeting_state(&m.id, MeetingState::Transcribing, None).unwrap();
    let d = deps(
        Arc::clone(&db),
        &dir,
        Arc::new(AtomicUsize::new(0)),
        None,
        Arc::new(Mutex::new(Vec::new())),
    );
    assert_eq!(run_meeting_job(&d, &m.id).unwrap(), MeetingState::Failed);
    let row = db.get_meeting(&m.id).unwrap().unwrap();
    assert_eq!(row.state, "failed");
    assert_eq!(row.error.as_deref(), Some(meeting_pipeline::AUDIO_REMOVED_MESSAGE));
}

#[test]
fn the_production_stop_path_and_launch_wire_the_pipeline() {
    let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap();
    assert!(lib.contains("meeting_pipeline::spawn"), "lib.rs spawns the pipeline");
    assert!(lib.contains("reconcile_stranded_meetings"), "lib.rs reconciles at launch");
    let ctl =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/meeting_control.rs"))
            .unwrap();
    assert!(ctl.contains("meeting_pipeline::enqueue(&active.id)"), "stop enqueues");
}
