//! Renamed from `meeting_transcription_resume.rs` by YV99: this file is
//! error-matrix row `3a`, and the phase's acceptance sweep is a run over the
//! `matrix_*` binaries — under the old name the sweep could never reach it.
//!
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

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use wilson_voice_lib::asr_engine::TimedTranscript;
use wilson_voice_lib::meeting_asr::{
    BoundaryKind, ChunkAsr, ChunkConfig, ChunkStatus, JsonProgressStore, MeetingAsr,
    MeetingAsrConfig, MeetingProgress, NoDemand, ProgressStore,
};
use wilson_voice_lib::transcription::{
    CancelHandle, Transcriber, TranscriptionManager, ABANDONED_FOR_EXIT,
};

#[path = "support/meeting.rs"]
mod support;
use support::{expected_words, ramp_audio, RampDecoder, StraddleDecoder};

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
        first_windows.len() + second_windows.len() < 2 * ((MEETING_SECONDS / 30.0).ceil() as usize),
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
    assert_eq!(
        partial.merge.no_anchor_seams, 0,
        "every seam found its anchor"
    );
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

// ---------------------------------------------------------------------------
// The resume SEAM — the boundary the previous run made
// ---------------------------------------------------------------------------

/// A word cut in half by the resume boundary survives the relaunch exactly once.
///
/// This is the assertion `transcription_resumes_from_the_last_completed_chunk`
/// was *supposed* to be making and could not: `RampDecoder` centres every word
/// at `k + 0.5` and every boundary is a whole second, so no word it emits is
/// ever truncated by a cut and the seam it checks is trivially clean.
/// [`StraddleDecoder`] puts one word ON each boundary instead.
///
/// What that exposed: the plan for a resumed run seeded its first boundary as
/// `BoundaryKind::Edge` — "the plan starts here" — but the resume point is not
/// an edge, it is the cut the PREVIOUS run made. `merge_timed` picks its seam
/// rule from that kind, so a fixed-clock resume seam fell to the intersection
/// rule (`do the two spans cover the same instant?`), the two decodes of the
/// truncated word are adjacent rather than overlapping, and both were kept:
/// `w0089 w0090 w0090 w0091` where an uninterrupted run gives `w0089 w0090
/// w0091`. The PR claimed the resumed transcript was identical to an
/// uninterrupted one; it was not.
///
/// Both halves are asserted, and in that order, because "the resumed run is
/// clean" is worth nothing unless the uninterrupted run it is compared against
/// really does exercise a truncated word at every seam.
#[test]
fn a_word_straddling_the_resume_seam_survives_the_relaunch_exactly_once() {
    let dir = scratch("straddle");
    let audio = ramp_audio(MEETING_SECONDS);
    let expected = expected_words(MEETING_SECONDS as usize).join(" ");
    let demand = NoDemand;

    // ── The fixture is itself under test ────────────────────────────────────
    // Everything below is vacuous unless a word really does go THROUGH the cut,
    // and the first cut of this fixture silently stopped doing that (see
    // `BOUNDARY_EPS` in tests/support/meeting.rs). So the two decodes of the
    // straddling word are asserted before they are relied on: the outgoing
    // window's copy must END at the cut, the incoming window's copy must START
    // there, and the two must NOT overlap — because non-overlap is exactly what
    // the intersection rule cannot see through and the truncation rule can.
    let probe = StraddleDecoder::new(30.0, MEETING_SECONDS);
    let outgoing = probe.absolute_spans(58.0, 90.0);
    let incoming = probe.absolute_spans(88.0, 120.0);
    let cut_off = outgoing.last().expect("the outgoing window decodes words");
    let re_seen = incoming
        .iter()
        .find(|(_, _, t)| t == &cut_off.2)
        .expect("the incoming window re-sees the straddling word");
    assert_eq!(cut_off.2, "w0090", "the word the 90 s cut goes through");
    assert!(
        (cut_off.1 - 90.0).abs() < 1e-9,
        "the truncated copy must end AT the cut, not {}",
        cut_off.1
    );
    assert!(
        re_seen.0 >= cut_off.1,
        "the two copies must be disjoint ({:?} then {:?}) — overlapping ones are \
         caught by the intersection rule and this test proves nothing",
        cut_off,
        re_seen
    );

    // ── The reference: no interruption, straddling words at every seam ──────
    let whole = StraddleDecoder::new(30.0, MEETING_SECONDS);
    let mut store = JsonProgressStore::new(&dir);
    let never = || false;
    let mut job = MeetingAsr {
        audio: &audio,
        vad: None,
        asr: &whole,
        demand: &demand,
        store: &mut store,
        quit: &never,
        config: config(),
    };
    let baseline = job.run("straight-through").expect("uninterrupted run");
    assert_eq!(
        baseline.text, expected,
        "the fixture itself is wrong if an uninterrupted run cannot merge it"
    );
    drop(job);

    // ── Run 1: killed after three chunks, so the resume point is a cut ──────
    let straddle_first = StraddleAfter {
        inner: StraddleDecoder::new(30.0, MEETING_SECONDS),
        seen: AtomicUsize::new(0),
    };
    let mut store = JsonProgressStore::new(&dir);
    let quit = || straddle_first.seen.load(Ordering::Acquire) >= 3;
    let mut job = MeetingAsr {
        audio: &audio,
        vad: None,
        asr: &straddle_first,
        demand: &demand,
        store: &mut store,
        quit: &quit,
        config: config(),
    };
    let partial = job.run("straddle").expect("partial run");
    assert!(partial.interrupted);
    assert_eq!(partial.processed_through_seconds, 90.0);
    drop(job);

    // The ledger remembers WHAT KIND of cut 90.0s is. Without this the fix has
    // nothing to read and the assertion below can only be satisfied by luck.
    let ledger: MeetingProgress = JsonProgressStore::new(&dir)
        .load("straddle")
        .expect("ledger");
    let last = ledger
        .chunks
        .iter()
        .max_by_key(|c| c.index)
        .expect("three chunks landed");
    assert_eq!(
        last.content_end_seconds, 90.0,
        "the last ledger chunk ends at the resume point"
    );
    let truncated = last.spans.last().expect("the last ledger chunk has words");
    assert_eq!(truncated.text, "w0090");
    assert!(
        (truncated.end_seconds - 90.0).abs() < 1e-3,
        "the ledger must actually carry a word the cut truncated, ending at the \
         resume point — got {truncated:?}"
    );
    assert_eq!(
        last.end_boundary,
        BoundaryKind::FixedClock,
        "a no-VAD plan cuts on the clock, and the ledger must say so — an `Edge` \
         here is the bug: it routes the resume seam through the intersection rule"
    );

    // ── Run 2: the relaunch ────────────────────────────────────────────────
    let second = StraddleDecoder::new(30.0, MEETING_SECONDS);
    let mut store = JsonProgressStore::new(&dir);
    let mut job = MeetingAsr {
        audio: &audio,
        vad: None,
        asr: &second,
        demand: &demand,
        store: &mut store,
        quit: &never,
        config: config(),
    };
    let out = job.run("straddle").expect("resumed run");
    assert_eq!(out.chunks_resumed, 3);
    assert!(!out.interrupted);
    assert_eq!(
        out.text, expected,
        "the resume seam duplicated (or ate) the word the cut went through"
    );
    assert_eq!(
        out.segments
            .iter()
            .map(|s| s.text.clone())
            .collect::<Vec<_>>()
            .join(" "),
        expected,
        "segments are what the user actually reads — they must carry the fix too"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// [`StopAfter`], for the straddling fixture: counts the chunks it has been
/// asked for so the quit closure can stop the run at a known boundary.
struct StraddleAfter {
    inner: StraddleDecoder,
    seen: AtomicUsize,
}

impl ChunkAsr for StraddleAfter {
    fn transcribe_timed(&self, samples: &[f32]) -> Result<TimedTranscript, String> {
        self.seen.fetch_add(1, Ordering::AcqRel);
        self.inner.transcribe_timed(samples)
    }
}

// ---------------------------------------------------------------------------
// Quitting MID-DECODE — finding #3a's other half
// ---------------------------------------------------------------------------

/// The exit drain tells a meeting chunk it was ABANDONED, not that it failed.
///
/// `drain_and_unload` fired YV70's cancel hook and marked nothing, so the lease
/// handed the driver a bare `Err` — the same shape a genuine decode failure has.
/// This runs the REAL manager and the REAL drain against an engine that blocks
/// until it is cancelled, which is what a Metal decode does.
#[test]
fn the_exit_drain_reports_a_cancelled_chunk_as_abandoned_not_as_a_failure() {
    let (manager, model_path) = blocking_manager();
    manager.load("stub", &model_path).expect("stub loads");

    let decoding = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = decoding.clone();
    let chunk_manager = manager.clone();
    let chunk = std::thread::spawn(move || {
        flag.store(true, Ordering::Release);
        chunk_manager.transcribe_timed(vec![0.1f32; 16_000], None, None)
    });
    while !decoding.load(Ordering::Acquire) {
        std::thread::yield_now();
    }
    // The decode is on its way into the engine; give it the moment it needs to
    // be genuinely in flight before the quit lands on it.
    std::thread::sleep(Duration::from_millis(50));

    manager.drain_and_unload(Duration::from_secs(5));
    let outcome = chunk.join().expect("the chunk thread returns");

    assert_eq!(
        outcome.err().as_deref(),
        Some(ABANDONED_FOR_EXIT),
        "a chunk the exit drain cancelled must be told apart from one that FAILED \
         — the driver writes `asr_failed` for a failure and never retries it"
    );
}

/// …and the driver acts on it: nothing is recorded, so the next launch resumes
/// at the window that was interrupted rather than after it.
///
/// This is the permanent hole, end to end. With the cancelled chunk reported as
/// a plain failure, the driver wrote an `asr_failed` row for it, the ledger's
/// `processed_through_seconds` advanced past it, and — because a failed chunk is
/// deliberately never retried — the relaunch resumed AFTER a window whose audio
/// had never been transcribed. Quitting during chunk 1 of a five-chunk meeting
/// left `processed_through = 300 s` with 30–60 s permanently blank.
#[test]
fn quitting_mid_decode_leaves_the_unfinished_chunk_for_the_next_launch() {
    let dir = scratch("exit-drain");
    let audio = ramp_audio(MEETING_SECONDS);
    let demand = NoDemand;
    let never = || false;

    // ── Run 1: the second chunk's decode is cancelled by the exit drain ────
    let interrupted_at = 1usize;
    let first = AbandonAt {
        inner: RampDecoder::new(true),
        abandon_index: interrupted_at,
        seen: AtomicUsize::new(0),
    };
    let mut store = JsonProgressStore::new(&dir);
    let mut job = MeetingAsr {
        audio: &audio,
        vad: None,
        asr: &first,
        demand: &demand,
        store: &mut store,
        quit: &never,
        config: config(),
    };
    let partial = job.run("m-exit").expect("the quit is not an error");
    assert!(partial.interrupted, "the run stood down");
    assert_eq!(partial.chunks_decoded, 1, "only chunk 0 landed");
    drop(job);

    let ledger: MeetingProgress = JsonProgressStore::new(&dir).load("m-exit").expect("ledger");
    assert_eq!(
        ledger.processed_through_seconds, 30.0,
        "the ledger must stop at the END of the last chunk that was really \
         decoded — advancing past the abandoned one is the permanent hole"
    );
    assert!(
        ledger.chunks.iter().all(|c| c.status == ChunkStatus::Done),
        "an abandoned chunk is NOT a failure: an `asr_failed` row here is never \
         retried, which is what makes the hole permanent — got {:?}",
        ledger.chunks.iter().map(|c| c.status).collect::<Vec<_>>()
    );

    // ── Run 2: the relaunch re-decodes the abandoned window ────────────────
    let second = RampDecoder::new(true);
    let mut store = JsonProgressStore::new(&dir);
    let mut job = MeetingAsr {
        audio: &audio,
        vad: None,
        asr: &second,
        demand: &demand,
        store: &mut store,
        quit: &never,
        config: config(),
    };
    let out = job.run("m-exit").expect("the resumed run finishes");
    assert_eq!(out.chunks_resumed, 1);
    assert_eq!(out.chunks_failed, 0);
    assert_eq!(
        second.windows_decoded().first().map(|w| w.0),
        Some(28.0),
        "the relaunch picks the abandoned window back up (30 s minus the overlap)"
    );
    assert_eq!(
        out.text,
        expected_words(MEETING_SECONDS as usize).join(" "),
        "the quit cost the user a chunk of their meeting"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A decoder whose `abandon_index`-th chunk comes back the way a decode
/// cancelled by the exit drain does.
struct AbandonAt {
    inner: RampDecoder,
    abandon_index: usize,
    seen: AtomicUsize,
}

impl ChunkAsr for AbandonAt {
    fn transcribe_timed(&self, samples: &[f32]) -> Result<TimedTranscript, String> {
        let n = self.seen.fetch_add(1, Ordering::AcqRel);
        if n == self.abandon_index {
            return Err(ABANDONED_FOR_EXIT.to_string());
        }
        self.inner.transcribe_timed(samples)
    }
}

/// An engine whose decode blocks until its cancel hook fires — a stand-in for a
/// Metal decode the drain has to interrupt rather than wait out.
struct BlockingEngine {
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl Transcriber for BlockingEngine {
    fn transcribe(
        &mut self,
        _samples: &[f32],
        _language: Option<&str>,
        _bias: Option<&str>,
    ) -> Result<String, String> {
        while !self.cancelled.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(2));
        }
        Err("cancelled".to_string())
    }

    fn cancel_handle(&self) -> Option<CancelHandle> {
        let flag = self.cancelled.clone();
        Some(Arc::new(move || flag.store(true, Ordering::Release)))
    }

    fn reset_cancel(&mut self) {
        self.cancelled.store(false, Ordering::Release);
    }
}

fn blocking_manager() -> (TranscriptionManager, PathBuf) {
    // `load` insists the model file exists; any file will do — the loader below
    // never opens it.
    let model_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let m = TranscriptionManager::with_loader(
        Arc::new(|_p: &std::path::Path| {
            Ok(Box::new(BlockingEngine {
                cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            }) as Box<dyn Transcriber>)
        }),
        Duration::from_secs(3600),
        Duration::from_secs(3600),
        Duration::from_secs(30),
    );
    (m, model_path)
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
