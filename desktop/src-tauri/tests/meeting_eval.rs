//! YV90 — the meeting eval harness seed: a WER gate and a seam-ordering gate
//! over a sha256-verified, SYNTHETIC fixture corpus.
//!
//! Why this lands before any meeting capture or transcription code: the epic
//! plan's finding #16 is that every accuracy number in it is quoted from a
//! third party and never measured on this pipeline, and that its acceptance
//! criteria are unfalsifiable — *"no duplicated words at chunk seams" is
//! satisfied by a dedupe that deletes real words*. So the harness comes first,
//! and the two properties it gates are exactly the two 22-A needs:
//!
//! * **WER** — a number for transcript accuracy on a 15-minute single-speaker
//!   lecture, the stated 22-A target case (a student recording a lecture),
//!   scored on the WINDOWED decode 22-A will actually use (see
//!   [`lecture_decode`] for why a single pass is not an option).
//! * **Seam ordering** — on a fixture BUILT so known marker words land inside
//!   the chunker's overlap regions (derived from [`CHUNK_SECONDS`] and
//!   [`CHUNK_OVERLAP_SECONDS`], never from a hand-typed number), a chunked
//!   decode must reproduce each marker word exactly once (never twice: a missed
//!   dedupe; never zero: a dedupe that ate a real word), and segment start times
//!   must come out monotonic. The gate refuses to score a marker that only one
//!   window contains, because such a marker cannot distinguish a correct merge
//!   from any other.
//! * **Seam drift, and the merge itself** — because five marker words are five
//!   words. A merge that eats ordinary words while sparing the markers clears
//!   the counters above (measured: `duplicated=0 dropped=0` with 157 real words
//!   deleted), and a fixture with no markers in it — the lecture — is not
//!   scored by them at all. So the whole chunked transcript is also held to a
//!   [`SEAM_DRIFT_WER_GATE`] against a single continuous decode of the same
//!   audio, and the merge reports what it did at every seam
//!   ([`MergeReport`]) so an unmerged seam is a failure wherever it happens
//!   rather than only where a marker happens to sit.
//!
//! DER/JER/enrollment-EER are deliberately out of scope: they need RTTM speaker
//! ground truth that only exists once diarization lands in yap23.
//!
//! ## The corpus is synthetic, and that is permanent
//!
//! Same rule as `tests/gate_corpus.rs` and `tests/fixtures/README.md`: real
//! dictation never enters this public repo. Every fixture here is rendered by
//! the macOS speech synthesizer (`say` → `afconvert`) from invented, mundane
//! sentences with no names, no digits and no addresses — which additionally
//! makes the reference transcript EXACT by construction rather than
//! hand-transcribed, so a WER number here measures the decoder and nothing else.
//!
//! ## Where the audio lives
//!
//! Under `~/yap-eval-corpus/meetings/` — durable, outside the repo and outside
//! any scratch directory (same posture as `~/libby-scans`). What is committed is
//! the GENERATOR (below), the per-fixture `meta.json`, and
//! `tests/fixtures/meeting_eval_manifest.json` + `.sha256`, so the corpus can be
//! regrown and every byte of it checked. With the corpus absent — CI, a fresh
//! clone — every corpus-gated test prints one line and passes.
//!
//! ```sh
//! # grow the corpus (~2 minutes of `say`), then hash it
//! cargo test --test meeting_eval meeting_eval_generate_corpus -- --ignored --nocapture
//! # regrow only the seam fixture (after a change to the chunk geometry)
//! cargo test --test meeting_eval meeting_eval_generate_seam_stress -- --ignored --nocapture
//! # re-hash an existing corpus into the committed manifest
//! cargo test --test meeting_eval meeting_eval_write_manifest -- --ignored --nocapture
//! # run the gates
//! cargo test --test meeting_eval -- --nocapture
//! # check the corpus by hand, without cargo
//! (cd ~/yap-eval-corpus/meetings && shasum -a 256 -c \
//!    <repo>/desktop/src-tauri/tests/fixtures/meeting_eval_manifest.sha256)
//! ```

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// YV93: the geometry and the merge are no longer this file's own. The gates
// below score the SHIPPED chunker — same plan, same seam merge, same constants —
// so a regression in `meeting_asr` fails here instead of passing against a copy
// of itself.
use wilson_voice_lib::asr_engine::{TimedKind, TimedSpan, TimedTranscript};
use wilson_voice_lib::meeting_asr::{
    merge_chunk_tokens, merge_chunk_tokens_reporting, merge_timed, merge_timed_reporting,
    plan_windows, plan_windows_fixed, timestamps_are_usable, BoundaryKind, ChunkConfig,
    ChunkOutcome, ChunkStatus, ChunkWindow, MemoryWindows, MergeReport, ResumePoint,
    SampleWindows, SeamDecision, VoiceActivity, MAX_ANCHOR_TOKENS, MAX_HEAD_SKIP,
    MAX_TAIL_TRIM, OVERLAP_TOKEN_BUDGET, SEAM_TRUNCATION_SLACK,
};
use wilson_voice_lib::vad::WarmVad;

// YV109: fixture (d) is a two-track fixture, so the harness needs the same
// host-time vocabulary the phase-closing E2E uses — one reference, scored in
// two places, rather than two references that can disagree.
use support::two_track::{
    cross_track_residual_ms, marker_sequence, out_of_order, read_index_array_json,
    segments_from_host_spans, HostSpan, TrackTimeline, RESIDUAL_BUDGET_MS,
    RESIDUAL_HORIZON_SECONDS,
};
use wilson_voice_lib::meeting::{
    MeetingCapture, MeetingJournal, MeetingState, MIC_TRACK, SYSTEM_TRACK,
};
use wilson_voice_lib::meetings::{render_transcript, MIC_SPEAKER_LABEL, SYSTEM_SPEAKER_LABEL};
use wilson_voice_lib::rtring::CaptureAnchor;

// ---------------------------------------------------------------------------
// Where things are
// ---------------------------------------------------------------------------

/// Override for the corpus root. Unset, the corpus is the durable location.
const CORPUS_ENV: &str = "YAP_EVAL_CORPUS";
/// The durable location, relative to `$HOME`.
const CORPUS_HOME_RELATIVE: &str = "yap-eval-corpus/meetings";

/// The one line a corpus-gated test prints when there is nothing to run
/// against. Asserted verbatim by the acceptance criteria, so it is a constant
/// rather than a format string.
const CORPUS_ABSENT: &str = "meeting eval corpus not found at ~/yap-eval-corpus/meetings, skipping";

const LECTURE: &str = "lecture-15min";
const SEAM_STRESS: &str = "seam-stress";
const DEVICE_CHANGE: &str = "device-change";
/// YV109's fixture (d): two synthetic sources on two deliberately mismatched
/// clocks, with known words placed at known times on a clock BOTH of them can
/// be put back onto.
const TWO_TRACK: &str = "two-track-ordering";
/// Every fixture the manifest must name. Fixture (c) is generated here but
/// consumed by YV92 (anti-alias + input format change), not by this file's gates.
const FIXTURE_IDS: [&str; 4] = [LECTURE, SEAM_STRESS, DEVICE_CHANGE, TWO_TRACK];

/// MEASURED (YV93, Parakeet Unified EN 0.6B on Metal, no meeting tuning, clean
/// synthesized speech). The placeholder this replaces was 0.15 — a number from
/// nowhere, which YV90 shipped precisely so that it would be replaced by one
/// from somewhere. What the four arms of the lecture gate actually score:
///
/// | arm | merge | WER | insertions |
/// |---|---|---|---|
/// | fixed clock | text anchor (fallback) | 0.0048 | 0 |
/// | fixed clock | timed (primary) | 0.0090 | 13 |
/// | VAD-cut | text anchor (fallback) | 0.0042 | 0 |
/// | VAD-cut | timed (primary) | **0.0042** | **0** |
///
/// The gate is set at 0.02 — roughly twice the worst of those and four times the
/// shipped arm — so it fails on a real regression and survives the odd word
/// moving between machines.
const WER_GATE: f64 = 0.02;

/// How far the CHUNKED transcript may drift from a single continuous decode of
/// the same audio. This is the gate that makes "no duplicated words at chunk
/// seams" falsifiable in the way finding #16 demands: the marker counters only
/// see the five declared words, so a merge that eats ordinary words while
/// sparing the markers clears them — measured, by patching the merge to delete
/// every third token: markers intact, `duplicated=0 dropped=0`, and drift WER
/// 0.3326. Measured on the shipped merge under YV93's geometry: 0.0000, for both
/// the text-anchor merge and the timed one.
const SEAM_DRIFT_WER_GATE: f64 = 0.02;

/// How many words the lecture merge may INSERT, as a fraction of the reference.
/// An unmerged seam shows up here and nowhere else: a seam that finds no anchor
/// emits its overlap twice, which is an insertion against an exact reference.
/// At `MAX_TAIL_TRIM = 2` this fixture scored 32 insertions over 3117 words
/// (0.0103) from three unmerged seams and still passed a WER gate of 0.15;
/// measured now: 0 for the text-anchor merge on both arms and for the timed
/// merge on the VAD-cut arm, 13 (0.0042) for the timed merge on the fixed-clock
/// arm — which is the one case where a boundary can cut a word in half, and the
/// measurement that argues for the VAD arm being the shipped one.
const LECTURE_INSERTION_RATE_GATE: f64 = 0.005;

/// The chunk geometry, mirrored from the shipped [`ChunkConfig::default`] and
/// asserted equal to it by `the_harness_geometry_is_the_shipped_geometry`. Kept
/// as `const` here only because a `const` can be used where a call cannot; the
/// shipped values are the authority.
///
/// Under that geometry a meeting is cut at `30 s, 60 s, …` and each window
/// DECODES from two seconds before its own boundary — windows 0–30, 28–60,
/// 58–90 … — so the region two consecutive windows share is
/// `[30k - 2, 30k]`. YV93 moves the interior boundaries onto VAD silence inside
/// a [25 s, 35 s] search window; the gates below do not care which arm produced
/// the chunks, only that the merged transcript is correct at the seams.
const CHUNK_SECONDS: f64 = 30.0;
const CHUNK_OVERLAP_SECONDS: f64 = 2.0;

/// The only region of the audio that windows `k-1` and `k` BOTH contain:
/// `[k*CHUNK_SECONDS - overlap, k*CHUNK_SECONDS]`. A marker word must sit
/// entirely inside one of these or the seam gate is scoring nothing — which is
/// the defect this fixture was built to make impossible, and then reproduced
/// twice: first by deriving the region from a 28 s hop the shipped chunker does
/// not use, and before that by placing markers ON the boundary, where a word
/// straddles the cut and lands wholly inside a single window.
fn seam_region(k: usize) -> (f64, f64) {
    let boundary = k as f64 * CHUNK_SECONDS;
    (boundary - CHUNK_OVERLAP_SECONDS, boundary)
}

/// The harness's mirrored constants ARE the shipped ones.
#[test]
fn the_harness_geometry_is_the_shipped_geometry() {
    let cfg = ChunkConfig::default();
    assert_eq!(CHUNK_SECONDS, cfg.target_seconds);
    assert_eq!(CHUNK_OVERLAP_SECONDS, cfg.overlap_seconds);
    assert!(
        cfg.min_seconds <= CHUNK_SECONDS && CHUNK_SECONDS <= cfg.max_seconds,
        "the VAD arm can move a boundary outside the fixed geometry this file assumes"
    );
}

/// Everything downstream of the mic runs at 16 kHz mono.
const TARGET_RATE: u32 = 16_000;

// ---------------------------------------------------------------------------
// YV109 — fixture (d): two tracks, two clocks, one conversation
// ---------------------------------------------------------------------------
//
// Fixture (b) made the seam merge falsifiable by putting a known word inside
// every region two windows share. Fixture (d) is the same idea one axis over:
// known words on two SOURCES at known times on a clock neither source's wav
// records, with the two clocks deliberately mismatched so a merge that ignores
// the mismatch is caught by construction rather than by luck. Real desk-test
// hardware drifts by so little over a two-minute recording that a fixture built
// from it would pass under a merge that did nothing at all — which is the
// "unfalsifiable acceptance criteria" failure finding #16 named, arriving in a
// new place.
//
// Two independent errors are built in, and each has its own negative control:
//
//   1. a START OFFSET — the tap's aggregate device comes up 750 ms after the
//      mic's stream, so the two tracks' local second zero are different
//      moments. Control: `two_track_ordering_without_the_rebase_reorders_the_
//      conversation`, which asserts the un-rebased render swaps exactly the
//      pairs the layout was built to swap.
//   2. a RATE mismatch — the two devices' crystals differ by 290 ppm, which is
//      3.1 seconds by the three-hour cap. Control:
//      `two_track_nominal_rate_assumption_misses_the_budget`.

/// How late the tap's track starts, in host seconds.
const TWO_TRACK_ORIGIN_OFFSET_SECONDS: f64 = 0.750;

/// Each device's true rate, as a fraction off nominal, mic first. Opposite
/// signs because two crystals are two crystals; the SUM is what the merge has
/// to take out.
const TWO_TRACK_PPM: [f64; 2] = [-40e-6, 250e-6];

/// Each device's callback size in frames at [`TARGET_RATE`] — 10 ms for the
/// mic, 20 ms for the tap.
const TWO_TRACK_CALLBACK_FRAMES: [usize; 2] = [160, 320];

/// `(host_seconds, track, marker word)` — the conversation as it was spoken.
///
/// FOUR Me/Them pairs sit closer together than
/// [`TWO_TRACK_ORIGIN_OFFSET_SECONDS`] with **Me** first (12.0/12.4,
/// 36.0/36.5, 50.0/50.3, 68.0/68.6). That is the whole design: dropping the
/// rebase slides every "Them" 750 ms early and swaps exactly those four, so a
/// gate that would pass without the rebase cannot exist. Same-track markers are
/// never closer than five seconds, and none sits within four seconds of a chunk
/// boundary (30 s, 60 s) — the seam merge is fixture (b)'s subject, not this
/// one's, and a marker cut in half by a window boundary would confuse the two.
const TWO_TRACK_CONVERSATION: [(f64, usize, &str); 13] = [
    (3.0, MIC_TRACK, "avocado"),
    (7.0, SYSTEM_TRACK, "bramble"),
    (12.0, MIC_TRACK, "kettle"),
    (12.4, SYSTEM_TRACK, "custard"),
    (18.0, SYSTEM_TRACK, "harpoon"),
    (23.0, MIC_TRACK, "marigold"),
    (36.0, MIC_TRACK, "penguin"),
    (36.5, SYSTEM_TRACK, "meadow"),
    (44.0, SYSTEM_TRACK, "walrus"),
    (50.0, MIC_TRACK, "sandal"),
    (50.3, SYSTEM_TRACK, "turnip"),
    (68.0, MIC_TRACK, "violin"),
    (68.6, SYSTEM_TRACK, "tundra"),
];

/// The fixture's length in host seconds, with a tail past the last marker so
/// the final window is not mostly the end of the file.
const TWO_TRACK_SECONDS: f64 = 76.0;

/// Fixture (d)'s carrier, in two pieces so the marker WORD can be placed to the
/// sample — fixture (b)'s technique, with a shorter carrier because two of
/// these overlap in host time whenever a pair does.
const TWO_TRACK_PREFIX: &str = "The next word is";
const TWO_TRACK_SUFFIX: &str = "spoken once.";

/// The speaker label for a track, straight from the shipped renderer's own
/// constants so the fixture and the UI cannot disagree about who "Them" is.
fn two_track_speaker(track: usize) -> &'static str {
    if track == MIC_TRACK {
        MIC_SPEAKER_LABEL
    } else {
        SYSTEM_SPEAKER_LABEL
    }
}

/// A host second as one track's own finalized-wav second — the inverse of the
/// map the merge has to find. Used to BUILD the fixture, never to score it.
fn two_track_local_seconds(track: usize, host_seconds: f64) -> f64 {
    let origin = if track == MIC_TRACK {
        0.0
    } else {
        TWO_TRACK_ORIGIN_OFFSET_SECONDS
    };
    (host_seconds - origin) * (1.0 + TWO_TRACK_PPM[track])
}

/// The declared ground truth for one track, as a timeline a measurement can be
/// scored against.
fn two_track_truth(track: usize) -> TrackTimeline {
    let origin = if track == MIC_TRACK {
        0.0
    } else {
        TWO_TRACK_ORIGIN_OFFSET_SECONDS
    };
    TrackTimeline::exact(origin, TARGET_RATE as f64 * (1.0 + TWO_TRACK_PPM[track]))
}

/// The markers in the order they were spoken, as `Me:word` / `Them:word`.
/// Derived by sorting [`TWO_TRACK_CONVERSATION`] on the shared clock — the
/// ground truth is never typed twice.
fn two_track_expected_sequence() -> Vec<String> {
    let mut rows: Vec<(f64, usize, &str)> = TWO_TRACK_CONVERSATION.to_vec();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    rows.iter()
        .map(|(_, track, word)| format!("{}:{}", two_track_speaker(*track), word))
        .collect()
}

fn two_track_words() -> Vec<String> {
    TWO_TRACK_CONVERSATION
        .iter()
        .map(|(_, _, w)| (*w).to_string())
        .collect()
}

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meeting_eval_manifest.json")
}

fn checksum_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meeting_eval_manifest.sha256")
}

fn corpus_root() -> PathBuf {
    if let Ok(dir) = std::env::var(CORPUS_ENV) {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .expect("a home directory")
        .join(CORPUS_HOME_RELATIVE)
}

/// The corpus root, or `None` after printing [`CORPUS_ABSENT`]. Every gate that
/// needs audio starts here, so a machine without the corpus runs the whole file
/// green in milliseconds.
fn corpus() -> Option<PathBuf> {
    let root = corpus_root();
    if root.is_dir() {
        return Some(root);
    }
    eprintln!("{CORPUS_ABSENT}");
    None
}

// ---------------------------------------------------------------------------
// The committed manifest
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FileEntry {
    /// Relative to the corpus root. Never absolute, never contains `..` —
    /// `meeting_eval_manifest_is_committed_and_names_every_fixture` enforces it.
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Synthesis {
    tool: String,
    voice: String,
    words_per_minute: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    generator: String,
    corpus_root: String,
    fixtures: Vec<String>,
    synthesis: Synthesis,
    note: String,
    files: Vec<FileEntry>,
}

fn read_manifest() -> Manifest {
    let path = manifest_path();
    let body = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("the manifest is committed at {}: {e}", path.display()));
    serde_json::from_str(&body).unwrap_or_else(|e| panic!("{} is not valid: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// Per-fixture metadata (written next to the audio, hashed into the manifest)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Utterance {
    text: String,
    start_seconds: f64,
    end_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FixtureMeta {
    id: String,
    kind: String,
    sample_rate: u32,
    duration_seconds: f64,
    utterances: Vec<Utterance>,
    /// Seam fixture: the window-start times the marker words were placed to
    /// straddle — `k * (chunk_seconds - chunk_overlap_seconds)`, i.e. the START
    /// of each overlap region, derived from the two fields below rather than
    /// typed in. Checked against [`seam_region`] before the gate runs.
    #[serde(default)]
    boundary_seconds: Vec<f64>,
    /// Seam fixture: one unique word per boundary. Unique across the whole
    /// fixture BY CONSTRUCTION, which is what makes the seam gate falsifiable —
    /// the expected count in a correct transcript is exactly one, so a missed
    /// dedupe reads as two and a dedupe that deleted a real word reads as zero.
    #[serde(default)]
    seam_keywords: Vec<String>,
    /// Seam fixture: the exact span of each marker WORD (not of the sentence
    /// carrying it), in the same order as `seam_keywords`. The gate asserts each
    /// span lies inside its overlap region, so a fixture whose markers drifted
    /// out of the seams fails loudly instead of passing vacuously.
    #[serde(default)]
    marker_spans: Vec<Utterance>,
    /// Seam fixture: the chunk geometry the fixture was grown for. If either
    /// disagrees with [`CHUNK_SECONDS`] / [`CHUNK_OVERLAP_SECONDS`] the corpus
    /// predates a change to the chunker and must be regrown.
    #[serde(default)]
    chunk_seconds: Option<f64>,
    #[serde(default)]
    chunk_overlap_seconds: Option<f64>,
    /// Device-change fixture: where the input format changes (YV92).
    #[serde(default)]
    device_change_seconds: Option<f64>,
    /// Device-change fixture: the two nominal input rates, in order. 48000 is
    /// what a built-in mic reports; 24000 is what AirPods report (OS-9).
    #[serde(default)]
    source_rates_hz: Vec<u32>,
    /// YV109 fixture (d): everything the two-track ordering gate needs that a
    /// single-track fixture has no place for.
    #[serde(default)]
    two_track: Option<TwoTrackMeta>,
}

/// One marker word in fixture (d): which track said it, when on the SHARED host
/// clock, and where that lands in that track's own finalized wav.
///
/// Both numbers are committed because they are not the same number and the
/// difference IS the fixture: `local_seconds` is what a decoder reports and
/// `host_seconds` is what the transcript has to be ordered by.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TwoTrackMarker {
    word: String,
    track: i64,
    speaker: String,
    host_seconds: f64,
    local_seconds: f64,
}

/// Fixture (d)'s ground truth.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TwoTrackMeta {
    /// One finalized wav per track, in track order — produced by the SHIPPED
    /// capture path (`MeetingCapture` → `MeetingJournal` → `finalize`), not by
    /// writing a buffer straight to disk.
    wavs: Vec<String>,
    /// One index-record sidecar per track, in track order: the journal's own
    /// persisted `host_ns` / `captured_samples` / `spilled_samples` lines,
    /// copied out of the journal before finalize removed them.
    anchors: Vec<String>,
    /// Where each track's local sample 0 sat on the shared host clock. Track 1
    /// starts later because a tap has to build a process tap and an aggregate
    /// device before it can deliver anything (YV100's call sequence).
    origin_seconds: Vec<f64>,
    /// Each device's true rate as parts-per-million off the nominal 16 kHz.
    /// Deliberately non-zero and of OPPOSITE sign, so a fixture that happened
    /// to be recorded on two well-behaved crystals cannot pass this gate by
    /// being lucky — the mismatch is declared, and the gate checks it is there.
    clock_ppm: Vec<f64>,
    true_rate_hz: Vec<f64>,
    /// Each device's callback size in frames. Different per track on purpose:
    /// it is what makes the residual DIFFERENTIAL rather than common-mode.
    callback_frames: Vec<usize>,
    markers: Vec<TwoTrackMarker>,
    /// The markers as `Me:word` / `Them:word`, in the order they were spoken on
    /// the shared clock. What the rendered transcript must equal.
    expected_sequence: Vec<String>,
    residual_horizon_seconds: f64,
    residual_budget_ms: f64,
}

fn read_meta(root: &Path, id: &str) -> FixtureMeta {
    let path = root.join(id).join("meta.json");
    let body = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} is part of the fixture: {e}", path.display()));
    serde_json::from_str(&body).unwrap_or_else(|e| panic!("{} is not valid: {e}", path.display()))
}

fn read_reference(root: &Path, id: &str) -> String {
    let path = root.join(id).join("reference.txt");
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} is part of the fixture: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// Metrics — pure, and tested against their own negative controls below
// ---------------------------------------------------------------------------

/// Words, for scoring: ASCII alphanumerics and word-internal apostrophes,
/// lowercased; everything else is a separator. Deliberately plain — the fixture
/// text is written to avoid digits and abbreviations precisely so that no
/// number/date normalisation layer sits between the decoder and the score.
fn normalize(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            cur.push(ch.to_ascii_lowercase());
        } else if ch == '\'' && !cur.is_empty() {
            cur.push(ch);
        } else if !cur.is_empty() {
            push_word(&mut out, &cur);
            cur.clear();
        }
    }
    if !cur.is_empty() {
        push_word(&mut out, &cur);
    }
    out
}

fn push_word(out: &mut Vec<String>, raw: &str) {
    let w = raw.trim_matches('\'');
    if !w.is_empty() {
        out.push(w.to_string());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WerReport {
    substitutions: usize,
    deletions: usize,
    insertions: usize,
    reference_words: usize,
}

impl WerReport {
    fn errors(&self) -> usize {
        self.substitutions + self.deletions + self.insertions
    }
    fn wer(&self) -> f64 {
        if self.reference_words == 0 {
            return if self.insertions == 0 { 0.0 } else { 1.0 };
        }
        self.errors() as f64 / self.reference_words as f64
    }
}

impl std::fmt::Display for WerReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "WER {:.4} ({} errors over {} reference words: {} sub / {} del / {} ins)",
            self.wer(),
            self.errors(),
            self.reference_words,
            self.substitutions,
            self.deletions,
            self.insertions
        )
    }
}

/// Standard word error rate: Levenshtein alignment of hypothesis against
/// reference, with the edit counts split out (a bare distance cannot tell a
/// dedupe that DELETED words from one that duplicated them, which is the whole
/// point of the seam gate below).
fn wer(reference: &[String], hypothesis: &[String]) -> WerReport {
    let r = reference.len();
    let h = hypothesis.len();
    let w = h + 1;
    let mut d = vec![0u32; (r + 1) * w];
    for i in 0..=r {
        d[i * w] = i as u32;
    }
    for j in 0..=h {
        d[j] = j as u32;
    }
    for i in 1..=r {
        for j in 1..=h {
            let cost = u32::from(reference[i - 1] != hypothesis[j - 1]);
            d[i * w + j] = (d[(i - 1) * w + j] + 1)
                .min(d[i * w + j - 1] + 1)
                .min(d[(i - 1) * w + j - 1] + cost);
        }
    }

    let (mut i, mut j) = (r, h);
    let mut rep = WerReport {
        substitutions: 0,
        deletions: 0,
        insertions: 0,
        reference_words: r,
    };
    while i > 0 || j > 0 {
        if i > 0 && j > 0 {
            let cost = u32::from(reference[i - 1] != hypothesis[j - 1]);
            if d[i * w + j] == d[(i - 1) * w + j - 1] + cost {
                if cost == 1 {
                    rep.substitutions += 1;
                }
                i -= 1;
                j -= 1;
                continue;
            }
        }
        if i > 0 && d[i * w + j] == d[(i - 1) * w + j] + 1 {
            rep.deletions += 1;
            i -= 1;
            continue;
        }
        rep.insertions += 1;
        j -= 1;
    }
    rep
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SeamReport {
    /// A marker word the merged transcript emitted MORE than once — the overlap
    /// was not deduped.
    duplicated_word_count: usize,
    /// A marker word the merged transcript lost entirely, even though the
    /// single continuous decode of the same audio produced it — the dedupe ate
    /// a real word. This is the counter finding #16 says every "no duplicated
    /// words" claim is missing.
    dropped_word_count: usize,
    /// Marker words the continuous decode actually produced, i.e. the ones this
    /// report is entitled to have an opinion about.
    checked_keywords: usize,
    per_keyword: Vec<(String, usize, usize)>,
}

/// Score a merged chunked transcript at the seams, using a single continuous
/// decode of the same audio as the baseline for what is decodable at all.
///
/// A marker word the continuous decode also missed is NOT counted as dropped —
/// that is the acoustic model's business, not the chunker's.
fn seam_report(keywords: &[String], chunked: &[String], continuous: &[String]) -> SeamReport {
    let mut rep = SeamReport {
        duplicated_word_count: 0,
        dropped_word_count: 0,
        checked_keywords: 0,
        per_keyword: Vec::new(),
    };
    for keyword in keywords {
        let key = normalize(keyword)
            .into_iter()
            .next()
            .expect("a marker word is a word");
        let in_chunked = chunked.iter().filter(|w| **w == key).count();
        let in_continuous = continuous.iter().filter(|w| **w == key).count();
        rep.per_keyword
            .push((key.clone(), in_chunked, in_continuous));
        rep.duplicated_word_count += in_chunked.saturating_sub(1);
        if in_continuous >= 1 {
            rep.checked_keywords += 1;
            if in_chunked == 0 {
                rep.dropped_word_count += 1;
            }
        }
    }
    rep
}

/// The seam gate's CONTENT check: does the merged chunked transcript still say
/// what a single continuous decode of the same audio says?
///
/// A pure function, and deliberately separate from [`seam_report`], because the
/// two answer different questions and only one of them scales. `seam_report`
/// scores the five declared marker words — necessary, since only those are
/// KNOWN to sit in an overlap region, and demonstrably not sufficient, since a
/// merge that deletes ordinary words while sparing the markers passes it (see
/// `seam_drift_gate_catches_a_marker_preserving_word_eater`, which builds
/// exactly that merge). This one scores every word.
///
/// Two bounds, because they fail differently:
///
/// * a STRUCTURAL bound — a merge that only ever splices inside the overlap can
///   lose at most [`MAX_TAIL_TRIM`] + [`MAX_HEAD_SKIP`] tokens per seam, so more
///   than that means it spliced somewhere it had no business splicing;
/// * a RATE bound — [`SEAM_DRIFT_WER_GATE`] over the whole transcript, which
///   also catches insertions (an unmerged overlap) and substitutions, and which
///   does not grow with the number of seams.
fn drift_within_budget(drift: &WerReport, seams: usize) -> Result<(), String> {
    let deletion_budget = seams * (MAX_TAIL_TRIM + MAX_HEAD_SKIP);
    if drift.deletions > deletion_budget {
        return Err(format!(
            "the merge deleted {} words the continuous decode produced, over {seams} seams — \
             a merge that splices inside the overlap can lose at most {deletion_budget} \
             ({MAX_TAIL_TRIM} tail + {MAX_HEAD_SKIP} head per seam). {drift}",
            drift.deletions
        ));
    }
    if drift.wer() > SEAM_DRIFT_WER_GATE {
        return Err(format!(
            "the chunked transcript drifted {:.4} from the continuous decode of the same \
             audio, past the {SEAM_DRIFT_WER_GATE} gate. {drift}",
            drift.wer()
        ));
    }
    Ok(())
}

/// One decoded window of a meeting. `start_seconds` is the window's own start
/// today; when YV93 lands `asr_engine::transcribe_timed` it becomes the real
/// segment timestamp, and the ordering gate below stops being a check on the
/// harness's own arithmetic and starts being a check on the model's output.
#[derive(Debug, Clone, PartialEq)]
struct Segment {
    start_seconds: f64,
    text: String,
}

/// The shipped window plan, as `(audio_start, audio_end)` pairs.
///
/// This is `meeting_asr::plan_windows_fixed` — the fixed-clock arm of the
/// shipped chunker, which is what a VAD-less decode produces and what every
/// number in this file was measured on. The VAD-cut arm only moves interior
/// boundaries, and never outside [25 s, 35 s], so nothing below depends on
/// which arm ran.
fn chunk_plan(total_seconds: f64) -> Vec<(f64, f64)> {
    plan_windows_fixed(total_seconds, ResumePoint::start(), &ChunkConfig::default(), 0)
        .iter()
        .map(|w| (w.audio_start_seconds, w.audio_end_seconds))
        .collect()
}

// ---------------------------------------------------------------------------
// Decoding — the real engine, driven exactly the way the YV32 gate drives it
// ---------------------------------------------------------------------------

/// Runs the app's own headless transcriber (`--transcribe-file`) once per
/// window. One process per decode is slow and deliberate: it is the shipped
/// pipeline end to end (wav → 16 kHz mono → GGUF/Metal engine → text) with no
/// test-only path in it, which is the only kind of number worth gating on.
struct Decoder {
    bin: PathBuf,
    scratch: PathBuf,
}

/// One decode at a time, whatever `--test-threads` says. Each decode loads a
/// multi-hundred-MB GGUF onto the Metal device; letting four corpus tests do
/// that concurrently turns a measurement into a memory-pressure experiment.
static DECODE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl Decoder {
    fn new() -> Decoder {
        let scratch = std::env::temp_dir().join(format!("yap-meeting-eval-{}", std::process::id()));
        fs::create_dir_all(&scratch).expect("a scratch directory");
        Decoder {
            bin: PathBuf::from(env!("CARGO_BIN_EXE_wilson-voice")),
            scratch,
        }
    }

    fn decode(&self, samples: &[i16], label: &str) -> String {
        self.decode_timed(samples, label).text
    }

    /// The same decode, asking for the alignment (YV93's `--timed`, i.e.
    /// `asr_engine::transcribe_timed`). What comes back is what the SHIPPED
    /// model actually produces — which is the open question plan finding #11
    /// raised, answered here on real audio rather than from the GGUF metadata.
    fn decode_timed(&self, samples: &[i16], label: &str) -> TimedTranscript {
        let _one_at_a_time = DECODE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let wav = self.scratch.join(format!("{label}.wav"));
        write_wav_16k_mono(&wav, samples);
        let out = Command::new(&self.bin)
            .arg("--transcribe-file")
            .arg(&wav)
            .arg("--timed")
            .output()
            .unwrap_or_else(|e| panic!("cannot run {}: {e}", self.bin.display()));
        let _ = fs::remove_file(&wav);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success(),
            "decode of {label} failed ({}): {}",
            out.status,
            stderr.trim()
        );
        for line in stderr.lines().filter(|l| l.starts_with("using ")) {
            eprintln!("  [{label}] {line}");
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        serde_json::from_str(stdout.trim())
            .unwrap_or_else(|e| panic!("decode of {label} is not a timed transcript: {e}"))
    }

    /// Decode `samples` window by window and merge the seams.
    ///
    /// The PER-WINDOW token vectors come back too, and they are not a debugging
    /// convenience: they are what lets the seam gate prove it is scoring
    /// anything at all. A marker word that only one window contains produces the
    /// same counts under a correct merge, a duplicating merge and a
    /// word-eating one, so the gate has to be able to see, independently of the
    /// merge, which windows each marker was decodable in.
    fn decode_chunked(&self, samples: &[i16], label: &str) -> ChunkedDecode {
        let total = samples.len() as f64 / TARGET_RATE as f64;
        let plan = plan_windows_fixed(total, ResumePoint::start(), &ChunkConfig::default(), 0);
        self.decode_plan(samples, label, &plan)
    }

    /// Decode a plan somebody else made — the VAD-cut arm, in practice.
    fn decode_plan(&self, samples: &[i16], label: &str, plan: &[ChunkWindow]) -> ChunkedDecode {
        let mut per_chunk: Vec<Vec<String>> = Vec::new();
        let mut segments: Vec<Segment> = Vec::new();
        let mut outcomes: Vec<ChunkOutcome> = Vec::new();
        for window in plan {
            let from = (window.audio_start_seconds * TARGET_RATE as f64) as usize;
            let to = ((window.audio_end_seconds * TARGET_RATE as f64) as usize).min(samples.len());
            let transcript = self.decode_timed(
                &samples[from..to],
                &format!("{label}-chunk{:03}", window.index),
            );
            eprintln!(
                "  [{label}] window {:03} {:7.2}s–{:7.2}s  timestamps={} spans={}",
                window.index,
                window.audio_start_seconds,
                window.audio_end_seconds,
                transcript.kind.as_str(),
                transcript.best_spans().len()
            );
            per_chunk.push(normalize(&transcript.text));
            segments.push(Segment {
                start_seconds: window.content_start_seconds,
                text: transcript.text.clone(),
            });
            outcomes.push(ChunkOutcome::from_transcript(window, transcript));
        }
        let (merged, merge) = merge_chunk_tokens_reporting(&per_chunk);
        // The PRIMARY merge (time, not text) — scored beside the fallback so
        // both paths are measured on the corpus rather than only the one the
        // shipped model happens to take. The seam DECISIONS come back with it
        // so the tie-break can be scored directly (see
        // `meeting_eval_the_fixed_clock_tie_break_only_pops_words_the_cut_truncated`)
        // rather than only through its effect on the WER.
        let (timed_spans, seams) = merge_timed_reporting(&outcomes);
        // The SHIPPED predicate, not a copy of it: a quiet window is a
        // successful decode with no spans, and the harness must take the same
        // arm `assemble` takes or it scores a path the app never runs.
        let timestamps_are_real = timestamps_are_usable(&outcomes);
        ChunkedDecode {
            merged,
            merge,
            segments,
            per_chunk,
            timed_spans,
            seams,
            timestamps_are_real,
        }
    }
}

/// The result of a windowed decode: the merged transcript, the per-window
/// segments the ordering gate runs on, the per-window token vectors the
/// vacuity guard runs on, and the merge's own account of what it did at every
/// seam — the last of which is what lets a gate fail an unmerged seam on a
/// fixture that carries no marker words at all.
struct ChunkedDecode {
    merged: Vec<String>,
    merge: MergeReport,
    segments: Vec<Segment>,
    per_chunk: Vec<Vec<String>>,
    /// YV93: the timed merge's output — every span the model timestamped, each
    /// kept by exactly the one window whose content range holds its midpoint.
    timed_spans: Vec<wilson_voice_lib::asr_engine::TimedSpan>,
    /// YV93: what the timed merge decided at every seam, straight out of the
    /// shipped merge.
    seams: Vec<SeamDecision>,
    /// Whether the shipped model gave usable alignment on every window, i.e.
    /// whether `timed_spans` is the transcript or the fallback is.
    timestamps_are_real: bool,
}

impl ChunkedDecode {
    /// The PRIMARY merge's transcript, normalised for scoring — the words the
    /// timed merge kept, in time order. Empty when the model gave no alignment.
    fn timed_tokens(&self) -> Vec<String> {
        self.timed_spans
            .iter()
            .flat_map(|s| normalize(&s.text))
            .collect()
    }

    /// How many windows decoded `word` at least once. `>= 2` is the condition
    /// under which the seam counts mean something.
    fn windows_containing(&self, word: &str) -> usize {
        self.per_chunk
            .iter()
            .filter(|c| c.iter().any(|w| w == word))
            .count()
    }
}

// ---------------------------------------------------------------------------
// WAV I/O
// ---------------------------------------------------------------------------

fn write_wav_16k_mono(path: &Path, samples: &[i16]) {
    write_wav(path, TARGET_RATE, samples);
}

fn write_wav(path: &Path, sample_rate: u32, samples: &[i16]) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec)
        .unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
    for s in samples {
        w.write_sample(*s).expect("wav sample");
    }
    w.finalize().expect("wav finalize");
}

fn read_wav_i16(path: &Path) -> (u32, Vec<i16>) {
    let mut r = hound::WavReader::open(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let spec = r.spec();
    assert_eq!(spec.channels, 1, "{} must be mono", path.display());
    assert_eq!(
        spec.bits_per_sample,
        16,
        "{} must be 16-bit PCM",
        path.display()
    );
    let samples: Vec<i16> = r.samples::<i16>().map(|s| s.expect("wav sample")).collect();
    (spec.sample_rate, samples)
}

fn sha256_file(path: &Path) -> (u64, String) {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    (bytes.len() as u64, format!("{:x}", hasher.finalize()))
}

// ---------------------------------------------------------------------------
// Gates that need no corpus — the metrics, held to their own negative controls
// ---------------------------------------------------------------------------

#[test]
fn meeting_eval_manifest_is_committed_and_names_every_fixture() {
    let m = read_manifest();
    assert_eq!(
        m.fixtures.len(),
        FIXTURE_IDS.len(),
        "the manifest names every fixture"
    );
    for id in FIXTURE_IDS {
        assert!(
            m.fixtures.iter().any(|f| f == id),
            "fixture {id} is missing from the manifest"
        );
        assert!(
            m.files
                .iter()
                .any(|f| f.path.starts_with(&format!("{id}/"))),
            "fixture {id} contributes no files to the manifest"
        );
    }
    assert!(!m.files.is_empty(), "an empty manifest checks nothing");
    for f in &m.files {
        assert_eq!(f.sha256.len(), 64, "{}: not a sha256", f.path);
        assert!(
            f.sha256.chars().all(|c| c.is_ascii_hexdigit()),
            "{}: not hex",
            f.path
        );
        assert!(f.bytes > 0, "{}: empty file", f.path);
        assert!(
            !f.path.starts_with('/') && !f.path.contains(".."),
            "{}: manifest paths are relative to the corpus root",
            f.path
        );
    }
    // The corpus is audio and text. A stray database, plist or archive dropped
    // into the corpus root would be caught here rather than shipped in a hash.
    for f in &m.files {
        let ok = f.path.ends_with(".wav") || f.path.ends_with(".txt") || f.path.ends_with(".json");
        assert!(ok, "{}: the corpus holds wav/txt/json only", f.path);
    }

    // The committed plain-checksum file is the same list, in the format
    // `shasum -a 256 -c` reads, so the corpus can be verified without cargo.
    let committed = fs::read_to_string(checksum_path()).expect("the checksum file is committed");
    assert_eq!(
        committed,
        checksum_file_body(&m),
        "meeting_eval_manifest.sha256 disagrees with the JSON manifest — \
         regenerate with `cargo test meeting_eval_write_manifest -- --ignored`"
    );
}

#[test]
fn wer_counts_substitutions_insertions_and_deletions() {
    let r = normalize("the room was quiet and the projector hummed");
    assert_eq!(wer(&r, &r).wer(), 0.0, "an exact hypothesis scores zero");

    let sub = normalize("the room was quiet and the projector hissed");
    let rep = wer(&r, &sub);
    assert_eq!(
        (rep.substitutions, rep.deletions, rep.insertions),
        (1, 0, 0)
    );
    assert!((rep.wer() - 1.0 / 8.0).abs() < 1e-9, "{rep}");

    let del = normalize("the room was quiet and the projector");
    let rep = wer(&r, &del);
    assert_eq!(
        (rep.substitutions, rep.deletions, rep.insertions),
        (0, 1, 0)
    );

    let ins = normalize("the room was very quiet and the projector hummed");
    let rep = wer(&r, &ins);
    assert_eq!(
        (rep.substitutions, rep.deletions, rep.insertions),
        (0, 0, 1)
    );

    // Case and punctuation are not errors; the normaliser eats them.
    let noisy = normalize("The room was quiet, and the projector hummed.");
    assert_eq!(wer(&r, &noisy).errors(), 0);
}

/// **The plan's own falsifiable line, as a test name.** Finding #16: *"no
/// duplicated words at chunk seams" is satisfied by a dedupe that deletes real
/// words*. So the gate must fail such a dedupe — this test builds one and
/// proves it does.
#[test]
fn seam_dedupe_never_deletes_real_words() {
    let keywords: Vec<String> = ["pineapple", "trombone", "lantern"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let continuous = normalize(
        "before the pineapple marker and after it the trombone marker and later the lantern marker",
    );

    // A correct merge: each marker exactly once.
    let good = continuous.clone();
    let rep = seam_report(&keywords, &good, &continuous);
    assert_eq!(rep.duplicated_word_count, 0);
    assert_eq!(rep.dropped_word_count, 0);
    assert_eq!(rep.checked_keywords, 3);

    // The failure mode the plan calls unfalsifiable: a "dedupe" that satisfies
    // "no duplicated words" by deleting the overlap outright, marker and all.
    let deleting: Vec<String> = continuous
        .iter()
        .filter(|w| !keywords.iter().any(|k| k == *w))
        .cloned()
        .collect();
    let rep = seam_report(&keywords, &deleting, &continuous);
    assert_eq!(
        rep.duplicated_word_count, 0,
        "it does satisfy the naive criterion — which is the point"
    );
    assert_eq!(
        rep.dropped_word_count, 3,
        "and the gate catches it anyway: three real words went missing"
    );

    // A marker the CONTINUOUS decode also missed is the model's problem, not
    // the chunker's, and must not be blamed on the seam.
    let neither: Vec<String> = normalize("before the marker and after it the marker");
    let rep = seam_report(&keywords, &neither, &neither);
    assert_eq!(rep.dropped_word_count, 0);
    assert_eq!(rep.checked_keywords, 0);

    // …and the same line held against the merge that actually SHIPS. Everything
    // above scores the TEXT fallback ([`merge_chunk_tokens`]); the primary path
    // is [`merge_timed`], and running the falsifiable line only against the arm
    // the app does not take is how a real deletion stayed invisible: the timed
    // merge's tie-break popped the previous span on start-time proximity alone,
    // which is exactly the shape of a speaker repeating a word across a pause —
    // and the VAD-cut chunker puts the boundary IN that pause. Measured before
    // the fix: `["okay", "Right,", "so"]` out of four real words.
    let across_a_pause = merge_timed(&[
        timed_chunk(
            0,
            0.0,
            30.0,
            &[(28.6, 29.0, "okay"), (29.4, 29.9, "Right.")],
        ),
        timed_chunk(1, 30.0, 60.0, &[(30.2, 30.7, "Right,"), (31.0, 31.6, "so")]),
    ]);
    // Scored with [`wer`] rather than [`seam_report`] on purpose: the marker
    // counters assume a marker is said ONCE and read a genuine repetition as a
    // duplicate, which is the very confusion that produced the bug.
    let spoken = normalize("okay right right so");
    let merged = normalize(&spans_text(&across_a_pause));
    let drift = wer(&spoken, &merged);
    assert_eq!(
        (drift.deletions, drift.insertions, drift.substitutions),
        (0, 0, 0),
        "the timed merge must keep both halves of a genuine repetition: {merged:?}"
    );

    // The tie-break it exists for still fires, at the one kind of seam where a
    // word CAN be cut in half — a fixed-clock cut, which the chunker only makes
    // when no pause cleared the floor anywhere in the search window.
    let mut incoming = timed_chunk(
        1,
        30.0,
        60.0,
        &[(30.1, 30.6, "particular"), (30.8, 31.2, "case")],
    );
    incoming.start_boundary = BoundaryKind::FixedClock;
    let half_cut = merge_timed(&[
        timed_chunk(
            0,
            0.0,
            30.0,
            &[(28.0, 28.5, "the"), (29.8, 30.0, "particular")],
        ),
        incoming,
    ]);
    assert_eq!(
        normalize(&spans_text(&half_cut)),
        normalize("the particular case")
    );
}

/// A finished chunk with word spans, cut on a VAD silence (the shipped arm), for
/// the merge gates that need no audio.
fn timed_chunk(index: usize, start: f64, end: f64, spans: &[(f64, f64, &str)]) -> ChunkOutcome {
    ChunkOutcome {
        index,
        audio_start_seconds: (start - CHUNK_OVERLAP_SECONDS).max(0.0),
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

fn spans_text(spans: &[TimedSpan]) -> String {
    spans
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A quiet window is a SUCCESSFUL decode with nothing in it, and it must not
/// drag the whole meeting off the time-primary merge — which would throw away
/// every real word timestamp the other windows produced and hand the user the
/// fallback's chunk-granularity rows instead.
#[test]
fn one_silent_chunk_does_not_demote_the_whole_meeting_to_the_text_fallback() {
    let speech_a = timed_chunk(0, 0.0, 30.0, &[(1.0, 1.5, "alpha")]);
    let mut silent = timed_chunk(1, 30.0, 60.0, &[]);
    silent.timestamp_kind = TimedKind::None;
    let speech_b = timed_chunk(2, 60.0, 90.0, &[(61.0, 61.5, "beta")]);
    assert!(timestamps_are_usable(&[
        speech_a.clone(),
        silent.clone(),
        speech_b.clone()
    ]));

    // …but a window that produced TEXT with no times is real evidence that this
    // model does not do alignment, and the fallback is right to take over.
    let mut untimed = timed_chunk(1, 30.0, 60.0, &[]);
    untimed.text = "words with no times at all".into();
    untimed.timestamp_kind = TimedKind::None;
    assert!(!timestamps_are_usable(&[speech_a, untimed, speech_b]));
}

/// **The same falsifiable line, one level up — and the hole the previous cut of
/// this harness had.** `seam_dedupe_never_deletes_real_words` proves the marker
/// counters catch a dedupe that eats a MARKER. This one proves what happens when
/// it eats everything BUT the markers: the counters come back clean, and the
/// gate has to fail it anyway. Reproduced against the real merge and the real
/// fixture before it was written here — every third token deleted, markers
/// preserved, `duplicated=0 dropped=0`, drift WER 0.3326, test green.
#[test]
fn seam_drift_gate_catches_a_marker_preserving_word_eater() {
    let keywords: Vec<String> = ["pineapple", "trombone", "lantern"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    // Sized like the seam fixture — five seams, ~475 words — so the budgets it
    // is held to are the ones the corpus gate applies.
    let mut continuous = normalize(
        "the first thing worth noticing is that the marker word here is pineapple and the \
         argument continues for a while after it before the second marker word which is \
         trombone arrives and then the talk runs on again until the last marker word lantern \
         is spoken out loud near the end of the recording",
    );
    let filler = normalize(
        "the notation gets in the way rather more often than the idea itself does and that is \
         worth saying out loud before anyone writes any of it down",
    );
    while continuous.len() < 475 {
        continuous.extend(filler.iter().cloned());
    }
    let seams = 5;

    // A word-eater that is careful to spare every marker.
    let eaten: Vec<String> = continuous
        .iter()
        .enumerate()
        .filter(|(i, w)| i % 3 != 2 || keywords.iter().any(|k| k == *w))
        .map(|(_, w)| w.clone())
        .collect();

    let rep = seam_report(&keywords, &eaten, &continuous);
    assert_eq!(
        (rep.duplicated_word_count, rep.dropped_word_count),
        (0, 0),
        "the marker counters are clean — which is exactly the hole this gate fills"
    );
    assert_eq!(rep.checked_keywords, 3);

    let drift = wer(&continuous, &eaten);
    assert!(
        drift.deletions > seams * (MAX_TAIL_TRIM + MAX_HEAD_SKIP),
        "the control must exceed the structural budget to be a control: {drift}"
    );
    let why = drift_within_budget(&drift, seams)
        .expect_err("a merge that deleted a third of the transcript must not pass");
    assert!(why.contains("deleted"), "{why}");

    // …and the RATE bound catches the other direction, which no deletion budget
    // can see: one seam that finds no anchor and emits its overlap twice.
    let mut doubled = continuous.clone();
    let repeat: Vec<String> = continuous[20..32].to_vec();
    for (n, w) in repeat.into_iter().enumerate() {
        doubled.insert(32 + n, w);
    }
    let drift = wer(&continuous, &doubled);
    assert_eq!((drift.deletions, drift.insertions), (0, 12));
    assert!(
        drift.deletions <= seams * (MAX_TAIL_TRIM + MAX_HEAD_SKIP),
        "the deletion budget is untouched — only the rate bound can catch this"
    );
    let why = drift_within_budget(&drift, seams)
        .expect_err("an overlap emitted twice is 12 insertions, past the rate gate");
    assert!(why.contains("drifted"), "{why}");

    // And the gate is not simply "always no": a merge that reproduces the
    // continuous decode passes it.
    let clean = continuous.clone();
    drift_within_budget(&wer(&continuous, &clean), seams).expect("an exact merge drifts by zero");
}

/// The counter that makes an unmerged seam visible on a fixture with no marker
/// words in it — the lecture, where three seams went unmerged unnoticed.
#[test]
fn merge_reports_the_seams_that_found_no_anchor() {
    // Overlapping windows: one seam, anchored, nothing appended twice.
    let a = normalize("the room was quiet and the projector hummed on the desk");
    let b = normalize("hummed on the desk while the lecture carried on");
    let (_, rep) = merge_chunk_tokens_reporting(&[a, b]);
    assert_eq!((rep.seams, rep.no_anchor_seams), (1, 0));

    // Nothing in common: the merge appends whole, and SAYS SO. This is the
    // branch that emitted the lecture's duplicated runs.
    let a = normalize("one two three four five");
    let b = normalize("six seven eight nine ten");
    let (merged, rep) = merge_chunk_tokens_reporting(&[a, b]);
    assert_eq!((rep.seams, rep.no_anchor_seams), (1, 1));
    assert_eq!(merged.len(), 10, "the whole window went in");

    // The lecture's actual shape, reduced: the outgoing window ran past the
    // seam and finished the sentence its own way, so the genuine anchor sits
    // four tokens back from the end of the running transcript. At the old
    // MAX_TAIL_TRIM of 2 this found no anchor and emitted the overlap twice.
    let a = normalize("that much is true before the break i want to look at the material");
    let b = normalize("before the break i want to leave you with a question and a warning");
    let (merged, rep) = merge_chunk_tokens_reporting(&[a, b]);
    assert_eq!(
        (rep.seams, rep.no_anchor_seams),
        (1, 0),
        "the overlap is six tokens — inside OVERLAP_TOKEN_BUDGET — so it anchors"
    );
    assert_eq!(
        merged,
        normalize(
            "that much is true before the break i want to leave you with a question and a warning"
        ),
        "the incoming window's rendering of the overlap wins, and nothing is said twice"
    );
    assert!(
        rep.tail_tokens_trimmed <= MAX_TAIL_TRIM,
        "{rep:?} may only move tokens the overlap can hold"
    );
}

/// [`OVERLAP_TOKEN_BUDGET`] is a literal because a float-to-int cast is not
/// const. It is not a guess: this ties it back to the chunk geometry and the
/// corpus's own speaking rate, so changing either without changing it fails.
#[test]
fn overlap_token_budget_matches_the_chunk_geometry() {
    let derived = (CHUNK_OVERLAP_SECONDS * WORDS_PER_MINUTE as f64 / 60.0).ceil() as usize;
    assert_eq!(
        OVERLAP_TOKEN_BUDGET, derived,
        "{CHUNK_OVERLAP_SECONDS}s at {WORDS_PER_MINUTE} wpm holds {derived} words"
    );
    assert_eq!(MAX_TAIL_TRIM, OVERLAP_TOKEN_BUDGET);
    const {
        assert!(
            MAX_ANCHOR_TOKENS > OVERLAP_TOKEN_BUDGET,
            "an anchor must be able to span the whole overlap"
        )
    };
}

#[test]
fn seam_gate_catches_a_duplicated_boundary_word() {
    let keywords = vec!["pineapple".to_string()];
    let continuous = normalize("the marker word here is pineapple and the talk goes on");
    // The classic missed dedupe: the overlap region decoded into both chunks.
    let doubled = normalize(
        "the marker word here is pineapple the marker word here is pineapple and the talk goes on",
    );
    let rep = seam_report(&keywords, &doubled, &continuous);
    assert_eq!(rep.duplicated_word_count, 1);
    assert_eq!(rep.dropped_word_count, 0);
}

#[test]
fn timestamps_are_sorted_gate_catches_out_of_order_segments() {
    let ordered = vec![
        Segment {
            start_seconds: 0.0,
            text: "one".into(),
        },
        Segment {
            start_seconds: 28.0,
            text: "two".into(),
        },
        Segment {
            start_seconds: 56.0,
            text: "three".into(),
        },
    ];
    let timestamps: Vec<f64> = ordered.iter().map(|s| s.start_seconds).collect();
    assert!(timestamps.is_sorted());

    let mut jumbled = ordered.clone();
    jumbled.swap(1, 2);
    let timestamps: Vec<f64> = jumbled.iter().map(|s| s.start_seconds).collect();
    assert!(
        !timestamps.is_sorted(),
        "an ordering gate that cannot fail is not a gate"
    );
}

#[test]
fn chunk_plan_covers_the_fixture_with_a_two_second_overlap() {
    let plan = chunk_plan(170.0);
    assert_eq!(plan[0], (0.0, 30.0));
    assert_eq!(
        plan[1].0, 28.0,
        "the second window re-sees the overlap before its own boundary"
    );
    for pair in plan.windows(2) {
        let overlap = pair[0].1 - pair[1].0;
        assert!(
            (overlap - CHUNK_OVERLAP_SECONDS).abs() < 1e-9 || pair[1].1 >= 170.0,
            "windows must overlap by {CHUNK_OVERLAP_SECONDS}s: {pair:?}"
        );
    }
    assert_eq!(
        plan.last().unwrap().1,
        170.0,
        "the last window is clipped to the end of the audio"
    );
    // No second of audio falls outside every window.
    let mut reach = 0.0f64;
    for (start, end) in &plan {
        assert!(*start <= reach + 1e-9, "gap before {start}");
        reach = reach.max(*end);
    }
    assert_eq!(reach, 170.0);

    // Audio shorter than one window is one window.
    assert_eq!(chunk_plan(12.0), vec![(0.0, 12.0)]);
}

/// The arithmetic the seam fixture is grown from, checked without any audio: a
/// marker placed inside [`seam_region`] is in TWO windows, and one placed ON a
/// boundary — a multiple of [`CHUNK_SECONDS`], which is where the first cut of
/// this fixture put them — is in one.
#[test]
fn seam_regions_are_the_only_places_two_windows_overlap() {
    let plan = chunk_plan(162.0);
    // How many windows wholly contain the span [from, to].
    let windows_over = |from: f64, to: f64| {
        plan.iter()
            .filter(|(ws, we)| *ws <= from + 1e-9 && *we >= to - 1e-9)
            .count()
    };
    for k in 1..=5 {
        let (from, to) = seam_region(k);
        assert_eq!(
            windows_over(from, to),
            2,
            "seam {k} at {from}s–{to}s must be inside exactly two windows"
        );

        // The bug this fixture exists to make impossible: a marker centred on
        // k * CHUNK_SECONDS. A window ENDS at its boundary and the next one
        // starts two seconds earlier, so a word straddling 30/60/90/120/150 s is
        // whole in exactly one window and no merge can change its count.
        let naive = k as f64 * CHUNK_SECONDS;
        assert_eq!(
            windows_over(naive - 0.3, naive + 0.3),
            1,
            "a marker on {naive}s is inside one window — which is exactly why \
             boundaries are derived from the hop, not from CHUNK_SECONDS"
        );
    }
}

#[test]
fn merge_chunk_tokens_removes_the_overlap_repeat() {
    let a = normalize("the room was quiet and the projector hummed on the desk");
    let b = normalize("hummed on the desk while the lecture carried on");
    let merged = merge_chunk_tokens(&[a, b]);
    assert_eq!(
        merged,
        normalize(
            "the room was quiet and the projector hummed on the desk while the lecture carried on"
        )
    );

    // Nothing in common: the merge must not invent a splice and eat words.
    let a = normalize("one two three");
    let b = normalize("four five six");
    assert_eq!(
        merge_chunk_tokens(&[a, b]),
        normalize("one two three four five six")
    );

    // The condition that actually holds at a window boundary, and the one the
    // first implementation of this function got wrong: the window cut a word in
    // half, so the incoming chunk opens with a token that was never spoken and
    // no PREFIX of it can ever match. Measured on the seam fixture — the marker
    // word came out twice — before the merge was anchored on the longest common
    // run instead.
    let a = normalize("out loud and the marker word for this boundary is pineapple");
    let b = normalize("her word for this boundary is pineapple and it is spoken exactly once");
    assert_eq!(
        merge_chunk_tokens(&[a, b]),
        normalize(
            "out loud and the marker word for this boundary is pineapple and it is spoken exactly once"
        )
    );

    // A two-token coincidence is not an anchor: splicing on it would delete real
    // words, which is the failure `seam_dedupe_never_deletes_real_words` exists
    // to catch.
    let a = normalize("the projector hummed in the room");
    let b = normalize("in the lecture carried on for another hour");
    assert_eq!(
        merge_chunk_tokens(&[a, b]),
        normalize("the projector hummed in the room in the lecture carried on for another hour")
    );
}

// ---------------------------------------------------------------------------
// Gates that need the corpus
// ---------------------------------------------------------------------------

#[test]
fn meeting_eval_corpus_matches_committed_sha256s() {
    let Some(root) = corpus() else { return };
    let m = read_manifest();
    let mut checked = 0usize;
    for entry in &m.files {
        let path = root.join(&entry.path);
        assert!(
            path.is_file(),
            "{} is in the manifest but not on disk — regrow the corpus with \
             `cargo test meeting_eval_generate_corpus -- --ignored`",
            entry.path
        );
        let (bytes, sha) = sha256_file(&path);
        assert_eq!(bytes, entry.bytes, "{}: size changed", entry.path);
        assert_eq!(sha, entry.sha256, "{}: sha256 changed", entry.path);
        checked += 1;
    }
    eprintln!("corpus verified: {checked} files match the committed sha256s");

    for id in FIXTURE_IDS {
        assert!(
            root.join(id).is_dir(),
            "{id} is missing from {}",
            root.display()
        );
    }
}

/// The 15-minute fixture, decoded once per test binary and shared by the WER
/// gate and the ordering gate below — 34 windows is minutes of Metal work and
/// there is no reason to pay for it twice.
static LECTURE_DECODE: std::sync::OnceLock<ChunkedDecode> = std::sync::OnceLock::new();

/// **Measured here, and it decided the shape of this gate.** A single-pass
/// decode of the whole 904.7 s fixture is not viable on the shipped engine: the
/// headless run climbed to ~5.1 GB RSS, drove the machine into swap and made no
/// decoding progress (8.7 s of CPU in 59 s of wall clock) before it was killed.
/// So the lecture is scored the way 22-A will actually transcribe a meeting —
/// in 30 s windows with a 2 s overlap, merged at the seams — and the WER number
/// below therefore includes seam cost, which is the number worth gating on. It
/// is also the first *measured* support for the plan's windowed-only decision
/// (finding #11) rather than an argument from first principles.
fn lecture_decode(root: &Path) -> &'static ChunkedDecode {
    LECTURE_DECODE.get_or_init(|| {
        let (rate, samples) = read_wav_i16(&root.join(LECTURE).join("audio.wav"));
        assert_eq!(rate, TARGET_RATE);
        Decoder::new().decode_chunked(&samples, LECTURE)
    })
}

/// Acceptance gate (a): a numeric WER on the 15-minute single-speaker lecture,
/// the stated 22-A target case.
#[test]
fn meeting_eval_lecture_wer_is_under_the_gate() {
    let Some(root) = corpus() else { return };
    let meta = read_meta(&root, LECTURE);
    let reference = normalize(&read_reference(&root, LECTURE));
    eprintln!(
        "{LECTURE}: {:.1}s of audio, {} reference words, {} utterances",
        meta.duration_seconds,
        reference.len(),
        meta.utterances.len()
    );

    let decode = lecture_decode(&root);
    let report = wer(&reference, &decode.merged);
    eprintln!(
        "{LECTURE}: {} windows, {report}; merge {:?}",
        decode.segments.len(),
        decode.merge
    );
    println!("meeting_eval {LECTURE} wer={:.4}", report.wer());
    assert!(
        report.wer() <= WER_GATE,
        "{LECTURE} regressed past the {WER_GATE} gate: {report}"
    );

    // The seam gates below run on the seam fixture, which is the only one with
    // marker words in its overlap regions — so THIS fixture, 32 seams of it,
    // used to be merged with nothing checking the merge. Two things check it
    // now, and both would have failed the first cut of this harness.
    //
    // (1) Every seam found an anchor. A seam that does not emits its overlap
    // twice; three of these 32 did, and no assertion in this file noticed.
    assert_eq!(
        decode.merge.seams,
        decode.per_chunk.len() - 1,
        "one seam per window after the first"
    );
    assert_eq!(
        decode.merge.no_anchor_seams, 0,
        "{} of {} lecture seams found no anchor and appended the whole window, \
         duplicating the overlap: {report}",
        decode.merge.no_anchor_seams, decode.merge.seams
    );

    // (2) The insertion rate, which is where an unmerged overlap lands when the
    // reference is exact. A gate on total WER alone has room for both.
    let insertion_rate = report.insertions as f64 / report.reference_words as f64;
    eprintln!(
        "{LECTURE}: insertion rate {insertion_rate:.4} (gate {LECTURE_INSERTION_RATE_GATE}), \
         {} seams, {} tail tokens trimmed, {} head tokens skipped",
        decode.merge.seams, decode.merge.tail_tokens_trimmed, decode.merge.head_tokens_skipped
    );
    assert!(
        insertion_rate <= LECTURE_INSERTION_RATE_GATE,
        "the merge inserted {} words over {} — rate {insertion_rate:.4} past the \
         {LECTURE_INSERTION_RATE_GATE} gate, which is what an unmerged seam looks like",
        report.insertions,
        report.reference_words
    );

    // …and the structural bound on what a merge is allowed to move, which holds
    // whatever the anchor search does.
    assert!(
        decode.merge.tail_tokens_trimmed <= decode.merge.seams * MAX_TAIL_TRIM
            && decode.merge.head_tokens_skipped <= decode.merge.seams * MAX_HEAD_SKIP,
        "the merge moved more tokens than the overlap can hold: {:?}",
        decode.merge
    );

    // (3) YV93: the PRIMARY merge, scored on the same fixture against the same
    // gate. The text-anchor merge above is the fallback; what a user's meeting
    // actually goes through is the timed merge, and until this ran there was no
    // number for it on real audio — only the argument that the midpoint rule is
    // lossless by construction. A rule that is lossless over the TIMELINE can
    // still lose words if the model times them badly, which is precisely what
    // finding #11 warns about, so it is measured rather than assumed.
    if decode.timestamps_are_real {
        let timed = decode.timed_tokens();
        let timed_report = wer(&reference, &timed);
        println!(
            "meeting_eval {LECTURE} timed_merge_wer={:.4}",
            timed_report.wer()
        );
        eprintln!("{LECTURE}: primary (timed) merge {timed_report}");
        assert!(
            timed_report.wer() <= WER_GATE,
            "the timed merge regressed past the {WER_GATE} gate: {timed_report}"
        );
        let timed_insertions = timed_report.insertions as f64 / timed_report.reference_words as f64;
        assert!(
            timed_insertions <= LECTURE_INSERTION_RATE_GATE,
            "the timed merge duplicated words at seams: {timed_insertions:.4} insertion rate"
        );
    } else {
        eprintln!(
            "{LECTURE}: the shipped model returned no alignment — only the text-anchor \
             fallback merge was scored"
        );
    }
}

/// How many of the lecture's 29 fixed-clock seams the tie-break pops. Measured,
/// and asserted exactly: a threshold change that silently stops deduping
/// half-cut words shows up as a lower number here long before it shows up as a
/// third decimal place of WER.
const FIXED_CLOCK_TIE_BREAK_POPS: usize = 16;

/// The model's own timestamp resolution: a 10 ms mel hop with 8× encoder
/// subsampling. Every seam gap this fixture produces is a multiple of it, which
/// is the reason `SEAM_TRUNCATION_SLACK` is expressed in frames rather than in
/// round decimals.
const PARAKEET_FRAME_SECONDS: f64 = 0.08;

/// Slop for comparing a frame-quantised gap against the frame itself. The gaps
/// are differences of f64 seconds read off the model (`30.0 - 29.92` is
/// `0.08000000000000185`), so an exact `<=` against 0.08 fails on arithmetic
/// rather than on anything about the audio.
const FRAME_COMPARISON_SLOP: f64 = 1e-6;

/// The seam tie-break, scored directly on real audio rather than through its
/// effect on the WER two arms away.
///
/// This is the one place in the merge where a real word can be deleted, and the
/// evidence it runs on is a millisecond-scale time comparison
/// (`SEAM_TRUNCATION_SLACK`): a word the cut TRUNCATED must end at the
/// boundary, because that is where the outgoing window's buffer stops, while a
/// word the speaker finished — the first half of a stutter, the shape that
/// makes this a defect rather than a nicety — ends before it with a real gap.
///
/// The gate is the MARGIN, not the median: what makes 80 ms a threshold rather
/// than a fitted constant is that the popped pairs cluster hard against the
/// boundary and nothing sits in the neighbourhood of the line. Both edges are
/// printed and both are asserted, so a model whose emission times drift enough
/// to close that gap fails here instead of quietly deleting words.
#[test]
fn meeting_eval_the_fixed_clock_tie_break_only_pops_words_the_cut_truncated() {
    let Some(root) = corpus() else { return };
    let decode = lecture_decode(&root);
    if !decode.timestamps_are_real {
        eprintln!("{LECTURE}: no alignment — the timed tie-break did not run");
        return;
    }
    let clock_seams: Vec<&SeamDecision> = decode
        .seams
        .iter()
        .filter(|s| s.kind == BoundaryKind::FixedClock)
        .collect();
    assert!(
        !clock_seams.is_empty(),
        "the fixed-clock arm produced no fixed-clock seams"
    );

    let popped: Vec<&&SeamDecision> = clock_seams.iter().filter(|s| s.popped).collect();
    // Candidates the tie-break DECLINED although the words matched: exactly the
    // set a repetition at a clock cut would land in.
    let spared: Vec<&&SeamDecision> = clock_seams
        .iter()
        .filter(|s| !s.popped && s.text_matches)
        .collect();
    for s in &clock_seams {
        eprintln!(
            "  [{LECTURE}] seam {:03} at {:8.3}s  {:>14} | {:<14}  gap={:+.3}s  match={} popped={}",
            s.chunk_index,
            s.boundary_seconds,
            s.previous_text,
            s.first_text,
            s.truncation_gap_seconds,
            s.text_matches,
            s.popped
        );
    }
    let widest_pop = popped
        .iter()
        .map(|s| s.truncation_gap_seconds)
        .fold(f64::NEG_INFINITY, f64::max);
    let closest_spare = spared
        .iter()
        .map(|s| s.truncation_gap_seconds)
        .fold(f64::INFINITY, f64::min);
    println!(
        "meeting_eval {LECTURE} fixed_clock_seams={} pops={} widest_pop_gap={:.3}s \
         spared_text_matches={} closest_spared_gap={:.3}s slack={SEAM_TRUNCATION_SLACK}s",
        clock_seams.len(),
        popped.len(),
        widest_pop,
        spared.len(),
        closest_spare
    );

    assert_eq!(
        popped.len(),
        FIXED_CLOCK_TIE_BREAK_POPS,
        "the tie-break stopped deduping half-cut words at fixed-clock seams"
    );
    // A word the cut truncated ends where the audio stopped, to within the
    // model's own resolution — one frame, not two.
    assert!(
        widest_pop <= PARAKEET_FRAME_SECONDS + FRAME_COMPARISON_SLOP,
        "a pop fired {widest_pop:.3}s short of the cut — more than the one frame \
         ({PARAKEET_FRAME_SECONDS}s) a truncated word can be off by, so it is not \
         a truncated word"
    );
    // …and the threshold sits BETWEEN the two populations rather than on the
    // edge of either: this is the margin that makes 0.12 s a decision and not a
    // fitted constant, and it is what fails first if the model's emission times
    // start drifting.
    assert!(
        widest_pop < SEAM_TRUNCATION_SLACK && closest_spare > SEAM_TRUNCATION_SLACK,
        "the {SEAM_TRUNCATION_SLACK}s threshold no longer separates the truncated \
         words (widest {widest_pop:.3}s) from the words the speaker finished \
         (closest {closest_spare:.3}s)"
    );
    assert!(
        closest_spare - widest_pop >= PARAKEET_FRAME_SECONDS - FRAME_COMPARISON_SLOP,
        "the truncated words and the finished words are now within one frame of \
         each other ({widest_pop:.3}s vs {closest_spare:.3}s) — time alone can no \
         longer tell a half-cut word from a repetition"
    );
}

/// Acceptance gate: start times over the FULL 15-minute fixture come out
/// monotonic.
///
/// Since YV93 this runs on the MODEL's own timestamps whenever the shipped
/// model produces them (`--timed` → `asr_engine::transcribe_timed`), shifted
/// onto the meeting timeline and merged by the shipped primary merge; it is no
/// longer a check on the harness's arithmetic over window starts. When the
/// model returns no alignment the window starts are all there is, and the test
/// says so out loud rather than quietly asserting something weaker. The negative
/// control that keeps it honest —
/// `timestamps_are_sorted_gate_catches_out_of_order_segments` — needs no corpus
/// and always runs.
#[test]
fn meeting_eval_lecture_segment_timestamps_are_sorted() {
    let Some(root) = corpus() else { return };
    let decode = lecture_decode(&root);
    let segments = &decode.segments;

    let timestamps: Vec<f64> = segments.iter().map(|s| s.start_seconds).collect();
    assert!(timestamps.is_sorted());
    assert!(
        segments.len() > 25,
        "a 15-minute fixture is more than {} windows",
        segments.len()
    );
    assert!(
        segments.iter().all(|s| !s.text.trim().is_empty()),
        "a window decoded to nothing"
    );

    if decode.timestamps_are_real {
        let starts: Vec<f64> = decode.timed_spans.iter().map(|s| s.start_seconds).collect();
        assert!(
            starts.is_sorted(),
            "the model's own timestamps came out unsorted after the merge"
        );
        assert!(
            decode.timed_spans.iter().all(|s| s.end_seconds >= s.start_seconds),
            "a span ends before it starts"
        );
        // Every span belongs to the window that owns its midpoint, so the whole
        // transcript has to fit inside the fixture.
        let meta = read_meta(&root, LECTURE);
        assert!(
            starts.last().copied().unwrap_or(0.0) <= meta.duration_seconds + 1e-6,
            "a span landed past the end of the audio"
        );
        println!(
            "meeting_eval {LECTURE} timed_spans={} kind=real first={:.2}s last={:.2}s",
            decode.timed_spans.len(),
            starts.first().copied().unwrap_or(0.0),
            starts.last().copied().unwrap_or(0.0)
        );
    } else {
        println!(
            "meeting_eval {LECTURE} timed_spans=0 kind=none — the shipped model returned no \
             alignment, so the ordering gate ran on window starts and the seam merge ran on text"
        );
    }

    eprintln!(
        "{LECTURE}: {} window start times, monotonic, {:.2}s to {:.2}s",
        segments.len(),
        timestamps.first().copied().unwrap_or(0.0),
        timestamps.last().copied().unwrap_or(0.0)
    );
}

/// Acceptance gates (b) and (c): the seam counts on the boundary-stress fixture,
/// and monotonic segment start times over the chunked decode.
///
/// Two guards run BEFORE the counts, and they are the substance of this test.
/// A seam gate is only worth its assertions if each marker word genuinely sits
/// where two windows overlap; a marker in the interior of one window yields
/// `duplicated == 0, dropped == 0` under every possible merge, correct or not.
/// So: the fixture's declared boundaries must be the chunker's own seams and
/// each marker's span must lie inside its overlap region (static, from
/// `meta.json`), and every scored marker must have been decoded in at least two
/// windows (dynamic, from the decode that just ran).
#[test]
fn meeting_eval_seam_dedupe_and_ordering_hold() {
    let Some(root) = corpus() else { return };
    let meta = read_meta(&root, SEAM_STRESS);
    assert!(
        !meta.seam_keywords.is_empty() && !meta.boundary_seconds.is_empty(),
        "the seam fixture must declare its boundaries and marker words"
    );
    assert_eq!(
        (meta.chunk_seconds, meta.chunk_overlap_seconds),
        (Some(CHUNK_SECONDS), Some(CHUNK_OVERLAP_SECONDS)),
        "the fixture was grown for a different chunk geometry — regrow it with \
         `cargo test meeting_eval_generate_seam_stress -- --ignored`"
    );
    assert_eq!(
        meta.marker_spans.len(),
        meta.seam_keywords.len(),
        "every marker word declares the span it occupies"
    );
    assert_eq!(meta.boundary_seconds.len(), meta.seam_keywords.len());

    // Static guard: the boundaries are the chunker's seams, and each marker word
    // is inside one. This is what the first cut of this fixture got wrong — it
    // placed markers on multiples of CHUNK_SECONDS (30 s), while the windows hop
    // by CHUNK_SECONDS - CHUNK_OVERLAP_SECONDS (28 s), so four of five markers
    // sat in the interior of a single window and the counts below were true
    // whatever the merge did.
    for (k, ((boundary, keyword), span)) in meta
        .boundary_seconds
        .iter()
        .zip(&meta.seam_keywords)
        .zip(&meta.marker_spans)
        .enumerate()
    {
        let (from, to) = seam_region(k + 1);
        assert!(
            (boundary - from).abs() < 1e-6,
            "declared boundary {boundary}s is not a chunk seam (expected {from}s \
             = {} * (CHUNK_SECONDS - CHUNK_OVERLAP_SECONDS)) — regrow the fixture",
            k + 1
        );
        assert_eq!(&span.text, keyword, "marker spans follow seam_keywords");
        assert!(
            span.start_seconds >= from - 1e-6 && span.end_seconds <= to + 1e-6,
            "marker {keyword} occupies {:.3}s–{:.3}s, outside the overlap region \
             {from:.3}s–{to:.3}s — it would sit inside a single window and the \
             seam gate would be vacuous",
            span.start_seconds,
            span.end_seconds
        );
    }

    let (rate, samples) = read_wav_i16(&root.join(SEAM_STRESS).join("audio.wav"));
    assert_eq!(rate, TARGET_RATE);

    let decoder = Decoder::new();
    let continuous = normalize(&decoder.decode(&samples, "seam-continuous"));
    let decode = decoder.decode_chunked(&samples, "seam");

    let seam = seam_report(&meta.seam_keywords, &decode.merged, &continuous);
    for (word, in_chunked, in_continuous) in &seam.per_keyword {
        eprintln!(
            "  marker {word:>10}: {} of {} windows, merged x{in_chunked}, continuous x{in_continuous}",
            decode.windows_containing(word),
            decode.per_chunk.len()
        );
    }
    let drift = wer(&continuous, &decode.merged);
    eprintln!(
        "{SEAM_STRESS}: {} seams at {:?}, {} markers scored; chunked-vs-continuous {drift}",
        meta.boundary_seconds.len(),
        meta.boundary_seconds,
        seam.checked_keywords
    );
    println!(
        "meeting_eval {SEAM_STRESS} duplicated={} dropped={}",
        seam.duplicated_word_count, seam.dropped_word_count
    );

    // Dynamic guard: a marker this report has an opinion about must have been
    // decodable in two windows, or the opinion is worthless.
    for (word, _, in_continuous) in &seam.per_keyword {
        if *in_continuous == 0 {
            continue;
        }
        let windows = decode.windows_containing(word);
        assert!(
            windows >= 2,
            "marker {word} is inside a single window — the seam gate would be \
             vacuous (decoded in {windows} of {} windows)",
            decode.per_chunk.len()
        );
    }

    let duplicated_word_count = seam.duplicated_word_count;
    let dropped_word_count = seam.dropped_word_count;
    assert!(
        seam.checked_keywords >= 3,
        "only {} markers survived the continuous decode — the gate would be \
         vacuous; regrow the fixture",
        seam.checked_keywords
    );
    assert_eq!(duplicated_word_count, 0);
    assert!(dropped_word_count <= 1);

    // The marker counters are the BOUNDARY-specific check and they are not the
    // whole gate: they see five words. The drift report — computed above, and
    // for one revision of this file printed and never asserted — sees all 475.
    // A merge that deletes every third token while sparing the five markers
    // clears the two counters above and fails here.
    if let Err(why) = drift_within_budget(&drift, decode.merge.seams) {
        panic!("{SEAM_STRESS}: {why}");
    }
    assert_eq!(
        decode.merge.no_anchor_seams, 0,
        "{} of {} seams found no anchor and duplicated their overlap: {drift}",
        decode.merge.no_anchor_seams, decode.merge.seams
    );

    let timestamps: Vec<f64> = decode.segments.iter().map(|s| s.start_seconds).collect();
    assert!(timestamps.is_sorted());
    assert!(
        decode.segments.iter().all(|s| !s.text.trim().is_empty()),
        "a window decoded to nothing — the seam numbers above are not trustworthy"
    );

    // YV93: the same two counters over the PRIMARY (timed) merge. This is the
    // acceptance criterion in its literal form — "no duplicated words at chunk
    // seams" — asked of the merge that actually ships, on a fixture built so
    // that every marker word sits in an overlap region where a merge bug has
    // somewhere to show itself.
    if decode.timestamps_are_real {
        let timed = decode.timed_tokens();
        let timed_seam = seam_report(&meta.seam_keywords, &timed, &continuous);
        let timed_drift = wer(&continuous, &timed);
        println!(
            "meeting_eval {SEAM_STRESS} timed_merge duplicated={} dropped={} drift={:.4}",
            timed_seam.duplicated_word_count,
            timed_seam.dropped_word_count,
            timed_drift.wer()
        );
        assert_eq!(
            timed_seam.duplicated_word_count, 0,
            "the timed merge emitted a marker word twice"
        );
        assert!(
            timed_seam.dropped_word_count <= 1,
            "the timed merge ate {} marker words",
            timed_seam.dropped_word_count
        );
        if let Err(why) = drift_within_budget(&timed_drift, decode.merge.seams) {
            panic!("{SEAM_STRESS} (timed merge): {why}");
        }
        let timed_starts: Vec<f64> = decode.timed_spans.iter().map(|s| s.start_seconds).collect();
        assert!(timed_starts.is_sorted(), "the timed merge came out unsorted");
    }
}

/// What the FIXED-CLOCK arm of the same fixture costs at the seams, measured by
/// `meeting_eval_lecture_wer_is_under_the_gate`: 13 duplicated words over 29
/// seams, after the primary merge's boundary tie-break has already removed 16 of
/// the 29 it started with (the 17th is the one `SEAM_TRUNCATION_SLACK` now
/// declines to pop, because the word ended two frames before the cut rather
/// than at it). The VAD arm has to beat it.
const FIXED_CLOCK_TIMED_INSERTIONS: usize = 13;

/// Where the app keeps the warm Silero model. The VAD arm below needs the real
/// one — a scripted VAD would be measuring this file's own assumptions.
fn silero_model() -> Option<PathBuf> {
    let path = dirs::data_dir()?
        .join("WilsonVoice")
        .join("models")
        .join("silero_vad_v4.onnx");
    path.is_file().then_some(path)
}

/// YV93's headline claim, measured: **cutting chunk boundaries on VAD silence
/// puts near-zero speech in the overlap**, and the seam merge is better for it.
///
/// The fixed-clock arm scored above cuts at 30 s whatever is happening — in a
/// continuous lecture that is mid-word every single time, which is why the
/// primary (timed) merge has to break a tie at every one of its 29 seams. The
/// VAD arm exists to make the tie not happen. Three things are checked, in
/// increasing order of how much they cost:
///
/// 1. every interior boundary landed in a pause (`BoundaryKind::Silence`) and
///    inside the [25 s, 35 s] search window;
/// 2. no boundary landed inside a word, with room to spare — measured with the
///    same VAD, over the same audio, not asserted from the design;
/// 3. decoded end to end, the primary merge over VAD-cut windows clears the
///    same WER and insertion gates as the fixed-clock arm.
#[test]
fn meeting_eval_vad_cut_boundaries_put_no_speech_in_the_overlap() {
    let Some(root) = corpus() else { return };
    let Some(model) = silero_model() else {
        eprintln!("silero VAD model not downloaded, skipping the VAD-cut arm");
        return;
    };
    let vad = match WarmVad::load(&model) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("silero VAD failed to load ({e}), skipping the VAD-cut arm");
            return;
        }
    };

    let (rate, samples) = read_wav_i16(&root.join(LECTURE).join("audio.wav"));
    assert_eq!(rate, TARGET_RATE);
    let floats: Vec<f32> = samples.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
    let audio = MemoryWindows::new(floats, TARGET_RATE);
    let cfg = ChunkConfig::default();
    let plan = plan_windows(&audio, Some(&vad as &dyn VoiceActivity), ResumePoint::start(), &cfg, 0).expect("plan");

    // (1) Every interior boundary is a pause, inside the search window.
    let clock_cuts = plan
        .iter()
        .skip(1)
        .filter(|w| w.start_boundary == BoundaryKind::FixedClock)
        .count();
    eprintln!(
        "{LECTURE}: VAD plan has {} windows, {clock_cuts} of {} interior boundaries fell back to \
         the clock",
        plan.len(),
        plan.len().saturating_sub(1)
    );
    for w in plan.iter().skip(1) {
        assert!(
            w.content_seconds() >= cfg.min_seconds - 1e-6 || w.index + 1 == plan.len(),
            "window {} is {:.2}s of content, below the {}s floor",
            w.index,
            w.content_seconds(),
            cfg.min_seconds
        );
        assert!(
            w.content_seconds() <= cfg.max_seconds + 1e-6,
            "window {} is {:.2}s of content, past the {}s ceiling",
            w.index,
            w.content_seconds(),
            cfg.max_seconds
        );
    }
    assert_eq!(
        clock_cuts, 0,
        "the lecture speaks in sentences with pauses between them; a clock fallback here means \
         the boundary search is not finding them"
    );

    // (2) The BOUNDARY sits in silence — measured with the same VAD, over the
    // same audio. This is the property that matters and it is narrower than the
    // one this test first asserted: "the overlap contains near-zero speech" is
    // only reachable when pauses are as long as the overlap, and this lecture is
    // spoken briskly (0.35 s between sentences), so 2 s of overlap necessarily
    // reaches back into the previous sentence — measured, 1.6 s of it. What
    // stops a word being duplicated at a seam is not an empty overlap, it is a
    // cut that falls BETWEEN two words, and that is what is asserted here.
    let mut speech_in_overlap = 0.0f64;
    let mut cuts_in_speech = 0usize;
    let mut margins: Vec<f64> = Vec::new();
    for w in plan.iter().skip(1) {
        let overlap = audio
            .window(w.audio_start_seconds, w.content_start_seconds)
            .expect("overlap region");
        speech_in_overlap += vad
            .voiced_spans(&overlap)
            .expect("vad")
            .iter()
            .map(|s| s.end_seconds - s.start_seconds)
            .sum::<f64>();

        // A second either side of the cut, and where the speech in it is.
        let from = w.content_start_seconds - 1.0;
        let region = audio
            .window(from, w.content_start_seconds + 1.0)
            .expect("boundary region");
        let voiced = vad.voiced_spans(&region).expect("vad");
        let cut = w.content_start_seconds - from;
        if voiced
            .iter()
            .any(|s| s.start_seconds < cut && s.end_seconds > cut)
        {
            cuts_in_speech += 1;
        }
        let margin = voiced
            .iter()
            .map(|s| (cut - s.end_seconds).abs().min((s.start_seconds - cut).abs()))
            .fold(f64::INFINITY, f64::min);
        if margin.is_finite() {
            margins.push(margin);
        }
    }
    let overlaps = plan.len().saturating_sub(1);
    let mean_overlap_speech = speech_in_overlap / overlaps.max(1) as f64;
    let mean_margin = margins.iter().sum::<f64>() / margins.len().max(1) as f64;
    println!(
        "meeting_eval {LECTURE} vad_cut boundaries={overlaps} cuts_in_speech={cuts_in_speech} \
         mean_margin_to_speech={mean_margin:.3}s mean_speech_in_overlap={mean_overlap_speech:.3}s \
         of {:.1}s",
        cfg.overlap_seconds
    );
    assert_eq!(
        cuts_in_speech, 0,
        "{cuts_in_speech} of {overlaps} VAD-cut boundaries landed inside a word"
    );
    assert!(
        mean_margin >= 0.04,
        "the cuts are only {mean_margin:.3}s from the nearest speech — a frame of VAD error \
         would put them inside a word"
    );

    // (3) Decoded: the primary merge over VAD-cut windows clears the gates.
    let decode = Decoder::new().decode_plan(&samples, "lecture-vad", &plan);
    let reference = normalize(&read_reference(&root, LECTURE));
    let fallback = wer(&reference, &decode.merged);
    eprintln!("{LECTURE} (VAD-cut): text-anchor merge {fallback}; {:?}", decode.merge);
    assert_eq!(decode.merge.no_anchor_seams, 0);
    assert!(fallback.wer() <= WER_GATE, "VAD-cut fallback merge: {fallback}");

    if decode.timestamps_are_real {
        let timed = wer(&reference, &decode.timed_tokens());
        let insertion_rate = timed.insertions as f64 / timed.reference_words as f64;
        // The fixed-clock arm of the same fixture, measured in
        // `meeting_eval_lecture_wer_is_under_the_gate`, needs a seam tie-break at
        // every window and still ends up with insertions. Cutting in the pauses
        // is supposed to be strictly better than that, so it is asserted to be.
        assert!(
            timed.insertions <= FIXED_CLOCK_TIMED_INSERTIONS,
            "VAD-cut seams inserted {} words — no better than the fixed clock's \
             {FIXED_CLOCK_TIMED_INSERTIONS}",
            timed.insertions
        );
        println!(
            "meeting_eval {LECTURE} vad_cut timed_merge_wer={:.4} insertions={}",
            timed.wer(),
            timed.insertions
        );
        eprintln!("{LECTURE} (VAD-cut): primary (timed) merge {timed}");
        assert!(timed.wer() <= WER_GATE, "VAD-cut timed merge: {timed}");
        assert!(
            insertion_rate <= LECTURE_INSERTION_RATE_GATE,
            "VAD-cut timed merge duplicated words at seams: rate {insertion_rate:.4}"
        );
        let starts: Vec<f64> = decode.timed_spans.iter().map(|s| s.start_seconds).collect();
        assert!(starts.is_sorted());
    }
}

/// Fixture (c) exists and carries what YV92 needs: a mid-recording input format
/// change, with both native-rate halves kept so the resampler can be fed at the
/// rate the device actually reported.
#[test]
fn meeting_eval_device_change_fixture_is_ready_for_yv92() {
    let Some(root) = corpus() else { return };
    let meta = read_meta(&root, DEVICE_CHANGE);
    let at = meta
        .device_change_seconds
        .expect("the device-change fixture declares where the format changes");
    assert!(at > 0.0 && at < meta.duration_seconds, "change at {at}s");
    assert_eq!(
        meta.source_rates_hz,
        vec![48_000, 24_000],
        "the fixture reproduces the AirPods case: 48 kHz then 24 kHz (OS-9)"
    );
    for (n, rate) in meta.source_rates_hz.iter().enumerate() {
        let path = root.join(DEVICE_CHANGE).join(format!(
            "segment-{}-{}hz.wav",
            (b'a' + n as u8) as char,
            rate
        ));
        let (found, samples) = read_wav_i16(&path);
        assert_eq!(
            found,
            *rate,
            "{} is not at its nominal rate",
            path.display()
        );
        assert!(!samples.is_empty());
    }
    eprintln!("{DEVICE_CHANGE}: format change at {at:.2}s, native halves present (YV92)");
}

/// YV92's aliasing arm (plan finding OS-8), run through the shipped pipeline.
///
/// The two decimators are compared on the ONE signal that can tell them apart:
/// native-rate speech with broadband energy above the 8 kHz Nyquist, i.e. the
/// far-field room noise a three-hour lecture recording is full of and a
// ---------------------------------------------------------------------------
// YV109 — fixture (d): the two-track ordering gate, and its negative controls
// ---------------------------------------------------------------------------

/// **The fixture is hard on purpose, and this is the proof — no corpus, no
/// model, no audio.**
///
/// Two independent errors are built into fixture (d), and a gate that would
/// pass without correcting either of them is not a gate. Both controls run here
/// so that a machine with no corpus still fails the day somebody flattens the
/// clock constants into "both tracks are 16 kHz starting at zero" — which would
/// leave the corpus gate below green and meaningless.
#[test]
fn two_track_ordering_fixture_is_hard_by_construction() {
    // (1) THE START OFFSET. Count the Me/Them pairs that a 750 ms slide would
    // swap: consecutive in spoken order, mic first, closer together than the
    // offset. Four of them, by construction.
    let mut rows: Vec<(f64, usize, &str)> = TWO_TRACK_CONVERSATION.to_vec();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    let swappable = rows
        .windows(2)
        .filter(|w| {
            w[0].1 == MIC_TRACK
                && w[1].1 == SYSTEM_TRACK
                && w[1].0 - w[0].0 < TWO_TRACK_ORIGIN_OFFSET_SECONDS
        })
        .count();
    assert_eq!(
        swappable, 4,
        "the fixture must contain pairs the un-rebased render gets WRONG; it \
         contains {swappable}"
    );

    // …and that is what the render actually does with them. Local seconds taken
    // at face value — every "Them" 750 ms early — against the spoken order.
    let naive = TrackTimeline::exact(0.0, TARGET_RATE as f64);
    let spans: Vec<HostSpan> = TWO_TRACK_CONVERSATION
        .iter()
        .map(|(host, track, word)| {
            let local = two_track_local_seconds(*track, *host);
            HostSpan {
                track: *track as i64,
                host_start_seconds: naive.host_seconds(local),
                host_end_seconds: naive.host_seconds(local + 0.4),
                text: (*word).to_string(),
            }
        })
        .collect();
    let lines = render_transcript(&segments_from_host_spans("no-rebase", &spans));
    let got = marker_sequence(&lines, &two_track_words());
    let want = two_track_expected_sequence();
    let displaced = got.iter().zip(want.iter()).filter(|(a, b)| a != b).count();
    assert_eq!(
        displaced,
        2 * swappable,
        "four swapped pairs is eight displaced rows\n  un-rebased: {got:?}\n  \
         spoken:     {want:?}"
    );

    // (2) THE RATE MISMATCH, which no amount of origin correction touches. The
    // two crystals differ by 290 ppm, which is seconds by the three-hour cap.
    let relative_ppm = (TWO_TRACK_PPM[SYSTEM_TRACK] - TWO_TRACK_PPM[MIC_TRACK]).abs();
    let drift_ms = relative_ppm * RESIDUAL_HORIZON_SECONDS * 1000.0;
    assert!(
        drift_ms > 20.0 * RESIDUAL_BUDGET_MS,
        "the declared clock mismatch drifts only {drift_ms:.1} ms by three \
         hours — too little to make a {RESIDUAL_BUDGET_MS:.0} ms gate mean \
         anything"
    );
    eprintln!(
        "two-track fixture: {swappable} swappable pairs, {:.0} ppm relative \
         clock error = {drift_ms:.0} ms at the 3 h cap",
        relative_ppm * 1e6
    );
}

/// **The phase-closing gate, on the corpus: one conversation, two clocks, real
/// speech through the real decoder.**
///
/// The chain scored here is the one the harness owns end to end — decode each
/// track with the SHIPPED windowed chunker and the SHIPPED timed seam merge,
/// put both tracks on the shared host clock using each track's OWN persisted
/// index records, and render with the SHIPPED renderer. What comes out must be
/// the conversation that was spoken, word for word in order, with the right
/// speaker on every line.
///
/// It is deliberately not the same test as the unit-level merge gates: those
/// score a hand-built span sequence, and a merge can be correct on hand-built
/// input while being wired to the wrong number in the real pipeline. Here the
/// input is a wav the shipped capture path produced and the times come from
/// records the shipped journal wrote.
#[test]
fn meeting_eval_two_track_ordering_survives_the_clock_mismatch() {
    let Some(root) = corpus() else { return };
    let dir = root.join(TWO_TRACK);
    let meta = read_meta(&root, TWO_TRACK);
    let tt = meta
        .two_track
        .as_ref()
        .expect("fixture (d) carries its two-track ground truth");

    // A corpus grown before a change to the clock constants would score the
    // wrong thing silently, so it is refused loudly instead.
    assert_eq!(
        tt.origin_seconds,
        vec![0.0, TWO_TRACK_ORIGIN_OFFSET_SECONDS],
        "the committed fixture's start offset is not this harness's — regrow it \
         with `cargo test meeting_eval_generate_two_track_ordering -- --ignored`"
    );
    assert_eq!(
        tt.clock_ppm,
        TWO_TRACK_PPM.iter().map(|p| p * 1e6).collect::<Vec<_>>(),
        "the committed fixture's clock mismatch is not this harness's"
    );
    assert_eq!(tt.callback_frames, TWO_TRACK_CALLBACK_FRAMES.to_vec());
    assert_eq!(tt.expected_sequence, two_track_expected_sequence());
    assert_eq!(tt.residual_budget_ms, RESIDUAL_BUDGET_MS);

    // ── the merge's own input: the journal's persisted records ───────────────
    let timelines: Vec<TrackTimeline> = tt
        .anchors
        .iter()
        .map(|name| TrackTimeline::from_records(&read_index_array_json(&dir.join(name))))
        .collect();
    for (track, timeline) in timelines.iter().enumerate() {
        let measured_ppm = (timeline.measured_rate() / TARGET_RATE as f64 - 1.0) * 1e6;
        eprintln!(
            "{TWO_TRACK} track {track}: measured rate {:.4} Hz ({measured_ppm:+.1} \
             ppm, declared {:+.1}), origin {:+.4} s (declared {:+.3})",
            timeline.measured_rate(),
            tt.clock_ppm[track],
            timeline.origin_seconds(),
            tt.origin_seconds[track],
        );
        // The measurement has to find the mismatch that is really there. A
        // rate estimate that came back "16 000.0000 Hz" would be the nominal
        // assumption wearing a measurement's clothes.
        assert!(
            (measured_ppm - tt.clock_ppm[track]).abs() < 1.0,
            "track {track}'s measured rate misses the declared one by \
             {:.2} ppm",
            measured_ppm - tt.clock_ppm[track]
        );
    }

    let residual_cross = cross_track_residual_ms(
        (&timelines[MIC_TRACK], &two_track_truth(MIC_TRACK)),
        (&timelines[SYSTEM_TRACK], &two_track_truth(SYSTEM_TRACK)),
        RESIDUAL_HORIZON_SECONDS,
    );
    let residual_mic =
        timelines[MIC_TRACK].residual_ms_at(&two_track_truth(MIC_TRACK), RESIDUAL_HORIZON_SECONDS);
    let residual_sys = timelines[SYSTEM_TRACK]
        .residual_ms_at(&two_track_truth(SYSTEM_TRACK), RESIDUAL_HORIZON_SECONDS);
    println!(
        "meeting_eval {TWO_TRACK} residual_ms_at_3h mic={residual_mic:.1} \
         system={residual_sys:.1} cross_track={residual_cross:.1}"
    );
    eprintln!(
        "{TWO_TRACK}: residual at the simulated {:.0} h mark — mic \
         {residual_mic:.1} ms, system {residual_sys:.1} ms, CROSS-TRACK \
         {residual_cross:.1} ms (budget {RESIDUAL_BUDGET_MS:.0} ms)",
        RESIDUAL_HORIZON_SECONDS / 3600.0
    );
    assert!(
        residual_cross <= RESIDUAL_BUDGET_MS,
        "Me and Them slide {residual_cross:.1} ms apart by three hours, past the \
         {RESIDUAL_BUDGET_MS:.0} ms budget"
    );

    // …and the control, on the SAME records: the pre-22-B assumption that both
    // devices ran at exactly 16 kHz misses that budget by two orders of
    // magnitude, which is what makes the number above worth printing.
    let nominal: Vec<TrackTimeline> = tt
        .anchors
        .iter()
        .map(|name| TrackTimeline::nominal_rate(&read_index_array_json(&dir.join(name))))
        .collect();
    let nominal_residual = cross_track_residual_ms(
        (&nominal[MIC_TRACK], &two_track_truth(MIC_TRACK)),
        (&nominal[SYSTEM_TRACK], &two_track_truth(SYSTEM_TRACK)),
        RESIDUAL_HORIZON_SECONDS,
    );
    eprintln!("{TWO_TRACK}: nominal-rate control drifts {nominal_residual:.0} ms");
    assert!(
        nominal_residual > 20.0 * RESIDUAL_BUDGET_MS,
        "the nominal-rate control drifted only {nominal_residual:.1} ms"
    );

    // ── decode both tracks through the shipped chunker + timed merge ─────────
    let decoder = Decoder::new();
    let mut spans: Vec<HostSpan> = Vec::new();
    for track in [MIC_TRACK, SYSTEM_TRACK] {
        let (rate, samples) = read_wav_i16(&dir.join(&tt.wavs[track]));
        assert_eq!(rate, TARGET_RATE);
        let decoded = decoder.decode_chunked(&samples, &format!("{TWO_TRACK}-t{track}"));
        assert!(
            decoded.timestamps_are_real,
            "track {track} decoded without usable alignment — an ordering gate \
             needs word times, and the chunk-granularity fallback cannot answer \
             who spoke first"
        );
        // Every marker this track was given must come back exactly once. A
        // marker the decoder lost would make the sequence below shorter and the
        // comparison vacuous in the direction that hides a merge bug.
        let tokens = decoded.timed_tokens();
        for (_, on_track, word) in TWO_TRACK_CONVERSATION {
            if on_track != track {
                continue;
            }
            let seen = tokens.iter().filter(|t| *t == word).count();
            assert_eq!(
                seen, 1,
                "track {track} decoded the marker {word} {seen} time(s): \
                 {tokens:?}"
            );
        }
        spans.extend(decoded.timed_spans.iter().map(|s| HostSpan {
            track: track as i64,
            host_start_seconds: timelines[track].host_seconds(s.start_seconds),
            host_end_seconds: timelines[track].host_seconds(s.end_seconds),
            text: s.text.clone(),
        }));
    }

    // ── render, and score the order ──────────────────────────────────────────
    // Appended track by track, exactly as the pipeline stores them (one
    // transcription pass per recorded wav), so an ordered transcript is ordered
    // because of the host clock and not because of insert order.
    let lines = render_transcript(&segments_from_host_spans(TWO_TRACK, &spans));
    let starts: Vec<f64> = lines.iter().map(|l| l.start_seconds).collect();
    assert!(
        out_of_order(&starts).is_empty(),
        "the rendered transcript goes backwards in time: {:?}",
        out_of_order(&starts)
    );
    let got = marker_sequence(&lines, &two_track_words());
    assert_eq!(
        got,
        two_track_expected_sequence(),
        "the merged two-track transcript is not the conversation that was spoken"
    );
    eprintln!("{TWO_TRACK}: {} markers, in spoken order", got.len());
    // The conversation as a person reads it. Printed rather than asserted — the
    // assertion above is the gate; this is what makes a failure diagnosable and
    // what the PR quotes as the human-facing artifact of a UI-less item.
    let words = two_track_words();
    for line in &lines {
        if line.text.split_whitespace().any(|w| {
            words.iter().any(|m| {
                *m == w
                    .trim_matches(|c: char| !c.is_ascii_alphanumeric())
                    .to_ascii_lowercase()
            })
        }) {
            eprintln!("    [{}] {}: {}", line.offset, line.speaker, line.text);
        }
    }
}

/// five-second close-mic dictation is not. Under pure linear interpolation that
/// band folds into 0–8 kHz with single-digit-dB rejection and the WER that comes
/// back gets blamed on the model; under the anti-aliased decimator it is gone
/// before the fold.
///
/// The noise is synthesised here rather than committed, so no fixture and no
/// manifest hash changes: it is a deterministic comb of tones from 8.5 kHz to
/// just under Nyquist, which is unambiguously in the fold band and needs no RNG
/// agreement between machines. Both arms decode through the same binary, so the
/// only difference between the two numbers is the resampler.
#[test]
fn meeting_eval_antialias_decimation_does_not_regress_wer_on_broadband_noise() {
    let Some(root) = corpus() else { return };
    let meta = read_meta(&root, DEVICE_CHANGE);
    let path = root.join(DEVICE_CHANGE).join("segment-a-48000hz.wav");
    let (rate, native) = read_wav_i16(&path);
    assert_eq!(rate, 48_000, "the aliasing arm needs the native-rate half");
    let reference = normalize(&meta.utterances[0].text);

    let speech: Vec<f32> = native.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
    let noisy = with_ultrasonic_noise(&speech, rate);

    let decoder = Decoder::new();
    // "Before": the pre-YV92 path — decimate with no lowpass at all.
    let aliased_f = wilson_voice_lib::resample::resample_linear(&noisy, rate, TARGET_RATE);
    // "After": what the app ships now.
    let filtered_f = wilson_voice_lib::resample::resample_decimate(&noisy, rate, TARGET_RATE);
    let aliased = to_i16(&aliased_f);
    let filtered = to_i16(&filtered_f);
    assert_eq!(aliased.len(), filtered.len());

    // Before scoring anything: prove the two arms actually differ in the way
    // the finding says they do. The residual against the SAME decimation of the
    // clean half is exactly the noise energy each path let into the 0–8 kHz
    // band, so this measures the fold itself rather than a proxy for it.
    let clean_aliased = wilson_voice_lib::resample::resample_linear(&speech, rate, TARGET_RATE);
    let clean_filtered = wilson_voice_lib::resample::resample_decimate(&speech, rate, TARGET_RATE);
    let folded_linear = residual_rms(&aliased_f, &clean_aliased);
    let folded_filtered = residual_rms(&filtered_f, &clean_filtered);
    let removed_db = 20.0 * (folded_linear / folded_filtered.max(1e-12)).log10();
    println!(
        "meeting_eval antialias in_band_fold_linear={folded_linear:.6} in_band_fold_antialiased={folded_filtered:.6} removed_db={removed_db:.1}"
    );
    assert!(
        removed_db >= 20.0,
        "the anti-aliased decimator must keep ≥20 dB more of the >8 kHz band out of the \
         speech band on real fixture audio, got {removed_db:.1} dB"
    );

    let before = wer(
        &reference,
        &normalize(&decoder.decode(&aliased, "alias-linear")),
    );
    let after = wer(
        &reference,
        &normalize(&decoder.decode(&filtered, "alias-filtered")),
    );
    println!(
        "meeting_eval antialias broadband-noise wer_linear={:.4} wer_antialiased={:.4}",
        before.wer(),
        after.wer()
    );
    eprintln!("  linear (pre-YV92): {before}");
    eprintln!("  anti-aliased:      {after}");
    assert!(
        after.wer() <= before.wer(),
        "the anti-aliased decimator must not regress WER on broadband-noise audio: \
         linear {:.4} → anti-aliased {:.4}",
        before.wer(),
        after.wer()
    );

    // …and the filter must not be paying for that by eating the speech: on the
    // CLEAN half, where there is nothing above 8 kHz to fold, the anti-aliased
    // decode must be no worse than the linear one either.
    let clean_before = wer(
        &reference,
        &normalize(&decoder.decode(&to_i16(&clean_aliased), "clean-linear")),
    );
    let clean_after = wer(
        &reference,
        &normalize(&decoder.decode(&to_i16(&clean_filtered), "clean-filtered")),
    );
    println!(
        "meeting_eval antialias clean wer_linear={:.4} wer_antialiased={:.4}",
        clean_before.wer(),
        clean_after.wer()
    );
    assert!(
        clean_after.wer() <= clean_before.wer(),
        "the filter must not cost accuracy on audio with nothing to fold: \
         linear {:.4} → anti-aliased {:.4}",
        clean_before.wer(),
        clean_after.wer()
    );
}

/// Add broadband energy ABOVE the 8 kHz target Nyquist — the band that folds.
/// Deterministic (a fixed comb of tones with fixed phases), scaled to half the
/// speech RMS so it is a realistic room-noise floor rather than a stress test
/// nobody would ever record.
fn with_ultrasonic_noise(speech: &[f32], rate: u32) -> Vec<f32> {
    let speech_rms =
        (speech.iter().map(|s| (s * s) as f64).sum::<f64>() / speech.len().max(1) as f64).sqrt();
    let tones: Vec<f32> = (0..58).map(|k| 8_500.0 + k as f32 * 250.0).collect();
    let scale = (0.5 * speech_rms / (tones.len() as f64 / 2.0).sqrt()) as f32;
    speech
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let t = i as f32 / rate as f32;
            let noise: f32 = tones
                .iter()
                .enumerate()
                .map(|(k, &hz)| {
                    let phase = k as f32 * 0.7;
                    (2.0 * std::f32::consts::PI * hz * t + phase).sin()
                })
                .sum();
            (s + noise * scale).clamp(-1.0, 1.0)
        })
        .collect()
}

/// RMS of `a - b` over their common length — the energy one decimation added
/// that the other did not.
fn residual_rms(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let sum: f64 = a
        .iter()
        .zip(b.iter())
        .take(n)
        .map(|(x, y)| {
            let d = (x - y) as f64;
            d * d
        })
        .sum();
    (sum / n as f64).sqrt() as f32
}

fn to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect()
}

// ---------------------------------------------------------------------------
// The generator — synthetic audio only, run by hand, never in CI
// ---------------------------------------------------------------------------

/// The voice the corpus is rendered with. `say` output is stable for a given
/// macOS + voice build, and NOT across them: regenerating on a different machine
/// legitimately changes every sha256, which is why the manifest writer is a
/// separate one-liner below.
const VOICE: &str = "Samantha";
const WORDS_PER_MINUTE: u32 = 175;
/// Gap between spoken sentences, in seconds — enough for a VAD to find a
/// boundary, short enough that the lecture is mostly speech.
const SENTENCE_GAP: f64 = 0.35;
/// Fixture (a) target length.
const LECTURE_TARGET_SECONDS: f64 = 900.0;

/// Lecture-style clause pool. Mundane and about nobody: no names, no places, no
/// digits, nothing that could be mistaken for a real person's speech (the same
/// rule `gate_corpus_holds_no_private_dictation` enforces on the gate corpus).
const OPENERS: [&str; 24] = [
    "The first thing worth noticing here is that the problem statement is smaller than it looks",
    "If you go back to the earlier example for a moment",
    "There is a common mistake at this point in the argument",
    "Most of the difficulty in this topic comes from the notation rather than the idea",
    "Suppose for the sake of the discussion that the first condition already holds",
    "The reason this matters outside of the classroom",
    "One way to remember the ordering is to say it out loud",
    "When the working set gets larger than the cache",
    "A useful habit is to write the units down beside every quantity",
    "The textbook chapter skips a step here",
    "Notice that nothing in the derivation depends on the starting value",
    "It helps to draw the whole thing as a picture before writing a single line",
    "The second half of the reading covers the same ground more slowly",
    "In practice the measurement is noisy",
    "A rough estimate is usually enough to tell you whether the exact answer is worth computing",
    "There is a shorter proof of the same statement",
    "The interesting case is the one where both terms are small",
    "Before the break I want to leave you with a question",
    "The method generalises without much trouble",
    "Anyone who has tried this by hand will recognise the pattern",
    "The exercise at the end of the chapter is a good test of whether this landed",
    "It is easy to lose track of which side of the equation you are on",
    "The historical version of this argument was much longer",
    "If the room is quiet enough you can hear the fan",
];

const CONTINUATIONS: [&str; 24] = [
    "so it pays to write the assumptions down before starting",
    "which is why the ordering of the two steps cannot be swapped",
    "and the correction is easier to see once the terms are grouped",
    "so read it slowly the first time and quickly the second",
    "and everything after that follows from the same three rules",
    "is that it turns a guess into something you can check",
    "which is a habit worth keeping for the rest of the term",
    "the whole thing slows down in a way that is easy to mistake for a bug",
    "because a mismatch will show up long before the arithmetic does",
    "and filling it in yourself is the best half hour you can spend on the topic",
    "which is a stronger statement than it first appears",
    "and the picture usually makes the answer obvious",
    "so use whichever version you find easier to follow",
    "and averaging over a longer window buys you more than a better instrument",
    "and that judgment is most of the skill",
    "although it hides the part that explains why the result is true",
    "and the general case is a short step from there",
    "and I would rather you think about it than look it up",
    "as long as you keep track of what stays fixed",
    "and the pattern is the useful part rather than the example",
    "so try it before the next session",
    "and a single line of scratch paper prevents most of those errors",
    "and the modern one is short enough to fit on a page",
    "which tells you something about how quiet the room actually is",
];

/// Deterministic sentence `i`: strides through both pools with coprime steps, so
/// consecutive sentences never repeat and the pair only recurs after 24 x 24.
fn lecture_sentence(i: usize) -> String {
    format!(
        "{}, {}.",
        OPENERS[(i * 5) % OPENERS.len()],
        CONTINUATIONS[(i * 7 + 3) % CONTINUATIONS.len()]
    )
}

/// One unique, unambiguous, easily-decoded word per chunk boundary. Each appears
/// EXACTLY ONCE in the whole fixture, which is what the seam gate leans on.
const SEAM_KEYWORDS: [&str; 5] = ["pineapple", "trombone", "lantern", "walnut", "envelope"];

/// The carrier sentence is synthesized in three pieces so the marker WORD can be
/// positioned to the sample, rather than the sentence being centred and the word
/// landing wherever the synthesizer's prosody puts it.
const MARKER_PREFIX: &str = "The marker word for this boundary is";
const MARKER_SUFFIX: &str = "and it is spoken exactly once.";
/// Silence between the three pieces, so they read as one sentence.
const MARKER_JOIN_SECONDS: f64 = 0.06;

fn straddle_sentence(keyword: &str) -> String {
    format!("{MARKER_PREFIX} {keyword}, {MARKER_SUFFIX}")
}

/// `say` pads its output with a little silence at both ends, and the marker word
/// has to be placed by where the WORD is, not where the file starts.
fn trim_silence(samples: &[i16]) -> &[i16] {
    // ~ -40 dBFS. Synthesized silence is digital-zero-ish, so this is generous.
    const FLOOR: u16 = 300;
    let Some(first) = samples.iter().position(|s| s.unsigned_abs() > FLOOR) else {
        return &[];
    };
    let last = samples
        .iter()
        .rposition(|s| s.unsigned_abs() > FLOOR)
        .expect("a first implies a last");
    &samples[first..=last]
}

const DEVICE_CHANGE_FIRST: &str =
    "This first half is captured from the built in microphone at its usual rate, and it runs \
     for about ten seconds before anything changes.";
const DEVICE_CHANGE_SECOND: &str =
    "This second half is captured after the input device changed to a headset, which reports a \
     different rate entirely, and the transcript must not break here.";

/// Render `text` with the system speech synthesizer at `rate_hz`, mono 16-bit.
/// Nothing leaves the machine and no third-party asset is involved.
fn synthesize(text: &str, rate_hz: u32) -> Vec<i16> {
    let scratch = std::env::temp_dir().join("yap-meeting-eval-gen");
    fs::create_dir_all(&scratch).expect("scratch dir");
    let aiff = scratch.join("utterance.aiff");
    let wav = scratch.join("utterance.wav");
    let _ = fs::remove_file(&aiff);
    let _ = fs::remove_file(&wav);

    let say = Command::new("say")
        .args(["-v", VOICE, "-r", &WORDS_PER_MINUTE.to_string(), "-o"])
        .arg(&aiff)
        .arg(text)
        .status()
        .expect("`say` is a macOS built-in");
    assert!(say.success(), "say failed on: {text}");

    let convert = Command::new("afconvert")
        .args(["-f", "WAVE", "-d", &format!("LEI16@{rate_hz}"), "-c", "1"])
        .arg(&aiff)
        .arg(&wav)
        .status()
        .expect("`afconvert` is a macOS built-in");
    assert!(convert.success(), "afconvert failed on: {text}");

    let (found, samples) = read_wav_i16(&wav);
    assert_eq!(found, rate_hz, "afconvert ignored the requested rate");
    samples
}

fn seconds(samples: usize) -> f64 {
    samples as f64 / TARGET_RATE as f64
}

fn write_fixture(root: &Path, meta: &FixtureMeta, audio: &[i16]) {
    let dir = root.join(&meta.id);
    fs::create_dir_all(&dir).expect("fixture dir");
    write_wav_16k_mono(&dir.join("audio.wav"), audio);
    let reference: String = meta
        .utterances
        .iter()
        .map(|u| u.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    fs::write(dir.join("reference.txt"), format!("{reference}\n")).expect("reference");
    fs::write(
        dir.join("meta.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(meta).expect("meta serialises")
        ),
    )
    .expect("meta");
    eprintln!(
        "wrote {} — {:.1}s, {} utterances",
        dir.display(),
        meta.duration_seconds,
        meta.utterances.len()
    );
}

/// Grow the whole corpus from scratch. Not a check — a writer, run by hand.
#[test]
#[ignore = "writer, not a check: renders ~18 minutes of synthetic audio with `say`"]
fn meeting_eval_generate_corpus() {
    let root = corpus_root();
    fs::create_dir_all(&root).expect("corpus root");
    eprintln!("growing the meeting eval corpus in {}", root.display());

    generate_lecture(&root);
    generate_seam_stress(&root);
    generate_device_change(&root);
    generate_two_track_ordering(&root);

    write_manifest_from(&root);
}

/// Fixture (a): a single-speaker lecture-style monologue, ~15 minutes. The
/// reference transcript is exact by construction — it is the text that was
/// spoken, not a human's guess at what was heard.
fn generate_lecture(root: &Path) {
    let gap = vec![0i16; (SENTENCE_GAP * TARGET_RATE as f64) as usize];
    let mut audio: Vec<i16> = Vec::new();
    let mut utterances: Vec<Utterance> = Vec::new();
    let mut i = 0usize;
    while seconds(audio.len()) < LECTURE_TARGET_SECONDS {
        let text = lecture_sentence(i);
        let spoken = synthesize(&text, TARGET_RATE);
        let start = seconds(audio.len());
        audio.extend_from_slice(&spoken);
        utterances.push(Utterance {
            text,
            start_seconds: start,
            end_seconds: seconds(audio.len()),
        });
        audio.extend_from_slice(&gap);
        i += 1;
        if i % 20 == 0 {
            eprintln!("  {LECTURE}: {:.0}s rendered", seconds(audio.len()));
        }
    }
    let meta = FixtureMeta {
        id: LECTURE.to_string(),
        kind: "single_speaker_lecture".to_string(),
        sample_rate: TARGET_RATE,
        duration_seconds: seconds(audio.len()),
        utterances,
        boundary_seconds: Vec::new(),
        seam_keywords: Vec::new(),
        marker_spans: Vec::new(),
        chunk_seconds: None,
        chunk_overlap_seconds: None,
        device_change_seconds: None,
        source_rates_hz: vec![TARGET_RATE],
        two_track: None,
    };
    write_fixture(root, &meta, &audio);
}

/// Fixture (b): built so that a marker WORD lands inside every chunk seam.
///
/// "Seam" is derived, never typed: with 30 s windows and a 2 s overlap the
/// windows are 0–30, 28–58, 56–86 …, so the only regions two windows both
/// contain are `[k*28, k*28+2]` ([`seam_region`]). The first cut of this fixture
/// centred each marker SENTENCE on `k * 30 s` instead, which put four of the
/// five markers in the interior of a single window — and a marker only one
/// window sees produces `duplicated == 0, dropped == 0` under a correct merge, a
/// duplicating merge and a word-eating merge alike. The gate was 80% vacuous:
/// the exact failure mode finding #16 describes, reproduced inside the harness
/// built to prevent it.
///
/// So each marker word is now placed entirely inside its overlap region, at the
/// region's midpoint, with the carrier sentence synthesized in three pieces so
/// the word itself can be positioned to the sample. Both windows therefore
/// decode the marker, and the seam counts have something to measure:
/// `meeting_eval_seam_dedupe_and_ordering_hold` re-checks that from `meta.json`
/// AND from the per-window decodes before it scores anything.
fn generate_seam_stress(root: &Path) {
    let boundaries: Vec<f64> = (1..=SEAM_KEYWORDS.len())
        .map(|k| seam_region(k).0)
        .collect();
    let last_seam_end = seam_region(SEAM_KEYWORDS.len()).1;
    let total_seconds = last_seam_end + 20.0;
    let mut audio = vec![0i16; (total_seconds * TARGET_RATE as f64) as usize];
    let mut placed: Vec<Utterance> = Vec::new();
    let mut marker_spans: Vec<Utterance> = Vec::new();

    let join = (MARKER_JOIN_SECONDS * TARGET_RATE as f64) as usize;
    let prefix = synthesize(MARKER_PREFIX, TARGET_RATE);
    let prefix = trim_silence(&prefix).to_vec();
    let suffix = synthesize(MARKER_SUFFIX, TARGET_RATE);
    let suffix = trim_silence(&suffix).to_vec();

    // The markers first: each marker WORD centred in its overlap region, so both
    // of the windows that share that region contain the whole word.
    let mut occupied: Vec<(usize, usize)> = Vec::new();
    for (k, keyword) in SEAM_KEYWORDS.iter().enumerate() {
        let (from, to) = seam_region(k + 1);
        let spoken_word = synthesize(keyword, TARGET_RATE);
        let spoken_word = trim_silence(&spoken_word);
        assert!(
            seconds(spoken_word.len()) + 2.0 * MARKER_JOIN_SECONDS < CHUNK_OVERLAP_SECONDS,
            "marker {keyword} is {:.2}s long and cannot fit inside a \
             {CHUNK_OVERLAP_SECONDS}s overlap",
            seconds(spoken_word.len())
        );

        let centre = ((from + to) / 2.0 * TARGET_RATE as f64) as usize;
        let word_start = centre - spoken_word.len() / 2;
        let word_end = word_start + spoken_word.len();
        assert!(
            seconds(word_start) >= from && seconds(word_end) <= to,
            "marker {keyword} at {:.3}s–{:.3}s escaped its overlap {from}s–{to}s",
            seconds(word_start),
            seconds(word_end)
        );

        let start = word_start - join - prefix.len();
        let end = word_end + join + suffix.len();
        assert!(end < audio.len(), "marker for {keyword} runs off the end");
        audio[start..start + prefix.len()].copy_from_slice(&prefix);
        audio[word_start..word_end].copy_from_slice(spoken_word);
        audio[word_end + join..end].copy_from_slice(&suffix);

        occupied.push((start, end));
        placed.push(Utterance {
            text: straddle_sentence(keyword),
            start_seconds: seconds(start),
            end_seconds: seconds(end),
        });
        marker_spans.push(Utterance {
            text: (*keyword).to_string(),
            start_seconds: seconds(word_start),
            end_seconds: seconds(word_end),
        });
    }

    // Then ordinary speech in the gaps, so the windows are not mostly silence.
    let gap = (SENTENCE_GAP * TARGET_RATE as f64) as usize;
    let mut filler = 0usize;
    let mut cursor = 0usize;
    for (marker_start, marker_end) in occupied.iter().copied().chain([(audio.len(), audio.len())]) {
        loop {
            // Ordinary lecture prose, from the same deterministic pool as
            // fixture (a) — every filler sentence is DISTINCT, so a merge can
            // never splice two windows on a repeated sentence and look correct
            // by accident.
            let text = lecture_sentence(filler);
            let spoken = synthesize(&text, TARGET_RATE);
            if cursor + spoken.len() + gap >= marker_start {
                break;
            }
            audio[cursor..cursor + spoken.len()].copy_from_slice(&spoken);
            placed.push(Utterance {
                text,
                start_seconds: seconds(cursor),
                end_seconds: seconds(cursor + spoken.len()),
            });
            cursor += spoken.len() + gap;
            filler += 1;
        }
        cursor = marker_end + gap;
    }
    placed.sort_by(|a, b| a.start_seconds.total_cmp(&b.start_seconds));

    let meta = FixtureMeta {
        id: SEAM_STRESS.to_string(),
        kind: "seam_stress".to_string(),
        sample_rate: TARGET_RATE,
        duration_seconds: seconds(audio.len()),
        utterances: placed,
        boundary_seconds: boundaries,
        seam_keywords: SEAM_KEYWORDS.iter().map(|s| s.to_string()).collect(),
        marker_spans,
        chunk_seconds: Some(CHUNK_SECONDS),
        chunk_overlap_seconds: Some(CHUNK_OVERLAP_SECONDS),
        device_change_seconds: None,
        source_rates_hz: vec![TARGET_RATE],
        two_track: None,
    };
    write_fixture(root, &meta, &audio);
}

/// Fixture (c): a short recording whose input format changes mid-way — 48 kHz
/// (built-in mic) then 24 kHz (what AirPods report). Both native-rate halves are
/// kept beside the resampled 16 kHz track so YV92 can drive `StreamResampler`
/// at the rate the device actually declared, which is the whole bug: the ratio
/// is computed once at stream start and never revisited (OS-9).
fn generate_device_change(root: &Path) {
    let dir = root.join(DEVICE_CHANGE);
    fs::create_dir_all(&dir).expect("fixture dir");

    let first_native = synthesize(DEVICE_CHANGE_FIRST, 48_000);
    let second_native = synthesize(DEVICE_CHANGE_SECOND, 24_000);
    write_wav(&dir.join("segment-a-48000hz.wav"), 48_000, &first_native);
    write_wav(&dir.join("segment-b-24000hz.wav"), 24_000, &second_native);

    let first = synthesize(DEVICE_CHANGE_FIRST, TARGET_RATE);
    let second = synthesize(DEVICE_CHANGE_SECOND, TARGET_RATE);
    let gap = vec![0i16; (SENTENCE_GAP * TARGET_RATE as f64) as usize];
    let mut audio = first.clone();
    audio.extend_from_slice(&gap);
    let change_at = seconds(audio.len());
    audio.extend_from_slice(&second);

    let meta = FixtureMeta {
        id: DEVICE_CHANGE.to_string(),
        kind: "input_format_change".to_string(),
        sample_rate: TARGET_RATE,
        duration_seconds: seconds(audio.len()),
        utterances: vec![
            Utterance {
                text: DEVICE_CHANGE_FIRST.to_string(),
                start_seconds: 0.0,
                end_seconds: seconds(first.len()),
            },
            Utterance {
                text: DEVICE_CHANGE_SECOND.to_string(),
                start_seconds: change_at,
                end_seconds: seconds(audio.len()),
            },
        ],
        boundary_seconds: Vec::new(),
        seam_keywords: Vec::new(),
        marker_spans: Vec::new(),
        chunk_seconds: None,
        chunk_overlap_seconds: None,
        device_change_seconds: Some(change_at),
        source_rates_hz: vec![48_000, 24_000],
        two_track: None,
    };
    write_fixture(root, &meta, &audio);
}

/// Fixture (d): a synthetic TWO-SOURCE capture, deliberately clock-offset.
///
/// Three things make it worth the disk it takes.
///
/// **The wavs come out of the shipped capture path**, not out of a buffer
/// written straight to a file. Each track's local audio is fed to a real
/// `MeetingCapture` in real callback-sized blocks, with the anchors a real
/// callback would have stamped, into a real `MeetingJournal`; the fixture's
/// `trackN.wav` is what `finalize` produced. So the audio the gate decodes has
/// been through the same high-pass, the same resampler and the same spill the
/// app puts a meeting through.
///
/// **The anchors come out of the same run.** The index sidecars the journal
/// wrote are copied into the fixture before `finalize` removes them, which is
/// what lets the gate rebuild each track's timeline from records the shipped
/// consumer produced rather than from arithmetic this file did.
///
/// **The clocks disagree on purpose.** Both devices are told they are 16 kHz;
/// one really runs 40 ppm slow and the other 250 ppm fast, and the second one
/// starts 750 ms late. A fixture recorded on two well-behaved crystals over two
/// minutes drifts by microseconds and would pass under a merge that did
/// nothing — the same "unfalsifiable by construction" failure fixture (b) was
/// rebuilt to escape.
fn generate_two_track_ordering(root: &Path) {
    let dir = root.join(TWO_TRACK);
    fs::create_dir_all(&dir).expect("fixture dir");

    let join = (MARKER_JOIN_SECONDS * TARGET_RATE as f64) as usize;
    let prefix = trim_silence(&synthesize(TWO_TRACK_PREFIX, TARGET_RATE)).to_vec();
    let suffix = trim_silence(&synthesize(TWO_TRACK_SUFFIX, TARGET_RATE)).to_vec();

    // Each track's own local length: the mic covers the whole meeting, the tap
    // covers it from the moment its aggregate device came up.
    let local_len = |track: usize| -> usize {
        let from = if track == MIC_TRACK {
            0.0
        } else {
            TWO_TRACK_ORIGIN_OFFSET_SECONDS
        };
        ((TWO_TRACK_SECONDS - from) * (1.0 + TWO_TRACK_PPM[track]) * TARGET_RATE as f64) as usize
    };
    let mut audio: Vec<Vec<i16>> = (0..2).map(|t| vec![0i16; local_len(t)]).collect();
    let mut occupied: Vec<Vec<(usize, usize)>> = vec![Vec::new(), Vec::new()];
    let mut markers: Vec<TwoTrackMarker> = Vec::new();
    let mut utterances: Vec<Utterance> = Vec::new();

    for (host_seconds, track, word) in TWO_TRACK_CONVERSATION {
        let spoken = synthesize(word, TARGET_RATE);
        let spoken = trim_silence(&spoken);
        let local = two_track_local_seconds(track, host_seconds);
        let word_start = (local * TARGET_RATE as f64) as usize;
        let word_end = word_start + spoken.len();
        let start = word_start
            .checked_sub(join + prefix.len())
            .unwrap_or_else(|| panic!("marker {word} starts before its own track does"));
        let end = word_end + join + suffix.len();
        assert!(
            end < audio[track].len(),
            "marker {word} runs off the end of track {track}"
        );
        audio[track][start..start + prefix.len()].copy_from_slice(&prefix);
        audio[track][word_start..word_end].copy_from_slice(spoken);
        audio[track][word_end + join..end].copy_from_slice(&suffix);
        occupied[track].push((start, end));
        markers.push(TwoTrackMarker {
            word: word.to_string(),
            track: track as i64,
            speaker: two_track_speaker(track).to_string(),
            host_seconds,
            local_seconds: seconds(word_start),
        });
        utterances.push(Utterance {
            text: format!(
                "{two_track_speaker}: {TWO_TRACK_PREFIX} {word}, {TWO_TRACK_SUFFIX}",
                two_track_speaker = two_track_speaker(track)
            ),
            start_seconds: host_seconds,
            end_seconds: host_seconds + seconds(spoken.len()),
        });
    }

    // Ordinary prose in the gaps so no window is mostly silence — and every
    // sentence DISTINCT, across both tracks, so a merge that attributed one
    // track's speech to the other could not look right by repeating itself.
    let gap = (SENTENCE_GAP * TARGET_RATE as f64) as usize;
    let mut filler = 0usize;
    for track in [MIC_TRACK, SYSTEM_TRACK] {
        occupied[track].sort();
        let mut cursor = 0usize;
        let track_len = audio[track].len();
        for (marker_start, marker_end) in occupied[track]
            .clone()
            .into_iter()
            .chain([(track_len, track_len)])
        {
            loop {
                let text = lecture_sentence(filler);
                let spoken = synthesize(&text, TARGET_RATE);
                if cursor + spoken.len() + gap >= marker_start {
                    break;
                }
                audio[track][cursor..cursor + spoken.len()].copy_from_slice(&spoken);
                cursor += spoken.len() + gap;
                filler += 1;
            }
            cursor = marker_end + gap;
        }
    }

    // ── the shipped capture path, driven with the anchors a real callback
    //    would have stamped ────────────────────────────────────────────────
    let cap_dir = std::env::temp_dir().join(format!(
        "yap-eval-two-track-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&cap_dir).expect("capture dir");
    // A deep queue, for the same reason `two_track_phase_e2e` opens one: this
    // loop feeds 76 seconds of audio as fast as the CPU will take it, and the
    // shipped bound (`MEETING_QUEUE_DEPTH` = 512) is sized for a real-time
    // producer. At the shipped depth the writer is outrun on a loaded machine
    // and the generator's own `spliced_silence_samples == 0` assertion fires —
    // which is the assertion doing its job, but it makes growing the corpus a
    // question of how busy the Mac is. Backpressure has its own test.
    let journal = MeetingJournal::start_with_depth(&cap_dir, 2, 32_768).expect("journal");
    let journal_id = journal.id().to_string();
    let capture = MeetingCapture::with_tracks(TARGET_RATE, 1, 2, Some(journal));
    for track in [MIC_TRACK, SYSTEM_TRACK] {
        let frames = TWO_TRACK_CALLBACK_FRAMES[track];
        let origin = if track == MIC_TRACK {
            0.0
        } else {
            TWO_TRACK_ORIGIN_OFFSET_SECONDS
        };
        let blocks = audio[track].len() / frames;
        for k in 0..blocks {
            let at = k * frames;
            let block: Vec<f32> = audio[track][at..at + frames]
                .iter()
                .map(|s| *s as f32 / 32_768.0)
                .collect();
            let host = origin + at as f64 / (TARGET_RATE as f64 * (1.0 + TWO_TRACK_PPM[track]));
            capture.accept_track(
                track,
                &block,
                &[CaptureAnchor {
                    host_ns: (host * 1_000_000_000.0) as u64,
                    sample_index: at as u64,
                    frames: frames as u32,
                    sample_rate: TARGET_RATE,
                    lost_frames: 0,
                }],
            );
        }
    }
    let records = support::two_track::wait_for_index_records(
        &cap_dir,
        &journal_id,
        2,
        TWO_TRACK_SECONDS as usize - 3,
    );
    let journal = capture.close().expect("the journal comes back");
    let finalized = journal
        .finalize(MeetingState::Complete)
        .expect("the synthetic capture finalizes");
    assert_eq!(
        finalized.spliced_silence_samples, 0,
        "the generator's own capture lost audio — the fixture's local sample \
         positions would no longer be the ones the markers were placed at"
    );

    for track in [MIC_TRACK, SYSTEM_TRACK] {
        let wav = finalized
            .wav_for_track(track)
            .unwrap_or_else(|| panic!("track {track} finalized into a wav"));
        let (rate, samples) = read_wav_i16(wav);
        assert_eq!(rate, TARGET_RATE);
        let fed = audio[track].len() / TWO_TRACK_CALLBACK_FRAMES[track]
            * TWO_TRACK_CALLBACK_FRAMES[track];
        assert!(
            samples.len().abs_diff(fed) < TARGET_RATE as usize / 100,
            "track {track} finalized {} samples for {fed} fed — a shifted track \
             would put every marker somewhere other than where it was placed",
            samples.len()
        );
        write_wav_16k_mono(&dir.join(format!("track{track}.wav")), &samples);
        // A JSON ARRAY rather than the journal's own JSON-lines: the corpus
        // holds wav/txt/json only (`meeting_eval_manifest_is_committed_and_
        // names_every_fixture` enforces it), and a `.jsonl` sidecar copied in
        // verbatim would be the one file in the corpus nothing could parse.
        // The records themselves are copied field for field.
        let body: Vec<String> = records[track]
            .iter()
            .map(|r| {
                format!(
                    "  {{ \"host_ns\": {}, \"captured_samples\": {}, \"spilled_samples\": {} }}",
                    r.host_ns, r.captured_samples, r.spilled_samples
                )
            })
            .collect();
        fs::write(
            dir.join(format!("track{track}-anchors.json")),
            format!("[\n{}\n]\n", body.join(",\n")),
        )
        .expect("anchors");
    }
    let _ = fs::remove_dir_all(&cap_dir);

    utterances.sort_by(|a, b| a.start_seconds.total_cmp(&b.start_seconds));
    let reference: String = utterances
        .iter()
        .map(|u| u.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(dir.join("reference.txt"), format!("{reference}\n")).expect("reference");

    let meta = FixtureMeta {
        id: TWO_TRACK.to_string(),
        kind: "two_track_ordering".to_string(),
        sample_rate: TARGET_RATE,
        duration_seconds: TWO_TRACK_SECONDS,
        utterances,
        boundary_seconds: Vec::new(),
        seam_keywords: Vec::new(),
        marker_spans: Vec::new(),
        chunk_seconds: Some(CHUNK_SECONDS),
        chunk_overlap_seconds: Some(CHUNK_OVERLAP_SECONDS),
        device_change_seconds: None,
        source_rates_hz: vec![TARGET_RATE, TARGET_RATE],
        two_track: Some(TwoTrackMeta {
            wavs: vec!["track0.wav".into(), "track1.wav".into()],
            anchors: vec!["track0-anchors.json".into(), "track1-anchors.json".into()],
            origin_seconds: vec![0.0, TWO_TRACK_ORIGIN_OFFSET_SECONDS],
            clock_ppm: TWO_TRACK_PPM.iter().map(|p| p * 1e6).collect(),
            true_rate_hz: TWO_TRACK_PPM
                .iter()
                .map(|p| TARGET_RATE as f64 * (1.0 + p))
                .collect(),
            callback_frames: TWO_TRACK_CALLBACK_FRAMES.to_vec(),
            markers,
            expected_sequence: two_track_expected_sequence(),
            residual_horizon_seconds: RESIDUAL_HORIZON_SECONDS,
            residual_budget_ms: RESIDUAL_BUDGET_MS,
        }),
    };
    fs::write(
        dir.join("meta.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&meta).expect("meta serialises")
        ),
    )
    .expect("meta");
    eprintln!(
        "wrote {} — two tracks, {:.1}s, {} markers",
        dir.display(),
        TWO_TRACK_SECONDS,
        TWO_TRACK_CONVERSATION.len()
    );
}

// ---------------------------------------------------------------------------
// The manifest writer
// ---------------------------------------------------------------------------

/// `shasum -a 256 -c` / `sha256sum -c` format, paths relative to the corpus
/// root. Committed beside the JSON so the corpus can be checked by hand:
/// `cd ~/yap-eval-corpus/meetings && shasum -a 256 -c …/meeting_eval_manifest.sha256`.
fn checksum_file_body(m: &Manifest) -> String {
    let mut out = String::new();
    for f in &m.files {
        out.push_str(&format!("{}  {}\n", f.sha256, f.path));
    }
    out
}

fn walk(dir: &Path, into: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|e| e.expect("dir entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(&path, into);
        } else if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        {
            continue;
        } else {
            into.push(path);
        }
    }
}

fn write_manifest_from(root: &Path) {
    let mut files: Vec<PathBuf> = Vec::new();
    walk(root, &mut files);
    assert!(!files.is_empty(), "nothing in {}", root.display());

    let entries: Vec<FileEntry> = files
        .iter()
        .map(|p| {
            let (bytes, sha256) = sha256_file(p);
            FileEntry {
                path: p
                    .strip_prefix(root)
                    .expect("inside the corpus root")
                    .to_string_lossy()
                    .to_string(),
                bytes,
                sha256,
            }
        })
        .collect();

    let manifest = Manifest {
        generator: "desktop/src-tauri/tests/meeting_eval.rs::meeting_eval_generate_corpus"
            .to_string(),
        corpus_root: format!("~/{CORPUS_HOME_RELATIVE}"),
        fixtures: FIXTURE_IDS.iter().map(|s| s.to_string()).collect(),
        synthesis: Synthesis {
            tool: "macOS `say` -> `afconvert` (no network, no third-party asset)".to_string(),
            voice: VOICE.to_string(),
            words_per_minute: WORDS_PER_MINUTE,
        },
        note: "SYNTHETIC audio only — no real dictation, no meeting recording, and no third \
               party's voice ever enters this corpus. `say` output is stable for a given macOS \
               + voice build and NOT across them, so regenerating on another machine changes \
               every hash below; re-run the writer and commit the diff."
            .to_string(),
        files: entries,
    };

    fs::write(
        manifest_path(),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&manifest).expect("manifest serialises")
        ),
    )
    .expect("manifest is writable");
    fs::write(checksum_path(), checksum_file_body(&manifest)).expect("checksum file is writable");
    eprintln!(
        "wrote {} and {} ({} files)",
        manifest_path().display(),
        checksum_path().display(),
        manifest.files.len()
    );
}

/// Regrow ONLY the seam fixture, then re-hash the whole corpus into both
/// manifests. The seam fixture is the one tied to the chunk geometry, so it is
/// the one that has to be rebuilt when [`CHUNK_SECONDS`] or
/// [`CHUNK_OVERLAP_SECONDS`] moves; rebuilding the 15-minute lecture at the same
/// time would change its hashes and invalidate a measured WER for no reason.
#[test]
#[ignore = "writer, not a check: renders the seam fixture with `say`"]
fn meeting_eval_generate_seam_stress() {
    let root = corpus_root();
    fs::create_dir_all(&root).expect("corpus root");
    generate_seam_stress(&root);
    write_manifest_from(&root);
}

/// Regrow ONLY the two-track fixture, then re-hash. Its geometry is tied to the
/// clock constants above and to the shipped capture path, so it is the one to
/// rebuild when either moves — rebuilding the 15-minute lecture beside it would
/// change its hashes and invalidate a measured WER for no reason.
#[test]
#[ignore = "writer, not a check: renders the two-track fixture with `say`"]
fn meeting_eval_generate_two_track_ordering() {
    let root = corpus_root();
    fs::create_dir_all(&root).expect("corpus root");
    generate_two_track_ordering(&root);
    write_manifest_from(&root);
}

/// Re-hash an existing corpus into the committed manifest, without regenerating
/// the audio. Not a check — a writer.
#[test]
#[ignore = "writer, not a check: rewrites the committed manifest"]
fn meeting_eval_write_manifest() {
    let Some(root) = corpus() else {
        panic!("{CORPUS_ABSENT}");
    };
    write_manifest_from(&root);
}
