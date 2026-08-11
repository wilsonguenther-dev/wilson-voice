//! YV93 — a meeting that was killed mid-transcription resumes at the last
//! completed chunk instead of re-running a full Metal decode from zero.
//!
//! Plan finding #3, verbatim: *"`drain_and_unload` cancels and discards
//! in-flight work on quit with no checkpoint; `meetings.state` has no per-chunk
//! column, so a resumed meeting restarts a full Metal decode from zero."* The
//! fix is `processed_through_seconds`, written as each chunk lands. This file
//! is the proof, and it proves three separate things, because "it resumed" is
//! satisfied by an implementation that quietly loses a chunk:
//!
//! 1. The ledger is on DISK after every chunk — not at the end, not in memory.
//!    Simulating the kill is then just: stop calling, throw the driver away, and
//!    build a new one from the file the way a relaunch would.
//! 2. The resumed run never re-decodes audio the first run finished. The
//!    fixture's samples carry their own timestamps (see `tests/support/`), so
//!    this is checked against the WINDOWS the decoder was actually handed, not
//!    against a call count.
//! 3. The transcript after the interruption is IDENTICAL to the transcript of
//!    an uninterrupted run — every word once, in order. A resume that drops the
//!    straddling word, or replays it, fails here.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use wilson_voice_lib::asr_engine::TimedTranscript;
use wilson_voice_lib::meeting_asr::{
    ChunkAsr, ChunkConfig, JsonProgressStore, MeetingAsr, MeetingAsrConfig, MeetingProgress,
    NoDemand, ProgressStore,
};

#[path = "support/meeting.rs"]
mod support;
use support::{expected_words, ramp_audio, RampDecoder};

const MEETING_SECONDS: f64 = 300.0;

fn config() -> MeetingAsrConfig {
    MeetingAsrConfig {
        chunk: ChunkConfig::default(),
        yield_poll: Duration::from_millis(1),
        ..MeetingAsrConfig::default()
    }
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("yap-yv93-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// The reference: one uninterrupted run over the whole fixture.
#[test]
fn an_uninterrupted_run_transcribes_every_word_exactly_once() {
    let dir = scratch("baseline");
    let audio = ramp_audio(MEETING_SECONDS);
    let decoder = RampDecoder::new(true);
    let mut store = JsonProgressStore::new(&dir);
    let never = || false;
    let demand = NoDemand;
    let mut job = MeetingAsr {
        audio: &audio,
        vad: None,
        asr: &decoder,
        demand: &demand,
        store: &mut store,
        quit: &never,
        config: config(),
    };
    let out = job.run("m1").expect("meeting transcribes");

    assert!(out.timestamps_are_real, "the fixture reports word times");
    assert_eq!(
        out.text,
        expected_words(MEETING_SECONDS as usize).join(" "),
        "every word, once, in order"
    );
    assert_eq!(out.chunks_resumed, 0);
    assert_eq!(out.chunks_failed, 0);
    assert!(!out.interrupted);
    assert_eq!(out.processed_through_seconds, MEETING_SECONDS);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A decoder that stops the world after `stop_after` chunks — the simulated
/// `kill -9`. The driver is asked to quit at the next boundary, which is
/// exactly what a real kill would interrupt: the ledger holds the chunks that
/// landed, and nothing else.
struct StopAfter {
    inner: RampDecoder,
    stop_after: usize,
    seen: AtomicUsize,
}

impl ChunkAsr for StopAfter {
    fn transcribe_timed(&self, samples: &[f32]) -> Result<TimedTranscript, String> {
        self.seen.fetch_add(1, Ordering::AcqRel);
        self.inner.transcribe_timed(samples)
    }
}

#[test]
fn transcription_resumes_from_the_last_completed_chunk() {
    let dir = scratch("resume");
    let audio = ramp_audio(MEETING_SECONDS);

    // ── Run 1: killed after three chunks ───────────────────────────────────
    let first = StopAfter {
        inner: RampDecoder::new(true),
        stop_after: 3,
        seen: AtomicUsize::new(0),
    };
    let mut store = JsonProgressStore::new(&dir);
    let demand = NoDemand;
    let killed = |seen: &AtomicUsize, after: usize| seen.load(Ordering::Acquire) >= after;
    let quit = || killed(&first.seen, first.stop_after);
    let mut job = MeetingAsr {
        audio: &audio,
        vad: None,
        asr: &first,
        demand: &demand,
        store: &mut store,
        quit: &quit,
        config: config(),
    };
    let partial = job.run("m1").expect("the partial run returns cleanly");
    assert!(partial.interrupted, "the run was cut short");
    assert_eq!(partial.chunks_decoded, 3);
    assert_eq!(
        partial.processed_through_seconds, 90.0,
        "three 30 s chunks landed"
    );
    let first_windows = first.inner.windows_decoded();
    drop(job);

    // ── The ledger is DURABLE: a relaunch reads it off disk ────────────────
    let on_disk = JsonProgressStore::new(&dir);
    let ledger: MeetingProgress = on_disk.load("m1").expect("ledger reads back");
    assert_eq!(ledger.chunks.len(), 3);
    assert_eq!(ledger.processed_through_seconds, 90.0);
    assert!(
        on_disk.path_for("m1").is_file(),
        "the ledger is a file, not a memory of one"
    );

    // ── Run 2: a fresh everything, exactly as a relaunch would ─────────────
    let second = RampDecoder::new(true);
    let mut store = JsonProgressStore::new(&dir);
    let never = || false;
    let mut job = MeetingAsr {
        audio: &audio,
        vad: None,
        asr: &second,
        demand: &demand,
        store: &mut store,
        quit: &never,
        config: config(),
    };
    let out = job.run("m1").expect("the resumed run finishes");

    assert_eq!(out.chunks_resumed, 3, "three chunks came from the ledger");
    assert_eq!(out.processed_through_seconds, MEETING_SECONDS);
    assert!(!out.interrupted);

    // (2) Nothing before the resume point was decoded a second time.
    let second_windows = second.windows_decoded();
    assert!(
        second_windows
            .iter()
            .all(|(start, _)| *start >= 90.0 - ChunkConfig::default().overlap_seconds - 1e-3),
        "the resumed run re-decoded audio it already had: {second_windows:?}"
    );
    assert_eq!(
        second_windows[0].0, 88.0,
        "…and it picks up at the resume point, minus the overlap it needs for context"
    );
    assert!(
        first_windows.len() + second_windows.len()
            < 2 * ((MEETING_SECONDS / 30.0).ceil() as usize),
        "a resume that re-decodes everything is not a resume"
    );

    // (3) The transcript is the same as an uninterrupted run's.
    assert_eq!(
        out.text,
        expected_words(MEETING_SECONDS as usize).join(" "),
        "the interruption cost, or duplicated, a word"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The resume point is a checkpoint, not a summary written at the end: after
/// ONE chunk the file already names it.
#[test]
fn the_ledger_is_written_after_every_chunk_not_at_the_end() {
    let dir = scratch("checkpoint");
    let audio = ramp_audio(MEETING_SECONDS);
    let decoder = StopAfter {
        inner: RampDecoder::new(true),
        stop_after: 1,
        seen: AtomicUsize::new(0),
    };
    let mut store = JsonProgressStore::new(&dir);
    let demand = NoDemand;
    let quit = || decoder.seen.load(Ordering::Acquire) >= decoder.stop_after;
    let mut job = MeetingAsr {
        audio: &audio,
        vad: None,
        asr: &decoder,
        demand: &demand,
        store: &mut store,
        quit: &quit,
        config: config(),
    };
    let out = job.run("m2").expect("one chunk lands");
    assert_eq!(out.chunks_decoded, 1);

    let raw = std::fs::read_to_string(JsonProgressStore::new(&dir).path_for("m2"))
        .expect("the checkpoint exists on disk");
    let ledger: MeetingProgress = serde_json::from_str(&raw).expect("valid JSON ledger");
    assert_eq!(ledger.processed_through_seconds, 30.0);
    assert_eq!(ledger.chunks.len(), 1);
    assert_eq!(ledger.chunks[0].index, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The FALLBACK merge path, end to end and across a resume: a model that
/// returns no alignment at all (plan finding #11 says nobody had confirmed the
/// shipped one does) still produces one clean transcript, with the text anchor
/// removing the repeat at every seam.
///
/// The overlap here is six seconds rather than two, because this fixture speaks
/// one word per second and the anchor needs at least three tokens of genuine
/// overlap to splice on — six seconds of it is the same SIX WORDS the shipped
/// two-second overlap holds at the 175 wpm the eval corpus is spoken at.
#[test]
fn a_meeting_with_no_timestamps_merges_on_text_and_still_resumes() {
    let dir = scratch("fallback");
    let audio = ramp_audio(MEETING_SECONDS);
    let wide_overlap = MeetingAsrConfig {
        chunk: ChunkConfig {
            overlap_seconds: 6.0,
            ..ChunkConfig::default()
        },
        yield_poll: Duration::from_millis(1),
        ..MeetingAsrConfig::default()
    };

    let first = StopAfter {
        inner: RampDecoder::new(false),
        stop_after: 2,
        seen: AtomicUsize::new(0),
    };
    let mut store = JsonProgressStore::new(&dir);
    let demand = NoDemand;
    let quit = || first.seen.load(Ordering::Acquire) >= first.stop_after;
    let mut job = MeetingAsr {
        audio: &audio,
        vad: None,
        asr: &first,
        demand: &demand,
        store: &mut store,
        quit: &quit,
        config: wide_overlap.clone(),
    };
    let partial = job.run("m-fallback").expect("partial run");
    assert!(!partial.timestamps_are_real, "no alignment in this fixture");
    assert_eq!(partial.merge.no_anchor_seams, 0, "every seam found its anchor");
    drop(job);

    let second = RampDecoder::new(false);
    let mut store = JsonProgressStore::new(&dir);
    let never = || false;
    let mut job = MeetingAsr {
        audio: &audio,
        vad: None,
        asr: &second,
        demand: &demand,
        store: &mut store,
        quit: &never,
        config: wide_overlap,
    };
    let out = job.run("m-fallback").expect("resumed run");
    assert!(!out.timestamps_are_real);
    assert_eq!(out.chunks_resumed, 2);
    assert_eq!(
        out.merge.no_anchor_seams, 0,
        "an unanchored seam duplicates the overlap"
    );
    assert_eq!(
        out.text,
        expected_words(MEETING_SECONDS as usize).join(" "),
        "the text-anchored merge lost or duplicated a word"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A corrupt ledger costs a re-decode and never a meeting: the audio is still
/// on disk, so the honest move is to start over rather than to refuse.
#[test]
fn a_corrupt_ledger_starts_over_instead_of_failing() {
    let dir = scratch("corrupt");
    let store = JsonProgressStore::new(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(store.path_for("m3"), "{ this is not json").unwrap();

    let audio = ramp_audio(60.0);
    let decoder = RampDecoder::new(true);
    let mut store = JsonProgressStore::new(&dir);
    let never = || false;
    let demand = NoDemand;
    let mut job = MeetingAsr {
        audio: &audio,
        vad: None,
        asr: &decoder,
        demand: &demand,
        store: &mut store,
        quit: &never,
        config: config(),
    };
    let out = job.run("m3").expect("a corrupt ledger is not fatal");
    assert_eq!(out.chunks_resumed, 0);
    assert_eq!(out.text, expected_words(60).join(" "));
    let _ = std::fs::remove_dir_all(&dir);
}
