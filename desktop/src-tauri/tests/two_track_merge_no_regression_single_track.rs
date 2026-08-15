//! YV107 acceptance — "feeding this function a mic-only (Track 1 empty) meeting
//! produces byte-identical output to the existing single-track `merge_timed`,
//! so 22-A recordings already in the wild are unaffected."
//!
//! The temptation this file exists to kill: once a merge can re-time a track
//! against its index records, it is one small step to re-time EVERY track that
//! way, "for consistency". For a mic-only meeting that buys nothing — there is
//! no second clock to be wrong against — and costs a silent change to the
//! timestamps of every recording made before this item, which are exactly the
//! recordings nobody is going to re-check.
//!
//! So the assertion is equality on the raw `f64` BITS, not approximate equality:
//! "close enough" is how a re-timing pass sneaks in.

#[path = "support/two_track.rs"]
mod two_track;

use two_track::{chunk, index_records};
use wilson_voice_lib::asr_engine::TimedSpan;
use wilson_voice_lib::meeting_asr::{
    merge_timed, merge_two_tracks_by_host_time, BoundaryKind, ChunkOutcome, TrackEpochs,
};
use wilson_voice_lib::meetings::{MeetingKind, UNCLUSTERED_SPEAKER_LABEL};

/// The kind these fixtures run under: a meeting the user called a CALL whose
/// second track never delivered a word — matrix rows 1, 2 and 12, and the
/// single most common way a "virtual" meeting ends up mic-only.
///
/// It is deliberately NOT `Unknown`, because that would make the label
/// assertions below trivially true for the wrong reason. Under `Virtual` the
/// branch has to notice that Track B produced nothing and fall back on its own
/// (YV125's third table row) — which is what these two tests then pin.
const CALL: MeetingKind = MeetingKind::Virtual;

fn span(start: f64, end: f64, text: &str) -> TimedSpan {
    TimedSpan {
        start_seconds: start,
        end_seconds: end,
        text: text.to_string(),
    }
}

/// A 22-A-shaped mic-only meeting, with the two seam cases `merge_timed`
/// actually branches on present: a fixed-clock cut through the middle of a word
/// (the duplicate it is supposed to pop) and a silence cut with a real
/// repetition across it (the one it must NOT pop).
fn mic_only_chunks() -> Vec<ChunkOutcome> {
    vec![
        chunk(
            0,
            0.0,
            30.0,
            BoundaryKind::Edge,
            vec![
                span(1.0, 1.4, "hello"),
                span(12.25, 12.9, "everyone"),
                span(29.55, 30.0, "into"),
            ],
        ),
        chunk(
            1,
            30.0,
            60.0,
            BoundaryKind::FixedClock,
            vec![
                span(29.97, 30.5, "into"),
                span(41.0, 41.3, "Right."),
                span(42.1, 42.4, "Right,"),
                span(48.0, 48.6, "so"),
            ],
        ),
        chunk(
            2,
            60.0,
            90.0,
            BoundaryKind::Silence,
            vec![span(61.0, 61.7, "anyway"), span(77.5, 78.2, "done")],
        ),
    ]
}

#[test]
fn a_mic_only_meeting_is_byte_for_byte_what_merge_timed_produces() {
    let chunks = mic_only_chunks();
    let want = merge_timed(&chunks);

    // Anchors that WOULD re-time it if the function were to try: 100 ppm, the
    // fixture offset, over the whole 90 seconds. The point is that they are
    // present and ignored.
    let anchors = index_records(100.0, 120);
    let got = merge_two_tracks_by_host_time(&chunks, &[], &anchors, &[], TrackEpochs::SHARED, CALL);

    assert_eq!(got.len(), want.len(), "no span gained or lost");
    for (a, b) in got.iter().zip(want.iter()) {
        assert_eq!(a.text, b.text);
        assert_eq!(
            a.start_seconds.to_bits(),
            b.start_seconds.to_bits(),
            "{:?} moved from {} to {} — a mic-only meeting must not be re-timed",
            b.text,
            b.start_seconds,
            a.start_seconds
        );
        assert_eq!(a.end_seconds.to_bits(), b.end_seconds.to_bits());
        assert_eq!(a.track, 0);
        // NOT "Me" (YV125): the user said this was a call, and the call's
        // audio never arrived on a second track — so the microphone carried
        // whatever was in the room, exactly as it would in person. The TIMES
        // are what this file guards, and they are untouched above.
        assert_eq!(a.speaker, UNCLUSTERED_SPEAKER_LABEL);
    }
}

/// The same, with no anchors at all — a meeting whose journal never wrote a
/// second index record. Still the 22-A output, still not a panic.
#[test]
fn a_mic_only_meeting_with_no_index_records_is_the_same_transcript() {
    let chunks = mic_only_chunks();
    let got = merge_two_tracks_by_host_time(&chunks, &[], &[], &[], TrackEpochs::SHARED, CALL);
    let want = merge_timed(&chunks);
    assert_eq!(got.len(), want.len());
    for (a, b) in got.iter().zip(want.iter()) {
        assert_eq!(a.text, b.text);
        assert_eq!(a.start_seconds.to_bits(), b.start_seconds.to_bits());
    }
}

/// And the fixture is not vacuous: `merge_timed` really does make a decision on
/// it, so "identical to `merge_timed`" is a claim about behaviour rather than
/// about a pass-through of four spans.
#[test]
fn the_fixture_exercises_both_seam_rules() {
    let merged = merge_timed(&mic_only_chunks());
    let texts: Vec<&str> = merged.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(
        texts.iter().filter(|t| **t == "into").count(),
        1,
        "the fixed-clock cut duplicated a word and the seam merge popped it"
    );
    assert_eq!(
        texts.iter().filter(|t| t.starts_with("Right")).count(),
        2,
        "a real repetition across a pause is two words, not one — the seam rule \
         must not have eaten it"
    );
    // hello, everyone, into, Right., Right,, so, anyway, done — nine spans in,
    // one popped at the fixed-clock seam.
    assert_eq!(texts.len(), 8, "{texts:?}");
}

/// A meeting with a SECOND track present is, by contrast, re-timed — otherwise
/// this item does nothing. Stated here so the "no regression" rule cannot be
/// read as "never re-time".
#[test]
fn a_two_track_meeting_is_re_timed_which_is_the_whole_point() {
    let mic = mic_only_chunks();
    let tap = vec![chunk(
        0,
        0.0,
        90.0,
        BoundaryKind::Edge,
        vec![span(
            // 100 ppm fast: spoken at real second 80.0, timestamped at 80.008.
            80.0 * 1.0001,
            80.4 * 1.0001,
            "them",
        )],
    )];
    let merged = merge_two_tracks_by_host_time(
        &mic,
        &tap,
        &index_records(0.0, 120),
        &index_records(100.0, 120),
        TrackEpochs::SHARED,
        CALL,
    );
    let them = merged.iter().find(|s| s.text == "them").unwrap();
    assert!(
        (them.start_seconds - 80.0).abs() < 0.01,
        "the tap's word was spoken at 80.0 s and timestamped at {:.4} on its own \
         clock; the merge must put it back at 80.0, got {:.4}",
        80.0 * 1.0001,
        them.start_seconds
    );
    assert_eq!(them.speaker, "Them");
    assert_eq!(merged.len(), 9, "the mic's eight spans plus the tap's one");
}
