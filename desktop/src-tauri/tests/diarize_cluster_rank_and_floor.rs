//! YV126 acceptance — a cluster count is not a verdict.
//!
//! ```sh
//! cargo test --test diarize_cluster_rank_and_floor
//! ```
//!
//! Merged finding #25's second half. The plan's matrix-#8 sanity gate rejected a
//! whole diarization pass when the cluster count exceeded `max(8, attendees ×
//! 2)`. A manually started meeting has no attendee count — every meeting in this
//! backlog, since the calendar is yap24 — so the cap is 8, and a real six-person
//! far-field room legitimately produces ten to fifteen raw clusters before any
//! merge. The gate therefore discards diarization exactly where it is hardest to
//! get and most wanted.
//!
//! The replacement ranks by speech time and surfaces above a floor: a chip per
//! cluster carrying ≥30 s AND ≥3 turns, everything else rolled into one "Other"
//! bucket, and NOTHING discarded — every turn keeps its stored `cluster_index`
//! so YV130's correction UX can promote or merge it later.
//!
//! The floors are surfacing decisions, not accuracy thresholds: they change what
//! a person is shown, never who the clusterer said was speaking. That is why
//! they are constants here and why no eval-harness measurement gates them.

use wilson_voice_lib::diarize::{
    rank_and_floor, DiarizedSegment, CHIP_FLOOR_SECONDS, CHIP_FLOOR_TURNS,
};
use wilson_voice_lib::meetings::{cluster_speaker_label, MIC_TRACK, OTHER_SPEAKER_LABEL};

/// `turns` turns of `each` seconds, all in cluster `cluster`, laid end to end
/// after `from`.
fn cluster(cluster: i64, turns: usize, each: f64, from: f64) -> Vec<DiarizedSegment> {
    (0..turns)
        .map(|i| {
            let start = from + i as f64 * (each + 1.0);
            DiarizedSegment::new(MIC_TRACK, start, start + each, Some(cluster))
        })
        .collect()
}

/// The far-field classroom the old gate threw away: twelve raw clusters, four of
/// which carry the conversation.
///
/// The eight below the floor are deliberately not all the same KIND of small —
/// two of them clear one floor and fail the other, which is the case a
/// single-condition implementation ("just use 30 seconds") gets wrong.
fn twelve_raw_clusters() -> Vec<DiarizedSegment> {
    let mut segments = Vec::new();
    // Four real participants: comfortably past both floors.
    segments.extend(cluster(0, 12, 20.0, 0.0)); // 240s, 12 turns — the instructor
    segments.extend(cluster(1, 6, 15.0, 300.0)); //  90s,  6 turns
    segments.extend(cluster(2, 4, 12.0, 500.0)); //  48s,  4 turns
    segments.extend(cluster(3, 3, 10.5, 600.0)); //  31.5s, 3 turns — barely in
                                                 // Eight that are not participants:
    segments.extend(cluster(4, 8, 3.0, 700.0)); //  24s,  8 turns — enough turns, not enough speech
    segments.extend(cluster(5, 2, 22.0, 800.0)); //  44s,  2 turns — enough speech, not enough turns
    for id in 6..12i64 {
        segments.extend(cluster(id, 2, 4.0, 900.0 + (id as f64 * 20.0))); // 8s, 2 turns
    }
    segments
}

/// The headline: four chips and one bucket, out of twelve raw clusters, and
/// never a rejection.
#[test]
fn twelve_raw_clusters_become_four_chips_and_one_other_bucket() {
    let segments = twelve_raw_clusters();
    let ranking = rank_and_floor(&segments);

    assert_eq!(
        ranking.surfaced.len(),
        4,
        "surfaced: {:?}",
        ranking
            .surfaced
            .iter()
            .map(|c| (c.cluster_index, c.speech_seconds, c.turns))
            .collect::<Vec<_>>()
    );
    assert_eq!(ranking.other.len(), 8, "everything else is in the bucket");

    assert_eq!(
        ranking.chips(),
        vec![
            cluster_speaker_label(1),
            cluster_speaker_label(2),
            cluster_speaker_label(3),
            cluster_speaker_label(4),
            OTHER_SPEAKER_LABEL.to_string(),
        ],
        "four names and one bucket — not fifteen chips, and not a rejection"
    );

    // Ranked by speech time, so `Speaker 1` is the person who spoke most rather
    // than whoever the clusterer happened to number 0.
    assert_eq!(
        ranking
            .surfaced
            .iter()
            .map(|c| c.cluster_index)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert!(ranking.surfaced[0].speech_seconds > ranking.surfaced[1].speech_seconds);

    // Nothing was discarded: every raw cluster is still accounted for, in one
    // list or the other, with its own stored index intact for YV130.
    let mut accounted: Vec<i64> = ranking
        .surfaced
        .iter()
        .chain(&ranking.other)
        .map(|c| c.cluster_index)
        .collect();
    accounted.sort_unstable();
    assert_eq!(accounted, (0..12).collect::<Vec<i64>>());
    assert!(ranking.other_speech_seconds() > 0.0);
}

/// BOTH floors are live. A cluster that clears one and fails the other is in the
/// bucket, which is what distinguishes this from a single-condition rule.
#[test]
fn each_floor_can_reject_a_cluster_on_its_own() {
    let ranking = rank_and_floor(&twelve_raw_clusters());
    let in_bucket = |id: i64| ranking.other.iter().any(|c| c.cluster_index == id);

    // 24 s across 8 turns: plenty of turns, not enough speech.
    let chatty = ranking
        .other
        .iter()
        .find(|c| c.cluster_index == 4)
        .expect("cluster 4 is in the bucket");
    assert!(chatty.turns >= CHIP_FLOOR_TURNS && chatty.speech_seconds < CHIP_FLOOR_SECONDS);
    assert!(in_bucket(4));

    // 44 s across 2 turns: plenty of speech, not part of the conversation.
    let monologue = ranking
        .other
        .iter()
        .find(|c| c.cluster_index == 5)
        .expect("cluster 5 is in the bucket");
    assert!(monologue.speech_seconds >= CHIP_FLOOR_SECONDS && monologue.turns < CHIP_FLOOR_TURNS);
    assert!(in_bucket(5));

    // …and one that clears both by a hair is surfaced, so the floors are not
    // just rejecting everything.
    let marginal = ranking
        .surfaced
        .iter()
        .find(|c| c.cluster_index == 3)
        .expect("cluster 3 is surfaced");
    assert!(marginal.speech_seconds >= CHIP_FLOOR_SECONDS && marginal.turns >= CHIP_FLOOR_TURNS);
}

/// The floors are the numbers finding #25 named, and they are the ones the code
/// applies. A test that only checked behaviour would pass with a floor of one
/// second.
#[test]
fn the_floors_are_the_ones_the_finding_named() {
    assert!((CHIP_FLOOR_SECONDS - 30.0).abs() < f64::EPSILON);
    assert_eq!(CHIP_FLOOR_TURNS, 3);
}

/// Ranking cannot fail, at any size. This is the property the hard reject did
/// not have.
#[test]
fn ranking_never_rejects_however_many_clusters_there_are() {
    // The pathological case the old gate was written for: 40 clusters, all tiny.
    let mut noise = Vec::new();
    for id in 0..40i64 {
        noise.extend(cluster(id, 1, 1.5, id as f64 * 5.0));
    }
    let ranking = rank_and_floor(&noise);
    assert!(ranking.surfaced.is_empty(), "none of them is a participant");
    assert_eq!(ranking.other.len(), 40, "…and none of them is thrown away");
    // **Never reject is not never degrade.** Every one of the forty is still
    // here, keyed by its stored index, for YV130 to promote or merge — that is
    // the "never rejects" property, and it is what the old hard gate did not
    // have. What a person is SHOWN is a different question, and matrix row 8
    // publishes the answer: a pass in which nothing cleared the floor found no
    // speaker, so it draws no chips at all. One "Other" chip over forty
    // fragments would present noise as attribution, and an earlier cut of this
    // file asserted exactly that — `matrix_row8_diarize_garbage_clusters` is
    // where the two answers were caught disagreeing.
    assert!(ranking.chips().is_empty(), "{:?}", ranking.chips());
    assert!(ranking.labels.is_plain());

    // The empty pass: no clusters, no chips, no panic.
    let empty = rank_and_floor(&[]);
    assert!(empty.surfaced.is_empty() && empty.other.is_empty());
    assert!(empty.chips().is_empty());

    // …and the bucket chip is still drawn when something DID clear the floor,
    // so the assertion above is about the degrade rather than about "Other"
    // having quietly stopped working.
    let mut mixed = cluster(0, 4, 40.0, 0.0);
    mixed.extend(cluster(1, 1, 2.0, 1000.0));
    let mixed = rank_and_floor(&mixed);
    assert_eq!(
        mixed.chips(),
        vec!["Speaker 1".to_string(), OTHER_SPEAKER_LABEL.to_string()]
    );

    // A stored index nobody surfaced still renders as something a person can
    // read rather than panicking.
    assert_eq!(ranking.label_for(999), OTHER_SPEAKER_LABEL);
}

/// The chip label is a RANK, not a cluster index. A meeting whose clusterer
/// numbered the loudest speaker 7 must still read "Speaker 1".
#[test]
fn chips_are_numbered_by_rank_not_by_cluster_index() {
    let mut segments = cluster(7, 10, 20.0, 0.0);
    segments.extend(cluster(2, 5, 15.0, 400.0));
    let ranking = rank_and_floor(&segments);

    assert_eq!(ranking.chips(), vec!["Speaker 1", "Speaker 2"]);
    assert_eq!(ranking.label_for(7), "Speaker 1");
    assert_eq!(ranking.label_for(2), "Speaker 2");
    assert_eq!(ranking.surfaced[0].rank, Some(1));
    assert_eq!(ranking.surfaced[0].cluster_index, 7);
}
