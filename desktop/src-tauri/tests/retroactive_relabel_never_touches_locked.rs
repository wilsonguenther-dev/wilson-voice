//! YV130 acceptance — a confirmed label is never in somebody else's batch.
//!
//! The rule (spec, from finding #31): `locked = 1` assignments are never
//! included in a retroactive-relabel batch — a user-confirmed label on a
//! specific segment is never silently touched by a different segment's
//! correction, **even when its embedding would otherwise match**.
//!
//! This is the rule that makes the feature safe to say yes to. Without it,
//! "label the 40 other places this voice appears" is an operation that can
//! silently overwrite the one place you had already gone to the trouble of
//! fixing by hand, and after that nobody presses Apply again.
//!
//! It is enforced in three places on purpose, and all three are tested here:
//!
//!   * in the **plan**, so the offer's count is honest about what Apply would
//!     do;
//!   * again inside the **transaction**, against the row rather than the plan,
//!     because the plan was computed before the user read the offer and a
//!     segment they locked in the meantime is theirs. A stale plan is not
//!     permission; and
//!   * in **undo**, which is the direction the rule is easiest to lose. Undo's
//!     "has somebody touched this since?" test cannot be a comparison of
//!     `speaker_id` values alone: a segment the user hand-confirmed to the
//!     batch's OWN answer holds exactly the value the batch wrote, so by value
//!     it is indistinguishable from an untouched row — and reverting it would
//!     discard the confirmation and clear the lock, which is the same silent
//!     overwrite the other two guards exist to prevent, arriving from the far
//!     side. The lock is what distinguishes them, so the lock is what undo asks
//!     about.

mod support;

use support::corrections::{
    attribution_of, confirm, history_rows, seed_clustered_meeting, voice, Utterance,
};
use support::{open_db, temp_dir};

/// A locked segment whose embedding is IDENTICAL to the matched voice's — the
/// strongest possible case for including it, and it is still excluded.
#[test]
fn a_locked_segment_is_excluded_even_when_its_embedding_matches_exactly() {
    let dir = temp_dir("locked-excluded");
    let db = open_db(&dir);

    let (_, ids) = seed_clustered_meeting(
        &db,
        &dir,
        "One voice, three turns",
        &[
            Utterance::new("turn one", 0, voice(0, 1)),
            // Byte-for-byte the same vector as turn one: nothing about the
            // audio distinguishes this segment from the ones the batch will
            // relabel. Only the human decision does.
            Utterance::new("turn two", 0, voice(0, 1)),
            Utterance::new("turn three", 0, voice(0, 1)),
        ],
    );

    // The user has already decided turn two by hand, and decided it is somebody
    // else.
    confirm(&db, &ids[1], "profile-jeisil");
    assert!(attribution_of(&dir, &ids[1]).2, "…and that decision is locked");

    let plan = db
        .plan_retroactive_relabel("profile-wilson", &ids.clone())
        .unwrap();
    assert_eq!(
        plan.segment_ids,
        vec![ids[0].clone(), ids[2].clone()],
        "the offer counts two segments, not three"
    );
    assert_eq!(
        plan.skipped_locked,
        vec![ids[1].clone()],
        "the excluded segment is named rather than quietly missing"
    );

    let batch = db.apply_retroactive_relabel(&plan).unwrap();
    assert_eq!(batch.touched, 2);
    assert_eq!(
        history_rows(&dir, &batch.batch_id),
        2,
        "no history row was written for the segment that was not touched"
    );

    let (_, speaker, locked) = attribution_of(&dir, &ids[1]);
    assert_eq!(
        speaker.as_deref(),
        Some("profile-jeisil"),
        "the confirmed label survived a batch that matched its audio perfectly"
    );
    assert!(locked, "…and it is still confirmed");

    // The other two did get relabelled, so the exclusion is a real filter over a
    // real batch rather than a batch that did nothing.
    assert_eq!(
        attribution_of(&dir, &ids[0]).1.as_deref(),
        Some("profile-wilson")
    );
    assert_eq!(
        attribution_of(&dir, &ids[2]).1.as_deref(),
        Some("profile-wilson")
    );
}

/// The plan is advisory; the transaction is authoritative. A segment locked
/// between the offer being shown and Apply being pressed is still excluded.
#[test]
fn a_segment_locked_after_the_offer_is_still_excluded_by_apply() {
    let dir = temp_dir("locked-after-offer");
    let db = open_db(&dir);

    let (_, ids) = seed_clustered_meeting(
        &db,
        &dir,
        "Locked while the offer was on screen",
        &[
            Utterance::new("turn one", 0, voice(0, 1)),
            Utterance::new("turn two", 0, voice(0, 2)),
        ],
    );

    let plan = db
        .plan_retroactive_relabel("profile-wilson", &ids.clone())
        .unwrap();
    assert_eq!(plan.segment_ids.len(), 2, "both are in the offer");

    // …and while the offer sits on screen, the user fixes one of them by hand.
    confirm(&db, &ids[1], "profile-jeisil");

    let batch = db.apply_retroactive_relabel(&plan).unwrap();
    assert_eq!(batch.touched, 1, "the transaction re-checked the row");
    assert_eq!(batch.skipped_locked, vec![ids[1].clone()]);
    assert_eq!(
        history_rows(&dir, &batch.batch_id),
        1,
        "and wrote no history for it, so undo cannot resurrect the overwrite"
    );
    assert_eq!(
        attribution_of(&dir, &ids[1]).1.as_deref(),
        Some("profile-jeisil")
    );
}

/// Locking is what `reassign_segment` does, so the two halves of the rule are
/// the same mechanism: correcting one segment by hand is exactly what makes it
/// ineligible for the next batch.
#[test]
fn a_hand_correction_is_what_makes_a_segment_ineligible() {
    let dir = temp_dir("locked-by-reassign");
    let db = open_db(&dir);
    let (_, ids) = seed_clustered_meeting(
        &db,
        &dir,
        "Reassign then batch",
        &[Utterance::new("only turn", 0, voice(0, 1))],
    );

    let before = db
        .plan_retroactive_relabel("profile-wilson", &[ids[0].clone()])
        .unwrap();
    assert_eq!(before.segment_ids.len(), 1, "eligible before the correction");

    db.reassign_segment(&ids[0], Some("profile-jeisil")).unwrap();

    let after = db
        .plan_retroactive_relabel("profile-wilson", &[ids[0].clone()])
        .unwrap();
    assert!(after.is_empty(), "ineligible after it");
    assert_eq!(after.skipped_locked, vec![ids[0].clone()]);
}

/// The undo-side case, and the one a value comparison cannot see: the user
/// AGREED with the batch by hand.
///
/// A batch labels three turns `profile-wilson`. The user then taps "Wilson" on
/// turn two — a real decision, made after the batch, that happens to name the
/// same speaker. By `speaker_id` alone that row is byte-identical to the two
/// nobody touched. Only its lock says a person went there. So undo must read
/// the lock: turn two keeps its label, keeps its lock, and is named in
/// `skipped_changed`, while the two untouched turns revert.
#[test]
fn a_segment_confirmed_to_the_batchs_own_answer_survives_undo() {
    let dir = temp_dir("locked-undo-agrees");
    let db = open_db(&dir);

    let (_, ids) = seed_clustered_meeting(
        &db,
        &dir,
        "The user agreed, by hand",
        &[
            Utterance::new("turn one", 0, voice(0, 1)),
            Utterance::new("turn two", 0, voice(0, 2)),
            Utterance::new("turn three", 0, voice(0, 3)),
        ],
    );

    let plan = db
        .plan_retroactive_relabel("profile-wilson", &ids.clone())
        .unwrap();
    let batch = db.apply_retroactive_relabel(&plan).unwrap();
    assert_eq!(batch.touched, 3);
    assert!(
        !attribution_of(&dir, &ids[1]).2,
        "a batch write is not a per-segment confirmation, so nothing is locked yet"
    );

    // …and then the user confirms turn two by hand — to the SAME speaker.
    confirm(&db, &ids[1], "profile-wilson");
    assert!(attribution_of(&dir, &ids[1]).2, "the hand tap locked it");

    let undo = db.undo_relabel_batch(&batch.batch_id).unwrap();
    assert_eq!(
        undo.restored, 2,
        "only the two segments nobody had confirmed were reverted"
    );
    assert_eq!(
        undo.skipped_changed,
        vec![ids[1].clone()],
        "the confirmed segment is named rather than silently reverted"
    );

    let (_, speaker, locked) = attribution_of(&dir, &ids[1]);
    assert_eq!(
        speaker.as_deref(),
        Some("profile-wilson"),
        "the hand-confirmed label survived an undo that shared its value"
    );
    assert!(
        locked,
        "…and undo did not clear the confirmation flag along with it"
    );

    // The rest of the batch really did come back, so this is a filter over a
    // real undo rather than an undo that did nothing.
    assert_eq!(attribution_of(&dir, &ids[0]).1, None);
    assert_eq!(attribution_of(&dir, &ids[2]).1, None);
    assert_eq!(
        history_rows(&dir, &batch.batch_id),
        0,
        "the batch is spent either way — an undo that leaves its record behind \
         can be applied twice"
    );
}

/// The same rule stated as the invariant it is: undo never lowers a lock. A
/// hand correction to a DIFFERENT speaker is already skipped by value, but the
/// flag has to survive too — a reverted label and a cleared confirmation are
/// the same loss to the person who made it.
#[test]
fn undo_never_clears_a_lock_it_did_not_set() {
    let dir = temp_dir("locked-undo-never-lowers");
    let db = open_db(&dir);
    let (_, ids) = seed_clustered_meeting(
        &db,
        &dir,
        "Corrected after the batch",
        &[Utterance::new("only turn", 0, voice(0, 1))],
    );

    let plan = db
        .plan_retroactive_relabel("profile-wilson", &[ids[0].clone()])
        .unwrap();
    let batch = db.apply_retroactive_relabel(&plan).unwrap();
    assert_eq!(batch.touched, 1);

    confirm(&db, &ids[0], "profile-jeisil");

    let undo = db.undo_relabel_batch(&batch.batch_id).unwrap();
    assert_eq!(undo.restored, 0);
    assert_eq!(undo.skipped_changed, vec![ids[0].clone()]);

    let (_, speaker, locked) = attribution_of(&dir, &ids[0]);
    assert_eq!(speaker.as_deref(), Some("profile-jeisil"));
    assert!(locked, "the confirmation outlives the batch's undo");
}
