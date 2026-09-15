//! transcribe_progress — what the pill is allowed to say about a decode that is
//! still running (Y3-C).
//!
//! Before this, a long take had two dishonest surfaces and no honest one:
//!
//!  1. The pill's live escalation was a TIMER. `live.ts`'s `wordsFromVoiced`
//!     multiplies seconds of voiced audio by an average speaking rate, and the
//!     tier ladder — quick / notes / desk / essay / saga — ran off that guess.
//!     On a ten-minute take the error is enormous and the persona it promises
//!     is unearned.
//!  2. After the hold ended there was NO progress signal at all. The pill's
//!     status carries `busy: bool`, so a fifteen-minute take decoding across a
//!     dozen chunks showed the same undifferentiated busy state for minutes.
//!
//! A chunked decode already knows the truth: each completed chunk yields REAL
//! text with a REAL word count, and the chunk count is known before the first
//! one starts. This module is the only thing allowed to report it.
//!
//! The rules it enforces — every one of them a lie it refuses to emit:
//!
//!  * **Nothing for a single-window take.** A short take has no progress to
//!    report, and a flicker of `1/1` is worse than silence.
//!  * **Only real completions.** There is no timer in here, no interpolation
//!    and no smoothing. A value moves when, and only when, a chunk finished.
//!    A smooth bar that is lying is precisely the defect being removed.
//!  * **Monotonic, and never past the end.** The index advances by one per
//!    completion and is clamped at the total; the cumulative word count can
//!    only grow. Words already decoded cannot un-decode.
//!  * **Silent once the take is over.** After [`ChunkProgress::finish`] nothing
//!    is ever emitted again, so a late chunk cannot repaint a pill that has
//!    already moved on to the transcript.
//!
//! The frontend half of the contract lives in `desktop/src/pill/live.ts`
//! (`acceptProgress`, which re-checks every one of these rules on the receiving
//! end, because an event is data and not a promise).

use serde::Serialize;

/// The event name both pills listen for. One per COMPLETED chunk.
pub const TRANSCRIBE_PROGRESS_EVENT: &str = "transcribe_progress";

/// Fewer chunks than this is not progress, it is a single decode. Mirrors
/// `MIN_REPORTABLE_CHUNKS` in `live.ts`; the two must agree or the frontend
/// would drop events the backend thought were worth sending.
pub const MIN_REPORTABLE_CHUNKS: u32 = 2;

/// The payload. `words_so_far` is REAL decoded words — never an estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TranscribeProgress {
    /// 1-based index of the chunk that just completed.
    pub chunk: u32,
    /// Total chunks this take was split into. Never reported below 2.
    pub of: u32,
    /// Cumulative real words in everything decoded so far.
    pub words_so_far: u32,
}

/// Where a progress event goes. A trait so the contract above is a unit test
/// rather than a thing you have to run an app to observe.
pub trait ProgressSink {
    fn progress(&self, event: TranscribeProgress);
}

/// The production sink: emit on the app handle, exactly like every other pill
/// event. A failed emit is not worth failing a take over — the decode is the
/// user's words, the progress bar is decoration on top of them.
pub struct AppProgress(pub tauri::AppHandle);

impl ProgressSink for AppProgress {
    fn progress(&self, event: TranscribeProgress) {
        use tauri::Emitter;
        let _ = self.0.emit(TRANSCRIBE_PROGRESS_EVENT, event);
    }
}

/// A sink that goes nowhere — headless runs (`--transcribe-file`) and the
/// command-mode path, neither of which has a pill in front of them.
pub struct NoProgress;

impl ProgressSink for NoProgress {
    fn progress(&self, _event: TranscribeProgress) {}
}

/// Real decoded words: whitespace-separated tokens, the same thing the
/// transcript row's word count means.
pub fn word_count(text: &str) -> u32 {
    text.split_whitespace().count() as u32
}

/// Reports progress through a decode of `of` chunks.
///
/// Construct it with the number of decode windows the caller is ACTUALLY going
/// to run — not a prediction, not a guess off the take's duration. The only
/// production constructor is [`TakeProgress::planned`], which is called from
/// inside Y3-B's window loop in `transcription.rs` with `windows.len()` AFTER
/// the planner has produced them, so the total can never be a guess off the
/// take's duration. A take the planner did not split never reaches that call
/// and therefore reports nothing at all.
pub struct ChunkProgress<'a> {
    of: u32,
    chunk: u32,
    words: u32,
    closed: bool,
    emitted: u32,
    sink: &'a dyn ProgressSink,
}

impl<'a> ChunkProgress<'a> {
    pub fn new(of: u32, sink: &'a dyn ProgressSink) -> Self {
        Self {
            of,
            chunk: 0,
            words: 0,
            closed: false,
            emitted: 0,
            sink,
        }
    }

    /// Will anything at all be reported? False for a single-window take.
    pub fn reports(&self) -> bool {
        self.of >= MIN_REPORTABLE_CHUNKS
    }

    /// How many events have actually gone out. The test surface for "one per
    /// completed chunk, and none for a single window".
    pub fn emitted(&self) -> u32 {
        self.emitted
    }

    /// One chunk finished, and here is the text it produced.
    ///
    /// Emits nothing when the take was a single window, when the take has
    /// already been closed out, or when every chunk has already been reported —
    /// the index can never run past `of`, so the fill can never pass 100%.
    pub fn chunk_done(&mut self, text: &str) {
        if self.closed || !self.reports() || self.chunk >= self.of {
            return;
        }
        self.chunk += 1;
        self.words = self.words.saturating_add(word_count(text));
        self.emitted += 1;
        self.sink.progress(TranscribeProgress {
            chunk: self.chunk,
            of: self.of,
            words_so_far: self.words,
        });
    }

    /// The take is over. Nothing may be emitted after this, ever — a late chunk
    /// must not repaint a pill that has already shown the transcript.
    pub fn finish(&mut self) {
        self.closed = true;
    }
}

/// What the decode loop reports to, so `transcription.rs` never has to know
/// what a pill is. Two calls, in this order and no other: `planned` once when
/// the window plan exists, then `chunk_done` once per window that really
/// finished. A decode that is never split calls neither.
pub trait TakeObserver {
    /// The planner produced `windows` decode windows. Called at most once, and
    /// never for a take that runs as a single unsplit decode.
    fn planned(&mut self, windows: usize);
    /// One window finished, and `text` is what it really decoded.
    fn chunk_done(&mut self, text: &str);
}

/// The observer for every decode with no pill in front of it — the headless
/// `--transcribe-file` path, command mode, and the Y3-B chunking tests.
pub struct NoObserver;

impl TakeObserver for NoObserver {
    fn planned(&mut self, _windows: usize) {}
    fn chunk_done(&mut self, _text: &str) {}
}

/// The dictation observer: holds the sink across a decode whose window count is
/// not known until the planner has run.
///
/// [`ChunkProgress`] needs its total up front, and the total only exists inside
/// the loop — so this defers construction to [`planned`](Self::planned) rather
/// than letting a caller invent a number before the plan exists. Until
/// `planned` fires there is no inner reporter and every call is a no-op, which
/// is exactly the single-window case.
pub struct TakeProgress<'a> {
    sink: &'a dyn ProgressSink,
    inner: Option<ChunkProgress<'a>>,
}

impl<'a> TakeProgress<'a> {
    pub fn new(sink: &'a dyn ProgressSink) -> Self {
        Self { sink, inner: None }
    }

    /// How many events actually went out. 0 for a single-window take.
    pub fn emitted(&self) -> u32 {
        self.inner.as_ref().map_or(0, ChunkProgress::emitted)
    }

    /// The take is over. Safe before `planned`, and idempotent.
    pub fn finish(&mut self) {
        if let Some(inner) = self.inner.as_mut() {
            inner.finish();
        }
    }
}

impl TakeObserver for TakeProgress<'_> {
    fn planned(&mut self, windows: usize) {
        // Re-planning mid-take is not a thing the loop does; if it ever were,
        // restarting the count backwards would break monotonicity, so the first
        // plan wins.
        if self.inner.is_none() {
            self.inner = Some(ChunkProgress::new(
                windows.min(u32::MAX as usize) as u32,
                self.sink,
            ));
        }
    }

    fn chunk_done(&mut self, text: &str) {
        if let Some(inner) = self.inner.as_mut() {
            inner.chunk_done(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Spy(Mutex<Vec<TranscribeProgress>>);
    impl ProgressSink for Spy {
        fn progress(&self, e: TranscribeProgress) {
            self.0.lock().unwrap().push(e);
        }
    }

    #[test]
    fn word_count_is_whitespace_tokens() {
        assert_eq!(word_count("  two   words  "), 2);
        assert_eq!(word_count(""), 0);
    }

    #[test]
    fn a_single_window_take_reports_nothing() {
        let spy = Spy::default();
        let mut p = ChunkProgress::new(1, &spy);
        assert!(!p.reports());
        p.chunk_done("some words here");
        p.finish();
        assert_eq!(p.emitted(), 0);
        assert!(spy.0.lock().unwrap().is_empty());
    }

    #[test]
    fn a_take_that_was_never_planned_emits_nothing() {
        let spy = Spy::default();
        let mut p = TakeProgress::new(&spy);
        p.chunk_done("words that never happened");
        p.finish();
        assert_eq!(p.emitted(), 0);
        assert!(spy.0.lock().unwrap().is_empty());
    }

    #[test]
    fn a_planned_take_reports_one_event_per_completed_chunk() {
        let spy = Spy::default();
        let mut p = TakeProgress::new(&spy);
        p.planned(3);
        p.chunk_done("one two");
        p.chunk_done("three");
        p.finish();
        p.chunk_done("too late");
        let seen = spy.0.lock().unwrap().clone();
        assert_eq!(seen.len(), 2);
        assert_eq!(
            seen[0],
            TranscribeProgress {
                chunk: 1,
                of: 3,
                words_so_far: 2
            }
        );
        assert_eq!(
            seen[1],
            TranscribeProgress {
                chunk: 2,
                of: 3,
                words_so_far: 3
            }
        );
    }

    #[test]
    fn the_index_never_runs_past_of() {
        let spy = Spy::default();
        let mut p = ChunkProgress::new(2, &spy);
        for _ in 0..5 {
            p.chunk_done("a b");
        }
        let seen = spy.0.lock().unwrap().clone();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1].chunk, 2);
    }
}
