//! YV125 — merged finding #4, as a table.
//!
//! "Diarization architecture is inverted relative to Wilson's stated IRL
//! priority. Plan hardcodes Track A (mic/room) as a single un-clustered 'me'
//! and diarizes only Track B (system audio) — exactly backwards for
//! in-person/cannot-join meetings, where Track B doesn't exist and Track A
//! carries every speaker in the room." Flagged Critical by all three lenses.
//!
//! The fix is one pure function, `meetings::diarization_target`, and this file
//! is its truth table. Pure: no audio, no database, no sidecar, no model — the
//! branch is a decision about which mechanism runs, and it is decidable from
//! two facts (what the user said this meeting is, and whether a second track
//! actually delivered anything). YV126 is what consumes the answer; this is
//! what makes the answer checkable before it exists.
//!
//! The second half of the file follows the same branch through
//! `speaker_label`/`render_transcript`, because a target nothing reads is a
//! constant. The label is the one reader that exists on this base.

use chrono::{TimeZone, Utc};
use wilson_voice_lib::meetings::{
    self, diarization_target, DiarizationTarget, MeetingKind, MeetingSegment, MIC_SPEAKER_LABEL,
    MIC_TRACK, SYSTEM_SPEAKER_LABEL, SYSTEM_TRACK, UNCLUSTERED_SPEAKER_LABEL,
};

/// The spec's table, transcribed: `(kind, a live second track?, target, why)`.
const TABLE: &[(MeetingKind, bool, DiarizationTarget, &str)] = &[
    (
        MeetingKind::InPerson,
        false,
        DiarizationTarget::ClusterTrackA,
        "everyone is in the room and the microphone carries all of them",
    ),
    (
        MeetingKind::InPerson,
        true,
        DiarizationTarget::ClusterTrackA,
        "a room that also has audio coming out of the speakers is a HYBRID \
         meeting — the room is still on Track A and still has to be split",
    ),
    (
        MeetingKind::Unknown,
        false,
        DiarizationTarget::ClusterTrackA,
        "the picker was skipped; the safer, more general branch is the one \
         that does not assume a call",
    ),
    (
        MeetingKind::Unknown,
        true,
        DiarizationTarget::ClusterTrackA,
        "a tap proves audio came out of the speakers, never that the room \
         behind the microphone is empty",
    ),
    (
        MeetingKind::Virtual,
        true,
        DiarizationTarget::MicIsMe,
        "the other participants are on Track B, cleanly isolated, so the \
         microphone really does hold one voice — free and correct exactly here",
    ),
    (
        MeetingKind::Virtual,
        false,
        DiarizationTarget::ClusterTrackA,
        "the tap was denied, revoked or unavailable (matrix rows 1, 2, 12), so \
         the call is arriving through the room's speakers into the microphone \
         — acoustically the in-person case",
    ),
];

#[test]
fn the_diarization_target_follows_the_kind_and_the_second_track() {
    for (kind, has_system_track, want, why) in TABLE {
        assert_eq!(
            diarization_target(*kind, *has_system_track),
            *want,
            "kind={} second_track={has_system_track}: {why}",
            kind.as_str()
        );
    }
}

/// Exactly ONE row of that table is `MicIsMe`. Stated as its own assertion so
/// the shortcut cannot be widened by adding a row without anybody noticing.
#[test]
fn only_a_call_with_a_live_second_track_may_call_the_mic_me() {
    let me: Vec<(MeetingKind, bool)> = TABLE
        .iter()
        .filter(|(_, _, target, _)| *target == DiarizationTarget::MicIsMe)
        .map(|(k, t, _, _)| (*k, *t))
        .collect();
    assert_eq!(me, vec![(MeetingKind::Virtual, true)]);

    // And the table itself covers every kind, under both answers, so "only one
    // row" is a statement about the whole space rather than about six rows
    // somebody chose.
    for kind in [
        MeetingKind::Virtual,
        MeetingKind::InPerson,
        MeetingKind::Unknown,
    ] {
        for has_track in [true, false] {
            assert!(
                TABLE
                    .iter()
                    .any(|(k, t, _, _)| *k == kind && *t == has_track),
                "the table does not cover kind={} second_track={has_track}",
                kind.as_str()
            );
        }
    }
}

fn seg(start: f64, track: i64, text: &str) -> MeetingSegment {
    MeetingSegment {
        id: format!("seg-t{track}-{start}"),
        meeting_id: "m-kind".into(),
        start_seconds: start,
        end_seconds: start + 3.0,
        text: text.into(),
        confidence: None,
        created_at: Utc.with_ymd_and_hms(2026, 8, 15, 17, 0, 0).unwrap(),
        track,
    }
}

/// The branch reaches the labels, which is the only reader it has on this base.
///
/// `render_transcript` does not take "was there a tap" as an argument — it
/// reads it off the segments that actually render, the same question
/// `is_two_track` answers for the layout. So a `virtual` meeting whose Track B
/// rows are all blank is a mic-only meeting to the labeller, exactly as it is
/// to the reader.
#[test]
fn the_label_follows_the_same_branch_the_target_does() {
    let room = [
        seg(0.0, MIC_TRACK, "let us start with the release checklist"),
        seg(4.0, MIC_TRACK, "friday works if the signing cert lands"),
    ];
    for kind in [MeetingKind::InPerson, MeetingKind::Unknown] {
        let lines = meetings::render_transcript(&room, kind);
        assert_eq!(lines.len(), 2);
        assert!(
            lines.iter().all(|l| l.speaker == UNCLUSTERED_SPEAKER_LABEL),
            "kind={}: an un-clustered room track is not this user",
            kind.as_str()
        );
    }

    // A call whose second track delivered words: the one "Me" configuration.
    let call = [
        seg(0.0, MIC_TRACK, "can we ship on friday"),
        seg(4.0, SYSTEM_TRACK, "friday works for us"),
    ];
    let lines = meetings::render_transcript(&call, MeetingKind::Virtual);
    assert_eq!(lines[0].speaker, MIC_SPEAKER_LABEL);
    assert_eq!(lines[1].speaker, SYSTEM_SPEAKER_LABEL);

    // The SAME call, with the far side silent — a tap that was configured and
    // never delivered a word. Matrix rows 1/2/12, and the mic is back to
    // carrying the room.
    let call_no_tap = [
        seg(0.0, MIC_TRACK, "can we ship on friday"),
        seg(4.0, SYSTEM_TRACK, "   "),
    ];
    let lines = meetings::render_transcript(&call_no_tap, MeetingKind::Virtual);
    assert_eq!(lines.len(), 1, "the blank far-side span renders nothing");
    assert_eq!(lines[0].speaker, UNCLUSTERED_SPEAKER_LABEL);

    // The hybrid case, spelled out because it is the expensive one: the tap
    // delivered, and the room still gets clustered.
    let hybrid = meetings::render_transcript(&call, MeetingKind::InPerson);
    assert_eq!(hybrid[0].speaker, UNCLUSTERED_SPEAKER_LABEL);
    assert_eq!(
        hybrid[1].speaker, SYSTEM_SPEAKER_LABEL,
        "the far side of a hybrid meeting is still not this microphone"
    );
}

/// The system track's label does NOT move with the branch, and that is
/// deliberate: "this did not come out of your microphone" is mechanical, not
/// inferred, so no kind can turn a tap segment into the person holding the Mac.
#[test]
fn the_system_track_is_never_this_user_under_either_branch() {
    for target in [DiarizationTarget::MicIsMe, DiarizationTarget::ClusterTrackA] {
        assert_eq!(
            meetings::speaker_label(SYSTEM_TRACK, target),
            SYSTEM_SPEAKER_LABEL
        );
        assert_ne!(
            meetings::speaker_label(SYSTEM_TRACK, target),
            MIC_SPEAKER_LABEL
        );
    }
}

/// The mechanical half of "the 'me' shortcut moved off the track index".
///
/// The acceptance criterion is a grep: `is_me` must read as a property of an
/// enrolled `speaker_profiles` row (YV128) and must NOT be a track-index
/// conditional inside `speaker_label`. A grep is a human check, so this is the
/// same claim asserted against the source text, which is the only thing that
/// can hold it once nobody is reading the diff any more.
///
/// Source-text tests are a blunt instrument and this one is deliberately narrow:
/// it reads ONE function's body and asks two questions about it. It cannot tell
/// you the branch is right — the table above does that — only that the identity
/// question is not being answered from a channel number again.
#[test]
fn speaker_label_decides_on_the_target_and_never_on_an_identity_flag() {
    let src = include_str!("../src/meetings.rs");
    let start = src
        .find("pub fn speaker_label(")
        .expect("speaker_label must exist");
    let body = &src[start..];
    let end = body.find("\n}\n").expect("a closing brace") + 3;
    let body = &body[..end];

    assert!(
        body.contains("target: DiarizationTarget"),
        "speaker_label must take the meeting's diarization target, not a track \
         alone:\n{body}"
    );
    assert!(
        body.contains("DiarizationTarget::MicIsMe => MIC_SPEAKER_LABEL"),
        "the ONE arm that may answer \"Me\" must be the target's, not the \
         track's:\n{body}"
    );
    assert!(
        !body.contains("is_me"),
        "whether a voice is this user is a `speaker_profiles.is_me` match \
         (YV128), never something re-derived inside the renderer:\n{body}"
    );
    // …and the file does mention `is_me`, in the place that says where it
    // actually lives — so this is not passing because the concept was deleted.
    assert!(
        src.contains("speaker_profiles"),
        "meetings.rs must still name where `is_me` lives"
    );
}

/// **YV125's manual acceptance criterion is only HALF met on this base, and
/// this test is what stops that from being forgotten.**
///
/// The criterion reads: "a synthetic two-speaker `in_person`-kind recording
/// produces two distinct (unnamed, pre-enrollment) speaker clusters on Track A
/// — not one 'Me' label swallowing both voices."
///
/// The second clause ships here and is evidenced in the PR: an `in_person`
/// meeting is routed to `ClusterTrackA` and its transcript carries no "Me" at
/// all, so nothing swallows the room any more. The FIRST clause — two distinct
/// clusters — cannot ship here, because on this base there is no clusterer:
/// YV126 is the item that adds `cluster_track` and the `meeting_segments.
/// cluster_index` column (migration 5) the clusters are stored in. Producing
/// "two clusters" today would mean inventing them.
///
/// So the gap is instrumented rather than described. This asserts that no
/// migration in the shipped ladder mentions `cluster_index` — the one condition
/// under which "there is nothing to count clusters with" is honest. The day
/// YV126 adds that column, this goes red, on the merge commit, with no corpus,
/// no model and no audio hardware needed. Closing it means recording the
/// two-cluster evidence for an `in_person` fixture in YV126's PR and THEN
/// deleting this test — in that order, not the other one.
#[test]
fn the_two_cluster_half_of_the_manual_criterion_expires_when_yv126_lands() {
    let ladder = [
        meetings::MIGRATION_1_MEETINGS,
        meetings::MIGRATION_2_MEETING_DIAGNOSTICS,
        meetings::MIGRATION_3_TWO_TRACK,
        meetings::MIGRATION_4_MEETING_KIND,
    ];
    assert!(
        !ladder.iter().any(|sql| sql.contains("cluster_index")),
        "`meeting_segments.cluster_index` has landed, so clustering exists and \
         YV125's manual criterion can finally be met in full. Record the \
         two-distinct-clusters evidence for an `in_person` fixture in that PR, \
         then delete this test."
    );
    // The ladder above must be the WHOLE ladder, or this could pass by omission.
    assert_eq!(
        wilson_voice_lib::meetings::SCHEMA_VERSION,
        ladder.len() as i64,
        "a migration was added without being listed here, so this expiry stopped \
         looking at the whole schema"
    );
}

/// The two labels are different strings. Without this, every assertion above
/// would still pass if somebody set `UNCLUSTERED_SPEAKER_LABEL = "Me"`.
#[test]
fn the_branch_is_not_vacuous() {
    assert_ne!(UNCLUSTERED_SPEAKER_LABEL, MIC_SPEAKER_LABEL);
    assert_ne!(UNCLUSTERED_SPEAKER_LABEL, SYSTEM_SPEAKER_LABEL);
    assert_ne!(DiarizationTarget::MicIsMe, DiarizationTarget::ClusterTrackA);
    assert_ne!(
        meetings::speaker_label(MIC_TRACK, DiarizationTarget::MicIsMe),
        meetings::speaker_label(MIC_TRACK, DiarizationTarget::ClusterTrackA),
    );
}
