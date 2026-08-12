//! YV94 — "FTS search finds a phrase from a segment" (the plan's own line).
//!
//! `meeting_segments_fts` is an EXTERNAL-CONTENT FTS5 index: the tokens live in
//! shadow tables and are kept in step by the ai/ad/au trigger trio copied from
//! `transcripts_fts`. Nothing but those triggers may write it, so the tests
//! below drive the index only through ordinary row writes.

mod support;

use support::{open_db, seed_meeting, temp_dir};

#[test]
fn fts_search_finds_a_phrase_from_a_segment() {
    let dir = temp_dir("fts");
    let db = open_db(&dir);

    let wanted = seed_meeting(
        &db,
        &dir,
        "Monday standup",
        &[
            "we should ship the notetaker before the semester starts",
            "the aliasing fix landed yesterday",
        ],
    );
    let other = seed_meeting(
        &db,
        &dir,
        "Lecture: thermodynamics",
        &["entropy always increases in a closed system"],
    );

    let hits = db
        .list_meetings(50, Some("notetaker".into()))
        .expect("search");
    assert_eq!(hits.len(), 1, "one meeting mentions the notetaker");
    assert_eq!(hits[0].id, wanted);

    // Multi-word: every token must match somewhere in the same meeting.
    let hits = db.list_meetings(50, Some("aliasing fix".into())).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, wanted);

    // Prefix search — the same `"tok"*` shape the transcript search uses, so a
    // half-typed query is useful before the user stops typing.
    let hits = db.list_meetings(50, Some("entrop".into())).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, other);

    // Title search is OR-ed in: a meeting is findable by what it was called even
    // when the word was never spoken.
    let hits = db.list_meetings(50, Some("thermodynamics".into())).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, other);

    assert!(db
        .list_meetings(50, Some("kubernetes".into()))
        .unwrap()
        .is_empty());
}

/// An FTS5 MATCH can reject a query outright (a lone `*`, an unbalanced quote).
/// The list must degrade to a substring search, never to an error the user sees.
#[test]
fn a_malformed_query_degrades_instead_of_failing() {
    let dir = temp_dir("fts-bad");
    let db = open_db(&dir);
    seed_meeting(&db, &dir, "Notes", &["quarterly planning session"]);

    for q in ["\"", "*", "^", "NEAR(", "AND"] {
        let res = db.list_meetings(50, Some(q.to_string()));
        assert!(
            res.is_ok(),
            "query {q:?} must not surface an error: {res:?}"
        );
    }
    // And a query that IS valid but only matches as a substring still finds it.
    let hits = db.list_meetings(50, Some("quarterly".into())).unwrap();
    assert_eq!(hits.len(), 1);
}

/// Segments are returned in wall-clock order, and the search index tracks edits.
#[test]
fn segments_are_chronological_and_the_index_tracks_deletes() {
    let dir = temp_dir("fts-order");
    let db = open_db(&dir);
    let id = seed_meeting(&db, &dir, "Ordering", &["first", "second", "third"]);

    let segs = db.list_meeting_segments(&id).unwrap();
    let starts: Vec<f64> = segs.iter().map(|s| s.start_seconds).collect();
    assert_eq!(starts, vec![0.0, 4.0, 8.0]);
    assert!(
        starts.windows(2).all(|w| w[0] <= w[1]),
        "a transcript may only be read in the order it was spoken"
    );

    assert_eq!(support::fts_docsize_rows(&dir), 3);
    db.delete_meeting_with_audio(&id).unwrap();
    assert_eq!(
        support::fts_docsize_rows(&dir),
        0,
        "the ad trigger must take the tokens with the rows"
    );
    assert!(db
        .list_meetings(50, Some("second".into()))
        .unwrap()
        .is_empty());
}
