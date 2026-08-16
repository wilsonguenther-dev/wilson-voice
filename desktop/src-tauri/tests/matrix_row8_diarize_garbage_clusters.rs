//! YV132 · matrix row 8 — the diarizer ran, and what it returned is noise.
//!
//! ```sh
//! cargo test --test matrix_row8_diarize_garbage_clusters
//! ```
//!
//! Row 8 exists separately from row 7 for one reason, and every assertion here
//! is downstream of it: **a sidecar that crashed and a sidecar that answered
//! with rubbish are two different bugs.** They produce the same artifact — the
//! plain transcript, the meeting still complete — so the only thing that can
//! keep them apart afterwards is the reason each one carries, which is why this
//! file spends as much of its budget on the log tag as on the gate.
//!
//! **The published behaviour is not the plan's.** §6 row 8 says "cluster count >
//! `max(8, attendees×2)` ⇒ reject"; merged finding #25 killed that rule, and the
//! table publishes what replaced it — rank, floor, one "Other" bucket, degrade
//! only when nothing survives. The tests below assert the replacement *and* that
//! the old rule is absent, because a hard reject is exactly the behaviour a
//! future reader would assume from the plan.
//!
//! **And the degrade is a requirement on YV126, not a second opinion beside
//! it.** #141 ships `diarize::rank_and_floor`, which is the gate a meeting will
//! actually call, and for an all-below-floor pass it answers with one "Other"
//! chip rather than with this row's degrade. Two tests at the bottom of this
//! file are what keep that from being a table that says one thing while the
//! code does another: the absence tripwire now names the shipping symbols too,
//! and `the_published_degrade_is_a_requirement_on_yv126s_gate_and_the_two_may_not_diverge`
//! goes red the day `rank_and_floor` lands with nothing applying the degrade.
//!
//! **And it asserts that this module owns no number.** The floor is the
//! caller's, and YV126 is what measures it against the eval fixtures. A `const`
//! here would be a threshold entering the codebase through the file that renders
//! the published matrix, which is the worst available door for one.

use std::fs;
use std::path::PathBuf;

use wilson_voice_lib::diarize::DiarizeError;
use wilson_voice_lib::meeting_matrix::{
    cluster_sanity, diarize_sidecar_degrade, ClusterFloor, ClusterSummary, DegradeReason,
    SpeakerLabels, DIARIZATION_FAILED,
};
use wilson_voice_lib::meetings::MeetingState;

#[path = "support/callsite.rs"]
mod callsite;
use callsite::{call_sites, promote_the_row};

/// A floor with the shape YV126 will measure. The NUMBERS are this test's, not
/// the app's — they appear here and in no `src/` file, which is the point.
fn a_floor() -> ClusterFloor {
    ClusterFloor {
        min_speech_seconds: 30.0,
        min_turns: 3,
    }
}

fn cluster(index: u32, speech_seconds: f64, turns: usize) -> ClusterSummary {
    ClusterSummary {
        cluster: index,
        speech_seconds,
        turns,
    }
}

/// The forty-clusters case, as the plan names it: a pass that produced a pile of
/// fragments, none of which is a person. Nothing clears the floor, so the
/// meeting degrades to the same plain transcript row 7 produces.
#[test]
fn a_pass_with_nothing_above_the_floor_degrades_to_a_plain_transcript() {
    let confetti: Vec<ClusterSummary> = (0..40).map(|i| cluster(i, 1.5, 1)).collect();
    let verdict = cluster_sanity(&confetti, a_floor());

    assert_eq!(
        verdict.labels,
        SpeakerLabels::Plain(DegradeReason::Clusters { raw_clusters: 40 })
    );
    assert_eq!(verdict.labels.marker(), Some(DIARIZATION_FAILED));
    assert_eq!(
        verdict.labels.meeting_state_after(MeetingState::Complete),
        MeetingState::Complete,
        "a noisy diarization pass never fails a meeting either"
    );
    // "Never show 40 speaker chips" — the plan's own words for this row.
    assert!(verdict.chips.is_empty(), "{:?}", verdict.chips);
    assert_eq!(verdict.other, 40);
}

/// An empty answer is the same case and must not read as "one speaker".
#[test]
fn an_empty_cluster_set_degrades_rather_than_implying_a_single_speaker() {
    let verdict = cluster_sanity(&[], a_floor());
    assert_eq!(
        verdict.labels,
        SpeakerLabels::Plain(DegradeReason::Clusters { raw_clusters: 0 })
    );
    assert!(verdict.chips.is_empty());
}

/// The case the plan's original rule would have thrown away: a real
/// six-person far-field room, 15 raw clusters, four of them people.
///
/// Under `count > max(8, attendees×2)` this pass is rejected outright — 15 > 8,
/// no attendee list on a manually started meeting — and the room loses its
/// diarization entirely. Under rank + floor it keeps its four speakers.
#[test]
fn a_real_room_keeps_its_speakers_instead_of_being_rejected_for_being_busy() {
    let mut clusters = vec![
        cluster(0, 412.0, 31),
        cluster(1, 190.5, 22),
        cluster(2, 88.0, 9),
        cluster(3, 44.25, 4),
    ];
    // …plus eleven fragments, which is what a far-field room's raw output looks
    // like before anything is merged.
    clusters.extend((4..15).map(|i| cluster(i, 2.0, 1)));
    assert_eq!(clusters.len(), 15);

    let verdict = cluster_sanity(&clusters, a_floor());
    assert_eq!(verdict.labels, SpeakerLabels::Attributed);
    assert_eq!(verdict.labels.marker(), None);
    // Ranked, most speech first — a chip row that reshuffles between two
    // identical passes is its own bug.
    assert_eq!(verdict.chips, vec![0, 1, 2, 3]);
    assert_eq!(
        verdict.other, 11,
        "the fragments are one bucket, not eleven chips"
    );
}

/// Both halves of the floor bite, and neither alone is enough. A cluster with
/// plenty of speech in two turns is a doorbell and a cough; a cluster with forty
/// turns of half a second each is a fragmentation artifact.
#[test]
fn the_floor_is_two_conditions_and_both_of_them_matter() {
    let floor = a_floor();
    let long_but_few_turns = cluster(0, 300.0, 2);
    let many_but_short = cluster(1, 12.0, 40);
    assert!(!floor.admits(&long_but_few_turns));
    assert!(!floor.admits(&many_but_short));
    assert!(floor.admits(&cluster(2, 30.0, 3)), "the floor is inclusive");

    let verdict = cluster_sanity(&[long_but_few_turns, many_but_short], floor);
    assert!(verdict.labels.is_plain());
}

/// **The number belongs to the caller.** The same cluster set is a good pass
/// under one floor and a garbage pass under another, and this module has no
/// opinion about which — because YV126 measures the floor against the eval
/// fixtures and nothing else may declare it.
#[test]
fn the_floor_is_the_callers_number_and_this_module_declares_none() {
    let clusters = [cluster(0, 45.0, 4), cluster(1, 20.0, 2)];

    let lenient = cluster_sanity(
        &clusters,
        ClusterFloor {
            min_speech_seconds: 10.0,
            min_turns: 2,
        },
    );
    assert_eq!(lenient.labels, SpeakerLabels::Attributed);
    assert_eq!(lenient.chips, vec![0, 1]);

    let strict = cluster_sanity(
        &clusters,
        ClusterFloor {
            min_speech_seconds: 120.0,
            min_turns: 10,
        },
    );
    assert!(
        strict.labels.is_plain(),
        "the same clusters must be able to fail a stricter floor, or the floor is not the input"
    );

    // …and the source itself carries no such number. A `const` in this file
    // would be a threshold entering the tree through the module that renders the
    // published matrix — the eval-first sequencing of this whole phase exists to
    // stop exactly that.
    let src = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("meeting_matrix.rs"),
    )
    .expect("read src/meeting_matrix.rs");
    for line in src.lines() {
        let code = line.trim_start();
        if code.starts_with("//") || !code.contains("const ") {
            continue;
        }
        let lowered = code.to_ascii_lowercase();
        for banned in [
            "min_speech",
            "min_turns",
            "floor",
            "threshold",
            "cluster_count",
        ] {
            assert!(
                !lowered.contains(banned),
                "src/meeting_matrix.rs declares `{line}`. Row 8's floor is measured by YV126 \
                 against `diarize_metrics` on the eval fixtures and passed in; a constant here \
                 would be a guess published as the required behaviour."
            );
        }
    }
}

/// The plan's original rule is not implemented, and this asserts it from the
/// direction a future reader would come at it: a big cluster count, on its own,
/// is not a rejection.
#[test]
fn a_high_cluster_count_is_not_by_itself_a_rejection() {
    // Twelve clusters, all substantial: a large but genuine meeting. The old
    // rule rejects at 9.
    let big: Vec<ClusterSummary> = (0..12).map(|i| cluster(i, 60.0, 8)).collect();
    let verdict = cluster_sanity(&big, a_floor());
    assert_eq!(verdict.labels, SpeakerLabels::Attributed);
    assert_eq!(verdict.chips.len(), 12);
    assert_eq!(verdict.other, 0);
}

/// **The two failure modes are diagnosable apart.** Same artifact, same marker,
/// different reason — asserted on the strings a human actually reads, because
/// that is where the distinction has to survive.
#[test]
fn a_crashed_sidecar_and_a_noisy_one_are_distinguishable_in_the_log() {
    let crashed = diarize_sidecar_degrade(&DiarizeError::Deadline);
    let noisy = cluster_sanity(&[cluster(0, 1.0, 1)], a_floor()).labels;

    // Same artifact: both keep the transcript and both complete the meeting.
    assert!(crashed.is_plain() && noisy.is_plain());
    assert_eq!(crashed.marker(), noisy.marker());
    assert_eq!(crashed.marker(), Some(DIARIZATION_FAILED));

    // Different reason, and the tags are the two words the row publishes.
    assert_eq!(crashed.reason().map(|r| r.log_tag()), Some("sidecar"));
    assert_eq!(noisy.reason().map(|r| r.log_tag()), Some("clusters"));
    assert_ne!(crashed, noisy);

    let noisy_line = noisy.reason().expect("a reason").log_line();
    assert!(noisy_line.contains("reason=clusters"), "{noisy_line}");
    assert!(
        noisy_line.contains("raw_clusters=1"),
        "the count is what makes 1-for-five-people and 40-for-three different \
         lines in a log: {noisy_line}"
    );
    assert!(
        !noisy_line.contains("reason=sidecar"),
        "the two reasons must not both appear: {noisy_line}"
    );
}

/// The half of row 8 this gate CANNOT see, asserted so nobody mistakes the gate
/// for an accuracy check.
///
/// "One cluster for five people" clears every floor: it is long, it has plenty
/// of turns, and nothing about it is detectable without ground truth. That half
/// is YV126's DER gate on the eval fixtures — a measurement, not a runtime
/// check. This test exists so the limitation is written down where the gate is,
/// rather than being discovered later as a bug in it.
#[test]
fn under_clustering_is_not_detectable_here_and_the_gate_does_not_pretend() {
    let five_people_heard_as_one = cluster(0, 1_800.0, 240);
    let verdict = cluster_sanity(&[five_people_heard_as_one], a_floor());
    assert_eq!(
        verdict.labels,
        SpeakerLabels::Attributed,
        "a single long cluster is indistinguishable from a genuinely \
         single-speaker recording without ground truth"
    );
    assert_eq!(verdict.chips, vec![0]);
}

/// The row's own tripwire: row 8 is `Policy only, NOT WIRED` until something
/// applies a rank+floor gate to real output.
///
/// **The symbol this watches MOVED when YV126 landed, and that is the point of
/// the move rather than a loosening.** It used to watch `cluster_sanity`, on
/// the reasoning that a caller for it would mean somebody had wired the row.
/// YV126 gave it one — `diarize::rank_and_floor` now applies it, which is how
/// the two stopped being two answers to one question — so watching
/// `cluster_sanity` would fire on the change that made the published behaviour
/// TRUE, and it would be watching the wrong symbol anyway: the thing still
/// absent from the app is the GATE.
///
/// So this asserts the two halves separately. `rank_and_floor`, `cluster_track`
/// and `attribute_clusters` have no caller in `src/` outside `diarize.rs` — no
/// meeting is ever diarized, so there is no pass for any gate to be applied to,
/// and that is what keeps the row `PolicyOnly`. And `cluster_sanity` DOES have
/// one, which is asserted rather than assumed: if the shipping gate ever stops
/// applying the published degrade, the divergence comes back and this says so
/// at the point where it happened.
#[test]
fn nothing_in_the_app_applies_the_sanity_gate_yet() {
    for shipping in ["rank_and_floor", "cluster_track", "attribute_clusters"] {
        let wired = call_sites(shipping, &["diarize.rs"]);
        assert!(
            wired.is_empty(),
            "{}",
            promote_the_row("8", shipping, &wired)
        );
    }

    let applies: Vec<String> = call_sites("cluster_sanity", &["meeting_matrix.rs"])
        .iter()
        .map(|p| {
            PathBuf::from(p)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.clone())
        })
        .collect();
    assert_eq!(
        applies,
        vec!["diarize.rs".to_string()],
        "the published row 8 degrade must be applied by the shipping gate and by \
         nothing else. Found: {applies:?}. If `diarize.rs` has stopped calling \
         `cluster_sanity`, `rank_and_floor` has gone back to answering an \
         all-below-floor pass with one \"Other\" chip while this table publishes a \
         plain transcript and a `diarization_failed` marker — two answers to one \
         question, which is the state this file exists to make impossible."
    );
}

/// **The shipping gate gives row 8's answer, driven through the shipping type.**
///
/// The pure-function tests above establish what the policy IS. This establishes
/// that the code a meeting will call performs it — on the identical
/// forty-fragment input, through `diarize::rank_and_floor`, on
/// `diarize::DiarizedSegment`s rather than on `ClusterSummary`s built by hand.
///
/// The old version of this file could not do this: `rank_and_floor` did not
/// exist on `main`, so the binding had to be a source-level reconciliation
/// check. It exists now, so the row is held to the real thing.
#[test]
fn the_shipping_gate_degrades_the_same_way_on_the_same_input() {
    use wilson_voice_lib::diarize::{rank_and_floor, DiarizedSegment, CHIP_FLOOR_SECONDS};
    use wilson_voice_lib::meetings::MIC_TRACK;

    // Forty fragments of 1.5 s, one turn each — the same pass
    // `a_pass_with_nothing_above_the_floor_degrades_to_a_plain_transcript`
    // hands the pure function.
    let confetti: Vec<DiarizedSegment> = (0..40)
        .map(|i| {
            let start = i as f64 * 2.0;
            DiarizedSegment::new(MIC_TRACK, start, start + 1.5, Some(i))
        })
        .collect();
    let ranking = rank_and_floor(&confetti);

    assert!(
        ranking.surfaced.is_empty(),
        "nothing clears the floor: {:?}",
        ranking.surfaced
    );
    assert_eq!(ranking.other.len(), 40, "and nothing is thrown away either");
    assert_eq!(
        ranking.labels,
        SpeakerLabels::Plain(DegradeReason::Clusters { raw_clusters: 40 })
    );
    assert_eq!(ranking.labels.marker(), Some(DIARIZATION_FAILED));
    assert!(
        ranking.chips().is_empty(),
        "the published behaviour is NO chips for a pass that found no speaker — \
         one \"Other\" chip over forty fragments presents noise as attribution: {:?}",
        ranking.chips()
    );

    // Non-vacuity: the same gate on a pass that DOES clear the floor still
    // surfaces a speaker, so the assertion above is about the degrade and not
    // about `rank_and_floor` having stopped working.
    let a_real_speaker: Vec<DiarizedSegment> = (0..4)
        .map(|i| {
            let start = i as f64 * 20.0;
            DiarizedSegment::new(
                MIC_TRACK,
                start,
                start + CHIP_FLOOR_SECONDS / 3.0 + 1.0,
                Some(0),
            )
        })
        .collect();
    let ranking = rank_and_floor(&a_real_speaker);
    assert_eq!(ranking.labels, SpeakerLabels::Attributed);
    assert_eq!(ranking.chips(), vec!["Speaker 1".to_string()]);
}

/// **Why the reconciliation test that used to live here is gone.**
///
/// It asserted that `src/diarize.rs`'s rank+floor gate and this row's degrade
/// could not diverge, and it could only assert it at the level of source text —
/// `rank_and_floor` did not exist on `main` when it was written, so a test
/// naming the symbol would not have compiled. It has now been discharged in the
/// way it asked for: the shipping gate applies `cluster_sanity`, so there is one
/// implementation of the behaviour instead of two, and
/// `the_shipping_gate_degrades_the_same_way_on_the_same_input` drives the real
/// type on the real input instead of comparing two descriptions of it. The
/// source-text check is deleted rather than left beside its own replacement —
/// which is what its own instructions said to do, and the same discipline YV125's
/// expiring test was closed under.
#[test]
fn the_reconciliation_is_discharged_by_the_shipping_gate_not_by_a_source_scan() {
    // The claim in one line, checked rather than asserted in prose: this file
    // holds row 8 to the shipping type, and the module it lives in is the one
    // that applies the published degrade.
    let diarize_src =
        fs::read_to_string(callsite::src_dir().join("diarize.rs")).expect("read src/diarize.rs");
    assert!(
        callsite::mentions_as_code(&diarize_src, "cluster_sanity("),
        "src/diarize.rs no longer applies `cluster_sanity`, so the published row 8 \
         degrade and the gate a meeting will call have come apart again"
    );
    assert!(
        callsite::mentions_as_code(&diarize_src, "fn rank_and_floor"),
        "the shipping gate is gone"
    );
}
