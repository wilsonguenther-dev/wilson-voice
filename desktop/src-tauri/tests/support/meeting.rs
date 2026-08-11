//! Shared fixtures for the YV93 meeting-ASR test files.
//!
//! Compiled into each of `meeting_transcription_resume`,
//! `meeting_dictation_preempts_transcription` and
//! `meeting_chunk_timeout_isolation` via `#[path]` rather than living in the
//! crate: a stub decoder is test scaffolding and has no business shipping in
//! the binary. It sits under `tests/support/` so cargo does not build it as a
//! test target of its own.
//!
//! The one idea worth stating: **the audio carries its own clock.** Sample `i`
//! of the fixture holds the value `i / 16000` — the second it sits at — so a
//! decoder that is handed a bare `&[f32]` window can say exactly which part of
//! the meeting it was given, and a test can then prove that a resumed run never
//! re-decoded audio it had already done. Without that, "resumes from the last
//! completed chunk" is only checkable by counting calls, which a merge bug
//! would sail straight through.
#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use wilson_voice_lib::asr_engine::{TimedKind, TimedSpan, TimedTranscript};
use wilson_voice_lib::meeting_asr::{ChunkAsr, MemoryWindows, MEETING_RATE};

/// A meeting whose samples ARE its timeline: `samples[i] == i / rate`.
pub fn ramp_audio(seconds: f64) -> MemoryWindows {
    let n = (seconds * MEETING_RATE as f64) as usize;
    let samples: Vec<f32> = (0..n).map(|i| i as f32 / MEETING_RATE as f32).collect();
    MemoryWindows::at_meeting_rate(samples)
}

/// Where in the meeting a decoded window came from, read back off the ramp.
pub fn window_bounds(samples: &[f32]) -> (f64, f64) {
    let start = *samples.first().expect("a non-empty window") as f64;
    let end = *samples.last().expect("a non-empty window") as f64 + 1.0 / MEETING_RATE as f64;
    (start, end)
}

/// The word the fixture "says" in second `k`. One per second, unique, so a
/// duplicate or a deletion at a seam is visible by inspection.
pub fn word_at(second: usize) -> String {
    format!("w{second:04}")
}

/// Where word `k` sits: the MIDDLE of its second, never on a boundary. Chunk
/// boundaries land on whole seconds, and a word whose edge coincided with one
/// would make the test's own arithmetic the thing under test (f32 ramp values
/// are exact to ~1e-4 s, which is plenty to place a half-second centre and far
/// too little to place a boundary-hugging one).
pub fn word_centre(second: usize) -> f64 {
    second as f64 + 0.5
}

/// Every word of a `seconds`-long fixture, in order — the exact transcript a
/// correct chunker + merge must produce.
pub fn expected_words(seconds: usize) -> Vec<String> {
    (0..seconds).map(word_at).collect()
}

/// A decoder that "hears" the ramp: one word per whole second inside the
/// window it was handed, timestamped where that second actually is (relative to
/// the window, the way a real engine reports it).
pub struct RampDecoder {
    pub calls: Mutex<Vec<(f64, f64)>>,
    pub decoded: AtomicUsize,
    /// Emit word timestamps (the primary merge path) or text only (the
    /// text-anchor fallback path).
    pub timed: bool,
    /// Sleep this long per chunk, so a test can interleave something with a
    /// decode that is genuinely in flight.
    pub chunk_delay: std::time::Duration,
}

impl RampDecoder {
    pub fn new(timed: bool) -> RampDecoder {
        RampDecoder {
            calls: Mutex::new(Vec::new()),
            decoded: AtomicUsize::new(0),
            timed,
            chunk_delay: std::time::Duration::from_millis(0),
        }
    }

    pub fn with_delay(timed: bool, chunk_delay: std::time::Duration) -> RampDecoder {
        RampDecoder {
            chunk_delay,
            ..RampDecoder::new(timed)
        }
    }

    pub fn windows_decoded(&self) -> Vec<(f64, f64)> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The transcript of one window, as a real engine would report it.
    pub fn transcript_for(&self, start: f64, end: f64) -> TimedTranscript {
        let first = start.floor() as usize;
        let last = end.ceil() as usize;
        let mut words = Vec::new();
        let mut text = Vec::new();
        for second in first..last {
            let centre = word_centre(second);
            if centre <= start || centre >= end {
                continue;
            }
            text.push(word_at(second));
            words.push(TimedSpan {
                // Relative to the window, like the engine's own rows.
                start_seconds: centre - start - 0.2,
                end_seconds: centre - start + 0.2,
                text: word_at(second),
            });
        }
        TimedTranscript {
            text: text.join(" "),
            kind: if self.timed {
                TimedKind::Word
            } else {
                TimedKind::None
            },
            segments: Vec::new(),
            words: if self.timed { words } else { Vec::new() },
        }
    }
}

impl ChunkAsr for RampDecoder {
    fn transcribe_timed(&self, samples_16k_mono: &[f32]) -> Result<TimedTranscript, String> {
        let (start, end) = window_bounds(samples_16k_mono);
        self.calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((start, end));
        self.decoded.fetch_add(1, Ordering::AcqRel);
        if !self.chunk_delay.is_zero() {
            std::thread::sleep(self.chunk_delay);
        }
        Ok(self.transcript_for(start, end))
    }
}
