//! yap-notetaker-onboard — an Otter export becomes a real, searchable meeting,
//! and a malformed one leaves NO row behind.
//!
//! Real SQLite file (not in-memory), closed and reopened for the persistence
//! claim, like the rest of the YV94 suite.

mod support;

use chrono::{TimeZone, Utc};
use support::{open_db, temp_dir};
use wilson_voice_lib::meeting_import;

const TXT: &str = include_str!("fixtures/otter/sample.txt");
const SRT: &str = include_str!("fixtures/otter/sample.srt");
const BAD_SRT: &str = include_str!("fixtures/otter/malformed.srt");

#[test]
fn meeting_import_otter_txt_lands_as_a_complete_searchable_meeting() {
    let dir = temp_dir("import-txt");
    let started = Utc.with_ymd_and_hms(2026, 10, 1, 15, 0, 0).unwrap();
    let id = {
        let db = open_db(&dir);
        let parsed = meeting_import::parse("Pricing sync.txt", TXT).unwrap();
        let m = db.import_meeting(&parsed, started).unwrap();
        assert_eq!(m.source, "imported:otter");
        assert_eq!(m.state, "complete");
        assert_eq!(m.title, "Pricing sync");
        assert!(!m.audio_kept);
        assert!(m.mic_wav_path.is_none());
        assert_eq!(m.segment_count, 3);
        assert_eq!(m.started_at, started);
        m.id
    };
    // Reopen: it persisted, and FTS finds it by a word only the import holds.
    let db = open_db(&dir);
    let hits = db.list_meetings(50, Some("discount".into())).unwrap();
    assert_eq!(hits.len(), 1, "FTS must find the imported words");
    assert_eq!(hits[0].id, id);
    let segs = db.list_meeting_segments(&id).unwrap();
    assert_eq!(segs.len(), 3);
    assert!(segs[1].text.starts_with("Jeisil Hernandez: "));
}

#[test]
fn meeting_import_otter_srt_lands_as_segments() {
    let dir = temp_dir("import-srt");
    let db = open_db(&dir);
    let parsed = meeting_import::parse("call.srt", SRT).unwrap();
    let m = db.import_meeting(&parsed, Utc::now()).unwrap();
    assert_eq!(m.segment_count, 3);
    assert_eq!(
        db.list_meeting_segments(&m.id).unwrap()[1].start_seconds,
        4.5
    );
}

#[test]
fn meeting_import_malformed_file_leaves_no_partial_row() {
    let dir = temp_dir("import-bad");
    let db = open_db(&dir);
    let before = db.list_meetings(500, None).unwrap().len();
    let err = meeting_import::parse("broken.srt", BAD_SRT).unwrap_err();
    assert!(err.to_string().contains("Nothing was imported"), "{err}");
    // The first cue of that file IS valid: a parser that wrote as it went
    // would have left a one-segment meeting behind.
    assert_eq!(db.list_meetings(500, None).unwrap().len(), before);
    assert!(db
        .list_meetings(50, Some("fine".into()))
        .unwrap()
        .is_empty());
}

// ---------------------------------------------------------------------------
// PR #271 review, BLOCKING 2: an unbounded clock in an imported file must be
// REFUSED, never a panic. `import_meetings` runs these on the command path; a
// `DateTime + TimeDelta` overflow or a `u64` multiply overflow there takes the
// whole app down instead of reporting one bad file.
// ---------------------------------------------------------------------------

#[test]
fn meeting_import_huge_txt_clock_is_refused_not_a_panic() {
    // The reviewer's probe: parse=Ok, then import_meeting panicked (rc=101).
    let err = meeting_import::parse("huge.txt", "Bob  10000000000:00:00\nhello there\n")
        .expect_err("a ten-billion-hour clock is not a meeting");
    assert!(err.to_string().contains("Nothing was imported"), "{err}");
}

#[test]
fn meeting_import_txt_clock_that_overflows_u64_is_refused_not_a_panic() {
    // 19 digits fits a u64; `* 60` on it does not (a panic in debug builds,
    // a silent wrap in release).
    let r = meeting_import::parse("wrap.txt", "Bob  9999999999999999999:00:00\nhello\n");
    assert!(r.is_err(), "an overflowing clock must be refused: {r:?}");
}

#[test]
fn meeting_import_huge_srt_cue_is_refused_not_a_panic() {
    let srt = "1\n00:00:00,000 --> 99999999999:00:00,000\nhello there\n";
    let err = meeting_import::parse("huge.srt", srt).expect_err("an absurd cue end is refused");
    assert!(err.to_string().contains("Nothing was imported"), "{err}");
}

#[test]
fn meeting_import_db_refuses_an_unrepresentable_duration_instead_of_panicking() {
    // Defence in depth at the write: even a ParsedMeeting built by hand with a
    // duration chrono cannot add to a date returns Err and writes no row.
    let dir = temp_dir("import-huge-duration");
    let db = open_db(&dir);
    let before = db.list_meetings(500, None).unwrap().len();
    for bad in [3.6e13_f64, f64::INFINITY, f64::NAN, -1.0] {
        let mut parsed = meeting_import::parse("ok.srt", SRT).unwrap();
        parsed.duration_seconds = bad;
        let r = db.import_meeting(&parsed, Utc::now());
        assert!(r.is_err(), "duration {bad} must be refused");
    }
    assert_eq!(db.list_meetings(500, None).unwrap().len(), before);
}
