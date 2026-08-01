//! Voice isolation / Silero VAD (YV36) — a WARM, load-once speech detector.
//!
//! Modeled on Handy's VAD module (`~/oss/handy/src-tauri/src/audio_toolkit/vad/`):
//! the `vad-rs` crate (Silero **v4** through `ort`, statically linked — nothing
//! to install) behind a `SmoothedVad` state machine, fed 30 ms / 480-sample
//! frames at 16 kHz with a 0.3 speech threshold.
//!
//! Why this replaces the previous tract path (YV29): tract could never even
//! ANALYSE the Silero v5 graph ("Failed analyse for node #5 If_0"), so voice
//! isolation had never once run — and because the model was loaded inside the
//! per-clip call, every single dictation paid a disk read + a graph analysis
//! just to fail again (155 identical warnings in one yap.log). Here the engine
//! is built ONCE at startup (see `run()` in lib.rs) and held in the app state;
//! a dictation only borrows it, resets the recurrent state, and streams frames.
//!
//! Two things come out of ONE pass over the finished clip: (a) a stronger
//! no-speech gate — reject a clip Silero finds no speech in, catching steady
//! noise the energy RMS gate scores as voiced; and (b) trimming the audio handed
//! to ASR down to the voiced span so Whisper never decodes (and hallucinates on)
//! leading/trailing non-speech. The energy VAD in `record.rs` stays exactly as
//! it was — it still owns `voiced_seconds` (the WPM denominator) and is the
//! explicit fallback whenever the model file is absent or the engine fails.
//!
//! The smoothing state machine and the segment→gate/trim decisions are pure and
//! unit-tested below; only the ONNX inference needs the model asset.

use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use vad_rs::Vad;

use crate::models;

/// Silero VAD's fixed operating rate (the clip is always written at 16 kHz).
pub const VAD_SR: u32 = 16_000;
/// Analysis frame: 30 ms at 16 kHz = 480 samples (Handy's frame size).
const FRAME_SAMPLES: usize = (VAD_SR as usize * 30) / 1000;
/// Per-frame speech-probability threshold — Handy's `VAD_THRESHOLD`.
const SPEECH_THRESHOLD: f32 = 0.3;
/// Frames of pre-roll released when speech starts (~450 ms), so a soft onset
/// isn't clipped off the front of the utterance.
const PREFILL_FRAMES: usize = 15;
/// Frames of speech kept after the last voiced frame (~450 ms), so a trailing
/// consonant or a natural inter-word pause doesn't end the utterance.
const HANGOVER_FRAMES: usize = 15;
/// Consecutive voiced frames required to declare speech — rejects single-frame
/// blips (a click, a keystroke).
const ONSET_FRAMES: usize = 2;
/// Minimum total voiced samples for a clip to count as containing speech (the
/// gate). ~150 ms at 16 kHz — below this it's a click/noise-only tap.
const MIN_SPEECH_SAMPLES: usize = VAD_SR as usize * 150 / 1000;
/// Extra padding (in frames) around the trimmed voiced span, on top of the
/// prefill/hangover the state machine already applied.
const PAD_FRAMES: usize = 3;

// ── The frame detector ──────────────────────────────────────────────────────

/// Anything that answers "is this 30 ms frame speech?". The real implementation
/// is [`SileroVad`]; the trait exists so the smoothing state machine below can
/// be unit-tested on scripted frame sequences without the model asset.
trait FrameVad {
    fn is_voice(&mut self, frame: &[f32]) -> Result<bool, String>;
    fn reset(&mut self);
}

/// Silero v4 via `vad-rs`. Holds the ONNX session (and its recurrent LSTM
/// state) for the process lifetime — constructing this is the expensive part,
/// and it happens exactly once.
struct SileroVad {
    engine: Vad,
    threshold: f32,
}

impl SileroVad {
    fn new(model_path: &Path, threshold: f32) -> Result<Self, String> {
        let engine =
            Vad::new(model_path, VAD_SR as usize).map_err(|e| format!("load silero vad: {e}"))?;
        Ok(Self { engine, threshold })
    }
}

impl FrameVad for SileroVad {
    fn is_voice(&mut self, frame: &[f32]) -> Result<bool, String> {
        if frame.len() != FRAME_SAMPLES {
            return Err(format!(
                "expected {FRAME_SAMPLES} samples, got {}",
                frame.len()
            ));
        }
        let result = self
            .engine
            .compute(frame)
            .map_err(|e| format!("silero compute: {e}"))?;
        Ok(result.prob > self.threshold)
    }

    fn reset(&mut self) {
        // Clear the LSTM hidden/cell state so a new dictation never inherits
        // recurrent context from the previous one.
        self.engine.reset();
    }
}

// ── The smoothing state machine ─────────────────────────────────────────────

/// What the state machine decided about one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameDecision {
    /// Non-speech (silence, noise).
    Noise,
    /// Speech. `prefill` is how many frames IMMEDIATELY BEFORE this one are
    /// retroactively speech too — the buffered pre-roll released at onset (0 on
    /// every frame except the onset frame).
    Speech { prefill: usize },
}

/// Handy's `SmoothedVad`: wraps a raw per-frame detector with onset counting
/// (N consecutive voiced frames before speech starts), a prefill pre-roll and a
/// hangover tail. Pure apart from the wrapped detector → unit-tested below.
struct SmoothedVad<V: FrameVad> {
    inner: V,
    prefill_frames: usize,
    hangover_frames: usize,
    onset_frames: usize,

    /// Frames pushed so far — bounds the pre-roll at the start of a clip.
    seen: usize,
    hangover_counter: usize,
    onset_counter: usize,
    in_speech: bool,
}

impl<V: FrameVad> SmoothedVad<V> {
    fn new(inner: V, prefill_frames: usize, hangover_frames: usize, onset_frames: usize) -> Self {
        Self {
            inner,
            prefill_frames,
            hangover_frames,
            onset_frames,
            seen: 0,
            hangover_counter: 0,
            onset_counter: 0,
            in_speech: false,
        }
    }

    fn push_frame(&mut self, frame: &[f32]) -> Result<FrameDecision, String> {
        let is_voice = self.inner.is_voice(frame)?;
        let preceding = self.seen;
        self.seen += 1;

        Ok(match (self.in_speech, is_voice) {
            // Potential start of speech — accumulate onset frames first.
            (false, true) => {
                self.onset_counter += 1;
                if self.onset_counter >= self.onset_frames.max(1) {
                    self.in_speech = true;
                    self.hangover_counter = self.hangover_frames;
                    self.onset_counter = 0;
                    // Release the pre-roll: the buffered frames before this one
                    // (which includes the frames spent proving the onset).
                    FrameDecision::Speech {
                        prefill: self.prefill_frames.min(preceding),
                    }
                } else {
                    FrameDecision::Noise
                }
            }
            // Ongoing speech — refresh the hangover tail.
            (true, true) => {
                self.hangover_counter = self.hangover_frames;
                FrameDecision::Speech { prefill: 0 }
            }
            // End of speech: keep emitting speech until the tail runs out.
            (true, false) => {
                if self.hangover_counter > 0 {
                    self.hangover_counter -= 1;
                    FrameDecision::Speech { prefill: 0 }
                } else {
                    self.in_speech = false;
                    FrameDecision::Noise
                }
            }
            // Silence, or a broken onset sequence.
            (false, false) => {
                self.onset_counter = 0;
                FrameDecision::Noise
            }
        })
    }

    fn reset(&mut self) {
        self.inner.reset();
        self.seen = 0;
        self.hangover_counter = 0;
        self.onset_counter = 0;
        self.in_speech = false;
    }
}

/// Run a clip through a smoothed detector → one keep/drop flag per 480-sample
/// frame, with the onset pre-roll backfilled onto the preceding frames. Pure
/// over the wrapped detector → unit-tested on scripted sequences.
fn speech_mask<V: FrameVad>(
    vad: &mut SmoothedVad<V>,
    samples: &[f32],
) -> Result<Vec<bool>, String> {
    let n_frames = samples.len() / FRAME_SAMPLES;
    let mut mask = vec![false; n_frames];
    for f in 0..n_frames {
        let base = f * FRAME_SAMPLES;
        match vad.push_frame(&samples[base..base + FRAME_SAMPLES])? {
            FrameDecision::Noise => {}
            FrameDecision::Speech { prefill } => {
                mask[f - prefill.min(f)..=f].fill(true);
            }
        }
    }
    Ok(mask)
}

// ── Segments → gate + trim decision ─────────────────────────────────────────

/// A contiguous voiced region in sample indices `[start, end)` over the clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Segment {
    start: usize,
    end: usize,
}

/// Collapse a per-frame voiced mask into voiced [`Segment`]s (in SAMPLE
/// indices). No gap-bridging or blip-dropping happens here — the smoothing
/// state machine above already did both (hangover bridges pauses, onset drops
/// blips), and doing it twice would smear the span. Pure → unit tested.
fn mask_to_segments(mask: &[bool], frame: usize) -> Vec<Segment> {
    if mask.is_empty() || frame == 0 {
        return Vec::new();
    }
    let mut segs = Vec::new();
    let mut i = 0;
    while i < mask.len() {
        if mask[i] {
            let start = i;
            while i < mask.len() && mask[i] {
                i += 1;
            }
            segs.push(Segment {
                start: start * frame,
                end: i * frame,
            });
        } else {
            i += 1;
        }
    }
    segs
}

/// Total voiced samples across all segments.
fn segments_voiced_samples(segs: &[Segment]) -> usize {
    segs.iter().map(|s| s.end.saturating_sub(s.start)).sum()
}

/// (a) Gate decision: does the clip hold at least `min_samples` of voiced audio?
fn has_speech(segs: &[Segment], min_samples: usize) -> bool {
    segments_voiced_samples(segs) >= min_samples
}

/// The padded voiced span [first_start-pad, last_end+pad) clamped to `[0, len)`,
/// or `None` when there are no segments. Pure → unit tested.
fn voiced_span(segs: &[Segment], len: usize, pad: usize) -> Option<(usize, usize)> {
    if segs.is_empty() || len == 0 {
        return None;
    }
    let first = segs.iter().map(|s| s.start).min().unwrap_or(0);
    let last = segs.iter().map(|s| s.end).max().unwrap_or(len);
    let start = first.saturating_sub(pad);
    let end = last.saturating_add(pad).min(len);
    if end <= start {
        return None;
    }
    Some((start, end))
}

/// (b) Trim a buffer to its padded voiced span. If there are NO segments the
/// input is returned UNCHANGED (never lose the utterance). Pure → unit tested.
fn trim_to_voiced(samples: &[f32], segs: &[Segment], pad: usize) -> Vec<f32> {
    match voiced_span(segs, samples.len(), pad) {
        Some((start, end)) => samples[start..end].to_vec(),
        None => samples.to_vec(),
    }
}

/// Result of one isolation pass over one clip.
pub struct Isolation {
    /// (a) Gate: true when Silero found ≥ the minimum voiced audio.
    pub has_speech: bool,
    /// Silero-measured voiced seconds (telemetry / logging only — the WPM
    /// denominator stays the energy VAD's `voiced_seconds`).
    pub voiced_seconds: f64,
    /// (b) The trimmed-to-voiced buffer to write back for ASR — `None` when
    /// nothing should be rewritten (no speech, or trim == whole clip).
    pub trimmed: Option<Vec<f32>>,
}

// ── The warm handle held by the app state ───────────────────────────────────

/// The load-once Silero VAD. Built during startup and stored in `AppState`;
/// dictations borrow it, so the ONNX session is never re-created on the hot
/// path. The mutex is uncontended in practice (one dictation at a time) and
/// exists because the engine carries recurrent state.
pub struct WarmVad {
    engine: Mutex<SmoothedVad<SileroVad>>,
}

impl WarmVad {
    /// Load the model ONCE. Returns `Err` on any model/session failure — the
    /// caller keeps running on the energy VAD alone.
    pub fn load(model_path: &Path) -> Result<Self, String> {
        let silero = SileroVad::new(model_path, SPEECH_THRESHOLD)?;
        Ok(Self {
            engine: Mutex::new(SmoothedVad::new(
                silero,
                PREFILL_FRAMES,
                HANGOVER_FRAMES,
                ONSET_FRAMES,
            )),
        })
    }

    /// Full isolation pass over a finished 16 kHz mono clip: stream frames
    /// through the warm engine, collapse the smoothed mask into voiced
    /// segments, then compute the gate decision + the trimmed-to-voiced buffer.
    /// Returns `Err` on inference failure (caller falls back to the energy VAD).
    pub fn isolate(&self, samples: &[f32], rate: u32) -> Result<Isolation, String> {
        if rate != VAD_SR {
            // The clip is written at 16 kHz; anything else means an unexpected path.
            return Err(format!("silero needs {VAD_SR} Hz, got {rate}"));
        }
        if samples.len() < FRAME_SAMPLES {
            return Err("clip shorter than one VAD frame".into());
        }
        let mut engine = self.engine.lock();
        // Fresh recurrent + smoothing state per dictation.
        engine.reset();
        let mask = speech_mask(&mut engine, samples)?;
        drop(engine);

        let segs = mask_to_segments(&mask, FRAME_SAMPLES);
        let voiced_samples = segments_voiced_samples(&segs);
        let has = has_speech(&segs, MIN_SPEECH_SAMPLES);
        let voiced_seconds = voiced_samples as f64 / rate as f64;
        // Only propose a trim when there IS speech and the span is a real subset
        // of the clip (otherwise leave the clip untouched — never lose the
        // utterance).
        let pad = FRAME_SAMPLES * PAD_FRAMES;
        let trimmed = if has {
            match voiced_span(&segs, samples.len(), pad) {
                Some((s, e)) if e - s < samples.len() => Some(trim_to_voiced(samples, &segs, pad)),
                _ => None,
            }
        } else {
            None
        };
        Ok(Isolation {
            has_speech: has,
            voiced_seconds,
            trimmed,
        })
    }
}

// ── The model asset ─────────────────────────────────────────────────────────

/// Where the Silero VAD model asset lives under Application Support.
pub fn model_path(data_dir: &Path) -> PathBuf {
    data_dir.join("models").join("silero_vad_v4.onnx")
}

/// Direct (NON-HuggingFace, per YV20) URL for the small Silero VAD ONNX model.
/// The graph is the **v4** release — the one `vad-rs` drives (inputs
/// `input`/`sr`/`h`/`c`), byte-identical to the model Handy ships.
///
/// YV45: the path is the immutable COMMIT the `v4.0` tag points at, not the tag
/// itself. A tag can be moved; a commit sha cannot, so the URL and the hash
/// below describe exactly one artifact forever.
const SILERO_MODEL_URL: &str = "https://github.com/snakers4/silero-vad/raw/915dd3d639b8333a52e001af095f87c5b7f1e0ac/files/silero_vad.onnx";
/// sha256 of that exact file — the trust anchor, same posture as the ASR model
/// catalog. A wrong-version model is what made the old tract path fail forever,
/// so it is checked before the file is ever installed AND every time a cached
/// copy is picked up.
const SILERO_MODEL_SHA256: &str =
    "a35ebf52fd3ce5f1469b2a36158dba761bc47b973ea3382b3186ca15b1f5af28";

/// The already-downloaded model — but only while it still hashes to
/// [`SILERO_MODEL_SHA256`] (YV45). This is the same rule `models::download_file`
/// applies to an ASR model already on disk: what is trusted is the pin, not the
/// filename. A truncated, edited or older file is deleted and re-fetched rather
/// than handed to the ONNX runtime. ~1.8 MB to hash, off the hot path.
fn cached_model(path: &Path) -> Option<PathBuf> {
    if !path.is_file() {
        return None;
    }
    match models::verify_sha256(path, SILERO_MODEL_SHA256) {
        Ok(()) => Some(path.to_path_buf()),
        Err(e) => {
            log::warn!("YV45 cached silero model rejected, re-fetching: {e}");
            let _ = std::fs::remove_file(path);
            None
        }
    }
}

/// Ensure the Silero VAD model is cached under Application Support, returning
/// its path when present. Downloads ONCE via the system `curl` from a pinned
/// direct URL if missing (best-effort, offline-safe: on any failure we return
/// `None` and the pipeline falls back to the energy VAD). After the first
/// success it is OFFLINE forever — the verified cached file is used and never
/// re-fetched. Call off the hot path (e.g. the startup background thread).
pub fn ensure_model(data_dir: &Path) -> Option<PathBuf> {
    let path = model_path(data_dir);
    // Existing installs cached the unusable v5 graph (YV29) — drop it rather
    // than leave 2 MB of dead weight in Application Support forever.
    let _ = std::fs::remove_file(data_dir.join("models").join("silero_vad.onnx"));
    if let Some(verified) = cached_model(&path) {
        return Some(verified);
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Best-effort single download; never blocks the utterance, never panics.
    let tmp = path.with_extension("onnx.part");
    let status = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "30",
            "-o",
            &tmp.to_string_lossy(),
            SILERO_MODEL_URL,
        ])
        .status();
    match status {
        Ok(s) if s.success() => {
            if let Err(e) = models::verify_sha256(&tmp, SILERO_MODEL_SHA256) {
                let _ = std::fs::remove_file(&tmp);
                log::warn!("YV36 silero model rejected: {e}");
                return None;
            }
            if std::fs::rename(&tmp, &path).is_ok() {
                log::info!("YV36 silero v4 model cached at {}", path.display());
                Some(path)
            } else {
                let _ = std::fs::remove_file(&tmp);
                None
            }
        }
        other => {
            let _ = std::fs::remove_file(&tmp);
            log::warn!("YV36 silero model download skipped/failed: {other:?}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scripted stand-in for [`SileroVad`]: replays a fixed voiced/unvoiced
    /// verdict per frame, so the smoothing state machine can be driven through
    /// onset, hangover and silence deterministically — no model, no I/O.
    struct ScriptedVad {
        script: Vec<bool>,
        idx: usize,
        resets: usize,
    }

    impl ScriptedVad {
        fn new(script: Vec<bool>) -> Self {
            Self {
                script,
                idx: 0,
                resets: 0,
            }
        }
    }

    impl FrameVad for ScriptedVad {
        fn is_voice(&mut self, _frame: &[f32]) -> Result<bool, String> {
            let v = *self.script.get(self.idx).unwrap_or(&false);
            self.idx += 1;
            Ok(v)
        }
        fn reset(&mut self) {
            self.idx = 0;
            self.resets += 1;
        }
    }

    /// Drive the state machine over a scripted sequence with the given tuning →
    /// the per-frame mask (pre-roll backfilled), exactly as `isolate` builds it.
    fn run(script: Vec<bool>, prefill: usize, hangover: usize, onset: usize) -> Vec<bool> {
        let frames = script.len();
        let mut vad = SmoothedVad::new(ScriptedVad::new(script), prefill, hangover, onset);
        let samples = vec![0.0f32; frames * FRAME_SAMPLES];
        speech_mask(&mut vad, &samples).expect("scripted vad never fails")
    }

    // ── Smoothing state machine (YV36) ──────────────────────────────────────

    #[test]
    fn silence_only_never_reports_speech() {
        assert_eq!(run(vec![false; 12], 2, 3, 2), vec![false; 12]);
    }

    #[test]
    fn onset_requires_consecutive_voiced_frames() {
        // Frame 3 alone is voiced (onset=2 needs two in a row) → dropped.
        let mask = run(
            vec![false, false, false, true, false, false, false],
            0,
            0,
            2,
        );
        assert_eq!(mask, vec![false; 7], "a 1-frame blip must not open speech");
    }

    #[test]
    fn speech_opens_on_the_onset_frame_and_backfills_prefill() {
        // voiced from frame 2; onset=2 → speech declared on frame 3, and the
        // 2-frame pre-roll retroactively marks frames 1..=2 as speech.
        let mask = run(vec![false, false, true, true, true], 2, 0, 2);
        assert_eq!(mask, vec![false, true, true, true, true]);
    }

    #[test]
    fn prefill_is_bounded_by_the_start_of_the_clip() {
        // Speech from the very first frame with a 15-frame pre-roll: the
        // backfill must clamp at 0 instead of underflowing.
        let mask = run(vec![true, true, true], 15, 0, 2);
        assert_eq!(mask, vec![true, true, true]);
    }

    #[test]
    fn hangover_keeps_speech_through_a_short_pause_then_closes() {
        // 3 voiced, then silence. hangover=2 → two silent frames stay speech,
        // the third closes it.
        let mask = run(vec![true, true, true, false, false, false, false], 0, 2, 2);
        assert_eq!(
            mask,
            vec![false, true, true, true, true, false, false],
            "hangover must bridge the tail then release"
        );
    }

    #[test]
    fn hangover_bridges_an_inter_word_gap_into_one_segment() {
        // speech | 2-frame gap (≤ hangover) | speech → ONE continuous region.
        let mask = run(
            vec![
                true, true, true, false, false, true, true, false, false, false,
            ],
            0,
            2,
            2,
        );
        let segs = mask_to_segments(&mask, FRAME_SAMPLES);
        assert_eq!(segs.len(), 1, "short gap must not split speech: {segs:?}");
        assert_eq!(
            segs[0],
            Segment {
                start: FRAME_SAMPLES,
                end: 9 * FRAME_SAMPLES
            }
        );
    }

    #[test]
    fn long_gap_splits_into_two_segments() {
        // A gap longer than the hangover ends the utterance; the second burst
        // has to re-earn its onset.
        let mut script = vec![true; 4];
        script.extend(vec![false; 6]);
        script.extend(vec![true; 4]);
        let mask = run(script, 0, 2, 2);
        let segs = mask_to_segments(&mask, FRAME_SAMPLES);
        assert_eq!(segs.len(), 2, "a long gap must split, got {segs:?}");
    }

    #[test]
    fn reset_clears_smoothing_state_between_dictations() {
        let mut vad = SmoothedVad::new(ScriptedVad::new(vec![true, true, true]), 15, 15, 2);
        let samples = vec![0.0f32; 3 * FRAME_SAMPLES];
        let first = speech_mask(&mut vad, &samples).unwrap();
        assert!(vad.in_speech, "still inside speech after the burst");
        vad.reset();
        assert!(!vad.in_speech);
        assert_eq!(vad.seen, 0);
        assert_eq!(vad.hangover_counter, 0);
        assert_eq!(vad.inner.resets, 1, "the ONNX LSTM state is reset too");
        // A second dictation replays identically instead of inheriting state.
        assert_eq!(speech_mask(&mut vad, &samples).unwrap(), first);
    }

    // ── Segments → gate / trim decision ─────────────────────────────────────

    const FR: usize = FRAME_SAMPLES;

    #[test]
    fn mask_to_segments_collapses_runs_into_sample_spans() {
        let mask = vec![false, true, true, false, true, false];
        assert_eq!(
            mask_to_segments(&mask, FR),
            vec![
                Segment {
                    start: FR,
                    end: 3 * FR
                },
                Segment {
                    start: 4 * FR,
                    end: 5 * FR
                },
            ]
        );
    }

    #[test]
    fn mask_to_segments_is_safe_on_empty_input() {
        assert!(mask_to_segments(&[], FR).is_empty());
        assert!(mask_to_segments(&[true, true], 0).is_empty());
        assert!(mask_to_segments(&[false; 8], FR).is_empty());
    }

    #[test]
    fn has_speech_honors_the_minimum_voiced_samples() {
        // Below the 150 ms floor → not speech; above it → speech.
        let tiny = vec![Segment { start: 0, end: 100 }]; // 100 samples ≪ 2400
        assert!(!has_speech(&tiny, MIN_SPEECH_SAMPLES));
        let enough = vec![Segment {
            start: 0,
            end: MIN_SPEECH_SAMPLES + 1,
        }];
        assert!(has_speech(&enough, MIN_SPEECH_SAMPLES));
        assert!(!has_speech(&[], MIN_SPEECH_SAMPLES), "empty → no speech");
    }

    #[test]
    fn voiced_span_pads_and_clamps_to_bounds() {
        let segs = vec![Segment {
            start: 1000,
            end: 2000,
        }];
        // Pad 500 each side, well within [0, 5000).
        assert_eq!(voiced_span(&segs, 5000, 500), Some((500, 2500)));
        // Pad that would run past the edges is clamped, never out of range.
        assert_eq!(voiced_span(&segs, 5000, 5000), Some((0, 5000)));
        // No segments → no span.
        assert_eq!(voiced_span(&[], 5000, 500), None);
    }

    #[test]
    fn trim_keeps_only_the_voiced_span() {
        // 500 silence | 1000 "speech" (0.7) | 500 silence, one segment over the
        // middle. Trim with no padding must keep exactly the voiced middle.
        let mut buf = vec![0.0f32; 500];
        buf.extend(vec![0.7f32; 1000]);
        buf.extend(vec![0.0f32; 500]);
        let segs = vec![Segment {
            start: 500,
            end: 1500,
        }];
        let trimmed = trim_to_voiced(&buf, &segs, 0);
        assert_eq!(trimmed.len(), 1000, "leading/trailing silence removed");
        assert!(
            trimmed.iter().all(|&s| (s - 0.7).abs() < 1e-6),
            "kept the voiced body"
        );
    }

    #[test]
    fn trim_with_no_segments_returns_input_unchanged() {
        // NEVER lose the utterance: no detected speech → return the clip as-is so
        // the energy-VAD gate decides, and ASR still sees the full audio.
        let buf = vec![0.3f32; 800];
        assert_eq!(trim_to_voiced(&buf, &[], PAD_FRAMES * FR), buf);
    }

    #[test]
    fn silero_model_is_pinned_to_one_immutable_artifact() {
        // YV45 supply chain: the model is fetched over the network and then run
        // as an inference graph in-process, so BOTH halves of the pin have to
        // stay exact — a commit-sha URL (never `master`/a tag/a branch) and a
        // full lowercase sha256 to check the bytes against.
        assert!(
            SILERO_MODEL_URL.starts_with("https://"),
            "model must be fetched over TLS: {SILERO_MODEL_URL}"
        );
        let rev = SILERO_MODEL_URL
            .split("/raw/")
            .nth(1)
            .and_then(|rest| rest.split('/').next())
            .expect("URL carries a /raw/<rev>/ segment");
        assert_eq!(
            rev.len(),
            40,
            "URL must pin a 40-char commit sha, got a moving ref: {rev}"
        );
        assert!(
            rev.chars().all(|c| c.is_ascii_hexdigit()),
            "URL must pin a commit sha, got: {rev}"
        );

        assert_eq!(
            SILERO_MODEL_SHA256.len(),
            64,
            "sha256 must be 64 hex chars: {SILERO_MODEL_SHA256}"
        );
        assert!(
            SILERO_MODEL_SHA256
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "sha256 must be lowercase hex: {SILERO_MODEL_SHA256}"
        );
    }

    #[test]
    fn cached_model_with_the_wrong_bytes_is_deleted_not_loaded() {
        // The trust rule: a file already on disk is only used while it still
        // hashes to the pin. An impostor must be rejected AND removed, so the
        // next `ensure_model` re-fetches instead of loading it forever (the old
        // check was size > 0, which any 20 bytes of garbage passed).
        let data_dir = std::env::temp_dir().join(format!(
            "yap-vad-pin-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = model_path(&data_dir);
        std::fs::create_dir_all(path.parent().unwrap()).expect("scratch dir");
        std::fs::write(&path, b"not the silero graph").expect("write impostor");

        assert!(cached_model(&path).is_none(), "impostor must be rejected");
        assert!(!path.exists(), "impostor must be deleted, not left cached");
        assert!(cached_model(&path).is_none(), "absent model is not cached");
        let _ = std::fs::remove_dir_all(&data_dir);
    }

    #[test]
    fn missing_model_file_yields_no_warm_vad() {
        // The explicit energy-VAD fallback: a model that isn't there must fail
        // loudly at load time (once), not silently every dictation.
        let missing = std::env::temp_dir().join("yap-no-such-silero-model.onnx");
        assert!(WarmVad::load(&missing).is_err());
    }

    // ── Real engine, real model (skipped when the asset isn't cached) ────────

    /// End-to-end proof that the ONNX graph actually RUNS (the thing tract could
    /// never do): the committed speech fixture must come back voiced and the
    /// same engine must then score digital silence as no-speech. The model is a
    /// runtime download, so a machine without it (CI) skips instead of failing —
    /// exactly the condition under which the app falls back to the energy VAD.
    #[test]
    fn real_model_finds_speech_in_the_fixture_and_none_in_silence() {
        let model = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("WilsonVoice")
            .join("models")
            .join("silero_vad_v4.onnx");
        if !model.exists() {
            eprintln!("skipping: no cached silero model at {}", model.display());
            return;
        }
        let warm = WarmVad::load(&model).expect("cached model loads");

        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("quick-brown-fox-16k.wav");
        let mut reader = hound::WavReader::open(&fixture).expect("fixture reads");
        let speech: Vec<f32> = reader
            .samples::<i16>()
            .map(|s| s.unwrap() as f32 / i16::MAX as f32)
            .collect();

        let iso = warm.isolate(&speech, VAD_SR).expect("inference runs");
        assert!(iso.has_speech, "the spoken fixture must register as speech");
        assert!(
            iso.voiced_seconds > 0.5,
            "expected most of the phrase voiced, got {:.2}s",
            iso.voiced_seconds
        );

        // Same warm engine, second dictation: silence must NOT inherit the
        // previous utterance's state.
        let silence = vec![0.0f32; VAD_SR as usize];
        let iso = warm.isolate(&silence, VAD_SR).expect("inference runs");
        assert!(
            !iso.has_speech,
            "digital silence must not register as speech"
        );
        assert_eq!(iso.voiced_seconds, 0.0);
    }
}
