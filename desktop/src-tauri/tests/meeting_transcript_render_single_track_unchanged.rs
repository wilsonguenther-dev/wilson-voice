//! YV108 — "a mic-only meeting's rendered transcript is byte-identical before
//! and after this item."
//!
//! The regression guard on the whole item. YV108 rewrote the transcript body of
//! `render_markdown` to go through `render_transcript`, and it changed the
//! Meetings UI's speaker column; every meeting recorded by 22-A — every meeting
//! anyone actually has today — must come out the other side unchanged.
//!
//! "Byte-identical" is asserted against a fixture CAPTURED FROM THE PRE-YV108
//! RENDERER (`tests/fixtures/mic_only_export_22a.md`, provenance in that
//! directory's README), not against a string re-typed from the new code — a
//! golden written by reading the implementation proves only that the author
//! copied carefully.
//!
//! The one normalisation: `- **Started:**` is rendered in the machine's LOCAL
//! timezone, so the fixture holds `<LOCAL>` there and both sides are masked
//! before the comparison. Everything else, including every space and blank
//! line, is compared verbatim.

use chrono::{DateTime, TimeZone, Utc};
use wilson_voice_lib::meetings::{self, Meeting, MeetingSegment, MIC_TRACK};

const FIXTURE: &str = include_str!("fixtures/mic_only_export_22a.md");

fn started() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 11, 16, 3, 0).unwrap()
}

/// The exact meeting the fixture was captured from.
fn meeting_22a() -> Meeting {
    Meeting {
        id: "m-golden".into(),
        title: "Thursday planning".into(),
        source: "manual".into(),
        started_at: started(),
        ended_at: None,
        duration_seconds: 750.0,
        state: "complete".into(),
        error: None,
        processed_through_seconds: 750.0,
        audio_kept: true,
        mic_wav_path: None,
        sys_wav_path: None,
        tap_rebuilds: None,
        summary: Some("We agreed to ship on Friday.".into()),
        summary_model: Some("stub-1.5b".into()),
        created_at: started(),
        segment_count: 3,
        diagnostics: None,
    }
}

const TEXTS: [&str; 3] = [
    "let us start with the release checklist",
    "the notarised dmg goes out on friday",
    "and we hold the price at nineteen dollars",
];

fn segments_22a() -> Vec<MeetingSegment> {
    TEXTS
        .iter()
        .enumerate()
        .map(|(i, t)| MeetingSegment {
            id: format!("segment-{i}"),
            meeting_id: "m-golden".into(),
            start_seconds: i as f64 * 4.0,
            end_seconds: i as f64 * 4.0 + 3.5,
            text: (*t).into(),
            confidence: None,
            created_at: started(),
            track: MIC_TRACK,
        })
        .collect()
}

/// Mask the one line that depends on where the machine is standing.
fn mask_started(md: &str) -> String {
    md.lines()
        .map(|l| {
            if l.starts_with("- **Started:**") {
                "- **Started:** <LOCAL>".to_string()
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_mic_only_export_is_byte_identical_to_the_pre_yv108_renderer() {
    let md = meetings::render_markdown(&meeting_22a(), &segments_22a());
    assert_eq!(
        mask_started(&md),
        mask_started(FIXTURE),
        "YV108 changed a mic-only export. The fixture is what 22-A shipped:\n{md}"
    );
}

/// The masking must not be able to hide the assertion: if `mask_started`
/// swallowed the whole document, the test above would pass against anything.
#[test]
fn the_comparison_is_not_vacuous() {
    let mut m = meeting_22a();
    m.title = "Thursday planning (edited)".into();
    let drifted = meetings::render_markdown(&m, &segments_22a());
    assert_ne!(mask_started(&drifted), mask_started(FIXTURE));

    let one_word_off = meetings::render_markdown(&meeting_22a(), &{
        let mut segs = segments_22a();
        segs[1].text = "the notarized dmg goes out on friday".into();
        segs
    });
    assert_ne!(mask_started(&one_word_off), mask_started(FIXTURE));
    assert!(mask_started(FIXTURE).contains("**[00:00:04] Me:**"));
}

/// The transcript half specifically: same lines, same order, every one of them
/// still "Me" — no phantom "Them" appears because the render path is now
/// track-aware.
#[test]
fn every_line_of_a_mic_only_transcript_is_still_me_in_input_order() {
    let segments = segments_22a();
    let lines = meetings::render_transcript(&segments);

    assert_eq!(lines.len(), TEXTS.len());
    assert!(!meetings::is_two_track(&segments));
    for (line, expected) in lines.iter().zip(TEXTS.iter()) {
        assert_eq!(line.speaker, meetings::MIC_SPEAKER_LABEL);
        assert_eq!(line.track, MIC_TRACK);
        assert_eq!(&line.text, expected);
    }
    assert_eq!(
        lines.iter().map(|l| l.offset.as_str()).collect::<Vec<_>>(),
        vec!["00:00:00", "00:00:04", "00:00:08"]
    );
    assert!(
        !meetings::render_markdown(&meeting_22a(), &segments).contains("Them"),
        "a mic-only meeting must never mention a second speaker"
    );
}

/// An empty mic-only meeting still says so rather than ending mid-document —
/// the other 22-A shape, and the one a `render_transcript` that returned an
/// empty vec could quietly have broken.
#[test]
fn an_empty_mic_only_meeting_still_exports_the_honest_line() {
    let md = meetings::render_markdown(&meeting_22a(), &[]);
    assert!(
        md.contains("## Transcript\n\n_No transcript segments._\n"),
        "{md}"
    );
}
