//! yap-1106-diarize-wire — the diarization pass every finished meeting gets.
//!
//! After transcription, [`diarize_meeting`] hands the right track to the
//! shipped `yap-diarize` sidecar (through [`crate::diarize::cluster_track`]),
//! then writes `meeting_segments.cluster_index`:
//!
//! - in-person and unknown meetings cluster the MIC track (several people in
//!   the room); a virtual meeting with a call track clusters the CALL track
//!   (several remote people mixed into one track) and its mic stays "You" —
//!   the branch is `meetings::diarization_target`, not re-decided here;
//! - each stored row is attributed from the ASR WORD times kept in the
//!   per-track resume ledger (`<id>.tN.asr-progress.json`), through
//!   [`crate::meeting_asr::build_speaker_turns`]: a word goes to the segment
//!   covering its midpoint, a turn is a run of whole words, and a row whose
//!   words belong to two speakers is SPLIT at the switch
//!   ([`crate::db::Database::split_segment_by_speaker`]) — never a word bled
//!   across the boundary, never a time interpolated from characters;
//! - a row with no word times (an untimed decode) falls back to the
//!   largest-overlap rule of [`crate::diarize::attribute_clusters`], unsplit.
//!
//! It never fails a meeting. Every error path is a [`DiarizeResult`] that
//! leaves the transcript exactly as transcription wrote it (plain labels) and
//! the row in the state transcription gave it — matrix row 7's
//! `diarize_sidecar_degrade` — and one log line:
//! `diarize result=skipped|failed|ok clusters=… speech_s=… wall_ms=…`.
//!
//! Ported ideas (docs/ANALYZER-RESEARCH.md, "2026-11-07 wiring" section): the
//! hedgehog analyzer's cosine distance cut (0.75, provisional) and its 1.5 s
//! speaker window as the shortest turn worth an embedding (provisional). Both
//! have an env override for evals only.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::asr_engine::TimedSpan;
use crate::db::Database;
use crate::diarize::{
    attribute_clusters, cluster_track, DiarizeError, DiarizePool, DiarizedSegment, MeetingTracks,
    TargetMode,
};
use crate::diarize_metrics::CosineDistance;
use crate::meeting_asr::{build_speaker_turns, ChunkStatus, JsonProgressStore, ProgressStore};
use crate::meeting_matrix::{diarize_sidecar_degrade, SpeakerLabels};
use crate::meetings::MeetingSegment;

/// The `meeting:progress` state emitted while this pass runs; the UI reads
/// it as the stage "Finding speakers" (`meetings/progress.ts`).
pub const FINDING_SPEAKERS_STATE: &str = "finding_speakers";

/// PROVISIONAL cosine DISTANCE cut for agglomerative clustering, ported from
/// the hedgehog analyzer (DIARIZE.md "Pipeline", `_cluster`). Not measured on
/// Yap's own corpus yet; `YAP_EVAL_CLUSTER_DISTANCE` overrides it for evals
/// only.
pub const PROVISIONAL_CLUSTER_DISTANCE: f32 = 0.75;

/// PROVISIONAL shortest turn this caller asks the sidecar to embed — the
/// hedgehog analyzer's 1.5 s speaker window (`_w_spk`). The diarization
/// crates deliberately carry no default (diarize_wire_unit_discipline.rs);
/// this is the caller's number, and `YAP_EVAL_MIN_TURN_MS` overrides it for
/// evals only.
pub const PROVISIONAL_SHORTEST_TURN: Duration = Duration::from_millis(1500);

/// A word belongs to a stored row when its midpoint lies inside the row, with
/// this much slack for float rounding of the stored times.
const ROW_SLACK_SECONDS: f64 = 0.01;

/// Which diarizer a pipeline job uses.
#[derive(Clone)]
pub enum DiarizeBackend {
    /// No diarization (tests that are not about it, a CLI without models).
    Off,
    /// The bundled `yap-diarize` sidecar ([`crate::diarize::pool`]), with the
    /// vendored model pair from the catalog.
    Shipped,
    /// A caller-owned pool whose models are already loaded (stub-process
    /// tests, evals).
    Pool(Arc<DiarizePool>),
}

impl DiarizeBackend {
    pub fn is_off(&self) -> bool {
        matches!(self, DiarizeBackend::Off)
    }
}

/// How the pass ended. Never a meeting failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiarizeResult {
    Ok,
    Skipped(&'static str),
    Failed(String),
}

impl DiarizeResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            DiarizeResult::Ok => "ok",
            DiarizeResult::Skipped(_) => "skipped",
            DiarizeResult::Failed(_) => "failed",
        }
    }
}

/// What [`diarize_meeting`] did, for the log line and the tests.
#[derive(Debug, Clone)]
pub struct DiarizeOutcome {
    pub result: DiarizeResult,
    pub track: Option<i64>,
    /// Distinct clusters the sidecar's turns were assigned.
    pub clusters: usize,
    /// Seconds of speech the clustered turns cover.
    pub speech_seconds: f64,
    pub wall_ms: u128,
    /// Rows whose `cluster_index` was written.
    pub rows_attributed: usize,
    /// Rows split at a speaker switch (each becomes 2+ rows).
    pub rows_split: usize,
    pub turns: usize,
    pub boundary_words_reassigned: usize,
    /// `Attributed`, or `Plain` with the matrix-row-7 reason on a failure.
    pub labels: SpeakerLabels,
}

impl DiarizeOutcome {
    fn new(result: DiarizeResult) -> Self {
        DiarizeOutcome {
            result,
            track: None,
            clusters: 0,
            speech_seconds: 0.0,
            wall_ms: 0,
            rows_attributed: 0,
            rows_split: 0,
            turns: 0,
            boundary_words_reassigned: 0,
            labels: SpeakerLabels::Attributed,
        }
    }

    /// `diarize result=… clusters=… speech_s=… wall_ms=…` plus the reason.
    /// No transcript text, no paths.
    pub fn log_line(&self, meeting_id: &str) -> String {
        let reason = match &self.result {
            DiarizeResult::Ok => String::new(),
            DiarizeResult::Skipped(why) => format!(" reason={why}"),
            DiarizeResult::Failed(tag) => format!(" reason={tag}"),
        };
        format!(
            "diarize result={} clusters={} speech_s={:.1} wall_ms={} meeting={meeting_id} track={} rows={} split={}{reason}",
            self.result.as_str(),
            self.clusters,
            self.speech_seconds,
            self.wall_ms,
            self.track.map(|t| t.to_string()).unwrap_or_else(|| "-".into()),
            self.rows_attributed,
            self.rows_split,
        )
    }
}

fn cluster_distance() -> CosineDistance {
    let value = std::env::var("YAP_EVAL_CLUSTER_DISTANCE")
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|v| v.is_finite() && *v > CosineDistance::MIN && *v <= CosineDistance::MAX)
        .unwrap_or(PROVISIONAL_CLUSTER_DISTANCE);
    CosineDistance::new(value)
}

fn shortest_turn() -> Duration {
    std::env::var("YAP_EVAL_MIN_TURN_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(PROVISIONAL_SHORTEST_TURN)
}

/// The ASR word rows one track's transcript rows were cut from, read back off
/// that track's resume ledger with the SAME ownership rule
/// `meeting_pipeline::chunk_segments` used to write them (a word belongs to the
/// window whose content span holds its start). Time-ordered. Empty when the
/// ledger is gone or the decode carried no word times.
pub fn ledger_words(progress_dir: &Path, meeting_id: &str, track: i64) -> Vec<TimedSpan> {
    let key = crate::meeting_pipeline::ledger_key(meeting_id, track);
    let Ok(progress) = JsonProgressStore::new(progress_dir).load(&key) else {
        return Vec::new();
    };
    let mut words: Vec<TimedSpan> = Vec::new();
    for chunk in &progress.chunks {
        if chunk.status != ChunkStatus::Done || !chunk.timestamp_kind.has_times() {
            continue;
        }
        let lo = chunk.content_start_seconds - 1e-6;
        let hi = chunk.content_end_seconds;
        words.extend(
            chunk
                .spans
                .iter()
                .filter(|s| s.start_seconds >= lo && s.start_seconds < hi)
                .filter(|s| !s.text.trim().is_empty())
                .cloned(),
        );
    }
    words.sort_by(|a, b| a.start_seconds.total_cmp(&b.start_seconds));
    words
}

/// The words whose midpoint lies inside `row`.
fn words_of_row(words: &[TimedSpan], row: &MeetingSegment) -> Vec<TimedSpan> {
    words
        .iter()
        .filter(|w| {
            let mid = (w.start_seconds + w.end_seconds.max(w.start_seconds)) / 2.0;
            mid >= row.start_seconds - ROW_SLACK_SECONDS
                && mid <= row.end_seconds + ROW_SLACK_SECONDS
        })
        .cloned()
        .collect()
}

/// One piece of a split row: start, end, text, cluster.
pub type SegmentPiece = (f64, f64, String, Option<i64>);

/// Attribute (and where needed split) one track's stored rows from the
/// diarized turns. Pure over its inputs apart from the two DB writes.
fn write_clusters(
    db: &Database,
    meeting_id: &str,
    track: i64,
    words: &[TimedSpan],
    diarized: &[DiarizedSegment],
    outcome: &mut DiarizeOutcome,
) -> Result<(), String> {
    let rows: Vec<MeetingSegment> = db
        .list_meeting_segments(meeting_id)?
        .into_iter()
        .filter(|s| s.track == track)
        .collect();
    let mut assignments: Vec<(String, Option<i64>)> = Vec::new();
    for row in &rows {
        let row_words = words_of_row(words, row);
        if row_words.is_empty() {
            // Untimed decode: whole-row largest-overlap attribution, unsplit.
            assignments.extend(attribute_clusters(std::slice::from_ref(row), diarized));
            continue;
        }
        let built = build_speaker_turns(track, &row_words, diarized);
        outcome.turns += built.turns.len();
        outcome.boundary_words_reassigned += built.boundary_words_reassigned;
        match built.turns.as_slice() {
            [] => {}
            [only] => assignments.push((row.id.clone(), only.cluster_index)),
            many => {
                let pieces: Vec<SegmentPiece> = many
                    .iter()
                    .map(|t| {
                        (
                            t.start_seconds,
                            t.end_seconds,
                            t.text.clone(),
                            t.cluster_index,
                        )
                    })
                    .collect();
                db.split_segment_by_speaker(meeting_id, &row.id, &pieces)?;
                outcome.rows_split += 1;
                outcome.rows_attributed += pieces.len();
            }
        }
    }
    outcome.rows_attributed += db.set_segment_clusters(meeting_id, &assignments)?;
    log::info!(
        "meeting_diarize turns built n={} boundary_words_reassigned={} words={} track={track} meeting={meeting_id}",
        outcome.turns,
        outcome.boundary_words_reassigned,
        words.len()
    );
    Ok(())
}

fn shipped_pool() -> Result<&'static DiarizePool, DiarizeResult> {
    use crate::models::{
        diarize_model_for_role, diarize_model_path, is_diarize_ready, DiarizeModelRole,
    };
    if !is_diarize_ready() {
        return Err(DiarizeResult::Skipped("models_missing"));
    }
    let (Some(seg), Some(emb)) = (
        diarize_model_for_role(DiarizeModelRole::Segmentation),
        diarize_model_for_role(DiarizeModelRole::Embedding),
    ) else {
        return Err(DiarizeResult::Skipped("models_missing"));
    };
    let pool = crate::diarize::pool();
    pool.load_models(&diarize_model_path(seg), &diarize_model_path(emb))
        .map_err(|e| DiarizeResult::Failed(format!("load_{}", e.tag())))?;
    Ok(pool)
}

fn failed(err: &DiarizeError) -> DiarizeOutcome {
    let labels = diarize_sidecar_degrade(err);
    // `saving_battery:*` is the resource governor deferring heavy work, not a
    // sidecar failure.
    let result = if err.tag().starts_with("saving_battery") {
        DiarizeResult::Skipped("deferred")
    } else {
        DiarizeResult::Failed(err.tag().to_string())
    };
    let mut outcome = DiarizeOutcome::new(result);
    outcome.labels = labels;
    outcome
}

/// Run the diarization pass for one finished meeting and write its clusters.
///
/// Never returns an error and never changes the meeting's state: on any
/// failure the transcript keeps the labels transcription gave it. Logs the
/// `diarize result=…` line (and the matrix-row-7 degrade line on a failure).
pub fn diarize_meeting(
    db: &Database,
    meeting_id: &str,
    progress_dir: &Path,
    backend: &DiarizeBackend,
) -> DiarizeOutcome {
    let started = Instant::now();
    let mut outcome = run(db, meeting_id, progress_dir, backend);
    outcome.wall_ms = started.elapsed().as_millis();
    match &outcome.result {
        DiarizeResult::Failed(_) => {
            log::warn!("{}", outcome.log_line(meeting_id));
            if let Some(reason) = outcome.labels.reason() {
                log::warn!("meeting_diarize meeting={meeting_id} {}", reason.log_line());
            }
        }
        _ => log::info!("{}", outcome.log_line(meeting_id)),
    }
    outcome
}

fn run(
    db: &Database,
    meeting_id: &str,
    progress_dir: &Path,
    backend: &DiarizeBackend,
) -> DiarizeOutcome {
    let pool: &DiarizePool = match backend {
        DiarizeBackend::Off => return DiarizeOutcome::new(DiarizeResult::Skipped("off")),
        DiarizeBackend::Pool(pool) => pool.as_ref(),
        DiarizeBackend::Shipped => match shipped_pool() {
            Ok(pool) => pool,
            Err(result) => {
                let mut outcome = DiarizeOutcome::new(result);
                if matches!(outcome.result, DiarizeResult::Failed(_)) {
                    outcome.labels = diarize_sidecar_degrade(&DiarizeError::Unavailable);
                }
                return outcome;
            }
        },
    };
    let meeting = match db.get_meeting(meeting_id) {
        Ok(Some(m)) => m,
        Ok(None) => return DiarizeOutcome::new(DiarizeResult::Skipped("deleted")),
        Err(_) => return DiarizeOutcome::new(DiarizeResult::Failed("db".into())),
    };
    let existing = |p: Option<&String>| p.map(std::path::PathBuf::from).filter(|p| p.is_file());
    let Some(mic) = existing(meeting.mic_wav_path.as_ref()) else {
        return DiarizeOutcome::new(DiarizeResult::Skipped("no_audio"));
    };
    let sys = existing(meeting.sys_wav_path.as_ref());
    let tracks = MeetingTracks {
        mic_wav: &mic,
        system_wav: sys.as_deref(),
    };
    let diarized = match cluster_track(
        pool,
        tracks,
        meeting.kind(),
        cluster_distance(),
        TargetMode::FullClustering,
        shortest_turn(),
    ) {
        Ok(d) => d,
        Err(e) => return failed(&e),
    };
    let clusters: BTreeSet<i64> = diarized.iter().filter_map(|d| d.cluster_index).collect();
    if clusters.is_empty() {
        let mut outcome = DiarizeOutcome::new(DiarizeResult::Skipped("no_clusters"));
        outcome.track = diarized.first().map(|d| d.track);
        return outcome;
    }
    let track = diarized[0].track;
    let mut outcome = DiarizeOutcome::new(DiarizeResult::Ok);
    outcome.track = Some(track);
    outcome.clusters = clusters.len();
    outcome.speech_seconds = diarized.iter().map(|d| d.duration()).sum();
    let words = ledger_words(progress_dir, meeting_id, track);
    if let Err(e) = write_clusters(db, meeting_id, track, &words, &diarized, &mut outcome) {
        log::warn!("meeting_diarize write_failed meeting={meeting_id} error=\"{e}\"");
        let mut failed = DiarizeOutcome::new(DiarizeResult::Failed("db".into()));
        failed.track = Some(track);
        failed.labels = diarize_sidecar_degrade(&DiarizeError::Protocol);
        return failed;
    }
    outcome
}
