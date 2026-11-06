//! Per-dictation latency breakdown (YV40).
//!
//! The pipeline used to log ONE mostly-cumulative line
//! (`latency hold→clipboard=…ms (press→capture=… dsp=… asr_done=… asr_model=…)`),
//! which conflated stages: `asr_model` included the model load, `asr_done`
//! included the DSP finalize, and cleanup + paste were invisible entirely. The
//! foundation audit (`yap-latency`) called this out — "instrumentation exists
//! for release→wav, release→asr-done and total pipeline_ms, but there is zero
//! timing for press→capture-start, … or paste dispatch" — so tuning any stage
//! meant guessing which part of a cumulative number moved.
//!
//! [`PipelineSpans`] is the whole take as DISJOINT spans, each measured from a
//! monotonic `Instant` at its own boundary, rendered as one INFO line in a
//! stable `key=value` format so a session's logs can be grepped/parsed straight
//! into a latency table:
//!
//! ```text
//! latency hold_clipboard_ms=986 press_capture_ms=41 capture_ms=2503 samples_ready_ms=118 asr_load_ms=0 asr_decode_ms=812 cleanup_ms=2 paste_ms=44 backend=native pasted=true
//! ```
//!
//! Keys are never reordered, never dropped and never conditionally formatted —
//! a missing stage reports `0` rather than vanishing, so every line has the same
//! shape.
//!
//! SP-03 appends the decode REAL-TIME FACTOR to every line: `rtf_last` is this
//! take's `asr_decode_ms / audio_ms` (lower is faster; `0.050` = twenty times
//! faster than real time) and `rtf_p50` is the median over the last
//! [`RTF_WINDOW`] takes of this session ([`record_decode_rtf`]). SP-01 reads
//! them to size its windows; `wilson-voice --bench` prints the same ratio.

use std::collections::VecDeque;
use std::sync::Mutex;

/// How many recent takes the rolling decode RTF is taken over.
pub const RTF_WINDOW: usize = 10;

/// `decode_ms / audio_ms` — the decode real-time factor. `0.0` (never NaN or
/// infinity) for an empty or non-positive audio length, so a degenerate take
/// cannot poison the rolling window or the log line.
pub fn decode_rtf(decode_ms: i64, audio_seconds: f64) -> f64 {
    if !(audio_seconds.is_finite() && audio_seconds > 0.0) || decode_ms < 0 {
        return 0.0;
    }
    decode_ms as f64 / (audio_seconds * 1000.0)
}

/// The last [`RTF_WINDOW`] decode real-time factors, oldest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RtfWindow {
    recent: VecDeque<f64>,
}

impl RtfWindow {
    pub const fn new() -> Self {
        Self {
            recent: VecDeque::new(),
        }
    }

    /// Add one take. Non-finite or negative values are dropped, and the oldest
    /// take falls out once the window is full.
    pub fn push(&mut self, rtf: f64) {
        if !rtf.is_finite() || rtf < 0.0 {
            return;
        }
        if self.recent.len() == RTF_WINDOW {
            self.recent.pop_front();
        }
        self.recent.push_back(rtf);
    }

    /// The most recent take's RTF, `0.0` before the first take.
    pub fn last(&self) -> f64 {
        self.recent.back().copied().unwrap_or(0.0)
    }

    /// Median of the window (mean of the two middle values for an even count),
    /// `0.0` before the first take.
    pub fn p50(&self) -> f64 {
        if self.recent.is_empty() {
            return 0.0;
        }
        let mut sorted: Vec<f64> = self.recent.iter().copied().collect();
        sorted.sort_by(|a, b| a.total_cmp(b));
        // Odd count: both indices are the middle; even: the two middles.
        let n = sorted.len();
        (sorted[(n - 1) / 2] + sorted[n / 2]) / 2.0
    }
}

/// The session's rolling window, shared by every dictation take.
static SESSION_RTF: Mutex<RtfWindow> = Mutex::new(RtfWindow::new());

/// Record one take's decode into the session window and return
/// `(rtf_p50, rtf_last)` for its latency line.
pub fn record_decode_rtf(decode_ms: i64, audio_seconds: f64) -> (f64, f64) {
    let rtf = decode_rtf(decode_ms, audio_seconds);
    let mut window = SESSION_RTF.lock().unwrap_or_else(|p| p.into_inner());
    window.push(rtf);
    (window.p50(), rtf)
}

/// One dictation's latency, stage by stage. Every `*_ms` field is a WALL span in
/// milliseconds, taken between two monotonic instants on the live path; nothing
/// here is derived from another field.
pub struct PipelineSpans<'a> {
    /// North-star metric: hotkey RELEASE → transcript on the clipboard. The sum
    /// of the release-side spans below plus the small gaps between them.
    pub hold_clipboard_ms: i64,
    /// Key PRESS → the capture stream is live and buffering (includes the
    /// deliberate hold-arm wait for a non-tap press).
    pub press_capture_ms: i64,
    /// Capture itself: press → release wall time (how long the user held).
    pub capture_ms: i64,
    /// Release → samples ready: the DSP finalize (AGC flush, edge fade, optional
    /// denoise) plus VAD, i.e. everything before ASR can be dispatched.
    pub samples_ready_ms: i64,
    /// Model load into the warm engine — `0` on the warm path (already resident).
    pub asr_load_ms: i64,
    /// The decode itself, with a warm engine.
    pub asr_decode_ms: i64,
    /// Dictionary → backtrack → formatting → polish (the cleanup pipeline).
    pub cleanup_ms: i64,
    /// Paste dispatch → pasted: clipboard write, wrong-target check, ⌘V and the
    /// read receipt. Clipboard-only takes still spend time here (see `pasted`).
    pub paste_ms: i64,
    /// ASR backend the decode ran on.
    pub backend: &'a str,
    /// Did the take actually reach the frontmost app, or stop at the clipboard?
    pub pasted: bool,
    /// SP-03 — median decode RTF over the last [`RTF_WINDOW`] takes, this one
    /// included (see [`record_decode_rtf`]).
    pub rtf_p50: f64,
    /// SP-03 — this take's decode RTF (`asr_decode_ms / audio_ms`).
    pub rtf_last: f64,
}

impl PipelineSpans<'_> {
    /// The single machine-parsable INFO line. Stable key order; `latency` stays
    /// the first token so the existing `grep latency` habit keeps working.
    pub fn summary_line(&self) -> String {
        format!(
            "latency hold_clipboard_ms={} press_capture_ms={} capture_ms={} samples_ready_ms={} \
             asr_load_ms={} asr_decode_ms={} cleanup_ms={} paste_ms={} backend={} pasted={} \
             rtf_p50={:.3} rtf_last={:.3}",
            self.hold_clipboard_ms,
            self.press_capture_ms,
            self.capture_ms,
            self.samples_ready_ms,
            self.asr_load_ms,
            self.asr_decode_ms,
            self.cleanup_ms,
            self.paste_ms,
            self.backend,
            self.pasted,
            self.rtf_p50,
            self.rtf_last,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_rtf, PipelineSpans, RtfWindow};

    #[test]
    fn summary_line_is_stable_key_value_with_every_span() {
        // Warm take: the model is already resident, so asr_load_ms is 0 — it is
        // still PRINTED, because a parser must see the same keys on every line.
        let warm = PipelineSpans {
            hold_clipboard_ms: 986,
            press_capture_ms: 41,
            capture_ms: 2503,
            samples_ready_ms: 118,
            asr_load_ms: 0,
            asr_decode_ms: 812,
            cleanup_ms: 2,
            paste_ms: 44,
            backend: "native",
            pasted: true,
            rtf_p50: 0.0612,
            rtf_last: 0.05,
        };
        assert_eq!(
            warm.summary_line(),
            "latency hold_clipboard_ms=986 press_capture_ms=41 capture_ms=2503 \
             samples_ready_ms=118 asr_load_ms=0 asr_decode_ms=812 cleanup_ms=2 paste_ms=44 \
             backend=native pasted=true rtf_p50=0.061 rtf_last=0.050"
        );

        // Cold take (first dictation after launch / after the idle unload) —
        // same keys, same order, with the load span populated and a
        // clipboard-only outcome.
        let cold = PipelineSpans {
            asr_load_ms: 1470,
            pasted: false,
            ..warm
        };
        assert_eq!(
            cold.summary_line(),
            "latency hold_clipboard_ms=986 press_capture_ms=41 capture_ms=2503 \
             samples_ready_ms=118 asr_load_ms=1470 asr_decode_ms=812 cleanup_ms=2 paste_ms=44 \
             backend=native pasted=false rtf_p50=0.061 rtf_last=0.050"
        );

        // Every stage of the pipeline is named, and the line parses as pure
        // key=value pairs after the leading `latency` token.
        let line = warm.summary_line();
        let mut tokens = line.split(' ');
        assert_eq!(tokens.next(), Some("latency"));
        let keys: Vec<&str> = tokens
            .map(|kv| {
                kv.split_once('=')
                    .unwrap_or_else(|| panic!("token is not key=value: {kv}"))
                    .0
            })
            .collect();
        assert_eq!(
            keys,
            vec![
                "hold_clipboard_ms",
                "press_capture_ms",
                "capture_ms",
                "samples_ready_ms",
                "asr_load_ms",
                "asr_decode_ms",
                "cleanup_ms",
                "paste_ms",
                "backend",
                "pasted",
                "rtf_p50",
                "rtf_last",
            ]
        );
    }

    /// SP-03 — the rolling decode RTF: last is the newest take, p50 the median
    /// of at most RTF_WINDOW takes, the oldest falls out, junk never enters.
    #[test]
    fn rolling_rtf_window_keeps_ten_takes_and_reports_median_and_last() {
        let mut w = RtfWindow::new();
        assert_eq!((w.p50(), w.last()), (0.0, 0.0));

        w.push(0.3);
        w.push(0.1);
        assert_eq!(w.last(), 0.1);
        assert!(
            (w.p50() - 0.2).abs() < 1e-12,
            "even count = mean of middles"
        );
        w.push(0.2);
        assert!((w.p50() - 0.2).abs() < 1e-12);

        w.push(f64::NAN);
        w.push(f64::INFINITY);
        w.push(-1.0);
        // Non-finite / negative RTFs are dropped: nothing moved.
        assert_eq!(w.last(), 0.2);
        assert!((w.p50() - 0.2).abs() < 1e-12);

        for i in 0..20 {
            w.push(1.0 + i as f64);
        }
        assert_eq!(w.last(), 20.0);
        // The window holds only the last RTF_WINDOW (10) takes, 11.0..=20.0,
        // so the median is (15 + 16) / 2 — every older take has fallen out.
        assert!((w.p50() - 15.5).abs() < 1e-12);
    }

    #[test]
    fn decode_rtf_is_decode_over_audio_and_never_nan() {
        assert!((decode_rtf(500, 10.0) - 0.05).abs() < 1e-12);
        assert_eq!(decode_rtf(500, 0.0), 0.0);
        assert_eq!(decode_rtf(500, f64::NAN), 0.0);
        assert_eq!(decode_rtf(-1, 10.0), 0.0);
    }
}
