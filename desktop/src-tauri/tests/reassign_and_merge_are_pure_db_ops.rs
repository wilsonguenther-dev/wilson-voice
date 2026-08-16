//! YV130 acceptance — correction is a database operation, not an inference one.
//!
//! Finding #25's framing, made falsifiable. The claim is that neither
//! `reassign_segment` nor `merge_clusters` reaches `diarize.rs`'s sidecar
//! client: the segments were embedded and clustered when the meeting was
//! processed, and a correction edits a label on that finished work.
//!
//! **How this is a call-graph test and not a comment.** `diarize.rs` funnels
//! every way into the child through `DiarizePool::request_with_deadline` — it is
//! the module's own documented single entry point, and `load_models`, `diarize`
//! and `embed` are all thin wrappers over it. `diarize::sidecar_requests()`
//! counts every call to it, in every pool in the process, incremented on the
//! first line before the lock, before the spawn and before every failure branch.
//! So a reading that does not move across a correction is proof that nothing
//! asked the model anything — including asking and failing, which a purity claim
//! that only held on machines with no sidecar binary would not distinguish.
//!
//! Belt and braces: this test also builds a `DiarizePool` whose launcher
//! **panics**, and holds it live across every correction. It is the test double
//! the acceptance criterion names. Nothing in the correction path can reach it,
//! which is exactly the property under test — and if that ever stops being true
//! by way of a pool handed in as an argument, the panic is louder than a
//! counter.

mod support;

use support::corrections::{attribution_of, seed_clustered_meeting, voice, Utterance};
use support::{open_db, temp_dir};

use wilson_voice_lib::diarize::{sidecar_requests, DiarizeError, DiarizePool};

/// A sidecar client that cannot be used without failing the test.
fn panicking_pool() -> DiarizePool {
    DiarizePool::new(
        Box::new(|| -> Result<std::process::Command, DiarizeError> {
            panic!("a correction reached the diarize sidecar; corrections are DB operations")
        }),
        std::time::Duration::from_secs(1),
    )
}

#[test]
fn reassign_and_merge_never_reach_the_sidecar() {
    let dir = temp_dir("pure-db-ops");
    let db = open_db(&dir);

    // One person, split into two clusters — the first of finding #25's two
    // dominant error modes — plus a turn attributed to the wrong person.
    let (meeting_id, ids) = seed_clustered_meeting(
        &db,
        &dir,
        "Standup",
        &[
            Utterance::new("morning all", 0, voice(0, 1)),
            Utterance::new("blocked on the migration", 0, voice(0, 2)),
            Utterance::new("i can pick that up", 1, voice(0, 3)),
            Utterance::new("ship it today", 2, voice(1, 1)),
        ],
    );

    let double = panicking_pool();
    assert!(
        !double.is_warm(),
        "the test double has not been used before the corrections run"
    );
    let before = sidecar_requests();

    // Error mode 2: this turn was attributed to the wrong person.
    let reassigned = db.reassign_segment(&ids[3], Some("profile-jeisil")).unwrap();
    assert_eq!(reassigned.prior_speaker_id, None);
    assert_eq!(reassigned.speaker_id.as_deref(), Some("profile-jeisil"));

    // Error mode 1: one voice, two clusters. Merge 1 into 0.
    let merged = db.merge_clusters(&meeting_id, 0, 1).unwrap();
    assert_eq!(merged.moved, 1, "the one segment in cluster 1 moved");
    assert!(
        merged.locked_labels.is_empty(),
        "neither cluster carried a user-confirmed label"
    );

    let after = sidecar_requests();
    assert_eq!(
        after, before,
        "reassign + merge made {} sidecar request(s); both are supposed to be \
         UPDATE statements over already-embedded segments",
        after - before
    );
    assert!(
        !double.is_warm(),
        "no correction may hold a sidecar process open"
    );

    // …and the corrections actually happened, so the counter is measuring a
    // real code path rather than two no-ops.
    let (cluster, speaker, locked) = attribution_of(&dir, &ids[2]);
    assert_eq!(cluster, Some(0), "the merged segment moved cluster");
    assert_eq!(speaker, None);
    assert!(!locked, "a merge is not a confirmation of anybody's label");

    let (_, speaker, locked) = attribution_of(&dir, &ids[3]);
    assert_eq!(speaker.as_deref(), Some("profile-jeisil"));
    assert!(locked, "a person reassigned this segment by hand");
}

/// A reassign is a decision, including the decision that a turn belongs to
/// nobody. Clearing a label still locks the segment, so the next retroactive
/// batch cannot put the name straight back on.
#[test]
fn clearing_a_label_is_also_a_decision_and_is_also_free() {
    let dir = temp_dir("pure-db-clear");
    let db = open_db(&dir);
    let (_, ids) = seed_clustered_meeting(
        &db,
        &dir,
        "Client call",
        &[Utterance::new("thanks for joining", 0, voice(0, 1))],
    );

    db.reassign_segment(&ids[0], Some("profile-wilson")).unwrap();
    let before = sidecar_requests();
    let cleared = db.reassign_segment(&ids[0], None).unwrap();
    assert_eq!(sidecar_requests(), before);

    assert_eq!(cleared.prior_speaker_id.as_deref(), Some("profile-wilson"));
    assert_eq!(cleared.speaker_id, None);
    let (_, speaker, locked) = attribution_of(&dir, &ids[0]);
    assert_eq!(speaker, None);
    assert!(
        locked,
        "deciding a turn belongs to nobody is a decision, and an automated pass \
         must not undo it"
    );
}

/// A merge that would swallow a user-confirmed label reports it rather than
/// resolving it. Two locked names inside one cluster is a question for the
/// person merging; picking one silently is the failure this item exists to stop.
#[test]
fn a_merge_reports_the_confirmed_labels_it_would_bring_together() {
    let dir = temp_dir("pure-db-locked");
    let db = open_db(&dir);
    let (meeting_id, ids) = seed_clustered_meeting(
        &db,
        &dir,
        "Two names, one merge",
        &[
            Utterance::new("first voice", 0, voice(0, 1)),
            Utterance::new("second voice", 1, voice(1, 1)),
        ],
    );
    support::corrections::confirm(&db, &ids[0], "profile-wilson");
    support::corrections::confirm(&db, &ids[1], "profile-jeisil");

    let before = sidecar_requests();
    let merged = db.merge_clusters(&meeting_id, 0, 1).unwrap();
    assert_eq!(sidecar_requests(), before, "still no model call");

    assert_eq!(merged.moved, 1);
    assert_eq!(
        merged.locked_labels,
        vec!["profile-jeisil".to_string(), "profile-wilson".to_string()],
        "both confirmed names come back for the UI to raise"
    );
    // And neither label was touched: a re-grouping is not permission to
    // overwrite an answer somebody gave.
    assert_eq!(
        attribution_of(&dir, &ids[0]).1.as_deref(),
        Some("profile-wilson")
    );
    assert_eq!(
        attribution_of(&dir, &ids[1]).1.as_deref(),
        Some("profile-jeisil")
    );
}

/// Merging a cluster into itself is refused rather than silently doing nothing —
/// a no-op that reports success is how a broken affordance stays broken.
#[test]
fn a_cluster_cannot_be_merged_into_itself() {
    let dir = temp_dir("pure-db-self-merge");
    let db = open_db(&dir);
    let (meeting_id, _) = seed_clustered_meeting(
        &db,
        &dir,
        "Self merge",
        &[Utterance::new("only voice", 0, voice(0, 1))],
    );
    assert!(db.merge_clusters(&meeting_id, 0, 0).is_err());
}
