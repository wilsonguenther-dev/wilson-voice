//! YV94 — "Markdown export of a meeting with 3+ segments produces a file that
//! round-trips readably (headers, timestamps, body text)."
//!
//! The backlog files that criterion as manual. It is cheaper as a test: the
//! renderer is pure, so "readable" can be stated precisely — an h1 title, a
//! metadata block, a `## Transcript` header, one `[hh:mm:ss]` line per segment
//! carrying the segment's own words, in order. The test also WRITES the file and
//! reads it back, because "the export works" has to mean bytes on disk.
//!
//! Markdown only. PDF was cut for v1 (finding #33).

mod support;

use support::{open_db, seed_meeting, temp_dir};
use wilson_voice_lib::meetings;

#[test]
fn a_three_segment_meeting_round_trips_readably() {
    let dir = temp_dir("md");
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
    assert_eq!(meeting.segment_count, 3);

    // Write it out the way the command does, then read it back — a renderer that
    // is right but a writer that truncates is still a broken export.
    let path = dir.join(format!(
        "{}.md",
        meetings::export_file_stem(&meeting.title, meeting.started_at)
    ));
    std::fs::write(&path, &md).unwrap();
    let round_tripped = std::fs::read_to_string(&path).unwrap();
    assert_eq!(round_tripped, md);

    let lines: Vec<&str> = round_tripped.lines().collect();
    assert_eq!(lines[0], "# Thursday planning", "an h1 title, first line");
    assert!(round_tripped.contains("- **Started:**"));
    assert!(round_tripped.contains("- **Duration:** 12s"));
    assert!(round_tripped.contains("- **Source:** manual · **State:** complete"));
    assert!(round_tripped.contains("## Transcript"));

    // Timestamps: one per segment, zero-padded hh:mm:ss, in order, each carrying
    // that segment's words and this meeting's speaker label.
    //
    // `seed_meeting` starts a meeting the way `Database::create_meeting` does —
    // with the kind picker skipped — so this row is `unknown`, its single mic
    // track is the one that has to be clustered, and its lines are the
    // un-named speaker rather than "Me" (YV125, merged finding #4). The
    // configuration that still reads "Me" is a call with a live second track:
    // `meeting_markdown_export_two_track.rs` covers it, and
    // `meeting_transcript_render_single_track_unchanged.rs` proves the speaker
    // WORD is the only byte of this document that YV125 moved.
    let who = meetings::UNCLUSTERED_SPEAKER_LABEL;
    assert!(round_tripped.contains(&format!(
        "**[00:00:00] {who}:** let us start with the release checklist"
    )));
    assert!(round_tripped.contains(&format!(
        "**[00:00:04] {who}:** the notarised dmg goes out on friday"
    )));
    assert!(round_tripped.contains(&format!(
        "**[00:00:08] {who}:** and we hold the price at nineteen dollars"
    )));
    assert!(
        !round_tripped.contains("] Me:**"),
        "a meeting whose kind was never asked may not claim the microphone was \
         only this user:\n{round_tripped}"
    );

    let stamps: Vec<&str> = round_tripped
        .lines()
        .filter(|l| l.starts_with("**["))
        .collect();
    assert_eq!(stamps.len(), 3, "one line per segment: {stamps:?}");
    let mut sorted = stamps.clone();
    sorted.sort_unstable();
    assert_eq!(stamps, sorted, "transcript lines must be chronological");

    // No stray h1s: segment text can never be promoted into document structure.
    assert_eq!(
        round_tripped
            .lines()
            .filter(|l| l.starts_with("# "))
            .count(),
        1
    );
}

#[test]
fn an_empty_meeting_exports_something_honest() {
    let dir = temp_dir("md-empty");
    let db = open_db(&dir);
    let m = db.create_meeting("Nothing was said", "manual").unwrap();

    let (_, md) = db.meeting_markdown(&m.id).unwrap();
    assert!(md.starts_with("# Nothing was said\n"));
    assert!(md.contains("## Transcript"));
    assert!(
        md.contains("_No transcript segments._"),
        "an empty export must say so rather than end mid-document:\n{md}"
    );
}

#[test]
fn exporting_a_deleted_meeting_is_an_error_not_a_panic() {
    let dir = temp_dir("md-gone");
    let db = open_db(&dir);
    let err = db.meeting_markdown("nope").unwrap_err();
    assert!(err.contains("no meeting"), "{err}");
}
