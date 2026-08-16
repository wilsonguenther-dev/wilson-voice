//! YV130 — correction is a database operation, not an inference one.
//!
//! Finding #25 names the two dominant real diarization error modes and the fact
//! that neither has a correction path today:
//!
//!   * **one person split into two clusters** — the fix is a merge, and a merge
//!     is `UPDATE meeting_segments SET cluster_index = ?`;
//!   * **a turn attributed to the wrong person** — the fix is a reassign, and a
//!     reassign is `UPDATE meeting_segments SET speaker_id = ?`.
//!
//! Neither re-runs the model. The segments were embedded and clustered when the
//! meeting was processed; a correction changes a **label** on work that is
//! already done. That claim is not a comment — `tests/reassign_and_merge_are_pure_db_ops.rs`
//! reads [`crate::diarize::sidecar_requests`], the counter every request against
//! every `DiarizePool` in the process bumps, across each of these calls and
//! requires it not to move.
//!
//! **Split is the one genuinely harder case**, and the schema is why it is
//! merely harder rather than impossible. Undoing a bad merge means
//! re-partitioning a cluster's members by what they sounded like, and a
//! post-clustering segment row does not otherwise retain that. Migration 5
//! attaches the cluster-assignment embedding to the segment (768 bytes at the
//! width CAM++ reports) specifically so [`split_cluster`] is a local recompute
//! over stored vectors and not a request to diarize the meeting again.
//!
//! **Retroactive relabel is offered, never performed** (finding #31 — the epic
//! plan described this as "fully automatic, no review, no undo", in the one
//! feature whose value is entirely a function of trust). The three stages are
//! separate functions on purpose, and the middle one is the user:
//! [`plan_retroactive_relabel`] reads and decides nothing, the UI shows *"this
//! voice appears in N earlier meetings — label them too?"* with **Apply** and
//! **Not now**, and only [`apply_retroactive_relabel`] writes. "Not now" is the
//! absence of a call, which is why it can never half-happen.
//!
//! ## No tuned constant lives here
//!
//! This module ships **zero thresholds**. That is not restraint, it is scope:
//! deciding WHICH segments are the same voice is YV129's `match_cluster`, tuned
//! against YV120's harness; YV130 is handed a candidate set and owns what
//! happens to it. The one comparison [`split_cluster`] makes is a *relative*
//! one — each member goes to whichever of two seeds it is closer to — which has
//! no number to move. `tests/correction_ships_no_tuned_threshold.rs` reads this
//! file and fails on any float literal in it at all.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::diarize_metrics::{cosine_similarity, is_zero_norm, CosineDistance};

/// Bytes per stored embedding component. The BLOB is little-endian `f32`, the
/// same encoding the sidecar's JSON numbers land in once parsed, and the same
/// one [`decode_embedding`] reads back.
pub const EMBEDDING_COMPONENT_BYTES: usize = 4;

/// One segment as the correction surface sees it: what it is attributed to,
/// whether a person said so, and what it sounded like.
///
/// Deliberately NOT [`crate::meetings::MeetingSegment`]. That type is the
/// transcript's row — text, timing, track — and it is serialised to the
/// frontend on every meeting open. A 768-byte vector per segment has no business
/// crossing that boundary, so the embedding lives on the type that needs it and
/// nowhere else.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentAttribution {
    pub id: String,
    pub meeting_id: String,
    pub start_seconds: f64,
    /// Which offline cluster this segment landed in. `None` = never clustered.
    pub cluster_index: Option<i64>,
    /// The enrolled voice this segment is attributed to. `None` = unlabelled,
    /// which is a state undo has to be able to restore.
    pub speaker_id: Option<String>,
    /// A person decided this specific segment. Never set by a batch.
    pub speaker_locked: bool,
    /// The cluster-assignment embedding, decoded. `None` = the row has no BLOB
    /// (every segment recorded before migration 5, and every segment whose
    /// meeting was never diarized).
    #[serde(skip)]
    pub embedding: Option<Vec<f32>>,
}

/// `f32` little-endian, exactly as [`decode_embedding`] reads it.
pub fn encode_embedding(values: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * EMBEDDING_COMPONENT_BYTES);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Read a stored embedding back, refusing anything that is not one.
///
/// Checked on every READ rather than only on write, for the same reason YV128
/// gives for its blob-length invariant: a truncated or half-written BLOB that is
/// silently accepted becomes a shorter vector that then scores plausibly against
/// everything. The width is not asserted against a constant here — this module
/// has no opinion about how wide the model is, and finding #19 is that nothing
/// in this codebase should — only that the bytes ARE whole `f32`s, that there is
/// at least one, and that none of them is NaN or infinite.
pub fn decode_embedding(blob: &[u8]) -> Result<Vec<f32>, String> {
    if blob.is_empty() {
        return Err("segment embedding blob is empty".into());
    }
    if blob.len() % EMBEDDING_COMPONENT_BYTES != 0 {
        return Err(format!(
            "segment embedding blob is {} bytes, which is not a whole number of f32s",
            blob.len()
        ));
    }
    let mut out = Vec::with_capacity(blob.len() / EMBEDDING_COMPONENT_BYTES);
    for chunk in blob.chunks_exact(EMBEDDING_COMPONENT_BYTES) {
        let v = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        if !v.is_finite() {
            return Err("segment embedding blob holds a non-finite component".into());
        }
        out.push(v);
    }
    Ok(out)
}

/// What [`reassign_segment`] did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reassignment {
    pub segment_id: String,
    pub prior_speaker_id: Option<String>,
    pub speaker_id: Option<String>,
}

/// Attribute one segment to `speaker_id`, or to nobody with `None`.
///
/// **Always sets `speaker_locked = 1`, including when clearing.** The flag means
/// "a person decided this segment", and deciding that a turn belongs to nobody
/// is a decision; leaving it unlocked would let the next retroactive batch put a
/// name back on the exact segment somebody just took one off.
///
/// No model call. `UPDATE`, one row, by primary key.
pub fn reassign_segment(
    conn: &Connection,
    segment_id: &str,
    speaker_id: Option<&str>,
) -> Result<Reassignment, String> {
    let prior: Option<Option<String>> = conn
        .query_row(
            "SELECT speaker_id FROM meeting_segments WHERE id = ?1",
            params![segment_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let prior_speaker_id = prior.ok_or_else(|| format!("no segment {segment_id}"))?;
    conn.execute(
        "UPDATE meeting_segments SET speaker_id = ?2, speaker_locked = 1 WHERE id = ?1",
        params![segment_id, speaker_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(Reassignment {
        segment_id: segment_id.to_string(),
        prior_speaker_id,
        speaker_id: speaker_id.map(str::to_string),
    })
}

/// What [`merge_clusters`] did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeOutcome {
    pub meeting_id: String,
    pub into_cluster: i64,
    pub from_cluster: i64,
    pub moved: usize,
    /// Distinct `speaker_id`s that were already user-confirmed (`locked = 1`) on
    /// either side. Reported, never resolved: two locked names inside one
    /// cluster is a question for the person merging, and silently dropping one
    /// of them is the failure this whole item exists to stop.
    pub locked_labels: Vec<String>,
}

/// One person, split into two clusters, becomes one cluster.
///
/// Moves membership only — `speaker_id` and `speaker_locked` are not read, not
/// written, and not merged. A cluster is a grouping the machine proposed; a
/// label is something a person may have confirmed, and re-grouping is not
/// permission to overwrite one. Any locked labels caught up in the merge come
/// back in [`MergeOutcome::locked_labels`] for the UI to raise.
///
/// No model call. One `UPDATE … WHERE cluster_index = ?`.
pub fn merge_clusters(
    conn: &Connection,
    meeting_id: &str,
    into_cluster: i64,
    from_cluster: i64,
) -> Result<MergeOutcome, String> {
    if into_cluster == from_cluster {
        return Err("a cluster cannot be merged into itself".into());
    }
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT speaker_id FROM meeting_segments
             WHERE meeting_id = ?1 AND cluster_index IN (?2, ?3)
               AND speaker_locked = 1 AND speaker_id IS NOT NULL
             ORDER BY speaker_id ASC",
        )
        .map_err(|e| e.to_string())?;
    let locked_labels: Vec<String> = stmt
        .query_map(params![meeting_id, into_cluster, from_cluster], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);

    let moved = conn
        .execute(
            "UPDATE meeting_segments SET cluster_index = ?2
             WHERE meeting_id = ?1 AND cluster_index = ?3",
            params![meeting_id, into_cluster, from_cluster],
        )
        .map_err(|e| e.to_string())?;
    Ok(MergeOutcome {
        meeting_id: meeting_id.to_string(),
        into_cluster,
        from_cluster,
        moved,
        locked_labels,
    })
}

/// A cluster's members, bipartitioned — the pure half of [`split_cluster`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitPartition {
    /// The part that keeps the original `cluster_index`.
    pub kept: Vec<String>,
    /// The part that moves to a fresh index.
    pub moved: Vec<String>,
}

/// Why a split did not happen. A refusal, never a guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplitRefusal {
    /// Fewer than two members carry a USABLE retained embedding, so there is
    /// nothing to re-partition BY. This is what a pre-migration-5 meeting looks
    /// like, and saying so is the honest answer.
    NotEnoughEmbeddings { with_embedding: usize },
    /// Two members are of different widths, so they are not in the same vector
    /// space and the distance between them would be a number with no meaning.
    WidthMismatch { expected: usize, found: usize },
    /// One or more members carry an all-zero embedding, which is not a position
    /// in the space — it is the absence of one.
    ///
    /// This is its own refusal and not a low score because of a unit trap:
    /// `cosine_similarity` answers `0.0` for a zero-norm vector meaning "no
    /// information", and `CosineDistance::from_similarity(0.0)` is `1.0`, which
    /// in the distance unit means "further than almost anything". Left
    /// unchecked, one silent-utterance embedding wins farthest-pair seeding
    /// against every real voice in the cluster and the two people who actually
    /// needed separating stay together. [`split_cluster`] never surfaces this
    /// variant — it drops degenerate members into `unpartitioned` first, so one
    /// bad vector cannot disable split for a whole cluster — but
    /// [`split_partition`] refuses rather than partition around one.
    DegenerateEmbedding { ids: Vec<String> },
    /// Every member is at distance zero from every other. Real embeddings of two
    /// different people are not; this is a fixture or a duplicated row, and any
    /// partition of it would be arbitrary.
    Indistinguishable,
}

impl SplitRefusal {
    pub fn message(&self) -> String {
        match self {
            SplitRefusal::NotEnoughEmbeddings { with_embedding } => format!(
                "this cluster has {with_embedding} segment(s) with a usable retained embedding; \
                 splitting needs at least two"
            ),
            SplitRefusal::DegenerateEmbedding { ids } => format!(
                "{} segment(s) in this cluster carry an all-zero embedding, which says \
                 nothing about who spoke; splitting cannot place them",
                ids.len()
            ),
            SplitRefusal::WidthMismatch { expected, found } => format!(
                "this cluster mixes embedding widths ({expected} and {found}); \
                 they are not the same vector space"
            ),
            SplitRefusal::Indistinguishable => {
                "every segment in this cluster is identical; there is nothing to split on".into()
            }
        }
    }
}

/// Bipartition a cluster's members by their retained embeddings.
///
/// **Two parts, because merge is pairwise.** `k = 2` is not a tuning choice
/// anybody could have made differently — split is the inverse of [`merge_clusters`],
/// which joins exactly two clusters, so undoing one produces exactly two. There
/// is no k to select and therefore no criterion for selecting it.
///
/// The partition is farthest-pair seeding: take the two members with the
/// greatest cosine distance between them as seeds, then send every other member
/// to whichever seed it is closer to. Every comparison is relative — "closer to
/// A or to B" — so the function holds no threshold, and both parts are non-empty
/// by construction because each seed is in its own.
///
/// **A zero-norm member is refused, not measured.** Farthest-pair seeding is
/// only sound if every input is a position in the space, and an all-zero
/// embedding is not one: `cosine_similarity` reports `0.0` for it meaning "no
/// information", which becomes distance `1.0` meaning "far", so a single
/// degenerate vector would out-seed two genuinely different speakers and the
/// split would silently fail to separate them. See
/// [`SplitRefusal::DegenerateEmbedding`]; [`split_cluster`] filters these out
/// before it ever gets here.
///
/// Deterministic on every tie, which matters because a correction the user
/// repeats must land the same way twice: the seed pair is the first maximal pair
/// in index order, and a member equidistant from both seeds goes to the seed
/// that came first.
pub fn split_partition(members: &[(String, Vec<f32>)]) -> Result<SplitPartition, SplitRefusal> {
    if members.len() < 2 {
        return Err(SplitRefusal::NotEnoughEmbeddings {
            with_embedding: members.len(),
        });
    }
    let width = members[0].1.len();
    if let Some((_, odd)) = members.iter().find(|(_, v)| v.len() != width) {
        return Err(SplitRefusal::WidthMismatch {
            expected: width,
            found: odd.len(),
        });
    }
    let degenerate: Vec<String> = members
        .iter()
        .filter(|(_, v)| is_zero_norm(v))
        .map(|(id, _)| id.clone())
        .collect();
    if !degenerate.is_empty() {
        return Err(SplitRefusal::DegenerateEmbedding { ids: degenerate });
    }

    let distance =
        |a: &[f32], b: &[f32]| CosineDistance::from_similarity(cosine_similarity(a, b)).get();

    let mut seed_a = 0usize;
    let mut seed_b = 1usize;
    let mut widest = distance(&members[0].1, &members[1].1);
    for i in 0..members.len() {
        for j in (i + 1)..members.len() {
            let d = distance(&members[i].1, &members[j].1);
            if d > widest {
                widest = d;
                seed_a = i;
                seed_b = j;
            }
        }
    }
    // `CosineDistance::MIN` is the identity of this scale, not a tuned floor:
    // "no distance at all" is the only value for which a bipartition has no
    // information to work from.
    if widest <= CosineDistance::MIN {
        return Err(SplitRefusal::Indistinguishable);
    }

    let mut kept = Vec::new();
    let mut moved = Vec::new();
    for (i, (id, vector)) in members.iter().enumerate() {
        if i == seed_a {
            kept.push(id.clone());
        } else if i == seed_b {
            moved.push(id.clone());
        } else if distance(vector, &members[seed_b].1) < distance(vector, &members[seed_a].1) {
            moved.push(id.clone());
        } else {
            kept.push(id.clone());
        }
    }
    Ok(SplitPartition { kept, moved })
}

/// What [`split_cluster`] did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitOutcome {
    pub meeting_id: String,
    pub cluster_index: i64,
    pub new_cluster_index: i64,
    pub kept: usize,
    pub moved: usize,
    /// Members with no USABLE retained embedding — no BLOB at all, or an
    /// all-zero one, which says nothing about who spoke. They stay on the
    /// original cluster and are named here rather than assigned by coin-flip.
    pub unpartitioned: Vec<String>,
}

/// Undo a bad merge: re-partition one cluster into two, using each member's
/// retained embedding and nothing else.
///
/// No model call — the vectors are already on the rows, which is the entire
/// reason migration 5 puts them there. `speaker_id` and `speaker_locked` are
/// untouched on both sides for the same reason [`merge_clusters`] leaves them
/// alone: this changes a grouping, not anybody's confirmed answer.
pub fn split_cluster(
    conn: &Connection,
    meeting_id: &str,
    cluster_index: i64,
) -> Result<SplitOutcome, String> {
    let members = cluster_members(conn, meeting_id, cluster_index)?;
    if members.is_empty() {
        return Err(format!(
            "meeting {meeting_id} has no segments in cluster {cluster_index}"
        ));
    }
    // A zero-norm vector is filed with the un-embedded, not with the usable.
    // "The row has no BLOB" and "the BLOB says nothing" are the same fact about
    // whether this segment can be placed, and treating the second as a position
    // is what lets one silent utterance out-seed two real speakers. Dropping
    // them here re-checks the two-usable-members floor for free: `split_partition`
    // refuses with `NotEnoughEmbeddings` if too few survive.
    let mut with_embedding: Vec<(String, Vec<f32>)> = Vec::new();
    let mut unpartitioned: Vec<String> = Vec::new();
    for seg in &members {
        match &seg.embedding {
            Some(v) if !is_zero_norm(v) => with_embedding.push((seg.id.clone(), v.clone())),
            _ => unpartitioned.push(seg.id.clone()),
        }
    }
    let partition = split_partition(&with_embedding).map_err(|r| r.message())?;

    let next_index: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(cluster_index), -1) + 1 FROM meeting_segments
             WHERE meeting_id = ?1",
            params![meeting_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;

    for id in &partition.moved {
        conn.execute(
            "UPDATE meeting_segments SET cluster_index = ?2 WHERE id = ?1",
            params![id, next_index],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(SplitOutcome {
        meeting_id: meeting_id.to_string(),
        cluster_index,
        new_cluster_index: next_index,
        kept: partition.kept.len(),
        moved: partition.moved.len(),
        unpartitioned,
    })
}

/// Every segment of one cluster, embeddings decoded, earliest first.
pub fn cluster_members(
    conn: &Connection,
    meeting_id: &str,
    cluster_index: i64,
) -> Result<Vec<SegmentAttribution>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, meeting_id, start_seconds, cluster_index, speaker_id, speaker_locked,
                    embedding
             FROM meeting_segments
             WHERE meeting_id = ?1 AND cluster_index = ?2
             ORDER BY start_seconds ASC, id ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![meeting_id, cluster_index], map_attribution)
        .map_err(|e| e.to_string())?;
    collect_attributions(rows)
}

/// The same shape, for an arbitrary set of segment ids — what
/// [`plan_retroactive_relabel`] is handed by the matcher.
pub fn attributions_for(
    conn: &Connection,
    segment_ids: &[String],
) -> Result<Vec<SegmentAttribution>, String> {
    let mut out = Vec::with_capacity(segment_ids.len());
    let mut stmt = conn
        .prepare(
            "SELECT id, meeting_id, start_seconds, cluster_index, speaker_id, speaker_locked,
                    embedding
             FROM meeting_segments WHERE id = ?1",
        )
        .map_err(|e| e.to_string())?;
    for id in segment_ids {
        let row = stmt
            .query_row(params![id], map_attribution)
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(row) = row {
            out.push(row);
        }
    }
    Ok(out)
}

fn map_attribution(row: &rusqlite::Row<'_>) -> rusqlite::Result<SegmentAttribution> {
    let blob: Option<Vec<u8>> = row.get(6)?;
    Ok(SegmentAttribution {
        id: row.get(0)?,
        meeting_id: row.get(1)?,
        start_seconds: row.get(2)?,
        cluster_index: row.get(3)?,
        speaker_id: row.get(4)?,
        speaker_locked: row.get::<_, i64>(5)? != 0,
        // A blob that will not decode is dropped to `None` rather than failing
        // the whole read: an unreadable vector makes ONE segment un-splittable,
        // and `SplitRefusal::NotEnoughEmbeddings` says so, which is a better
        // outcome than a meeting that will not open.
        embedding: blob.and_then(|b| decode_embedding(&b).ok()),
    })
}

fn collect_attributions<I>(rows: I) -> Result<Vec<SegmentAttribution>, String>
where
    I: Iterator<Item = rusqlite::Result<SegmentAttribution>>,
{
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// The **offer**, computed and shown; never the act.
///
/// Naming a voice in one meeting is what produces the candidate set (YV129's
/// matcher decides which segments those are — this module is handed them and
/// never re-derives them, which is why it holds no threshold). What YV130 owns
/// is what the user is then asked, and what is excluded before they are asked
/// it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelabelPlan {
    pub speaker_id: String,
    /// The segments Apply would write, in a stable order.
    pub segment_ids: Vec<String>,
    /// How many distinct earlier meetings those segments span — the N in
    /// "this voice appears in N earlier meetings".
    pub meetings: usize,
    /// Candidates dropped because a person had already decided them.
    pub skipped_locked: Vec<String>,
    /// Candidates dropped because they already carry this exact `speaker_id`;
    /// writing them would produce history rows that undo to themselves.
    pub skipped_unchanged: Vec<String>,
}

impl RelabelPlan {
    /// Nothing to offer. The UI shows no prompt at all in this case rather than
    /// an offer that would do nothing — an "Apply" that changes zero rows still
    /// teaches the user that Yap acts on its own.
    pub fn is_empty(&self) -> bool {
        self.segment_ids.is_empty()
    }
}

/// Filter a candidate set into the batch Apply would write. Pure: no writes, no
/// model, no decision about who anybody is.
///
/// Two exclusions, both from the spec:
///
///   * **`speaker_locked = 1` is never in a batch.** A user-confirmed label on a
///     specific segment is never silently touched by a different segment's
///     correction — even when its embedding would otherwise match perfectly.
///     This is the rule that makes the feature safe to say yes to.
///   * **already-this-speaker is never in a batch.** Not a safety rule, a
///     bookkeeping one: an unchanged row would still write a history row, and an
///     undo that restores a value to itself makes the undo count a lie.
pub fn plan_retroactive_relabel(
    speaker_id: &str,
    candidates: &[SegmentAttribution],
) -> RelabelPlan {
    let mut segment_ids = Vec::new();
    let mut skipped_locked = Vec::new();
    let mut skipped_unchanged = Vec::new();
    let mut meetings: Vec<&str> = Vec::new();
    for c in candidates {
        if c.speaker_locked {
            skipped_locked.push(c.id.clone());
            continue;
        }
        if c.speaker_id.as_deref() == Some(speaker_id) {
            skipped_unchanged.push(c.id.clone());
            continue;
        }
        segment_ids.push(c.id.clone());
        if !meetings.contains(&c.meeting_id.as_str()) {
            meetings.push(&c.meeting_id);
        }
    }
    RelabelPlan {
        speaker_id: speaker_id.to_string(),
        segment_ids,
        meetings: meetings.len(),
        skipped_locked,
        skipped_unchanged,
    }
}

/// One applied batch, and the handle its undo is addressed to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelabelBatch {
    pub batch_id: String,
    pub speaker_id: String,
    pub touched: usize,
    /// Candidates the plan listed that the transaction itself declined, because
    /// they had been locked between the offer and the Apply.
    pub skipped_locked: Vec<String>,
}

/// **Apply** — one batch write, one undo handle.
///
/// Every touched segment's prior `speaker_id` (including `NULL`) and prior lock
/// state are recorded in `speaker_relabel_history` BEFORE the overwrite, under
/// one `batch_id`, inside one transaction. That is what makes
/// [`undo_relabel_batch`] a single reversible operation across the whole batch
/// rather than a per-segment manual repair, which is the difference finding #31
/// is actually asking for.
///
/// The lock check runs **again here**, inside the transaction, against the row
/// rather than against the plan. The plan was computed before the user read the
/// offer; a segment they locked in the meantime is theirs, and a stale plan is
/// not permission.
///
/// Batch writes leave `speaker_locked` at `0`. The user confirmed a **voice**,
/// not each of the N segments this inferred; treating the batch as a per-segment
/// confirmation would make its own output immune to the next correction.
pub fn apply_retroactive_relabel(
    conn: &mut Connection,
    plan: &RelabelPlan,
) -> Result<RelabelBatch, String> {
    let batch_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let mut touched = 0usize;
    let mut skipped_locked = Vec::new();
    for segment_id in &plan.segment_ids {
        let row: Option<(Option<String>, i64)> = tx
            .query_row(
                "SELECT speaker_id, speaker_locked FROM meeting_segments WHERE id = ?1",
                params![segment_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some((prior_speaker_id, prior_locked)) = row else {
            continue; // deleted between the offer and the Apply
        };
        if prior_locked != 0 {
            skipped_locked.push(segment_id.clone());
            continue;
        }
        tx.execute(
            "INSERT INTO speaker_relabel_history
             (batch_id, segment_id, prior_speaker_id, prior_speaker_locked, new_speaker_id,
              created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                batch_id,
                segment_id,
                prior_speaker_id,
                prior_locked,
                plan.speaker_id,
                now
            ],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE meeting_segments SET speaker_id = ?2 WHERE id = ?1",
            params![segment_id, plan.speaker_id],
        )
        .map_err(|e| e.to_string())?;
        touched += 1;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(RelabelBatch {
        batch_id,
        speaker_id: plan.speaker_id.clone(),
        touched,
        skipped_locked,
    })
}

/// What one undo did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoOutcome {
    pub batch_id: String,
    pub restored: usize,
    /// Segments the batch wrote that somebody has since changed by hand. Their
    /// prior value is NOT restored — undoing a batch must not silently discard a
    /// decision made after it — and they are named so the UI can say so.
    pub skipped_changed: Vec<String>,
}

/// **Undo** — one call, the exact prior state of the whole batch.
///
/// Restores `speaker_id` and `speaker_locked` from `speaker_relabel_history`,
/// including rows whose prior `speaker_id` was `NULL`, in one transaction. The
/// batch's history rows are consumed either way: an undo that leaves its own
/// record behind is an undo that can be applied twice.
///
/// An unknown or already-undone `batch_id` is `Ok` with `restored: 0`, not an
/// error. Undo is a user gesture, and a second click on it should be a no-op,
/// not a failure dialog.
pub fn undo_relabel_batch(conn: &mut Connection, batch_id: &str) -> Result<UndoOutcome, String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let rows: Vec<(String, Option<String>, i64, String)> = {
        let mut stmt = tx
            .prepare(
                "SELECT segment_id, prior_speaker_id, prior_speaker_locked, new_speaker_id
                 FROM speaker_relabel_history WHERE batch_id = ?1
                 ORDER BY segment_id ASC",
            )
            .map_err(|e| e.to_string())?;
        let collected = stmt
            .query_map(params![batch_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        collected
    };

    let mut restored = 0usize;
    let mut skipped_changed = Vec::new();
    for (segment_id, prior_speaker_id, prior_locked, new_speaker_id) in rows {
        let current: Option<Option<String>> = tx
            .query_row(
                "SELECT speaker_id FROM meeting_segments WHERE id = ?1",
                params![segment_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(current) = current else {
            continue; // segment deleted since; nothing to restore it to
        };
        if current.as_deref() != Some(new_speaker_id.as_str()) {
            skipped_changed.push(segment_id);
            continue;
        }
        tx.execute(
            "UPDATE meeting_segments SET speaker_id = ?2, speaker_locked = ?3 WHERE id = ?1",
            params![segment_id, prior_speaker_id, prior_locked],
        )
        .map_err(|e| e.to_string())?;
        restored += 1;
    }
    tx.execute(
        "DELETE FROM speaker_relabel_history WHERE batch_id = ?1",
        params![batch_id],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(UndoOutcome {
        batch_id: batch_id.to_string(),
        restored,
        skipped_changed,
    })
}

/// How many history rows one batch still holds — what a test asserts, and what
/// the UI reads to know whether its undo affordance is still live.
pub fn relabel_batch_size(conn: &Connection, batch_id: &str) -> Result<usize, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM speaker_relabel_history WHERE batch_id = ?1",
        params![batch_id],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n as usize)
    .map_err(|e| e.to_string())
}
