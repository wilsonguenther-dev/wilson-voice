//! YV108 — "a synthetic merged span sequence with mixed track 0/1 spans renders
//! as one interleaved list with correct Me/Them labels in host-time order."
//!
//! The input here is what YV107's merge hands over: segments already carrying a
//! `track` and already rebased onto one common host-time origin. Everything
//! under test is `meetings::render_transcript`, which the Markdown export calls
//! directly and which `desktop/src/meetings/transcript.ts` mirrors for the
//! Meetings UI.
//!
//! "The screen and the exported file disagree about who said what" is therefore
//! prevented by FIXTURES, not by construction — the UI runs the mirror, not this
//! function. Every case in this file has a twin in `transcript.test.ts` asserting
//! the same expected output; the review of this item found the one rule that had
//! no twin (`empty_spans_never_become_blank_turns`, below) and the mirror had
//! duly drifted. A case added here without its twin is the next such gap.
//!
//! No audio, no database, no hardware: rendering is pure, and the ordering rule
//! is the part with a way to be wrong.

use chrono::{TimeZone, Utc};
use wilson_voice_lib::meetings::{
    self, DiarizationTarget, MeetingKind, MeetingSegment, MIC_SPEAKER_LABEL, MIC_TRACK,
    SYSTEM_SPEAKER_LABEL, SYSTEM_TRACK, UNCLUSTERED_SPEAKER_LABEL,
};

/// Every fixture in this file is a CALL whose second track really was recorded
/// — the one configuration in which the microphone holds exactly one speaker,
/// and therefore the only one whose mic lines read "Me" (YV125). The rules
/// under test here (ordering, tie-break, whitespace, dropped blanks) are the
/// same under either kind; `meeting_kind_branch.rs` is where the branch itself
/// is the subject.
const CALL: MeetingKind = MeetingKind::Virtual;

fn seg(start: f64, track: i64, text: &str) -> MeetingSegment {
    MeetingSegment {
        id: format!("seg-t{track}-{start}"),
        meeting_id: "m-two-track".into(),
        start_seconds: start,
        end_seconds: start + 3.0,
        text: text.into(),
        confidence: None,
        created_at: Utc.with_ymd_and_hms(2026, 8, 14, 17, 0, 0).unwrap(),
        track,
        cluster_index: None,
    }
}

/// A four-turn conversation: the user asks, the room answers, twice.
///
/// Deliberately supplied to the renderer **grouped by track** — the shape two
/// separate transcription passes produce — so a renderer that simply echoed its
/// input would fail this rather than pass it.
fn conversation() -> Vec<MeetingSegment> {
    vec![
        seg(0.0, MIC_TRACK, "can we ship the notarised build on friday"),
        seg(12.0, MIC_TRACK, "then we hold the price where it is"),
        seg(4.5, SYSTEM_TRACK, "friday works if the signing cert lands"),
        seg(
            17.25,
            SYSTEM_TRACK,
            "agreed nobody is discounting this quarter",
        ),
    ]
}

#[test]
fn mixed_tracks_render_as_one_interleaved_me_them_list() {
    let lines = meetings::render_transcript(&conversation(), CALL);

    let rendered: Vec<(String, &str, String)> = lines
        .iter()
        .map(|l| (l.offset.clone(), l.speaker, l.text.clone()))
        .collect();
    assert_eq!(
        rendered,
        vec![
            (
                "00:00:00".to_string(),
                "Me",
                "can we ship the notarised build on friday".to_string()
            ),
            (
                "00:00:04".to_string(),
                "Them",
                "friday works if the signing cert lands".to_string()
            ),
            (
                "00:00:12".to_string(),
                "Me",
                "then we hold the price where it is".to_string()
            ),
            (
                "00:00:17".to_string(),
                "Them",
                "agreed nobody is discounting this quarter".to_string()
            ),
        ],
        "expected one back-and-forth transcript in host-time order"
    );
}

/// The failure this item exists to prevent: two blocks of text the reader has
/// to interleave by eye. If the output were grouped by speaker, the track
/// sequence would be sorted; a real conversation's is not.
#[test]
fn the_output_is_a_conversation_not_two_stacked_columns() {
    let tracks: Vec<i64> = meetings::render_transcript(&conversation(), CALL)
        .iter()
        .map(|l| l.track)
        .collect();
    assert_eq!(
        tracks,
        vec![MIC_TRACK, SYSTEM_TRACK, MIC_TRACK, SYSTEM_TRACK]
    );
    assert!(
        tracks.windows(2).any(|w| w[0] != w[1]),
        "speakers must alternate, not stack: {tracks:?}"
    );
    assert!(
        !tracks.is_sorted(),
        "a sorted track sequence means the render grouped by speaker \
         instead of interleaving by time: {tracks:?}"
    );
}

/// Host time is the ordering key, and it is compared ACROSS tracks. A merge
/// whose seam boundaries do not line up (YV107's own case) still comes out in
/// one strictly non-decreasing timeline.
#[test]
fn ordering_is_by_host_time_across_both_tracks() {
    let mut segments = conversation();
    // Two more turns whose offsets fall between the existing ones, appended at
    // the end of the input so input order and time order cannot coincide.
    segments.push(seg(2.0, SYSTEM_TRACK, "one moment let me share"));
    segments.push(seg(15.5, MIC_TRACK, "that is what i wanted to hear"));

    let lines = meetings::render_transcript(&segments, CALL);
    let times: Vec<f64> = lines.iter().map(|l| l.start_seconds).collect();
    assert_eq!(times, vec![0.0, 2.0, 4.5, 12.0, 15.5, 17.25]);
    assert!(
        times.windows(2).all(|w| w[0] <= w[1]),
        "transcript must be non-decreasing in host time: {times:?}"
    );
}

/// A question and the answer that overlaps it can share an offset. Which one
/// reads first must be a rule, not whatever order the rows arrived in — the mic
/// goes first, both times.
#[test]
fn an_exact_tie_breaks_to_the_mic_whichever_order_it_arrived_in() {
    let forwards = meetings::render_transcript(
        &[
            seg(9.0, MIC_TRACK, "so we are agreed"),
            seg(9.0, SYSTEM_TRACK, "so we are agreed"),
        ],
        CALL,
    );
    let backwards = meetings::render_transcript(
        &[
            seg(9.0, SYSTEM_TRACK, "so we are agreed"),
            seg(9.0, MIC_TRACK, "so we are agreed"),
        ],
        CALL,
    );
    let speakers = |v: &[meetings::TranscriptLine]| -> Vec<&'static str> {
        v.iter().map(|l| l.speaker).collect()
    };
    assert_eq!(
        speakers(&forwards),
        vec![MIC_SPEAKER_LABEL, SYSTEM_SPEAKER_LABEL]
    );
    assert_eq!(speakers(&backwards), speakers(&forwards));
}

/// The label is derived from the track AND the meeting's diarization target, in
/// one place, for both surfaces (YV125).
#[test]
fn labels_come_from_the_track_and_an_unknown_track_is_not_you() {
    // The only configuration in which the microphone may be called "Me".
    assert_eq!(
        meetings::speaker_label(MIC_TRACK, DiarizationTarget::MicIsMe),
        "Me"
    );
    assert_eq!(
        meetings::speaker_label(SYSTEM_TRACK, DiarizationTarget::MicIsMe),
        "Them"
    );
    // Where the mic is the track being clustered, it is NOT this user — it is
    // whoever was in the room, and nothing has told them apart yet.
    assert_eq!(
        meetings::speaker_label(MIC_TRACK, DiarizationTarget::ClusterTrackA),
        UNCLUSTERED_SPEAKER_LABEL
    );
    assert_ne!(UNCLUSTERED_SPEAKER_LABEL, MIC_SPEAKER_LABEL);
    // `track` is an unconstrained INTEGER column. A value no writer in this
    // codebase produces must still render, it certainly is not the mic, and the
    // target does not change that: it did not come from this microphone under
    // either branch.
    for target in [DiarizationTarget::MicIsMe, DiarizationTarget::ClusterTrackA] {
        assert_eq!(meetings::speaker_label(7, target), SYSTEM_SPEAKER_LABEL);
    }
}

/// `is_two_track` is what the UI asks before it changes anything about the
/// layout — so it has to be true of a real two-track meeting and false of every
/// 22-A one, including an empty transcript.
#[test]
fn two_track_detection_is_about_the_segments_not_the_meeting_row() {
    assert!(meetings::is_two_track(&conversation()));
    assert!(!meetings::is_two_track(&[seg(0.0, MIC_TRACK, "just me")]));
    assert!(!meetings::is_two_track(&[]));
}

/// A tap track that renders NOTHING is not a second speaker.
///
/// Silence on the far side is routine — the far end is muted, or nobody has
/// spoken yet — and the ASR still emits spans for it, with no words in them.
/// `render_transcript` drops those, so a UI that widened its speaker gutter on
/// the raw rows would reserve room for a "Them" who is on neither the screen
/// nor in the exported file. `transcript.test.ts`'s "does not widen the gutter
/// for a tap track that renders nothing" is the twin of this case.
#[test]
fn a_tap_track_that_renders_nothing_is_not_a_second_speaker() {
    let blank_tap = [
        seg(0.0, MIC_TRACK, "real words"),
        seg(1.0, SYSTEM_TRACK, "   "),
    ];
    assert!(!meetings::is_two_track(&blank_tap));
    let lines = meetings::render_transcript(&blank_tap, CALL);
    assert_eq!(lines.len(), 1);
    assert!(lines.iter().all(|l| l.track == MIC_TRACK));

    // One real word from the far side and it is a two-track meeting again.
    let spoken = [
        seg(0.0, MIC_TRACK, "real words"),
        seg(1.0, SYSTEM_TRACK, "   "),
        seg(2.0, SYSTEM_TRACK, "hello"),
    ];
    assert!(meetings::is_two_track(&spoken));
}

/// Whitespace is not one set. Rust's `char::is_whitespace` is the Unicode
/// `White_Space` property — which includes U+0085 (NEL) — while JavaScript's
/// `\s`, the obvious thing to reach for in the mirror, excludes U+0085 and
/// *includes* U+FEFF (ZWNBSP, which Rust treats as an ordinary character).
///
/// So this pins both characters here, and `transcript.test.ts`'s "agrees with
/// Rust on the two characters JS and Rust disagree about" pins the identical
/// expectations there — the mirror spells the character class out rather than
/// using `\s` precisely because of this test.
#[test]
fn a_span_of_exotic_whitespace_is_collapsed_the_same_way_rust_collapses_it() {
    // NEL alone is whitespace: the span renders nothing and is dropped.
    assert!(meetings::render_transcript(&[seg(0.0, SYSTEM_TRACK, "\u{0085}")], CALL).is_empty());
    assert_eq!(
        meetings::render_transcript(&[seg(0.0, MIC_TRACK, "a\u{0085}b")], CALL)[0].text,
        "a b"
    );
    // ZWNBSP is NOT whitespace: the span survives, unchanged.
    let bom = meetings::render_transcript(&[seg(0.0, SYSTEM_TRACK, "\u{feff}")], CALL);
    assert_eq!(bom.len(), 1);
    assert_eq!(bom[0].text, "\u{feff}");
    // And the ordinary case both languages already agree on.
    assert_eq!(
        meetings::render_transcript(&[seg(0.0, MIC_TRACK, "  so   we\nare\tagreed  ")], CALL)[0]
            .text,
        "so we are agreed"
    );
}

/// An empty span is dropped rather than rendered as a timestamped blank line —
/// the behaviour 22-A's renderer already had, preserved now that both surfaces
/// share this function.
#[test]
fn empty_spans_never_become_blank_turns() {
    let lines = meetings::render_transcript(
        &[
            seg(0.0, MIC_TRACK, "real words"),
            seg(1.0, SYSTEM_TRACK, "   "),
            seg(2.0, SYSTEM_TRACK, "also real"),
        ],
        CALL,
    );
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].text, "real words");
    assert_eq!(lines[1].text, "also real");
}

/// ASR output is untrusted, including its numbers: a decoder that emits a NaN
/// or a negative offset must not reorder the transcript around it or panic the
/// sort's comparator.
#[test]
fn a_broken_offset_sorts_as_zero_instead_of_poisoning_the_order() {
    let lines = meetings::render_transcript(
        &[
            seg(5.0, MIC_TRACK, "second"),
            seg(f64::NAN, SYSTEM_TRACK, "first, clamped"),
        ],
        CALL,
    );
    assert_eq!(lines[0].text, "first, clamped");
    assert_eq!(lines[0].offset, "00:00:00");
    assert_eq!(lines[1].text, "second");
    // The raw number is carried through rather than healed into a 0 — the clamp
    // belongs to the comparator and to `format_offset`, and the mirror keeps
    // the same distinction (`transcript.test.ts` asserts `Number.isNaN` on it).
    assert!(lines[0].start_seconds.is_nan());
}
