//! YV93 — meeting ASR: a VAD-cut chunker over the one warm engine, preemptible
//! for dictation and resumable after a crash.
//!
//! Four things live here, together because they are one code path:
//!
//! 1. **A windowed-only chunker.** The original plan's §3 diagram implied a
//!    per-turn ASR path; plan finding #11 killed it (per-turn pays full
//!    mel+encoder setup on every short run and throws away cross-turn context).
//!    What survives is windows — but cut on VAD **silence** inside a
//!    [[`ChunkConfig::min_seconds`], [`ChunkConfig::max_seconds`]] search window
//!    rather than on a fixed 30 s clock, so the cut falls BETWEEN two words and
//!    the seam has nothing to duplicate. Measured on the eval lecture: 28 of 28
//!    boundaries landed in a pause, none inside a word, and the primary merge's
//!    insertions went from 13 to 0 against the fixed clock (WER 0.0090 → 0.0042).
//!    Note what is NOT claimed: the two-second overlap is not silent — this
//!    lecture pauses 0.35 s between sentences, so 1.6 s of that overlap is
//!    speech. It does not need to be silent. It needs to not be cut mid-word.
//! 2. **A seam merge whose PRIMARY key is time, never text.** Word/segment
//!    timestamps decide which window owns a span (the midpoint rule in
//!    [`merge_timed`]); text-level anchor alignment ([`merge_chunk_tokens`]) is
//!    the FALLBACK for a model that returns no alignment at all. Finding #11 is
//!    explicit that RNNT/TDT emission times are not stable across two runs with
//!    different left context, which is why the times are used to answer "whose
//!    audio was this" — a question about the WINDOW, not about the word — and
//!    never as a key to match one decode's words against another's.
//! 3. **Preemption.** There is exactly one warm engine (finding #2b:
//!    `transcription.rs`'s `Err("no ASR model is loaded")` is what a second
//!    concurrent caller gets). A meeting decode that held it end to end would
//!    hard-fail the sub-second dictation path for the length of the meeting. So
//!    meeting ASR checks [`EngineDemand`] at every chunk boundary and stands
//!    down until the dictation is through. Preemption is at the boundary and
//!    never mid-chunk: a cancelled decode is wasted Metal time and, per YV70,
//!    an abandoned engine.
//! 4. **Resumability.** `processed_through_seconds` is written as each chunk
//!    lands (finding #3), so a relaunch after a crash, a quit, or a killed
//!    process resumes at the last completed chunk instead of restarting a full
//!    Metal decode from zero. A chunk that FAILED — including one that blew
//!    through `TRANSCRIBE_TIMEOUT` — is recorded as processed with an empty
//!    text and an `asr_failed` status: it must not poison its neighbours, and
//!    it must not be retried forever on every relaunch either.
//!
//! Plus the English-only gate (finding #38): Parakeet EN has no meeting-language
//! path, so the Notetaker refuses honestly rather than transcribing Spanish as
//! garbled English.
//!
//! Everything here is written to be provable without audio hardware, without a
//! GGUF on disk and without ONNX: the geometry, the merge, the resume ledger and
//! the yield are pure or trait-injected. The corpus-scored half lives in
//! `tests/meeting_eval.rs`, which now scores THIS module's geometry and THIS
//! module's merge rather than a copy of them.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::asr_engine::{TimedKind, TimedSpan, TimedTranscript};
use crate::meeting::{IndexRecord, MIC_TRACK, SYSTEM_TRACK};
use crate::models;
use crate::os_version_gate;
use crate::transcription::{
    TranscriptionManager, ABANDONED_FOR_EXIT, MAX_SANCTIONED_CHUNK_DECODE_SECONDS,
    NO_ENGINE_LOADED, PREEMPTED_FOR_DICTATION, TRANSCRIBE_TIMEOUT, WORST_CASE_RTF_BUDGET,
};
use crate::vad::{VoicedSpan, WarmVad};

/// The rate every buffer in this module is in. The capture path writes 16 kHz
/// mono and the ASR engine wants 16 kHz mono; nothing here resamples.
pub const MEETING_RATE: u32 = 16_000;

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

/// Chunk geometry. The window is a RANGE, not a constant: the chunker aims at
/// `target_seconds` and takes the best silence it can find between
/// `min_seconds` and `max_seconds`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChunkConfig {
    pub target_seconds: f64,
    pub min_seconds: f64,
    pub max_seconds: f64,
    /// How much audio the incoming window re-sees before its own content
    /// starts. With a boundary cut in silence this is mostly silence — it
    /// exists so a word that DOES straddle the cut is complete in at least one
    /// window.
    pub overlap_seconds: f64,
    /// The shortest gap the chunker will accept as a place to cut. Below this a
    /// "silence" is a stop consonant, not a pause.
    pub min_silence_seconds: f64,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        ChunkConfig {
            target_seconds: 30.0,
            min_seconds: 25.0,
            max_seconds: 35.0,
            overlap_seconds: 2.0,
            min_silence_seconds: 0.2,
        }
    }
}

impl ChunkConfig {
    /// The longest buffer this geometry can hand the engine: a full-width
    /// window plus the overlap it re-sees.
    pub fn max_decode_seconds(&self) -> f64 {
        self.max_seconds + self.overlap_seconds
    }

    /// Sanity, checked at plan time rather than trusted.
    ///
    /// The `TRANSCRIBE_TIMEOUT` clause is finding #3's constraint made
    /// mechanical: past 120 s the manager abandons the ENGINE, not just the
    /// call, so the widest window this geometry can produce has to decode well
    /// inside that budget. At a pessimistic real-time factor of 1.0 (Parakeet
    /// 0.6B on Metal runs far under it) a 37 s buffer is 37 s of decode against
    /// a 120 s ceiling.
    pub fn validate(&self) -> Result<(), String> {
        if !(self.min_seconds <= self.target_seconds && self.target_seconds <= self.max_seconds) {
            return Err(format!(
                "chunk target {}s is outside the search window [{}s, {}s]",
                self.target_seconds, self.min_seconds, self.max_seconds
            ));
        }
        if self.overlap_seconds <= 0.0 || self.overlap_seconds >= self.min_seconds {
            return Err(format!(
                "overlap {}s must be positive and smaller than the shortest window ({}s)",
                self.overlap_seconds, self.min_seconds
            ));
        }
        // The SAME number `transcription::ENGINE_HANDBACK_WAIT` is derived
        // from, so a geometry this accepts can never outlast the wait a
        // dictation is willing to give it (the disagreement that used to lose
        // takes outright).
        let ceiling = MAX_SANCTIONED_CHUNK_DECODE_SECONDS;
        if self.max_decode_seconds() > ceiling {
            return Err(format!(
                "widest decode {}s exceeds the {}s the {}s TRANSCRIBE_TIMEOUT leaves at RTF {}",
                self.max_decode_seconds(),
                ceiling,
                TRANSCRIBE_TIMEOUT.as_secs(),
                WORST_CASE_RTF_BUDGET
            ));
        }
        Ok(())
    }
}

/// How a window boundary was chosen — kept per window because "the VAD found a
/// pause" and "nothing was quiet enough, so the clock decided" are different
/// confidence levels at the seam, and the second one is what the text-anchor
/// fallback exists for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryKind {
    /// The start of the meeting, or its end. Not a cut.
    #[default]
    Edge,
    /// Cut in the middle of a VAD-detected silence.
    Silence,
    /// No silence in the search window cleared `min_silence_seconds`; the
    /// target clock decided.
    FixedClock,
}

/// Where a run begins, and what kind of cut that instant is.
///
/// The seconds alone are not enough, and treating them as enough was a real
/// defect. On a RESUMED run the first window does not start at the beginning of
/// the meeting: it starts at the boundary the PREVIOUS run chose, and
/// [`merge_timed`] picks its seam rule from that boundary's kind. Seeding it as
/// [`BoundaryKind::Edge`] — which is what "the plan starts here" used to imply —
/// sent a fixed-clock resume seam down the intersection rule, so the word the
/// cut had truncated was kept twice: an uninterrupted run producing
/// `the meeting starts` came back from a resumed one as
/// `the meeting meeting starts`.
///
/// It is a struct rather than a second `f64` argument so that the kind cannot be
/// forgotten at a call site: [`ResumePoint::start`] is the only way to say "this
/// is the top of the audio", and it says so by name.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResumePoint {
    pub seconds: f64,
    /// The cut this instant IS. [`BoundaryKind::Edge`] only at the top of the
    /// audio — a resume point is by definition a cut somebody made.
    pub boundary: BoundaryKind,
}

impl ResumePoint {
    /// The top of the audio: a true edge, not a cut.
    pub fn start() -> ResumePoint {
        ResumePoint {
            seconds: 0.0,
            boundary: BoundaryKind::Edge,
        }
    }

    /// Resume at `seconds`, at a boundary of the given kind.
    pub fn at(seconds: f64, boundary: BoundaryKind) -> ResumePoint {
        ResumePoint {
            seconds: seconds.max(0.0),
            boundary,
        }
    }
}

/// One window of the meeting.
///
/// `audio_*` is what gets decoded; `content_*` is what this window OWNS in the
/// merged transcript. They differ by exactly the overlap: the window re-sees
/// `overlap_seconds` of the previous window's audio so a straddling word is
/// complete somewhere, and the merge then throws that region away on time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkWindow {
    pub index: usize,
    pub audio_start_seconds: f64,
    pub audio_end_seconds: f64,
    pub content_start_seconds: f64,
    pub content_end_seconds: f64,
    pub start_boundary: BoundaryKind,
    /// The kind of the cut this window ENDS at — i.e. the kind of the next
    /// window's start.
    ///
    /// Recorded (and persisted on [`ChunkOutcome`]) for exactly one reason: it
    /// is the only place a resumed run can learn what kind of boundary its
    /// resume point is. The run that chose it is gone, and the window that would
    /// have carried it as `start_boundary` is precisely the one that was never
    /// decoded.
    #[serde(default)]
    pub end_boundary: BoundaryKind,
}

impl ChunkWindow {
    pub fn audio_seconds(&self) -> f64 {
        self.audio_end_seconds - self.audio_start_seconds
    }
    pub fn content_seconds(&self) -> f64 {
        self.content_end_seconds - self.content_start_seconds
    }
}

/// Turn a boundary ladder into windows. `boundaries` is `[b0, b1, … bn]` with
/// `b0` the resume point and `bn` the end of the audio; window `k` owns
/// `[b_k, b_k+1)` and decodes `[b_k - overlap, b_k+1)`.
pub fn windows_from_boundaries(
    boundaries: &[f64],
    kinds: &[BoundaryKind],
    cfg: &ChunkConfig,
    first_index: usize,
) -> Vec<ChunkWindow> {
    let mut out = Vec::new();
    for k in 0..boundaries.len().saturating_sub(1) {
        let content_start = boundaries[k];
        let content_end = boundaries[k + 1];
        if content_end <= content_start {
            continue;
        }
        // The overlap is taken from the AUDIO, not from the plan: on a resumed
        // run the first window's overlap is audio the previous run already
        // transcribed, and re-seeing it is exactly what gives the decoder its
        // left context back after a relaunch. The merge drops it again on time.
        let audio_start = (content_start - cfg.overlap_seconds).max(0.0);
        out.push(ChunkWindow {
            index: first_index + out.len(),
            audio_start_seconds: audio_start,
            audio_end_seconds: content_end,
            content_start_seconds: content_start,
            content_end_seconds: content_end,
            start_boundary: kinds.get(k).copied().unwrap_or(BoundaryKind::Edge),
            end_boundary: kinds.get(k + 1).copied().unwrap_or(BoundaryKind::Edge),
        });
    }
    out
}

/// How far before the end of a pause the cut goes. The overlap only reaches
/// BACKWARDS from the boundary, so the latest legal instant of the pause is the
/// one that puts the most silence into the overlap — but not the very last
/// sample of it, because the VAD's idea of where speech resumes is a frame-
/// quantised estimate (30 ms frames) and the cut should stay on the quiet side
/// of it.
const SILENCE_EDGE_MARGIN: f64 = 0.05;

/// Where to cut, given the silence map of one search region (pure).
///
/// `voiced` are the voiced spans overlapping `region`, in ABSOLUTE meeting
/// seconds. The rule: take every gap between them at least
/// `min_silence_seconds` long, score each by where in it the cut would land, and
/// take the candidate closest to `target`.
///
/// Where in the gap: as LATE as the margin allows, floored at the gap's middle.
/// That is the whole mechanism by which "the overlap contains near-zero speech"
/// is true rather than hoped for — the overlap is `[boundary - overlap,
/// boundary]`, so pushing the boundary to the end of the pause pulls the maximum
/// amount of that pause into the overlap. Cutting at the middle instead (the
/// obvious first idea) throws half of every pause away and puts speech back into
/// the overlap of any pause shorter than twice the overlap.
///
/// With no qualifying gap the clock decides and the caller is told so, because
/// a clock-cut seam is exactly where the text-anchor fallback earns its keep.
pub fn pick_boundary(
    target: f64,
    region: (f64, f64),
    voiced: &[VoicedSpan],
    min_silence_seconds: f64,
) -> (f64, BoundaryKind) {
    let (from, to) = region;
    if to <= from {
        return (target.clamp(from, from.max(to)), BoundaryKind::FixedClock);
    }
    // Gaps = the complement of the voiced spans inside the region.
    let mut gaps: Vec<(f64, f64)> = Vec::new();
    let mut cursor = from;
    let mut spans: Vec<&VoicedSpan> = voiced
        .iter()
        .filter(|s| s.end_seconds > from && s.start_seconds < to)
        .collect();
    spans.sort_by(|a, b| a.start_seconds.total_cmp(&b.start_seconds));
    for s in spans {
        let start = s.start_seconds.max(from);
        if start > cursor {
            gaps.push((cursor, start));
        }
        cursor = cursor.max(s.end_seconds.min(to));
    }
    if cursor < to {
        gaps.push((cursor, to));
    }

    let best = gaps
        .iter()
        .filter(|(a, b)| b - a >= min_silence_seconds)
        .map(|(a, b)| ((a + b) / 2.0).max(b - SILENCE_EDGE_MARGIN).clamp(from, to))
        .min_by(|a, b| (a - target).abs().total_cmp(&(b - target).abs()));
    match best {
        Some(mid) => (mid, BoundaryKind::Silence),
        None => (target.clamp(from, to), BoundaryKind::FixedClock),
    }
}

/// The fixed-clock plan: windows every `target_seconds` with the overlap, no
/// VAD involved. This is the geometry the YV90 eval corpus was measured on, and
/// the fallback whenever no VAD is available.
pub fn plan_windows_fixed(
    total_seconds: f64,
    from: ResumePoint,
    cfg: &ChunkConfig,
    first_index: usize,
) -> Vec<ChunkWindow> {
    let mut boundaries = vec![from.seconds.max(0.0)];
    // NOT `Edge`. On a resumed run this is the cut the previous run made, and
    // the seam rule at the very first window is chosen by its kind — see
    // [`ResumePoint`].
    let mut kinds = vec![from.boundary];
    loop {
        let prev = *boundaries.last().expect("seeded above");
        if total_seconds - prev <= cfg.max_seconds {
            boundaries.push(total_seconds);
            kinds.push(BoundaryKind::Edge);
            break;
        }
        boundaries.push(prev + cfg.target_seconds);
        kinds.push(BoundaryKind::FixedClock);
    }
    windows_from_boundaries(&boundaries, &kinds, cfg, first_index)
}

/// The shipped plan: same ladder, but every interior boundary is moved onto the
/// quietest place the VAD can find within `[prev+min, prev+max]`.
///
/// One VAD pass per boundary over a `max-min` second region — ten seconds of
/// audio read from the source and handed to the warm Silero session — so
/// planning a three-hour meeting reads ~six minutes of audio in ten-second
/// pieces and never holds more than one region in memory.
pub fn plan_windows(
    audio: &dyn SampleWindows,
    vad: Option<&dyn VoiceActivity>,
    from: ResumePoint,
    cfg: &ChunkConfig,
    first_index: usize,
) -> Result<Vec<ChunkWindow>, String> {
    cfg.validate()?;
    let Some(vad) = vad else {
        return Ok(plan_windows_fixed(
            audio.total_seconds(),
            from,
            cfg,
            first_index,
        ));
    };
    let total = audio.total_seconds();
    let mut boundaries = vec![from.seconds.max(0.0)];
    // NOT `Edge` — see `plan_windows_fixed` and [`ResumePoint`].
    let mut kinds = vec![from.boundary];
    loop {
        let prev = *boundaries.last().expect("seeded above");
        if total - prev <= cfg.max_seconds {
            boundaries.push(total);
            kinds.push(BoundaryKind::Edge);
            break;
        }
        let region = (prev + cfg.min_seconds, (prev + cfg.max_seconds).min(total));
        let target = prev + cfg.target_seconds;
        let (boundary, kind) = match audio.window(region.0, region.1) {
            Ok(samples) => match vad.voiced_spans(&samples) {
                Ok(spans) => {
                    // The VAD reports relative to the region; the ladder is
                    // absolute.
                    let absolute: Vec<VoicedSpan> = spans
                        .iter()
                        .map(|s| VoicedSpan {
                            start_seconds: s.start_seconds + region.0,
                            end_seconds: s.end_seconds + region.0,
                        })
                        .collect();
                    pick_boundary(target, region, &absolute, cfg.min_silence_seconds)
                }
                // A VAD failure is never fatal to a meeting: the clock decides
                // and the seam falls back to text anchoring, exactly as it does
                // for a region with no pause in it.
                Err(e) => {
                    log::warn!("meeting chunker: VAD failed near {prev:.1}s ({e}) — clock boundary");
                    (target, BoundaryKind::FixedClock)
                }
            },
            Err(e) => {
                log::warn!("meeting chunker: cannot read {prev:.1}s search region ({e})");
                (target, BoundaryKind::FixedClock)
            }
        };
        // Never go backwards, never overshoot the search window.
        let boundary = boundary.clamp(region.0, region.1);
        boundaries.push(boundary);
        kinds.push(kind);
    }
    Ok(windows_from_boundaries(&boundaries, &kinds, cfg, first_index))
}

// ---------------------------------------------------------------------------
// Audio access
// ---------------------------------------------------------------------------

/// Random access to a meeting's 16 kHz mono audio, one window at a time.
///
/// A trait rather than a `Vec<f32>` because a three-hour meeting is ~690 MB of
/// f32 and the whole point of the capture design (finding #1) is that it never
/// sits in RAM. [`WavWindows`] reads each window off disk; [`MemoryWindows`] is
/// for tests and for clips small enough to already be in hand.
pub trait SampleWindows {
    fn total_seconds(&self) -> f64;
    /// Samples in `[start_seconds, end_seconds)`, clamped to the audio.
    fn window(&self, start_seconds: f64, end_seconds: f64) -> Result<Vec<f32>, String>;
}

/// Whole-clip source (tests, short takes).
pub struct MemoryWindows {
    samples: Vec<f32>,
    rate: u32,
}

impl MemoryWindows {
    pub fn new(samples: Vec<f32>, rate: u32) -> MemoryWindows {
        MemoryWindows { samples, rate }
    }
    pub fn at_meeting_rate(samples: Vec<f32>) -> MemoryWindows {
        MemoryWindows::new(samples, MEETING_RATE)
    }
}

impl SampleWindows for MemoryWindows {
    fn total_seconds(&self) -> f64 {
        self.samples.len() as f64 / self.rate as f64
    }
    fn window(&self, start_seconds: f64, end_seconds: f64) -> Result<Vec<f32>, String> {
        let from = ((start_seconds.max(0.0) * self.rate as f64) as usize).min(self.samples.len());
        let to = ((end_seconds.max(0.0) * self.rate as f64) as usize).min(self.samples.len());
        if to <= from {
            return Ok(Vec::new());
        }
        Ok(self.samples[from..to].to_vec())
    }
}

/// Windowed reads straight off a 16 kHz mono wav — the shape the meeting
/// journal finalises to. Each read re-opens the file and seeks: a header parse
/// per 30 s window is nothing next to a Metal decode, and it keeps the source
/// stateless (so it is `Sync`, and a resume needs no rewind bookkeeping).
pub struct WavWindows {
    path: PathBuf,
    rate: u32,
    frames: u64,
}

impl WavWindows {
    pub fn open(path: impl AsRef<Path>) -> Result<WavWindows, String> {
        let path = path.as_ref().to_path_buf();
        let reader = hound::WavReader::open(&path)
            .map_err(|e| format!("open {}: {e}", path.display()))?;
        let spec = reader.spec();
        if spec.channels != 1 {
            return Err(format!(
                "{} is {}-channel; meeting audio is mono",
                path.display(),
                spec.channels
            ));
        }
        if spec.sample_rate != MEETING_RATE {
            return Err(format!(
                "{} is {} Hz; meeting audio is {MEETING_RATE} Hz",
                path.display(),
                spec.sample_rate
            ));
        }
        Ok(WavWindows {
            path,
            rate: spec.sample_rate,
            frames: reader.duration() as u64,
        })
    }
}

impl SampleWindows for WavWindows {
    fn total_seconds(&self) -> f64 {
        self.frames as f64 / self.rate as f64
    }

    fn window(&self, start_seconds: f64, end_seconds: f64) -> Result<Vec<f32>, String> {
        let from = ((start_seconds.max(0.0) * self.rate as f64) as u64).min(self.frames);
        let to = ((end_seconds.max(0.0) * self.rate as f64) as u64).min(self.frames);
        if to <= from {
            return Ok(Vec::new());
        }
        let mut reader = hound::WavReader::open(&self.path)
            .map_err(|e| format!("open {}: {e}", self.path.display()))?;
        reader
            .seek(from as u32)
            .map_err(|e| format!("seek {} to {from}: {e}", self.path.display()))?;
        let want = (to - from) as usize;
        let spec = reader.spec();
        let out: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Int => {
                let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
                reader
                    .samples::<i32>()
                    .take(want)
                    .filter_map(|s| s.ok())
                    .map(|s| s as f32 * scale)
                    .collect()
            }
            hound::SampleFormat::Float => reader
                .samples::<f32>()
                .take(want)
                .filter_map(|s| s.ok())
                .collect(),
        };
        Ok(out)
    }
}

/// The silence map the chunker cuts on. Implemented by the app's warm Silero
/// instance; a trait so the planner is testable with a scripted VAD and no ONNX.
pub trait VoiceActivity {
    /// Voiced spans of a 16 kHz mono buffer, in seconds RELATIVE to its start.
    fn voiced_spans(&self, samples: &[f32]) -> Result<Vec<VoicedSpan>, String>;
}

impl VoiceActivity for WarmVad {
    fn voiced_spans(&self, samples: &[f32]) -> Result<Vec<VoicedSpan>, String> {
        WarmVad::speech_spans(self, samples, MEETING_RATE)
    }
}

// ---------------------------------------------------------------------------
// The engine seam
// ---------------------------------------------------------------------------

/// One chunk decode. Trait-injected so the driver's resume/preempt/timeout
/// behaviour is provable without a GGUF.
pub trait ChunkAsr {
    fn transcribe_timed(&self, samples_16k_mono: &[f32]) -> Result<TimedTranscript, String>;
}

/// Is an interactive decode (a dictation) waiting for the one warm engine?
pub trait EngineDemand {
    fn dictation_pending(&self) -> bool;
}

impl EngineDemand for TranscriptionManager {
    fn dictation_pending(&self) -> bool {
        self.interactive_pending()
    }
}

/// Meeting chunks decoded on the app's warm engine.
///
/// It re-`load`s before every chunk on purpose. `load` is idempotent and free
/// when the model is already resident, and it is the ONLY thing standing
/// between a chunk that blew through `TRANSCRIBE_TIMEOUT` — which abandons the
/// engine, not just the call (YV70) — and every chunk after it failing with
/// `no ASR model is loaded`. That is finding #3's "must not poison remaining
/// chunks", and it is what `tests/matrix_new_asr_chunk_timeout.rs` proves.
pub struct WarmEngineChunkAsr {
    manager: TranscriptionManager,
    model_id: String,
    model_path: PathBuf,
    language: Option<String>,
    bias_prompt: Option<String>,
}

impl WarmEngineChunkAsr {
    pub fn new(
        manager: TranscriptionManager,
        model_id: impl Into<String>,
        model_path: impl AsRef<Path>,
        language: Option<String>,
        bias_prompt: Option<String>,
    ) -> WarmEngineChunkAsr {
        WarmEngineChunkAsr {
            manager,
            model_id: model_id.into(),
            model_path: model_path.as_ref().to_path_buf(),
            language,
            bias_prompt,
        }
    }
}

impl ChunkAsr for WarmEngineChunkAsr {
    fn transcribe_timed(&self, samples_16k_mono: &[f32]) -> Result<TimedTranscript, String> {
        // `is_loaded` is true while the engine is LEASED OUT as well as while it
        // sits in the slot, and that distinction is the whole point: without it,
        // a chunk that starts while a dictation holds the engine would see an
        // empty slot and build a SECOND multi-hundred-MB engine beside the live
        // one. Contention is answered by the driver's retry, not by a second
        // Metal device.
        if !self.manager.is_loaded() {
            self.manager.load(&self.model_id, &self.model_path)?;
        }
        self.manager.transcribe_timed(
            samples_16k_mono.to_vec(),
            self.language.clone(),
            self.bias_prompt.clone(),
        )
    }
}

// ---------------------------------------------------------------------------
// The seam merge
// ---------------------------------------------------------------------------

/// A splice anchor must be at least this many tokens long. Two tokens is a
/// coincidence ("the room"); three is an overlap.
pub const MIN_ANCHOR_TOKENS: usize = 3;
/// …and at most this many. The overlap is 2 s of speech — six or seven words.
pub const MAX_ANCHOR_TOKENS: usize = 10;
/// How many tokens [`ChunkConfig::overlap_seconds`] of speech can hold, at the
/// 175 wpm the eval corpus is spoken at: `ceil(2 s * 175 / 60)` = 6. This is the
/// budget for everything a merge is allowed to move at a seam, because the
/// overlap region is the ONLY audio two windows both saw. Kept as a literal
/// because a float-to-int cast is not const, and tied back to the geometry by
/// the eval harness's `overlap_token_budget_matches_the_chunk_geometry`.
pub const OVERLAP_TOKEN_BUDGET: usize = 6;
/// How far back from the end of the running transcript the anchor may sit.
///
/// It has to be the whole overlap budget, and the measurement that says so is
/// the YV90 lecture fixture: at 2 (the first cut of this function) three of its
/// 32 seams found NO anchor and fell into the append-whole branch, emitting the
/// overlap twice — 32 insertions against the reference, WER 0.0151. The cause is
/// not a half-cut word but a truncated-window continuation: the outgoing window
/// ends mid-sentence and the model finishes the sentence its own way, so the
/// genuine anchor sits several tokens back from the end. Trimming those tokens
/// is safe by construction — they are inside the overlap, which the INCOMING
/// window re-supplies from the anchor onward, so nothing is deleted that is not
/// immediately re-emitted. At 6 all 32 seams anchor, insertions go to 0.
pub const MAX_TAIL_TRIM: usize = OVERLAP_TOKEN_BUDGET;
/// …and the same at the HEAD of the incoming chunk, where a window boundary
/// cutting a word in half does show up. Left at 4: sweeping it to 12 changes
/// neither eval fixture's numbers, and every token of slack here widens the
/// deletion budget the seam gate has to allow.
pub const MAX_HEAD_SKIP: usize = 4;

/// What a merge did at the seams, so a gate can check the MERGE and not only
/// the marker words that happen to sit in one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MergeReport {
    /// Seams merged — one per window after the first.
    pub seams: usize,
    /// Tokens dropped from the tail of the running transcript.
    pub tail_tokens_trimmed: usize,
    /// Tokens skipped at the head of an incoming chunk.
    pub head_tokens_skipped: usize,
    /// Seams where no anchor was found and the chunk was appended whole. A
    /// duplicate at such a seam is a VISIBLE defect; deleting words on a guess
    /// would be a silent one.
    pub no_anchor_seams: usize,
}

/// Casefold a token for comparison — the merge matches on what the words ARE,
/// not on how the decoder punctuated them, but emits the ORIGINAL token so a
/// user-visible transcript keeps its punctuation.
fn fold(token: &str) -> String {
    token
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// FALLBACK seam dedupe: longest common run at the seam, anchored to where the
/// overlap actually is (plan finding #11 — text alignment is never the primary
/// key).
///
/// What survives anchors the run to the END of the running transcript and the
/// START of the incoming chunk, with the overlap's own word budget of slack at
/// the tail and a cut word's worth at the head. It can therefore delete at most
/// [`MAX_TAIL_TRIM`] + [`MAX_HEAD_SKIP`] tokens at a seam by construction, and
/// only tokens inside the overlap region — which the incoming window re-supplies
/// from the anchor onward.
pub fn merge_chunk_tokens(chunks: &[Vec<String>]) -> Vec<String> {
    merge_chunk_tokens_reporting(chunks).0
}

/// [`merge_chunk_tokens`], plus what it did at each seam.
pub fn merge_chunk_tokens_reporting(chunks: &[Vec<String>]) -> (Vec<String>, MergeReport) {
    let merged = merge_chunk_tokens_segmented(chunks);
    (merged.tokens, merged.report)
}

/// [`merge_chunk_tokens_reporting`], plus WHICH merged tokens each input chunk
/// ended up owning.
///
/// This exists because `MeetingTranscript.segments` on the text-fallback path
/// used to be built from each chunk's RAW text, which still contains the
/// overlap the merge had just removed from `text` — so the field YV94 persists
/// and the detail UI renders duplicated ~6 words at every seam while the merged
/// `text` beside it did not. Segments are sliced out of the MERGED stream now,
/// so `segments.join(" ") == text` holds by construction on both merge paths.
///
/// The ranges are contiguous and cover the whole merged stream: token `i` of
/// the merge belongs to exactly one chunk. Anchor tokens — the words both
/// windows decoded, kept once — are attributed to the EARLIER chunk, which is
/// the window whose content range holds them.
pub struct SegmentedMerge {
    pub tokens: Vec<String>,
    pub report: MergeReport,
    /// One `[start, end)` range into [`SegmentedMerge::tokens`] per input
    /// chunk, in input order. May be empty for a chunk the merge fully absorbed.
    pub chunk_ranges: Vec<(usize, usize)>,
}

impl SegmentedMerge {
    /// The tokens chunk `i` owns, as one string. Empty if it owns none.
    pub fn chunk_text(&self, i: usize) -> String {
        let (start, end) = self.chunk_ranges[i];
        self.tokens[start..end].join(" ")
    }
}

pub fn merge_chunk_tokens_segmented(chunks: &[Vec<String>]) -> SegmentedMerge {
    let mut merged: Vec<String> = Vec::new();
    let mut folded: Vec<String> = Vec::new();
    let mut report = MergeReport::default();
    let mut chunk_ranges: Vec<(usize, usize)> = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        let chunk_folded: Vec<String> = chunk.iter().map(|t| fold(t)).collect();
        if merged.is_empty() {
            let start = merged.len();
            merged.extend(chunk.iter().cloned());
            folded.extend(chunk_folded);
            chunk_ranges.push((start, merged.len()));
            continue;
        }
        report.seams += 1;
        // (keep in merged, tokens skipped before the anchor, anchor length)
        let mut splice: Option<(usize, usize, usize)> = None;
        'search: for n in (MIN_ANCHOR_TOKENS..=MAX_ANCHOR_TOKENS).rev() {
            for trim in 0..=MAX_TAIL_TRIM {
                if folded.len() < trim + n {
                    continue;
                }
                let keep = folded.len() - trim;
                let tail = &folded[keep - n..keep];
                for skip in 0..=MAX_HEAD_SKIP {
                    if chunk_folded.len() < skip + n {
                        break;
                    }
                    if tail == &chunk_folded[skip..skip + n] {
                        splice = Some((keep, skip, n));
                        break 'search;
                    }
                }
            }
        }
        match splice {
            Some((keep, skip, anchor)) => {
                // The anchor tokens themselves are not "moved": they are the
                // same words on both sides of the seam, kept once. Only the
                // trimmed tail and the tokens before the anchor are.
                report.tail_tokens_trimmed += merged.len() - keep;
                report.head_tokens_skipped += skip;
                merged.truncate(keep);
                folded.truncate(keep);
                // The tail trim shortens what the EARLIER chunks own, so their
                // ranges shrink with it — that is what keeps the ranges a
                // partition of the merged stream rather than a set of stale
                // offsets into a vector that has since been truncated.
                for range in chunk_ranges.iter_mut() {
                    range.0 = range.0.min(keep);
                    range.1 = range.1.min(keep);
                }
                let start = merged.len();
                merged.extend(chunk[skip + anchor..].iter().cloned());
                folded.extend(chunk_folded[skip + anchor..].iter().cloned());
                chunk_ranges.push((start, merged.len()));
            }
            None => {
                report.no_anchor_seams += 1;
                let start = merged.len();
                merged.extend(chunk.iter().cloned());
                folded.extend(chunk_folded);
                chunk_ranges.push((start, merged.len()));
            }
        }
    }
    SegmentedMerge {
        tokens: merged,
        report,
        chunk_ranges,
    }
}

/// PRIMARY seam dedupe: a span belongs to the window whose CONTENT range holds
/// its midpoint.
///
/// This is a question about the window, not about the word — which is what
/// makes it safe to ask of RNNT emission times that finding #11 correctly says
/// are unstable across runs. Every instant of the meeting is inside exactly one
/// content range, so the union is lossless and the intersection is empty: a
/// duplicate at a seam is impossible by construction, and a deletion needs a
/// span whose midpoint the model placed outside the window it was decoded in.
pub fn merge_timed(chunks: &[ChunkOutcome]) -> Vec<TimedSpan> {
    merge_timed_reporting(chunks).0
}

/// What [`merge_timed`] decided at ONE seam, and on what evidence.
///
/// This exists so the eval harness can score the tie-break itself rather than
/// only its effect on the WER two arms away. The seam rule is the one place in
/// the merge where a real word can be deleted, its discriminator is a
/// millisecond-scale time comparison, and the corpus that would catch a bad
/// threshold is `say`-generated TTS that never stutters — so the harness gets
/// the decisions, not a re-implementation of them.
#[derive(Debug, Clone, PartialEq)]
pub struct SeamDecision {
    /// Index of the INCOMING chunk (the seam is at its content start).
    pub chunk_index: usize,
    pub boundary_seconds: f64,
    pub kind: BoundaryKind,
    /// The last word the outgoing window kept, and where the model ended it.
    pub previous_text: String,
    pub previous_end_seconds: f64,
    /// The first word the incoming window owns, and where the model started it.
    pub first_text: String,
    pub first_start_seconds: f64,
    /// The same word on both sides, under [`fold`].
    pub text_matches: bool,
    /// `boundary - previous_end`: ~0 for a word the cut truncated (the outgoing
    /// buffer stops at the boundary), a real gap for a word the speaker
    /// finished before it. Negative when the model timed the word slightly past
    /// the end of the audio it was given.
    pub truncation_gap_seconds: f64,
    /// Whether the outgoing word was dropped as a second decode of the incoming
    /// one.
    pub popped: bool,
}

/// [`merge_timed`], plus every seam decision it made.
pub fn merge_timed_reporting(chunks: &[ChunkOutcome]) -> (Vec<TimedSpan>, Vec<SeamDecision>) {
    let mut out: Vec<TimedSpan> = Vec::new();
    let mut decisions: Vec<SeamDecision> = Vec::new();
    for chunk in chunks {
        let kept: Vec<&TimedSpan> = chunk
            .spans
            .iter()
            .filter(|span| {
                let mid = (span.start_seconds + span.end_seconds) / 2.0;
                mid >= chunk.content_start_seconds && mid < chunk.content_end_seconds
            })
            .collect();
        // The ONE case the midpoint rule cannot settle on its own, and it is
        // measured rather than imagined: a word the boundary cuts in half. The
        // outgoing window's audio stops mid-word, so it emits a truncated word
        // ending at the cut (midpoint just BEFORE the boundary — kept here); the
        // incoming window re-sees the overlap and emits the whole word, whose
        // midpoint lands just AFTER the boundary (kept there too). One word, two
        // windows, both sides of the line. Measured on the 15-minute lecture
        // with fixed-clock boundaries: exactly 29 such duplicates over 29 seams,
        // WER 0.0141 against 0.0048 for the text-anchored merge.
        //
        // The tie-break stays a tie-break: time is still the key, and text is
        // consulted for one comparison at one instant — the last word before the
        // seam against the first word after it — never as an alignment search
        // over the transcript (finding #11). The later window wins because it
        // saw the whole word; the earlier one only saw as much as the cut left.
        //
        // **What the tie-break is allowed to assume depends on the seam it is
        // standing at, and that is the correction this code needed.** On its
        // own, "the same word, starting within a second" is also the exact
        // shape of a speaker repeating a word across a pause — "Right. Right.",
        // "Yeah. Yeah.", "so, so" — and the VAD-cut chunker deliberately puts
        // the boundary IN that pause, so the shipped arm manufactures the shape
        // this rule then deleted. Measured on the shipped module before the
        // guard below: `okay / Right. / Right, / so` came back as
        // `okay / Right, / so`, one real word gone. That is the plan's own
        // falsifiable line, "seam dedupe never deletes real words", failing on
        // the PRIMARY path while the eval harness only exercised the fallback.
        //
        // So:
        //
        // * [`BoundaryKind::FixedClock`] — a word cut in half is exactly what
        //   this seam produces. **But "no pause cleared
        //   [`ChunkConfig::min_silence_seconds`] anywhere in the search window"
        //   does NOT mean no repetition can occur here**, and the first cut of
        //   this guard claimed it did. The repetition that occurs at a cut with
        //   no pause in it is a STUTTER — "that that", "I I", "the the", "so
        //   so" — which by definition has no pause, and no pause is precisely
        //   the condition that produced the fixed-clock boundary. Proximity
        //   alone therefore deletes a real word here just as surely as it did
        //   at the silence seam. What separates the two is
        //   [`SEAM_TRUNCATION_SLACK`]: the outgoing window's AUDIO stops at the
        //   boundary, so a word the cut truncated must END there (within a
        //   frame of it), while a stutter's first token ends before the cut
        //   with a real gap after it. Measured on the lecture's fixed-clock
        //   arm: 17 pops over 29 seams, insertions 29 → 12, primary merge WER
        //   0.0141 → 0.0087, and 16 of those 17 pops survive the truncation gate
        //   because their words end within one frame of the cut. The 17th
        //   (`into | into`, two frames short) is now kept: insertions 13, WER
        //   0.0090 — one visible duplicate on an arm that is not the shipped
        //   one, bought for a class of silent deletions on every arm. See
        //   `meeting_eval_the_fixed_clock_tie_break_only_pops_words_the_cut_truncated`
        //   for the raw distribution.
        // * [`BoundaryKind::Silence`] — the VAD asserted a pause here, so the
        //   only duplicate a pause cannot explain is two spans that INTERSECT
        //   on the timeline: the same instant decoded twice. A repetition
        //   across a pause is disjoint by construction, because the pause IS
        //   the gap. Measured on the shipped VAD-cut arm: 0 insertions, primary
        //   merge WER 0.0042 — the same numbers as before this fix, with the
        //   deletion gone.
        //
        // The intersection test is deliberately NOT applied at a fixed-clock
        // seam. Emission times drift between two decodes with different left
        // context (finding #11): measured on this fixture, 8 of the 17
        // fixed-clock duplicate pairs are DISJOINT in time, one by 0.24 s, so
        // requiring intersection there would silently stop deduping half of
        // them (measured: insertions 12 → 20).
        if let (Some(previous), Some(first)) = (out.last(), kept.first()) {
            let boundary_gap = (first.start_seconds - previous.start_seconds).abs();
            let same_instant = previous.end_seconds > first.start_seconds
                && first.end_seconds > previous.start_seconds;
            // Did the cut itself end this word, or did the speaker? The
            // outgoing window has no audio past `content_start_seconds` — that
            // instant is where its buffer stops — so a truncated fragment can
            // only end there, and anything ending earlier is a word that
            // finished on its own.
            let truncated_by_the_cut =
                previous.end_seconds >= chunk.content_start_seconds - SEAM_TRUNCATION_SLACK;
            let repairable = match chunk.start_boundary {
                BoundaryKind::FixedClock => truncated_by_the_cut,
                BoundaryKind::Silence | BoundaryKind::Edge => same_instant,
            };
            let text_matches =
                fold(&previous.text) == fold(&first.text) && !fold(&first.text).is_empty();
            let popped = repairable && boundary_gap <= SEAM_DUPLICATE_TOLERANCE && text_matches;
            decisions.push(SeamDecision {
                chunk_index: chunk.index,
                boundary_seconds: chunk.content_start_seconds,
                kind: chunk.start_boundary,
                previous_text: previous.text.clone(),
                previous_end_seconds: previous.end_seconds,
                first_text: first.text.clone(),
                first_start_seconds: first.start_seconds,
                text_matches,
                truncation_gap_seconds: chunk.content_start_seconds - previous.end_seconds,
                popped,
            });
            if popped {
                out.pop();
            }
        }
        out.extend(kept.into_iter().cloned());
    }
    out.sort_by(|a, b| a.start_seconds.total_cmp(&b.start_seconds));
    (out, decisions)
}

// ---------------------------------------------------------------------------
// YV107 / OS-2 — the CROSS-TRACK merge, on host time
// ---------------------------------------------------------------------------

/// One span of the merged, host-time-ordered, speaker-labeled transcript.
///
/// Deliberately not a [`TimedSpan`]: a merged span answers a question a
/// single-track span cannot — *whose* words these are — and bolting a speaker
/// onto `TimedSpan` would put that question into every one of the dozen places
/// that carry an unlabeled span for a single track.
///
/// `start_seconds` / `end_seconds` are **session** seconds: one timeline both
/// tracks were mapped onto, not either track's own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergedSpan {
    /// [`crate::meeting::MIC_TRACK`] or [`crate::meeting::SYSTEM_TRACK`].
    pub track: usize,
    /// [`crate::meetings::speaker_label`] of `track` — "Me" or "Them".
    pub speaker: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub text: String,
}

/// Where each track's `host_ns` zero sits on the SESSION's clock, in
/// nanoseconds.
///
/// This exists because `IndexRecord::host_ns` is rebased to its own stream's
/// first callback (`record.rs`'s `build_capture_stream` does it for the mic;
/// the tap's IOProc does the same for track B), so the two tracks' host clocks
/// are two clocks with the same TICK and different ZEROS. The tick is what
/// defeats drift; the zero is a constant offset that nothing in the numbers
/// themselves can recover, because the aggregate device takes real time to
/// build and the tap's first callback therefore lands after the mic's.
///
/// So it is a parameter, not an assumption. Passing [`TrackEpochs::SHARED`]
/// asserts the two streams started at the same instant — true for the synthetic
/// fixtures and for a session that stamps both epochs from one clock read, and
/// a lie the caller has to tell out loud rather than one this module makes on
/// its behalf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrackEpochs {
    pub a_ns: i64,
    pub b_ns: i64,
}

impl TrackEpochs {
    /// Both tracks' first callbacks treated as the same session instant.
    pub const SHARED: TrackEpochs = TrackEpochs { a_ns: 0, b_ns: 0 };

    pub fn new(a_ns: i64, b_ns: i64) -> TrackEpochs {
        TrackEpochs { a_ns, b_ns }
    }
}

/// Merge two tracks into ONE host-time-ordered, speaker-labeled transcript
/// (OS-2, finding #10).
///
/// **The bug this deletes.** Every track time upstream of here is
/// `samples ÷ nominal_rate`, and the nominal rate is the number that is wrong.
/// Track A is clocked by the input device's crystal, track B by the aggregate's
/// main sub-device — two independent oscillators the moment those are different
/// hardware, spec'd ±20–50 ppm, which is **≈216 ms of relative drift at 20 ppm
/// over the 3-hour cap, ≈430 ms at 40 ppm and ≈1.08 s at the ±50/±50 worst
/// case**. Interleaving two such timelines by their own seconds starts placing
/// an answer before its question at about 200 ms and is systematically wrong
/// through the back half of every long meeting. It is invisible on a desk test,
/// because built-in mic and built-in speakers on Apple Silicon are ONE clock
/// domain; it appears in the configuration people actually use, which is AirPods
/// or a monitor for output and something else for input.
///
/// **The fix, in one sentence.** Both tracks already carry a common time base —
/// [`IndexRecord::host_ns`], the mach host time the anchor was captured at,
/// which YV91/YV100/YV106 persist about once a second — so a track second is
/// converted to a HOST second by interpolating that track's own index records,
/// and the interleave happens there. Interpolating real host times against real
/// capture counts uses each device's TRUE rate by construction: the nominal
/// rate never enters the arithmetic, so there is nothing left for it to be
/// wrong about. Residual error is bounded by one index interval of curvature —
/// at a 1 s cadence and 100 ppm that is 100 µs, three orders of magnitude under
/// the 50 ms the acceptance criterion asks for at 3 hours.
///
/// That bound is the DRIFT bound, and it is stated for as long as one stream
/// runs. A stream REOPENED mid-meeting — an AirPods swap on track 0, YV103's
/// `RebuildAggregate` on track 1 — restarts its clock at zero underneath a wav
/// that keeps running, and [`HostTimeline`] handles that by SEGMENTING rather
/// than by believing the new zero. Its doc comment states what that recovers,
/// and states the one thing it cannot: the dead air across the reopen, a
/// bounded one-off that never accumulates.
///
/// This is deliberately **additive**: [`merge_timed`] and
/// [`merge_timed_reporting`] are untouched and still own the within-track seam
/// dedupe, which this runs FIRST, per track. The two questions are separate —
/// "did these two windows of the same microphone decode the same word twice"
/// and "which of these two microphones spoke first" — and answering them in one
/// pass would make the seam rule's careful text tie-break start comparing words
/// across speakers.
///
/// **A mic-only meeting is not re-timed.** With `track_b` empty there is nothing
/// to align against, so the function returns exactly what [`merge_timed`]
/// returns, labeled — same spans, same times, same order. Every 22-A recording
/// in the wild goes through this path, and buying them a rounding change for no
/// alignment benefit would be a regression with nothing on the other side of it.
/// `tests/two_track_merge_no_regression_single_track.rs` is the guard.
pub fn merge_two_tracks_by_host_time(
    track_a: &[ChunkOutcome],
    track_b: &[ChunkOutcome],
    anchors_a: &[IndexRecord],
    anchors_b: &[IndexRecord],
    epochs: TrackEpochs,
) -> Vec<MergedSpan> {
    let spans_a = merge_timed(track_a);
    let spans_b = merge_timed(track_b);

    // The 22-A shape, preserved byte for byte. See the doc comment above: no
    // second track means no cross-clock question to answer.
    if spans_b.is_empty() {
        return spans_a
            .into_iter()
            .map(|s| label(MIC_TRACK, s.start_seconds, s.end_seconds, s.text))
            .collect();
    }

    let map_a = HostTimeline::new(anchors_a, epochs.a_ns);
    let map_b = HostTimeline::new(anchors_b, epochs.b_ns);

    let mut out: Vec<MergedSpan> = Vec::with_capacity(spans_a.len() + spans_b.len());
    for (track, spans, map) in [
        (MIC_TRACK, spans_a, &map_a),
        (SYSTEM_TRACK, spans_b, &map_b),
    ] {
        for span in spans {
            let start = map.session_seconds(span.start_seconds);
            // The END is mapped through the same timeline rather than carried
            // as `start + duration`: a word's duration is in the track's own
            // seconds, and a track running 100 ppm fast holds words that are
            // 100 ppm short. Mapping both ends puts the whole span on the
            // session clock instead of anchoring one end to it and letting the
            // other keep drifting.
            let end = map.session_seconds(span.end_seconds);
            out.push(label(track, start, end.max(start), span.text));
        }
    }

    // Ordered by host time, and by TRACK when two spans start at the same
    // instant — which is a real overlap (both people talking), not a tie to be
    // broken arbitrarily. `total_cmp` rather than `partial_cmp().unwrap()`: a
    // NaN out of a degenerate timeline must not panic a three-hour transcript.
    out.sort_by(|x, y| {
        x.start_seconds
            .total_cmp(&y.start_seconds)
            .then(x.track.cmp(&y.track))
    });
    out
}

/// The one place a track number becomes a speaker name in the merge.
///
/// The cast is the seam between two shipped numberings that are both correct
/// and are NOT the same type: [`crate::meeting::MIC_TRACK`] is a `usize`
/// because it indexes the journal's per-track vectors, and
/// [`crate::meetings::MIC_TRACK`] is an `i64` because it is a SQLite `INTEGER`
/// column. YV108 shipped `meetings::speaker_label` against the DB numbering and
/// the UI's TypeScript mirror is held to it; calling it here — rather than
/// re-deriving "Me"/"Them" against the journal numbering, which would compile
/// and would be a third copy of the rule — is what keeps the merge, the
/// Markdown export and the screen from ever disagreeing about who spoke.
/// `the_merge_labels_through_the_shipped_render_rule` is the guard.
fn label(track: usize, start_seconds: f64, end_seconds: f64, text: String) -> MergedSpan {
    MergedSpan {
        track,
        speaker: crate::meetings::speaker_label(track as i64).to_string(),
        start_seconds,
        end_seconds,
        text,
    }
}

/// One track's own index records, as a monotone map from track seconds to
/// session seconds.
///
/// The map's two axes are "where in the FINALIZED track an instant is" against
/// `host_ns`, "when that instant really happened". The second axis is a column
/// the record stores. **The first one is not**, and getting that wrong is the
/// whole failure mode this doc comment exists for.
///
/// The finalized wav — the one the chunker cut and the ASR timestamped — is
/// what reached the disk plus what the repair put back, which is exactly
/// [`crate::meeting::finalized_positions`]:
/// `spilled_samples + Σ silence of the splices planned at or before the record`.
///
/// Neither stored column is that number:
///
/// * `spilled_samples` alone ignores the repair, so it shifts by every gap the
///   splice filled in.
/// * `captured_samples` is right only under [`plan_silence_splices`]'s COUNTER
///   rule, where the interval splices exactly `captured − spilled`. Under its
///   WALL-CLOCK rule it is wrong by the whole stall: a device that stops
///   delivering fires no callbacks, so both counters freeze in perfect
///   agreement (`captured − spilled == 0`) while the finalize splices seconds
///   of silence, and the finalized wav is LONGER than `captured`. A ten-second
///   stall an hour into a meeting then mis-times every word after it by ten
///   seconds against this item's 50 ms budget — and no lossless or merely lossy
///   fixture can see it, because both of those reduce `finalized` back to
///   `captured`. `a_device_stall_does_not_shift_the_rest_of_the_meeting` in
///   `tests/two_track_merge_ordering.rs` is the guard, and the system track is
///   where it bites: a process tap gets no callbacks whenever the tapped app is
///   silent, which makes a stall splice routine on track 1 rather than exotic.
///
/// ## The seam a reopen leaves, and why dropping records at it is not enough
///
/// `host_ns` is rebased to the FIRST CALLBACK OF ITS OWN STREAM
/// (`record.rs::build_capture_stream`), and it has to be: `cpal` only defines
/// `StreamInstant::duration_since` within one stream, so there is no absolute
/// tick to hand downstream. [`crate::meeting::MeetingCapture::retune_track`] —
/// the documented handler for exactly this, an AirPods swap on track 0 or
/// YV103's `RebuildAggregate` on track 1 — deliberately keeps `captured_samples`
/// and `spilled_samples` running across the reopen, so the wav is CONTINUOUS
/// while the clock underneath it restarts at zero.
///
/// So the sequence is not one monotone run, it is several. A filter that merely
/// dropped the backwards records would recover for as long as the new clock
/// stayed under the old maximum and then start accepting again on the NEW zero —
/// re-timing the whole post-reopen remainder of the meeting by the length of the
/// pre-reopen run. A five-minute-in mic swap in a 90-minute meeting would place
/// a word spoken at session second 4000 at 3700: a 300,000 ms residual against
/// this item's 50 ms budget, and worse the longer the meeting runs, which is the
/// common case rather than the exotic one.
///
/// This map is therefore SEGMENTED. A backwards `host_ns` closes the current
/// segment and opens the next, whose epoch is the session time already elapsed —
/// derived from the finalized position the two runs meet at, which is the one
/// axis the reopen did not disturb. Session time is continuous and monotone
/// across the seam by construction.
///
/// **What a reopen still costs, stated rather than buried.** The dead air
/// between the old stream's last callback and the new stream's first is audio
/// that was never captured, so it is not in the wav and no map can invent it:
/// the two clocks are rebased to different instants, and nothing in the sidecar
/// measures the distance between them. Post-reopen spans are therefore early by
/// exactly that reopen gap — a fraction of a second, once, bounded by how long a
/// stream rebuild takes rather than by how long the meeting runs. It does not
/// accumulate, which is the whole difference between it and the ppm drift this
/// item exists to delete. Stamping a session offset into the sidecar at
/// `retune_track` is what would close it; [`crate::meeting::TrackRate::segments`]
/// is what makes it visible in the meantime.
///
/// [`plan_silence_splices`]: crate::meeting::plan_silence_splices
struct HostTimeline {
    /// One per monotonic run of the clock, in track order. Never empty.
    segments: Vec<TimelineSegment>,
}

/// One stretch of a track over which `host_ns` never restarted.
struct TimelineSegment {
    /// `(finalized_samples, host_ns)` — strictly increasing on both axes.
    points: Vec<(f64, f64)>,
    /// An instant in this segment is at session nanosecond
    /// `epoch_ns + host_ns`. For the first segment this is the caller's track
    /// epoch; for every later one it is whatever keeps session time continuous
    /// across the seam.
    epoch_ns: f64,
}

impl HostTimeline {
    fn new(records: &[IndexRecord], epoch_ns: i64) -> HostTimeline {
        // The finalized position is DERIVED, not stored — see the struct's doc
        // comment. It is derived by running the shipping splice planner over
        // this track's own records rather than by re-implementing its two
        // rules here, so the map and the wav it maps can never disagree about
        // what the finalize did.
        let finalized = crate::meeting::finalized_positions(records);
        let epoch_ns = epoch_ns as f64;
        let mut timeline = HostTimeline {
            segments: Vec::new(),
        };
        let mut run: Vec<(f64, f64)> = Vec::new();
        let mut previous_host: Option<f64> = None;
        for (record, position) in records.iter().zip(finalized) {
            let (position, host) = (position as f64, record.host_ns as f64);
            // A record that runs backwards on the clock is a REOPEN, not a
            // negative interval: it is the new stream's own first callback.
            // That closes the run — it does not discard the run, and it does
            // not discard what comes after it either.
            if previous_host.is_some_and(|previous| host <= previous) {
                timeline.close(std::mem::take(&mut run), epoch_ns);
            }
            previous_host = Some(host);
            // Inside a run, still strictly forward on BOTH axes: an interval
            // with no width on either axis is one this cannot divide by.
            match run.last() {
                Some(&(samples, last_host)) => {
                    if position > samples && host > last_host {
                        run.push((position, host));
                    }
                }
                None => run.push((position, host)),
            }
        }
        timeline.close(run, epoch_ns);
        if timeline.segments.is_empty() {
            // A journal that never got a record written. The nominal timeline
            // shifted by the epoch is the 22-A behaviour — exactly as good as
            // what the caller had before and no worse.
            timeline.segments.push(TimelineSegment {
                points: vec![(0.0, 0.0)],
                epoch_ns,
            });
        }
        timeline
    }

    /// Close a run, giving it the epoch that makes session time continuous with
    /// the segment before it.
    fn close(&mut self, points: Vec<(f64, f64)>, initial_epoch_ns: f64) {
        let Some(&(first_sample, first_host)) = points.first() else {
            return;
        };
        let epoch_ns = match self.segments.last() {
            None => initial_epoch_ns,
            Some(previous) => {
                let &(last_sample, last_host) = previous
                    .points
                    .last()
                    .expect("`close` never pushes an empty segment");
                // Session time where the previous run ended, plus the WAV the
                // two runs have between them. The reopen gap itself contributed
                // no audio, so the bridge is what the finalized track actually
                // gained across the seam, at the nominal rate — the only rate
                // there is, since neither clock spans the gap. `max(0.0)` keeps
                // the map monotone even if a future producer lets the position
                // axis restart too.
                let session_end_ns = previous.epoch_ns + last_host;
                let bridge_ns = (first_sample - last_sample).max(0.0) / MEETING_RATE as f64 * 1e9;
                session_end_ns + bridge_ns - first_host
            }
        };
        self.segments.push(TimelineSegment { points, epoch_ns });
    }

    /// The segment a finalized sample belongs to: the LAST run that starts at
    /// or before it, so a sample in the dead air between two runs is placed off
    /// the run that preceded it rather than off one that had not started yet.
    fn segment_for(&self, sample: f64) -> &TimelineSegment {
        let at = self
            .segments
            .partition_point(|segment| segment.points[0].0 <= sample);
        &self.segments[at.saturating_sub(1)]
    }

    /// Track seconds → session seconds.
    ///
    /// With no usable interval there is nothing measured to lean on, so this
    /// degrades to the nominal timeline shifted by the epoch — the 22-A
    /// behaviour, which is exactly as good as what the caller had before and no
    /// worse. That is the honest floor for a meeting whose journal never got a
    /// second record written.
    fn session_seconds(&self, track_seconds: f64) -> f64 {
        let sample = track_seconds * MEETING_RATE as f64;
        let segment = self.segment_for(sample);
        let host_ns = match segment.points.len() {
            0 | 1 => {
                let (base_sample, base_host) =
                    segment.points.first().copied().unwrap_or((0.0, 0.0));
                base_host + (sample - base_sample) / MEETING_RATE as f64 * 1e9
            }
            _ => {
                // The first interval whose right edge is at or past `sample`.
                // Clamped so a sample before the first record extrapolates
                // backwards off the FIRST interval and one past the last
                // extrapolates forwards off the LAST — both at that interval's
                // own measured rate, which is the best evidence available for
                // an instant the records do not bracket.
                let at = segment
                    .points
                    .partition_point(|(samples, _)| *samples < sample)
                    .clamp(1, segment.points.len() - 1);
                let (s0, h0) = segment.points[at - 1];
                let (s1, h1) = segment.points[at];
                let frac = (sample - s0) / (s1 - s0);
                h0 + frac * (h1 - h0)
            }
        };
        (segment.epoch_ns + host_ns) / 1e9
    }
}

/// How far apart two decodes of the SAME word at a seam may be timestamped
/// before they stop being the same word. A second is generous next to a word
/// (RNNT emission times drift between runs — finding #11 — but not by a second).
///
/// It is a SECOND condition, never the only one: on its own, "the same word
/// starting within a second" is also the exact shape of a speaker repeating a
/// word — across a pause at a [`BoundaryKind::Silence`] seam, or with no pause
/// at all (a stutter) at a [`BoundaryKind::FixedClock`] one. What keeps a
/// repetition from being deleted is the per-seam evidence [`merge_timed`]
/// requires beside it: two spans covering the same INSTANT at a silence cut,
/// and a word the cut TRUNCATED ([`SEAM_TRUNCATION_SLACK`]) at a clock cut.
const SEAM_DUPLICATE_TOLERANCE: f64 = 1.0;

/// How far short of the cut the outgoing window's last word may end and still
/// count as a word the cut TRUNCATED rather than a word the speaker finished.
///
/// This is the whole discriminator between the duplicate a fixed-clock seam
/// manufactures and the repetition a speaker produces at one. The outgoing
/// window's buffer ENDS at the boundary, so a word still being spoken there is
/// emitted with its end at the buffer's end; a stutter's first token ends
/// before the cut and the silence after it is inside the buffer, so the model
/// times it where it actually stopped.
///
/// The value is one and a half Parakeet frames, and both halves of that are
/// measured. The model's timestamp resolution is one frame — a 10 ms mel hop
/// with 8× encoder subsampling, 80 ms — and the lecture's fixed-clock arm
/// shows the two populations falling exactly where the argument above says they
/// should, quantised to that grid (printed in full by
/// `meeting_eval_the_fixed_clock_tie_break_only_pops_words_the_cut_truncated`):
///
/// * 16 of the 29 seams carry a word the cut truncated. Every one of them ends
///   within ONE frame of the boundary — gaps of −0.08 s, 0.00 s or +0.08 s,
///   the model placing the word's end on the sample the audio stopped at, give
///   or take its own resolution.
/// * the nearest text-matching candidate the gate declines to pop ends TWO
///   frames short (+0.16 s).
///
/// So the threshold goes between the clusters, at 0.12 s, with half a frame of
/// margin on each side rather than sitting on either population's edge. The
/// eval test asserts that margin, not just the count, so a model whose emission
/// times drift enough to close it fails there instead of quietly deleting a
/// word here.
///
/// The one seam this costs, stated rather than buried: `into | into` at 720 s
/// (two frames short) is a genuine duplicate that the old proximity-only rule
/// popped and this one keeps, so the fixed-clock arm's insertions go 12 → 13
/// (WER 0.0087 → 0.0090). That is the trade the plan already made explicitly —
/// *"a duplicate at such a seam is a VISIBLE defect; deleting words on a guess
/// would be a silent one"* — and it is paid on the fixed-clock arm only; the
/// shipped VAD-cut arm still has 0 insertions and WER 0.0042.
///
/// What it does NOT resolve, stated rather than hidden: a stutter whose first
/// token ends within 0.12 s of the cut is indistinguishable from a truncated
/// word by time alone, because at this resolution the two are nearly the same
/// observation. The eval corpus's WER gate is what bounds the residue.
pub const SEAM_TRUNCATION_SLACK: f64 = 0.12;

// ---------------------------------------------------------------------------
// Per-chunk results + the resume ledger
// ---------------------------------------------------------------------------

/// How one chunk ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChunkStatus {
    Done,
    /// The decode failed — including by blowing through `TRANSCRIBE_TIMEOUT`,
    /// which abandons the engine (YV70). The chunk carries an empty text, the
    /// meeting carries on, and the chunk is NOT retried on resume.
    AsrFailed,
}

/// One finished chunk, as persisted. `spans` are already on the meeting's
/// timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkOutcome {
    pub index: usize,
    pub audio_start_seconds: f64,
    pub content_start_seconds: f64,
    pub content_end_seconds: f64,
    pub start_boundary: BoundaryKind,
    /// The kind of the cut this chunk ENDS at — persisted because a relaunch
    /// that resumes here has no other way to know what kind of seam it is
    /// standing at, and [`merge_timed`] picks its rule from that (see
    /// [`ResumePoint`]). `#[serde(default)]` so a ledger written before this
    /// field existed still loads: it costs the old, wrong `Edge` behaviour on
    /// exactly one seam of one already-in-flight meeting, where refusing to
    /// parse would cost the whole transcript.
    #[serde(default)]
    pub end_boundary: BoundaryKind,
    pub status: ChunkStatus,
    pub text: String,
    pub spans: Vec<TimedSpan>,
    pub timestamp_kind: TimedKind,
    /// Present only on [`ChunkStatus::AsrFailed`] — what went wrong, kept so a
    /// Diagnostics row can say which chunk died and why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ChunkOutcome {
    /// Build a finished chunk from a window and what the engine returned.
    /// Public because the eval harness assembles chunks exactly this way — a
    /// second copy of the shift-and-clip rule is a second thing to get wrong.
    pub fn from_transcript(window: &ChunkWindow, transcript: TimedTranscript) -> ChunkOutcome {
        let spans: Vec<TimedSpan> = transcript
            .best_spans()
            .iter()
            .map(|s| s.shifted(window.audio_start_seconds))
            .filter(|s| !s.text.is_empty())
            .collect();
        ChunkOutcome {
            index: window.index,
            audio_start_seconds: window.audio_start_seconds,
            content_start_seconds: window.content_start_seconds,
            content_end_seconds: window.content_end_seconds,
            start_boundary: window.start_boundary,
            end_boundary: window.end_boundary,
            status: ChunkStatus::Done,
            text: transcript.text,
            spans,
            timestamp_kind: transcript.kind,
            error: None,
        }
    }

    pub fn failed(window: &ChunkWindow, error: String) -> ChunkOutcome {
        ChunkOutcome {
            index: window.index,
            audio_start_seconds: window.audio_start_seconds,
            content_start_seconds: window.content_start_seconds,
            content_end_seconds: window.content_end_seconds,
            start_boundary: window.start_boundary,
            end_boundary: window.end_boundary,
            status: ChunkStatus::AsrFailed,
            text: String::new(),
            spans: Vec::new(),
            timestamp_kind: TimedKind::None,
            error: Some(error),
        }
    }
}

/// The resume ledger for one meeting (finding #3).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingProgress {
    pub meeting_id: String,
    /// Everything before this second has been decoded (or definitively failed).
    /// A relaunch starts exactly here.
    pub processed_through_seconds: f64,
    pub chunks: Vec<ChunkOutcome>,
}

impl MeetingProgress {
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }
}

/// Where per-chunk progress is written. YV94 lands the `meeting_chunks` table
/// and can implement this over SQLite; until then [`JsonProgressStore`] is a
/// file next to the wav, which is all "survive a kill -9" needs.
pub trait ProgressStore {
    fn load(&self, meeting_id: &str) -> Result<MeetingProgress, String>;
    /// Persist ONE finished chunk. Must be durable before it returns: this is
    /// the write that makes the difference between resuming and restarting.
    fn record_chunk(&mut self, meeting_id: &str, chunk: &ChunkOutcome) -> Result<(), String>;
}

/// Progress as a small JSON file per meeting, written whole and renamed into
/// place so a crash mid-write leaves the previous ledger intact.
pub struct JsonProgressStore {
    dir: PathBuf,
}

impl JsonProgressStore {
    pub fn new(dir: impl AsRef<Path>) -> JsonProgressStore {
        JsonProgressStore {
            dir: dir.as_ref().to_path_buf(),
        }
    }

    pub fn path_for(&self, meeting_id: &str) -> PathBuf {
        self.dir.join(format!("{meeting_id}.asr-progress.json"))
    }
}

impl ProgressStore for JsonProgressStore {
    fn load(&self, meeting_id: &str) -> Result<MeetingProgress, String> {
        let path = self.path_for(meeting_id);
        if !path.exists() {
            return Ok(MeetingProgress {
                meeting_id: meeting_id.to_string(),
                ..MeetingProgress::default()
            });
        }
        let raw = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        // A half-written or corrupt ledger costs a re-decode, never a lost
        // meeting: the audio is still on disk.
        match serde_json::from_str::<MeetingProgress>(&raw) {
            Ok(p) => Ok(p),
            Err(e) => {
                log::warn!("meeting {meeting_id}: unreadable ASR progress ({e}) — starting over");
                Ok(MeetingProgress {
                    meeting_id: meeting_id.to_string(),
                    ..MeetingProgress::default()
                })
            }
        }
    }

    fn record_chunk(&mut self, meeting_id: &str, chunk: &ChunkOutcome) -> Result<(), String> {
        let mut progress = self.load(meeting_id)?;
        progress.meeting_id = meeting_id.to_string();
        progress.chunks.retain(|c| c.index != chunk.index);
        progress.chunks.push(chunk.clone());
        progress.chunks.sort_by_key(|c| c.index);
        progress.processed_through_seconds = progress
            .chunks
            .iter()
            .map(|c| c.content_end_seconds)
            .fold(0.0f64, f64::max);
        fs::create_dir_all(&self.dir)
            .map_err(|e| format!("create {}: {e}", self.dir.display()))?;
        let path = self.path_for(meeting_id);
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_string(&progress).map_err(|e| format!("encode progress: {e}"))?;
        fs::write(&tmp, body).map_err(|e| format!("write {}: {e}", tmp.display()))?;
        fs::rename(&tmp, &path).map_err(|e| format!("rename {}: {e}", path.display()))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The driver
// ---------------------------------------------------------------------------

/// Knobs the driver takes beyond the geometry.
#[derive(Debug, Clone)]
pub struct MeetingAsrConfig {
    pub chunk: ChunkConfig,
    /// ISO code for the decode. English-only in 22-A (see
    /// [`meeting_availability`]).
    pub language: Option<String>,
    pub bias_prompt: Option<String>,
    /// How often the yield loop re-checks whether the dictation is through.
    pub yield_poll: Duration,
    /// How long meeting ASR will stand down before deciding the dictation side
    /// is wedged and taking the engine anyway. Without a ceiling a stuck
    /// interactive claim would stall a meeting forever.
    pub max_yield: Duration,
}

impl Default for MeetingAsrConfig {
    fn default() -> Self {
        MeetingAsrConfig {
            chunk: ChunkConfig::default(),
            language: Some("en".to_string()),
            bias_prompt: None,
            yield_poll: Duration::from_millis(20),
            max_yield: Duration::from_secs(120),
        }
    }
}

/// What a meeting transcription run produced.
#[derive(Debug, Clone, PartialEq)]
pub struct MeetingTranscript {
    pub text: String,
    /// Timed rows on the meeting timeline, monotonic by construction.
    ///
    /// **Invariant, on BOTH merge paths: `segments.join(" ") == text`.** This is
    /// the field YV94 persists into `meeting_segments` and the field the detail
    /// screen and the Markdown export render, so it — not `text` — is what the
    /// user actually reads. The fallback path used to build these rows from
    /// each chunk's RAW text, which still contained the overlap the merge had
    /// just removed, and shipped ~6 duplicated words at every seam to the user
    /// while the `text` beside it was clean. Asserted by
    /// `segments_reconstruct_the_merged_text_on_both_merge_paths`.
    pub segments: Vec<TimedSpan>,
    /// True when the model gave real alignment and [`merge_timed`] was the
    /// merge; false when the text-anchor fallback ran.
    pub timestamps_are_real: bool,
    pub merge: MergeReport,
    /// Chunks decoded in THIS run.
    pub chunks_decoded: usize,
    /// Chunks that were already in the ledger and were not decoded again.
    pub chunks_resumed: usize,
    /// Chunks (this run + resumed) that failed ASR.
    pub chunks_failed: usize,
    pub processed_through_seconds: f64,
    /// The quit signal fired before the last chunk. The ledger is complete up
    /// to `processed_through_seconds`.
    pub interrupted: bool,
    /// How many chunk boundaries stood down for a dictation.
    pub preempt_yields: usize,
    /// How many chunk decodes a dictation cancelled MID-chunk. Each one was
    /// re-decoded (or, at [`PREEMPTED_CHUNK_RETRIES`], left for the next run) —
    /// never written into the ledger as a failure.
    pub preempted_decodes: usize,
}

/// One meeting transcription job.
pub struct MeetingAsr<'a> {
    pub audio: &'a dyn SampleWindows,
    pub vad: Option<&'a dyn VoiceActivity>,
    pub asr: &'a dyn ChunkAsr,
    pub demand: &'a dyn EngineDemand,
    pub store: &'a mut dyn ProgressStore,
    /// Quit/cancel signal — the app is exiting, or the user deleted the
    /// meeting. Checked at chunk boundaries only, for the same reason
    /// preemption is: a cancelled decode is wasted work AND an abandoned engine.
    pub quit: &'a dyn Fn() -> bool,
    pub config: MeetingAsrConfig,
}

/// How many times a chunk that found the engine leased out is retried before it
/// is written off. Three attempts spans any plausible dictation (a 60 s take is
/// ~1 s on Metal) without letting a wedged engine stall the meeting.
const CONTENDED_CHUNK_RETRIES: usize = 3;

/// How many times one chunk may be cancelled mid-decode by a dictation before
/// the meeting stands down for this run rather than trying again.
///
/// Every retry is preceded by a full yield — the driver waits until no
/// dictation is pending at all — so reaching this bound means the user started
/// three more dictations while this one chunk was being decoded. Standing down
/// then is not a failure: the ledger is complete up to the previous chunk, so
/// the next run resumes exactly here. Spinning instead would be the livelock,
/// and writing the chunk off as `asr_failed` would be the data loss.
const PREEMPTED_CHUNK_RETRIES: usize = 3;

/// How close a ledger chunk's end has to be to the resume point to be the chunk
/// that MADE it. Both numbers are the same `f64` written and read back through
/// JSON, so this is a serialisation-noise tolerance, not a search radius — one
/// sample at 16 kHz is 62.5 µs, four orders of magnitude wider than this.
const RESUME_BOUNDARY_EPSILON: f64 = 1e-9;

/// Nothing to yield to, ever — for tests and for headless runs.
pub struct NoDemand;
impl EngineDemand for NoDemand {
    fn dictation_pending(&self) -> bool {
        false
    }
}

/// Never quits — the default for a run that has nothing to cancel it.
pub fn never_quit() -> impl Fn() -> bool {
    || false
}

impl MeetingAsr<'_> {
    pub fn run(&mut self, meeting_id: &str) -> Result<MeetingTranscript, String> {
        self.config.chunk.validate()?;
        let resume = self.store.load(meeting_id)?;
        let mut chunks: Vec<ChunkOutcome> = resume.chunks.clone();
        chunks.sort_by_key(|c| c.index);
        let resumed = chunks.len();
        // The resume point is a place AND a kind. The seconds come from the
        // ledger's high-water mark; the kind comes from whichever chunk ended
        // exactly there, because that chunk's `end_boundary` is the cut the
        // previous run made and the one `merge_timed` is about to stand at.
        // Defaulting it to `Edge` — which is what "a plan starts here" used to
        // mean — routes a fixed-clock resume seam through the intersection rule
        // and duplicates the word the cut truncated, so the resumed transcript
        // is NOT identical to an uninterrupted one. See [`ResumePoint`].
        let from_seconds = resume.processed_through_seconds.max(0.0);
        let from = ResumePoint::at(
            from_seconds,
            chunks
                .iter()
                .rev()
                .find(|c| (c.content_end_seconds - from_seconds).abs() <= RESUME_BOUNDARY_EPSILON)
                .map(|c| c.end_boundary)
                // No chunk ends here: either the ledger is empty (the top of the
                // audio, a genuine edge) or it disagrees with its own high-water
                // mark, in which case the conservative rule — the one that can
                // only ever keep a word twice, never delete one — is right.
                .unwrap_or(BoundaryKind::Edge),
        );
        if resumed > 0 {
            log::info!(
                "meeting {meeting_id}: resuming ASR at {from_seconds:.1}s at a {:?} boundary \
                 ({resumed} chunks already done)",
                from.boundary
            );
        }

        let plan = plan_windows(
            self.audio,
            self.vad,
            from,
            &self.config.chunk,
            chunks.last().map(|c| c.index + 1).unwrap_or(0),
        )?;

        let mut decoded = 0usize;
        let mut yields = 0usize;
        let mut preempted = 0usize;
        let mut interrupted = false;
        for window in &plan {
            if (self.quit)() {
                interrupted = true;
                break;
            }
            // Preemption happens HERE — between chunks, never inside one.
            if self.yield_to_interactive() {
                yields += 1;
            }
            if (self.quit)() {
                interrupted = true;
                break;
            }
            let samples = self
                .audio
                .window(window.audio_start_seconds, window.audio_end_seconds)?;
            let mut attempt = 0usize;
            let mut preempts = 0usize;
            let outcome = loop {
                match self.asr.transcribe_timed(&samples) {
                    Ok(transcript) => {
                        break Some(ChunkOutcome::from_transcript(window, transcript))
                    }
                    // A dictation arrived mid-chunk and took the engine off this
                    // decode. Not a failure and not contention: the chunk never
                    // ran. Its audio is still on disk, so it is re-decoded once
                    // the dictation is through — and NOT recorded in the ledger
                    // in between, because an `asr_failed` row would make a
                    // dictation the user issued at the wrong moment into a
                    // permanent hole in their transcript.
                    Err(ref e) if e == PREEMPTED_FOR_DICTATION => {
                        preempts += 1;
                        preempted += 1;
                        if preempts > PREEMPTED_CHUNK_RETRIES {
                            // The user is dictating continuously. Stand the
                            // meeting down rather than spin: the ledger is
                            // complete up to the previous chunk, so this window
                            // is simply the resume point next time.
                            log::info!(
                                "meeting {meeting_id}: chunk {} preempted {preempts} times — \
                                 standing down, it resumes at {:.1}s",
                                window.index,
                                window.content_start_seconds
                            );
                            break None;
                        }
                        if self.yield_to_interactive() {
                            yields += 1;
                        }
                    }
                    // The app is quitting and the exit drain cancelled this
                    // decode mid-chunk. Not a failure, and — unlike a preemption
                    // — not something to retry either: the engine has been
                    // unloaded and the process is leaving, so every retry would
                    // come back `NO_ENGINE_LOADED` and the third would be
                    // written off as `asr_failed`. Stand down with NOTHING
                    // recorded, so `processed_through_seconds` still points at
                    // the end of the last chunk that really was decoded and the
                    // next launch resumes exactly at this window.
                    //
                    // This arm is the other half of the fix in
                    // `transcription::abandon_in_flight`, and its absence is what
                    // made a Cmd-Q during chunk 1 of a five-chunk meeting leave
                    // 30–60 s permanently blank with the ledger claiming 300 s
                    // were done.
                    Err(ref e) if e == ABANDONED_FOR_EXIT => {
                        log::info!(
                            "meeting {meeting_id}: chunk {} abandoned to the exit drain — \
                             it resumes at {:.1}s",
                            window.index,
                            window.content_start_seconds
                        );
                        break None;
                    }
                    // Lost the engine to a dictation that arrived between the
                    // yield and the take. That is contention, not a decode
                    // failure: wait it out and decode this chunk properly rather
                    // than writing a hole into the transcript.
                    Err(ref e)
                        if e == NO_ENGINE_LOADED && attempt < CONTENDED_CHUNK_RETRIES =>
                    {
                        attempt += 1;
                        if self.yield_to_interactive() {
                            yields += 1;
                        }
                        std::thread::sleep(self.config.yield_poll);
                    }
                    Err(e) => {
                        // Finding #3's second new error-matrix row: one chunk's
                        // failure (a timeout abandons the engine, so the next
                        // chunk reloads) must not take the meeting with it.
                        log::warn!(
                            "meeting {meeting_id}: chunk {} ({:.1}s–{:.1}s) failed ASR: {e}",
                            window.index,
                            window.content_start_seconds,
                            window.content_end_seconds
                        );
                        break Some(ChunkOutcome::failed(window, e));
                    }
                }
            };
            let Some(outcome) = outcome else {
                interrupted = true;
                break;
            };
            self.store.record_chunk(meeting_id, &outcome)?;
            chunks.push(outcome);
            decoded += 1;
        }

        let mut out = assemble(chunks, resumed, decoded, yields, interrupted);
        out.preempted_decodes = preempted;
        Ok(out)
    }

    /// Stand down while a dictation wants the engine. Returns true if it
    /// actually waited.
    fn yield_to_interactive(&self) -> bool {
        if !self.demand.dictation_pending() {
            return false;
        }
        let started = Instant::now();
        while self.demand.dictation_pending() {
            if started.elapsed() >= self.config.max_yield {
                log::warn!(
                    "meeting ASR waited {}s for the dictation path and is taking the engine",
                    self.config.max_yield.as_secs()
                );
                break;
            }
            std::thread::sleep(self.config.yield_poll);
        }
        true
    }
}

/// Whether a set of finished chunks can be merged on TIME (the primary path)
/// rather than on text.
///
/// Public and used by the eval harness so there is one rule, not two: a second
/// copy of this predicate in `tests/meeting_eval.rs` is a second thing to get
/// wrong, and getting it wrong is invisible — the harness would score an arm the
/// app never takes.
///
/// A chunk that decoded to NOTHING is not evidence against the times. A 30 s
/// window in which nobody spoke — a lull, someone muted, a break — is a
/// perfectly SUCCESSFUL decode with zero spans, and demanding spans from it
/// demoted the whole meeting to the text fallback: every real word timestamp
/// from every other chunk discarded, segments collapsed to chunk granularity,
/// over one quiet window. So the requirement falls on the chunks that actually
/// produced text, and at least one chunk has to carry spans or there is nothing
/// to merge on at all.
pub fn timestamps_are_usable(chunks: &[ChunkOutcome]) -> bool {
    let decodable = chunks.iter().filter(|c| c.status == ChunkStatus::Done);
    let mut any_spans = false;
    for chunk in decodable {
        if !chunk.spans.is_empty() {
            any_spans = true;
        } else if !chunk.text.trim().is_empty() {
            return false;
        }
        if !chunk.text.trim().is_empty() && !chunk.timestamp_kind.has_times() {
            return false;
        }
    }
    any_spans
}

/// Merge finished chunks into one transcript. Split out of [`MeetingAsr::run`]
/// so a caller with a full ledger (a relaunch that has nothing left to decode,
/// the meeting-detail screen) can rebuild the transcript without any audio.
pub fn assemble(
    chunks: Vec<ChunkOutcome>,
    resumed: usize,
    decoded: usize,
    yields: usize,
    interrupted: bool,
) -> MeetingTranscript {
    let mut chunks = chunks;
    chunks.sort_by_key(|c| c.index);
    let failed = chunks
        .iter()
        .filter(|c| c.status == ChunkStatus::AsrFailed)
        .count();
    let processed_through = chunks
        .iter()
        .map(|c| c.content_end_seconds)
        .fold(0.0f64, f64::max);

    // PRIMARY path: every decoded chunk carried usable times.
    let timed = timestamps_are_usable(&chunks);

    if timed {
        let segments = merge_timed(&chunks);
        let text = segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        return MeetingTranscript {
            text: text.trim().to_string(),
            segments,
            timestamps_are_real: true,
            // One "seam" per window after the first, none of which needed the
            // text fallback.
            merge: MergeReport {
                seams: chunks.len().saturating_sub(1),
                ..MergeReport::default()
            },
            chunks_decoded: decoded,
            chunks_resumed: resumed,
            chunks_failed: failed,
            processed_through_seconds: processed_through,
            interrupted,
            preempt_yields: yields,
            // Filled in by the driver, which is the only place that knows.
            preempted_decodes: 0,
        };
    }

    // FALLBACK: no usable alignment — anchor the seams on text.
    let spoken: Vec<&ChunkOutcome> = chunks
        .iter()
        .filter(|c| !c.text.trim().is_empty())
        .collect();
    let per_chunk: Vec<Vec<String>> = spoken
        .iter()
        .map(|c| {
            c.text
                .split_whitespace()
                .map(|t| t.to_string())
                .collect::<Vec<String>>()
        })
        .collect();
    let segmented = merge_chunk_tokens_segmented(&per_chunk);
    let report = segmented.report;
    // Segments come out of the MERGED stream, never out of `c.text`: the raw
    // chunk text covers `audio_start..content_end` and therefore still carries
    // the overlap the merge just removed. Building rows from it re-emitted
    // ~6 duplicated words at every seam in the one field YV94 persists and the
    // detail UI renders, while the `text` beside it was clean. Slicing the
    // merged stream makes `segments.join(" ") == text` true by construction.
    let segments: Vec<TimedSpan> = spoken
        .iter()
        .enumerate()
        .map(|(i, c)| TimedSpan {
            start_seconds: c.content_start_seconds,
            end_seconds: c.content_end_seconds,
            text: segmented.chunk_text(i),
        })
        .filter(|s| !s.text.is_empty())
        .collect();
    MeetingTranscript {
        text: segmented.tokens.join(" ").trim().to_string(),
        segments,
        timestamps_are_real: false,
        merge: report,
        chunks_decoded: decoded,
        chunks_resumed: resumed,
        chunks_failed: failed,
        processed_through_seconds: processed_through,
        interrupted,
        preempt_yields: yields,
        preempted_decodes: 0,
    }
}

// ---------------------------------------------------------------------------
// The English-only gate (finding #38)
// ---------------------------------------------------------------------------

/// Why the Notetaker is unavailable, or `Ok` if it is not.
///
/// Shaped like the macOS-version gates elsewhere in the app: a single enum the
/// UI turns into an honest empty state, not a silent no-op. 22-A is English end
/// to end (Parakeet EN, `lang_detect:false`) and there is no meeting-language
/// picker path, so a Spanish "language I speak" would produce garbled English
/// rather than Spanish — refuse instead.
#[derive(Debug, Clone, PartialEq)]
pub enum MeetingUnavailable {
    /// No ASR model selected/downloaded yet.
    NoModel,
    /// The selected model is not in the catalog, so nothing is known about what
    /// it speaks.
    UnknownModel { model_id: String },
    /// The selected model does not do English at all.
    ModelNotEnglish {
        model_id: String,
        model_name: String,
        languages: Vec<String>,
    },
    /// The model does English, but the user's "Language I speak" is not.
    SpokenLanguageNotEnglish { language: String },
    /// YV101 (plan finding OS-11) — this Mac is older than the macOS 14.4 the
    /// CoreAudio process tap needs, so the *system-audio* track cannot run.
    ///
    /// Only ever returned for [`MeetingCapture::MicPlusSystemAudio`]. Mic-only
    /// recording keeps Yap's macOS 12 floor and is never refused for this
    /// reason — see [`meeting_availability_for`].
    RequiresMacOS14_4 { found: os_version_gate::OsVersion },
}

impl MeetingUnavailable {
    /// The sentence the empty state shows. No blame, one instruction.
    pub fn message(&self) -> String {
        match self {
            MeetingUnavailable::NoModel => {
                "Meeting notes need a speech model. Download one in Settings › Model to record a meeting.".to_string()
            }
            MeetingUnavailable::UnknownModel { model_id } => format!(
                "Meeting notes need an English speech model, and Yap can't tell what '{model_id}' transcribes. Pick a catalog model in Settings › Model."
            ),
            MeetingUnavailable::ModelNotEnglish {
                model_name,
                languages,
                ..
            } => format!(
                "Meeting notes are English-only for now. {model_name} transcribes {} — switch to an English model in Settings › Model to record a meeting.",
                languages.join(", ")
            ),
            MeetingUnavailable::SpokenLanguageNotEnglish { language } => format!(
                "Meeting notes are English-only for now. Your 'Language I speak' is set to {language} — set it to English to record a meeting."
            ),
            // The only refusal in this enum with no next step on the machine —
            // so the sentence spends its second half saying what still works,
            // rather than sending the user to a Settings pane that cannot fix
            // their OS version. The requirement text itself lives in exactly
            // one place (`os_version_gate::SYSTEM_AUDIO_REQUIREMENT`).
            MeetingUnavailable::RequiresMacOS14_4 { found } => {
                let requirement = os_version_gate::SYSTEM_AUDIO_REQUIREMENT;
                if found.is_known() {
                    format!(
                        "{requirement}, and this Mac is on macOS {found}. Meeting notes still record your microphone."
                    )
                } else {
                    format!("{requirement}. Meeting notes still record your microphone.")
                }
            }
        }
    }
}

/// The English code the gate accepts, plus the regional variants a settings
/// picker can produce.
fn is_english(code: &str) -> bool {
    let c = code.trim().to_ascii_lowercase();
    c == "en" || c.starts_with("en-") || c.starts_with("en_")
}

/// Which audio a meeting would record — the axis the macOS 14.4 gate splits on.
///
/// 22-A's mic-only recording is the app's floor-level capability and runs on
/// macOS 12. 22-B adds a second track captured through a CoreAudio process tap,
/// which does not exist below macOS 14.4. They are the same feature to the
/// user and two different capability questions to the app, so they are two
/// values here rather than two functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetingCapture {
    /// 22-A: the microphone only. Never gated on the OS version.
    MicOnly,
    /// 22-B: the microphone plus the other end of the call, captured through a
    /// CoreAudio process tap. Needs macOS 14.4.
    MicPlusSystemAudio,
}

/// Is the Notetaker available with this model + spoken-language setting?
///
/// `spoken_language` is the app's "Language I speak" setting; `None` (or an
/// empty string) means autodetect, which an English-only model resolves to
/// English anyway.
///
/// This is the **mic-only** door (22-A) and it stays that way: it can never
/// return [`MeetingUnavailable::RequiresMacOS14_4`], on any OS. YV101 must not
/// regress the macOS 12 floor mic-only recording already ships on.
pub fn meeting_availability(
    model_id: Option<&str>,
    spoken_language: Option<&str>,
) -> Result<(), MeetingUnavailable> {
    meeting_availability_for(
        MeetingCapture::MicOnly,
        model_id,
        spoken_language,
        os_version_gate::OsVersion::current(),
    )
}

/// [`meeting_availability`], asked about a specific capture mode on a specific
/// macOS version (YV101, plan finding OS-11).
///
/// `os` is a parameter rather than a read so the gate is pure and the table in
/// `tests/meeting_availability_144_gate.rs` can drive every version this app
/// will meet; production callers pass
/// [`os_version_gate::OsVersion::current`].
///
/// **Check order is deliberate.** The OS gate runs *before* the model and
/// language gates, and only for [`MeetingCapture::MicPlusSystemAudio`]. On a
/// macOS 13 machine the system-audio affordance is disabled no matter what is
/// in Settings — downloading a model or switching a language cannot make a
/// missing CoreAudio API appear — so that is the sentence the disabled control
/// has to show. The model/language refusals are shared with mic-only recording
/// and the Notetaker's own empty state already carries them.
pub fn meeting_availability_for(
    capture: MeetingCapture,
    model_id: Option<&str>,
    spoken_language: Option<&str>,
    os: os_version_gate::OsVersion,
) -> Result<(), MeetingUnavailable> {
    if capture == MeetingCapture::MicPlusSystemAudio {
        if let os_version_gate::SystemAudioGate::RequiresMacOS14_4 { found } =
            os_version_gate::system_audio_gate(os)
        {
            return Err(MeetingUnavailable::RequiresMacOS14_4 { found });
        }
    }
    let Some(model_id) = model_id.map(str::trim).filter(|m| !m.is_empty()) else {
        return Err(MeetingUnavailable::NoModel);
    };
    let Some(model) = models::catalog_model(model_id) else {
        return Err(MeetingUnavailable::UnknownModel {
            model_id: model_id.to_string(),
        });
    };
    if !model.languages.is_empty() && !model.languages.iter().any(|l| is_english(l)) {
        return Err(MeetingUnavailable::ModelNotEnglish {
            model_id: model.id.clone(),
            model_name: model.name.clone(),
            languages: model.languages.clone(),
        });
    }
    if let Some(language) = spoken_language.map(str::trim).filter(|l| !l.is_empty()) {
        if !is_english(language) {
            return Err(MeetingUnavailable::SpokenLanguageNotEnglish {
                language: language.to_string(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(v: &[(f64, f64)]) -> Vec<VoicedSpan> {
        v.iter()
            .map(|(a, b)| VoicedSpan {
                start_seconds: *a,
                end_seconds: *b,
            })
            .collect()
    }

    /// The geometry the plan specifies, and the constraint finding #3 puts on
    /// it: past `TRANSCRIBE_TIMEOUT` the manager abandons the ENGINE, so the
    /// widest window this config can produce must decode well inside it.
    #[test]
    fn default_geometry_is_inside_the_transcribe_timeout_ceiling() {
        let cfg = ChunkConfig::default();
        assert_eq!(cfg.target_seconds, 30.0);
        assert_eq!((cfg.min_seconds, cfg.max_seconds), (25.0, 35.0));
        assert_eq!(cfg.overlap_seconds, 2.0);
        cfg.validate().expect("the shipped geometry validates");
        assert!(
            cfg.max_decode_seconds() < TRANSCRIBE_TIMEOUT.as_secs_f64(),
            "a full-width window has to fit inside the timeout that abandons the engine"
        );

        // …and a geometry that does not is refused rather than shipped.
        let greedy = ChunkConfig {
            max_seconds: 300.0,
            ..cfg
        };
        assert!(greedy.validate().is_err());
        let inverted = ChunkConfig {
            target_seconds: 40.0,
            ..cfg
        };
        assert!(inverted.validate().is_err());
        let swallowed = ChunkConfig {
            overlap_seconds: 30.0,
            ..cfg
        };
        assert!(swallowed.validate().is_err());
    }

    /// The point of the whole VAD-cut design, and the property the eval
    /// harness's corpus test measures on real speech: with a pause at least as
    /// long as the overlap, the ENTIRE overlap the next window re-sees is
    /// silence — so there is nothing at the seam for either window to
    /// duplicate.
    #[test]
    fn the_overlap_lands_inside_the_pause() {
        let cfg = ChunkConfig::default();
        // Speech everywhere in [25, 35] except a 3 s pause at 30.0–33.0.
        let voiced = spans(&[(25.0, 30.0), (33.0, 35.0)]);
        let (boundary, kind) = pick_boundary(30.0, (25.0, 35.0), &voiced, cfg.min_silence_seconds);
        assert_eq!(kind, BoundaryKind::Silence);
        assert!(
            boundary - cfg.overlap_seconds >= 30.0 && boundary <= 33.0,
            "overlap [{}, {boundary}] is not inside the pause [30, 33]",
            boundary - cfg.overlap_seconds
        );
    }

    /// A pause SHORTER than the overlap cannot hide the whole overlap — so the
    /// cut goes as late in it as the margin allows, which is the placement that
    /// puts the most silence in the overlap. Cutting at the pause's middle (the
    /// obvious first idea, and what this function did first) leaves 0.5 s more
    /// speech in the overlap for no benefit.
    #[test]
    fn a_short_pause_is_cut_as_late_as_the_margin_allows() {
        let cfg = ChunkConfig::default();
        let voiced = spans(&[(25.0, 31.0), (32.0, 35.0)]);
        let (boundary, kind) = pick_boundary(30.0, (25.0, 35.0), &voiced, cfg.min_silence_seconds);
        assert_eq!(kind, BoundaryKind::Silence);
        assert!(
            (boundary - (32.0 - SILENCE_EDGE_MARGIN)).abs() < 1e-9,
            "cut at {boundary}, expected {}",
            32.0 - SILENCE_EDGE_MARGIN
        );
        assert!(boundary < 32.0, "the cut stays on the quiet side of onset");
        let speech_in_overlap = 31.0 - (boundary - cfg.overlap_seconds);
        assert!(
            speech_in_overlap < 1.1,
            "{speech_in_overlap}s of speech in the overlap"
        );
    }

    /// Two pauses: the one whose cut lands nearest the 30 s target wins, not the
    /// first one and not the longest one.
    #[test]
    fn the_pause_nearest_the_target_wins() {
        let voiced = spans(&[(25.0, 26.0), (27.0, 29.6), (30.4, 35.0)]);
        let (boundary, kind) = pick_boundary(30.0, (25.0, 35.0), &voiced, 0.2);
        assert_eq!(kind, BoundaryKind::Silence);
        assert!(
            (boundary - (30.4 - SILENCE_EDGE_MARGIN)).abs() < 1e-9,
            "cut at {boundary}, expected the 29.6–30.4 pause"
        );
    }

    /// A stop consonant is not a pause: a gap under the floor is not a place to
    /// cut, and with nothing else on offer the clock decides — and SAYS so, so
    /// the seam knows it may need the text fallback.
    #[test]
    fn a_gap_under_the_floor_is_not_a_boundary() {
        let voiced = spans(&[(25.0, 29.95), (30.0, 35.0)]);
        let (boundary, kind) = pick_boundary(30.0, (25.0, 35.0), &voiced, 0.2);
        assert_eq!(kind, BoundaryKind::FixedClock);
        assert_eq!(boundary, 30.0);
    }

    /// Wall-to-wall speech, no VAD at all, and a VAD that failed all end the
    /// same honest way.
    #[test]
    fn unbroken_speech_falls_back_to_the_clock() {
        let voiced = spans(&[(25.0, 35.0)]);
        let (boundary, kind) = pick_boundary(30.0, (25.0, 35.0), &voiced, 0.2);
        assert_eq!(kind, BoundaryKind::FixedClock);
        assert_eq!(boundary, 30.0);
        let (b2, k2) = pick_boundary(30.0, (25.0, 35.0), &[], 0.2);
        assert_eq!(k2, BoundaryKind::Silence, "a silent region is all pause");
        assert!(
            b2 > 30.0 && b2 <= 35.0,
            "a wholly silent region cuts as late as it can: {b2}"
        );
    }

    /// Windows cover the whole meeting exactly once (content ranges tile it)
    /// and re-see exactly the overlap.
    #[test]
    fn windows_tile_the_meeting_and_overlap_by_exactly_the_overlap() {
        let cfg = ChunkConfig::default();
        let plan = plan_windows_fixed(100.0, ResumePoint::start(), &cfg, 0);
        assert!(plan.len() >= 3, "100 s at 30 s windows: {}", plan.len());
        assert_eq!(plan[0].content_start_seconds, 0.0);
        assert_eq!(
            plan.last().unwrap().content_end_seconds,
            100.0,
            "the last window ends at the end of the audio"
        );
        for pair in plan.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            assert_eq!(
                a.content_end_seconds, b.content_start_seconds,
                "content ranges must tile with no gap and no overlap"
            );
            assert!(
                (b.content_start_seconds - b.audio_start_seconds - cfg.overlap_seconds).abs()
                    < 1e-9,
                "window {} re-sees {}s, expected {}s",
                b.index,
                b.content_start_seconds - b.audio_start_seconds,
                cfg.overlap_seconds
            );
            assert!(b.audio_seconds() <= cfg.max_decode_seconds() + 1e-9);
        }
        // The first window has nothing to re-see.
        assert_eq!(plan[0].audio_start_seconds, plan[0].content_start_seconds);
    }

    /// A short meeting is one window, and an empty one is no windows at all
    /// (rather than a zero-length decode).
    #[test]
    fn short_and_empty_meetings_do_not_produce_junk_windows() {
        let cfg = ChunkConfig::default();
        let one = plan_windows_fixed(12.0, ResumePoint::start(), &cfg, 0);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].audio_start_seconds, 0.0);
        assert_eq!(one[0].audio_end_seconds, 12.0);
        assert!(plan_windows_fixed(0.0, ResumePoint::start(), &cfg, 0).is_empty());
    }

    /// Resuming plans from the resume point — the audio before it is never read
    /// again, and the window indices carry on from the ledger.
    #[test]
    fn a_resumed_plan_starts_at_the_resume_point() {
        let cfg = ChunkConfig::default();
        let plan = plan_windows_fixed(200.0, ResumePoint::at(61.0, BoundaryKind::FixedClock), &cfg, 2);
        assert_eq!(plan[0].content_start_seconds, 61.0);
        assert_eq!(plan[0].index, 2);
        assert_eq!(
            plan[0].audio_start_seconds, 59.0,
            "a resumed run re-sees the overlap so the decoder has left context"
        );
    }

    struct ScriptedVad(Vec<(f64, f64)>);
    impl VoiceActivity for ScriptedVad {
        fn voiced_spans(&self, samples: &[f32]) -> Result<Vec<VoicedSpan>, String> {
            let len = samples.len() as f64 / MEETING_RATE as f64;
            Ok(self
                .0
                .iter()
                .filter(|(a, _)| *a < len)
                .map(|(a, b)| VoicedSpan {
                    start_seconds: *a,
                    end_seconds: b.min(len),
                })
                .collect())
        }
    }

    /// End to end over a scripted VAD: every interior boundary sits in a pause,
    /// no window is outside the search window, and the tiling still holds.
    #[test]
    fn the_vad_plan_cuts_every_boundary_in_a_pause() {
        let cfg = ChunkConfig::default();
        let audio = MemoryWindows::at_meeting_rate(vec![0.0; MEETING_RATE as usize * 200]);
        // A 0.6 s pause every 10 s of the ten-second search region, so each
        // region has one at +6 s (i.e. 31 s past the previous boundary).
        let vad = ScriptedVad(vec![(0.0, 6.0), (6.6, 10.0)]);
        let plan = plan_windows(&audio, Some(&vad), ResumePoint::start(), &cfg, 0).expect("plan");
        assert!(plan.len() >= 6);
        for w in plan.iter().skip(1) {
            assert_eq!(
                w.start_boundary,
                BoundaryKind::Silence,
                "window {} cut on the clock",
                w.index
            );
            assert!(w.content_seconds() >= cfg.min_seconds - 1e-9 || w.index == plan.len() - 1);
            assert!(w.content_seconds() <= cfg.max_seconds + 1e-9);
        }
        for pair in plan.windows(2) {
            assert_eq!(pair[0].content_end_seconds, pair[1].content_start_seconds);
        }
        assert_eq!(plan.last().unwrap().content_end_seconds, 200.0);
    }

    /// A resumed plan starts at the cut the PREVIOUS run made, and says so.
    ///
    /// The unit-level half of the resume-seam fix. `windows_from_boundaries`
    /// takes `kinds[0]` from the caller, and both planners used to hard-code
    /// that seed to `Edge` — the honest description of a plan that begins at
    /// 0.0 s, and a false one for every plan that begins at a resume point.
    /// `merge_timed` reads exactly this field to choose its seam rule, so an
    /// `Edge` here silently swaps the "did the cut truncate this word?" test for
    /// the "do these two spans cover the same instant?" one, and keeps both
    /// decodes of a truncated word.
    #[test]
    fn a_resumed_plan_carries_the_kind_of_the_boundary_it_resumes_at() {
        let cfg = ChunkConfig::default();
        let audio = MemoryWindows::at_meeting_rate(vec![0.0; MEETING_RATE as usize * 200]);

        for kind in [BoundaryKind::FixedClock, BoundaryKind::Silence] {
            let fixed = plan_windows_fixed(200.0, ResumePoint::at(90.0, kind), &cfg, 3);
            assert_eq!(fixed[0].content_start_seconds, 90.0);
            assert_eq!(fixed[0].start_boundary, kind, "fixed-clock planner");

            let vadless =
                plan_windows(&audio, None, ResumePoint::at(90.0, kind), &cfg, 3).expect("plan");
            assert_eq!(vadless[0].start_boundary, kind, "no-VAD planner");
        }

        // …and the top of the audio is still a true edge.
        assert_eq!(
            plan_windows_fixed(200.0, ResumePoint::start(), &cfg, 0)[0].start_boundary,
            BoundaryKind::Edge
        );
    }

    /// Every window records the cut it ENDS at, because that is the only thing a
    /// relaunch can read the resume point's kind out of: the run that chose it
    /// is gone, and the window that would have carried it as `start_boundary` is
    /// precisely the one that never got decoded.
    #[test]
    fn a_window_records_the_boundary_it_ends_at_and_it_is_the_next_windows_start() {
        let cfg = ChunkConfig::default();
        let plan = plan_windows_fixed(100.0, ResumePoint::start(), &cfg, 0);
        assert!(plan.len() >= 3);
        for pair in plan.windows(2) {
            assert_eq!(
                pair[0].end_boundary, pair[1].start_boundary,
                "one boundary, two windows — they cannot disagree about its kind"
            );
        }
        assert_eq!(
            plan.last().expect("windows").end_boundary,
            BoundaryKind::Edge,
            "the end of the audio is an edge, not a cut"
        );
        assert_eq!(plan[0].end_boundary, BoundaryKind::FixedClock);

        // It survives the ledger: `ChunkOutcome` is what a relaunch reads.
        let chunk = ChunkOutcome::failed(&plan[0], "irrelevant".to_string());
        let round_tripped: ChunkOutcome =
            serde_json::from_str(&serde_json::to_string(&chunk).expect("encode")).expect("decode");
        assert_eq!(round_tripped.end_boundary, BoundaryKind::FixedClock);
    }

    /// No VAD is not an error — it is the fixed clock, which is what the eval
    /// corpus was measured on.
    #[test]
    fn planning_without_a_vad_is_the_fixed_clock() {
        let cfg = ChunkConfig::default();
        let audio = MemoryWindows::at_meeting_rate(vec![0.0; MEETING_RATE as usize * 100]);
        let with_none = plan_windows(&audio, None, ResumePoint::start(), &cfg, 0).expect("plan");
        assert_eq!(with_none, plan_windows_fixed(100.0, ResumePoint::start(), &cfg, 0));
    }

    fn outcome(index: usize, start: f64, end: f64, spans: &[(f64, f64, &str)]) -> ChunkOutcome {
        ChunkOutcome {
            index,
            audio_start_seconds: (start - 2.0).max(0.0),
            content_start_seconds: start,
            content_end_seconds: end,
            start_boundary: BoundaryKind::Silence,
            end_boundary: BoundaryKind::Silence,
            status: ChunkStatus::Done,
            text: spans
                .iter()
                .map(|(_, _, t)| *t)
                .collect::<Vec<_>>()
                .join(" "),
            spans: spans
                .iter()
                .map(|(a, b, t)| TimedSpan {
                    start_seconds: *a,
                    end_seconds: *b,
                    text: (*t).to_string(),
                })
                .collect(),
            timestamp_kind: TimedKind::Word,
            error: None,
        }
    }

    /// The primary merge: the overlap words the incoming window re-decoded are
    /// dropped because their midpoints belong to the previous window — not
    /// because their text matched anything.
    #[test]
    fn the_timed_merge_drops_the_overlap_without_looking_at_the_text() {
        let a = outcome(
            0,
            0.0,
            30.0,
            &[(28.0, 28.5, "pineapple"), (29.0, 29.6, "trombone")],
        );
        // The incoming window re-saw 28.0–30.0 and decoded those two words
        // AGAIN, with different emission times — plus its own content.
        let b = outcome(
            1,
            30.0,
            60.0,
            &[
                (28.1, 28.6, "pineapple"),
                (29.05, 29.7, "trombone"),
                (30.4, 31.0, "lantern"),
            ],
        );
        let merged = merge_timed(&[a, b]);
        let words: Vec<&str> = merged.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(words, vec!["pineapple", "trombone", "lantern"]);
        assert!(merged.windows(2).all(|p| p[0].start_seconds <= p[1].start_seconds));
    }

    /// The tie-break that saves a half-cut word must not eat a word the speaker
    /// genuinely said twice — and this is the exact shape the VAD-cut chunker
    /// manufactures, because the pause between "Right." and "Right," is where it
    /// deliberately puts the boundary.
    ///
    /// Reproduced against the shipped merge before the fix: it returned
    /// `["okay", "Right,", "so"]` — the second-to-last real word deleted, which
    /// is precisely the plan's falsifiable line
    /// `seam dedupe never deletes real words` failing on the PRIMARY path while
    /// the eval harness only exercised the text fallback.
    #[test]
    fn seam_dedupe_never_deletes_real_words_on_the_timed_merge() {
        // The speaker says "Right." then, after a pause, "Right, so …" — and the
        // chunker cut in that pause, so the two live in different windows.
        let a = outcome(
            0,
            0.0,
            30.0,
            &[(28.6, 29.0, "okay"), (29.4, 29.9, "Right.")],
        );
        let b = outcome(1, 30.0, 60.0, &[(30.2, 30.7, "Right,"), (31.0, 31.6, "so")]);
        assert_eq!(
            b.start_boundary,
            BoundaryKind::Silence,
            "the shape only arises at a cut the VAD put in a pause"
        );
        let merged = merge_timed(&[a, b]);
        let words: Vec<&str> = merged.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            words,
            vec!["okay", "Right.", "Right,", "so"],
            "a genuine repetition across a VAD-cut boundary is two DISJOINT \
             spans, not one word decoded twice"
        );
    }

    /// The same falsifiable line at the OTHER seam, which is where it was still
    /// failing after the first fix: a repetition with no pause in it — a
    /// stutter — straddling a [`BoundaryKind::FixedClock`] cut.
    ///
    /// The first cut of the fix left fixed-clock seams on pure text+proximity,
    /// reasoning that "a repetition across a pause is not a shape that can
    /// occur at a cut with no pause in it". True and beside the point: the
    /// repetition that occurs at a pauseless cut is a stutter, which HAS no
    /// pause, and no pause is exactly the condition that makes the boundary
    /// fixed-clock in the first place. Against that code this input returned
    /// `["think", "that", "is", "fine"]` — the speaker said five words and four
    /// came back — and no test covered it, because
    /// `seam_dedupe_never_deletes_real_words_on_the_timed_merge` asserts a
    /// Silence seam and the eval corpus is `say`-generated TTS, which never
    /// stutters.
    ///
    /// Blast radius, which is why this is not a corner case:
    /// `plan_windows(audio, None, …)` falls back to `plan_windows_fixed`, whose
    /// every interior boundary is `FixedClock` — so a meeting transcribed on a
    /// build with no Silero instance took proximity-only dedupe at EVERY seam.
    #[test]
    fn seam_dedupe_never_deletes_real_words_at_a_fixed_clock_seam() {
        // "…think that / that is fine" — a stutter, spoken straight through.
        // The first "that" ENDS at 29.85, a clear 0.15 s before the cut at
        // 30.0: the outgoing window's audio ran past it and the model timed it
        // where the speaker actually stopped. A word the cut truncated could
        // not do that — the buffer ends at 30.0.
        let a = outcome(
            0,
            0.0,
            30.0,
            &[(29.3, 29.5, "think"), (29.6, 29.85, "that")],
        );
        let mut b = outcome(
            1,
            30.0,
            60.0,
            &[
                (30.0, 30.25, "that"),
                (30.4, 30.6, "is"),
                (30.7, 30.9, "fine"),
            ],
        );
        b.start_boundary = BoundaryKind::FixedClock;
        let merged = merge_timed(&[a, b]);
        let words: Vec<&str> = merged.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            words,
            vec!["think", "that", "that", "is", "fine"],
            "a stutter at a fixed-clock seam is two real words, not one word \
             decoded twice"
        );
    }

    /// The same shape at a fixed-clock seam reached through the SHIPPED entry
    /// point rather than a hand-set `start_boundary`, because that is the path
    /// a real meeting takes when no VAD is available: `plan_windows(audio,
    /// None, …)` is `plan_windows_fixed`, and every interior boundary it makes
    /// is `FixedClock`.
    #[test]
    fn a_vad_less_plan_makes_fixed_clock_seams_everywhere() {
        let cfg = ChunkConfig::default();
        let audio = MemoryWindows::at_meeting_rate(vec![0.0f32; MEETING_RATE as usize * 200]);
        let plan = plan_windows(&audio, None, ResumePoint::start(), &cfg, 0).expect("a plan");
        let kinds: Vec<BoundaryKind> = plan.iter().map(|w| w.start_boundary).collect();
        assert!(
            kinds.len() >= 6,
            "200 s at a 30 s target is more than six windows: {kinds:?}"
        );
        assert!(
            kinds[1..].iter().all(|k| *k == BoundaryKind::FixedClock),
            "every interior boundary of a VAD-less plan is fixed-clock: {kinds:?}"
        );
    }

    /// …and the case the tie-break exists for still fires, at the only kind of
    /// seam where a word CAN be cut in half: a fixed-clock cut, which happens
    /// exactly when the VAD found no pause anywhere in the search window.
    #[test]
    fn a_word_the_boundary_cut_in_half_is_still_deduped() {
        let a = outcome(
            0,
            0.0,
            30.0,
            &[(28.0, 28.5, "the"), (29.8, 30.0, "particular")],
        );
        let mut b = outcome(
            1,
            30.0,
            60.0,
            &[(30.1, 30.6, "particular"), (30.8, 31.2, "case")],
        );
        b.start_boundary = BoundaryKind::FixedClock;
        let merged = merge_timed(&[a, b]);
        let words: Vec<&str> = merged.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            words,
            vec!["the", "particular", "case"],
            "the later window saw the whole word and wins"
        );
    }

    /// A VAD-cut seam still dedupes the one duplicate a pause cannot explain:
    /// two spans that cover the SAME INSTANT, which is the same audio decoded
    /// twice however imperfect the VAD's idea of "silence" was.
    #[test]
    fn a_silence_seam_still_dedupes_two_decodes_of_the_same_instant() {
        let a = outcome(0, 0.0, 30.0, &[(28.0, 28.5, "the"), (29.7, 30.0, "point")]);
        let b = outcome(1, 30.0, 60.0, &[(29.9, 30.4, "point"), (30.8, 31.2, "is")]);
        assert_eq!(b.start_boundary, BoundaryKind::Silence);
        let merged = merge_timed(&[a, b]);
        let words: Vec<&str> = merged.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(words, vec!["the", "point", "is"]);
    }

    /// One 30 s window in which nobody spoke — a lull, a mute, a break — is a
    /// SUCCESSFUL decode with zero spans. Demanding spans from it used to demote
    /// the entire meeting to the text fallback, discarding every real word
    /// timestamp from every other chunk.
    #[test]
    fn one_silent_chunk_does_not_demote_the_whole_meeting_to_the_text_fallback() {
        let a = outcome(0, 0.0, 30.0, &[(1.0, 1.5, "alpha")]);
        let mut silent = outcome(1, 30.0, 60.0, &[]);
        silent.timestamp_kind = TimedKind::None; // no audio, nothing to time
        let b = outcome(2, 60.0, 90.0, &[(61.0, 61.5, "beta")]);

        let out = assemble(vec![a, silent, b], 0, 3, 0, false);
        assert!(
            out.timestamps_are_real,
            "a silent window is not evidence against the other windows' times"
        );
        assert_eq!(out.text, "alpha beta");
        assert_eq!(out.segments.len(), 2);
        assert_eq!(out.chunks_failed, 0);
    }

    /// …and with nothing timed anywhere, the fallback is still what runs.
    #[test]
    fn a_meeting_with_no_spans_at_all_still_falls_back_to_text() {
        let mut a = outcome(0, 0.0, 30.0, &[]);
        a.text = "alpha beta gamma".into();
        a.timestamp_kind = TimedKind::None;
        let out = assemble(vec![a], 0, 1, 0, false);
        assert!(!out.timestamps_are_real);
        assert_eq!(out.text, "alpha beta gamma");
    }

    /// **The user-visible transcript is `segments`, not `text`.** `segments` is
    /// what YV94 persists into `meeting_segments` and what the detail screen and
    /// the Markdown export render, so a merge that dedupes `text` and leaves the
    /// overlap in `segments` ships the duplicate to the user anyway. Both merge
    /// paths must agree.
    #[test]
    fn segments_reconstruct_the_merged_text_on_both_merge_paths() {
        // PRIMARY (timed) path.
        let a = outcome(
            0,
            0.0,
            30.0,
            &[(28.0, 28.5, "pineapple"), (29.0, 29.6, "trombone")],
        );
        let b = outcome(
            1,
            30.0,
            60.0,
            &[
                (28.1, 28.6, "pineapple"),
                (29.05, 29.7, "trombone"),
                (30.4, 31.0, "lantern"),
            ],
        );
        let timed = assemble(vec![a, b], 0, 2, 0, false);
        assert!(timed.timestamps_are_real);
        assert_eq!(joined(&timed.segments), timed.text);

        // FALLBACK (text-anchor) path — the chunk texts carry the overlap.
        let mut a = outcome(0, 0.0, 30.0, &[]);
        a.text = "the quick brown fox jumps over the lazy".into();
        a.timestamp_kind = TimedKind::None;
        let mut b = outcome(1, 30.0, 60.0, &[]);
        b.text = "over the lazy dog and then it stopped".into();
        b.timestamp_kind = TimedKind::None;

        let fallback = assemble(vec![a, b], 0, 2, 0, false);
        assert!(!fallback.timestamps_are_real);
        assert_eq!(
            fallback.text,
            "the quick brown fox jumps over the lazy dog and then it stopped"
        );
        assert_eq!(
            joined(&fallback.segments),
            fallback.text,
            "the segment rows used to re-emit the overlap the text had just \
             deduped — ~6 duplicated words at every seam"
        );
        assert_eq!(fallback.segments.len(), 2);
        assert_eq!(
            fallback.segments[1].text, "dog and then it stopped",
            "the incoming chunk owns only what it added past the anchor"
        );
    }

    /// A seam that finds no anchor appends the chunk whole — a VISIBLE duplicate
    /// rather than a silent deletion — and the segment rows must show exactly
    /// what the text does, duplicate included.
    #[test]
    fn segments_track_an_unanchored_seam_too() {
        let mut a = outcome(0, 0.0, 30.0, &[]);
        a.text = "alpha beta gamma".into();
        a.timestamp_kind = TimedKind::None;
        let mut b = outcome(1, 30.0, 60.0, &[]);
        b.text = "delta epsilon zeta".into();
        b.timestamp_kind = TimedKind::None;
        let out = assemble(vec![a, b], 0, 2, 0, false);
        assert_eq!(out.merge.no_anchor_seams, 1);
        assert_eq!(joined(&out.segments), out.text);
        assert_eq!(out.segments[0].text, "alpha beta gamma");
        assert_eq!(out.segments[1].text, "delta epsilon zeta");
    }

    /// The ranges a segmented merge hands back are a PARTITION of the merged
    /// stream: contiguous, in order, covering every token exactly once. The tail
    /// trim is the part that makes this non-trivial — it shortens what an
    /// earlier chunk owns after that chunk's range was already recorded.
    #[test]
    fn segmented_merge_ranges_partition_the_merged_stream() {
        let chunks: Vec<Vec<String>> = [
            "one two three four five six seven",
            "five six seven eight nine ten eleven",
            "ten eleven twelve thirteen",
            "nothing in common here at all",
        ]
        .iter()
        .map(|s| s.split(' ').map(String::from).collect())
        .collect();
        let seg = merge_chunk_tokens_segmented(&chunks);
        assert_eq!(seg.chunk_ranges.len(), chunks.len());
        let mut cursor = 0usize;
        for (start, end) in &seg.chunk_ranges {
            assert_eq!(
                *start, cursor,
                "ranges must be contiguous: {:?}",
                seg.chunk_ranges
            );
            assert!(end >= start);
            cursor = *end;
        }
        assert_eq!(
            cursor,
            seg.tokens.len(),
            "ranges must cover the whole merge"
        );
        let rebuilt: Vec<String> = (0..chunks.len())
            .map(|i| seg.chunk_text(i))
            .filter(|t| !t.is_empty())
            .collect();
        assert_eq!(rebuilt.join(" "), seg.tokens.join(" "));
    }

    fn joined(segments: &[TimedSpan]) -> String {
        segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The fallback merge removes the repeat when there are no times to merge
    /// on — and keeps the ORIGINAL casing/punctuation while matching on the
    /// folded form.
    #[test]
    fn the_text_fallback_removes_the_repeat_and_keeps_punctuation() {
        let a: Vec<String> = "the quick brown fox jumps over the lazy"
            .split(' ')
            .map(String::from)
            .collect();
        let b: Vec<String> = "over the lazy dog, and then it stopped."
            .split(' ')
            .map(String::from)
            .collect();
        let (merged, report) = merge_chunk_tokens_reporting(&[a, b]);
        assert_eq!(
            merged.join(" "),
            "the quick brown fox jumps over the lazy dog, and then it stopped."
        );
        assert_eq!(report.seams, 1);
        assert_eq!(report.no_anchor_seams, 0);
    }

    /// An unanchored seam is COUNTED rather than guessed at: emitting a visible
    /// duplicate beats deleting words nobody can get back.
    #[test]
    fn an_unanchored_seam_is_reported_not_guessed() {
        let a: Vec<String> = "alpha beta gamma".split(' ').map(String::from).collect();
        let b: Vec<String> = "delta epsilon zeta".split(' ').map(String::from).collect();
        let (merged, report) = merge_chunk_tokens_reporting(&[a, b]);
        assert_eq!(merged.len(), 6);
        assert_eq!(report.no_anchor_seams, 1);
        assert_eq!(report.tail_tokens_trimmed + report.head_tokens_skipped, 0);
    }

    /// The bound the seam gate relies on: a merge can only ever move tokens
    /// that live inside the overlap.
    #[test]
    fn a_seam_can_only_move_the_overlaps_worth_of_tokens() {
        assert_eq!(MAX_TAIL_TRIM, OVERLAP_TOKEN_BUDGET);
        assert!(MAX_HEAD_SKIP <= OVERLAP_TOKEN_BUDGET);
        assert!(MIN_ANCHOR_TOKENS >= 3, "two tokens is a coincidence");
    }

    /// A failed chunk is a hole in the transcript, not the end of it — and the
    /// ledger still advances past it so a relaunch does not retry it forever.
    #[test]
    fn a_failed_chunk_leaves_its_neighbours_alone() {
        let good_a = outcome(0, 0.0, 30.0, &[(1.0, 1.5, "alpha")]);
        let mut bad = outcome(1, 30.0, 60.0, &[]);
        bad.status = ChunkStatus::AsrFailed;
        bad.text = String::new();
        bad.timestamp_kind = TimedKind::None;
        bad.error = Some("transcription timed out after 120s".into());
        let good_b = outcome(2, 60.0, 90.0, &[(61.0, 61.5, "omega")]);

        let out = assemble(vec![good_a, bad, good_b], 0, 3, 0, false);
        assert_eq!(out.chunks_failed, 1);
        assert_eq!(out.processed_through_seconds, 90.0);
        assert_eq!(out.text, "alpha omega");
        assert!(out.timestamps_are_real);
    }

    /// The English-only gate (finding #38), against the REAL bundled catalog.
    #[test]
    fn the_english_gate_names_what_is_wrong() {
        assert!(meeting_availability(Some("handy-computer/parakeet-unified-en-0.6b-gguf"), None).is_ok());
        assert!(meeting_availability(
            Some("handy-computer/parakeet-unified-en-0.6b-gguf"),
            Some("en-US")
        )
        .is_ok());
        let blocked = meeting_availability(
            Some("handy-computer/parakeet-unified-en-0.6b-gguf"),
            Some("es"),
        )
        .expect_err("a Spanish setting is refused");
        assert!(blocked.message().contains("English-only"), "{}", blocked.message());
        assert_eq!(meeting_availability(None, None), Err(MeetingUnavailable::NoModel));
        assert!(matches!(
            meeting_availability(Some("some/local-model"), None),
            Err(MeetingUnavailable::UnknownModel { .. })
        ));
    }
}
