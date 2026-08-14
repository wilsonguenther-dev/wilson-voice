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
//! Run against the real `TranscriptionManager` with a 300 ms timeout and a stub
//! engine that hangs on exactly one window.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
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

/// Far shorter than production's 120 s, same mechanism.
const TIMEOUT: Duration = Duration::from_millis(300);
/// The hung window's decode — comfortably past the timeout.
const HANG: Duration = Duration::from_millis(900);
/// Which window hangs: the second one, so it has neighbours on both sides.
const POISONED_WINDOW: (f64, f64) = (28.0, 60.0);

const MEETING_SECONDS: f64 = 120.0;

struct FlakyEngine {
    decoder: RampDecoder,
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
            // Wedged native call: it never comes back inside the budget. The
            // manager gives up on it AND on the engine.
            std::thread::sleep(HANG);
        }
        Ok(self.decoder.transcript_for(start, end))
    }
}

#[test]
fn a_chunk_that_exceeds_the_transcribe_timeout_fails_alone() {
    let loads = Arc::new(AtomicUsize::new(0));
    let loader_loads = loads.clone();
    let manager = TranscriptionManager::with_loader(
        Arc::new(move |_p: &std::path::Path| {
            loader_loads.fetch_add(1, Ordering::AcqRel);
            Ok(Box::new(FlakyEngine {
                decoder: RampDecoder::new(true),
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
    let manager = TranscriptionManager::with_loader(
        Arc::new(move |_p: &std::path::Path| {
            loader_loads.fetch_add(1, Ordering::AcqRel);
            Ok(Box::new(FlakyEngine {
                decoder: RampDecoder::new(true),
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

    let started = std::time::Instant::now();
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
    assert!(
        started.elapsed() < HANG,
        "the relaunch re-ran the wedged chunk ({:?})",
        started.elapsed()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
