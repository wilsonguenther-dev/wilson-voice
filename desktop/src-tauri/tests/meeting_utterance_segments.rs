//! yap-audit-utterance-segments — a stored meeting segment is an UTTERANCE
//! (silence-cut word rows re-joined into 3–12 s same-track turns), not a whole
//! 30 s window.
//!
//! Audit issue #4: the average stored segment was 29.4–30.1 s on every real
//! meeting, so a two-person call read as alternating 30 s paragraphs. The word
//! rows that would fix it were already there (`timestampKind token`, 62 of 62
//! chunks on the audited meeting) and were thrown away at insert.
//!
//! What is proven here:
//! * `split_utterances` at the EDGES of its three thresholds (0.6 s cut,
//!   0.25 s after a sentence end, 1.2 s re-join, 20 s cap, 3 s fragment);
//! * it never cuts inside a word row and never drops a word;
//! * the pipeline wrapper keeps a window with no times whole;
//! * a two-track synthetic meeting yields interleaved turns ordered by start;
//! * a real ledger of the NT0 synthetic corpus (timings only, text replaced)
//!   splits into readable turns;
//! * on `call-bleed-2voice` after the AEC pre-pass, the median stored segment
//!   is 3–12 s (corpus-gated, like `meeting_eval.rs`);
//! * MAP chunk planning on a 31-minute two-track transcript keeps every chunk,
//!   seg labels included, under the 1400-token budget, at no more than 1.25×
//!   the pass count of the whole-window rows.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use support::eval_metrics::{median_segment_seconds, TimedText};
use support::{meeting_token_count, segments_from};
use wilson_voice_lib::asr_engine::{TimedKind, TimedSpan, TimedTranscript};
use wilson_voice_lib::meeting_asr::{
    ends_sentence, median_utterance_seconds, plan_windows_fixed, split_utterances, BoundaryKind,
    ChunkConfig, ChunkOutcome, ChunkStatus, MeetingProgress, ResumePoint, Utterance,
    UTTERANCE_CUT_GAP_SECONDS, UTTERANCE_MAX_SECONDS, UTTERANCE_MERGE_GAP_SECONDS,
    UTTERANCE_MIN_SECONDS, UTTERANCE_SENTENCE_GAP_SECONDS,
};
use wilson_voice_lib::meeting_pipeline::chunk_segments;
use wilson_voice_lib::meetings::{MeetingKind, MeetingSegment, MIC_TRACK, SYSTEM_TRACK};
use wilson_voice_lib::summarize::{
    plan_chunks, transcript_lines_with, SummaryError, TokenCounter, TrackSpeakers,
    MAP_CHUNK_TOKENS,
};

const EPS: f64 = 1e-9;

// ── scaffolding ─────────────────────────────────────────────────────────────

fn span(start: f64, end: f64, text: &str) -> TimedSpan {
    TimedSpan {
        start_seconds: start,
        end_seconds: end,
        text: text.into(),
    }
}

/// Word rows every 0.25 s with 0.05 s between them, from `start` until `end`.
/// The last row ends exactly at `end`, and carries `last` as its text.
fn run(start: f64, end: f64, last: &str) -> Vec<TimedSpan> {
    let mut out = Vec::new();
    let mut t = start;
    while t + 0.25 < end - EPS {
        out.push(span(t, t + 0.2, "w"));
        t += 0.25;
    }
    out.push(span(t, end, last));
    out
}

fn chunk(index: usize, content: (f64, f64), kind: TimedKind, spans: Vec<TimedSpan>) -> ChunkOutcome {
    ChunkOutcome {
        index,
        audio_start_seconds: (content.0 - 2.0).max(0.0),
        content_start_seconds: content.0,
        content_end_seconds: content.1,
        start_boundary: BoundaryKind::Edge,
        end_boundary: BoundaryKind::Edge,
        status: ChunkStatus::Done,
        text: spans
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        spans,
        timestamp_kind: kind,
        error: None,
    }
}

fn lengths(pieces: &[Utterance]) -> Vec<f64> {
    pieces.iter().map(|p| (p.seconds() * 100.0).round() / 100.0).collect()
}

// ── the three thresholds, at their edges ────────────────────────────────────

#[test]
fn a_silence_of_the_cut_threshold_cuts_and_a_hair_under_does_not() {
    // Two 12 s runs: joined they are over the 20 s cap, so step two cannot
    // undo the cut and the raw rule is observable on its own.
    let mut rows = run(0.0, 12.0, "w");
    rows.extend(run(12.0 + UTTERANCE_CUT_GAP_SECONDS, 24.6, "w"));
    let pieces = split_utterances(&rows);
    assert_eq!(lengths(&pieces), vec![12.0, 12.0], "{pieces:?}");

    let mut rows = run(0.0, 12.0, "w");
    rows.extend(run(12.0 + UTTERANCE_CUT_GAP_SECONDS - 0.01, 24.59, "w"));
    let pieces = split_utterances(&rows);
    assert_eq!(pieces.len(), 1, "0.59 s is not a cut: {pieces:?}");
}

#[test]
fn a_sentence_end_lowers_the_cut_threshold_to_a_quarter_second() {
    let mut rows = run(0.0, 12.0, "done.");
    rows.extend(run(12.0 + UTTERANCE_SENTENCE_GAP_SECONDS, 24.25, "w"));
    let pieces = split_utterances(&rows);
    assert_eq!(lengths(&pieces), vec![12.0, 12.0], "{pieces:?}");
    assert_eq!(pieces[0].text.split_whitespace().last(), Some("done."));

    // The same silence without a sentence end is nothing.
    let mut rows = run(0.0, 12.0, "done");
    rows.extend(run(12.0 + UTTERANCE_SENTENCE_GAP_SECONDS, 24.25, "w"));
    assert_eq!(split_utterances(&rows).len(), 1);

    // A hair under the sentence threshold is nothing, sentence end or not.
    let mut rows = run(0.0, 12.0, "done?");
    rows.extend(run(12.0 + UTTERANCE_SENTENCE_GAP_SECONDS - 0.01, 24.24, "w"));
    assert_eq!(split_utterances(&rows).len(), 1);

    // Closing quotes after the mark still count as a sentence end.
    assert!(ends_sentence("done.\""));
    assert!(ends_sentence("really?)"));
    assert!(ends_sentence("wait\u{2026}"));
    assert!(!ends_sentence("done,"));
    assert!(!ends_sentence(""));
}

#[test]
fn pieces_rejoin_under_the_merge_gap_and_stay_apart_at_it() {
    // 5 s, a 1.19 s silence, 5 s: raw-cut at 0.6, re-joined under 1.2.
    let mut rows = run(0.0, 5.0, "w");
    rows.extend(run(5.0 + UTTERANCE_MERGE_GAP_SECONDS - 0.01, 11.19, "w"));
    let pieces = split_utterances(&rows);
    assert_eq!(pieces.len(), 1, "{pieces:?}");
    assert!((pieces[0].start_seconds - 0.0).abs() < EPS);
    assert!((pieces[0].end_seconds - 11.19).abs() < EPS);

    // At exactly 1.2 s both halves are over the 3 s floor, so they stay apart.
    let mut rows = run(0.0, 5.0, "w");
    rows.extend(run(5.0 + UTTERANCE_MERGE_GAP_SECONDS, 11.2, "w"));
    let pieces = split_utterances(&rows);
    assert_eq!(lengths(&pieces), vec![5.0, 5.0], "{pieces:?}");
}

#[test]
fn a_merge_may_reach_the_cap_and_never_pass_it() {
    let mut rows = run(0.0, 10.0, "w");
    rows.extend(run(11.0, UTTERANCE_MAX_SECONDS, "w"));
    let pieces = split_utterances(&rows);
    assert_eq!(pieces.len(), 1, "exactly 20 s is allowed: {pieces:?}");

    let mut rows = run(0.0, 10.0, "w");
    rows.extend(run(11.0, UTTERANCE_MAX_SECONDS + 0.01, "w"));
    let pieces = split_utterances(&rows);
    assert_eq!(pieces.len(), 2, "20.01 s is not: {pieces:?}");
}

#[test]
fn a_fragment_joins_its_nearest_neighbour_when_the_result_fits() {
    // A(0–5) · 2 s · B(7–9, a 2 s fragment) · 2 s · C(11–18): a tie goes to
    // the earlier neighbour.
    let mut rows = run(0.0, 5.0, "w");
    rows.extend(run(7.0, 9.0, "b"));
    rows.extend(run(11.0, 18.0, "w"));
    let pieces = split_utterances(&rows);
    assert_eq!(lengths(&pieces), vec![9.0, 7.0], "{pieces:?}");
    assert!(pieces[0].text.ends_with(" b"));

    // Closer to C: joins C.
    let mut rows = run(0.0, 5.0, "w");
    rows.extend(run(7.0, 9.0, "b"));
    rows.extend(run(10.5, 18.0, "w"));
    let pieces = split_utterances(&rows);
    assert_eq!(lengths(&pieces), vec![5.0, 11.0], "{pieces:?}");
    assert!(pieces[1].text.starts_with("w"));
    assert!(pieces[1].text.contains(" b "));

    // Only the later neighbour has room under the cap.
    let mut rows = run(0.0, 18.5, "w");
    rows.extend(run(20.5, 22.0, "b"));
    rows.extend(run(24.0, 30.0, "w"));
    let pieces = split_utterances(&rows);
    assert_eq!(lengths(&pieces), vec![18.5, 9.5], "{pieces:?}");

    // A run of fragments collapses into one turn, not one fragment per pass.
    let mut rows = Vec::new();
    for k in 0..6 {
        rows.extend(run(k as f64 * 3.5, k as f64 * 3.5 + 1.5, "w"));
    }
    let pieces = split_utterances(&rows);
    assert_eq!(pieces.len(), 1, "{pieces:?}");
    assert!(pieces[0].seconds() >= UTTERANCE_MIN_SECONDS);
}

#[test]
fn a_fragment_is_kept_alone_only_when_it_is_the_whole_window_or_nothing_can_take_it() {
    let pieces = split_utterances(&run(0.0, 1.5, "w"));
    assert_eq!(lengths(&pieces), vec![1.5]);

    // Both neighbours would pass the cap with it.
    let mut rows = run(0.0, 19.5, "w");
    rows.extend(run(21.5, 23.0, "b"));
    rows.extend(run(25.0, 44.0, "w"));
    let pieces = split_utterances(&rows);
    assert_eq!(lengths(&pieces), vec![19.5, 1.5, 19.0], "{pieces:?}");
}

#[test]
fn a_cut_never_lands_inside_a_word_row_and_never_drops_a_word() {
    // Irregular rows, some long, some touching, some after a silence.
    let rows = vec![
        span(0.0, 0.9, "Okay"),
        span(0.9, 1.4, "so"),
        span(1.4, 2.6, "pricing."),
        span(3.2, 3.6, "We"),
        span(3.6, 4.4, "agreed"),
        span(4.4, 6.0, "yesterday."),
        span(8.0, 8.5, "Right?"),
        span(9.1, 9.9, "Jeisil"),
        span(9.9, 10.3, "will"),
        span(10.3, 11.4, "send"),
        span(11.4, 12.0, "it."),
        span(14.5, 15.0, "And"),
        span(15.0, 16.0, "Wilson"),
        span(16.0, 17.2, "books"),
        span(17.2, 18.0, "it."),
        span(30.1, 31.0, "Thanks"),
        span(31.0, 32.0, "all."),
    ];
    let pieces = split_utterances(&rows);
    let starts: Vec<f64> = rows.iter().map(|r| r.start_seconds).collect();
    let ends: Vec<f64> = rows.iter().map(|r| r.end_seconds).collect();
    for p in &pieces {
        assert!(starts.iter().any(|s| (s - p.start_seconds).abs() < EPS), "{p:?}");
        assert!(ends.iter().any(|e| (e - p.end_seconds).abs() < EPS), "{p:?}");
        assert!(p.seconds() <= UTTERANCE_MAX_SECONDS + EPS, "{p:?}");
    }
    let joined = pieces.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join(" ");
    let all = rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>().join(" ");
    assert_eq!(joined, all, "every word, once, in order");
    assert!(pieces.len() >= 3, "{pieces:?}");
    assert!(pieces.windows(2).all(|w| w[0].end_seconds <= w[1].start_seconds + EPS));
    // Empty rows are ignored; an empty input is an empty answer.
    assert!(split_utterances(&[]).is_empty());
    assert!(split_utterances(&[span(0.0, 1.0, "  ")]).is_empty());
}

// ── the pipeline wrapper ────────────────────────────────────────────────────

#[test]
fn a_window_with_no_times_stays_whole_and_a_timed_window_becomes_utterances() {
    let mut whole = chunk(0, (0.0, 30.0), TimedKind::None, Vec::new());
    whole.text = "no alignment came back for this window".into();
    let rows = chunk_segments(&whole, MIC_TRACK);
    assert_eq!(rows.len(), 1);
    assert!((rows[0].start_seconds - 0.0).abs() < EPS);
    assert!((rows[0].end_seconds - 30.0).abs() < EPS);
    assert_eq!(rows[0].text, whole.text);
    assert_eq!(rows[0].track, MIC_TRACK);

    let mut spans = run(0.5, 8.0, "one.");
    spans.extend(run(10.0, 17.0, "two."));
    spans.extend(run(19.0, 29.0, "three."));
    let timed = chunk(0, (0.0, 30.0), TimedKind::Token, spans);
    let rows = chunk_segments(&timed, SYSTEM_TRACK);
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert!(rows.iter().all(|r| r.track == SYSTEM_TRACK));
    assert!((rows[0].start_seconds - 0.5).abs() < EPS);
    assert!((rows[2].end_seconds - 29.0).abs() < EPS);
    assert!(rows[0].text.ends_with("one."));

    // A failed window writes nothing; a window whose rows all sit in the
    // overlap another window owns writes nothing either.
    let mut failed = timed.clone();
    failed.status = ChunkStatus::AsrFailed;
    assert!(chunk_segments(&failed, MIC_TRACK).is_empty());
    let overlap_only = chunk(1, (30.0, 60.0), TimedKind::Token, run(28.0, 29.5, "w"));
    assert!(chunk_segments(&overlap_only, MIC_TRACK).is_empty());
}

#[test]
fn a_two_track_synthetic_meeting_yields_interleaved_turns_ordered_by_start() {
    let mut mic = run(0.0, 8.0, "me.");
    mic.extend(run(20.0, 28.0, "me."));
    let mut sys = run(9.0, 18.0, "them.");
    sys.extend(run(28.0, 30.0, "them."));
    let mut rows = chunk_segments(&chunk(0, (0.0, 30.0), TimedKind::Token, mic), MIC_TRACK);
    rows.extend(chunk_segments(&chunk(0, (0.0, 30.0), TimedKind::Token, sys), SYSTEM_TRACK));
    rows.sort_by(|a, b| a.start_seconds.total_cmp(&b.start_seconds));
    let tracks: Vec<i64> = rows.iter().map(|r| r.track).collect();
    assert_eq!(tracks, vec![MIC_TRACK, SYSTEM_TRACK, MIC_TRACK, SYSTEM_TRACK], "{rows:?}");
    assert!(rows.windows(2).all(|w| w[0].start_seconds < w[1].start_seconds));
    assert!(rows[0].text.ends_with("me."));
    assert!(rows[1].text.ends_with("them."));
}

// ── a real ledger of the synthetic corpus, timings only ─────────────────────

/// `<id>.t1.asr-progress.json` of `call-bleed-2voice` (NT0 synthetic corpus,
/// `say`-generated), written by [`meeting_utterance_segments_write_corpus_fixture`]
/// with every word replaced by `x` (`x.` when it ended a sentence). Times are
/// the shipped model's; no transcript text is in the repo.
fn ledger_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/meeting-utterances/call-bleed-2voice.t1.asr-progress.json")
}

#[test]
fn a_corpus_ledger_splits_into_readable_turns() {
    let text = fs::read_to_string(ledger_fixture())
        .unwrap_or_else(|e| panic!("{}: {e}", ledger_fixture().display()));
    let ledger: MeetingProgress = serde_json::from_str(&text).expect("a resume ledger");
    assert!(ledger.chunks.len() >= 5, "a few windows at least");
    let mut rows = Vec::new();
    let mut windows = 0usize;
    for chunk in &ledger.chunks {
        assert!(chunk.timestamp_kind.has_times(), "window {} carries times", chunk.index);
        assert!(chunk.text.split_whitespace().all(|w| w == "x" || w == "x."), "timings only");
        let starts: Vec<f64> = chunk.spans.iter().map(|s| s.start_seconds).collect();
        let ends: Vec<f64> = chunk.spans.iter().map(|s| s.end_seconds).collect();
        let pieces = chunk_segments(chunk, SYSTEM_TRACK);
        for p in &pieces {
            assert!(p.end_seconds - p.start_seconds <= UTTERANCE_MAX_SECONDS + EPS, "{p:?}");
            assert!(starts.iter().any(|s| (s - p.start_seconds).abs() < EPS), "{p:?}");
            assert!(ends.iter().any(|e| (e - p.end_seconds).abs() < EPS), "{p:?}");
            assert!(p.start_seconds >= chunk.content_start_seconds - 1e-6);
            assert!(p.start_seconds < chunk.content_end_seconds);
        }
        if !pieces.is_empty() {
            windows += 1;
        }
        rows.extend(pieces);
    }
    let timed: Vec<TimedText> = rows
        .iter()
        .map(|r| TimedText::new(r.start_seconds, r.end_seconds, &r.text))
        .collect();
    let median = median_segment_seconds(&timed);
    eprintln!(
        "ledger fixture: windows={windows} rows={} median_s={median:.2}",
        rows.len()
    );
    assert!(rows.len() >= 2 * windows, "at least two turns per window, got {} for {windows}", rows.len());
    assert!((3.0..=12.0).contains(&median), "median {median:.2} s is not readable");
    assert!(rows.windows(2).all(|w| w[0].start_seconds <= w[1].start_seconds + EPS));
}

// ── the corpus (skipped without it, like meeting_eval.rs) ───────────────────

const CORPUS_ENV: &str = "YAP_EVAL_CORPUS";
const REQUIRE_ENV: &str = "YAP_EVAL_REQUIRE";
const MODEL_DIR_ENV: &str = "YAP_MODEL_DIR";
const DATA_DIR_ENV: &str = "YAP_DATA_DIR";
const CALL_BLEED: &str = "call-bleed-2voice";
const RATE: usize = 16_000;

fn corpus() -> Option<PathBuf> {
    let root = std::env::var(CORPUS_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::home_dir().expect("home").join("yap-eval-corpus/meetings"));
    let required = std::env::var(REQUIRE_ENV).map(|v| v.trim() == "1").unwrap_or(false);
    if root.is_dir() {
        provision_pinned_model();
        // Never decode without the pinned model in the scratch root: the
        // binary would fall back to the smallest model and DOWNLOAD it.
        let pinned = wilson_voice_lib::models::recommended_model();
        if wilson_voice_lib::models::is_downloaded(pinned) {
            return Some(root);
        }
        assert!(!required, "{REQUIRE_ENV}=1 but {} is not installed — set {MODEL_DIR_ENV}", pinned.id);
        eprintln!("meeting_utterance_segments: corpus present but {} is not installed — skipped", pinned.id);
        return None;
    }
    assert!(!required, "{REQUIRE_ENV}=1 but no corpus at {}", root.display());
    eprintln!("meeting_utterance_segments: no eval corpus at {} — skipped", root.display());
    None
}

/// `YAP_MODEL_DIR` → link the pinned model into `<YAP_DATA_DIR>/models` and
/// select it there (the same contract `meeting_eval.rs` uses).
fn provision_pinned_model() {
    let Ok(model_dir) = std::env::var(MODEL_DIR_ENV) else { return };
    let data_dir = std::env::var(DATA_DIR_ENV)
        .ok()
        .filter(|d| !d.trim().is_empty())
        .unwrap_or_else(|| panic!("{MODEL_DIR_ENV} is set but {DATA_DIR_ENV} is not"));
    let pinned = wilson_voice_lib::models::recommended_model();
    let file = pinned.default_file().expect("the pinned model lists a file");
    let src = PathBuf::from(&model_dir).join(&file.filename);
    assert!(src.is_file(), "{} is not a file", src.display());
    let models_dir = PathBuf::from(&data_dir).join("models");
    fs::create_dir_all(&models_dir).expect("models dir");
    let dest = models_dir.join(&file.filename);
    if !dest.exists() {
        std::os::unix::fs::symlink(&src, &dest).expect("link the pinned model");
    }
    let settings = PathBuf::from(&data_dir).join("settings.json");
    if !settings.exists() {
        fs::write(&settings, serde_json::json!({ "nativeModel": pinned.id }).to_string())
            .expect("settings");
    }
}

fn read_wav_16k(path: &Path) -> Vec<i16> {
    let mut r = hound::WavReader::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let spec = r.spec();
    assert_eq!(spec.channels, 1);
    assert_eq!(spec.sample_rate as usize, RATE);
    r.samples::<i16>().map(|s| s.expect("sample")).collect()
}

fn write_wav_16k(path: &Path, samples: &[i16]) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE as u32,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).expect("wav");
    for s in samples {
        w.write_sample(*s).expect("sample");
    }
    w.finalize().expect("finalize");
}

/// The shipped geometry, the shipped decoder (`--transcribe-file --timed`),
/// the shipped shift-and-clip — one [`ChunkOutcome`] per window.
fn decode_windows(samples: &[i16], scratch: &Path, label: &str) -> Vec<ChunkOutcome> {
    let total = samples.len() as f64 / RATE as f64;
    let plan = plan_windows_fixed(total, ResumePoint::start(), &ChunkConfig::default(), 0);
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_wilson-voice"));
    let mut out = Vec::new();
    for window in &plan {
        let from = (window.audio_start_seconds * RATE as f64) as usize;
        let to = ((window.audio_end_seconds * RATE as f64) as usize).min(samples.len());
        let wav = scratch.join(format!("{label}-{:03}.wav", window.index));
        write_wav_16k(&wav, &samples[from..to]);
        let run = Command::new(&bin)
            .arg("--transcribe-file")
            .arg(&wav)
            .arg("--timed")
            .output()
            .unwrap_or_else(|e| panic!("cannot run {}: {e}", bin.display()));
        let _ = fs::remove_file(&wav);
        assert!(
            run.status.success(),
            "decode of {label} window {} failed: {}",
            window.index,
            String::from_utf8_lossy(&run.stderr).trim()
        );
        let transcript: TimedTranscript =
            serde_json::from_str(String::from_utf8_lossy(&run.stdout).trim())
                .expect("a timed transcript");
        out.push(ChunkOutcome::from_transcript(window, transcript));
    }
    out
}

fn as_timed(rows: &[wilson_voice_lib::meetings::NewMeetingSegment]) -> Vec<TimedText> {
    rows.iter()
        .map(|r| TimedText::new(r.start_seconds, r.end_seconds, &r.text))
        .collect()
}

/// The gate audit issue #4 asked for: on the two-voice call, AFTER the AEC
/// pre-pass (yap24-NT11) has cleaned the far side out of the mic, the rows the
/// pipeline would store for track 0 have a median length between 3 and 12 s.
/// Printed beside it: the whole-window median the same windows had before.
#[test]
fn call_bleed_after_the_aec_prepass_stores_three_to_twelve_second_turns() {
    let Some(root) = corpus() else { return };
    let scratch = std::env::temp_dir().join(format!("yap-utterances-{}", std::process::id()));
    fs::create_dir_all(&scratch).expect("scratch");
    let mic = root.join(CALL_BLEED).join("track0.wav");
    let sys = root.join(CALL_BLEED).join("track1.wav");
    let cleaned = scratch.join("track0.aec.wav");
    let report = wilson_voice_lib::meeting_aec::aec_prepass(&mic, &sys, &cleaned)
        .expect("the AEC pre-pass");
    eprintln!("{CALL_BLEED}: aec call_atten_p50_db={:.1}", report.call_atten_p50_db);
    let samples = read_wav_16k(&cleaned);
    let chunks = decode_windows(&samples, &scratch, "call-bleed-aec");
    let _ = fs::remove_dir_all(&scratch);
    if let Ok(dump) = std::env::var("YAP_UTTERANCE_DUMP") {
        // Timings only, for a developer reading the windows by hand.
        let mut redacted = chunks.clone();
        for c in &mut redacted {
            for s in &mut c.spans {
                s.text = if ends_sentence(&s.text) { "x.".into() } else { "x".into() };
            }
            c.text = String::new();
        }
        fs::write(&dump, serde_json::to_string_pretty(&redacted).unwrap()).expect("dump");
    }

    let mut rows = Vec::new();
    for c in &chunks {
        rows.extend(chunk_segments(c, MIC_TRACK));
    }
    let whole: Vec<TimedText> = chunks
        .iter()
        .filter(|c| !c.text.trim().is_empty())
        .map(|c| TimedText::new(c.content_start_seconds, c.content_end_seconds, &c.text))
        .collect();
    let median = median_segment_seconds(&as_timed(&rows));
    let timed_windows = chunks.iter().filter(|c| c.timestamp_kind.has_times()).count();
    eprintln!(
        "meeting_eval {CALL_BLEED} after_aec windows={} timed_windows={timed_windows} rows={} median_segment_seconds={median:.2} whole_window_median_s={:.2}",
        chunks.len(),
        rows.len(),
        median_segment_seconds(&whole)
    );
    assert!(timed_windows > 0, "the shipped model returned no alignment");
    assert!(
        (3.0..=12.0).contains(&median),
        "median stored segment is {median:.2} s; the readable band is 3–12 s"
    );
    assert!(rows.len() > whole.len(), "more turns than windows");
}

/// Writer for [`ledger_fixture`]. `cargo test --test meeting_utterance_segments
/// -- --ignored meeting_utterance_segments_write_corpus_fixture` with the
/// corpus and `YAP_MODEL_DIR`/`YAP_DATA_DIR` set. Text is replaced before
/// anything is written.
#[test]
#[ignore]
fn meeting_utterance_segments_write_corpus_fixture() {
    let root = corpus().expect("the eval corpus");
    let scratch = std::env::temp_dir().join(format!("yap-utterances-w-{}", std::process::id()));
    fs::create_dir_all(&scratch).expect("scratch");
    let samples = read_wav_16k(&root.join(CALL_BLEED).join("track1.wav"));
    let mut chunks = decode_windows(&samples, &scratch, "call-bleed-sys");
    let _ = fs::remove_dir_all(&scratch);
    for c in &mut chunks {
        for s in &mut c.spans {
            s.text = if ends_sentence(&s.text) { "x.".into() } else { "x".into() };
        }
        c.text = c.spans.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
    }
    let through = chunks.iter().map(|c| c.content_end_seconds).fold(0.0, f64::max);
    let ledger = MeetingProgress {
        meeting_id: "nt0-call-bleed-2voice-timings-only".into(),
        processed_through_seconds: through,
        chunks,
    };
    let pieces: Vec<Utterance> = ledger
        .chunks
        .iter()
        .flat_map(|c| chunk_segments(c, SYSTEM_TRACK))
        .map(|r| Utterance {
            start_seconds: r.start_seconds,
            end_seconds: r.end_seconds,
            text: r.text,
        })
        .collect();
    eprintln!(
        "fixture: windows={} rows={} median_s={:.2}",
        ledger.chunks.len(),
        pieces.len(),
        median_utterance_seconds(&pieces)
    );
    fs::create_dir_all(ledger_fixture().parent().unwrap()).expect("fixture dir");
    fs::write(ledger_fixture(), serde_json::to_string_pretty(&ledger).unwrap()).expect("write");
}

// ── MAP chunk planning on a 31-minute two-track transcript ──────────────────

struct Counter;

impl TokenCounter for Counter {
    fn count_tokens(&self, text: &str) -> Result<usize, SummaryError> {
        Ok(meeting_token_count(text))
    }
}

const WORDS: &[&str] = &[
    "the", "pricing", "review", "for", "the", "consulting", "tier", "stays", "where", "we",
    "agreed", "and", "Jeisil", "will", "send", "the", "updated", "document", "to", "everyone",
    "before", "Friday", "while", "Wilson", "books", "the", "demo", "with", "the", "university",
    "next", "week", "so", "the", "open", "question", "is", "whether", "annual", "billing",
    "gets", "a", "discount", "at", "$49.99", "a", "seat",
];

/// 31 minutes of two people taking turns: turn lengths cycle through 3–12 s,
/// 1 s between turns, 2.6 words per second. Returns the utterance rows and
/// the whole-window rows (one per 30 s window per track) of the same words.
fn thirty_one_minute_call() -> (Vec<MeetingSegment>, Vec<MeetingSegment>) {
    const TURNS: [f64; 8] = [4.0, 7.0, 10.0, 5.0, 8.0, 12.0, 3.0, 6.0];
    let mut t = 0.0;
    let mut turn = 0usize;
    let mut word = 0usize;
    let mut utterances: Vec<(i64, f64, f64, String)> = Vec::new();
    while t < 31.0 * 60.0 {
        let len = TURNS[turn % TURNS.len()];
        let n = (len * 2.6).round() as usize;
        let mut text = Vec::with_capacity(n);
        for _ in 0..n {
            text.push(WORDS[word % WORDS.len()]);
            word += 1;
        }
        let track = if turn % 2 == 0 { MIC_TRACK } else { SYSTEM_TRACK };
        utterances.push((track, t, t + len, format!("{}.", text.join(" "))));
        t += len + 1.0;
        turn += 1;
    }
    let mut windows: Vec<(i64, f64, f64, String)> = Vec::new();
    for (track, start, end, text) in &utterances {
        let k = (start / 30.0).floor();
        match windows
            .iter_mut()
            .find(|w| w.0 == *track && (w.1 / 30.0).floor() == k)
        {
            Some(w) => {
                w.2 = *end;
                w.3.push(' ');
                w.3.push_str(text);
            }
            None => windows.push((*track, *start, *end, text.clone())),
        }
    }
    windows.sort_by(|a, b| a.1.total_cmp(&b.1));
    let to_segments = |rows: &[(i64, f64, f64, String)]| -> Vec<MeetingSegment> {
        let texts: Vec<&str> = rows.iter().map(|r| r.3.as_str()).collect();
        let mut segs = segments_from(&texts);
        for (s, r) in segs.iter_mut().zip(rows) {
            s.track = r.0;
            s.start_seconds = r.1;
            s.end_seconds = r.2;
        }
        segs
    };
    (to_segments(&utterances), to_segments(&windows))
}

#[test]
fn map_chunks_with_seg_labels_fit_the_budget_at_no_more_than_1_25x_the_window_passes() {
    let (utterances, windows) = thirty_one_minute_call();
    assert!(utterances.last().unwrap().end_seconds >= 31.0 * 60.0 - 15.0);
    let turns: Vec<TimedText> = utterances
        .iter()
        .map(|s| TimedText::new(s.start_seconds, s.end_seconds, &s.text))
        .collect();
    let median = median_segment_seconds(&turns);
    assert!((3.0..=12.0).contains(&median), "fixture median {median:.1}");
    assert!(windows.len() < utterances.len(), "{} windows, {} turns", windows.len(), utterances.len());

    let speakers = TrackSpeakers::for_meeting(MeetingKind::Virtual, &utterances);
    let lines = transcript_lines_with(&utterances, &speakers);
    assert!(lines.iter().all(|l| l.render().contains("(Me): ") || l.render().contains("(Them): ")));
    let planned = plan_chunks(&lines, MAP_CHUNK_TOKENS, &Counter).expect("a plan");
    for (i, c) in planned.iter().enumerate() {
        assert!(c.tokens <= MAP_CHUNK_TOKENS, "chunk {i} is {} tokens", c.tokens);
        let rendered = c.lines.iter().map(|l| l.render()).collect::<Vec<_>>().join("\n");
        assert!(
            meeting_token_count(&rendered) <= MAP_CHUNK_TOKENS,
            "chunk {i} renders to {} tokens with its labels",
            meeting_token_count(&rendered)
        );
        assert!(!c.truncated);
    }
    let window_lines = transcript_lines_with(&windows, &speakers);
    let window_plan = plan_chunks(&window_lines, MAP_CHUNK_TOKENS, &Counter).expect("a plan");
    eprintln!(
        "map planning: turns={} window_rows={} map_passes={} window_passes={}",
        utterances.len(),
        windows.len(),
        planned.len(),
        window_plan.len()
    );
    assert!(
        planned.len() as f64 <= 1.25 * window_plan.len() as f64,
        "{} MAP passes on utterances vs {} on whole windows",
        planned.len(),
        window_plan.len()
    );
    // Labels are positional: the nth line is seg_n, whichever row shape made it.
    assert_eq!(lines[0].label, "seg_0001");
    assert_eq!(lines[41].label, "seg_0042");
}
