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
//! It is enforced in two places on purpose, and both are tested here:
//!
//!   * in the **plan**, so the offer's count is honest about what Apply would
//!     do; and
//!   * again inside the **transaction**, against the row rather than the plan,
//!     because the plan was computed before the user read the offer and a
//!     segment they locked in the meantime is theirs. A stale plan is not
//!     permission.

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
