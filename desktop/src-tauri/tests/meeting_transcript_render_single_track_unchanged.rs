//! YV108 — "a mic-only meeting's rendered transcript is byte-identical before
//! and after this item." **YV125 deliberately changes ONE word of that, and
//! this file is now the guard on how little else moved.**
//!
//! YV108 rewrote the transcript body of `render_markdown` to go through
//! `render_transcript` and asserted every byte of a mic-only export against a
//! fixture CAPTURED FROM THE PRE-YV108 RENDERER
//! (`tests/fixtures/mic_only_export_22a.md`, provenance in that directory's
//! README) — a golden written by reading the implementation proves only that
//! the author copied carefully, so the fixture stays exactly as it was.
//!
//! ## What YV125 changes, and why the fixture was NOT re-captured
//!
//! Merged finding #4: labelling the microphone "Me" is a claim about identity
//! derived from a channel number. It holds for a call whose other participants
//! are on a separate track, and it is false for an in-person meeting, a class,
//! a hybrid room, or a call whose tap never attached — on all of which track 0
//! carries the whole room and "Me" swallows every other voice in it. Migration
//! 4 stores the meeting's kind, `meetings::diarization_target` branches on it,
//! and a mic-only meeting — which is EVERY meeting recorded before this build,
//! whose kind reads `unknown` — is the branch that has to be clustered. Until
//! YV126 clusters it, its lines read `Speaker:` rather than `Me:`.
//!
//! That is a real, user-visible change to the export of meetings already on
//! disk, and it is the point of the item rather than a side effect of it. So
//! this file does not re-capture the golden (which would silently accept
//! anything else that moved with it). It asserts the blast radius INSTEAD:
//! replace the new speaker word with the old one and the document is the
//! pre-YV108 fixture, byte for byte. One word per transcript line, and nothing
//! else — not a space, not a blank line, not a heading.
//!
//! The one normalisation, unchanged: `- **Started:**` is rendered in the
//! machine's LOCAL timezone, so the fixture holds `<LOCAL>` there and both
//! sides are masked before the comparison.

use chrono::{DateTime, TimeZone, Utc};
use wilson_voice_lib::meetings::{
    self, Meeting, MeetingKind, MeetingSegment, DEFAULT_MEETING_KIND, MIC_SPEAKER_LABEL, MIC_TRACK,
    UNCLUSTERED_SPEAKER_LABEL,
};

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
        // The value migration 4 gives every row that existed before it: the
        // question was never asked, so it was never answered.
        kind: DEFAULT_MEETING_KIND.into(),
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

/// Turn the new speaker word back into the old one — and ONLY that.
///
/// Anchored on the `] ` and `:**` the export puts around a speaker so it cannot
/// touch a segment that happens to contain the word "Speaker".
fn unmask_speaker(md: &str) -> String {
    md.replace(
        &format!("] {UNCLUSTERED_SPEAKER_LABEL}:**"),
        &format!("] {MIC_SPEAKER_LABEL}:**"),
    )
}

/// The blast radius of YV125 on an export that already exists: one word per
/// transcript line, and not one byte more.
#[test]
fn a_mic_only_export_differs_from_the_pre_yv108_renderer_only_in_the_speaker_word() {
    let md = meetings::render_markdown(&meeting_22a(), &segments_22a());
    assert!(
        md.contains(&format!("**[00:00:00] {UNCLUSTERED_SPEAKER_LABEL}:**")),
        "a meeting whose kind is `unknown` is clustered, so its mic lines are \
         not 'Me' (YV125):\n{md}"
    );
    assert!(
        !md.contains(&format!("] {MIC_SPEAKER_LABEL}:**")),
        "…and none of them may still claim to be:\n{md}"
    );
    assert_eq!(
        mask_started(&unmask_speaker(&md)),
        mask_started(FIXTURE),
        "YV125 may change the SPEAKER WORD of a mic-only export and nothing \
         else. The fixture is what 22-A shipped:\n{md}"
    );
}

/// The one configuration whose export YV125 does not touch at all: a call with
/// a live second track, where the microphone really does hold one speaker.
///
/// This is what stops the fix from being "delete the Me label" — it is a branch,
/// and this is the branch that still says "Me".
#[test]
fn a_call_with_a_second_track_still_exports_me_byte_for_byte() {
    let mut m = meeting_22a();
    m.kind = MeetingKind::Virtual.as_str().into();
    let mut segments = segments_22a();
    // One real word from the far side is what makes it a two-track meeting.
    segments.push(MeetingSegment {
        id: "segment-them".into(),
        track: wilson_voice_lib::meetings::SYSTEM_TRACK,
        start_seconds: 12.0,
        end_seconds: 15.0,
        text: "sounds right to me".into(),
        ..segments_22a()[0].clone()
    });
    let md = meetings::render_markdown(&m, &segments);
    for (i, expected) in TEXTS.iter().enumerate() {
        assert!(
            md.contains(&format!(
                "**[00:00:{:02}] {MIC_SPEAKER_LABEL}:** {expected}",
                i * 4
            )),
            "{md}"
        );
    }
    assert!(
        md.contains("**[00:00:12] Them:** sounds right to me"),
        "{md}"
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
    assert_ne!(
        mask_started(&unmask_speaker(&one_word_off)),
        mask_started(FIXTURE)
    );
    assert!(mask_started(FIXTURE).contains("**[00:00:04] Me:**"));
    // And `unmask_speaker` is not a second mask that could swallow a real
    // difference either: it rewrites the speaker word and leaves the rest,
    // including a segment whose TEXT is the word it rewrites.
    let says_speaker = meetings::render_markdown(&meeting_22a(), &{
        let mut segs = segments_22a();
        segs[1].text = "Speaker: the notarised dmg goes out on friday".into();
        segs
    });
    assert!(
        unmask_speaker(&says_speaker).contains("Me:** Speaker: the notarised"),
        "the un-mask must anchor on the export's own punctuation, not on the \
         bare word:\n{}",
        unmask_speaker(&says_speaker)
    );
    assert_ne!(
        mask_started(&unmask_speaker(&says_speaker)),
        mask_started(FIXTURE)
    );
}

/// The transcript half specifically: same lines, same order, one speaker — and
/// no phantom "Them" appears, because the render path is track-aware and the
/// kind branch does not invent a second track it never had.
#[test]
fn every_line_of_a_mic_only_transcript_is_one_unnamed_speaker_in_input_order() {
    let segments = segments_22a();
    let lines = meetings::render_transcript(&segments, meeting_22a().kind());

    assert_eq!(lines.len(), TEXTS.len());
    assert!(!meetings::is_two_track(&segments));
    for (line, expected) in lines.iter().zip(TEXTS.iter()) {
        assert_eq!(line.speaker, UNCLUSTERED_SPEAKER_LABEL);
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
