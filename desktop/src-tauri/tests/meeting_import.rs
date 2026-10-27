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
