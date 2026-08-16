//! YV130 acceptance — split is a local recompute over stored vectors.
//!
//! Split is the one genuinely harder correction: undoing a bad merge means
//! re-partitioning a cluster's members by what they sounded like, and a
//! post-clustering segment row does not otherwise retain that. Migration 5
//! attaches the cluster-assignment embedding to the segment for exactly this
//! reason, so the fix stays local instead of becoming "diarize the meeting
//! again".
//!
//! The scenario below is the one the acceptance criterion names: a cluster that
//! is really two people, merged. Splitting it must recover the original two
//! groupings from the retained BLOBs alone, with `diarize::sidecar_requests()`
//! unmoved.

mod support;

use support::corrections::{attribution_of, seed_clustered_meeting, voice, Utterance};
use support::{open_db, temp_dir};

use wilson_voice_lib::diarize::sidecar_requests;
use wilson_voice_lib::speaker_corrections::{split_partition, SplitPartition, SplitRefusal};

#[test]
fn a_merged_cluster_splits_back_into_its_two_original_clusters() {
    let dir = temp_dir("split-retained");
    let db = open_db(&dir);

    // Two people, wrongly in one cluster: exactly the state a bad merge leaves.
    let (meeting_id, ids) = seed_clustered_meeting(
        &db,
        &dir,
        "Two people, one cluster",
        &[
            Utterance::new("a-one", 0, voice(0, 1)),
            Utterance::new("b-one", 0, voice(1, 1)),
            Utterance::new("a-two", 0, voice(0, 2)),
            Utterance::new("b-two", 0, voice(1, 2)),
            Utterance::new("a-three", 0, voice(0, 3)),
        ],
    );

    let before = sidecar_requests();
    let outcome = db.split_cluster(&meeting_id, 0).unwrap();
    assert_eq!(
        sidecar_requests(),
        before,
        "split re-partitions from the retained embeddings; it must not diarize \
         the meeting again"
    );

    assert_eq!(outcome.cluster_index, 0);
    assert_eq!(
        outcome.new_cluster_index, 1,
        "the moved part takes the next free index in this meeting"
    );
    assert!(
        outcome.unpartitioned.is_empty(),
        "every member carried an embedding"
    );
    assert_eq!(outcome.kept + outcome.moved, 5);

    // The partition IS the original two speakers: a-* together, b-* together.
    let cluster_of = |i: usize| attribution_of(&dir, &ids[i]).0.expect("clustered");
    let a = [cluster_of(0), cluster_of(2), cluster_of(4)];
    let b = [cluster_of(1), cluster_of(3)];
    assert!(
        a.iter().all(|c| *c == a[0]),
        "the three utterances of speaker A landed together: {a:?}"
    );
    assert!(
        b.iter().all(|c| *c == b[0]),
        "the two utterances of speaker B landed together: {b:?}"
    );
    assert_ne!(a[0], b[0], "…and A and B are not the same cluster: {a:?} {b:?}");

    // Nobody's label was touched — a re-grouping is not a relabel.
    for id in &ids {
        let (_, speaker, locked) = attribution_of(&dir, id);
        assert_eq!(speaker, None);
        assert!(!locked);
    }
}

/// Two splits in a row do not collide: the second moved part takes the next free
/// index rather than reusing one the first split already handed out.
#[test]
fn a_second_split_takes_the_next_free_index() {
    let dir = temp_dir("split-twice");
    let db = open_db(&dir);
    let (meeting_id, _) = seed_clustered_meeting(
        &db,
        &dir,
        "Split twice",
        &[
            Utterance::new("a-one", 0, voice(0, 1)),
            Utterance::new("b-one", 0, voice(1, 1)),
            Utterance::new("a-two", 0, voice(0, 2)),
            Utterance::new("b-two", 0, voice(1, 2)),
        ],
    );

    let first = db.split_cluster(&meeting_id, 0).unwrap();
    assert_eq!(first.new_cluster_index, 1);
    let second = db.split_cluster(&meeting_id, first.new_cluster_index).unwrap();
    assert_eq!(
        second.new_cluster_index, 2,
        "the next free index, not a reused one"
    );
}

/// A cluster whose members were never embedded cannot be split, and says so
/// rather than partitioning by coin flip. This is every meeting recorded before
/// migration 5.
#[test]
fn a_cluster_with_no_retained_embeddings_refuses_to_split() {
    let dir = temp_dir("split-no-embeddings");
    let db = open_db(&dir);
    let (meeting_id, _) = seed_clustered_meeting(
        &db,
        &dir,
        "Recorded before the vectors were kept",
        &[
            Utterance::without_embedding("who said this", 0),
            Utterance::without_embedding("or this", 0),
        ],
    );

    let before = sidecar_requests();
    let err = db.split_cluster(&meeting_id, 0).unwrap_err();
    assert_eq!(
        sidecar_requests(),
        before,
        "a refusal must not become a quiet re-diarize"
    );
    assert!(
        err.contains("0 segment(s) with a retained embedding"),
        "the refusal has to name the reason: {err}"
    );
}

/// A cluster where only SOME members were embedded splits the ones it can and
/// names the ones it cannot, leaving them where they were.
#[test]
fn members_without_an_embedding_stay_put_and_are_reported() {
    let dir = temp_dir("split-mixed");
    let db = open_db(&dir);
    let (meeting_id, ids) = seed_clustered_meeting(
        &db,
        &dir,
        "Mixed",
        &[
            Utterance::new("a-one", 0, voice(0, 1)),
            Utterance::new("b-one", 0, voice(1, 1)),
            Utterance::without_embedding("nobody kept my vector", 0),
        ],
    );

    let outcome = db.split_cluster(&meeting_id, 0).unwrap();
    assert_eq!(
        outcome.unpartitioned,
        vec![ids[2].clone()],
        "the un-embedded segment is named, not guessed at"
    );
    assert_eq!(
        attribution_of(&dir, &ids[2]).0,
        Some(0),
        "…and it stays on the original cluster"
    );
    assert_eq!(outcome.kept + outcome.moved, 2);
}

// ── the pure partition ───────────────────────────────────────────────────────

/// `k = 2` is arity, not tuning: split is the inverse of a pairwise merge. Both
/// parts are non-empty by construction because each seed is in its own.
#[test]
fn the_partition_is_deterministic_and_both_parts_are_non_empty() {
    let members: Vec<(String, Vec<f32>)> = vec![
        ("a1".into(), voice(0, 1)),
        ("b1".into(), voice(1, 1)),
        ("a2".into(), voice(0, 2)),
        ("b2".into(), voice(1, 2)),
    ];
    let first = split_partition(&members).unwrap();
    let second = split_partition(&members).unwrap();
    assert_eq!(first, second, "the same input must partition the same way");
    assert!(!first.kept.is_empty() && !first.moved.is_empty());

    let SplitPartition { kept, moved } = first;
    let mut all: Vec<&String> = kept.iter().chain(moved.iter()).collect();
    all.sort();
    assert_eq!(all, vec!["a1", "a2", "b1", "b2"], "nothing is lost or doubled");
    assert!(kept.contains(&"a1".to_string()) && kept.contains(&"a2".to_string()));
    assert!(moved.contains(&"b1".to_string()) && moved.contains(&"b2".to_string()));
}

/// Identical vectors are refused. Real embeddings of two different people are
/// not at distance zero from each other; anything that is, is a fixture, a
/// duplicated row or a degenerate all-zero vector, and any partition of it would
/// be arbitrary.
#[test]
fn an_indistinguishable_cluster_is_refused_rather_than_halved() {
    let members: Vec<(String, Vec<f32>)> = vec![
        ("a".into(), voice(0, 1)),
        ("b".into(), voice(0, 1)),
        ("c".into(), voice(0, 1)),
    ];
    assert_eq!(
        split_partition(&members),
        Err(SplitRefusal::Indistinguishable)
    );
}

/// Two widths are two vector spaces, and the distance between them would be a
/// number with no meaning. Refused, with both widths in the message.
#[test]
fn mixed_embedding_widths_are_refused() {
    let members: Vec<(String, Vec<f32>)> = vec![
        ("a".into(), voice(0, 1)),
        ("b".into(), vec![1.0, 0.0, 0.0]),
    ];
    let err = split_partition(&members).unwrap_err();
    assert_eq!(
        err,
        SplitRefusal::WidthMismatch {
            expected: support::corrections::FIXTURE_WIDTH,
            found: 3
        }
    );
    assert!(err.message().contains("not the same vector space"));
}

/// One member is not a cluster to split.
#[test]
fn fewer_than_two_members_is_refused() {
    assert_eq!(
        split_partition(&[("only".to_string(), voice(0, 1))]),
        Err(SplitRefusal::NotEnoughEmbeddings { with_embedding: 1 })
    );
}
