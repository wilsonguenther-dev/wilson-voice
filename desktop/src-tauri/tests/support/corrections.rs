//! YV130 — the scaffolding the four correction acceptance tests share.
//!
//! Two pieces, both deliberately small:
//!
//!   * `seed_clustered_meeting` stands up what a finished diarization pass
//!     leaves behind — segments with a `cluster_index` and a retained embedding
//!     — through the SAME accessor the real pass will use
//!     (`Database::record_segment_clustering`), so a test never writes a row
//!     shape production cannot produce.
//!   * `voice` synthesises embeddings for two distinguishable speakers. They are
//!     deterministic: a fixed base direction per speaker plus a deterministic
//!     per-utterance jitter, so "does split recover the original two clusters"
//!     asks about the algorithm and never about a seed.

#![allow(dead_code)]

use wilson_voice_lib::db::Database;
use wilson_voice_lib::meetings::NewMeetingSegment;

/// The synthetic embedding width. Not 192, not 512, and not a constant borrowed
/// from anywhere in `src/` — the whole point of finding #19 is that this
/// codebase holds no opinion about how wide a model is, so a fixture that
/// matched CAM++'s real width would be quietly asserting one.
pub const FIXTURE_WIDTH: usize = 8;

/// A deterministic embedding for `speaker`, utterance `n`.
///
/// Each speaker gets its own base axis; the jitter is a fixed integer hash
/// scaled down, so two utterances of the same speaker are near each other and
/// far from the other speaker's, exactly as real embeddings of two people are —
/// and identically on every run, on every machine.
pub fn voice(speaker: usize, n: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; FIXTURE_WIDTH];
    v[speaker % FIXTURE_WIDTH] = 1.0;
    // A tiny, reproducible wobble on the two axes after the base one.
    let jitter = ((n * 2_654_435_761) % 97) as f32 / 4000.0;
    v[(speaker + 1) % FIXTURE_WIDTH] += jitter;
    v[(speaker + 2) % FIXTURE_WIDTH] -= jitter / 2.0;
    v
}

/// An all-zero embedding of the same width — a vector with no direction at all.
///
/// Not a hypothetical: this codebase already documents two ways to get one. A
/// segment over silence embeds to nothing, and OS-4's ghost tap (see
/// `syscapture.rs`) delivers all-zero buffers while the meeting looks healthy.
/// Nothing upstream rejects it either — `record_segment_clustering` only refuses
/// an EMPTY slice — so a zero vector reaches `meeting_segments.embedding`
/// through the production accessor, which is why these fixtures write it that
/// way rather than poking the row.
pub fn silent() -> Vec<f32> {
    vec![0.0f32; FIXTURE_WIDTH]
}

/// One segment's worth of fixture: what it says, which cluster it was assigned
/// to, and (optionally) the embedding it was assigned by.
pub struct Utterance {
    pub text: &'static str,
    pub cluster: i64,
    pub embedding: Option<Vec<f32>>,
}

impl Utterance {
    pub fn new(text: &'static str, cluster: i64, embedding: Vec<f32>) -> Self {
        Self {
            text,
            cluster,
            embedding: Some(embedding),
        }
    }

    /// A segment the clustering pass never embedded — every row written before
    /// migration 5, and the case `split_cluster` must refuse to guess at.
    pub fn without_embedding(text: &'static str, cluster: i64) -> Self {
        Self {
            text,
            cluster,
            embedding: None,
        }
    }
}

/// A finished meeting whose segments carry clusters and retained embeddings.
///
/// Returns `(meeting_id, segment_ids)` in transcript order.
pub fn seed_clustered_meeting(
    db: &Database,
    dir: &std::path::Path,
    title: &str,
    utterances: &[Utterance],
) -> (String, Vec<String>) {
    let meeting = db.create_meeting(title, "manual").expect("create meeting");
    let segments: Vec<NewMeetingSegment> = utterances
        .iter()
        .enumerate()
        .map(|(i, u)| NewMeetingSegment::new(i as f64 * 4.0, i as f64 * 4.0 + 3.5, u.text))
        .collect();
    db.append_meeting_segments(&meeting.id, &segments)
        .expect("append segments");

    let ids: Vec<String> = db
        .list_meeting_segments(&meeting.id)
        .expect("list segments")
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(ids.len(), utterances.len());

    for (id, u) in ids.iter().zip(utterances) {
        match &u.embedding {
            Some(e) => db
                .record_segment_clustering(id, u.cluster, e)
                .expect("record clustering"),
            None => set_cluster_only(dir, id, u.cluster),
        }
    }
    (meeting.id, ids)
}

/// A cluster assignment with NO retained embedding — the pre-migration-5 shape.
///
/// Written through a second connection to the same file, the way
/// `fts_docsize_rows` already reads shadow tables here, because
/// `record_segment_clustering` deliberately refuses it: the production path
/// always has the vector it clustered by, and only history does not.
pub fn set_cluster_only(dir: &std::path::Path, segment_id: &str, cluster: i64) {
    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).expect("open db file");
    conn.execute(
        "UPDATE meeting_segments SET cluster_index = ?2 WHERE id = ?1",
        rusqlite::params![segment_id, cluster],
    )
    .expect("set cluster");
}

/// The raw attribution of one segment: `(cluster_index, speaker_id, locked)`.
///
/// Read from the FILE rather than through a typed accessor on purpose: the
/// claims these tests make are about what is durably in the database after a
/// correction, and a getter that happened to share a bug with its setter would
/// agree with itself.
pub fn attribution_of(
    dir: &std::path::Path,
    segment_id: &str,
) -> (Option<i64>, Option<String>, bool) {
    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).expect("open db file");
    conn.query_row(
        "SELECT cluster_index, speaker_id, speaker_locked FROM meeting_segments WHERE id = ?1",
        rusqlite::params![segment_id],
        |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, i64>(2)? != 0,
            ))
        },
    )
    .expect("read attribution")
}

/// How many history rows exist for `batch_id`, read from the file.
pub fn history_rows(dir: &std::path::Path, batch_id: &str) -> i64 {
    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).expect("open db file");
    conn.query_row(
        "SELECT COUNT(*) FROM speaker_relabel_history WHERE batch_id = ?1",
        rusqlite::params![batch_id],
        |r| r.get(0),
    )
    .expect("count history")
}

/// Lock one segment's label the way a person does — through the real
/// `reassign_segment`, not by poking the flag, so the fixture cannot drift from
/// what the production path actually writes.
pub fn confirm(db: &Database, segment_id: &str, speaker_id: &str) {
    db.reassign_segment(segment_id, Some(speaker_id))
        .expect("reassign");
}
