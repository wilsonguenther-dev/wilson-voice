//! Y3-C — the `transcribe_progress` event contract, from the emitting end.
//!
//! What this file holds still is the set of lies the pill used to be able to
//! draw. Each test is one of them:
//!
//!  * one event per COMPLETED chunk, and nothing in between — no timer, no
//!    interpolation, no smoothing;
//!  * ZERO events for a single-window take — a flicker of `1/1` is worse than
//!    silence;
//!  * NONE after the take ends — a late chunk must not repaint a pill that has
//!    already shown the transcript;
//!  * and the numbers themselves monotonic, cumulative and never past `of`.
//!
//! The counterpart on the receiving end is `desktop/src/pill/live.ts`
//! (`acceptProgress`), which re-checks all of it — an event is data, not a
//! promise.

use std::sync::Mutex;
use wilson_voice_lib::transcribe_progress::{
    word_count, ChunkProgress, NoProgress, ProgressSink, TranscribeProgress, MIN_REPORTABLE_CHUNKS,
    TRANSCRIBE_PROGRESS_EVENT,
};

#[derive(Default)]
struct Spy(Mutex<Vec<TranscribeProgress>>);

impl ProgressSink for Spy {
    fn progress(&self, event: TranscribeProgress) {
        self.0.lock().unwrap().push(event);
    }
}

impl Spy {
    fn seen(&self) -> Vec<TranscribeProgress> {
        self.0.lock().unwrap().clone()
    }
}

/// The name is half the contract: both pills listen for this exact string.
#[test]
fn the_event_name_is_the_one_the_pill_listens_for() {
    assert_eq!(TRANSCRIBE_PROGRESS_EVENT, "transcribe_progress");
    assert_eq!(MIN_REPORTABLE_CHUNKS, 2);
}

#[test]
fn one_event_per_completed_chunk_and_not_one_more() {
    let spy = Spy::default();
    let mut p = ChunkProgress::new(12, &spy);
    assert!(p.reports());

    // Twelve chunks come back, each with real text.
    let chunks = [
        "so the thing about a long take",
        "is that the decoder knows exactly",
        "how many chunks it is going to run",
        "before it runs the first one",
        "which means the count is knowable",
        "and a knowable count should be stated",
        "not estimated from a stopwatch",
        "and never interpolated between events",
        "because a smooth bar that is lying",
        "is worse than a stepped bar that is not",
        "so this one steps",
        "and stops",
    ];
    for c in chunks {
        p.chunk_done(c);
    }
    p.finish();

    let seen = spy.seen();
    assert_eq!(
        seen.len(),
        chunks.len(),
        "exactly one event per completed chunk"
    );
    assert_eq!(p.emitted(), chunks.len() as u32);

    // Indices are 1-based, dense, and stop at `of`.
    for (i, e) in seen.iter().enumerate() {
        assert_eq!(e.chunk, i as u32 + 1);
        assert_eq!(e.of, 12);
    }

    // Word counts are REAL and cumulative — the sum of the chunks' own words,
    // which is the number the pill escalates Yappy's commentary on. The whole
    // point of the item is that this is not `voicedSeconds * WORDS_PER_SEC`.
    let expected: u32 = chunks.iter().map(|c| word_count(c)).sum();
    assert_eq!(seen.last().unwrap().words_so_far, expected);
    let mut running = 0u32;
    for (c, e) in chunks.iter().zip(seen.iter()) {
        running += word_count(c);
        assert_eq!(e.words_so_far, running, "cumulative, not per-chunk");
    }

    // Monotonic in both fields, always.
    for w in seen.windows(2) {
        assert!(w[1].chunk > w[0].chunk);
        assert!(w[1].words_so_far >= w[0].words_so_far);
        assert!(w[1].chunk <= w[1].of, "the fill can never pass 100%");
    }
}

#[test]
fn a_single_window_take_emits_zero_events() {
    // A short take decodes in one window. There is no progress to report, so
    // the pill is never put into the transcribing phase at all.
    let spy = Spy::default();
    let mut p = ChunkProgress::new(1, &spy);
    assert!(!p.reports());
    p.chunk_done("a short take that decodes in exactly one window");
    p.finish();
    assert_eq!(p.emitted(), 0);
    assert!(spy.seen().is_empty(), "no event, not even a 1/1 flicker");

    // Zero is the same: a take that was never chunked cannot report chunks.
    let spy0 = Spy::default();
    let mut z = ChunkProgress::new(0, &spy0);
    z.chunk_done("anything");
    assert!(spy0.seen().is_empty());
}

#[test]
fn nothing_is_emitted_after_the_take_ends() {
    let spy = Spy::default();
    let mut p = ChunkProgress::new(4, &spy);
    p.chunk_done("chunk one");
    p.chunk_done("chunk two");
    assert_eq!(spy.seen().len(), 2);

    p.finish();

    // A late chunk landing after the transcript must change nothing on screen.
    p.chunk_done("a chunk that arrived too late");
    p.chunk_done("and another");
    p.finish();
    assert_eq!(
        spy.seen().len(),
        2,
        "the take is over — the pill has moved on"
    );
    assert_eq!(p.emitted(), 2);
}

#[test]
fn the_index_is_clamped_at_of_even_if_the_caller_over_reports() {
    // A caller that reports more chunks than it declared would otherwise fill
    // the bar past 100% — a visibly impossible state.
    let spy = Spy::default();
    let mut p = ChunkProgress::new(3, &spy);
    for _ in 0..9 {
        p.chunk_done("one two three");
    }
    let seen = spy.seen();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen.last().unwrap().chunk, 3);
    assert_eq!(seen.last().unwrap().of, 3);
}

#[test]
fn the_null_sink_is_silent_and_still_counts_nothing() {
    // Headless runs and command mode have no pill in front of them.
    let none = NoProgress;
    let mut p = ChunkProgress::new(5, &none);
    p.chunk_done("words");
    p.chunk_done("more words");
    p.finish();
    assert_eq!(
        p.emitted(),
        2,
        "the reporter still tracks; the sink discards"
    );
}

#[test]
fn the_payload_serialises_with_the_snake_case_keys_the_pill_reads() {
    let json = serde_json::to_string(&TranscribeProgress {
        chunk: 3,
        of: 12,
        words_so_far: 91,
    })
    .expect("payload serialises");
    assert!(json.contains("\"chunk\":3"), "{json}");
    assert!(json.contains("\"of\":12"), "{json}");
    assert!(json.contains("\"words_so_far\":91"), "{json}");
}

// ─────────────────────────────────────────────────────────────────────────────
// THE WIRING. Everything above proves the reporter keeps its own rules; none of
// it would fail if the reporter were never called. These drive the REAL Y3-B
// decode loop through a stub engine and assert on what came out the other end,
// so an unwired observer is a red test rather than a green one.
// ─────────────────────────────────────────────────────────────────────────────

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use wilson_voice_lib::asr_engine::TimedTranscript;
use wilson_voice_lib::meeting_asr::{ChunkConfig, MEETING_RATE};
use wilson_voice_lib::transcribe_progress::{NoObserver, TakeObserver, TakeProgress};
use wilson_voice_lib::transcription::{
    DictationChunking, Transcriber, TranscriptionManager, DICTATION_CHUNK_SAMPLE_RATE,
};

#[path = "support/meeting.rs"]
mod support;
use support::{window_bounds, RampDecoder};

/// The same ramp stub the Y3-B suite uses: sample `i` holds `i / 16000`, so a
/// decoded window's text says exactly which seconds it covered.
struct RampEngine(RampDecoder);

impl Transcriber for RampEngine {
    fn transcribe(
        &mut self,
        samples: &[f32],
        _language: Option<&str>,
        _bias: Option<&str>,
    ) -> Result<String, String> {
        let (a, b) = window_bounds(samples);
        Ok(self.0.transcript_for(a, b).text)
    }

    fn transcribe_timed(
        &mut self,
        samples: &[f32],
        _language: Option<&str>,
        _bias: Option<&str>,
    ) -> Result<TimedTranscript, String> {
        let (a, b) = window_bounds(samples);
        Ok(self.0.transcript_for(a, b))
    }
}

fn manager() -> TranscriptionManager {
    let m = TranscriptionManager::with_loader(
        Arc::new(|_p: &Path| {
            Ok(Box::new(RampEngine(RampDecoder::new(true))) as Box<dyn Transcriber>)
        }),
        Duration::from_secs(600),
        Duration::from_secs(600),
        Duration::from_secs(30),
    );
    let stub = std::env::temp_dir().join("yap-y3c-stub-model.gguf");
    std::fs::write(&stub, b"stub").expect("stub model file");
    m.load("stub", &stub).expect("stub load");
    m
}

fn ramp_take(seconds: f64) -> Vec<f32> {
    let n = (seconds * MEETING_RATE as f64) as usize;
    (0..n).map(|i| i as f32 / MEETING_RATE as f32).collect()
}

/// Twelve five-second windows — the "chunk 9 of 12" shape the item names.
fn twelve_window_plan() -> DictationChunking {
    DictationChunking {
        threshold_seconds: 10.0,
        config: ChunkConfig {
            target_seconds: 5.0,
            min_seconds: 4.0,
            max_seconds: 6.0,
            overlap_seconds: 1.0,
            min_silence_seconds: 0.2,
        },
        sample_rate: DICTATION_CHUNK_SAMPLE_RATE,
    }
}

/// THE REACHABILITY TEST. A real windowed decode emits one event per completed
/// window, and the totals are the planner's, not a guess off the duration.
#[test]
fn the_real_decode_loop_drives_the_observer_once_per_window() {
    let spy = Spy::default();
    let mut progress = TakeProgress::new(&spy);
    let take = manager()
        .transcribe_take_with_observed(
            ramp_take(60.0),
            None,
            None,
            &twelve_window_plan(),
            &mut progress,
        )
        .expect("windowed decode");
    progress.finish();

    assert!(take.windowed, "the take must actually have split");
    let seen = spy.seen();
    assert_eq!(
        seen.len(),
        take.chunks_total,
        "one event per completed window, no more and no fewer"
    );
    assert!(
        seen.len() >= 2,
        "expected the 12-window shape, got {seen:?}"
    );
    // `of` is the planner's count, identical on every event.
    for (i, e) in seen.iter().enumerate() {
        assert_eq!(e.of as usize, take.chunks_total);
        assert_eq!(e.chunk as usize, i + 1, "the index advances by exactly one");
    }
    // Monotonic, cumulative, and never past the end.
    for pair in seen.windows(2) {
        assert!(pair[1].words_so_far >= pair[0].words_so_far);
        assert!(pair[1].chunk > pair[0].chunk);
    }
    assert!(seen.last().unwrap().chunk <= seen.last().unwrap().of);

    // THE HONESTY CLAIM: the word count is REAL decoded words, not
    // `voicedSeconds * WORDS_PER_SEC`. The ramp fixture is one word per second,
    // so a 60 s take decodes ~60 words, while the old estimate at
    // WORDS_PER_SEC = 2.5 would have claimed ~150.
    let real = word_count(&take.text);
    assert!(
        seen.last().unwrap().words_so_far >= real / 2,
        "the cumulative count must track the real transcript ({real} words)"
    );
    assert!(
        seen.last().unwrap().words_so_far < (60.0 * 2.5) as u32,
        "a real count must not match the discarded voiced-seconds estimate"
    );
}

/// A take the planner never split reaches neither observer call, so the pill
/// gets silence rather than a `1/1` flicker.
#[test]
fn a_real_single_window_take_emits_nothing_at_all() {
    let spy = Spy::default();
    let mut progress = TakeProgress::new(&spy);
    // 5 s, well under the default 120 s threshold: the unchanged single-call path.
    let take = manager()
        .transcribe_take_observed(ramp_take(5.0), None, None, &mut progress)
        .expect("short decode");
    progress.finish();

    assert!(!take.windowed, "a short take must not window");
    assert_eq!(progress.emitted(), 0);
    assert!(spy.seen().is_empty(), "got {:?}", spy.seen());
}

/// The headless observer is inert against the same real loop — no panic, no
/// event, no change to the transcript.
#[test]
fn the_null_observer_survives_a_real_windowed_decode() {
    let take = manager()
        .transcribe_take_with_observed(
            ramp_take(60.0),
            None,
            None,
            &twelve_window_plan(),
            &mut NoObserver,
        )
        .expect("windowed decode");
    assert!(take.windowed);
    assert!(!take.text.trim().is_empty());
}

/// `NoProgress` and `NoObserver` are different nulls for different seams; this
/// keeps the unused-import lint honest about both being real API.
#[test]
fn both_null_implementations_are_reachable() {
    let mut o = NoObserver;
    o.planned(9);
    o.chunk_done("nothing happens");
    let mut p = ChunkProgress::new(4, &NoProgress);
    p.chunk_done("nor here");
    assert_eq!(p.emitted(), 1);
}
