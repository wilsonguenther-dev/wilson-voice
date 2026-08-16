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
//! * **Speaker ground truth** — YV120 adds the diarization half. The metrics
//!   themselves (DER, JER, enrollment-EER) are pure functions over labeled
//!   intervals and live in `wilson_voice_lib::diarize_metrics`, pinned to
//!   hand-worked answers in `tests/diarization_metrics.rs`. What lives HERE is
//!   what only a corpus can carry: fixtures (e) and (f), each with RTTM speaker
//!   turns that are exact by construction because the fixture was ASSEMBLED
//!   from them. No DER/JER number is gated yet — there is no diarizer to score
//!   until YV126 — and the gate constants say `None` rather than a guess, for
//!   the same reason YV90 shipped `WER_GATE = 0.15` as an admitted placeholder
//!   and YV93 replaced it with a measurement.
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
//! # regrow only YV120's diarization fixtures (e) and (f)
//! cargo test --test meeting_eval meeting_eval_generate_diarization_fixtures -- --ignored --nocapture
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
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// YV93: the geometry and the merge are no longer this file's own. The gates
// below score the SHIPPED chunker — same plan, same seam merge, same constants —
// so a regression in `meeting_asr` fails here instead of passing against a copy
// of itself.
use wilson_voice_lib::asr_engine::{TimedKind, TimedSpan, TimedTranscript};
// YV120: the diarization metrics score fixtures (e) and (f). Same rule as the
// chunker above — the harness runs the SHIPPED metric, never a copy of it, so a
// change to `der`/`jer` fails here as well as in its own unit file.
// YV124 adds the EER half of finding OS-8's validation arm, and takes the same
// posture: `cosine_similarity` and `enrollment_eer` are the SHIPPED functions,
// not a copy of them, so the arm cannot be scored by a metric that has drifted
// away from the one enrollment will use.
use wilson_voice_lib::diarize_metrics::{
    cosine_similarity, der, enrollment_eer, jer, CosineSimilarity, EerReport, RttmTurn,
};
use wilson_voice_lib::meeting_asr::{
    merge_chunk_tokens, merge_chunk_tokens_reporting, merge_timed, merge_timed_reporting,
    plan_windows, plan_windows_fixed, timestamps_are_usable, BoundaryKind, ChunkConfig,
    ChunkOutcome, ChunkStatus, ChunkWindow, MemoryWindows, MergeReport, ResumePoint, SampleWindows,
    SeamDecision, VoiceActivity, MAX_ANCHOR_TOKENS, MAX_HEAD_SKIP, MAX_TAIL_TRIM,
    OVERLAP_TOKEN_BUDGET, SEAM_TRUNCATION_SLACK,
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
use wilson_voice_lib::meetings::{
    render_transcript, MeetingKind, MIC_SPEAKER_LABEL, SYSTEM_SPEAKER_LABEL,
};
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
/// YV120 fixture (e): three people round one mic, near-field, no overlap — the
/// case pyannote-segmentation-3.0's mechanism ceiling has no trouble with, and
/// therefore the fixture YV126's clustering threshold gets TUNED against.
const ROOM_3: &str = "room-3-near-field";
/// YV120 fixture (f): six people, far-field, deliberate crosstalk — built to
/// EXCEED that ceiling (merged finding #5: 3 speakers per 10 s window, 2
/// simultaneous), so that "full N-way clustering cannot do this" is a
/// measurement rather than an opinion.
const CLASSROOM_6: &str = "classroom-6-far-field";
/// Every fixture the manifest must name. Fixture (c) is generated here but
/// consumed by YV92 (anti-alias + input format change), not by this file's gates.
const FIXTURE_IDS: [&str; 6] = [
    LECTURE,
    SEAM_STRESS,
    DEVICE_CHANGE,
    TWO_TRACK,
    ROOM_3,
    CLASSROOM_6,
];

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
    /// YV120 fixtures (e)/(f): who is speaking when, as RTTM speaker turns.
    ///
    /// EXACT by construction, not annotated: the generator places one
    /// synthesized utterance per turn at a start it chose, so the ground truth
    /// is the schedule the audio was assembled from rather than a human's guess
    /// at what they heard. That is what makes a DER measured against it a
    /// statement about the diarizer and nothing else — the same argument that
    /// makes this corpus's WER references exact.
    ///
    /// Serialised as `{speaker_id, start_seconds, end_seconds}`, the shape
    /// `pyannote.metrics` and `dscore` consume, so a cross-check against either
    /// costs a `RttmTurn::to_rttm_line` call.
    #[serde(default)]
    rttm: Vec<RttmTurn>,
    /// YV120 fixtures (e)/(f): which synthesizer voice each RTTM speaker id was
    /// rendered with, and how far from the mic they were placed.
    ///
    /// Committed because "these two speakers are different people" is the
    /// fixture's central claim: a corpus regrown with one voice for all six
    /// would still have well-formed RTTM and would silently be scoring nothing.
    ///
    /// This field is provenance, NOT evidence. It records what the generator was
    /// asked to render; it cannot record what it rendered, and the first cut of
    /// this corpus proved the difference by shipping six declared voices over
    /// one actual one. The claim is checked against the samples, in
    /// [`assert_rttm_fits_the_audio`].
    #[serde(default)]
    speakers: Vec<FixtureSpeaker>,
}

/// One speaker of a YV120 diarization fixture.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FixtureSpeaker {
    /// The id used in [`FixtureMeta::rttm`] and in `reference.txt`.
    id: String,
    /// The macOS `say` voice. Distinct per speaker, by construction.
    voice: String,
    /// Direct-path gain applied before mixing — 1.0 is at the mic. Fixture (f)
    /// seats its six speakers at different distances, because a far-field room
    /// where everyone is equally loud is not a far-field room.
    direct_gain: f32,
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
    plan_windows_fixed(
        total_seconds,
        ResumePoint::start(),
        &ChunkConfig::default(),
        0,
    )
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
            decode
                .timed_spans
                .iter()
                .all(|s| s.end_seconds >= s.start_seconds),
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
        assert!(
            timed_starts.is_sorted(),
            "the timed merge came out unsorted"
        );
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
    let floats: Vec<f32> = samples
        .iter()
        .map(|&s| s as f32 / i16::MAX as f32)
        .collect();
    let audio = MemoryWindows::new(floats, TARGET_RATE);
    let cfg = ChunkConfig::default();
    let plan = plan_windows(
        &audio,
        Some(&vad as &dyn VoiceActivity),
        ResumePoint::start(),
        &cfg,
        0,
    )
    .expect("plan");

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
            .map(|s| {
                (cut - s.end_seconds)
                    .abs()
                    .min((s.start_seconds - cut).abs())
            })
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
    eprintln!(
        "{LECTURE} (VAD-cut): text-anchor merge {fallback}; {:?}",
        decode.merge
    );
    assert_eq!(decode.merge.no_anchor_seams, 0);
    assert!(
        fallback.wer() <= WER_GATE,
        "VAD-cut fallback merge: {fallback}"
    );

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
    let lines = // A CALL with both tracks recorded — the configuration whose
    // labels are Me/Them (YV125).
    render_transcript(
        &segments_from_host_spans("no-rebase", &spans),
        MeetingKind::Virtual,
    );
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
    let lines = render_transcript(
        &segments_from_host_spans(TWO_TRACK, &spans),
        MeetingKind::Virtual,
    );
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
    // YV124: this was a bare `20.0` and the EER arm below had its own,
    // identical literal, while both the PR body and the backlog claimed the two
    // arms shared "one constant, asserted in two places". They did not — two
    // independent numbers, and moving one moved nothing else. The claim is made
    // TRUE here rather than deleted, because the drift it described is a real
    // hazard: the WER arm and the EER arm exist to agree about what "the fix
    // works" means on the same measurement. The VALUE is unchanged, so this
    // arm's behaviour is unchanged — see
    // `meeting_eval_antialias_both_fold_gates_spend_one_constant` for the guard
    // that keeps a literal from creeping back in beside it.
    assert!(
        removed_db >= FOLD_REJECTION_DB,
        "the anti-aliased decimator must keep ≥{FOLD_REJECTION_DB:.0} dB more of the >8 kHz \
         band out of the speech band on real fixture audio, got {removed_db:.1} dB"
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
// YV120 — the diarization fixtures (e) and (f)
// ---------------------------------------------------------------------------
//
// The metrics themselves are unit-tested in `tests/diarization_metrics.rs`
// against hand-worked answers. What is checked HERE is the other half: that the
// two fixtures really carry the ground truth those metrics need, and that each
// is hard in the specific way it was built to be hard. A fixture nobody checked
// is worth exactly as much as a metric nobody checked — YV109's two-track
// fixture has `two_track_ordering_fixture_is_hard_by_construction` for the same
// reason, and it caught a fixture whose markers had drifted out of the seams.

/// The DER gate for fixture (e), MEASURED by YV126's sweep on 2026-08-16.
///
/// **This is a REGRESSION gate, not a quality claim, and the difference is the
/// whole point of the number.** `tune_clustering_threshold` swept 19 candidate
/// distances through the shipped `cluster_track` against the real sidecar and
/// the catalog's two models; the winner was distance 0.30 at DER 0.446441 / JER
/// 0.480151, recorded here rounded UP at the fourth decimal so the gate is a
/// ceiling the measurement clears rather than a value it sits exactly on. Nothing here says 44.6 % error is
/// good — it is not — it says that is what this pipeline scores on THIS corpus
/// today, and any change that scores worse has to explain itself.
///
/// **Why the number is so large is measured, not guessed, and it is mostly not
/// the threshold.** YV122 and YV124 both established it against this same
/// corpus: every voice in it is the Mac's `say` synthesiser, and CAM++ hears
/// the SYNTHESISER rather than the persona — an enrollment EER of 0.272 on
/// these fixtures against <1 % on VoxCeleb. YV124's own conclusion, that the
/// thresholds deciding whether two clips are the same person still have to be
/// set on real human speech, is unchanged by this measurement and is the reason
/// this gate may not be read as an accuracy figure for Yap.
///
/// The tuning table is in the PR and in the backlog's YV126 measurement record.
const ROOM_3_DER_GATE: Option<f64> = Some(0.4465);
const ROOM_3_JER_GATE: Option<f64> = Some(0.4802);
/// Fixture (f) scores BADLY under full N-way clustering — the segmentation model
/// cannot do the task (merged finding #5), and the fix is a smaller task, not a
/// better threshold. So the full-clustering number on this fixture is RECORDED
/// and never gated, and these two gate the `EnrolledVsEveryoneElse` mode, whose
/// own two-dimensional sweep produced them.
///
/// Same reading as fixture (e)'s: a regression floor measured on a synthetic
/// corpus whose own speaker-identity ceiling YV124 measured separately, not a
/// statement about what Yap achieves in a real lecture hall.
///
/// MEASURED 2026-08-16 by `tune_enrollment_band`'s two-dimensional sweep:
/// clustering distance 0.80 × acceptance band 0.75 → DER 0.348913 / JER
/// 0.475733, scored from 4.19 s (the enrollment span excluded), reproduced
/// end to end through `cluster_track` at the same pair. Recorded rounded UP at
/// the fourth decimal. Full N-way on the same fixture scored DER 0.6405 —
/// recorded, never gated, and the mechanism reason is in
/// `fixture_f_binary_fallback_der`'s arm 1.
const CLASSROOM_6_DER_GATE: Option<f64> = Some(0.3490);
const CLASSROOM_6_JER_GATE: Option<f64> = Some(0.4758);

/// The clustering distance fixture (e)'s sweep chose, recorded beside the gate
/// it produced.
///
/// **A gate without its provenance is a number somebody typed.** These record
/// WHICH point of the sweep the gates above came from, and
/// `fixture_e_der_gate` / `fixture_f_binary_fallback_der` fail on a machine that
/// can measure if the sweep's winner has moved away from them — so a gate can
/// never quietly start describing a different configuration than the one that
/// was measured. On a machine with no models they are inert, exactly like the
/// gates.
const ROOM_3_TUNED_DISTANCE: Option<f64> = Some(0.30);
/// Fixture (f)'s 2-class winner as `(clustering distance, acceptance band)`.
///
/// Two numbers because the 2-class task is tuned in two dimensions — the band
/// decides the label, the distance decides the turn the label lands on — and
/// the sweep is what proved the second dimension is not free. The 2-class task
/// wants distance **0.80**; fixture (e)'s full-clustering task wants **0.30**.
/// Running the 2-class arm at fixture (e)'s distance, which an earlier cut of
/// this item did on the stated ground that the distance "decides nothing" in
/// binary mode, scores its best at DER 0.5146 against 0.3489 here — 47 % worse,
/// measured on the same fixture in the same run.
const CLASSROOM_6_TUNED: Option<(f64, f64)> = Some((0.80, 0.75));

/// Fixture (e)'s speakers: three voices, three distances, near-field.
///
/// The voice is what makes each of these a different PERSON. The gain is only
/// where they sit — and gain is exactly the quantity an L2-normalised cosine
/// embedding throws away, so a roster that varied only the gain would carry no
/// separable identity at all. `generate_room_3` renders through
/// [`synthesize_with`] with the voice named here, and
/// [`assert_rttm_fits_the_audio`] re-measures the rendered audio rather than
/// trusting this table.
const ROOM_3_SPEAKERS: [(&str, &str, f32); 3] = [
    ("spk_a", "Samantha", 1.00),
    ("spk_b", "Fred", 0.92),
    ("spk_c", "Kathy", 0.85),
];

/// Fixture (f)'s speakers: six voices across a room, the instructor nearest the
/// mic and the back row a third of that. Distinct synthesizer voices are the
/// fixture's central claim — see [`FixtureSpeaker`].
///
/// The six voices are chosen for MEASURED fundamental-frequency spread, not for
/// having six different names: `say`'s "Junior" and "Kathy" both render at a
/// 216 Hz median on this machine and "Albert" at 232 Hz, so the original
/// roster's top three were one cluster wearing three names — six names is not
/// six speakers. Flo and Sandy replace Junior and Albert, chosen off a sweep of
/// all 43 installed English `say` voices (transcript in
/// `docs/pr-screenshots/YV120/f0-separation.txt`).
/// [`assert_rttm_fits_the_audio`] re-measures the spread on the rendered mix, so
/// this comment cannot drift away from the audio.
const CLASSROOM_6_SPEAKERS: [(&str, &str, f32); 6] = [
    ("spk_a", "Samantha", 0.55),
    ("spk_b", "Fred", 0.34),
    ("spk_c", "Kathy", 0.30),
    ("spk_d", "Ralph", 0.26),
    ("spk_e", "Flo (English (US))", 0.24),
    ("spk_f", "Sandy (English (US))", 0.22),
];

/// The RTTM turns of a fixture, in start order.
fn read_rttm(root: &Path, id: &str) -> Vec<RttmTurn> {
    let meta = read_meta(root, id);
    let mut turns = meta.rttm;
    assert!(
        !turns.is_empty(),
        "{id} carries no RTTM — regrow it with \
         `cargo test meeting_eval_generate_diarization_fixtures -- --ignored`"
    );
    turns.sort_by(|a, b| a.start_seconds.total_cmp(&b.start_seconds));
    turns
}

/// The distinct speaker ids in an RTTM, sorted.
fn rttm_speakers(turns: &[RttmTurn]) -> Vec<String> {
    let mut ids: Vec<String> = turns.iter().map(|t| t.speaker_id.clone()).collect();
    ids.sort();
    ids.dedup();
    ids
}

/// The greatest number of speakers audible at one instant.
///
/// `1` means the fixture has no overlap at all; pyannote-segmentation-3.0 caps
/// at `2` simultaneous, so a fixture built to exceed the mechanism ceiling has
/// to reach `3`.
fn max_simultaneous(turns: &[RttmTurn]) -> usize {
    let mut events: Vec<(f64, i32)> = Vec::with_capacity(turns.len() * 2);
    for t in turns {
        events.push((t.start_seconds, 1));
        events.push((t.end_seconds, -1));
    }
    // Ends before starts at the same instant: two turns that merely touch are
    // not overlapping.
    events.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut here = 0i32;
    let mut most = 0i32;
    for (_, delta) in events {
        here += delta;
        most = most.max(here);
    }
    most as usize
}

/// The greatest number of DISTINCT speakers inside any window of `window`
/// seconds — the quantity pyannote-segmentation-3.0's "3 speakers per 10 s
/// window" ceiling is expressed in.
///
/// An optimal window can always be slid so that its left edge sits on a turn
/// start or its right edge on a turn end, so those are the only candidates
/// worth evaluating.
fn max_speakers_in_window(turns: &[RttmTurn], window: f64) -> usize {
    let mut starts: Vec<f64> = turns.iter().map(|t| t.start_seconds).collect();
    starts.extend(turns.iter().map(|t| t.end_seconds - window));
    let mut most = 0usize;
    for start in starts {
        let end = start + window;
        let mut ids: Vec<&str> = turns
            .iter()
            .filter(|t| t.start_seconds < end && t.end_seconds > start)
            .map(|t| t.speaker_id.as_str())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        most = most.max(ids.len());
    }
    most
}

/// Total speech time per speaker.
fn speaker_seconds(turns: &[RttmTurn], speaker: &str) -> f64 {
    turns
        .iter()
        .filter(|t| t.speaker_id == speaker)
        .map(RttmTurn::duration)
        .sum()
}

/// The band the fundamental-frequency estimator will report in.
///
/// Wide enough for every voice in the two rosters as this machine renders them
/// (Ralph is the lowest at ~72 Hz, Sandy the highest at ~302 Hz) and narrow
/// enough that nothing outside a human fundamental can be reported as one.
const F0_MIN_HZ: f64 = 60.0;
const F0_MAX_HZ: f64 = 340.0;
/// A frame counts as voiced only if its normalized autocorrelation peak clears
/// this. Unvoiced speech and room tone have no periodicity to find, and letting
/// them vote would drag every speaker's median toward the same noise.
const F0_VOICED_AUTOCORRELATION: f32 = 0.30;
/// How far apart two speakers' medians have to sit before the audio is willing
/// to call them two people — as a RATIO, not a difference in Hz.
///
/// The unit matters and is not a stylistic choice. This estimator resolves F0
/// by picking an integer autocorrelation lag, so its resolution is `f^2 / rate`:
/// about 0.8 Hz at 80 Hz and 13 Hz at 320 Hz. A fixed Hz floor is therefore
/// simultaneously far too lax at the bottom of the band and nearly unsatisfiable
/// at the top, and would push the roster toward voices that differ in Hz rather
/// than voices that differ.
///
/// MEASURED, not chosen — the sweep and the per-fixture numbers are in
/// `docs/pr-screenshots/YV120/f0-separation.txt`. On the committed corpus:
/// fixture (e) is 103.9 / 170.2 / 216.2 Hz, closest pair 1.270x; fixture (f) is
/// 77.7 / 105.3 / 170.2 / 210.5 / 250.0 / 296.3 Hz, closest pair 1.185x
/// (spk_e/spk_f). 1.12 leaves the tightest shipped pair 35% of headroom above
/// the floor, and sits ~4x above the estimator's own resolution there (1.032x at
/// 250 Hz). It fails a roster that collapsed — the corpus this replaces measured
/// 170.2 / 170.2 / 170.2 Hz, i.e. 1.000x — and passes the one that ships.
const MIN_SPEAKER_F0_RATIO: f64 = 1.12;

/// Median fundamental frequency over the voiced frames of `samples`, or `None`
/// if nothing in it was voiced.
///
/// Plain time-domain autocorrelation, run at half the corpus rate — `F0_MAX_HZ`
/// is two decades below 8 kHz, and halving the rate quarters the work, which
/// keeps this an O(seconds) check rather than a reason not to run it. Frames
/// quieter than twice the fixture's own `noise_floor` are skipped, so the
/// far-field fixture's back row is measured on its speech and not on the HVAC
/// underneath it.
fn median_f0_hz(samples: &[f32], noise_floor: f64) -> Option<f64> {
    // 3-tap binomial lowpass before decimating by two: cheap, and enough to
    // keep the room tone's 236 Hz component from folding down into the band.
    let half: Vec<f32> = samples
        .windows(3)
        .step_by(2)
        .map(|w| 0.25 * w[0] + 0.5 * w[1] + 0.25 * w[2])
        .collect();
    let rate = TARGET_RATE as f64 / 2.0;
    let frame = (0.040 * rate) as usize;
    let hop = (0.020 * rate) as usize;
    let lo = (rate / F0_MAX_HZ) as usize;
    let hi = ((rate / F0_MIN_HZ) as usize).min(frame - 1);
    let gate = (noise_floor * 2.0).max(0.002) as f32;

    let mut voiced: Vec<f64> = Vec::new();
    let mut start = 0usize;
    while start + frame <= half.len() {
        let window = &half[start..start + frame];
        start += hop;
        let mean = window.iter().sum::<f32>() / frame as f32;
        let centred: Vec<f32> = window.iter().map(|s| s - mean).collect();
        let energy: f32 = centred.iter().map(|v| v * v).sum();
        if (energy / frame as f32).sqrt() < gate {
            continue;
        }
        let mut best_lag = 0usize;
        let mut best = 0.0f32;
        for lag in lo..=hi {
            let mut acc = 0.0f32;
            for i in 0..frame - lag {
                acc += centred[i] * centred[i + lag];
            }
            if acc > best {
                best = acc;
                best_lag = lag;
            }
        }
        if best_lag == 0 || best / energy < F0_VOICED_AUTOCORRELATION {
            continue;
        }
        voiced.push(rate / best_lag as f64);
    }
    if voiced.is_empty() {
        return None;
    }
    voiced.sort_by(f64::total_cmp);
    Some(voiced[voiced.len() / 2])
}

/// Every declared turn lies inside the audio, is long enough to embed, and the
/// declared duration is the wav's real one — checks that belong to both
/// fixtures.
fn assert_rttm_fits_the_audio(root: &Path, id: &str, turns: &[RttmTurn]) {
    let meta = read_meta(root, id);
    let (rate, samples) = read_wav_i16(&root.join(id).join("audio.wav"));
    assert_eq!(rate, TARGET_RATE, "{id}: the corpus is 16 kHz mono");
    let wav_seconds = samples.len() as f64 / TARGET_RATE as f64;
    assert!(
        (wav_seconds - meta.duration_seconds).abs() < 0.05,
        "{id}: meta says {:.2}s, the wav is {wav_seconds:.2}s",
        meta.duration_seconds
    );
    for t in turns {
        assert!(
            t.start_seconds >= 0.0 && t.end_seconds <= wav_seconds + 1e-6,
            "{id}: turn {t:?} falls outside the audio"
        );
        // CAM++ needs something to embed. A turn shorter than this is not a
        // speaker attribution problem, it is a VAD problem, and putting one in
        // the ground truth would charge the diarizer for the wrong thing.
        assert!(
            t.duration() >= 0.5,
            "{id}: turn {t:?} is too short to carry speaker identity"
        );
    }
    // The RTTM describes THIS audio, and the check for that is energy: every
    // declared turn has to be loud against the fixture's own room tone. A turn
    // placed past the end of the mix, or a script edit that left a start where
    // no speech landed, is silent — well-formed ground truth pointing at
    // nothing, and every DER measured against it would be measuring the
    // fixture's bug. Measured on the committed corpus: the quietest turn of
    // fixture (f) (the back row, far-field) runs 5.8x its silence floor and the
    // loudest 24.1x, and fixture (e) runs 120.4x to 281.6x, so a 4x gate fails a
    // placement bug long before it fails a quiet speaker.
    let rms = |from: f64, to: f64| -> f64 {
        let a = ((from * TARGET_RATE as f64) as usize).min(samples.len());
        let b = ((to * TARGET_RATE as f64) as usize).min(samples.len());
        if b <= a {
            return 0.0;
        }
        let sum: f64 = samples[a..b]
            .iter()
            .map(|s| {
                let v = *s as f64 / 32_768.0;
                v * v
            })
            .sum();
        (sum / (b - a) as f64).sqrt()
    };
    let mut speech = vec![false; samples.len()];
    for t in turns {
        let a = ((t.start_seconds * TARGET_RATE as f64) as usize).min(samples.len());
        let b = ((t.end_seconds * TARGET_RATE as f64) as usize).min(samples.len());
        speech[a..b].iter_mut().for_each(|s| *s = true);
    }
    let quiet: Vec<i16> = samples
        .iter()
        .zip(speech.iter())
        .filter(|(_, on)| !**on)
        .map(|(s, _)| *s)
        .collect();
    let floor = {
        let sum: f64 = quiet
            .iter()
            .map(|s| {
                let v = *s as f64 / 32_768.0;
                v * v
            })
            .sum();
        (sum / quiet.len().max(1) as f64).sqrt()
    };
    assert!(
        floor > 0.0,
        "{id}: digital silence between turns is not a room"
    );
    for t in turns {
        let level = rms(t.start_seconds, t.end_seconds);
        assert!(
            level > floor * 4.0,
            "{id}: turn {t:?} carries {level:.5} RMS against a {floor:.5} floor — \
             the ground truth points at silence"
        );
    }
    let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
    assert!(
        peak < 32_400,
        "{id}: the mix clips at {peak} — a clipped fixture measures the mixer"
    );

    let speakers = rttm_speakers(turns);
    assert_eq!(
        speakers.len(),
        meta.speakers.len(),
        "{id}: the RTTM and the speaker table disagree on how many people are in the room"
    );
    let mut voices: Vec<&str> = meta.speakers.iter().map(|s| s.voice.as_str()).collect();
    voices.sort_unstable();
    voices.dedup();
    assert_eq!(
        voices.len(),
        meta.speakers.len(),
        "{id}: two speakers share a synthesizer voice — the fixture would be \
         scoring one person against themselves"
    );

    // …and the same claim again, measured off the AUDIO rather than off the
    // metadata. The check above reads a string the generator wrote from the same
    // table it was supposed to render with: it passes just as happily on a
    // corpus where all six speakers are literally one voice, which is precisely
    // the failure its own message names. So take each speaker's own turns,
    // concatenate them, and estimate a fundamental — a per-speaker descriptor
    // that is a property of the samples and of nothing else.
    //
    // F0 is deliberately a WEAK proxy for speaker identity: it cannot prove two
    // voices embed apart, and it is not trying to. It proves the one thing that
    // was actually wrong, at a cost of a couple of seconds and zero models — the
    // rendered audio carries per-speaker variation that survives the L2
    // normalisation a cosine embedding ends with, instead of only the scalar
    // gain, which does not.
    //
    // Only the SOLO part of each turn is measured. Fixture (f) overlaps on
    // purpose, and a frame where two people are talking belongs to neither of
    // them as a descriptor — including it makes a speaker's number depend on who
    // happened to interrupt them, which is how a "measurement" starts moving
    // when a neighbouring speaker is swapped.
    let mut covering = vec![0u16; samples.len()];
    for t in turns {
        let a = ((t.start_seconds * TARGET_RATE as f64) as usize).min(samples.len());
        let b = ((t.end_seconds * TARGET_RATE as f64) as usize).min(samples.len());
        covering[a..b].iter_mut().for_each(|c| *c += 1);
    }
    let mut f0: Vec<(String, f64)> = Vec::new();
    for speaker in &speakers {
        let mut solo: Vec<f32> = Vec::new();
        for t in turns.iter().filter(|t| &t.speaker_id == speaker) {
            let a = ((t.start_seconds * TARGET_RATE as f64) as usize).min(samples.len());
            let b = ((t.end_seconds * TARGET_RATE as f64) as usize).min(samples.len());
            solo.extend(
                samples[a..b]
                    .iter()
                    .zip(covering[a..b].iter())
                    .filter(|(_, c)| **c == 1)
                    .map(|(s, _)| *s as f32 / 32_768.0),
            );
        }
        assert!(
            solo.len() > TARGET_RATE as usize,
            "{id}: {speaker} has under a second of un-overlapped speech — \
             there is nothing to take a descriptor from"
        );
        let hz = median_f0_hz(&solo, floor)
            .unwrap_or_else(|| panic!("{id}: {speaker}'s turns contain no voiced speech at all"));
        f0.push((speaker.clone(), hz));
    }
    eprintln!(
        "{id}: measured median F0 — {}",
        f0.iter()
            .map(|(s, hz)| format!("{s} {hz:.1} Hz"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    for (i, (a, a_hz)) in f0.iter().enumerate() {
        for (b, b_hz) in &f0[i + 1..] {
            let ratio = a_hz.max(*b_hz) / a_hz.min(*b_hz);
            assert!(
                ratio >= MIN_SPEAKER_F0_RATIO,
                "{id}: {a} ({a_hz:.1} Hz) and {b} ({b_hz:.1} Hz) are the same \
                 voice as far as the audio is concerned — {ratio:.3}x apart, \
                 under the {MIN_SPEAKER_F0_RATIO:.3}x floor. meta.json can name \
                 them differently and it changes nothing: a fixture whose \
                 speakers differ only by gain has no identity for an embedding \
                 to separate — gain is exactly what L2 normalisation removes — \
                 and every distance measured on it would be noise that looks \
                 like a measurement. Fix the roster, then regrow with \
                 `cargo test meeting_eval_generate_diarization_fixtures -- --ignored`."
            );
        }
    }
}

/// No number is gated without its provenance.
///
/// **This test changed shape when the measurement arrived, and the shape is the
/// point.** It used to assert every gate was `None`, so that a placeholder could
/// not be quietly promoted: editing it meant writing down where a number came
/// from. YV126's sweep is that writing-down — 19 candidate distances for
/// fixture (e) and 19 × 19 for fixture (f), through the shipped `cluster_track`
/// against the real sidecar and the catalog's models — so the gates now carry
/// numbers, and what has to stay impossible is a gate with no measured
/// configuration behind it.
///
/// Each gate is therefore paired with the sweep point that produced it
/// ([`ROOM_3_TUNED_DISTANCE`], [`CLASSROOM_6_TUNED`]), and the arms that CAN
/// measure — `fixture_e_der_gate`, `fixture_f_binary_fallback_der` — fail if
/// the sweep's winner has drifted away from the recorded point. This half runs
/// everywhere, including a CI machine with no corpus and no models: it is the
/// structural claim (a gate and its provenance are set or unset TOGETHER) and
/// needs no audio to check.
#[test]
fn meeting_eval_diarization_gates_carry_the_configuration_that_produced_them() {
    for (gate_name, gate, provenance_name, provenance) in [
        (
            "ROOM_3_DER_GATE",
            ROOM_3_DER_GATE,
            "ROOM_3_TUNED_DISTANCE",
            ROOM_3_TUNED_DISTANCE.is_some(),
        ),
        (
            "ROOM_3_JER_GATE",
            ROOM_3_JER_GATE,
            "ROOM_3_TUNED_DISTANCE",
            ROOM_3_TUNED_DISTANCE.is_some(),
        ),
        (
            "CLASSROOM_6_DER_GATE",
            CLASSROOM_6_DER_GATE,
            "CLASSROOM_6_TUNED",
            CLASSROOM_6_TUNED.is_some(),
        ),
        (
            "CLASSROOM_6_JER_GATE",
            CLASSROOM_6_JER_GATE,
            "CLASSROOM_6_TUNED",
            CLASSROOM_6_TUNED.is_some(),
        ),
    ] {
        assert_eq!(
            gate.is_some(),
            provenance,
            "{gate_name} is {gate:?} but {provenance_name} says the sweep that \
             produces it is {}. A gate without the configuration it was measured \
             at is a number somebody typed, and a recorded configuration with no \
             gate is a measurement nobody is holding the code to.",
            if provenance { "recorded" } else { "absent" }
        );
        if let Some(value) = gate {
            assert!(
                (0.0..=1.0).contains(&value),
                "{gate_name} is {value}, which is not a rate"
            );
        }
    }

    // The 2-class fixture is tuned in two dimensions, and both are real: a
    // recorded pair whose two halves were equal would mean somebody copied one
    // task's number into the other's slot, which is the exact substitution the
    // second sweep dimension exists to prevent.
    if let (Some(distance), Some((binary_distance, band))) =
        (ROOM_3_TUNED_DISTANCE, CLASSROOM_6_TUNED)
    {
        assert!(
            (0.0..=1.0).contains(&binary_distance) && (0.0..=1.0).contains(&band),
            "fixture (f)'s tuned pair ({binary_distance}, {band}) is not two \
             cosine-unit numbers"
        );
        assert!(
            (distance - binary_distance).abs() > f64::EPSILON,
            "fixture (f)'s 2-class distance ({binary_distance}) is fixture (e)'s \
             full-clustering distance ({distance}). That may be a genuine \
             measurement, and if it is, delete this assertion and say so in the \
             backlog — but it is also exactly what an inherited number looks \
             like, and inheriting it is the defect the two-dimensional sweep was \
             added to fix."
        );
    }
}

/// Fixture (e) is the tuning fixture: three people, near-field, NO overlap.
///
/// Every property asserted here is one YV126's threshold tune depends on. If the
/// fixture drifted into overlap, a clustering threshold tuned on it would be
/// compensating for the mechanism ceiling instead of measuring similarity — and
/// the number would look fine.
#[test]
fn meeting_eval_room_3_is_the_clean_near_field_case_it_claims_to_be() {
    let Some(root) = corpus() else { return };
    let turns = read_rttm(&root, ROOM_3);
    assert_rttm_fits_the_audio(&root, ROOM_3, &turns);

    assert_eq!(rttm_speakers(&turns).len(), 3, "three people in the room");
    assert_eq!(
        max_simultaneous(&turns),
        1,
        "fixture (e) is the NO-OVERLAP case: {turns:#?}"
    );
    assert!(
        max_speakers_in_window(&turns, 10.0) <= 3,
        "fixture (e) must stay inside pyannote-segmentation-3.0's 3-per-10s \
         ceiling — that is what makes it the fixture a threshold can be tuned on"
    );
    let changes = turns
        .windows(2)
        .filter(|w| w[0].speaker_id != w[1].speaker_id)
        .count();
    assert!(changes >= 3, "only {changes} speaker changes");
    eprintln!(
        "fixture (e): {} turns, {} speaker changes, {:.1}s",
        turns.len(),
        changes,
        turns.last().map(|t| t.end_seconds).unwrap_or(0.0)
    );
}

/// Fixture (f) is the fixture built to FAIL, on purpose.
///
/// Merged finding #5: pyannote-segmentation-3.0 caps at 3 speakers per 10 s
/// window and 2 simultaneous, and sherpa's pipeline deletes every overlapped
/// frame before embedding. A six-person classroom exceeds that, so full N-way
/// clustering is expected to produce visibly bad DER here — not because the
/// threshold is wrong but because the mechanism cannot do the task. That
/// argument is only falsifiable against a fixture that really does exceed the
/// ceiling, so this test measures the excess rather than asserting it in prose.
#[test]
fn meeting_eval_classroom_6_exceeds_the_segmentation_ceiling_on_purpose() {
    let Some(root) = corpus() else { return };
    let turns = read_rttm(&root, CLASSROOM_6);
    assert_rttm_fits_the_audio(&root, CLASSROOM_6, &turns);

    assert_eq!(rttm_speakers(&turns).len(), 6, "six people in the room");
    let simultaneous = max_simultaneous(&turns);
    assert!(
        simultaneous >= 3,
        "fixture (f) must exceed the 2-simultaneous ceiling; peak is {simultaneous}"
    );
    let in_ten = max_speakers_in_window(&turns, 10.0);
    assert!(
        in_ten >= 4,
        "fixture (f) must exceed the 3-speakers-per-10s ceiling; peak is {in_ten}"
    );
    let overlapped: f64 = {
        // Seconds during which more than one person is speaking.
        let mut events: Vec<(f64, i32)> = Vec::new();
        for t in &turns {
            events.push((t.start_seconds, 1));
            events.push((t.end_seconds, -1));
        }
        events.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut here = 0i32;
        let mut last = 0.0f64;
        let mut total = 0.0f64;
        for (at, delta) in events {
            if here > 1 {
                total += at - last;
            }
            here += delta;
            last = at;
        }
        total
    };
    assert!(
        overlapped > 3.0,
        "only {overlapped:.2}s of crosstalk — sherpa drops overlapped frames \
         before embedding, so a fixture with a token amount of overlap proves \
         nothing about what that costs"
    );
    eprintln!(
        "fixture (f): {} turns, {simultaneous} simultaneous at peak, \
         {in_ten} distinct speakers in one 10s window, {overlapped:.1}s overlapped",
        turns.len()
    );
}

/// The metrics, pointed at the real fixtures.
///
/// Two arms, both with an answer that is known without a diarizer:
///
/// * The ground truth scored against ITSELF is 0.0 error. A metric that could
///   not return zero for a perfect hypothesis would fail every later comparison
///   in a direction nobody would question.
/// * A "perfect VAD, one cluster" hypothesis — every reference turn kept, all
///   relabeled to one speaker — is the exact baseline a diarizer has to beat.
///   On fixture (e), where there is no overlap, its DER is derivable on paper:
///   nothing is missed and nothing is invented, so the error is precisely the
///   speech belonging to the two speakers that were not the mapped one.
///
/// The second arm is what proves the harness is scoring the FIXTURE and not a
/// hand-built interval list: its expected value is computed from the fixture's
/// own RTTM at test time.
#[test]
fn meeting_eval_diarization_metrics_score_the_real_fixtures() {
    let Some(root) = corpus() else { return };
    for id in [ROOM_3, CLASSROOM_6] {
        let turns = read_rttm(&root, id);
        let perfect = der(&turns, &turns);
        assert!(
            perfect.rate().abs() < 1e-9,
            "{id}: the ground truth scored against itself is not zero: {perfect:?}"
        );
        assert!(jer(&turns, &turns).abs() < 1e-9, "{id}: JER against itself");

        let one_cluster: Vec<RttmTurn> = turns
            .iter()
            .map(|t| RttmTurn::new("cluster_0", t.start_seconds, t.end_seconds))
            .collect();
        let report = der(&turns, &one_cluster);
        eprintln!(
            "{id}: one-cluster baseline DER {:.4} JER {:.4}",
            report.rate(),
            jer(&turns, &one_cluster)
        );
        assert!(
            report.rate() > 0.3,
            "{id}: a single cluster cannot be a good hypothesis for this fixture"
        );
    }

    // Fixture (e) has no overlap, so the one-cluster baseline's DER is exactly
    // "everything that is not the busiest speaker".
    let turns = read_rttm(&root, ROOM_3);
    let total: f64 = turns.iter().map(RttmTurn::duration).sum();
    let busiest = rttm_speakers(&turns)
        .iter()
        .map(|s| speaker_seconds(&turns, s))
        .fold(0.0f64, f64::max);
    let one_cluster: Vec<RttmTurn> = turns
        .iter()
        .map(|t| RttmTurn::new("cluster_0", t.start_seconds, t.end_seconds))
        .collect();
    let report = der(&turns, &one_cluster);
    assert!(report.miss.abs() < 1e-9 && report.false_alarm.abs() < 1e-9);
    assert!(
        (report.confusion - (total - busiest)).abs() < 1e-6,
        "confusion {:.4} is not the {:.4}s of speech outside the busiest speaker",
        report.confusion,
        total - busiest
    );
    assert!(
        (report.rate() - (total - busiest) / total).abs() < 1e-9,
        "DER {:.6} is not the hand-derivable {:.6}",
        report.rate(),
        (total - busiest) / total
    );
}

// ---------------------------------------------------------------------------
// YV124 — the anti-alias EER arm: plan finding OS-8's deferred half
// ---------------------------------------------------------------------------
//
// OS-8 asked for two arms on the same fixture, not one: "run one fixture
// through both resamplers and compare CAM++ EER **and** WER… before the
// enrollment thresholds are tuned, or those thresholds permanently encode the
// aliasing." YV92/YV93 shipped the WER half — it is
// `meeting_eval_antialias_decimation_does_not_regress_wer_on_broadband_noise`,
// forty lines above, and this item changes exactly one line in it (a bare
// `20.0` becoming the constant the two arms are documented as sharing). The EER
// half could not ship then because there was no embedding extractor in the
// repo. YV122 put one there, and this is the measurement it unblocked.
//
// **The headline is not the direction of a difference, it is that the
// experiment needed a control before it could be read at all.** The two noisy
// arms disagree between fixtures; the control arms are what make the
// disagreement resolvable, and what turned "did the score get worse" into a
// criterion an anti-alias filter can actually be held to. Read
// [`shipped_arm_tracks_the_control`] before changing anything in this section.
//
// ## Which fixture, and why it is not fixture (c)
//
// The WER arm scores fixture (c) `device-change`, which is the right fixture
// for a WER: one voice, native 48 kHz, a known reference transcript. An EER is
// not a WER. It needs a labeled genuine/impostor distribution, which needs at
// least two PEOPLE, and fixture (c) has exactly one (`meta.json` carries no
// `speakers` and no `rttm`). The corpus's two multi-speaker fixtures are
// (e) `room-3-near-field` — three distinct `say` voices, four turns each, every
// turn 3.2–4.4 s — and (f) `classroom-6-far-field` — six voices at
// `direct_gain` 0.22–0.55, 21 turns, the far-field case OS-8's wording actually
// names. [`ARM_FIXTURES`] scores BOTH. Each takes its FOLD-BAND CONTENT from
// the same `with_ultrasonic_noise` the WER arm uses, which is the "far-field
// room tone: HVAC/fan/keyboard energy above 8 kHz" the finding names.
//
// ## Getting fixture (e) to a native rate honestly
//
// The corpus stores fixture (e) at the pipeline's 16 kHz target, so the arm
// has to put it back at a capture rate before it can decimate it two ways.
// That is done with the app's own upsampler (`resample_linear` 16→48 kHz, the
// direction `resample.rs` documents as unable to fold) followed by the SHIPPED
// anti-alias cascade — `LowPassCascade::for_decimation(48k, 16k)`, the exact
// 8th-order Butterworth the app installs — run once over the upsampled signal
// to remove the interpolator's images. After that step the ONLY energy above
// 8 kHz in the arm's input is the fold-band noise the arm adds on purpose,
// which is the experiment OS-8 describes and not a resampling artifact wearing
// its clothes.
//
// ## What is measured with no model on the machine, and what is not
//
// The fold measurement below is real on any machine with the corpus and needs
// no model: each arm's decimated output is compared against THAT SAME ARM's
// decimation of the clean signal, so the number is the energy the noise put
// into 0–8 kHz and the two arms' filter phase never enters it. The WER arm
// measures the fold the same way and against the same bar — and since a review
// finding, against the same CONSTANT rather than against an equal literal; see
// [`meeting_eval_antialias_both_fold_gates_spend_one_constant`].
//
// The EER itself needs embeddings, and embeddings need an inference backend in
// `yap-diarize` AND the two catalog models on disk. YV122 shipped the backend,
// so the remaining precondition is the model files — one state, not two, and
// `support::diarize::embedder()` panics on every failure that is NOT that one
// honest state, so a broken sidecar can never present here as a green arm. A
// machine that has neither corpus nor models runs everything in this section
// except the arm; a machine that has both runs the arm and must not skip.
//
// ## FOUR arms per fixture, because two could not answer the question
//
// The two noisy arms (pre-fix and shipped decimation of audio with the >8 kHz
// comb added) are the experiment. The two CONTROL arms are the same utterances
// decimated each way with nothing added, and they exist because the two noisy
// arms gave opposite verdicts on the corpus's two multi-speaker fixtures — the
// shipped decimator better on (e), the pre-fix one better on every statistic on
// (f). With nothing above 8 kHz there is nothing to fold, so the two control
// arms must agree; measured, they do to the digit. Against that reference (f)
// resolves: the pre-fix arm moved away from the control in the FLATTERING
// direction, inflating same-speaker similarity while leaving different-speaker
// similarity where it was, because the folded comb is identical in every
// utterance and two clips that both carry it look alike. The full argument, the
// numbers and the criterion it forced are on [`shipped_arm_tracks_the_control`].
//
// ## The skip expired, and this is what the expiry produced
//
// The first shipped version of this arm returned quietly on `NoBackend` and on
// `ModelsMissing`, which made it a permanently self-skipping gate: it would
// have stayed green forever without ever computing an EER, including after
// YV122 landed. Nothing mechanical enforced OS-8's ordering requirement; it
// rested on a human remembering to come back.
//
// The first fix turned that skip into a DECLARATION — a machine with no
// embedder has to name its reason in [`EER_UNMEASURED_OK`] or this arm PANICS —
// and then claimed CI was held to it. **CI was not, and could not be.** This
// test opens on `corpus()`, and the corpus is `say`-generated audio under
// `~/yap-eval-corpus/meetings` that no CI runner has and no repo carries. On a
// corpus-less machine the arm returns at its first line: before the fold
// assert, before `embedder()`, and before the declaration is ever consulted.
// The models were never the precondition — the CORPUS is, and installing the
// two catalog models in CI would have changed nothing on its own. That claim is
// deleted rather than softened, and `ci.yml` no longer exports a variable
// nothing there reads (see
// [`meeting_eval_anti_alias_eer_ci_does_not_declare_what_it_never_reaches`],
// which keeps the dead configuration from coming back).
//
// The second fix added a gate that needed neither: a probe that asked the
// shipped sidecar whether this build had an inference backend at all, and
// required the answer to still be `no_backend` — the one condition under which
// `EER: UNMEASURED` was an honest thing to write down. **That gate has now
// done its job.** YV122 merged, the sidecar gained sherpa-onnx, the probe went
// red on the rebase, and the EER below is the number that was owed. What
// remains of it is the inverse guard,
// [`meeting_eval_anti_alias_eer_measurement_stays_backed_by_a_real_backend`]:
// the measured numbers are only worth the backend that produced them, so the
// same probe now requires a real backend to still be there.
//
// One gate stayed exactly as it was, and it is the one that still has work to
// do: a machine with no models installed cannot measure this, and it must NAME
// that reason in [`EER_UNMEASURED_OK`] rather than skip quietly. The
// declaration carries the REASON (`models_missing`) rather than a bare `1`, so
// an `export` pasted into a shell profile and forgotten expires by itself — the
// stale `no_backend` declarations this item's own transcripts contain stopped
// counting the day YV122 landed, because no machine can have that reason any
// more and the tag no longer exists in `diarize_protocol.rs` to name.
//
// ## Two fixtures, and a statistic that does not saturate — review fix #2
//
// An EER over 18 genuine pairs moves in steps of 1/18 and is floored at zero.
// Three clearly distinct `say` voices will very likely score 0.0000 on BOTH
// arms, and `assert!(shipped.eer <= pre_fix.eer)` would then pass as 0 ≤ 0
// while measuring nothing — a saturated result recorded as OS-8's answer. Two
// changes fix that, and neither is a threshold anyone tuned:
//
//  1. **Separation statistics beside the EER.** [`ArmScore`] carries the
//     genuine/impostor MARGIN (mean genuine similarity minus mean impostor
//     similarity, in `CosineSimilarity`), d′ and ROC-AUC. The margin and d′ are
//     continuous — they move for a difference far below one EER quantum — and
//     the arm asserts on them as well as on the EER. The per-arm quantisation
//     (1/n_genuine, 1/n_impostor and the resulting EER step) is PRINTED, so a
//     reader of the transcript can see how much resolution the EER had.
//  2. **Fixture (f) as a second scored fixture.** OS-8 names far-field
//     classroom room tone; fixture (e) is a three-voice near-field room. The
//     corpus already carries fixture (f) `classroom-6-far-field`: six voices,
//     `direct_gain` 0.22–0.55, 21 turns. Its overlap is what usually rules it
//     out — but this arm slices embeddings out of the GROUND-TRUTH RTTM rather
//     than out of a segmentation pass, so the mechanism ceiling YV126 hits does
//     not apply here. [`unoverlapped_spans`] trims each turn down to its
//     longest stretch with nobody else talking and drops what is left too
//     short, which is exactly what sherpa's own pipeline does to overlapped
//     frames (deletes them) — with the difference that here it is done from
//     labels, so it cannot be wrong.

/// The native capture rate the two decimators are compared at — the rate OS-8's
/// 3:1 analysis is about, and the rate fixture (c)'s native half was rendered
/// at.
const ARM_NATIVE_RATE: u32 = 48_000;

/// How much more of the >8 kHz band the shipped decimator must keep out of the
/// speech band than the pre-fix one, in dB. The same bar the WER arm holds on
/// the same measurement, so the two arms cannot drift apart on what "the fix
/// works" means.
const FOLD_REJECTION_DB: f32 = 20.0;

/// The fixtures this arm scores, in the order it scores them. Both of the
/// corpus's multi-speaker fixtures — see the section header for why (f) is
/// admissible here and not in YV126.
const ARM_FIXTURES: [&str; 2] = [ROOM_3, CLASSROOM_6];

/// The shortest stretch of one-person speech this arm will embed, in seconds.
///
/// **Not an accuracy threshold and not tuned as one.** It is a
/// fixture-construction floor whose only job is to keep the leftovers of an
/// overlap trim — fragments of a few hundred milliseconds — from entering the
/// distribution as if they were utterances. It sits below every untrimmed turn
/// in either fixture (fixture (e)'s shortest is 3.2 s; fixture (f)'s is 2.39 s),
/// so on audio with no overlap it excludes nothing at all, and the arm PRINTS
/// how many spans survived on each fixture so the constant's effect is visible
/// in the transcript rather than hidden in a header.
///
/// **It is spent a second time, on purpose, as the `min_embed` floor YV122 made
/// mandatory** — and that is one number doing one job, not two. YV122 shipped
/// `DiarizePool::embed` with no default floor anywhere in either crate, so
/// every caller has to name one; naming the value this arm already trims to
/// makes the call an ASSERTION that the trim did its job, because a span that
/// somehow arrived under it comes back as `audio_too_short` and the arm panics
/// with the fixture and index that produced it. It still sets no accuracy
/// threshold: nothing is scored against it, and how much audio is really enough
/// is a question only real human speech can answer — see YV122's
/// truncation-stability sweep, which this corpus cannot resolve.
const ARM_MIN_UTTERANCE_SECONDS: f64 = 2.0;

/// One enrollment utterance: who said it, and the fixture's own samples for the
/// RTTM span that bounds it.
struct ArmUtterance {
    speaker: String,
    /// 16 kHz, as the corpus stores it.
    samples: Vec<f32>,
}

/// Each turn trimmed down to its longest stretch with **nobody else talking**,
/// dropping whatever is left shorter than `min_seconds`.
///
/// Fixture (f) is built to overlap — that is its whole point for YV126 — and an
/// embedding sliced across two simultaneous voices belongs to neither of them.
/// Since the slicing here is done from the ground-truth RTTM rather than from a
/// segmentation pass, the overlapped frames can simply be removed by label,
/// which is the same thing sherpa's pipeline does to them acoustically.
///
/// Returns `(speaker, start_seconds, end_seconds)` in the input's order.
fn unoverlapped_spans(turns: &[RttmTurn], min_seconds: f64) -> Vec<(String, f64, f64)> {
    let mut kept = Vec::new();
    for (i, turn) in turns.iter().enumerate() {
        // Every other turn, clipped to this one's span.
        // Blocked by ANOTHER PERSON, not by another ROW. A review advisory:
        // this filtered on turn index alone, so a speaker with two adjacent
        // turns of their own would have been treated as contaminating himself.
        // That direction is safe — it discards clean audio rather than
        // admitting dirty audio — but it is still wrong about what the mask is
        // for, and it is the kind of wrong that a later fixture with
        // back-to-back turns would silently make expensive.
        // `meeting_eval_anti_alias_eer_a_speaker_does_not_contaminate_himself`
        // pins the corrected behaviour; on the two corpus fixtures scored here
        // the two definitions agree exactly, because neither RTTM has a
        // speaker overlapping himself, so no number in this item moved.
        let mut blocked: Vec<(f64, f64)> = turns
            .iter()
            .enumerate()
            .filter(|(j, other)| *j != i && other.speaker_id != turn.speaker_id)
            .map(|(_, other)| {
                (
                    other.start_seconds.max(turn.start_seconds),
                    other.end_seconds.min(turn.end_seconds),
                )
            })
            .filter(|(a, b)| b > a)
            .collect();
        blocked.sort_by(|a, b| a.0.total_cmp(&b.0));

        // Sweep the free runs between them and keep the longest.
        let mut best = (turn.start_seconds, turn.start_seconds);
        let mut cursor = turn.start_seconds;
        let consider = |from: f64, to: f64, best: &mut (f64, f64)| {
            if to - from > best.1 - best.0 {
                *best = (from, to);
            }
        };
        for (start, end) in blocked {
            if start > cursor {
                consider(cursor, start, &mut best);
            }
            cursor = cursor.max(end);
        }
        if turn.end_seconds > cursor {
            consider(cursor, turn.end_seconds, &mut best);
        }

        if best.1 - best.0 >= min_seconds {
            kept.push((turn.speaker_id.clone(), best.0, best.1));
        }
    }
    kept
}

/// A fixture's turns, sliced out of its audio. Exact by construction: the
/// fixture was ASSEMBLED from these turns, so a slice is the utterance and not
/// an approximation of one.
fn arm_utterances(root: &Path, id: &str) -> Vec<ArmUtterance> {
    let (rate, samples) = read_wav_i16(&root.join(id).join("audio.wav"));
    assert_eq!(
        rate, TARGET_RATE,
        "{id} is stored at the pipeline's target rate"
    );
    unoverlapped_spans(&read_rttm(root, id), ARM_MIN_UTTERANCE_SECONDS)
        .into_iter()
        .map(|(speaker, from, to)| {
            let start = (from * rate as f64) as usize;
            let end = ((to * rate as f64) as usize).min(samples.len());
            assert!(end > start, "empty span {speaker} {from}..{to} in {id}");
            ArmUtterance {
                speaker,
                samples: samples[start..end]
                    .iter()
                    .map(|&s| s as f32 / i16::MAX as f32)
                    .collect(),
            }
        })
        .collect()
}

/// Put a 16 kHz utterance back at [`ARM_NATIVE_RATE`] with nothing above 8 kHz
/// in it. See the section header for why the image-removal pass matters.
fn at_native_rate(utterance: &[f32]) -> Vec<f32> {
    let mut up =
        wilson_voice_lib::resample::resample_linear(utterance, TARGET_RATE, ARM_NATIVE_RATE);
    let mut images =
        wilson_voice_lib::resample::LowPassCascade::for_decimation(ARM_NATIVE_RATE, TARGET_RATE)
            .expect("48 kHz → 16 kHz decimates, so the shipped cascade exists");
    images.process(&mut up);
    up
}

/// The two decimations of one utterance, plus the clean control each is scored
/// against.
struct DecimatedPair {
    speaker: String,
    /// Pre-fix: `resample_linear`, no lowpass ahead of a 3:1 reduction.
    linear: Vec<f32>,
    /// Shipped: `resample_decimate`, the 8th-order Butterworth then the
    /// interpolator.
    shipped: Vec<f32>,
    /// The same two decimations of the SAME utterance without the fold-band
    /// noise — the reference each arm's folded energy is measured against, so
    /// the measurement never crosses the two filters' phase responses.
    linear_clean: Vec<f32>,
    shipped_clean: Vec<f32>,
}

fn decimate_both_ways(utterances: &[ArmUtterance]) -> Vec<DecimatedPair> {
    utterances
        .iter()
        .map(|u| {
            let clean = at_native_rate(&u.samples);
            let noisy = with_ultrasonic_noise(&clean, ARM_NATIVE_RATE);
            DecimatedPair {
                speaker: u.speaker.clone(),
                linear: wilson_voice_lib::resample::resample_linear(
                    &noisy,
                    ARM_NATIVE_RATE,
                    TARGET_RATE,
                ),
                shipped: wilson_voice_lib::resample::resample_decimate(
                    &noisy,
                    ARM_NATIVE_RATE,
                    TARGET_RATE,
                ),
                linear_clean: wilson_voice_lib::resample::resample_linear(
                    &clean,
                    ARM_NATIVE_RATE,
                    TARGET_RATE,
                ),
                shipped_clean: wilson_voice_lib::resample::resample_decimate(
                    &clean,
                    ARM_NATIVE_RATE,
                    TARGET_RATE,
                ),
            }
        })
        .collect()
}

/// Every pair of embeddings, split by whether the two utterances are the same
/// person. This is the labeled distribution `enrollment_eer` needs, and the
/// label comes from the fixture's RTTM rather than from anything the pipeline
/// decided — the ground truth cannot be contaminated by the thing being scored.
fn genuine_and_impostor(
    embeddings: &[(String, Vec<f32>)],
) -> (Vec<CosineSimilarity>, Vec<CosineSimilarity>) {
    let mut genuine = Vec::new();
    let mut impostor = Vec::new();
    for (i, (a_speaker, a)) in embeddings.iter().enumerate() {
        for (b_speaker, b) in embeddings.iter().skip(i + 1) {
            let score = cosine_similarity(a, b);
            if a_speaker == b_speaker {
                genuine.push(score);
            } else {
                impostor.push(score);
            }
        }
    }
    (genuine, impostor)
}

/// The EER of one arm's embeddings.
fn arm_eer(embeddings: &[(String, Vec<f32>)]) -> EerReport {
    let (genuine, impostor) = genuine_and_impostor(embeddings);
    enrollment_eer(&genuine, &impostor)
}

/// One arm's separation, in four numbers instead of one.
///
/// **Why more than the EER.** An EER computed over `g` genuine and `i` impostor
/// pairs cannot take a value between `0` and `1/(2g)`: FRR moves in steps of
/// `1/g`, FAR in steps of `1/i`, and the reported EER is their mean. On fixture
/// (e) that is 18 genuine pairs, so the EER's floor step is 2.8 points and its
/// value on well-separated voices is `0.0000` — at which point "the shipped arm
/// is not worse" is `0 <= 0` and carries no information. [`margin`] and
/// [`d_prime`] are continuous: they move for a difference orders of magnitude
/// below one EER quantum, so a real degradation cannot hide under the floor.
///
/// [`margin`]: ArmScore::margin
/// [`d_prime`]: ArmScore::d_prime
#[derive(Debug, Clone, Copy)]
struct ArmScore {
    eer: EerReport,
    mean_genuine: f64,
    mean_impostor: f64,
    /// `mean(genuine) - mean(impostor)`, in `CosineSimilarity`. Larger is a
    /// cleaner separation. Never saturates.
    margin: f64,
    /// The margin in pooled standard deviations. Larger is better; scale-free,
    /// so it is comparable between fixtures where the raw margin is not.
    d_prime: f64,
    /// `P(genuine score > impostor score)`, ties at a half. Saturates at 1.0 —
    /// printed for context, never asserted on alone.
    auc: f64,
    /// The smallest change in FRR one genuine pair can make: `1/n_genuine`.
    frr_step: f64,
    /// The smallest change in FAR one impostor pair can make: `1/n_impostor`.
    far_step: f64,
}

impl ArmScore {
    /// The smallest non-zero EER this distribution can express.
    fn eer_step(&self) -> f64 {
        self.frr_step.min(self.far_step) / 2.0
    }

    /// The EER is pinned to its floor and has no room to report a difference.
    fn eer_is_saturated(&self) -> bool {
        self.eer.eer < self.eer_step()
    }
}

/// Everything [`ArmScore`] holds, over one arm's embeddings.
fn arm_score(embeddings: &[(String, Vec<f32>)]) -> ArmScore {
    let (genuine_s, impostor_s) = genuine_and_impostor(embeddings);
    let eer = enrollment_eer(&genuine_s, &impostor_s);
    let genuine: Vec<f64> = genuine_s.iter().map(|s| s.get() as f64).collect();
    let impostor: Vec<f64> = impostor_s.iter().map(|s| s.get() as f64).collect();

    let mean = |xs: &[f64]| xs.iter().sum::<f64>() / xs.len() as f64;
    // Sample variance (n-1); a single pair has no spread to report, so it
    // contributes zero rather than a NaN.
    let var = |xs: &[f64], m: f64| {
        if xs.len() < 2 {
            0.0
        } else {
            xs.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (xs.len() - 1) as f64
        }
    };
    let mean_genuine = mean(&genuine);
    let mean_impostor = mean(&impostor);
    let margin = mean_genuine - mean_impostor;
    // Pooled SD, floored so a degenerate distribution reports a huge d′ rather
    // than an infinity that prints as `inf` and compares strangely.
    let pooled_sd = ((var(&genuine, mean_genuine) + var(&impostor, mean_impostor)) / 2.0)
        .sqrt()
        .max(1e-9);

    // Mann-Whitney U / |g| |i| — ROC-AUC without building the curve.
    let mut wins = 0.0f64;
    for g in &genuine {
        for i in &impostor {
            wins += match g.total_cmp(i) {
                std::cmp::Ordering::Greater => 1.0,
                std::cmp::Ordering::Equal => 0.5,
                std::cmp::Ordering::Less => 0.0,
            };
        }
    }

    ArmScore {
        eer,
        mean_genuine,
        mean_impostor,
        margin,
        d_prime: margin / pooled_sd,
        auc: wins / (genuine.len() * impostor.len()) as f64,
        frr_step: 1.0 / genuine.len() as f64,
        far_step: 1.0 / impostor.len() as f64,
    }
}

/// **OS-8's question, as one function — and it is not the question this arm
/// asked before it had a backend.**
///
/// The first version of this gate asserted `shipped_eer <= pre_fix_eer`: the
/// shipped decimator must not make speaker embeddings worse. The first real
/// measurement falsified that as a criterion, and not by a hair. On fixture (e)
/// the shipped decimator was better (EER 0.3924 → 0.2743); **on fixture (f) the
/// PRE-FIX one was better, on every statistic** (EER 0.0610 vs 0.2635, margin
/// 0.2978 vs 0.1700, d′ 2.577 vs 1.165, AUC 0.9635 vs 0.7880). One experiment,
/// two fixtures, opposite verdicts.
///
/// The CONTROL is what resolves it, and it costs nothing because the arm
/// already computes the audio it needs: decimate the SAME utterances with
/// nothing above 8 kHz added, each way. With no ultrasonic content there is
/// nothing to fold, so the two decimators must agree — and measured, they do,
/// to the digit: EER 0.2743 vs 0.2743 on (e) and 0.3176 vs 0.3176 on (f), with
/// margins inside 0.006 of each other. That is the score these voices earn on
/// this corpus, and it is the reference both noisy arms are then compared
/// against.
///
/// Against it the (f) result reads correctly. The pre-fix arm did not become a
/// better speaker recogniser — it moved AWAY from the control in the flattering
/// direction, inflating same-speaker similarity (mean genuine 0.716 → 0.847)
/// while leaving different-speaker similarity where it was (0.536 → 0.549). The
/// folded 58-tone comb is deterministic and identical in every utterance, so
/// two clips that both carry it look more like each other; the embedding is
/// partly fingerprinting the artifact instead of the voice. On (e) the same
/// fold moved the same arm away from the control in the other direction —
/// impostor similarity inflated 0.506 → 0.784, everyone starting to sound like
/// everyone else, which is the collapse OS-8's finding actually predicts. The
/// shipped arm, on both fixtures, lands back on the control.
///
/// **So the criterion is FIDELITY TO THE CONTROL, not the sign of a
/// difference.** An anti-alias filter's job is to make the >8 kHz content leave
/// no trace; a filter that traded one distortion for a flattering one would
/// pass a `<=` gate and fail this one. That is the whole reason the sign test
/// was the wrong shape: an EER arm that only asks "did the number get worse"
/// would have reported the ALIASED path as better on (f) and licensed YV129 to
/// tune enrollment bands on it.
///
/// Every comparison below is between two distances, so **there is no tolerance
/// constant anywhere in this function** — nothing to tune, nothing copied from
/// anywhere, and nothing that has to be re-derived on another machine.
///
/// It is a function rather than inline `assert!`s because the arm's
/// `pool.embed()` loop needs the corpus AND both catalog models, and a gate
/// that only executes on one laptop is not a gate. This half executes on every
/// machine, in both directions, from
/// [`meeting_eval_anti_alias_eer_the_gate_rejects_a_worse_shipped_arm`].
///
/// The message is tagged `[premise]` / `[eer]` / `[margin]` so a test can pin
/// WHICH comparison rejected without matching on prose.
fn shipped_arm_tracks_the_control(
    control: &ArmScore,
    control_alt: &ArmScore,
    pre_fix: &ArmScore,
    shipped: &ArmScore,
) -> Result<(), String> {
    let eer_from = |a: &ArmScore| (a.eer.eer - control.eer.eer).abs();
    let margin_from = |a: &ArmScore| (a.margin - control.margin).abs();

    // ── the premise ──────────────────────────────────────────────────────────
    // The control is only a reference if the decimator choice does not move it.
    // Asserted rather than assumed, and asserted in the units of the thing it
    // is a reference FOR: the two clean arms must agree with each other more
    // closely than the pre-fix arm agrees with either. If a future corpus has
    // in-band content the two filters treat differently, this is what says so
    // instead of silently redefining "control".
    let control_spread = (control.margin - control_alt.margin).abs();
    if control_spread >= margin_from(pre_fix) {
        return Err(format!(
            "[premise] with nothing above 8 kHz to fold, the two decimators must agree: \
             the clean arms' margins differ by {control_spread:.6}, which is not smaller \
             than the {:.6} the pre-fix arm differs from the control by. There is no \
             reference here to compare either noisy arm against.",
            margin_from(pre_fix)
        ));
    }
    if (control.eer.eer - control_alt.eer.eer).abs() > control.eer_step() {
        return Err(format!(
            "[premise] the two clean arms' EERs differ by more than the {:.4} quantum this \
             distribution can express ({:.4} vs {:.4}) — the decimator is changing the \
             score on audio that has nothing to fold, so it is not an aliasing effect \
             being measured below.",
            control.eer_step(),
            control.eer.eer,
            control_alt.eer.eer
        ));
    }

    // ── the gate ─────────────────────────────────────────────────────────────
    if eer_from(shipped) > eer_from(pre_fix) {
        return Err(format!(
            "[eer] the shipped decimator must leave the added >8 kHz band without a trace, \
             and it is further from the no-ultrasonic control than the pre-fix decimator \
             is: control {:.4}, shipped {:.4} (Δ {:.4}), pre-fix {:.4} (Δ {:.4})",
            control.eer.eer,
            shipped.eer.eer,
            eer_from(shipped),
            pre_fix.eer.eer,
            eer_from(pre_fix)
        ));
    }
    if margin_from(shipped) >= margin_from(pre_fix) {
        return Err(format!(
            "[margin] same test on the genuine/impostor margin, which is the statistic that \
             carries this when both EERs sit at the {:.4} quantisation floor: control \
             {:.6}, shipped {:.6} (Δ {:.6}), pre-fix {:.6} (Δ {:.6})",
            control.eer_step(),
            control.margin,
            shipped.margin,
            margin_from(shipped),
            pre_fix.margin,
            margin_from(pre_fix)
        ));
    }

    // **d′ and ROC-AUC are printed beside these two and deliberately NOT gated,
    // and the reason is measured twice over.** First:
    // `meeting_eval_anti_alias_eer_dprime_is_blind_to_the_degradation_os8_predicts`
    // needs neither corpus nor model to show that folded energy does not ADD to
    // an embedding, it takes over part of one — so it compresses within-speaker
    // spread by the same factor as between-speaker margin, and d′ is the ratio
    // of exactly those two. Under a model that compresses both, the margin
    // falls by more than two orders of magnitude while d′ falls by under a
    // quarter and AUC does not move at all. Second, on the real fixtures: on
    // (e) both statistics rank the ALIASED arm as closer to the control (d′
    // 0.835 vs 0.735 against a control 0.884; AUC 0.727 vs 0.706 against
    // 0.736) while the EER and the margin both rank the shipped arm closer, and
    // on (f) they agree with the EER and the margin. A statistic that answers
    // differently per fixture on a mechanism question is a diagnostic. It stays
    // in the transcript — a d′ that moves while the margin does not is a spread
    // story worth reading — and out of the gate, because it cannot carry one.
    Ok(())
}

/// The environment variable a machine that cannot compute an EER must set
/// before this arm will let it pass.
///
/// See the section header for where this gate reaches and where it does not.
/// The short version: it is consulted only on a machine that HAS the eval
/// corpus, because this arm returns at `corpus()` before anything else. It is
/// deliberately set nowhere in the repo — `ci.yml` used to export it, which was
/// dead configuration on a runner that never reaches this line, and
/// [`meeting_eval_anti_alias_eer_ci_does_not_declare_what_it_never_reaches`]
/// keeps it from coming back.
///
/// Since YV122 landed there is exactly ONE value this can honestly take —
/// `models_missing` — because a build with no inference backend no longer
/// exists. The `no_backend` declarations in this item's own earlier transcripts
/// stopped counting on that merge, which is the self-expiry the reason-naming
/// was for.
const EER_UNMEASURED_OK: &str = "YAP_EER_UNMEASURED_OK";

/// Whether a given value of [`EER_UNMEASURED_OK`] counts as a declaration of
/// THIS machine's current reason for having no embedder.
///
/// Split out from the panic so the POLICY can be tested without a test writing
/// to the process environment — which is both racy under the test harness's
/// threads and `unsafe` on modern editions.
///
/// The value has to be the reason's own tag (`Embedder::skip_tag`), not a bare
/// `1`. That is the difference between a declaration and an escape hatch: an
/// `export YAP_EER_UNMEASURED_OK=1` in a shell profile silences this arm
/// permanently, whereas `=no_backend` stops counting the moment `yap-diarize`
/// gains a backend and the machine's reason becomes `models_missing`. An empty
/// value, a `0` or a `true` left over from some other tool have never been
/// declarations and still are not.
fn unmeasured_eer_is_declared(value: Option<&str>, reason_tag: &str) -> bool {
    value == Some(reason_tag)
}

/// Panic unless this machine has DECLARED, by name, why it cannot measure an
/// EER.
fn require_declared_unmeasured_eer(why: &str, reason_tag: &str) {
    let declared = std::env::var(EER_UNMEASURED_OK);
    if unmeasured_eer_is_declared(declared.as_deref().ok(), reason_tag) {
        eprintln!(
            "  {EER_UNMEASURED_OK}={reason_tag} — this machine has declared, on the record, \
             that it cannot measure an EER: {why}"
        );
        return;
    }
    panic!(
        "the EER half of OS-8's arm did not run and this machine did not declare why.\n\
         \n  reason from the shipped sidecar: {why}\n\
         \nOS-8 requires the EER comparison BEFORE any enrollment threshold is tuned, so \
         a silent skip here is the failure it was written to prevent. This machine has \
         the corpus, which makes it one of the few that can close the item. Do one of:\n\
         \n  * install the two catalog diarization models and re-run — this is the path \
         that closes the item;\n\
         \n  * set {EER_UNMEASURED_OK}={reason_tag} for this run, which is a statement in \
         the PR transcript that this machine has no embedder, and which stops counting by \
         itself as soon as that stops being the reason. Nothing in the repo sets it: CI \
         has no corpus, so CI never reaches this line and a declaration there would be \
         decoration.\n"
    );
}

/// **The arm.** Both decimations of the same fixture, scored on enrollment-EER.
///
/// ```sh
/// cargo test --test meeting_eval anti_alias_eer_regression -- --nocapture
/// ```
///
/// Three things are asserted, and the first runs everywhere the corpus does:
///
/// 1. **The two arms differ, in the way OS-8 says they differ.** The shipped
///    decimator keeps at least [`FOLD_REJECTION_DB`] more of the >8 kHz band
///    out of 0–8 kHz than the pre-fix one, measured on each fixture's own
///    speech. Without this, an EER comparison could report "no difference" and
///    mean "there was nothing to fold".
/// 2. **The shipped arm's EER is not worse** — the literal ask.
/// 3. **The shipped arm's genuine/impostor MARGIN and d′ are not worse** — the
///    same ask, in a statistic that does not bottom out at zero. See
///    [`ArmScore`]. Without this, three well-separated voices score `0.0000`
///    on both arms and criterion 2 passes while measuring nothing.
///
/// The last two need an embedder; see the section header for what a machine
/// without one has to declare before this test will pass.
///
/// **All three need the CORPUS**, which is why the numbers this arm produces
/// are measured on a corpus-equipped developer machine and pasted into the
/// backlog rather than recomputed per run. On every other machine — CI
/// included — the very first line below returns, and what binds there is
/// [`meeting_eval_anti_alias_eer_measurement_stays_backed_by_a_real_backend`],
/// which needs neither corpus nor model.
#[test]
fn meeting_eval_anti_alias_eer_regression() {
    let Some(root) = corpus() else {
        // Said out loud, because a run that prints nothing about this arm reads
        // in a log exactly like a run where the arm passed. Nothing below here
        // executed: not the fold assert, not the embedder request, and not the
        // declaration gate.
        println!(
            "meeting_eval anti_alias_eer UNRUN reason=no corpus at {} — fold, EER and the \
             {EER_UNMEASURED_OK} gate all need it; the gate that binds without a corpus is \
             meeting_eval_anti_alias_eer_measurement_stays_backed_by_a_real_backend",
            corpus_root().display()
        );
        return;
    };

    // ── (1) the two arms are actually different on this audio ────────────────
    let prepared: Vec<(&str, Vec<DecimatedPair>, f32)> = ARM_FIXTURES
        .iter()
        .map(|id| {
            let utterances = arm_utterances(&root, id);
            let speakers = {
                let mut ids: Vec<&str> = utterances.iter().map(|u| u.speaker.as_str()).collect();
                ids.sort_unstable();
                ids.dedup();
                ids.len()
            };
            assert!(
                speakers >= 2 && utterances.len() >= 4,
                "{id}: an EER needs impostor pairs: {speakers} speaker(s), {} utterance(s)",
                utterances.len()
            );
            let turns = read_rttm(&root, id).len();
            // The pair counts depend only on the LABELS, so they are known
            // before any embedder exists — and they are what sets the EER's
            // resolution, which is the second review finding's whole subject.
            let (genuine, impostor) = {
                let labels: Vec<(String, Vec<f32>)> = utterances
                    .iter()
                    .map(|u| (u.speaker.clone(), vec![0.0]))
                    .collect();
                let (g, i) = genuine_and_impostor(&labels);
                (g.len(), i.len())
            };
            println!(
                "meeting_eval anti_alias_eer fixture={id} rttm_turns={turns} \
                 unoverlapped_spans={} speakers={speakers} \
                 min_span_seconds={ARM_MIN_UTTERANCE_SECONDS:.1} genuine_pairs={genuine} \
                 impostor_pairs={impostor} frr_step={:.4} far_step={:.4}",
                utterances.len(),
                1.0 / genuine as f64,
                1.0 / impostor as f64,
            );

            let pairs = decimate_both_ways(&utterances);
            // Concatenated so this is one number over the whole fixture rather
            // than a per-utterance average that a single loud turn could carry.
            let cat = |pick: fn(&DecimatedPair) -> &Vec<f32>| -> Vec<f32> {
                pairs.iter().flat_map(|p| pick(p).iter().copied()).collect()
            };
            let folded_linear = residual_rms(&cat(|p| &p.linear), &cat(|p| &p.linear_clean));
            let folded_shipped = residual_rms(&cat(|p| &p.shipped), &cat(|p| &p.shipped_clean));
            let removed_db = 20.0 * (folded_linear / folded_shipped.max(1e-12)).log10();
            println!(
                "meeting_eval anti_alias_eer fixture={id} in_band_fold_linear={folded_linear:.6} \
                 in_band_fold_antialiased={folded_shipped:.6} removed_db={removed_db:.1}"
            );
            assert!(
                removed_db >= FOLD_REJECTION_DB,
                "{id}: the shipped decimator must keep ≥{FOLD_REJECTION_DB:.0} dB more of the \
                 >8 kHz band out of the speech band, got {removed_db:.1} dB — with the two \
                 arms this close there is nothing for an EER to separate"
            );
            (*id, pairs, removed_db)
        })
        .collect();

    // ── (2)+(3) the separation of each arm, on each fixture ──────────────────
    // Resolved once, before any fixture is scored, so the declaration below is
    // evaluated exactly once per run rather than once per fixture.
    let embedder = support::diarize::embedder();
    let (pool, embedding_dim) = match &embedder {
        support::diarize::Embedder::Ready {
            pool,
            embedding_dim,
        } => (pool, *embedding_dim),
        other => {
            let why = other.skip_reason().expect("not Ready");
            let reason_tag = other.skip_tag().expect("not Ready");
            for (id, _, removed_db) in &prepared {
                println!(
                    "meeting_eval anti_alias_eer fixture={id} EER=UNMEASURED reason={why} \
                     fold_rejection_db={removed_db:.1}"
                );
            }
            eprintln!(
                "  the fold half of OS-8's arm is measured above; the EER half needs an \
                 embedder and this machine has none: {why}"
            );
            require_declared_unmeasured_eer(&why, reason_tag);
            return;
        }
    };
    eprintln!("  embedder ready: {embedding_dim}-dimensional embeddings");

    let dir = support::temp_dir("yv124-anti-alias-eer");

    // ── the embedding floor is LIVE on this path, proved once per run ─────────
    // Every span this arm embeds is trimmed to at least
    // [`ARM_MIN_UTTERANCE_SECONDS`], so the floor passed to `embed` below never
    // fires on these fixtures — which would make it a parameter nobody could
    // tell from `Duration::ZERO`. YV122 shipped it as a REFUSAL precisely so
    // that "the engine gave us something" stops being evidence that it gave us
    // anything, and a floor that has never refused anything here is a claim
    // this arm has not earned. So it is exercised: half the floor, in the same
    // process, against the same child, and it must come back as a refusal.
    {
        let short = dir.join("under-the-floor.wav");
        let samples = vec![0.0f32; (TARGET_RATE as f64 * ARM_MIN_UTTERANCE_SECONDS / 2.0) as usize];
        write_wav_16k_mono(&short, &to_i16(&samples));
        let answer = pool.embed(&short, Duration::from_secs_f64(ARM_MIN_UTTERANCE_SECONDS));
        let refused = match answer {
            Err(e) => e,
            // Reported as a WIDTH rather than as the vector: a 512-float dump
            // buries the sentence that matters, and the width is the whole
            // point — YV122 measured that a fifth of a second comes back as a
            // perfectly ordinary-looking full-width vector that matches its own
            // speaker worse than an average stranger does.
            Ok(vector) => panic!(
                "a clip half the length of the floor came back as a {}-float vector instead \
                 of a refusal, so the floor this arm passes below is inert and every \
                 embedding under it is a number nobody measured",
                vector.len()
            ),
        };
        println!(
            "meeting_eval anti_alias_eer embed_floor_seconds={ARM_MIN_UTTERANCE_SECONDS:.1} \
             half_length_clip={refused:?} — the floor refuses, so passing it below is a \
             live assertion that the overlap trim did its job"
        );
    }

    // Every fixture is scored and PRINTED before any of them is allowed to
    // fail. A gate that panics inside the loop hides the second fixture's
    // numbers, and on this arm the second fixture is the one that disagreed
    // with the first — the transcript that made the control necessary would
    // never have existed.
    let mut verdicts: Vec<(&str, Result<(), String>)> = Vec::new();
    for (id, pairs, _) in &prepared {
        // Four arms, not two. The two `*_clean` decimations are of the SAME
        // utterances with no >8 kHz content added, so they are what these
        // voices score when there is nothing to fold — the reference both noisy
        // arms are measured against. They cost two more embed calls per
        // utterance and they are the difference between a sign test and a
        // measurement (see `shipped_arm_tracks_the_control`).
        let mut control_shipped_e = Vec::new();
        let mut control_linear_e = Vec::new();
        let mut linear_embeddings = Vec::new();
        let mut shipped_embeddings = Vec::new();
        for (i, pair) in pairs.iter().enumerate() {
            for (arm, samples, out) in [
                (
                    "control-shipped",
                    &pair.shipped_clean,
                    &mut control_shipped_e,
                ),
                ("control-linear", &pair.linear_clean, &mut control_linear_e),
                ("linear", &pair.linear, &mut linear_embeddings),
                ("shipped", &pair.shipped, &mut shipped_embeddings),
            ] {
                let path = dir.join(format!("{id}-{arm}-{i:02}.wav"));
                write_wav_16k_mono(&path, &to_i16(samples));
                // The floor YV122 made mandatory, spent here as the SAME
                // number this arm already trims its spans to. Passing
                // [`ARM_MIN_UTTERANCE_SECONDS`] is not a second threshold: it
                // is the assertion that the trim above did its job, because a
                // span that reached here under the floor comes back as
                // `audio_too_short` and this `unwrap_or_else` panics with the
                // fixture and the index that produced it. No accuracy number
                // is set anywhere in this arm — see the `NOTE` printed below
                // every fixture's scores.
                let embedding = pool
                    .embed(&path, Duration::from_secs_f64(ARM_MIN_UTTERANCE_SECONDS))
                    .unwrap_or_else(|e| panic!("embed {id} {arm}-{i:02}: {e:?}"));
                assert_eq!(
                    embedding.len() as u32,
                    embedding_dim,
                    "the child reported {embedding_dim} dimensions and then returned {}",
                    embedding.len()
                );
                out.push((pair.speaker.clone(), embedding));
            }
        }

        // The arms fed the embedder DIFFERENT audio, so they must have got
        // different vectors back. Identical ones would mean the decimations
        // never reached the child — every comparison below would then be a
        // vector compared against itself, and every assertion trivially true.
        // Checked for the noisy pair AND for noisy-against-control, because it
        // is the control that carries the criterion now.
        for (what, a, b) in [
            (
                "the two noisy arms",
                &linear_embeddings,
                &shipped_embeddings,
            ),
            (
                "the shipped arm and its control",
                &shipped_embeddings,
                &control_shipped_e,
            ),
        ] {
            assert!(
                a.iter().zip(b.iter()).any(|((_, x), (_, y))| x != y),
                "{id}: {what} produced byte-identical embeddings, so nothing under test \
                 reached the embedder"
            );
        }

        let control = arm_score(&control_shipped_e);
        let control_alt = arm_score(&control_linear_e);
        let pre_fix = arm_score(&linear_embeddings);
        let shipped = arm_score(&shipped_embeddings);
        for (arm, score) in [
            ("control_shipped", &control),
            ("control_linear", &control_alt),
            ("pre_fix", &pre_fix),
            ("shipped", &shipped),
        ] {
            println!(
                "meeting_eval anti_alias_eer fixture={id} arm={arm} eer={:.4} \
                 mean_genuine={:.6} mean_impostor={:.6} margin={:.6} dprime={:.4} \
                 auc={:.4} threshold_at_eer={:.4} genuine={} impostor={} eer_step={:.4}",
                score.eer.eer,
                score.mean_genuine,
                score.mean_impostor,
                score.margin,
                score.d_prime,
                score.auc,
                score.eer.threshold_at_eer.get(),
                score.eer.genuine,
                score.eer.impostor,
                score.eer_step(),
            );
        }
        // The criterion itself, in the two numbers it compares, so a reader of
        // the transcript never has to recompute a distance by hand.
        println!(
            "meeting_eval anti_alias_eer fixture={id} distance_from_control \
             eer_shipped={:.4} eer_pre_fix={:.4} margin_shipped={:.6} \
             margin_pre_fix={:.6} control_spread_margin={:.6}",
            (shipped.eer.eer - control.eer.eer).abs(),
            (pre_fix.eer.eer - control.eer.eer).abs(),
            (shipped.margin - control.margin).abs(),
            (pre_fix.margin - control.margin).abs(),
            (control.margin - control_alt.margin).abs(),
        );
        eprintln!("  {id} control (no >8 kHz added, Butterworth): {control:?}");
        eprintln!("  {id} control (no >8 kHz added, linear):      {control_alt:?}");
        eprintln!("  {id} pre-fix (linear decimation):            {pre_fix:?}");
        eprintln!("  {id} shipped (Butterworth):                  {shipped:?}");
        if pre_fix.eer_is_saturated() && shipped.eer_is_saturated() {
            println!(
                "meeting_eval anti_alias_eer fixture={id} EER=SATURATED both arms sit at the \
                 {:.4} floor this many pairs can express — the margin comparison is what \
                 carries this fixture",
                shipped.eer_step()
            );
        }
        eprintln!(
            "  NOTE: no threshold_at_eer above is a shipped threshold, and the control's is \
             not one either. YV129 tunes the enrollment bands, against post-fix embeddings \
             only, and not on this corpus — see YV122's `diarize_embed_smoke`."
        );

        verdicts.push((
            id,
            shipped_arm_tracks_the_control(&control, &control_alt, &pre_fix, &shipped),
        ));
    }
    pool.shutdown();

    let failed: Vec<String> = verdicts
        .iter()
        .filter_map(|(id, verdict)| verdict.as_ref().err().map(|why| format!("{id}: {why}")))
        .collect();
    assert!(
        failed.is_empty(),
        "{} of {} fixtures failed OS-8's arm:\n  {}",
        failed.len(),
        verdicts.len(),
        failed.join("\n  ")
    );
}

/// **d′ is near-blind to the degradation OS-8 predicts, and this is the
/// experiment that says so.**
///
/// A previous revision of this arm added d′ as a third gate beside the EER and
/// the margin, on the reasoning that the EER quantises and a continuous
/// statistic cannot. The first half of that is right and
/// [`meeting_eval_anti_alias_eer_margin_resolves_what_the_eer_quantises_away`]
/// still holds it. The second half picked the wrong continuous statistic, and
/// the real measurement is what exposed it: fixture (e)'s shipped arm improved
/// the EER by 30 % and more than doubled the margin while d′ went DOWN.
///
/// The mechanism, and it is not subtle once stated. Folded energy is not noise
/// laid on top of an embedding — the fold happens in the audio, so the
/// corrupted component is INSIDE the vector the model returns, taking over part
/// of it. It therefore compresses the within-speaker spread by the same factor
/// as the between-speaker margin. d′ is `margin / pooled_sd`: numerator and
/// denominator both shrink, and the ratio barely moves. ROC-AUC is worse still
/// for this purpose — it reads only the ORDER of the scores, and a
/// common-mode component that crushes every cosine toward every other one can
/// leave the order completely intact.
///
/// So the model below scales the per-utterance jitter with the speaker
/// centroid, which is what the physics does and what
/// [`synthetic_embeddings`] deliberately does not (it adds jitter at full
/// scale afterwards — a fine model of *additive* noise, and the reason d′
/// appeared to work when it was only ever tested against that one).
///
/// **Neither model is "the real one" and no number here is a measurement of
/// the resampler.** The claim is narrow and it is about a statistic: there
/// exists a physically motivated common-mode corruption under which the margin
/// falls by more than two orders of magnitude, d′ falls by under a quarter, and
/// AUC does not move at all. That is enough to disqualify d′ as a gate for this
/// specific effect, which is all this test is used for.
#[test]
fn meeting_eval_anti_alias_eer_dprime_is_blind_to_the_degradation_os8_predicts() {
    // The same generator as `synthetic_embeddings`, with ONE change: the jitter
    // is compressed along with the centroid instead of being added at full
    // scale on top of the corrupted vector.
    fn common_mode_inside_the_vector(common_mode: f32) -> Vec<(String, Vec<f32>)> {
        let mut state = 0x5eed_1234u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state as f32 / u32::MAX as f32) * 2.0 - 1.0
        };
        let shared: Vec<f32> = (0..SYNTHETIC_DIM).map(|_| next()).collect();
        let mut out = Vec::new();
        for s in 0..SYNTHETIC_SPEAKERS {
            let centroid: Vec<f32> = (0..SYNTHETIC_DIM).map(|_| next()).collect();
            for _ in 0..SYNTHETIC_UTTERANCES {
                let vector: Vec<f32> = centroid
                    .iter()
                    .zip(shared.iter())
                    .map(|(c, k)| {
                        (c + next() * SYNTHETIC_JITTER) * (1.0 - common_mode) + k * common_mode
                    })
                    .collect();
                out.push((format!("spk_{s}"), vector));
            }
        }
        out
    }

    let clean = arm_score(&common_mode_inside_the_vector(0.0));
    let swamped = arm_score(&common_mode_inside_the_vector(0.95));
    println!(
        "meeting_eval anti_alias_eer dprime_blindness margin {:.6} → {:.6} ({:.0}x) \
         dprime {:.4} → {:.4} ({:.2}x) auc {:.4} → {:.4} eer {:.4} → {:.4} \
         mean_impostor {:.4} → {:.4}",
        clean.margin,
        swamped.margin,
        clean.margin / swamped.margin,
        clean.d_prime,
        swamped.d_prime,
        clean.d_prime / swamped.d_prime,
        clean.auc,
        swamped.auc,
        clean.eer.eer,
        swamped.eer.eer,
        clean.mean_impostor,
        swamped.mean_impostor,
    );

    // The premise: this really is a severe common-mode corruption. Two
    // different speakers end up scoring as almost the same person.
    assert!(
        swamped.mean_impostor > 0.99,
        "the corruption is not severe enough for this test to be about a severe \
         corruption: impostor pairs mean {:.4}",
        swamped.mean_impostor
    );

    // What the margin sees: a collapse of more than two orders of magnitude.
    assert!(
        clean.margin / swamped.margin > 100.0,
        "the margin must see this corruption: {:.6} → {:.6}",
        clean.margin,
        swamped.margin
    );

    // What d′ sees: almost nothing. Stated as a ratio so the assertion is about
    // the SIZE of the blindness rather than about a direction that a float
    // wobble could satisfy.
    assert!(
        clean.d_prime / swamped.d_prime < 1.5,
        "d′ tracked this corruption after all ({:.4} → {:.4}) — if that is now true, \
         the reason `arm_is_not_worse` stopped gating on it no longer holds and the \
         gate should get it back",
        clean.d_prime,
        swamped.d_prime
    );

    // What AUC sees: nothing whatsoever. The order is untouched.
    assert_eq!(
        clean.auc, swamped.auc,
        "AUC moved under a pure common-mode corruption, which is the one thing a rank \
         statistic should be invariant to"
    );

    // And the EER agrees with AUC here, which is the honest limit of this
    // experiment: a corruption that preserves ORDER is invisible to both. That
    // is not a reason to drop the EER — on the real fixtures it is nowhere near
    // saturated and it moved by 30 % — it is the reason the margin is gated
    // BESIDE it rather than instead of it.
    assert_eq!(
        clean.eer.eer, swamped.eer.eer,
        "the EER moved under a corruption that preserves the ranking, so this test's \
         account of why the margin is the gate is wrong somewhere"
    );
}

/// **The arm's scoring path, proved able to fail — with no corpus and no
/// model.**
///
/// The gate above needs the corpus AND the two catalog models, so on every
/// machine that has neither the machinery between "two sets of embeddings" and
/// "the assertion" would never execute. It executes here, on the SAME
/// [`genuine_and_impostor`] and the SAME `enrollment_eer` the arm calls, over
/// vectors this test builds.
///
/// **These vectors are not CAM++ output and nothing here is a measurement of
/// the resampler.** They are a model of one specific thing: what folded
/// broadband noise does to an embedding. Aliased energy lands in the same part
/// of the spectrum for every speaker, so it enters every utterance's embedding
/// as a shared, common-mode direction — which drags impostor pairs toward each
/// other and collapses the margin an EER measures. The degraded set below is
/// the clean set plus exactly that common-mode component, and the assertion is
/// that the arm's own scoring reports it as worse. An arm that could not tell
/// these two apart could not tell the two decimators apart either.
///
/// **Read this model's LIMIT alongside it.** It adds the common-mode component
/// on top of a per-utterance jitter left at full scale, which makes it a model
/// of additive noise rather than of energy that has taken over part of the
/// vector.
/// [`meeting_eval_anti_alias_eer_dprime_is_blind_to_the_degradation_os8_predicts`]
/// is the same idea with the jitter compressed too, and the two models
/// disagree about d′ — which is exactly how d′ came to be a printed diagnostic
/// here rather than a gate.
#[test]
fn meeting_eval_anti_alias_eer_arm_scores_the_degradation_os8_predicts() {
    let clean = synthetic_embeddings(0.0);
    let folded = synthetic_embeddings(0.9);

    let clean_eer = arm_eer(&clean);
    let folded_eer = arm_eer(&folded);
    println!(
        "meeting_eval anti_alias_eer synthetic clean_eer={:.4} common_mode_eer={:.4} \
         genuine={} impostor={}",
        clean_eer.eer, folded_eer.eer, clean_eer.genuine, clean_eer.impostor
    );

    // The gate the arm above applies, as a function, so it can be run in both
    // directions: `assert!(shipped.eer <= pre_fix.eer)`.
    let gate = |shipped: f64, pre_fix: f64| shipped <= pre_fix;
    assert!(
        gate(clean_eer.eer, folded_eer.eer),
        "the arm's scoring cannot see a common-mode corruption it is supposed to \
         catch: clean {:.4}, corrupted {:.4}",
        clean_eer.eer,
        folded_eer.eer
    );
    // …and both ends are what they claim to be. A corrupted set that still
    // separates, or a clean set that does not, would make the comparison above
    // true for reasons that have nothing to do with the arm working. MEASURED:
    // clean 0.0000, nine-parts-shared 0.4410 (near the 0.5 of a coin flip).
    assert!(
        folded_eer.eer > 0.25,
        "nine parts shared noise to one part person still separates at EER {:.4}, so \
         the degradation is not a degradation",
        folded_eer.eer
    );
    assert!(
        clean_eer.eer < 0.05,
        "the clean distribution does not separate, so this test would pass on any \
         two piles of noise: clean EER {:.4}",
        clean_eer.eer
    );
    // …and the same gate with the two arms swapped must REJECT. Without this
    // line, a gate that is true for every input would satisfy everything above.
    assert!(
        !gate(folded_eer.eer, clean_eer.eer),
        "`eer_shipped <= eer_pre_fix` holds even with the arms swapped, so it \
         gates nothing"
    );
}

/// **The separation statistics, proved to resolve what the EER cannot.**
///
/// Review fix #2's premise, as a test rather than an argument: an EER over 18
/// genuine pairs is quantised at [`ArmScore::eer_step`] and floored at zero, so
/// two distributions that differ by less than that quantum report the SAME EER.
/// The margin and d′ report the difference anyway. If that ever stopped being
/// true, the arm above would be back to gating nothing whenever its voices are
/// cleanly separated, and this test is what notices.
#[test]
fn meeting_eval_anti_alias_eer_margin_resolves_what_the_eer_quantises_away() {
    let clean = arm_score(&synthetic_embeddings(0.0));
    // Small enough that neither distribution crosses a single pair over the
    // decision boundary — both EERs stay pinned at the floor.
    let barely_worse = arm_score(&synthetic_embeddings(0.05));

    println!(
        "meeting_eval anti_alias_eer resolution genuine={} impostor={} frr_step={:.4} \
         far_step={:.4} eer_step={:.4} eer_clean={:.4} eer_barely_worse={:.4} \
         margin_clean={:.6} margin_barely_worse={:.6} dprime_clean={:.4} \
         dprime_barely_worse={:.4} auc_clean={:.4} auc_barely_worse={:.4}",
        clean.eer.genuine,
        clean.eer.impostor,
        clean.frr_step,
        clean.far_step,
        clean.eer_step(),
        clean.eer.eer,
        barely_worse.eer.eer,
        clean.margin,
        barely_worse.margin,
        clean.d_prime,
        barely_worse.d_prime,
        clean.auc,
        barely_worse.auc,
    );

    // The premise: fixture (e)'s pair counts, and an EER that cannot express
    // anything between zero and a step.
    assert_eq!(clean.eer.genuine, 18, "the 1/18 resolution floor is real");
    assert_eq!(clean.eer.impostor, 48);
    assert!(
        clean.eer_is_saturated() && barely_worse.eer_is_saturated(),
        "both EERs must sit at the floor for this test to be about the floor: \
         {:.4} and {:.4}, step {:.4}",
        clean.eer.eer,
        barely_worse.eer.eer,
        clean.eer_step()
    );
    assert_eq!(
        clean.eer.eer, barely_worse.eer.eer,
        "the EER already separates these two, so this test is not measuring what it \
         claims to measure"
    );

    // The point: the continuous statistics see the degradation the EER cannot,
    // and by a wide margin rather than by a float wobble.
    assert!(
        clean.margin > barely_worse.margin,
        "the margin cannot see a degradation the EER quantised away: {:.6} → {:.6}",
        clean.margin,
        barely_worse.margin
    );
    // d′ orders these two as well — UNDER THIS MODEL, which adds its jitter at
    // full scale on top of the corruption. Asserted here because it is true
    // here and because the contrast is the point: swap in the model where the
    // jitter is compressed too and d′ stops ordering anything
    // (`meeting_eval_anti_alias_eer_dprime_is_blind_to_the_degradation_os8_predicts`).
    // A statistic that answers correctly only for the corruption model you
    // happen to write down is a diagnostic, not a gate, and `arm_is_not_worse`
    // treats it as one.
    assert!(
        clean.d_prime > barely_worse.d_prime,
        "d′ cannot see a degradation the EER quantised away, even in the additive \
         model: {:.4} → {:.4}",
        clean.d_prime,
        barely_worse.d_prime
    );
    // …and the arm's own gate, run in both directions on the margin, so it is
    // not a comparison that would hold for any two inputs.
    let gate = |shipped: f64, pre_fix: f64| shipped >= pre_fix;
    assert!(gate(clean.margin, barely_worse.margin));
    assert!(
        !gate(barely_worse.margin, clean.margin),
        "`margin_shipped >= margin_pre_fix` holds with the arms swapped, so it gates \
         nothing"
    );
}

/// **Every fixture that CAN carry an EER is scored.**
///
/// [`ARM_FIXTURES`] is derived from the corpus here rather than trusted: any
/// fixture whose RTTM names two or more speakers can produce a genuine/impostor
/// distribution, and one that can and is not scored is a fixture the arm is
/// quietly ignoring. This is what makes dropping fixture (f) back out — the
/// far-field case OS-8's wording actually names — a red test rather than a
/// smaller diff.
#[test]
fn meeting_eval_anti_alias_eer_scores_every_multi_speaker_fixture() {
    let Some(root) = corpus() else { return };
    let mut can_carry_an_eer: Vec<&str> = FIXTURE_IDS
        .iter()
        .copied()
        .filter(|id| {
            let mut ids: Vec<String> = read_meta(&root, id)
                .rttm
                .iter()
                .map(|t| t.speaker_id.clone())
                .collect();
            ids.sort();
            ids.dedup();
            ids.len() >= 2
        })
        .collect();
    can_carry_an_eer.sort_unstable();
    let mut scored = ARM_FIXTURES.to_vec();
    scored.sort_unstable();
    assert_eq!(
        scored, can_carry_an_eer,
        "the arm scores {scored:?} but the corpus can carry an EER on \
         {can_carry_an_eer:?}"
    );
}

/// **The arm's whole gate, run in both directions, with no model and no corpus
/// on the machine.**
///
/// [`shipped_arm_tracks_the_control`] is the ~40 lines the arm's `pool.embed()`
/// loop feeds, and that loop needs the eval corpus and both catalog models.
/// This test runs the gate itself on the same [`ArmScore`] type the arm builds,
/// so the criterion is executed and falsified everywhere rather than on one
/// laptop — and each comparison is regressed on its own, so none of them can be
/// silently dropped without a red test.
///
/// The scores are built from [`synthetic_embeddings`] rather than typed in, so
/// the objects the gate sees are the objects [`arm_score`] produces.
#[test]
fn meeting_eval_anti_alias_eer_the_gate_rejects_a_worse_shipped_arm() {
    // The control: what these vectors score with nothing folded into them. Both
    // "decimations" of it are the same clean set, which is the measured
    // situation — on the real fixtures the two clean arms scored an identical
    // EER and margins within 0.006 of each other.
    let control = arm_score(&synthetic_embeddings(0.0));
    let control_alt = arm_score(&synthetic_embeddings(0.001));
    // A noisy arm that lands back on the control, and one that does not.
    let faithful = arm_score(&synthetic_embeddings(0.01));
    let degraded = arm_score(&synthetic_embeddings(0.9));

    // The direction the arm asserts: the shipped arm tracks the control, the
    // pre-fix one has wandered off it.
    assert_eq!(
        shipped_arm_tracks_the_control(&control, &control_alt, &degraded, &faithful),
        Ok(())
    );

    // Swapped, it must reject — and it must reject on the EER, which is the
    // statistic that CAN see a corruption this large.
    let rejected = shipped_arm_tracks_the_control(&control, &control_alt, &faithful, &degraded)
        .expect_err("the gate accepts a shipped arm that has wandered off the control");
    assert!(rejected.starts_with("[eer]"), "{rejected}");

    // **The finding that forced this shape**: a "shipped" arm that is BETTER
    // than the control on every statistic is still rejected, because an
    // anti-alias filter that flatters the score has not left the >8 kHz content
    // without a trace — it has traded one distortion for a nicer-looking one.
    // This is fixture (f)'s real result in miniature, and the `<=` gate this
    // replaced would have waved it through.
    let flattered = ArmScore {
        eer: EerReport {
            eer: control.eer.eer * 0.2,
            ..control.eer
        },
        margin: control.margin * 1.6,
        ..control
    };
    let rejected = shipped_arm_tracks_the_control(&control, &control_alt, &faithful, &flattered)
        .expect_err(
            "a shipped arm that scores far BETTER than the no-ultrasonic control is \
             accepted — that is the fixture (f) result, and reading it as a pass is what \
             would license YV129 to tune enrollment bands on an artifact",
        );
    assert!(
        rejected.starts_with("[eer]") || rejected.starts_with("[margin]"),
        "{rejected}"
    );

    // Each comparison on its own, with the other held equal, so neither can be
    // dropped from the gate and covered for by the other.
    let mut off_on_margin_only = faithful;
    off_on_margin_only.margin = control.margin + (faithful.margin - control.margin) * 40.0;
    let rejected =
        shipped_arm_tracks_the_control(&control, &control_alt, &faithful, &off_on_margin_only)
            .expect_err("the margin is in the report but not in the gate");
    assert!(rejected.starts_with("[margin]"), "{rejected}");

    // And the premise: without a control the two decimators agree on, there is
    // nothing to measure either arm against, and the gate says so rather than
    // quietly picking one.
    let mut disagreeing = control_alt;
    disagreeing.margin = control.margin - (degraded.margin - control.margin).abs() * 2.0;
    let rejected = shipped_arm_tracks_the_control(&control, &disagreeing, &degraded, &faithful)
        .expect_err("a control the two decimators disagree about is accepted as a reference");
    assert!(rejected.starts_with("[premise]"), "{rejected}");

    // **d′ on its own is deliberately ACCEPTED.** This assertion exists to stop
    // the gate quietly growing that arm back: d′ was a gate in an earlier
    // revision, and it is out for two measured reasons — see
    // `meeting_eval_anti_alias_eer_dprime_is_blind_to_the_degradation_os8_predicts`
    // and the (e)/(f) split recorded on `shipped_arm_tracks_the_control`. If
    // somebody decides it belongs back, this line is where they have to say so
    // on purpose.
    // Constructed so that a `shipped.d_prime < pre_fix.d_prime` comparison
    // WOULD reject: the pre-fix arm here is the faithful one, whose d′ is far
    // above the value planted below. Getting that backwards is how the first
    // cut of this assertion let a re-added d′ gate survive a mutation.
    let mut worse_dprime = faithful;
    worse_dprime.d_prime = degraded.d_prime * 0.1;
    assert!(
        worse_dprime.d_prime < degraded.d_prime,
        "this case does not regress d′ against the pre-fix arm at all ({:.4} vs {:.4}), \
         so accepting it proves nothing about whether d′ is gated",
        worse_dprime.d_prime,
        degraded.d_prime
    );
    assert_eq!(
        shipped_arm_tracks_the_control(&control, &control_alt, &degraded, &worse_dprime),
        Ok(()),
        "d′ is gating again. It is printed, not gated, and two measurements say why — \
         read them before re-adding the arm."
    );
}

/// **The declaration expires by itself.** The local half of the policy, as a
/// table.
///
/// The failure this replaces was not a wrong number, it was a green test that
/// had never run its own subject: `Embedder::NoBackend` and
/// `Embedder::ModelsMissing` both returned quietly, so the arm would have
/// stayed green past YV122 without ever computing an EER. The arm now panics
/// unless the machine says out loud why it cannot measure one.
///
/// "Out loud" means the machine's CURRENT reason, not a bare `1`. That is the
/// second half of the same review finding: a bare-`1` gate is one
/// `export YAP_EER_UNMEASURED_OK=1` away from being permanently off, while a
/// reason-named one goes red by itself the moment the reason changes
/// underneath it — which is exactly the moment the EER becomes measurable.
#[test]
fn meeting_eval_anti_alias_eer_unmeasured_needs_an_explicit_declaration() {
    // A declaration is the reason this machine actually has.
    assert!(unmeasured_eer_is_declared(Some("no_backend"), "no_backend"));
    assert!(unmeasured_eer_is_declared(
        Some("models_missing"),
        "models_missing"
    ));

    // The stale-export case, and it is no longer hypothetical. A declaration
    // written for one world does not carry into the next one: the
    // `YAP_EER_UNMEASURED_OK=no_backend` this item's own earlier transcripts
    // told a developer to export stopped counting the day YV122 landed and the
    // only reason a machine can now have became `models_missing`. The tag it
    // named is not merely stale — `diarize_protocol.rs` no longer defines it
    // and `Embedder` no longer has a variant that could return it.
    assert!(!unmeasured_eer_is_declared(
        Some("no_backend"),
        "models_missing"
    ));
    assert!(!unmeasured_eer_is_declared(
        Some("models_missing"),
        "no_backend"
    ));

    for not_a_declaration in [
        None,
        Some(""),
        Some("0"),
        Some("true"),
        Some("yes"),
        // The old bare-`1` form, which is what made the escape permanent.
        Some("1"),
        Some("no_backend "),
        Some("NO_BACKEND"),
    ] {
        assert!(
            !unmeasured_eer_is_declared(not_a_declaration, "no_backend"),
            "{not_a_declaration:?} must not count as a declaration that this machine \
             cannot measure an EER"
        );
    }

    // The tag is the `Embedder` state's own, so the value a developer is told
    // to export is the value the arm will compare against — and there is now
    // exactly ONE such value, because a build with no inference backend is not
    // a state any machine can be in since YV122.
    assert_eq!(
        support::diarize::Embedder::ModelsMissing("x".into()).skip_tag(),
        Some("models_missing")
    );
    assert_eq!(
        support::diarize::Embedder::ModelsMissing("x".into())
            .skip_reason()
            .as_deref(),
        Some("diarization model not installed: x")
    );

    // The variable is named in exactly one place in the test tree, so a rename
    // cannot leave a workflow exporting a name nothing reads.
    assert_eq!(EER_UNMEASURED_OK, "YAP_EER_UNMEASURED_OK");
}

/// This file's own source, read at compile time.
///
/// Same `include_str!` trick as [`CI_WORKFLOW`] below, pointed at the test file
/// rather than the workflow, and for the same reason: a claim about what the
/// tests SAY has to be checked against what they say, not against what the
/// author remembers writing.
const THIS_FILE: &str = include_str!("meeting_eval.rs");

/// **Both fold gates spend one constant, and this is what makes that true.**
///
/// A review finding, accepted: the PR body and the backlog both claimed
/// "the same bar the WER arm holds on the same measurement, so the two arms
/// cannot drift apart" while the WER arm asserted a bare `20.0` literal and the
/// EER arm asserted [`FOLD_REJECTION_DB`]. Two independent numbers wearing one
/// sentence. The literal is gone; this keeps it gone.
///
/// It reads the source rather than the behaviour because that is where the
/// defect lived — both arms passed the whole time, and would have gone on
/// passing while somebody moved one number and not the other.
#[test]
fn meeting_eval_antialias_both_fold_gates_spend_one_constant() {
    // Comment lines are stripped: the paragraph explaining this rule at the WER
    // arm's assert names the old literal on purpose, and a rule a comment can
    // trip is a rule somebody deletes the day it cries wolf.
    let code: Vec<&str> = THIS_FILE
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect();

    // The needle is JOINED at runtime so this line does not match itself. It
    // did in the first cut, which counted three gates instead of two — and
    // because the test's own name says `antialias` while every filter used
    // while writing it said `anti_alias`, the broken version had never once
    // executed. A source-reading test has to be told, explicitly, not to read
    // itself.
    let needle = format!("removed_db {}", ">=");
    let gates: Vec<&&str> = code.iter().filter(|line| line.contains(&needle)).collect();
    assert_eq!(
        gates.len(),
        2,
        "expected exactly two fold gates — the WER arm's and the EER arm's — found {}: {gates:#?}. \
         If a third arm has landed it belongs on the same constant; if one has gone, this \
         count is the thing to update deliberately.",
        gates.len()
    );
    for gate in &gates {
        assert!(
            gate.contains("FOLD_REJECTION_DB"),
            "a fold gate asserts on something other than FOLD_REJECTION_DB: `{}`. Both arms \
             exist to agree about what \"the anti-alias fix works\" means on the same \
             measurement, and two literals that happen to be equal today are not agreement.",
            gate.trim()
        );
    }

    // Non-vacuity, in the same test: the name the two gates share has to resolve
    // to a real positive figure in THIS file, or "both gates spend one constant"
    // is true of a constant that means nothing. Read out of the source for the
    // same reason as everything else here — a `FOLD_REJECTION_DB > 0.0` written
    // in Rust is constant-folded and asserts nothing at run time.
    let definition = code
        .iter()
        .find(|line| line.contains("const FOLD_REJECTION_DB"))
        .unwrap_or_else(|| {
            panic!(
                "FOLD_REJECTION_DB is not defined in this file, so the two gates above are \
                 not reading the constant this test thinks they are"
            )
        });
    let value: f32 = definition
        .rsplit('=')
        .next()
        .and_then(|rhs| rhs.trim().trim_end_matches(';').parse().ok())
        .unwrap_or_else(|| panic!("FOLD_REJECTION_DB is not a literal number: `{definition}`"));
    assert!(
        value > 0.0,
        "FOLD_REJECTION_DB is not a positive dB figure: `{definition}`"
    );
}

/// The workflow this repo actually runs, read at compile time — the same
/// `include_str!` trick `syscapture_os_gate.rs` uses to keep a CI claim
/// honest.
const CI_WORKFLOW: &str = include_str!("../../../.github/workflows/ci.yml");

/// **CI does not declare what CI never reaches.**
///
/// The revision this replaces exported `YAP_EER_UNMEASURED_OK: "1"` in
/// `ci.yml`'s `cargo test` step and said in the PR that CI was therefore held
/// to the expiry. It was not: `meeting_eval_anti_alias_eer_regression` opens on
/// `corpus()`, CI has no corpus, and the arm returns before the declaration is
/// read. The variable was inert, and the comment beside it named the wrong
/// precondition (the two catalog models) — so anyone who followed the written
/// deletion condition would have installed models, deleted the line, and
/// believed a hole was closed that was still open.
///
/// This test is what keeps that configuration from drifting back. It is
/// conditional rather than absolute on purpose: if CI ever DOES grow a corpus,
/// the declaration stops being decoration and the choice between measuring and
/// declaring becomes a real one to make there.
#[test]
fn meeting_eval_anti_alias_eer_ci_does_not_declare_what_it_never_reaches() {
    // Comments are stripped first: this is a claim about what the workflow
    // CONFIGURES, and the step above the `cargo test` line explains all of this
    // in prose that names both variables.
    let configuration: String = CI_WORKFLOW
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");

    if configuration.contains(CORPUS_ENV) {
        return;
    }
    assert!(
        !configuration.contains(EER_UNMEASURED_OK),
        "ci.yml sets {EER_UNMEASURED_OK} but never sets {CORPUS_ENV}, so \
         `meeting_eval_anti_alias_eer_regression` returns at its `corpus()` line and the \
         declaration is never read. Dead configuration that reads like a gate is worse \
         than no configuration: either give CI the corpus (and then this test steps \
         aside), or leave the variable out and let \
         `meeting_eval_anti_alias_eer_measurement_stays_backed_by_a_real_backend` be the \
         gate CI does hold."
    );
}

/// **The regression guard that replaced the expiry, once the expiry fired.**
///
/// This test used to be
/// `meeting_eval_anti_alias_eer_unmeasured_expires_when_the_sidecar_gains_a_backend`,
/// and it did the job it was written for. `EER: UNMEASURED` was honest under
/// exactly one condition — this build of `yap-diarize` has no inference
/// backend, so no machine anywhere can produce a CAM++ embedding — and it
/// asserted that condition still held. **YV122 merged, the condition stopped
/// holding, and the assertion went red on the rebase.** The EER was then
/// measured on a corpus machine and written into the backlog's MEASURED block,
/// which is what closes OS-8's deferred half.
///
/// What is left to guard is the inverse, and it is worth guarding: the numbers
/// in that block are only worth the backend that produced them. So the same
/// probe now runs with the opposite expectation. `load_backend` checks that
/// both paths EXIST before it looks at them, so two ordinary files that are not
/// models separate the answers cleanly — `model_not_found` would mean the probe
/// itself was wrong, and `model_load_failed` means a real backend looked at
/// real bytes and rejected them. That is the answer a build WITH sherpa-onnx in
/// it gives, and it is the answer this test requires.
///
/// If the backend is ever compiled back out — a feature flag, a dependency
/// dropped, a sidecar rebuilt from an older tree — this goes red on a machine
/// with no corpus and no model in a fraction of a second, and the EER numbers
/// in the backlog have to be re-earned rather than quietly inherited. It is
/// deliberately NOT keyed on `no_backend`: that tag was retired from
/// `diarize_protocol.rs` by YV122, so naming it here would be naming a string
/// no build can produce.
#[test]
fn meeting_eval_anti_alias_eer_measurement_stays_backed_by_a_real_backend() {
    use wilson_voice_lib::diarize::{DiarizeError, DiarizePool};
    use wilson_voice_lib::diarize_protocol::{ERR_MODEL_LOAD_FAILED, ERR_MODEL_NOT_FOUND};

    let dir = support::temp_dir("yv124-backend-probe");
    let segmentation = dir.join("not-a-segmentation-model.onnx");
    let embedding = dir.join("not-an-embedding-model.onnx");
    for path in [&segmentation, &embedding] {
        std::fs::write(path, b"not a model").expect("write probe file");
    }

    let pool = DiarizePool::new(
        support::diarize::launcher(),
        std::time::Duration::from_secs(10),
    );
    let answer = pool.load_models(&segmentation, &embedding);
    let status = pool.status();
    pool.shutdown();

    match answer {
        Err(DiarizeError::Refused(tag)) if tag == ERR_MODEL_LOAD_FAILED => {
            println!(
                "meeting_eval anti_alias_eer backend_probe={ERR_MODEL_LOAD_FAILED} — a real \
                 inference backend read two non-model files and rejected them, so the EER \
                 numbers in YV124's MEASURED block still have a backend behind them"
            );
        }
        Err(DiarizeError::Refused(tag)) if tag == ERR_MODEL_NOT_FOUND => panic!(
            "the probe is broken, not the build: `yap-diarize` answered `{tag}` for two \
             files this test just wrote to {dir:?}. Nothing about the backend was tested. \
             Fix the probe before reading anything into a green run here."
        ),
        Err(DiarizeError::Refused(tag)) => panic!(
            "`yap-diarize` answered `{tag}` where a build with an inference backend answers \
             `{ERR_MODEL_LOAD_FAILED}`. If the backend has been compiled out, YV124's \
             MEASURED block is quoting numbers no machine can now reproduce — either put \
             the backend back, or re-open `EER: UNMEASURED` in the backlog and say why."
        ),
        Ok(embedding_dim) => panic!(
            "`yap-diarize` LOADED a backend from two files that are not models and reported \
             {embedding_dim}-dimensional embeddings. Whatever that build is doing, it is not \
             reading the bytes it was handed, and no embedding it produces can be trusted — \
             YV124's measured EER included."
        ),
        Err(other) => panic!(
            "the backend probe got no answer at all: {other:?} (status {status:?}). A spawn \
             failure, a missed deadline or a garbled frame is a defect in the sidecar, not a \
             machine without a model — and while it stands, nothing here can vouch for the \
             measured EER either."
        ),
    }
}

/// **Overlap removal, on fixture (f)'s real RTTM shape.**
///
/// The arm slices fixture (f) by label, and the labels are what decide which
/// audio is one person talking. This pins the arithmetic on a hand-checked
/// miniature — two speakers with a known overlap, one turn that survives whole,
/// one that survives trimmed and one that does not survive at all — so
/// [`unoverlapped_spans`] cannot quietly start returning overlapped audio.
#[test]
fn meeting_eval_anti_alias_eer_overlapped_frames_are_removed_by_label() {
    let turns = vec![
        // Untouched: nobody else is talking.
        RttmTurn::new("spk_a", 0.0, 4.0),
        // Trimmed at 12.0 by spk_b, 3.0 s survives.
        RttmTurn::new("spk_a", 9.0, 13.0),
        RttmTurn::new("spk_b", 12.0, 16.0),
        // Buried: spk_b covers all but 0.5 s of it, on both sides.
        RttmTurn::new("spk_b", 20.0, 24.0),
        RttmTurn::new("spk_a", 20.5, 23.5),
    ];
    // With the floor dropped low enough to see everything, the trim leaves the
    // one-speaker stretches and nothing else. `spk_b 20.0..24.0` is left with
    // two 0.5 s remnants and reports the first; `spk_a 20.5..23.5` is buried
    // entirely and reports nothing at all.
    let kept = unoverlapped_spans(&turns, 0.4);
    let rendered: Vec<String> = kept
        .iter()
        .map(|(who, from, to)| format!("{who} {from:.1}..{to:.1}"))
        .collect();
    assert_eq!(
        rendered,
        vec![
            "spk_a 0.0..4.0".to_string(),
            "spk_a 9.0..12.0".to_string(),
            "spk_b 13.0..16.0".to_string(),
            "spk_b 20.0..20.5".to_string(),
        ],
        "the trim did not leave exactly the one-speaker stretches"
    );
    // …and that 0.5 s remnant is only in the list because that call asked for
    // it. The arm's own floor drops it, which is the floor's whole job.
    let at_arm_floor = unoverlapped_spans(&turns, ARM_MIN_UTTERANCE_SECONDS);
    assert_eq!(at_arm_floor.len(), 3);
    assert!(at_arm_floor
        .iter()
        .all(|(_, from, to)| to - from >= ARM_MIN_UTTERANCE_SECONDS));

    // Nothing is trimmed on audio with no overlap at all — the arm's other
    // fixture must be unaffected by this whole mechanism.
    let sequential = vec![
        RttmTurn::new("spk_a", 0.0, 3.0),
        RttmTurn::new("spk_b", 3.0, 6.0),
        RttmTurn::new("spk_a", 6.0, 9.0),
    ];
    let kept = unoverlapped_spans(&sequential, ARM_MIN_UTTERANCE_SECONDS);
    assert_eq!(kept.len(), sequential.len());
    for (turn, (who, from, to)) in sequential.iter().zip(kept.iter()) {
        assert_eq!(
            (&turn.speaker_id, turn.start_seconds, turn.end_seconds),
            (who, *from, *to)
        );
    }
}

/// **A speaker does not contaminate himself.**
///
/// A review advisory, accepted and fixed: [`unoverlapped_spans`] used to mask
/// on turn INDEX, so two back-to-back turns by the same person blocked each
/// other. The error is in the safe direction — clean audio thrown away, never
/// dirty audio kept — which is exactly why it needed a test rather than a
/// second reading: nothing it did could ever turn an arm red.
///
/// The case below is the one the old code got wrong, and it cannot be
/// constructed out of either corpus fixture, which is the other half of the
/// advisory's answer: on `room-3-near-field` and `classroom-6-far-field` the
/// two definitions return identical spans (no speaker in either RTTM overlaps
/// himself), so this correction moved no number this item publishes. It is here
/// so the next fixture — a real recording, where one person's turns really do
/// abut — does not silently lose most of its speech.
#[test]
fn meeting_eval_anti_alias_eer_a_speaker_does_not_contaminate_himself() {
    // One person, talking for six seconds, split across two RTTM rows that
    // overlap by a second. It is one voice throughout: masking either row
    // against the other removes audio that is already this speaker's own.
    let self_overlap = vec![
        RttmTurn::new("spk_a", 0.0, 4.0),
        RttmTurn::new("spk_a", 3.0, 6.0),
    ];
    let kept = unoverlapped_spans(&self_overlap, ARM_MIN_UTTERANCE_SECONDS);
    let rendered: Vec<String> = kept
        .iter()
        .map(|(who, from, to)| format!("{who} {from:.1}..{to:.1}"))
        .collect();
    assert_eq!(
        rendered,
        vec!["spk_a 0.0..4.0".to_string(), "spk_a 3.0..6.0".to_string()],
        "a speaker's own neighbouring turn was treated as an interruption"
    );

    // Non-vacuity: the very same shape with the second row spoken by somebody
    // else DOES get trimmed, so the assertion above is about who is speaking
    // and not about the mask having been switched off.
    let other_overlap = vec![
        RttmTurn::new("spk_a", 0.0, 4.0),
        RttmTurn::new("spk_b", 3.0, 6.0),
    ];
    let kept = unoverlapped_spans(&other_overlap, ARM_MIN_UTTERANCE_SECONDS);
    let rendered: Vec<String> = kept
        .iter()
        .map(|(who, from, to)| format!("{who} {from:.1}..{to:.1}"))
        .collect();
    assert_eq!(
        rendered,
        vec!["spk_a 0.0..3.0".to_string(), "spk_b 4.0..6.0".to_string()],
        "a second speaker's overlap must still be cut out"
    );
}

/// The pair labelling, which is where an EER arm silently goes wrong: a genuine
/// list that quietly contains cross-speaker pairs reports a beautiful EER for
/// any embedder at all.
#[test]
fn meeting_eval_anti_alias_eer_pairs_are_labeled_by_speaker() {
    let embeddings = synthetic_embeddings(0.0);
    let speakers = SYNTHETIC_SPEAKERS;
    let per_speaker = SYNTHETIC_UTTERANCES;
    let (genuine, impostor) = genuine_and_impostor(&embeddings);

    // Every unordered pair is used exactly once, and split the one way the
    // fixture's own labels allow.
    let n = speakers * per_speaker;
    assert_eq!(
        genuine.len(),
        speakers * per_speaker * (per_speaker - 1) / 2
    );
    assert_eq!(impostor.len(), n * (n - 1) / 2 - genuine.len());

    // The labels are the speakers', not the scores': re-derive the split by
    // brute force from the ids and check it agrees pair for pair.
    let mut expected_genuine = 0usize;
    for (i, (a, _)) in embeddings.iter().enumerate() {
        for (b, _) in embeddings.iter().skip(i + 1) {
            if a == b {
                expected_genuine += 1;
            }
        }
    }
    assert_eq!(genuine.len(), expected_genuine);

    // And a mislabelled distribution — one impostor pair smuggled into the
    // genuine list — moves the number, so the assertion above is load-bearing.
    let mut smuggled = genuine.clone();
    smuggled.push(*impostor.first().expect("impostor pairs exist"));
    assert_ne!(
        enrollment_eer(&genuine, &impostor).eer,
        enrollment_eer(&smuggled, &impostor).eer,
        "a cross-speaker pair in the genuine list changes nothing, so the labels \
         are not being read"
    );
}

const SYNTHETIC_SPEAKERS: usize = 3;
const SYNTHETIC_UTTERANCES: usize = 4;
/// Embedding width for the synthetic vectors. Not the shipped CAM++ width and
/// deliberately not equal to it — nothing in this file may look like a claim
/// about the model's geometry (`models.rs` records why that number lives only
/// where the graph is loaded).
const SYNTHETIC_DIM: usize = 32;

/// How far one utterance of a speaker sits from that speaker's own centroid,
/// as a fraction of the centroid's own scale. Small enough that `common_mode
/// = 0` separates perfectly, large enough that a collapsed identity component
/// leaves nothing behind.
const SYNTHETIC_JITTER: f32 = 0.25;

/// `SYNTHETIC_SPEAKERS × SYNTHETIC_UTTERANCES` labeled vectors: a per-speaker
/// direction, per-utterance jitter, and `common_mode` — the fraction of each
/// vector that is one direction SHARED by every speaker rather than that
/// speaker's own.
///
/// It is a mix rather than an addition because that is what the fold does: the
/// aliased energy does not sit alongside the identity information, it lands on
/// top of the formant region the embedding reads, so the identity component
/// shrinks as the shared one grows. `common_mode = 0` is three well-separated
/// speakers; `common_mode = 0.9` is nine parts shared noise to one part person.
///
/// Deterministic (a fixed xorshift, the same trick [`room_tone`] uses) because
/// a test that reports an EER off an RNG reports a different number on every
/// machine.
fn synthetic_embeddings(common_mode: f32) -> Vec<(String, Vec<f32>)> {
    let mut state = 0x5eed_1234u32;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        (state as f32 / u32::MAX as f32) * 2.0 - 1.0
    };
    let shared: Vec<f32> = (0..SYNTHETIC_DIM).map(|_| next()).collect();
    let mut out = Vec::new();
    for s in 0..SYNTHETIC_SPEAKERS {
        let centroid: Vec<f32> = (0..SYNTHETIC_DIM).map(|_| next()).collect();
        for _ in 0..SYNTHETIC_UTTERANCES {
            let vector: Vec<f32> = centroid
                .iter()
                .zip(shared.iter())
                .map(|(c, k)| c * (1.0 - common_mode) + k * common_mode + next() * SYNTHETIC_JITTER)
                .collect();
            out.push((format!("spk_{s}"), vector));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The generator — synthetic audio only, run by hand, never in CI
// ---------------------------------------------------------------------------

/// The voice the SINGLE-speaker fixtures (a)-(d) are rendered with. `say` output
/// is stable for a given macOS + voice build, and NOT across them: regenerating
/// on a different machine legitimately changes every sha256, which is why the
/// manifest writer is a separate one-liner below.
///
/// The two diarization fixtures do NOT use this constant — they render each
/// speaker with that speaker's own voice from [`ROOM_3_SPEAKERS`] /
/// [`CLASSROOM_6_SPEAKERS`], through [`synthesize_with`]. A multi-speaker
/// fixture rendered with one voice is one person wearing several names, and an
/// L2-normalised cosine embedding is invariant to the per-speaker gain that
/// would be the only thing left distinguishing them.
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

/// Render `text` in the corpus's default [`VOICE`] at `rate_hz`. The
/// single-speaker fixtures (a)-(d) go through here, and their bytes — and so
/// their committed sha256s — are exactly what they were before
/// [`synthesize_with`] existed.
fn synthesize(text: &str, rate_hz: u32) -> Vec<i16> {
    synthesize_with(text, VOICE, rate_hz)
}

/// Render `text` with the system speech synthesizer at `rate_hz`, mono 16-bit,
/// in the named `say` voice. Nothing leaves the machine and no third-party asset
/// is involved.
///
/// The voice is a parameter and not a constant because the diarization fixtures
/// are the only place in this corpus where WHO is speaking is the measured
/// quantity.
///
/// **`say -v` does not fail on a voice this machine lacks** — it exits 0 and
/// renders with a fallback (measured under YV122; two different nonsense voice
/// names produce byte-identical audio). What protects THIS corpus is not the
/// exit code but the committed hashes: a roster that collapsed would change the
/// generated bytes, and `meeting_eval_corpus_matches_committed_sha256s` fails
/// on the mismatch. A fixture generated fresh with no committed hash to check
/// against has no such protection and must assert the roster itself — see
/// `support/diarize_models.rs::assert_voices_are_distinct`.
fn synthesize_with(text: &str, voice: &str, rate_hz: u32) -> Vec<i16> {
    let scratch = std::env::temp_dir().join("yap-meeting-eval-gen");
    fs::create_dir_all(&scratch).expect("scratch dir");
    let aiff = scratch.join("utterance.aiff");
    let wav = scratch.join("utterance.wav");
    let _ = fs::remove_file(&aiff);
    let _ = fs::remove_file(&wav);

    let say = Command::new("say")
        .args(["-v", voice, "-r", &WORDS_PER_MINUTE.to_string(), "-o"])
        .arg(&aiff)
        .arg(text)
        .status()
        .expect("`say` is a macOS built-in");
    assert!(say.success(), "say -v {voice} failed on: {text}");

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
    generate_room_3(&root);
    generate_classroom_6(&root);

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
        rttm: Vec::new(),
        speakers: Vec::new(),
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
        rttm: Vec::new(),
        speakers: Vec::new(),
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
        rttm: Vec::new(),
        speakers: Vec::new(),
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
        rttm: Vec::new(),
        speakers: Vec::new(),
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
// YV120 — the diarization fixture writers
// ---------------------------------------------------------------------------

/// Fixture (e)'s script: twelve turns, three people, never the same person
/// twice in a row. Mundane and about nobody, same rule as the rest of this
/// corpus — no names, no digits, no addresses.
const ROOM_3_TURNS: [(usize, &str); 12] = [
    (
        0,
        "Let us start with the part everyone said was confusing last time",
    ),
    (
        1,
        "I read it twice and the second half still does not follow from the first",
    ),
    (
        2,
        "The example in the middle is the one that finally made it click for me",
    ),
    (
        0,
        "Then we lead with the example and keep the longer argument for later",
    ),
    (
        2,
        "That would also give us room to cut the closing section entirely",
    ),
    (
        1,
        "I would rather shorten it than rewrite it the week before we ship",
    ),
    (
        0,
        "Fine, we shorten it, and we leave the appendix exactly as it is",
    ),
    (
        1,
        "Someone still has to check the numbers in the middle table",
    ),
    (
        2,
        "I can take that this week if nobody else has picked it up already",
    ),
    (
        0,
        "Take it, and tell us on the call if anything in there looks wrong",
    ),
    (
        1,
        "One more thing before we stop, the room is booked for an hour only",
    ),
    (
        2,
        "Then we finish here and carry whatever is left to the next session",
    ),
];

/// Fixture (f)'s script: `(start on the shared clock, speaker, sentence)`.
///
/// The starts are chosen, not accumulated, which is what makes the crosstalk
/// deliberate rather than emergent. Three of them (the burst at twenty seconds)
/// begin within a second of each other, so three voices are live at once — past
/// pyannote-segmentation-3.0's 2-simultaneous ceiling — and a fourth speaker
/// inside the same ten-second window puts it past the 3-per-window ceiling too.
/// The generator MEASURES both before writing the fixture, so a script edit that
/// accidentally made the fixture easy fails at generation time rather than
/// silently turning fixture (f) into a second fixture (e).
const CLASSROOM_6_TURNS: [(f64, usize, &str); 21] = [
    (
        0.5,
        0,
        "The reading for today is the short chapter, not the one on the syllabus",
    ),
    (6.0, 1, "Does that mean the problem set moves as well"),
    (
        10.0,
        0,
        "It moves, and I will say so again at the end so nobody misses it",
    ),
    (
        14.5,
        2,
        "Could you go back to the diagram from the last session",
    ),
    (
        20.0,
        3,
        "I still do not see where the second term comes from",
    ),
    (21.0, 4, "It comes from the substitution two lines above it"),
    (
        22.2,
        5,
        "That is what I said and nobody listened to me either",
    ),
    (
        26.5,
        0,
        "One at a time please, the back of the room cannot hear any of this",
    ),
    (
        31.0,
        1,
        "Sorry, my question was whether the substitution is even allowed there",
    ),
    (
        35.5,
        2,
        "It is allowed as long as nothing in the denominator goes to zero",
    ),
    (
        40.0,
        0,
        "That is the condition, and it is the one people forget on the exam",
    ),
    (
        45.0,
        3,
        "So the whole method falls apart the moment the room is not ideal",
    ),
    (
        48.5,
        4,
        "Not falls apart, it just needs the correction term we skipped",
    ),
    (
        49.5,
        5,
        "Which is the part that never fits on one page of notes",
    ),
    (
        54.0,
        0,
        "We will do the correction term properly on the board next week",
    ),
    (59.0, 1, "Can we have the worked example before then"),
    (
        62.5,
        2,
        "And maybe one that is not the same example as in the book",
    ),
    (
        63.5,
        3,
        "The book example is the only one I actually understood",
    ),
    (
        68.0,
        0,
        "I will write a new one and post both of them together",
    ),
    (
        73.0,
        4,
        "Thank you, that is all I wanted to ask about today",
    ),
    // The back row gets the last word, alone. Speaker 5's other two turns both
    // land inside the crosstalk bursts on purpose, which left them with under a
    // second of un-overlapped speech in the whole fixture — not enough for
    // anything, human or model, to characterise that voice from. A speaker who
    // is never audible by themselves cannot be enrolled and cannot be measured,
    // and a fixture that contains one is quietly a five-speaker fixture.
    (
        77.0,
        5,
        "One more from the back, is the correction term going to be on the exam",
    ),
];

/// Early reflections of a hard-surfaced room: three taps, no one of them a
/// multiple of another, so the comb they make has no single deep null that a
/// later spectral measurement could mistake for the anti-alias filter's work.
const CLASSROOM_REFLECTIONS: [(f64, f32); 3] = [(0.013, 0.35), (0.023, 0.25), (0.037, 0.18)];

/// Room tone level for each fixture, as a peak fraction of full scale. Fixture
/// (e) is near-field and nearly clean; fixture (f) carries the HVAC-and-laptops
/// floor of a real lecture hall, which is a large part of why far-field
/// embeddings are worse.
const ROOM_3_TONE: f32 = 0.0015;
const CLASSROOM_6_TONE: f32 = 0.010;

/// Deterministic room tone: filtered noise plus a low hum, from a fixed seed.
///
/// Deterministic because the manifest hashes this corpus — a random noise floor
/// would change every sha256 on every regeneration and make the checksum file
/// meaningless. An xorshift and two fixed-phase tones are enough to sound like
/// a room and cost nothing to reproduce.
fn room_tone(len: usize, seed: u32, level: f32) -> Vec<f32> {
    let mut state = seed | 1;
    let mut low = 0.0f32;
    (0..len)
        .map(|i| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let white = (state as f32 / u32::MAX as f32) * 2.0 - 1.0;
            // One-pole lowpass: HVAC is not white.
            low = low * 0.85 + white * 0.15;
            let t = i as f32 / TARGET_RATE as f32;
            let hum = (2.0 * std::f32::consts::PI * 118.0 * t).sin() * 0.35
                + (2.0 * std::f32::consts::PI * 236.0 * t + 1.1).sin() * 0.15;
            (low * 2.2 + hum) * level
        })
        .collect()
}

/// One speaker's utterance as the single room mic hears it: attenuated by
/// distance, plus [`CLASSROOM_REFLECTIONS`].
fn far_field(samples: &[i16], gain: f32) -> Vec<f32> {
    let direct: Vec<f32> = samples
        .iter()
        .map(|s| *s as f32 / 32_768.0 * gain)
        .collect();
    let tail = CLASSROOM_REFLECTIONS
        .iter()
        .map(|(delay, _)| (delay * TARGET_RATE as f64) as usize)
        .max()
        .unwrap_or(0);
    let mut out = vec![0.0f32; direct.len() + tail];
    out[..direct.len()].copy_from_slice(&direct);
    for (delay, reflection_gain) in CLASSROOM_REFLECTIONS {
        let offset = (delay * TARGET_RATE as f64) as usize;
        for (i, s) in direct.iter().enumerate() {
            out[i + offset] += s * reflection_gain;
        }
    }
    out
}

/// Write a diarization fixture: the mixed audio, one `speaker: text` line per
/// turn in start order, and the meta carrying the RTTM.
///
/// `reference.txt` is speaker-prefixed for the same reason fixture (d)'s is:
/// the reference for a multi-speaker recording is not a bag of words, it is who
/// said what, and a file that dropped the speaker would be unable to express
/// the thing the fixture exists to check.
fn write_diarization_fixture(root: &Path, meta: &FixtureMeta, audio: &[f32]) {
    let dir = root.join(&meta.id);
    fs::create_dir_all(&dir).expect("fixture dir");
    write_wav_16k_mono(&dir.join("audio.wav"), &to_i16(audio));
    let reference: String = meta
        .utterances
        .iter()
        .map(|u| u.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
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
        "wrote {} — {:.1}s, {} turns, {} speakers, peak {} simultaneous",
        dir.display(),
        meta.duration_seconds,
        meta.rttm.len(),
        meta.speakers.len(),
        max_simultaneous(&meta.rttm)
    );
}

/// Fixture (e): three people round one mic in a small room, near-field, no
/// overlap. The case the segmentation model handles, and therefore the only
/// honest place to tune a clustering threshold (YV126).
fn generate_room_3(root: &Path) {
    let gap = (SENTENCE_GAP * TARGET_RATE as f64) as usize;
    let mut mix: Vec<f32> = Vec::new();
    let mut rttm: Vec<RttmTurn> = Vec::new();
    let mut utterances: Vec<Utterance> = Vec::new();

    for (who, text) in ROOM_3_TURNS {
        let (id, voice, gain) = ROOM_3_SPEAKERS[who];
        let spoken = trim_silence(&synthesize_with(text, voice, TARGET_RATE)).to_vec();
        let start = seconds(mix.len());
        mix.extend(spoken.iter().map(|s| *s as f32 / 32_768.0 * gain));
        let end = seconds(mix.len());
        mix.extend(std::iter::repeat_n(0.0f32, gap));
        rttm.push(RttmTurn::new(id, start, end));
        utterances.push(Utterance {
            text: format!("{id}: {text}"),
            start_seconds: start,
            end_seconds: end,
        });
    }

    let tone = room_tone(mix.len(), 0x5EED_C0DE, ROOM_3_TONE);
    for (s, n) in mix.iter_mut().zip(tone.iter()) {
        *s = (*s + *n).clamp(-1.0, 1.0);
    }

    // Hard by construction, checked before it is written: fixture (e) is the
    // NO-OVERLAP case, and a generator that produced overlap here would hand
    // YV126 a threshold tuned against the mechanism ceiling.
    assert_eq!(max_simultaneous(&rttm), 1, "fixture (e) must not overlap");
    assert!(max_speakers_in_window(&rttm, 10.0) <= 3);

    let meta = FixtureMeta {
        id: ROOM_3.to_string(),
        kind: "conference_room_3_near_field".to_string(),
        sample_rate: TARGET_RATE,
        duration_seconds: seconds(mix.len()),
        utterances,
        boundary_seconds: Vec::new(),
        seam_keywords: Vec::new(),
        marker_spans: Vec::new(),
        chunk_seconds: None,
        chunk_overlap_seconds: None,
        device_change_seconds: None,
        source_rates_hz: vec![TARGET_RATE],
        two_track: None,
        rttm,
        speakers: ROOM_3_SPEAKERS
            .iter()
            .map(|(id, voice, gain)| FixtureSpeaker {
                id: (*id).to_string(),
                voice: (*voice).to_string(),
                direct_gain: *gain,
            })
            .collect(),
    };
    write_diarization_fixture(root, &meta, &mix);
}

/// Fixture (f): six people in a lecture hall, one far-field mic, deliberate
/// crosstalk — engineered to break full N-way clustering.
///
/// Every voice is attenuated by its distance and carries the room's early
/// reflections, and the whole mix sits on an HVAC floor. That is not decoration:
/// far-field is where speaker embeddings degrade (it is why OS-8's aliasing bug
/// mattered at all), and a "six speaker" fixture recorded as six clean
/// near-field voices would be an easy fixture wearing a hard fixture's name.
fn generate_classroom_6(root: &Path) {
    let mut rendered: Vec<(f64, usize, &str, Vec<i16>)> = Vec::new();
    for (start, who, text) in CLASSROOM_6_TURNS {
        let (_, voice, _) = CLASSROOM_6_SPEAKERS[who];
        let spoken = trim_silence(&synthesize_with(text, voice, TARGET_RATE)).to_vec();
        rendered.push((start, who, text, spoken));
    }

    let total_seconds = rendered
        .iter()
        .map(|(start, _, _, spoken)| start + seconds(spoken.len()))
        .fold(0.0f64, f64::max)
        + 1.0;
    let mut mix = vec![0.0f32; (total_seconds * TARGET_RATE as f64) as usize];
    let mut rttm: Vec<RttmTurn> = Vec::new();
    let mut utterances: Vec<Utterance> = Vec::new();

    for (start, who, text, spoken) in &rendered {
        let (id, _, gain) = CLASSROOM_6_SPEAKERS[*who];
        let at = (start * TARGET_RATE as f64) as usize;
        let wet = far_field(spoken, gain);
        for (i, s) in wet.iter().enumerate() {
            if at + i < mix.len() {
                mix[at + i] += s;
            }
        }
        // The TURN is the dry speech, not its reverb tail: the tail is what the
        // room did, and charging a diarizer for failing to attribute a decaying
        // reflection would be scoring the fixture's own reverb.
        let end = start + seconds(spoken.len());
        rttm.push(RttmTurn::new(id, *start, end));
        utterances.push(Utterance {
            text: format!("{id}: {text}"),
            start_seconds: *start,
            end_seconds: end,
        });
    }

    let tone = room_tone(mix.len(), 0xC0FF_EE11, CLASSROOM_6_TONE);
    for (s, n) in mix.iter_mut().zip(tone.iter()) {
        *s = (*s + *n).clamp(-1.0, 1.0);
    }

    rttm.sort_by(|a, b| a.start_seconds.total_cmp(&b.start_seconds));
    utterances.sort_by(|a, b| a.start_seconds.total_cmp(&b.start_seconds));

    // Hard by construction, checked before it is written (merged finding #5).
    let simultaneous = max_simultaneous(&rttm);
    let in_ten = max_speakers_in_window(&rttm, 10.0);
    assert!(
        simultaneous >= 3,
        "fixture (f) came out with only {simultaneous} simultaneous speakers — \
         the script's starts no longer overlap after synthesis"
    );
    assert!(
        in_ten >= 4,
        "fixture (f) came out with only {in_ten} distinct speakers in a 10s window"
    );

    let meta = FixtureMeta {
        id: CLASSROOM_6.to_string(),
        kind: "classroom_6_far_field_overlap".to_string(),
        sample_rate: TARGET_RATE,
        duration_seconds: seconds(mix.len()),
        utterances,
        boundary_seconds: Vec::new(),
        seam_keywords: Vec::new(),
        marker_spans: Vec::new(),
        chunk_seconds: None,
        chunk_overlap_seconds: None,
        device_change_seconds: None,
        source_rates_hz: vec![TARGET_RATE],
        two_track: None,
        rttm,
        speakers: CLASSROOM_6_SPEAKERS
            .iter()
            .map(|(id, voice, gain)| FixtureSpeaker {
                id: (*id).to_string(),
                voice: (*voice).to_string(),
                direct_gain: *gain,
            })
            .collect(),
    };
    write_diarization_fixture(root, &meta, &mix);
}

/// Regrow ONLY the two diarization fixtures, then re-hash. Their geometry is
/// tied to the scripts above and to the installed speech voices, not to the
/// chunk geometry, so they are regrown on their own — rebuilding the 15-minute
/// lecture beside them would change its hashes and invalidate a measured WER
/// for no reason.
#[test]
#[ignore = "writer, not a check: renders both diarization fixtures with `say`"]
fn meeting_eval_generate_diarization_fixtures() {
    let root = corpus_root();
    fs::create_dir_all(&root).expect("corpus root");
    generate_room_3(&root);
    generate_classroom_6(&root);
    write_manifest_from(&root);
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

// ═══════════════════════════════════════════════════════════════════════════
// YV126 — the clustering gates on fixtures (e) and (f)
// ═══════════════════════════════════════════════════════════════════════════
//
// Two different kinds of number live below, and conflating them would be the
// whole failure this backlog's eval-first sequencing exists to prevent.
//
//   **A MEASUREMENT** is what the pipeline scored on real audio. It needs an
//   embedding extractor, which needs the sidecar YV122 (#137) put a real
//   backend in and the two catalog models on disk. Both exist, so both gates
//   below carry measured numbers — `tune_clustering_threshold` (19 distances on
//   fixture (e)) and `tune_enrollment_band` (19 distances x 19 bands on fixture
//   (f)), both driving the shipped `cluster_track`. A machine without the models
//   prints `DIARIZER_ABSENT` and checks nothing, which is CI's state and is why
//   `meeting_eval_diarization_gates_carry_the_configuration_that_produced_them`
//   exists to check the gates structurally there.
//
//   **A measurement is not a quality claim, and this corpus makes that
//   unusually important.** YV122 and YV124 both measured that these fixtures'
//   voices are the Mac's `say` synthesiser and that CAM++ hears the synthesiser
//   rather than the persona (EER 0.272 here against <1 % on VoxCeleb). The
//   numbers below are REGRESSION floors for this pipeline on this corpus. They
//   are not what Yap does to human speech, and YV124's conclusion — that the
//   thresholds deciding whether two clips are the same person have to be set on
//   real speech — is untouched by them.
//
//   **A MECHANISM CEILING** is what the pipeline could score at BEST, computed
//   from the fixture's own ground truth and the one thing the mechanism is
//   documented to do: sherpa's pipeline deletes every overlapped frame before
//   embedding (merged finding #5), so no clusterer — perfect or otherwise —
//   can attribute speech in those frames. That is arithmetic over an RTTM with
//   YV120's own `der`, it needs no model, and it is falsifiable today. It is
//   NOT a measurement of this pipeline and is never reported as one.
//
// The ceiling is what makes finding #5's reframe checkable now: if the 2-class
// task's ceiling on fixture (f) is materially better than full clustering's,
// the reframe is right for a reason that has nothing to do with tuning. If it
// were not, the reframe would be an opinion and this file would say so.

/// Printed when the measured arm cannot run. Verbatim, so it is greppable in a
/// CI log the way [`CORPUS_ABSENT`] is.
///
/// Since YV122 (#137) there is exactly one honest reason for this on a machine
/// that has the corpus: the catalog's two diarization models are not installed.
/// CI is such a machine — it has neither corpus nor models — which is why the
/// gates below are checked against a recorded measurement rather than produced
/// by one on every runner.
const DIARIZER_ABSENT: &str =
    "no diarization models installed on this machine, skipping the measured arm";

/// The candidate thresholds a tuning run sweeps, as cosine DISTANCES.
///
/// A grid, not a guess: the point of a sweep is that the winner is chosen by
/// the harness rather than written down in advance. It spans the whole usable
/// range — 0.05 (only near-identical turns merge) to 0.95 (almost everything
/// does) — so a tuned value cannot land on an edge without that being visible.
const THRESHOLD_SWEEP: [f32; 19] = [
    0.05, 0.10, 0.15, 0.20, 0.25, 0.30, 0.35, 0.40, 0.45, 0.50, 0.55, 0.60, 0.65, 0.70, 0.75, 0.80,
    0.85, 0.90, 0.95,
];

/// The candidate acceptance bands a tuning run sweeps for the 2-class task, as
/// cosine SIMILARITIES.
///
/// Binary mode is a different task with a different unit, so it gets a sweep of
/// its own rather than borrowing the clustering distance. That is not a detail:
/// a first cut of this item ran fixture (f)'s binary arm at a hard-coded `0.35`
/// — a number that was neither tuned for the 2-class task nor even in the right
/// unit for it — which is the vendor-blog threshold this backlog forbids,
/// arrived at by inattention rather than by citation.
///
/// The grid spans the usable half of the similarity range. Below 0.0 a cosine
/// band accepts turns pointing AWAY from the enrolled voice, which is not a
/// decision anybody wants tuned; the top end goes to 0.95 so a winner cannot
/// land on the edge without that being visible.
const SIMILARITY_SWEEP: [f32; 19] = [
    0.05, 0.10, 0.15, 0.20, 0.25, 0.30, 0.35, 0.40, 0.45, 0.50, 0.55, 0.60, 0.65, 0.70, 0.75, 0.80,
    0.85, 0.90, 0.95,
];

/// Every interval in which two or more speakers are talking at once.
///
/// These are the frames sherpa's `ExcludeOverlap` deletes before embedding, and
/// therefore the frames no clustering configuration can attribute.
fn overlapped_regions(turns: &[RttmTurn]) -> Vec<(f64, f64)> {
    let mut events: Vec<(f64, i32)> = Vec::with_capacity(turns.len() * 2);
    for t in turns {
        if t.duration() <= 0.0 {
            continue;
        }
        events.push((t.start_seconds, 1));
        events.push((t.end_seconds, -1));
    }
    events.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut here = 0i32;
    let mut last = 0.0f64;
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (at, delta) in events {
        if here > 1 && at > last {
            match out.last_mut() {
                Some(previous) if (previous.1 - last).abs() < 1e-12 => previous.1 = at,
                _ => out.push((last, at)),
            }
        }
        here += delta;
        last = at;
    }
    out
}

/// `spans` with every part of `cuts` removed.
fn subtract(spans: &[(f64, f64)], cuts: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for (start, end) in spans {
        let mut pieces = vec![(*start, *end)];
        for (cut_start, cut_end) in cuts {
            let mut next = Vec::new();
            for (a, b) in pieces {
                if *cut_end <= a || *cut_start >= b {
                    next.push((a, b));
                    continue;
                }
                if a < *cut_start {
                    next.push((a, *cut_start));
                }
                if *cut_end < b {
                    next.push((*cut_end, b));
                }
            }
            pieces = next;
        }
        out.extend(pieces);
    }
    out
}

/// The best hypothesis the MECHANISM can produce on this fixture: the ground
/// truth with every overlapped frame deleted, exactly as sherpa's pipeline
/// deletes them before embedding.
///
/// Speaker identity is perfect here, boundaries are perfect, clustering is
/// perfect. Everything this scores as error is loss the mechanism inflicts
/// before any threshold is chosen.
fn mechanism_ceiling_hypothesis(turns: &[RttmTurn]) -> Vec<RttmTurn> {
    let overlap = overlapped_regions(turns);
    let mut out = Vec::new();
    for turn in turns {
        for (start, end) in subtract(&[(turn.start_seconds, turn.end_seconds)], &overlap) {
            if end > start {
                out.push(RttmTurn::new(turn.speaker_id.clone(), start, end));
            }
        }
    }
    out
}

/// An RTTM collapsed to the 2-class task: the enrolled voice, and everyone
/// else.
fn collapse_rttm(turns: &[RttmTurn], enrolled: &str) -> Vec<RttmTurn> {
    turns
        .iter()
        .map(|t| {
            let id = if t.speaker_id == enrolled {
                "enrolled"
            } else {
                "everyone_else"
            };
            RttmTurn::new(id, t.start_seconds, t.end_seconds)
        })
        .collect()
}

/// The shipped sidecar with the catalog's two models loaded, spawned ONCE.
///
/// Reuses YV124's `support::diarize::embedder()` rather than resolving and
/// launching a second time: that seam already knows the one honest reason a
/// machine cannot embed (the models are not installed) and PANICS on everything
/// else, which is the posture an eval arm needs — a gate that quietly returns
/// early whenever anything is missing measures nothing and says nothing.
///
/// One pool for a whole sweep, not one per candidate. A previous cut spawned a
/// child and re-ran `load_models` for each of the 19 candidates on each
/// fixture; the pool is a shipped connection-holder and using it as one is both
/// faster and closer to what the app does.
fn loaded_pool() -> Option<wilson_voice_lib::diarize::DiarizePool> {
    match support::diarize::embedder() {
        support::diarize::Embedder::Ready {
            pool,
            embedding_dim,
        } => {
            eprintln!(
                "diarize backend ready: the catalog's models loaded, {embedding_dim}-dim \
                 embeddings — the measured arms below are real"
            );
            Some(pool)
        }
        missing => {
            eprintln!(
                "{DIARIZER_ABSENT} ({})",
                missing.skip_reason().unwrap_or_default()
            );
            None
        }
    }
}

/// The floor handed to every measured diarization pass, in seconds.
///
/// **Not an accuracy threshold and not tuned as one** — the same constant, the
/// same job and the same argument as the anti-alias arm's
/// [`ARM_MIN_UTTERANCE_SECONDS`], which this deliberately IS rather than
/// duplicates: YV122 made `min_embed` mandatory and defaultless, so an eval arm
/// has to name one, and naming the value the corpus's own turns already clear
/// makes it an assertion about the fixtures rather than a knob. It is not an
/// accuracy threshold in either direction: turns under it are not misattributed,
/// they are stored unattributed, and their speech shows up as MISS in the DER
/// rather than as a better score. [`assert_the_parent_clustered_this_pass`]
/// prints how many turns that was on every sweep row, so the floor's effect on
/// the tuning table is visible rather than hidden in a header.
fn measurement_floor() -> std::time::Duration {
    std::time::Duration::from_secs_f64(ARM_MIN_UTTERANCE_SECONDS)
}

/// One raw pass of the shipped child over one fixture at one distance.
///
/// Separate from [`measured_hypothesis`] because the 2-class sweep needs the
/// SAME child answer scored at nineteen different acceptance bands: the band is
/// the parent's arithmetic and never reaches the wire, so re-spawning per band
/// would be nineteen identical ONNX passes for one answer.
fn raw_turns(
    pool: &wilson_voice_lib::diarize::DiarizePool,
    root: &Path,
    fixture: &str,
    distance: f32,
) -> Option<Vec<wilson_voice_lib::diarize_protocol::DiarizeSegment>> {
    use wilson_voice_lib::diarize_metrics::CosineDistance;

    let wav = root.join(fixture).join("audio.wav");
    match pool.diarize(&wav, CosineDistance::new(distance), measurement_floor()) {
        Ok(raw) => Some(raw),
        Err(e) => {
            eprintln!("{DIARIZER_ABSENT} (diarize: {})", e.tag());
            None
        }
    }
}

/// **The soundness check every measured arm runs before it believes a number.**
///
/// `diarize::assign_clusters` falls back to the CHILD's own cluster ids when
/// **not one** turn in a pass carries a usable embedding, and logs when it does.
/// That fallback is correct behaviour for the app and wrong for a measurement:
/// a DER produced through it scores sherpa's clustering wearing this repo's
/// threshold, which is the "a number nobody here can step through" failure the
/// whole item exists to avoid.
///
/// It also prints how many turns went UNATTRIBUTED, which since YV122's
/// per-turn `min_embed` floor is a normal and load-bearing quantity: those
/// seconds are a MISS in every DER below, so the tuning table's numbers cannot
/// be read without them.
fn assert_the_parent_clustered_this_pass(
    raw: &[wilson_voice_lib::diarize_protocol::DiarizeSegment],
    fixture: &str,
    distance: f32,
) {
    let embedded = raw.iter().filter(|s| !s.embedding.is_empty()).count();
    assert!(
        embedded > 0 || raw.is_empty(),
        "{fixture} at distance {distance:.2}: none of {} turns carried an embedding, so \
         `cluster_track` fell back to the child's own cluster ids and this measurement \
         would be scoring sherpa's clustering rather than this repo's threshold",
        raw.len()
    );
    if embedded < raw.len() {
        eprintln!(
            "    ({} of {} turns are under the {:.1}s embedding floor and are stored \
             unattributed — their speech is a MISS in the DER below)",
            raw.len() - embedded,
            raw.len(),
            ARM_MIN_UTTERANCE_SECONDS
        );
    }
}

/// A REAL hypothesis for one fixture at one threshold, or `None` with the
/// reason printed.
///
/// This is the shipped path end to end — the staged sidecar, the vendored model
/// pair, `cluster_track` with the kind branch — so the numbers this file gates
/// on come from the code that ships and not from a harness-local
/// reimplementation of it.
fn measured_hypothesis(
    pool: &wilson_voice_lib::diarize::DiarizePool,
    root: &Path,
    fixture: &str,
    threshold: f32,
    mode: wilson_voice_lib::diarize::TargetMode,
) -> Option<Vec<RttmTurn>> {
    use wilson_voice_lib::diarize::{cluster_track, MeetingTracks};
    use wilson_voice_lib::diarize_metrics::CosineDistance;

    let wav = root.join(fixture).join("audio.wav");
    let result = cluster_track(
        pool,
        MeetingTracks {
            mic_wav: &wav,
            system_wav: None,
        },
        // Both diarization fixtures are one microphone in a room, which is
        // exactly what `in_person` means.
        MeetingKind::InPerson,
        CosineDistance::new(threshold),
        mode,
        measurement_floor(),
    );
    match result {
        Ok(segments) => Some(turns_of(&segments)),
        Err(e) => {
            eprintln!("{DIARIZER_ABSENT} (diarize: {})", e.tag());
            None
        }
    }
}

/// `DiarizedSegment`s as RTTM turns named `cluster_<index>`.
///
/// **An unattributed turn is dropped, on purpose.** `cluster_index: None` means
/// the pipeline made no claim about who was speaking then — a turn under
/// YV122's `min_embed` floor, with no embedding to compare. Emitting it as a
/// speaker called "none" would invent a person and pollute the confusion count;
/// dropping it lets its seconds fall to DER's MISS term, which is what "we said
/// nothing about this speech" is supposed to cost.
fn turns_of(segments: &[wilson_voice_lib::diarize::DiarizedSegment]) -> Vec<RttmTurn> {
    segments
        .iter()
        .filter_map(|s| {
            s.cluster_index.map(|cluster| {
                RttmTurn::new(format!("cluster_{cluster}"), s.start_seconds, s.end_seconds)
            })
        })
        .collect()
}

/// Enrol a fixture speaker the way a person would: from ONE span of their
/// voice, not from the answer key.
///
/// Binary mode compares each turn against an enrolled centroid, so a measured
/// run needs one, and where it comes from decides what the measurement means.
/// This takes the speaker's FIRST ground-truth span, keeps the diarizer's turns
/// that lie mostly inside it, and averages their embeddings (L2-normalised —
/// the same shape YV128's `speaker_profiles` stores). Every later span is
/// untouched and is what the DER is then scored over, so the enrollment sample
/// and the test material are disjoint: a centroid built from all of a speaker's
/// speech would be scoring the harness's knowledge of the answer, not the
/// pipeline.
///
/// The turns come from the SAME pass the labels will be scored on, because the
/// child's segmentation moves with the distance — enrolling from one distance's
/// turns and scoring another's would mix two segmentations in one number.
fn enrol_from_first_span(
    root: &Path,
    fixture: &str,
    speaker: &str,
    raw: &[wilson_voice_lib::diarize_protocol::DiarizeSegment],
) -> Option<(Vec<f32>, f64)> {
    let reference = read_rttm(root, fixture);
    let span = reference.iter().find(|t| t.speaker_id == speaker)?;
    let (span_start, span_end) = (span.start_seconds, span.end_seconds);

    let mine: Vec<&wilson_voice_lib::diarize_protocol::DiarizeSegment> = raw
        .iter()
        .filter(|s| {
            let inside = s.end.min(span_end) - s.start.max(span_start);
            inside > 0.0 && inside > (s.end - s.start) * 0.5
        })
        .filter(|s| !s.embedding.is_empty())
        .collect();
    if mine.is_empty() {
        eprintln!(
            "no diarized turn sits inside {speaker}'s first span ({span_start:.2}–{span_end:.2}s) \
             — nothing to enrol from"
        );
        return None;
    }
    let dim = mine[0].embedding.len();
    if mine.iter().any(|s| s.embedding.len() != dim) {
        eprintln!("the sidecar returned mixed embedding dimensions — refusing to enrol");
        return None;
    }
    let mut centroid = vec![0.0f32; dim];
    for turn in &mine {
        for (slot, value) in centroid.iter_mut().zip(&turn.embedding) {
            *slot += value / mine.len() as f32;
        }
    }
    let norm = centroid.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm <= f32::EPSILON {
        eprintln!("the enrolled centroid is degenerate — refusing to enrol");
        return None;
    }
    for slot in centroid.iter_mut() {
        *slot /= norm;
    }
    Some((centroid, span_end))
}

/// The 2-class hypothesis for one raw pass at one acceptance band, through the
/// shipped `label_against_enrolled`.
fn binary_hypothesis(
    raw: &[wilson_voice_lib::diarize_protocol::DiarizeSegment],
    centroid: &[f32],
    band: f32,
) -> Option<Vec<RttmTurn>> {
    use wilson_voice_lib::diarize::{label_against_enrolled, EnrolledSpeaker};
    use wilson_voice_lib::diarize_metrics::CosineSimilarity;
    use wilson_voice_lib::meetings::MIC_TRACK;

    let enrolled = EnrolledSpeaker::new(1, centroid.to_vec(), CosineSimilarity::new(band));
    match label_against_enrolled(MIC_TRACK, raw, &enrolled) {
        Ok(segments) => Some(turns_of(&segments)),
        Err(e) => {
            eprintln!("binary mode refused at band {band:.2}: {}", e.tag());
            None
        }
    }
}

/// `cluster_<ENROLLED_CLUSTER>` / everything else, renamed to the two strings
/// the collapsed reference uses so the DER compares like with like.
fn as_two_classes(turns: Vec<RttmTurn>, scored_from: f64) -> Vec<RttmTurn> {
    let enrolled = format!("cluster_{}", wilson_voice_lib::diarize::ENROLLED_CLUSTER);
    turns
        .into_iter()
        .filter(|t| t.start_seconds >= scored_from)
        .map(|t| {
            let id = if t.speaker_id == enrolled {
                "enrolled"
            } else {
                "everyone_else"
            };
            RttmTurn::new(id, t.start_seconds, t.end_seconds)
        })
        .collect()
}

/// What one 2-class sweep found: the two numbers that produced it and the two
/// it scored.
#[derive(Debug, Clone, Copy)]
struct BinaryWinner {
    distance: f32,
    band: f32,
    der: f64,
    jer: f64,
    scored_from: f64,
}

/// Sweep the clustering distance **and** the acceptance band for the 2-class
/// task on `fixture`, printing the whole table.
///
/// **Two dimensions, and the review finding that made it two.** An earlier cut
/// swept only the band, at fixture (e)'s tuned distance, on the stated ground
/// that the distance "decides nothing" in binary mode. That is true of the
/// PARENT — no cluster id decides a label here — and false of the pipeline:
/// sherpa clusters in order to segment, so the distance moves the turn set, and
/// a turn that merged two speakers gets one label whatever the band is. So the
/// distance is tuned FOR this task, on this fixture, against this reference,
/// rather than inherited from a different task on a different fixture.
///
/// Each distance costs one child pass; each band is then pure arithmetic over
/// that pass's embeddings, which is why 19 × 19 candidates cost 19 ONNX passes
/// and not 361. `binary_sweep_agrees_with_the_shipped_path` re-runs the winner
/// through `cluster_track` end to end, so the shortcut cannot drift from what
/// the app would do.
fn tune_enrollment_band(
    pool: &wilson_voice_lib::diarize::DiarizePool,
    root: &Path,
    fixture: &str,
    speaker: &str,
) -> Option<BinaryWinner> {
    let full_reference = read_rttm(root, fixture);
    let mut best: Option<BinaryWinner> = None;

    for distance in THRESHOLD_SWEEP {
        let raw = raw_turns(pool, root, fixture, distance)?;
        assert_the_parent_clustered_this_pass(&raw, fixture, distance);
        let Some((centroid, scored_from)) = enrol_from_first_span(root, fixture, speaker, &raw)
        else {
            eprintln!("  distance {distance:.2}: nothing to enrol from, skipped");
            continue;
        };
        let reference: Vec<RttmTurn> = collapse_rttm(&full_reference, speaker)
            .into_iter()
            .filter(|t| t.start_seconds >= scored_from)
            .collect();
        assert!(
            !reference.is_empty(),
            "the enrollment span swallowed the whole fixture — nothing left to score"
        );
        for band in SIMILARITY_SWEEP {
            let Some(hypothesis) = binary_hypothesis(&raw, &centroid, band) else {
                continue;
            };
            let hypothesis = as_two_classes(hypothesis, scored_from);
            let report = der(&reference, &hypothesis);
            let (rate, jaccard) = (report.rate(), jer(&reference, &hypothesis));
            eprintln!(
                "  distance {distance:.2} band {band:.2}: {} turns, DER {rate:.4} \
                 (miss {:.2}s, fa {:.2}s, conf {:.2}s), JER {jaccard:.4}",
                raw.len(),
                report.miss,
                report.false_alarm,
                report.confusion
            );
            if best.is_none_or(|b| rate < b.der) {
                best = Some(BinaryWinner {
                    distance,
                    band,
                    der: rate,
                    jer: jaccard,
                    scored_from,
                });
            }
        }
    }
    best
}

/// Sweep [`THRESHOLD_SWEEP`] against fixture (e) and return the best
/// `(threshold, DER, JER)`, printing the whole table.
///
/// This is the tuning run the backlog demands, and it is the ONLY sanctioned
/// source for `ROOM_3_DER_GATE`/`ROOM_3_JER_GATE`. Fixture (e) is the fixture
/// to tune on because it has zero overlap — its mechanism ceiling is 0.0 DER,
/// asserted below — so every error it reports belongs to the clustering
/// threshold rather than to frames the segmenter deleted.
fn tune_clustering_threshold(
    pool: &wilson_voice_lib::diarize::DiarizePool,
    root: &Path,
) -> Option<(f32, f64, f64)> {
    use wilson_voice_lib::diarize::TargetMode;

    let reference = read_rttm(root, ROOM_3);
    let mut best: Option<(f32, f64, f64)> = None;
    for threshold in THRESHOLD_SWEEP {
        let raw = raw_turns(pool, root, ROOM_3, threshold)?;
        assert_the_parent_clustered_this_pass(&raw, ROOM_3, threshold);
        let hypothesis =
            measured_hypothesis(pool, root, ROOM_3, threshold, TargetMode::FullClustering)?;
        let report = der(&reference, &hypothesis);
        let (rate, jaccard) = (report.rate(), jer(&reference, &hypothesis));
        let clusters = {
            let mut ids: Vec<&str> = hypothesis.iter().map(|t| t.speaker_id.as_str()).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len()
        };
        eprintln!(
            "  distance {threshold:.2}: {} turns, {clusters} clusters, DER {rate:.4} \
             (miss {:.2}s, fa {:.2}s, conf {:.2}s), JER {jaccard:.4}",
            hypothesis.len(),
            report.miss,
            report.false_alarm,
            report.confusion
        );
        if best.is_none_or(|(_, best_der, _)| rate < best_der) {
            best = Some((threshold, rate, jaccard));
        }
    }
    best
}

/// **Fixture (e) — the tuning fixture.** Three people, near-field, no overlap.
///
/// ```sh
/// cargo test --test meeting_eval fixture_e_der_gate -- --nocapture
/// ```
///
/// Two arms, and the difference between them is the difference between a
/// measurement and arithmetic:
///
/// * The mechanism ceiling, computable anywhere: fixture (e) has ZERO
///   overlapped speech, so sherpa's frame deletion costs it nothing and a
///   perfect clusterer would score 0.0 DER. That is what qualifies it as the
///   fixture a threshold is tuned on — nothing else can be blamed for what the
///   number turns out to be.
/// * The tuned measurement, which needs the corpus and the catalog's two
///   models. On a machine with both — the one this item's tuning table was
///   produced on — the sweep runs and the gates below are checked against it.
///   On CI, which has neither, this prints [`DIARIZER_ABSENT`] and the gates are
///   checked by
///   `meeting_eval_diarization_gates_carry_the_configuration_that_produced_them`
///   for having a recorded provenance instead.
#[test]
fn fixture_e_der_gate() {
    let Some(root) = corpus() else { return };
    let reference = read_rttm(&root, ROOM_3);

    // Arm 1 — the mechanism ceiling. Real arithmetic, no model.
    assert!(
        overlapped_regions(&reference).is_empty(),
        "fixture (e) is the NO-OVERLAP case; if it has drifted into overlap, a \
         threshold tuned on it would be compensating for the mechanism ceiling \
         instead of measuring similarity"
    );
    let ceiling = der(&reference, &mechanism_ceiling_hypothesis(&reference));
    assert!(
        ceiling.rate() < 1e-9,
        "fixture (e)'s mechanism ceiling is not zero: {ceiling:?}"
    );
    eprintln!(
        "fixture (e) mechanism ceiling: DER {:.4}, JER {:.4} — nothing the \
         segmenter deletes costs anything here, so every point of error a \
         measured run reports belongs to the clustering threshold",
        ceiling.rate(),
        jer(&reference, &mechanism_ceiling_hypothesis(&reference))
    );

    // Arm 2 — the tuned measurement.
    let Some(pool) = loaded_pool() else { return };
    eprintln!("fixture (e) threshold sweep:");
    let tuned = tune_clustering_threshold(&pool, &root);
    pool.shutdown();
    let Some((threshold, measured_der, measured_jer)) = tuned else {
        return;
    };
    eprintln!(
        "fixture (e) TUNED: clustering distance {threshold:.2} → DER {measured_der:.6}, \
         JER {measured_jer:.6}"
    );
    let (Some(der_gate), Some(jer_gate)) = (ROOM_3_DER_GATE, ROOM_3_JER_GATE) else {
        panic!(
            "a measurement is available ({measured_der:.4} DER at distance \
             {threshold:.2}) but the gates are still `None`. Record the tuning \
             table above in the backlog and set ROOM_3_DER_GATE / \
             ROOM_3_JER_GATE from it."
        );
    };
    assert!(
        measured_der <= der_gate,
        "fixture (e) DER {measured_der:.4} is worse than the recorded gate {der_gate:.4}"
    );
    assert!(
        measured_jer <= jer_gate,
        "fixture (e) JER {measured_jer:.4} is worse than the recorded gate {jer_gate:.4}"
    );
    assert!(
        ROOM_3_TUNED_DISTANCE
            .is_some_and(|recorded| (f64::from(threshold) - recorded).abs() < 1e-6),
        "the sweep's winner is distance {threshold:.2} but ROOM_3_TUNED_DISTANCE \
         records {ROOM_3_TUNED_DISTANCE:?} — the gate and the number that \
         produced it have come apart, and a gate whose provenance is stale is a \
         guess with a decimal point"
    );
}

/// **Fixture (f) — the fixture built to fail, and the reframe that answers it.**
///
/// ```sh
/// cargo test --test meeting_eval fixture_f_binary_fallback_der -- --nocapture
/// ```
///
/// Merged finding #5 says full N-way clustering cannot do a six-person
/// far-field room, and that the fix is a smaller task rather than a better
/// threshold. Arm 1 is that claim as arithmetic and needs no model: sherpa
/// deletes overlapped frames before embedding, so the BEST DER available to any
/// full-clustering configuration on this fixture is the cost of those deleted
/// frames — and the same deletion costs the 2-class task materially less.
///
/// If that ordering ever reversed, finding #5's reframe would be wrong and this
/// test would say so instead of the backlog assuming it.
///
/// **Read arm 1's two numbers for exactly what they are.** They are CEILINGS:
/// the DER floor the deleted frames impose on a *perfect* implementation of each
/// task. They establish that the 2-class task has more headroom on this fixture.
/// They are not scores, and quoting either as what this code achieves would be a
/// false capability claim. What binary mode achieves is arm 2, which is measured
/// and gated separately.
///
/// **Arm 2 tunes each task for itself, in both of its numbers.** The 2-class
/// acceptance band is a cosine SIMILARITY and the clustering distance is a
/// cosine DISTANCE, and BOTH are swept on THIS fixture against the collapsed
/// reference, over the material left after the enrollment span is removed. An
/// earlier cut ran the band sweep at fixture (e)'s distance on the ground that
/// the distance decides nothing in binary mode; it decides the child's
/// segmentation, which binary mode inherits whole.
#[test]
fn fixture_f_binary_fallback_der() {
    let Some(root) = corpus() else { return };
    let reference = read_rttm(&root, CLASSROOM_6);
    // The instructor: the nearest-mic voice, and the one a student would enrol.
    let instructor = CLASSROOM_6_SPEAKERS[0].0;

    // Arm 1 — the two mechanism ceilings, from ground truth alone.
    let deleted: f64 = overlapped_regions(&reference)
        .iter()
        .map(|(s, e)| e - s)
        .sum();
    assert!(
        deleted > 3.0,
        "fixture (f) must carry real crosstalk or this comparison is vacuous: \
         {deleted:.2}s"
    );
    let full_ceiling = der(&reference, &mechanism_ceiling_hypothesis(&reference));
    let binary_reference = collapse_rttm(&reference, instructor);
    let binary_ceiling = der(
        &binary_reference,
        &collapse_rttm(&mechanism_ceiling_hypothesis(&reference), instructor),
    );
    eprintln!(
        "fixture (f) mechanism ceilings ({deleted:.2}s of overlapped speech deleted \
         before embedding):\n  \
         full N-way   : DER {:.4} (miss {:.2}s of {:.2}s reference speaker time)\n  \
         enrolled/rest: DER {:.4} (miss {:.2}s of {:.2}s)",
        full_ceiling.rate(),
        full_ceiling.miss,
        full_ceiling.total,
        binary_ceiling.rate(),
        binary_ceiling.miss,
        binary_ceiling.total,
    );
    assert!(
        full_ceiling.rate() > 0.15,
        "full clustering's ceiling on fixture (f) is {:.4} — if the mechanism \
         cost has become small, the fixture stopped exceeding the ceiling it \
         was built to exceed",
        full_ceiling.rate()
    );
    assert!(
        binary_ceiling.rate() < full_ceiling.rate() * 0.75,
        "the 2-class ceiling ({:.4}) is not materially better than the full \
         one ({:.4}) — merged finding #5's reframe rests on it being so, and \
         this is where that would stop being true",
        binary_ceiling.rate(),
        full_ceiling.rate()
    );

    // Arm 2 — the measured comparison.
    let Some(pool) = loaded_pool() else { return };
    eprintln!("fixture (f) 2-class sweep (clustering distance × acceptance band):");
    let winner = tune_enrollment_band(&pool, &root, CLASSROOM_6, instructor);
    let Some(winner) = winner else {
        pool.shutdown();
        return;
    };

    // The full N-way arm, recorded and never gated: it is documented as beyond
    // the mechanism on this fixture, and its distance comes from fixture (e)'s
    // sweep because (e) is the zero-ceiling fixture a distance for THAT task can
    // honestly be tuned on.
    // The distance is `ROOM_3_TUNED_DISTANCE` — the recorded output of fixture
    // (e)'s sweep, not a literal and not a re-run: `fixture_e_der_gate` fails on
    // any machine that can measure if that constant has drifted from what the
    // sweep chooses, so reading it here is reading the sweep.
    let full_distance = ROOM_3_TUNED_DISTANCE.map(|d| d as f32);
    eprintln!(
        "fixture (f) full-clustering arm — distance {full_distance:?} from fixture (e)'s \
         recorded sweep:"
    );
    let measured_full = full_distance.and_then(|distance| {
        measured_hypothesis(
            &pool,
            &root,
            CLASSROOM_6,
            distance,
            wilson_voice_lib::diarize::TargetMode::FullClustering,
        )
        .map(|full| (distance, der(&reference, &full).rate()))
    });

    // The winner, re-run through the SHIPPED end-to-end path rather than through
    // the sweep's raw-pass shortcut — so the number recorded below is one
    // `cluster_track` produces, not one only this file can.
    let end_to_end = {
        let (centroid, scored_from) = {
            let raw = raw_turns(&pool, &root, CLASSROOM_6, winner.distance)
                .expect("the winning distance ran a moment ago");
            enrol_from_first_span(&root, CLASSROOM_6, instructor, &raw)
                .expect("the winning distance enrolled a moment ago")
        };
        let hypothesis = measured_hypothesis(
            &pool,
            &root,
            CLASSROOM_6,
            winner.distance,
            wilson_voice_lib::diarize::TargetMode::EnrolledVsEveryoneElse(
                wilson_voice_lib::diarize::EnrolledSpeaker::new(
                    1,
                    centroid,
                    wilson_voice_lib::diarize_metrics::CosineSimilarity::new(winner.band),
                ),
            ),
        )
        .expect("the shipped path answers at the winning pair");
        let reference: Vec<RttmTurn> = binary_reference
            .iter()
            .filter(|t| t.start_seconds >= scored_from)
            .cloned()
            .collect();
        der(&reference, &as_two_classes(hypothesis, scored_from)).rate()
    };
    pool.shutdown();

    assert!(
        (end_to_end - winner.der).abs() < 1e-9,
        "the sweep scored {:.4} at (distance {:.2}, band {:.2}) but the shipped \
         `cluster_track` scores {end_to_end:.4} at the same pair — the sweep's \
         one-pass-per-distance shortcut has drifted from the path that ships, \
         and the tuned numbers describe something the app does not do",
        winner.der,
        winner.distance,
        winner.band
    );

    match measured_full {
        Some((distance, rate)) => eprintln!(
            "fixture (f) MEASURED: full N-way DER {rate:.4} at clustering distance \
             {distance:.2} (recorded, not gated)"
        ),
        None => eprintln!("fixture (f): the full N-way arm did not run"),
    }
    eprintln!(
        "fixture (f) MEASURED: enrolled/rest DER {:.6}, JER {:.6} at clustering \
         distance {:.2} and acceptance band {:.2}, scored from {:.2}s (the \
         enrollment span is excluded); the shipped end-to-end path reproduces it \
         at {end_to_end:.6}",
        winner.der, winner.jer, winner.distance, winner.band, winner.scored_from
    );
    // The full and binary arms are NOT directly comparable as scores — one is
    // scored against a 6-class reference over the whole fixture, the other
    // against a 2-class reference over the post-enrollment part — and neither is
    // comparable to the ceilings in arm 1, which are a different quantity
    // altogether. They are printed together because that is the table the
    // backlog records, not because one minus the other means anything.
    let (Some(der_gate), Some(jer_gate)) = (CLASSROOM_6_DER_GATE, CLASSROOM_6_JER_GATE) else {
        panic!(
            "a measurement is available (binary {:.4} DER / {:.4} JER at distance \
             {:.2}, band {:.2}) but CLASSROOM_6_DER_GATE / CLASSROOM_6_JER_GATE \
             are still `None`. Record the sweep table above in the backlog and \
             gate the BINARY one — full clustering here is documented as beyond \
             the mechanism, not tuned.",
            winner.der, winner.jer, winner.distance, winner.band
        );
    };
    assert!(
        winner.der <= der_gate,
        "fixture (f) binary-mode DER {:.4} is worse than the recorded gate {der_gate:.4}",
        winner.der
    );
    assert!(
        winner.jer <= jer_gate,
        "fixture (f) binary-mode JER {:.4} is worse than the recorded gate {jer_gate:.4}",
        winner.jer
    );
    assert!(
        CLASSROOM_6_TUNED.is_some_and(|(d, b)| (f64::from(winner.distance) - d).abs() < 1e-6
            && (f64::from(winner.band) - b).abs() < 1e-6),
        "the sweep's winner is (distance {:.2}, band {:.2}) but CLASSROOM_6_TUNED \
         records {CLASSROOM_6_TUNED:?}",
        winner.distance,
        winner.band
    );
}

/// No literal threshold reaches a diarization measurement.
///
/// The two sweeps above exist so that every number entering
/// `fixture_e_der_gate` / `fixture_f_binary_fallback_der` is an OUTPUT of this
/// harness. That discipline is a property of the SOURCE, not of a run: a run on
/// a machine with no models checks nothing, and a run on a machine with them
/// checks the numbers rather than where they came from. So it is checked the way
/// `meeting_kind_branch.rs` checks its rules — against the file — and it covers
/// `raw_turns` as well as `measured_hypothesis`, because since the 2-class sweep
/// became two-dimensional `raw_turns` is the other function a distance reaches
/// the wire through.
///
/// The specific regression: the fixture (f) binary arm previously called
/// `measured_hypothesis(..., 0.35, ...)` with the literal written inline, for
/// the 2-class task, in the wrong unit, tuned for nothing.
#[test]
fn every_diarization_measurement_takes_its_threshold_from_a_sweep() {
    for call in ["measured_hypothesis(", "raw_turns("] {
        assert_no_literal_threshold_at(call);
    }
}

/// Every call to `call` in this file, with no numeric literal in any argument at
/// that call's own paren depth.
fn assert_no_literal_threshold_at(call_name: &str) {
    let call = call_name;
    // Comments stripped first: prose that quotes the old `…, 0.35, …` call is
    // the point of the comments, not a call site. Multi-line calls survive,
    // because only each line's trailing comment is removed.
    let src: String = include_str!("meeting_eval.rs")
        .lines()
        .map(|line| line.split("//").next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    let src = src.as_str();
    let mut checked = 0usize;

    for (at, _) in src.match_indices(call) {
        let before = &src[..at];
        // The definition, and this test's own mentions of the name in string
        // literals, are not call sites.
        if before.ends_with("fn ") || before.ends_with('"') {
            continue;
        }
        // The arguments, at THIS call's paren depth: a nested
        // `EnrolledSpeaker::new(1, …)` is not a threshold and is not checked.
        let mut depth = 1usize;
        let mut argument = String::new();
        let mut arguments: Vec<String> = Vec::new();
        for ch in src[at + call.len()..].chars() {
            match ch {
                '(' | '[' => depth += 1,
                ')' | ']' if depth == 1 => break,
                ')' | ']' => depth -= 1,
                ',' if depth == 1 => {
                    arguments.push(std::mem::take(&mut argument));
                    continue;
                }
                _ => {}
            }
            argument.push(ch);
        }
        arguments.push(argument);
        checked += 1;

        for argument in &arguments {
            let argument = argument.trim();
            let numeric = argument
                .trim_start_matches('-')
                .starts_with(|c: char| c.is_ascii_digit() || c == '.');
            assert!(
                !numeric,
                "a diarization measurement is being taken at the literal `{argument}` \
                 — every threshold in this epic is an output of a sweep, and the \
                 specific regression this catches is the 2-class arm's inline 0.35"
            );
        }
    }

    assert!(
        checked >= 2,
        "only {checked} `{call}` call sites found — this guard has stopped \
         guarding anything"
    );
}
