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
}

impl PipelineSpans<'_> {
    /// The single machine-parsable INFO line. Stable key order; `latency` stays
    /// the first token so the existing `grep latency` habit keeps working.
    pub fn summary_line(&self) -> String {
        format!(
            "latency hold_clipboard_ms={} press_capture_ms={} capture_ms={} samples_ready_ms={} \
             asr_load_ms={} asr_decode_ms={} cleanup_ms={} paste_ms={} backend={} pasted={}",
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
        )
    }
}

#[cfg(test)]
mod tests {
    use super::PipelineSpans;

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
        };
        assert_eq!(
            warm.summary_line(),
            "latency hold_clipboard_ms=986 press_capture_ms=41 capture_ms=2503 \
             samples_ready_ms=118 asr_load_ms=0 asr_decode_ms=812 cleanup_ms=2 paste_ms=44 \
             backend=native pasted=true"
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
             backend=native pasted=false"
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
            ]
        );
    }
}
