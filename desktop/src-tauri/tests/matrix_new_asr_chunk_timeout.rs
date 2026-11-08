//! Renamed from `meeting_chunk_timeout_isolation.rs` by YV99: this file is
//! error-matrix row `3b`, and the phase's acceptance sweep is a run over the
//! `matrix_*` binaries — under the old name the sweep could never reach it.
//!
//! YV93 — one chunk that blows through `TRANSCRIBE_TIMEOUT` must not poison the
//! rest of the meeting.
//!
//! This is the second of the two error-matrix rows plan finding #3 adds. What
//! makes it more than a `match` arm is the mechanism underneath: past the
//! timeout the manager abandons the ENGINE, not just the call (the worker thread
//! keeps it and drops it whenever the native side finally returns). So a naive
//! "log it and carry on" gets `no ASR model is loaded` for every chunk after the
//! first timeout — every remaining chunk fails, and the meeting comes out empty
//! from the first bad window onwards. That is the failure this file exists to
//! catch.
//!
//! Run against the real `TranscriptionManager` with a stub engine that wedges
//! on exactly one window.
//!
//! fix-main-red-chunk-timeout (main run 42432221845) — this file used to race
//! the clock. The wedged window SLEPT 900 ms against a 300 ms timeout, and the
//! healthy windows had to finish inside that same 300 ms on a decode thread the
//! manager puts in the Darwin background band (meeting work is `PRIO_DARWIN_BG`,
//! yap-1106-sp-04). On a loaded CI runner a background-band thread can be
//! starved past 300 ms before it runs a line, so a HEALTHY neighbour timed out
//! too and the run reported two dead chunks. Nothing in the production path was
//! shared between chunks — every lease starts its own `recv_timeout` — the test
//! simply made "healthy" and "hung" a few hundred milliseconds apart.
//!
//! Now neither side depends on how fast the runner is:
//! - the wedged window blocks on a [`Gate`] the test opens only AFTER the
//!   meeting has finished, so it cannot come back inside any deadline, however
//!   long that deadline turns out to be;
//! - the healthy windows get a deadline (5 s) four orders of magnitude above
//!   the microseconds their decode costs, so scheduling noise cannot reach it.
//!
//! Which window is slow is decided by the stub's audio, never by a sleep.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use wilson_voice_lib::asr_engine::TimedTranscript;
use wilson_voice_lib::meeting_asr::{
    ChunkConfig, ChunkStatus, JsonProgressStore, MeetingAsr, MeetingAsrConfig, MeetingProgress,
    NoDemand, ProgressStore, WarmEngineChunkAsr,
};
use wilson_voice_lib::transcription::{Transcriber, TranscriptionManager};

#[path = "support/meeting.rs"]
mod support;
use support::{ramp_audio, window_bounds, word_at, RampDecoder};

/// Far shorter than production's 120 s, same mechanism. The wedged window
/// fails because it is GATED past the meeting's end, not because it out-sleeps
/// this number, so the number only has to be generous for the healthy windows
/// — whose stub decode is microseconds even on a background-band thread on a
/// saturated runner.
const TIMEOUT: Duration = Duration::from_secs(5);
/// Which window hangs: the second one, so it has neighbours on both sides.
const POISONED_WINDOW: (f64, f64) = (28.0, 60.0);

const MEETING_SECONDS: f64 = 120.0;

/// A latch the wedged decode waits on. Closed until the test opens it; opening
/// is idempotent. The wait has no timeout of its own: while closed, the decode
/// is a native call that is not coming back.
#[derive(Clone, Default)]
struct Gate(Arc<(Mutex<bool>, Condvar)>);

impl Gate {
    fn wait(&self) {
        let (open, cv) = &*self.0;
        let mut open = open.lock().unwrap_or_else(|p| p.into_inner());
        while !*open {
            open = cv.wait(open).unwrap_or_else(|p| p.into_inner());
        }
    }

    fn open(&self) {
        let (open, cv) = &*self.0;
        *open.lock().unwrap_or_else(|p| p.into_inner()) = true;
        cv.notify_all();
    }
}

/// Opens the gate when the test ends, pass or panic, so the abandoned worker
/// thread finishes and drops its engine instead of outliving the test.
struct OpenOnDrop(Gate);

impl Drop for OpenOnDrop {
    fn drop(&mut self) {
        self.0.open();
    }
}

struct FlakyEngine {
    decoder: RampDecoder,
    wedge: Gate,
    /// How many times the poisoned window reached the engine, across every
    /// engine the loader builds.
    poisoned_calls: Arc<AtomicUsize>,
}

impl Transcriber for FlakyEngine {
    fn transcribe(
        &mut self,
        _samples: &[f32],
        _language: Option<&str>,
        _bias: Option<&str>,
    ) -> Result<String, String> {
        Ok(String::new())
    }

    fn transcribe_timed(
        &mut self,
        samples: &[f32],
        _language: Option<&str>,
        _bias: Option<&str>,
    ) -> Result<TimedTranscript, String> {
        let (start, end) = window_bounds(samples);
        if (start - POISONED_WINDOW.0).abs() < 0.5 && (end - POISONED_WINDOW.1).abs() < 0.5 {
            // Wedged native call: it does not come back until the meeting is
            // over, so it misses the budget whatever the budget is. The
            // manager gives up on it AND on the engine.
            self.poisoned_calls.fetch_add(1, Ordering::AcqRel);
            self.wedge.wait();
        }
        Ok(self.decoder.transcript_for(start, end))
    }
}

#[test]
fn a_chunk_that_exceeds_the_transcribe_timeout_fails_alone() {
    let loads = Arc::new(AtomicUsize::new(0));
    let loader_loads = loads.clone();
    let wedge = Gate::default();
    let _release = OpenOnDrop(wedge.clone());
    let poisoned_calls = Arc::new(AtomicUsize::new(0));
    let (loader_wedge, loader_poisoned) = (wedge.clone(), poisoned_calls.clone());
    let manager = TranscriptionManager::with_loader(
        Arc::new(move |_p: &std::path::Path| {
            loader_loads.fetch_add(1, Ordering::AcqRel);
            Ok(Box::new(FlakyEngine {
                decoder: RampDecoder::new(true),
                wedge: loader_wedge.clone(),
                poisoned_calls: loader_poisoned.clone(),
            }) as Box<dyn Transcriber>)
        }),
        Duration::from_secs(3600),
        Duration::from_secs(3600),
        TIMEOUT,
    );
    let model_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    manager.load("stub", &model_path).expect("stub loads");

    let dir = std::env::temp_dir().join(format!("yap-yv93-timeout-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");

    let audio = ramp_audio(MEETING_SECONDS);
    let asr = WarmEngineChunkAsr::new(
        manager.clone(),
        "stub",
        &model_path,
        Some("en".to_string()),
        None,
    );
    let mut store = JsonProgressStore::new(&dir);
    let never = || false;
    let demand = NoDemand;
    let mut job = MeetingAsr {
        audio: &audio,
        vad: None,
        asr: &asr,
        demand: &demand,
        store: &mut store,
        quit: &never,
        config: MeetingAsrConfig {
            chunk: ChunkConfig::default(),
            yield_poll: Duration::from_millis(2),
            ..MeetingAsrConfig::default()
        },
    };
    let out = job.run("m-timeout").expect("the meeting still completes");
    // The meeting is over; only now may the wedged decode come back. Its
    // engine was abandoned long ago, so the worker drops it.
    wedge.open();
    assert_eq!(
        poisoned_calls.load(Ordering::Acquire),
        1,
        "the wedged window was decoded exactly once"
    );

    // The meeting did not fail wholesale.
    assert_eq!(out.chunks_failed, 1, "exactly one chunk died");
    assert_eq!(out.processed_through_seconds, MEETING_SECONDS);

    let ledger: MeetingProgress = JsonProgressStore::new(&dir)
        .load("m-timeout")
        .expect("ledger");
    assert_eq!(ledger.chunks.len(), 4, "120 s at 30 s windows");
    let failed: Vec<_> = ledger
        .chunks
        .iter()
        .filter(|c| c.status == ChunkStatus::AsrFailed)
        .collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].index, 1);
    assert_eq!(failed[0].text, "", "a failed chunk carries an empty text");
    assert!(
        failed[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("timed out"),
        "the failure says what it was: {:?}",
        failed[0].error
    );

    // The neighbours are untouched — this is the "must not poison" clause.
    for chunk in ledger.chunks.iter().filter(|c| c.index != 1) {
        assert_eq!(chunk.status, ChunkStatus::Done, "chunk {}", chunk.index);
        assert!(
            !chunk.text.is_empty(),
            "chunk {} lost its text",
            chunk.index
        );
    }
    assert!(
        out.text.contains(&word_at(5)),
        "the first chunk's words are gone"
    );
    assert!(
        out.text.contains(&word_at(95)),
        "the chunks AFTER the timeout are gone — the engine was never rebuilt"
    );
    assert!(
        !out.text.contains(&word_at(45)),
        "the timed-out chunk's words came from somewhere"
    );

    // …and the mechanism that made the recovery possible: the abandoned engine
    // was rebuilt exactly once, rather than a fresh one per chunk.
    assert_eq!(
        loads.load(Ordering::Acquire),
        2,
        "one initial load plus one rebuild after the abandonment"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A relaunch does not retry a chunk that already blew the timeout — 120 s of
/// wedged Metal per relaunch is how a meeting never finishes.
#[test]
fn a_failed_chunk_is_not_retried_on_resume() {
    let dir = std::env::temp_dir().join(format!("yap-yv93-noretry-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");

    let loads = Arc::new(AtomicUsize::new(0));
    let loader_loads = loads.clone();
    let wedge = Gate::default();
    let _release = OpenOnDrop(wedge.clone());
    let poisoned_calls = Arc::new(AtomicUsize::new(0));
    let (loader_wedge, loader_poisoned) = (wedge.clone(), poisoned_calls.clone());
    let manager = TranscriptionManager::with_loader(
        Arc::new(move |_p: &std::path::Path| {
            loader_loads.fetch_add(1, Ordering::AcqRel);
            Ok(Box::new(FlakyEngine {
                decoder: RampDecoder::new(true),
                wedge: loader_wedge.clone(),
                poisoned_calls: loader_poisoned.clone(),
            }) as Box<dyn Transcriber>)
        }),
        Duration::from_secs(3600),
        Duration::from_secs(3600),
        TIMEOUT,
    );
    let model_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    manager.load("stub", &model_path).expect("stub loads");
    let audio = ramp_audio(MEETING_SECONDS);
    let asr = WarmEngineChunkAsr::new(
        manager.clone(),
        "stub",
        &model_path,
        Some("en".to_string()),
        None,
    );
    let never = || false;
    let demand = NoDemand;
    {
        let mut store = JsonProgressStore::new(&dir);
        let mut job = MeetingAsr {
            audio: &audio,
            vad: None,
            asr: &asr,
            demand: &demand,
            store: &mut store,
            quit: &never,
            config: MeetingAsrConfig {
                yield_poll: Duration::from_millis(2),
                ..MeetingAsrConfig::default()
            },
        };
        job.run("m-timeout").expect("first pass");
    }
    // Let the abandoned first-pass decode finish before the relaunch, so the
    // count below is the relaunch's alone.
    wedge.open();
    assert_eq!(
        poisoned_calls.load(Ordering::Acquire),
        1,
        "the first pass hit the wedged window once"
    );

    let mut store = JsonProgressStore::new(&dir);
    let mut job = MeetingAsr {
        audio: &audio,
        vad: None,
        asr: &asr,
        demand: &demand,
        store: &mut store,
        quit: &never,
        config: MeetingAsrConfig {
            yield_poll: Duration::from_millis(2),
            ..MeetingAsrConfig::default()
        },
    };
    let out = job.run("m-timeout").expect("second pass");
    assert_eq!(out.chunks_decoded, 0, "nothing was left to do");
    assert_eq!(out.chunks_resumed, 4);
    assert_eq!(out.chunks_failed, 1, "the hole is remembered, not re-dug");
    // Counted, not timed: the relaunch never handed the wedged window to an
    // engine at all (it used to be a `< 900 ms` wall-clock bound, which a slow
    // runner could fail with no retry and a fast one could pass with one).
    assert_eq!(
        poisoned_calls.load(Ordering::Acquire),
        1,
        "the relaunch re-ran the wedged chunk"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
