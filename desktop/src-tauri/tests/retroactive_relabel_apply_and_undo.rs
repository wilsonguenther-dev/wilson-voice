//! YV130 acceptance — retroactive relabel: offered, applied as one batch, undone
//! by one call.
//!
//! Finding #31. The epic plan described this as "fully automatic, no review, no
//! undo", in the one feature whose entire value is trust. What ships instead has
//! three separate stages and the middle one is the user:
//!
//!   1. `plan_retroactive_relabel` — a READ. It is the offer *"this voice
//!      appears in N earlier meetings — label them too?"*, and it writes nothing.
//!   2. **Apply** — `apply_retroactive_relabel`, one transaction, one `batch_id`,
//!      one history row per touched segment holding what that segment held
//!      before.
//!   3. **Undo** — `undo_relabel_batch`, one call, the exact prior state of the
//!      whole batch restored, including the segments that held nothing.
//!
//! "Not now" is the absence of step 2, which is why it can never half-happen.

mod support;

use support::corrections::{attribution_of, history_rows, seed_clustered_meeting, voice, Utterance};
use support::{open_db, temp_dir};

use wilson_voice_lib::diarize::sidecar_requests;

/// Three earlier meetings, one voice in each: the scenario the acceptance
/// criterion names, with the third of them deliberately already carrying a
/// DIFFERENT label so the `NULL` case and the overwrite case are both in the
/// same batch and both have to come back.
#[test]
fn apply_writes_one_batch_and_one_undo_restores_every_prior_value() {
    let dir = temp_dir("relabel-apply-undo");
    let db = open_db(&dir);

    let (m1, a) = seed_clustered_meeting(
        &db,
        &dir,
        "Monday sync",
        &[Utterance::new("same voice, first meeting", 0, voice(0, 1))],
    );
    let (m2, b) = seed_clustered_meeting(
        &db,
        &dir,
        "Tuesday review",
        &[Utterance::new("same voice, second meeting", 0, voice(0, 2))],
    );
    let (m3, c) = seed_clustered_meeting(
        &db,
        &dir,
        "Wednesday retro",
        &[Utterance::new("same voice, third meeting", 0, voice(0, 3))],
    );
    assert_ne!(m1, m2);
    assert_ne!(m2, m3);

    // The third one was already attributed — to the wrong person, unlocked (a
    // clustering pass's guess, not a human's answer). Its prior value is a
    // string, where the other two are NULL.
    db.plan_retroactive_relabel("profile-someone-else", &[c[0].clone()])
        .and_then(|p| db.apply_retroactive_relabel(&p))
        .unwrap();
    assert_eq!(
        attribution_of(&dir, &c[0]).1.as_deref(),
        Some("profile-someone-else")
    );

    let candidates = vec![a[0].clone(), b[0].clone(), c[0].clone()];
    let before = sidecar_requests();

    // ── the offer ────────────────────────────────────────────────────────────
    let plan = db
        .plan_retroactive_relabel("profile-wilson", &candidates)
        .unwrap();
    assert_eq!(plan.segment_ids.len(), 3, "three prior appearances");
    assert_eq!(plan.meetings, 3, "…across three earlier meetings");
    assert!(plan.skipped_locked.is_empty());
    assert!(!plan.is_empty());

    // The offer is a read: nothing has changed yet, which is what makes "Not
    // now" a real answer rather than a rollback.
    assert_eq!(attribution_of(&dir, &a[0]).1, None);
    assert_eq!(
        attribution_of(&dir, &c[0]).1.as_deref(),
        Some("profile-someone-else")
    );

    // ── Apply ────────────────────────────────────────────────────────────────
    let batch = db.apply_retroactive_relabel(&plan).unwrap();
    assert_eq!(batch.touched, 3);
    assert!(batch.skipped_locked.is_empty());
    assert_eq!(
        history_rows(&dir, &batch.batch_id),
        3,
        "one history row per touched segment, under one batch id"
    );
    assert_eq!(db.relabel_batch_size(&batch.batch_id).unwrap(), 3);

    for id in &candidates {
        let (_, speaker, locked) = attribution_of(&dir, id);
        assert_eq!(
            speaker.as_deref(),
            Some("profile-wilson"),
            "every segment in the batch now carries the new speaker"
        );
        assert!(
            !locked,
            "a batch is a decision about a VOICE, not a per-segment confirmation \
             — locking here would make the batch's own output immune to the next \
             correction"
        );
    }

    // ── Undo ─────────────────────────────────────────────────────────────────
    let undo = db.undo_relabel_batch(&batch.batch_id).unwrap();
    assert_eq!(undo.restored, 3, "one call, the whole batch");
    assert!(undo.skipped_changed.is_empty());

    assert_eq!(
        attribution_of(&dir, &a[0]).1,
        None,
        "a segment that was never labelled goes back to never labelled — the \
         NULL case finding #31 explicitly asks for"
    );
    assert_eq!(attribution_of(&dir, &b[0]).1, None);
    assert_eq!(
        attribution_of(&dir, &c[0]).1.as_deref(),
        Some("profile-someone-else"),
        "…and a segment that held a different label gets THAT back, not NULL"
    );

    assert_eq!(
        history_rows(&dir, &batch.batch_id),
        0,
        "the batch is consumed: an undo that leaves its own record behind can be \
         applied twice"
    );
    assert_eq!(
        sidecar_requests(),
        before,
        "plan, apply and undo are all database operations"
    );
}

/// A second undo of the same batch is a no-op, not an error. Undo is a user
/// gesture and a double click on it should do nothing loudly.
#[test]
fn undoing_twice_is_a_no_op() {
    let dir = temp_dir("relabel-double-undo");
    let db = open_db(&dir);
    let (_, ids) = seed_clustered_meeting(
        &db,
        &dir,
        "Once",
        &[Utterance::new("hello", 0, voice(0, 1))],
    );

    let plan = db
        .plan_retroactive_relabel("profile-wilson", &[ids[0].clone()])
        .unwrap();
    let batch = db.apply_retroactive_relabel(&plan).unwrap();
    assert_eq!(db.undo_relabel_batch(&batch.batch_id).unwrap().restored, 1);
    let again = db.undo_relabel_batch(&batch.batch_id).unwrap();
    assert_eq!(again.restored, 0);
    assert!(again.skipped_changed.is_empty());
    assert_eq!(attribution_of(&dir, &ids[0]).1, None, "still unlabelled");
}

/// An undo must not discard a decision made AFTER the batch. A segment the user
/// has since reassigned by hand is left alone and named.
#[test]
fn undo_declines_to_clobber_a_later_hand_correction() {
    let dir = temp_dir("relabel-undo-conflict");
    let db = open_db(&dir);
    let (_, ids) = seed_clustered_meeting(
        &db,
        &dir,
        "Changed my mind",
        &[
            Utterance::new("first", 0, voice(0, 1)),
            Utterance::new("second", 0, voice(0, 2)),
        ],
    );

    let plan = db
        .plan_retroactive_relabel("profile-wilson", &[ids[0].clone(), ids[1].clone()])
        .unwrap();
    let batch = db.apply_retroactive_relabel(&plan).unwrap();
    assert_eq!(batch.touched, 2);

    // The user then corrects one of them by hand.
    db.reassign_segment(&ids[1], Some("profile-jeisil")).unwrap();

    let undo = db.undo_relabel_batch(&batch.batch_id).unwrap();
    assert_eq!(undo.restored, 1);
    assert_eq!(
        undo.skipped_changed,
        vec![ids[1].clone()],
        "the hand-corrected segment is reported, not silently reverted"
    );
    assert_eq!(attribution_of(&dir, &ids[0]).1, None);
    assert_eq!(
        attribution_of(&dir, &ids[1]).1.as_deref(),
        Some("profile-jeisil"),
        "a decision made after the batch survives the batch's undo"
    );
}

/// A candidate that already carries this exact speaker is not in the batch. Not
/// a safety rule — a bookkeeping one: an unchanged row would still write a
/// history row, and an undo that restores a value to itself makes its own count
/// a lie.
#[test]
fn already_this_speaker_is_not_in_the_batch() {
    let dir = temp_dir("relabel-unchanged");
    let db = open_db(&dir);
    let (_, ids) = seed_clustered_meeting(
        &db,
        &dir,
        "Already right",
        &[
            Utterance::new("first", 0, voice(0, 1)),
            Utterance::new("second", 0, voice(0, 2)),
        ],
    );

    let first = db
        .plan_retroactive_relabel("profile-wilson", &[ids[0].clone()])
        .unwrap();
    db.apply_retroactive_relabel(&first).unwrap();

    let plan = db
        .plan_retroactive_relabel("profile-wilson", &[ids[0].clone(), ids[1].clone()])
        .unwrap();
    assert_eq!(plan.segment_ids, vec![ids[1].clone()]);
    assert_eq!(plan.skipped_unchanged, vec![ids[0].clone()]);
    let batch = db.apply_retroactive_relabel(&plan).unwrap();
    assert_eq!(batch.touched, 1);
    assert_eq!(history_rows(&dir, &batch.batch_id), 1);
}

/// Deleting the meeting takes its relabel history with it. A history row names a
/// speaker and a segment of a meeting the user just asked to be gone; leaving it
/// behind is the same half-delete `delete_meeting_with_audio` exists to refuse.
#[test]
fn deleting_a_meeting_deletes_its_relabel_history() {
    let dir = temp_dir("relabel-delete-cascade");
    let db = open_db(&dir);
    let (meeting_id, ids) = seed_clustered_meeting(
        &db,
        &dir,
        "Delete me",
        &[Utterance::new("something private", 0, voice(0, 1))],
    );

    let plan = db
        .plan_retroactive_relabel("profile-wilson", &[ids[0].clone()])
        .unwrap();
    let batch = db.apply_retroactive_relabel(&plan).unwrap();
    assert_eq!(history_rows(&dir, &batch.batch_id), 1);

    db.delete_meeting(&meeting_id).unwrap();
    assert_eq!(
        history_rows(&dir, &batch.batch_id),
        0,
        "who was in that meeting must not outlive the meeting"
    );
}

/// An empty offer is empty, and the UI shows no prompt at all for one. An
/// "Apply" that would change zero rows still teaches the user that Yap acts on
/// its own.
#[test]
fn an_offer_with_nothing_to_change_is_empty() {
    let dir = temp_dir("relabel-empty");
    let db = open_db(&dir);
    let plan = db.plan_retroactive_relabel("profile-wilson", &[]).unwrap();
    assert!(plan.is_empty());
    assert_eq!(plan.meetings, 0);
    let batch = db.apply_retroactive_relabel(&plan).unwrap();
    assert_eq!(batch.touched, 0);
    assert_eq!(history_rows(&dir, &batch.batch_id), 0);
}
