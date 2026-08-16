//! YV129 — "who is this?" is a **completed-meeting** affordance. Not a modal,
//! not mid-recording, and not reachable from the live capture module at all.
//!
//! The plan's F2 flow is an inline chip row in the meeting-detail view:
//! `Speaker 2 → [Jeisil ▾] [Aidan] [+ new]`. The failure it is written against
//! is the one every meeting tool eventually ships: a dialog that interrupts the
//! thing being recorded to ask about the thing being recorded.
//!
//! Two independent proofs, because either alone is weak:
//!
//! 1. **The mechanism refuses.** `who_is_this_chips` takes whether the meeting
//!    has ended and returns [`ChipRowError::MeetingStillLive`] when it has not.
//!    A caller cannot get a chip row for a running meeting even by asking.
//! 2. **The call site does not exist.** No symbol of the chip row appears as
//!    code anywhere in `meeting.rs`, the live capture module — the backlog's own
//!    `grep -n "who_is_this\|SpeakerChip" desktop/src-tauri/src/meeting.rs`
//!    criterion, run by a machine, with comments and string literals excluded
//!    (`tests/support/callsite.rs` learned both exclusions from false results).
//!
//! The grep half is also checked for NON-vacuity: the same symbols must be
//! present as code in the module that owns them, so a rename, a deletion or a
//! typo in the needle cannot make "no matches in meeting.rs" mean nothing.

use wilson_voice_lib::diarize_metrics::CosineSimilarity;
use wilson_voice_lib::speaker_profiles::{
    match_meeting_clusters, who_is_this_chips, Centroid, ChipFloor, ChipRowError, ClusterDecision,
    ClusterSummary, Embedding, EnrollmentBands, MatchResult, SpeakerProfile,
};

#[path = "support/callsite.rs"]
mod callsite;

/// The symbols the backlog's grep names, plus the two types the row is made of.
const CHIP_SYMBOLS: [&str; 4] = ["who_is_this", "SpeakerChip", "ChipRow", "ChipFloor"];

/// The module that owns the affordance — where the symbols MUST appear.
const OWNER: &str = "speaker_profiles.rs";

/// The live-capture module — where they must NOT.
const LIVE: &str = "meeting.rs";

fn src(file: &str) -> String {
    std::fs::read_to_string(callsite::src_dir().join(file))
        .unwrap_or_else(|e| panic!("read src/{file}: {e}"))
}

fn dims(k: usize) -> Embedding {
    let mut v = vec![0.0f32; 8];
    v[k] = 1.0;
    Embedding::new(v)
}

fn bands() -> EnrollmentBands {
    EnrollmentBands::new(CosineSimilarity::new(0.90), CosineSimilarity::new(0.50))
        .expect("well-ordered test bands")
}

fn floor() -> ChipFloor {
    // The backlog's sketched floor, supplied by the CALLER — see `ChipFloor`'s
    // doc comment for why it is a parameter and not a constant in the crate.
    ChipFloor::new(30.0, 3)
}

fn cluster(index: i64, speech_seconds: f64, turns: usize, voice: usize) -> ClusterSummary {
    ClusterSummary {
        cluster_index: index,
        label: format!("Speaker {}", index + 1),
        centroid: dims(voice),
        speech_seconds,
        turns,
    }
}

fn roster() -> Vec<SpeakerProfile> {
    vec![
        SpeakerProfile {
            id: "p_jeisil".into(),
            display_name: "Jeisil".into(),
            is_me: false,
            centroids: vec![Centroid::new("laptop_mic_near", dims(0))],
        },
        SpeakerProfile {
            id: "p_aidan".into(),
            display_name: "Aidan".into(),
            is_me: false,
            centroids: vec![Centroid::new("laptop_mic_near", dims(1))],
        },
    ]
}

/// The acceptance criterion, both halves.
#[test]
fn who_is_this_never_modal_never_live() {
    // (1) the mechanism refuses a meeting that has not ended.
    let decisions = vec![ClusterDecision {
        cluster: cluster(0, 120.0, 9, 5),
        result: MatchResult::New,
    }];
    assert_eq!(
        who_is_this_chips(false, &decisions, &[], &roster(), floor()),
        Err(ChipRowError::MeetingStillLive),
        "a running meeting must not be able to produce a who-is-this row"
    );
    let row = who_is_this_chips(true, &decisions, &[], &roster(), floor())
        .expect("a finished meeting may");
    assert_eq!(row.chips.len(), 1, "and the same input DOES produce one once \
                                     the meeting has ended — the refusal above \
                                     is about the meeting's state, not about an \
                                     empty fixture");

    // (2) no symbol of the row is code anywhere in the live capture module.
    let live = src(LIVE);
    let owner = src(OWNER);
    for symbol in CHIP_SYMBOLS {
        assert!(
            !callsite::mentions_as_code(&live, symbol),
            "`{symbol}` appears as CODE in src/{LIVE}. The who-is-this row is a \
             meeting-detail affordance; a live-capture call site is the \
             mid-meeting prompt the plan's F2 flow rules out."
        );
        assert!(
            callsite::mentions_as_code(&owner, symbol),
            "`{symbol}` is not code in src/{OWNER} either, so the assertion above \
             proves nothing. If the symbol was renamed, rename it here too."
        );
    }
}

/// Nothing in the whole shipping tree calls the chip row from a capture path.
///
/// Broader than the backlog's grep, and cheap: the row's builder may only be
/// reached from a surface that has a finished meeting in hand. Today that is
/// nowhere in `src/` outside its own module, which is stated as the fact it is —
/// the Meetings detail view is React, and it reaches this through the same
/// rules mirrored in `src/meetings/speakerChips.ts` (the mirror discipline
/// `meetings::render_transcript` already documents).
#[test]
fn the_chip_row_builder_has_no_call_site_on_any_capture_path() {
    let capture_modules = [
        "meeting.rs",
        "meeting_asr.rs",
        "meeting_control.rs",
        "meeting_energy.rs",
        "record.rs",
        "syscapture.rs",
        "dictation.rs",
    ];
    for module in capture_modules {
        let body = src(module);
        assert!(
            !callsite::mentions_as_code(&body, "who_is_this_chips"),
            "src/{module} calls who_is_this_chips — capture must never ask"
        );
        assert!(
            !callsite::mentions_as_code(&body, "match_cluster"),
            "src/{module} calls match_cluster — identity is decided after a \
             meeting ends, not while it records"
        );
    }
}

/// A confident match asks nothing. That is what the auto-confirm band is FOR.
#[test]
fn an_auto_confirmed_cluster_gets_no_chip() {
    let clusters = vec![
        cluster(0, 200.0, 12, 0), // exactly Jeisil's centroid
        cluster(1, 150.0, 10, 7), // orthogonal to everyone enrolled
    ];
    let decisions = match_meeting_clusters(&clusters, &roster(), bands());
    assert!(matches!(decisions[0].result, MatchResult::Known { .. }));
    assert_eq!(decisions[1].result, MatchResult::New);

    let row = who_is_this_chips(true, &decisions, &[], &roster(), floor()).expect("finished");
    assert_eq!(
        row.chips.iter().map(|c| c.cluster_index).collect::<Vec<_>>(),
        vec![1],
        "only the unknown voice is asked about"
    );
    assert_eq!(row.rolled_into_other, 0);
}

/// Offered once, inline, and never again.
#[test]
fn a_cluster_the_user_has_already_answered_is_never_re_offered() {
    let decisions = vec![
        ClusterDecision {
            cluster: cluster(0, 200.0, 12, 5),
            result: MatchResult::New,
        },
        ClusterDecision {
            cluster: cluster(1, 150.0, 10, 6),
            result: MatchResult::New,
        },
    ];
    let row = who_is_this_chips(true, &decisions, &[0], &roster(), floor()).expect("finished");
    assert_eq!(
        row.chips.iter().map(|c| c.cluster_index).collect::<Vec<_>>(),
        vec![1],
        "cluster 0 was answered; re-asking is the spam this item is about"
    );
    // Non-vacuous: with nothing answered, both are offered.
    let both = who_is_this_chips(true, &decisions, &[], &roster(), floor()).expect("finished");
    assert_eq!(both.chips.len(), 2);
}

/// **The batching claim, on a six-speaker classroom.**
///
/// Six clusters; four clear the floor and two are quiet stragglers. The row
/// asks four questions and reports the other two as one "Other" bucket — never
/// six chips, never one per voice change, and never a hard reject of the whole
/// pass (the misfire YV126 replaced with this ranking + floor).
#[test]
fn a_six_speaker_classroom_asks_four_questions_not_six() {
    let clusters = vec![
        cluster(0, 210.0, 18, 2),
        cluster(1, 95.0, 11, 3),
        cluster(2, 61.0, 7, 4),
        cluster(3, 33.0, 4, 5),
        cluster(4, 12.0, 3, 6), // under the 30 s floor
        cluster(5, 44.0, 2, 7), // over on seconds, under on turns
    ];
    let decisions = match_meeting_clusters(&clusters, &roster(), bands());
    let row = who_is_this_chips(true, &decisions, &[], &roster(), floor()).expect("finished");

    assert_eq!(row.chips.len(), 4, "{:#?}", row.chips);
    assert_eq!(row.rolled_into_other, 2);
    assert_eq!(
        row.chips.iter().map(|c| c.cluster_index).collect::<Vec<_>>(),
        vec![0, 1, 2, 3],
        "busiest voice first — the ranking, not the cluster order"
    );
    for chip in &row.chips {
        assert!(chip.allow_new, "the roster is never assumed complete");
        assert_eq!(
            chip.alternatives.len(),
            2,
            "both enrolled people are offered when nothing is suggested"
        );
        assert!(chip.cluster_label.starts_with("Speaker "));
    }
}

/// A near-threshold match arrives pre-selected, with the rest of the roster
/// beside it — the `[Jeisil ▾] [Aidan] [+ new]` shape, and never two chips for
/// one voice.
#[test]
fn a_suggested_cluster_arrives_pre_selected_with_the_rest_of_the_roster_beside_it() {
    // cos([1,1,0,…], Jeisil) = 0.7071 — between the bands.
    let mut v = vec![0.0f32; 8];
    v[0] = 1.0;
    v[4] = 1.0;
    let between = ClusterSummary {
        cluster_index: 3,
        label: "Speaker 4".into(),
        centroid: Embedding::new(v),
        speech_seconds: 88.0,
        turns: 9,
    };
    let decisions = match_meeting_clusters(&[between], &roster(), bands());
    assert!(matches!(decisions[0].result, MatchResult::Suggested { .. }));

    let row = who_is_this_chips(true, &decisions, &[], &roster(), floor()).expect("finished");
    assert_eq!(row.chips.len(), 1, "one voice, one question");
    let chip = &row.chips[0];
    let suggested = chip.suggested.as_ref().expect("a suggestion is pre-selected");
    assert_eq!(suggested.profile_id, "p_jeisil");
    assert_eq!(suggested.display_name, "Jeisil");
    assert!((suggested.score.get() - 0.7071).abs() < 1e-3);
    assert_eq!(
        chip.alternatives
            .iter()
            .map(|a| a.display_name.as_str())
            .collect::<Vec<_>>(),
        vec!["Aidan"],
        "the suggested person is not repeated in the alternatives"
    );
    assert_eq!(chip.cluster_label, "Speaker 4");
}
