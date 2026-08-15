//! YV108 — "a two-track meeting's Markdown export contains both speaker labels
//! in the correct interleaved order; a mic-only meeting's export is unchanged
//! from 22-A's."
//!
//! Extends `tests/meeting_markdown_export.rs` (YV94) rather than replacing it:
//! that file still pins the single-track shape through the same DB path, and
//! this one adds the two-track case beside it. Both go through
//! `Database::meeting_markdown`, not `meetings::render_markdown` directly, so
//! the SQL that orders the rows is under test too — the ordering of a two-track
//! transcript is decided in two places (the `ORDER BY` and the renderer's sort)
//! and a test that skipped the query could only ever check one of them.

mod support;

use support::{open_db, seed_meeting, seed_two_track_meeting, temp_dir};
use wilson_voice_lib::meetings::{self, MIC_TRACK, SYSTEM_TRACK};

/// The turns, in the order they were SAID. `seed_two_track_meeting` writes them
/// in two batches (all the mic's, then all the tap's), which is what the real
/// pipeline does — one transcription pass per recorded wav.
const TURNS: [(f64, i64, &str); 6] = [
    (0.0, MIC_TRACK, "can we ship the notarised build on friday"),
    (4.5, SYSTEM_TRACK, "friday works if the signing cert lands"),
    (9.0, MIC_TRACK, "the cert cleared this morning"),
    (13.5, SYSTEM_TRACK, "then i will tell the reseller friday"),
    (18.0, MIC_TRACK, "and we hold the price where it is"),
    (
        22.5,
        SYSTEM_TRACK,
        "agreed nobody is discounting this quarter",
    ),
];

#[test]
fn a_two_track_export_reads_as_one_labelled_conversation() {
    let dir = temp_dir("md-two-track");
    let db = open_db(&dir);
    let id = seed_two_track_meeting(&db, &dir, "Reseller sync", &TURNS);

    let (meeting, md) = db.meeting_markdown(&id).unwrap();
    assert_eq!(meeting.segment_count, 6);
    assert!(
        meeting.sys_wav_path.is_some(),
        "a two-track meeting has a second wav"
    );

    // Write it and read it back: an export is bytes on disk, not a return value.
    let path = dir.join(format!(
        "{}.md",
        meetings::export_file_stem(&meeting.title, meeting.started_at)
    ));
    std::fs::write(&path, &md).unwrap();
    let round_tripped = std::fs::read_to_string(&path).unwrap();
    assert_eq!(round_tripped, md);

    let transcript: Vec<&str> = round_tripped
        .lines()
        .filter(|l| l.starts_with("**["))
        .collect();
    assert_eq!(
        transcript,
        vec![
            "**[00:00:00] Me:** can we ship the notarised build on friday",
            "**[00:00:04] Them:** friday works if the signing cert lands",
            "**[00:00:09] Me:** the cert cleared this morning",
            "**[00:00:13] Them:** then i will tell the reseller friday",
            "**[00:00:18] Me:** and we hold the price where it is",
            "**[00:00:22] Them:** agreed nobody is discounting this quarter",
        ],
        "the export must be the conversation, in the order it happened"
    );

    // Both labels present, and neither one swallowed the other.
    assert_eq!(
        transcript.iter().filter(|l| l.contains("] Me:**")).count(),
        3
    );
    assert_eq!(
        transcript
            .iter()
            .filter(|l| l.contains("] Them:**"))
            .count(),
        3
    );

    // Chronological, and still exactly one h1 — untrusted ASR text can no more
    // become document structure with two tracks than it could with one.
    let mut sorted = transcript.clone();
    sorted.sort_unstable();
    assert_eq!(transcript, sorted, "transcript lines must be chronological");
    assert_eq!(
        round_tripped
            .lines()
            .filter(|l| l.starts_with("# "))
            .count(),
        1
    );
}

/// The insert order is deliberately NOT the speaking order in the fixture above
/// (the tap's rows are written after every one of the mic's). If the export
/// were ordered by `created_at` or by rowid, it would come out grouped by
/// speaker — three "Me" lines then three "Them" lines — which is exactly the
/// two-columns-to-reconcile failure this item exists to prevent.
#[test]
fn the_order_comes_from_the_clock_not_from_the_insert_order() {
    let dir = temp_dir("md-two-track-order");
    let db = open_db(&dir);
    let id = seed_two_track_meeting(&db, &dir, "Reseller sync", &TURNS);

    let speakers: Vec<i64> = db
        .list_meeting_segments(&id)
        .unwrap()
        .iter()
        .map(|s| s.track)
        .collect();
    assert_eq!(
        speakers,
        vec![
            MIC_TRACK,
            SYSTEM_TRACK,
            MIC_TRACK,
            SYSTEM_TRACK,
            MIC_TRACK,
            SYSTEM_TRACK
        ],
        "rows came back grouped by track — the query is ordering by insert time"
    );
}

/// The other half of the criterion. A mic-only meeting exported through the
/// same DB path: same lines, same order, no "Them" anywhere, no second wav.
///
/// YV125 changed exactly one word of it. `seed_meeting` skips the kind picker,
/// so the row is `unknown`; a meeting nobody said was a call has one recorded
/// track carrying whoever was in the room, and its lines are the un-named
/// speaker rather than "Me" (merged finding #4).
/// `meeting_transcript_render_single_track_unchanged.rs` is what proves that
/// word is the ONLY byte of this document that moved.
#[test]
fn a_mic_only_export_is_unchanged_from_22a() {
    let dir = temp_dir("md-mic-only");
    let db = open_db(&dir);
    let id = seed_meeting(
        &db,
        &dir,
        "Thursday planning",
        &[
            "let us start with the release checklist",
            "the notarised dmg goes out on friday",
            "and we hold the price at nineteen dollars",
        ],
    );

    let (meeting, md) = db.meeting_markdown(&id).unwrap();
    assert!(meeting.sys_wav_path.is_none());
    assert!(!md.contains("Them"), "no phantom second speaker:\n{md}");

    let transcript: Vec<&str> = md.lines().filter(|l| l.starts_with("**[")).collect();
    let who = meetings::UNCLUSTERED_SPEAKER_LABEL;
    assert_eq!(
        transcript,
        vec![
            format!("**[00:00:00] {who}:** let us start with the release checklist"),
            format!("**[00:00:04] {who}:** the notarised dmg goes out on friday"),
            format!("**[00:00:08] {who}:** and we hold the price at nineteen dollars"),
        ]
    );
    assert!(
        !md.contains("] Me:**"),
        "one recorded track and no answered kind is not evidence that the \
         microphone held only this user:\n{md}"
    );
    assert!(md.starts_with("# Thursday planning\n"));
    assert!(md.contains("- **Duration:** 12s"));
    assert!(md.contains("- **Source:** manual · **State:** complete"));
    assert!(md.contains("## Transcript"));
}
