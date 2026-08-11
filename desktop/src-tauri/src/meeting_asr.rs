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
//!    insertions went from 12 to 0 against the fixed clock (WER 0.0087 → 0.0042).
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
use crate::models;
use crate::transcription::{TranscriptionManager, NO_ENGINE_LOADED, TRANSCRIBE_TIMEOUT};
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
        let ceiling = TRANSCRIBE_TIMEOUT.as_secs_f64() * WORST_CASE_RTF_BUDGET;
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

/// Fraction of `TRANSCRIBE_TIMEOUT` a full-width window may spend, at a
/// real-time factor of 1.0. Half the budget: the engine is shared with
/// dictation and the timeout also has to cover a cold Metal warm-up.
const WORST_CASE_RTF_BUDGET: f64 = 0.5;

/// How a window boundary was chosen — kept per window because "the VAD found a
/// pause" and "nothing was quiet enough, so the clock decided" are different
/// confidence levels at the seam, and the second one is what the text-anchor
/// fallback exists for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryKind {
    /// The start of the meeting, or its end. Not a cut.
    Edge,
    /// Cut in the middle of a VAD-detected silence.
    Silence,
    /// No silence in the search window cleared `min_silence_seconds`; the
    /// target clock decided.
    FixedClock,
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
    from_seconds: f64,
    cfg: &ChunkConfig,
    first_index: usize,
) -> Vec<ChunkWindow> {
    let mut boundaries = vec![from_seconds.max(0.0)];
    let mut kinds = vec![BoundaryKind::Edge];
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
    from_seconds: f64,
    cfg: &ChunkConfig,
    first_index: usize,
) -> Result<Vec<ChunkWindow>, String> {
    cfg.validate()?;
    let Some(vad) = vad else {
        return Ok(plan_windows_fixed(
            audio.total_seconds(),
            from_seconds,
            cfg,
            first_index,
        ));
    };
    let total = audio.total_seconds();
    let mut boundaries = vec![from_seconds.max(0.0)];
    let mut kinds = vec![BoundaryKind::Edge];
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
/// chunks", and it is what `tests/meeting_chunk_timeout_isolation.rs` proves.
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
    let mut merged: Vec<String> = Vec::new();
    let mut folded: Vec<String> = Vec::new();
    let mut report = MergeReport::default();
    for chunk in chunks {
        let chunk_folded: Vec<String> = chunk.iter().map(|t| fold(t)).collect();
        if merged.is_empty() {
            merged.extend(chunk.iter().cloned());
            folded.extend(chunk_folded);
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
                merged.extend(chunk[skip + anchor..].iter().cloned());
                folded.extend(chunk_folded[skip + anchor..].iter().cloned());
            }
            None => {
                report.no_anchor_seams += 1;
                merged.extend(chunk.iter().cloned());
                folded.extend(chunk_folded);
            }
        }
    }
    (merged, report)
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
    let mut out: Vec<TimedSpan> = Vec::new();
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
        // VAD-cut boundaries make this rare by putting the cut in silence, which
        // is exactly why they are the shipped arm.
        if let (Some(previous), Some(first)) = (out.last(), kept.first()) {
            let boundary_gap = (first.start_seconds - previous.start_seconds).abs();
            if boundary_gap <= SEAM_DUPLICATE_TOLERANCE
                && fold(&previous.text) == fold(&first.text)
                && !fold(&first.text).is_empty()
            {
                out.pop();
            }
        }
        out.extend(kept.into_iter().cloned());
    }
    out.sort_by(|a, b| a.start_seconds.total_cmp(&b.start_seconds));
    out
}

/// How far apart two decodes of the SAME word at a seam may be timestamped
/// before they stop being the same word. A second is generous next to a word
/// (RNNT emission times drift between runs — finding #11 — but not by a second)
/// and tight next to the 25 s minimum window, so it cannot reach a genuine
/// repetition in ordinary speech at the far end of a chunk.
const SEAM_DUPLICATE_TOLERANCE: f64 = 1.0;

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
        let from = resume.processed_through_seconds.max(0.0);
        if resumed > 0 {
            log::info!(
                "meeting {meeting_id}: resuming ASR at {from:.1}s ({resumed} chunks already done)"
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
            let outcome = loop {
                match self.asr.transcribe_timed(&samples) {
                    Ok(transcript) => break ChunkOutcome::from_transcript(window, transcript),
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
                        break ChunkOutcome::failed(window, e);
                    }
                }
            };
            self.store.record_chunk(meeting_id, &outcome)?;
            chunks.push(outcome);
            decoded += 1;
        }

        Ok(assemble(chunks, resumed, decoded, yields, interrupted))
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
    let decodable: Vec<&ChunkOutcome> = chunks
        .iter()
        .filter(|c| c.status == ChunkStatus::Done)
        .collect();
    let timed = !decodable.is_empty()
        && decodable
            .iter()
            .all(|c| c.timestamp_kind.has_times() && !c.spans.is_empty());

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
        };
    }

    // FALLBACK: no usable alignment — anchor the seams on text.
    let per_chunk: Vec<Vec<String>> = chunks
        .iter()
        .filter(|c| !c.text.trim().is_empty())
        .map(|c| {
            c.text
                .split_whitespace()
                .map(|t| t.to_string())
                .collect::<Vec<String>>()
        })
        .collect();
    let (merged, report) = merge_chunk_tokens_reporting(&per_chunk);
    let segments: Vec<TimedSpan> = chunks
        .iter()
        .filter(|c| !c.text.trim().is_empty())
        .map(|c| TimedSpan {
            start_seconds: c.content_start_seconds,
            end_seconds: c.content_end_seconds,
            text: c.text.clone(),
        })
        .collect();
    MeetingTranscript {
        text: merged.join(" ").trim().to_string(),
        segments,
        timestamps_are_real: false,
        merge: report,
        chunks_decoded: decoded,
        chunks_resumed: resumed,
        chunks_failed: failed,
        processed_through_seconds: processed_through,
        interrupted,
        preempt_yields: yields,
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
        }
    }
}

/// The English code the gate accepts, plus the regional variants a settings
/// picker can produce.
fn is_english(code: &str) -> bool {
    let c = code.trim().to_ascii_lowercase();
    c == "en" || c.starts_with("en-") || c.starts_with("en_")
}

/// Is the Notetaker available with this model + spoken-language setting?
///
/// `spoken_language` is the app's "Language I speak" setting; `None` (or an
/// empty string) means autodetect, which an English-only model resolves to
/// English anyway.
pub fn meeting_availability(
    model_id: Option<&str>,
    spoken_language: Option<&str>,
) -> Result<(), MeetingUnavailable> {
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
        let plan = plan_windows_fixed(100.0, 0.0, &cfg, 0);
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
        let one = plan_windows_fixed(12.0, 0.0, &cfg, 0);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].audio_start_seconds, 0.0);
        assert_eq!(one[0].audio_end_seconds, 12.0);
        assert!(plan_windows_fixed(0.0, 0.0, &cfg, 0).is_empty());
    }

    /// Resuming plans from the resume point — the audio before it is never read
    /// again, and the window indices carry on from the ledger.
    #[test]
    fn a_resumed_plan_starts_at_the_resume_point() {
        let cfg = ChunkConfig::default();
        let plan = plan_windows_fixed(200.0, 61.0, &cfg, 2);
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
        let plan = plan_windows(&audio, Some(&vad), 0.0, &cfg, 0).expect("plan");
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

    /// No VAD is not an error — it is the fixed clock, which is what the eval
    /// corpus was measured on.
    #[test]
    fn planning_without_a_vad_is_the_fixed_clock() {
        let cfg = ChunkConfig::default();
        let audio = MemoryWindows::at_meeting_rate(vec![0.0; MEETING_RATE as usize * 100]);
        let with_none = plan_windows(&audio, None, 0.0, &cfg, 0).expect("plan");
        assert_eq!(with_none, plan_windows_fixed(100.0, 0.0, &cfg, 0));
    }

    fn outcome(index: usize, start: f64, end: f64, spans: &[(f64, f64, &str)]) -> ChunkOutcome {
        ChunkOutcome {
            index,
            audio_start_seconds: (start - 2.0).max(0.0),
            content_start_seconds: start,
            content_end_seconds: end,
            start_boundary: BoundaryKind::Silence,
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
