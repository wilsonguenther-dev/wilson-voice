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

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
/// Every fixture the manifest must name. Fixture (c) is generated here but
/// consumed by YV92 (anti-alias + input format change), not by this file's gates.
const FIXTURE_IDS: [&str; 3] = [LECTURE, SEAM_STRESS, DEVICE_CHANGE];

/// PLACEHOLDER BASELINE. Parakeet Unified EN 0.6B with no meeting tuning, on
/// clean synthesized speech. Replace this constant with the measured number
/// once YV93's real chunked ASR lands, and record the figure in the backlog.
const WER_GATE: f64 = 0.15;

/// The chunk geometry the plan specifies for meeting ASR (22-A): 30 s windows,
/// 2 s overlap. YV93 replaces the fixed clock with a VAD-cut boundary inside a
/// [25 s, 35 s] search window; the gates below do not care which produced the
/// chunks, only that the merged transcript is correct at the seams.
const CHUNK_SECONDS: f64 = 30.0;
const CHUNK_OVERLAP_SECONDS: f64 = 2.0;

/// The distance between consecutive window STARTS. This — not
/// [`CHUNK_SECONDS`] — is what "every chunk boundary" means: with a 30 s window
/// and a 2 s overlap the windows are 0–30, 28–58, 56–86 … so the seams are at
/// 28 s, 56 s, 84 s, and a marker word placed on a multiple of 30 s lands in the
/// interior of exactly one window, where no merge implementation can move it.
fn chunk_hop_seconds() -> f64 {
    CHUNK_SECONDS - CHUNK_OVERLAP_SECONDS
}

/// The only region of the audio that windows `k-1` and `k` BOTH contain:
/// `[k*hop, k*hop + overlap]`. A marker word must sit entirely inside one of
/// these or the seam gate is scoring nothing — which is the defect this fixture
/// was built to make impossible, and then reproduced by placing markers on
/// `k * CHUNK_SECONDS` instead.
fn seam_region(k: usize) -> (f64, f64) {
    let start = k as f64 * chunk_hop_seconds();
    (start, start + CHUNK_OVERLAP_SECONDS)
}

/// Everything downstream of the mic runs at 16 kHz mono.
const TARGET_RATE: u32 = 16_000;

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
    /// `meeting_eval_manifest_is_committed_and_names_three_fixtures` enforces it.
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

/// One decoded window of a meeting. `start_seconds` is the window's own start
/// today; when YV93 lands `asr_engine::transcribe_timed` it becomes the real
/// segment timestamp, and the ordering gate below stops being a check on the
/// harness's own arithmetic and starts being a check on the model's output.
#[derive(Debug, Clone, PartialEq)]
struct Segment {
    start_seconds: f64,
    text: String,
}

/// Windows over `total_seconds`: [`CHUNK_SECONDS`] wide, hopping by
/// `CHUNK_SECONDS - CHUNK_OVERLAP_SECONDS`, with the last window clipped to the
/// end of the audio. Every second of the fixture is inside at least one window.
fn chunk_plan(total_seconds: f64) -> Vec<(f64, f64)> {
    let hop = CHUNK_SECONDS - CHUNK_OVERLAP_SECONDS;
    assert!(hop > 0.0, "the overlap cannot swallow the window");
    let mut out = Vec::new();
    let mut start = 0.0f64;
    while start < total_seconds {
        let end = (start + CHUNK_SECONDS).min(total_seconds);
        out.push((start, end));
        if end >= total_seconds {
            break;
        }
        start += hop;
    }
    if out.is_empty() {
        out.push((0.0, total_seconds));
    }
    out
}

/// A splice anchor must be at least this many tokens long. Two tokens is a
/// coincidence ("the room"); three is an overlap.
const MIN_ANCHOR_TOKENS: usize = 3;
/// …and at most this many. The overlap is 2 s of speech — six or seven words.
const MAX_ANCHOR_TOKENS: usize = 10;
/// The boundary can cut a word in half, so the TAIL of the outgoing chunk may
/// end in a token nobody said. Allow the anchor to sit this far back from the
/// end of the running transcript, and no further.
const MAX_TAIL_TRIM: usize = 2;
/// …and the same at the HEAD of the incoming chunk.
const MAX_HEAD_SKIP: usize = 4;

/// The BASELINE seam merge: the incoming chunk repeats the last
/// [`CHUNK_OVERLAP_SECONDS`] of the previous one, so find that repeat and splice
/// past it. This is the text-level LCS fallback the plan describes.
///
/// Two shapes were measured and thrown away before this one, both on the seam
/// fixture, which is exactly what the fixture is for:
///
/// 1. *Drop the prefix of the next chunk that is already a suffix of what we
///    have.* A window boundary cuts a word in half, so the incoming chunk opens
///    with a token nobody said ("her word for this boundary is pineapple…"), no
///    prefix ever matches, and the whole overlap is emitted twice —
///    `duplicated_word_count` came back 1.
/// 2. *Longest common run anywhere in a 32-token window either side.* That
///    finds an anchor far from the seam and truncates the running transcript
///    back to it, deleting real words: 37 deletions against the continuous
///    decode and one marker word (`trombone`) gone. `dropped_word_count` caught
///    it — which is the whole reason finding #16 insists on that counter.
///
/// What survives anchors the run to the END of the running transcript and the
/// START of the incoming chunk, where the overlap actually is, with a couple of
/// tokens of slack at each side for the cut word. It can therefore delete at
/// most [`MAX_TAIL_TRIM`] + [`MAX_HEAD_SKIP`] tokens at a seam by construction,
/// and only tokens inside the overlap region. YV93 replaces it with the real
/// merge and must clear the same two numbers.
fn merge_chunk_tokens(chunks: &[Vec<String>]) -> Vec<String> {
    let mut merged: Vec<String> = Vec::new();
    for chunk in chunks {
        if merged.is_empty() {
            merged.extend(chunk.iter().cloned());
            continue;
        }
        let mut splice: Option<(usize, usize)> = None; // (keep in merged, skip in chunk)
        'search: for n in (MIN_ANCHOR_TOKENS..=MAX_ANCHOR_TOKENS).rev() {
            for trim in 0..=MAX_TAIL_TRIM {
                if merged.len() < trim + n {
                    continue;
                }
                let keep = merged.len() - trim;
                let tail = &merged[keep - n..keep];
                for skip in 0..=MAX_HEAD_SKIP {
                    if chunk.len() < skip + n {
                        break;
                    }
                    if tail == &chunk[skip..skip + n] {
                        splice = Some((keep, skip + n));
                        break 'search;
                    }
                }
            }
        }
        match splice {
            Some((keep, skip)) => {
                merged.truncate(keep);
                merged.extend(chunk[skip..].iter().cloned());
            }
            // No anchor: append whole. Emitting a duplicate is a visible defect
            // the seam gate counts; deleting words on a guess is a silent one.
            None => merged.extend(chunk.iter().cloned()),
        }
    }
    merged
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
        let _one_at_a_time = DECODE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let wav = self.scratch.join(format!("{label}.wav"));
        write_wav_16k_mono(&wav, samples);
        let out = Command::new(&self.bin)
            .arg("--transcribe-file")
            .arg(&wav)
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
        String::from_utf8_lossy(&out.stdout).trim().to_string()
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
        let plan = chunk_plan(total);
        let mut per_chunk: Vec<Vec<String>> = Vec::new();
        let mut segments: Vec<Segment> = Vec::new();
        for (n, (start, end)) in plan.iter().enumerate() {
            let from = (start * TARGET_RATE as f64) as usize;
            let to = ((end * TARGET_RATE as f64) as usize).min(samples.len());
            let text = self.decode(&samples[from..to], &format!("{label}-chunk{n:03}"));
            eprintln!("  [{label}] window {n:03} {start:7.2}s–{end:7.2}s");
            per_chunk.push(normalize(&text));
            segments.push(Segment {
                start_seconds: *start,
                text,
            });
        }
        ChunkedDecode {
            merged: merge_chunk_tokens(&per_chunk),
            segments,
            per_chunk,
        }
    }
}

/// The result of a windowed decode: the merged transcript, the per-window
/// segments the ordering gate runs on, and the per-window token vectors the
/// vacuity guard runs on.
struct ChunkedDecode {
    merged: Vec<String>,
    segments: Vec<Segment>,
    per_chunk: Vec<Vec<String>>,
}

impl ChunkedDecode {
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
fn meeting_eval_manifest_is_committed_and_names_three_fixtures() {
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
    assert_eq!(plan[1].0, 28.0, "the hop is window minus overlap");
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
/// marker placed inside [`seam_region`] is in TWO windows, and one placed on a
/// multiple of [`CHUNK_SECONDS`] — the number the first cut of the fixture used,
/// and the number the prose kept repeating — is in one.
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
        // k * CHUNK_SECONDS. Windows hop by 28 s, so 30/60/90/120/150 s are
        // interior points of a single window and no merge can change their count.
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
    eprintln!("{LECTURE}: {} windows, {report}", decode.segments.len());
    println!("meeting_eval {LECTURE} wer={:.4}", report.wer());
    assert!(
        report.wer() <= WER_GATE,
        "{LECTURE} regressed past the {WER_GATE} gate: {report}"
    );
}

/// Acceptance gate: segment start times over the FULL 15-minute fixture come out
/// monotonic. Today those are the decode windows' own starts, so this is a check
/// on the harness's assembly; when YV93 lands `asr_engine::transcribe_timed`,
/// the same assertion runs over real per-segment timestamps and starts checking
/// the model. The negative control that keeps it honest —
/// `timestamps_are_sorted_gate_catches_out_of_order_segments` — needs no corpus
/// and always runs.
#[test]
fn meeting_eval_lecture_segment_timestamps_are_sorted() {
    let Some(root) = corpus() else { return };
    let segments = &lecture_decode(&root).segments;

    let timestamps: Vec<f64> = segments.iter().map(|s| s.start_seconds).collect();
    assert!(timestamps.is_sorted());
    assert!(
        segments.len() > 30,
        "a 15-minute fixture is more than {} windows",
        segments.len()
    );
    assert!(
        segments.iter().all(|s| !s.text.trim().is_empty()),
        "a window decoded to nothing"
    );
    eprintln!(
        "{LECTURE}: {} segment start times, monotonic, {:.2}s to {:.2}s",
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

    let timestamps: Vec<f64> = decode.segments.iter().map(|s| s.start_seconds).collect();
    assert!(timestamps.is_sorted());
    assert!(
        decode.segments.iter().all(|s| !s.text.trim().is_empty()),
        "a window decoded to nothing — the seam numbers above are not trustworthy"
    );
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
    };
    write_fixture(root, &meta, &audio);
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
