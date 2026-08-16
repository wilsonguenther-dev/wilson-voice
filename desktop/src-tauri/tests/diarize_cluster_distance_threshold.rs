//! YV126 acceptance — the clustering threshold is a cosine DISTANCE, and it
//! decides what groups and what splits.
//!
//! ```sh
//! cargo test --test diarize_cluster_distance_threshold
//! ```
//!
//! Pure in the sense that matters: no model, no onnxruntime, no audio hardware,
//! no corpus. The embeddings are unit vectors placed at chosen angles, so the
//! cosine distance between any two of them is a number this file ASKED for
//! rather than one it discovered — which is what makes "below the threshold
//! groups, above it splits" a falsifiable claim rather than a description of
//! whatever the code happened to do.
//!
//! Every threshold here is a [`CosineDistance`]. That is not a style rule:
//! `sherpa-onnx`'s clustering threshold is a distance (smaller = more similar)
//! and the plan's enrollment bands are similarities (larger = more similar), and
//! a codebase of bare `f32`s would compare the two without a word (merged
//! finding #20). `the_public_clustering_api_takes_a_typed_distance` holds the
//! shipped signature to it.

mod support;

use std::path::Path;

use support::diarize_stub::{
    angle_for_distance, ray, stub_echoing_min_embed, stub_returning, stub_with_body, StubTurn,
    STUB_MIN_EMBED,
};
use wilson_voice_lib::diarize::{
    cluster_by_distance_threshold, cluster_track, DiarizePool, MeetingTracks, TargetMode,
};
use wilson_voice_lib::diarize_metrics::{cosine_similarity, CosineDistance, CosineSimilarity};
use wilson_voice_lib::meetings::{MeetingKind, MIC_TRACK};

const READY: std::time::Duration = std::time::Duration::from_secs(10);

/// How many distinct clusters a labelling contains.
fn distinct(labels: &[usize]) -> usize {
    let mut seen: Vec<usize> = labels.to_vec();
    seen.sort_unstable();
    seen.dedup();
    seen.len()
}

/// The mechanism, at the smallest size that can show it: two turns at a KNOWN
/// cosine distance, and a threshold swept across it.
///
/// The pair is 0.30 apart. At 0.35 it is one speaker; at 0.25 it is two. Same
/// audio, same embeddings, same code — the threshold is the whole difference,
/// which is precisely why it may not be copied from a blog that measured a
/// different embedding model.
#[test]
fn one_pair_groups_below_the_threshold_and_splits_above_it() {
    let separation = 0.30f64;
    let a = ray(0.0);
    let b = ray(angle_for_distance(separation));
    // The fixture is what it claims to be, measured rather than asserted by
    // comment: a test built on a mis-stated distance would prove nothing.
    let measured = CosineDistance::from_similarity(cosine_similarity(&a, &b));
    assert!(
        (f64::from(measured.get()) - separation).abs() < 1e-4,
        "the two rays are {} apart, not {separation}",
        measured.get()
    );

    let points: Vec<&[f32]> = vec![&a, &b];
    assert_eq!(
        distinct(&cluster_by_distance_threshold(
            &points,
            CosineDistance::new(0.35)
        )),
        1,
        "0.30 apart, threshold 0.35: one speaker"
    );
    assert_eq!(
        distinct(&cluster_by_distance_threshold(
            &points,
            CosineDistance::new(0.25)
        )),
        2,
        "0.30 apart, threshold 0.25: two speakers"
    );
}

/// The same claim through the SHIPPED entry point — `cluster_track`, over the
/// sidecar wire, against a stub process with zero model bytes.
///
/// The stub's own `cluster` field says every turn is cluster 0. That is
/// deliberate: the parent re-clusters from the embeddings at the threshold it
/// was given, and a stub that already agreed with the right answer could not
/// tell a working clusterer from one that forwarded the child's opinion.
#[test]
fn cluster_track_groups_and_splits_at_the_threshold_it_was_given() {
    // Three turns: two of one voice (0.10 apart), one of another (0.80 away).
    let close = angle_for_distance(0.10);
    let far = angle_for_distance(0.80);
    let turns = vec![
        StubTurn::new(0.0, 4.0, 0, ray(0.0)),
        StubTurn::new(4.5, 9.0, 0, ray(close)),
        StubTurn::new(9.5, 14.0, 0, ray(far)),
    ];

    for (threshold, expected, note) in [
        (
            0.35f32,
            vec![Some(0i64), Some(0), Some(1)],
            "two voices: the near pair merges, the far turn does not",
        ),
        (
            0.05,
            vec![Some(0), Some(1), Some(2)],
            "a threshold under every gap: three singletons",
        ),
        (
            0.95,
            vec![Some(0), Some(0), Some(0)],
            "a threshold over every gap: one cluster, correctly",
        ),
    ] {
        let pool = DiarizePool::new(stub_returning(turns.clone()), READY);
        let mic = Path::new("/tmp/does-not-need-to-exist.wav");
        let out = cluster_track(
            &pool,
            MeetingTracks {
                mic_wav: mic,
                system_wav: None,
            },
            MeetingKind::InPerson,
            CosineDistance::new(threshold),
            TargetMode::FullClustering,
            STUB_MIN_EMBED,
        )
        .expect("the stub answers");
        pool.shutdown();

        let got: Vec<Option<i64>> = out.iter().map(|s| s.cluster_index).collect();
        assert_eq!(got, expected, "threshold {threshold}: {note}");
        // The turn boundaries and the track survive clustering untouched — the
        // pass attributes speech, it does not re-cut it.
        assert_eq!(out.len(), turns.len());
        assert!(out.iter().all(|s| s.track == MIC_TRACK));
        assert!((out[0].start_seconds - 0.0).abs() < 1e-9);
        assert!((out[2].end_seconds - 14.0).abs() < 1e-9);
    }
}

/// The child's cluster ids are the FALLBACK, not the answer.
///
/// A backend that returns turns without embeddings still gets used — a
/// diarization with the vendor's own clustering beats no diarization — but the
/// two paths must be distinguishable, or "the tuned threshold decided this"
/// becomes unfalsifiable. Same turns, same threshold, two answers.
#[test]
fn without_embeddings_the_childs_own_clusters_are_used_and_they_differ() {
    let mic = Path::new("/tmp/does-not-need-to-exist.wav");
    let tracks = MeetingTracks {
        mic_wav: mic,
        system_wav: None,
    };
    let threshold = CosineDistance::new(0.35);

    // No `embedding` field at all: the shape a backend that only reports
    // sherpa's own assignment produces.
    let bodyless = String::from(
        r#"{"start":0.0,"end":4.0,"cluster":7},{"start":4.5,"end":9.0,"cluster":7},{"start":9.5,"end":14.0,"cluster":9}"#,
    );
    let pool = DiarizePool::new(stub_with_body(bodyless), READY);
    let fallback = cluster_track(
        &pool,
        tracks,
        MeetingKind::InPerson,
        threshold,
        TargetMode::FullClustering,
        STUB_MIN_EMBED,
    )
    .expect("the stub answers");
    pool.shutdown();
    assert_eq!(
        fallback.iter().map(|s| s.cluster_index).collect::<Vec<_>>(),
        vec![Some(7), Some(7), Some(9)],
        "with no embeddings the child's own ids are passed through verbatim"
    );

    // The same three turns WITH embeddings that disagree with those ids: the
    // parent's threshold wins, and the answer changes.
    let turns = vec![
        StubTurn::new(0.0, 4.0, 7, ray(0.0)),
        StubTurn::new(4.5, 9.0, 7, ray(angle_for_distance(0.80))),
        StubTurn::new(9.5, 14.0, 9, ray(angle_for_distance(0.05))),
    ];
    let pool = DiarizePool::new(stub_returning(turns), READY);
    let ours = cluster_track(
        &pool,
        tracks,
        MeetingKind::InPerson,
        threshold,
        TargetMode::FullClustering,
        STUB_MIN_EMBED,
    )
    .expect("the stub answers");
    pool.shutdown();
    assert_eq!(
        ours.iter().map(|s| s.cluster_index).collect::<Vec<_>>(),
        vec![Some(0), Some(1), Some(0)],
        "turns 1 and 3 are 0.05 apart and turn 2 is 0.80 away — the child said \
         the opposite, and the child is not who decides"
    );
}

/// The unit discipline reaches the SIGNATURE, not just the prose.
///
/// A bare `f32` threshold on this API is the whole of merged finding #20: a
/// caller with a similarity in hand would pass it, clustering would run looser
/// than the identity decision it feeds, and every number downstream would be
/// wrong by an amount nobody could see. Rust cannot grep intent, so this greps
/// the source — the same trick `meeting_kind_branch.rs` uses to hold a rule to
/// a file.
#[test]
fn the_public_clustering_api_takes_a_typed_distance() {
    let src = include_str!("../src/diarize.rs");
    for signature in [
        "pub fn cluster_by_distance_threshold(",
        "pub fn cluster_track(",
    ] {
        let at = src
            .find(signature)
            .unwrap_or_else(|| panic!("{signature} is gone"));
        let body = &src[at..at + 400];
        let head = body.split(") ->").next().unwrap_or_default();
        assert!(
            head.contains("threshold: CosineDistance"),
            "{signature} no longer takes a typed cosine distance:\n{head}"
        );
        // An embedding is legitimately `&[f32]` — a vector of measurements has
        // no unit to confuse. What may never be a bare `f32` is a parameter
        // whose NAME says it is one of the two cosine quantities.
        for parameter in head.lines().filter(|l| {
            let l = l.to_ascii_lowercase();
            l.contains("threshold") || l.contains("distance") || l.contains("similarity")
        }) {
            assert!(
                !parameter.contains(": f32"),
                "{signature} takes a bare f32 where a cosine unit belongs — the \
                 mixed-unit bug is back:\n{parameter}"
            );
        }
    }

    // …and the two units really are different numbers, so none of the above is
    // passing because somebody made them aliases.
    let distance = CosineDistance::new(0.35);
    let similarity = CosineSimilarity::from_distance(distance);
    assert!((similarity.get() - 0.65).abs() < 1e-6);
    assert_ne!(distance.get(), similarity.get());
}

/// No accuracy threshold ships as a constant. Not one.
///
/// The clustering threshold is a PARAMETER of `cluster_track` precisely because
/// the only honest source for a value is a measurement against fixture (e), and
/// this tree has no inference backend to measure with yet. A `const
/// DEFAULT_CLUSTERING_DISTANCE` would be sherpa's 0.5 (or OpenWhispr's 0.70/0.55
/// read as the wrong unit) wearing a Rust type — the exact failure this
/// backlog's eval-first sequencing exists to prevent.
///
/// **Both units, not just the clustering one.** Binary mode
/// (`TargetMode::EnrolledVsEveryoneElse`) decides each turn against an
/// acceptance band in cosine SIMILARITY, and that band is a field on
/// `EnrolledSpeaker` supplied by the caller for exactly the same reason: YV129
/// measures it against YV120's `enrollment_eer`. A `const` of either unit in
/// this file is the same failure wearing a different newtype.
#[test]
fn no_tuned_clustering_constant_ships_in_the_crate() {
    /// The ONE fractional constant this file is allowed to hold, and the
    /// reason: it is a surfacing floor from the audit's own finding #25 — it
    /// changes what a person is shown, never what is computed or stored — so
    /// there is no measurement it could be an output of. Adding a name here is
    /// the deliberate act this test exists to force.
    const SURFACING_FLOORS: [&str; 1] = ["CHIP_FLOOR_SECONDS"];

    let src = include_str!("../src/diarize.rs");
    for line in src.lines() {
        let code = line.split("//").next().unwrap_or_default();
        if !code.contains("const") {
            continue;
        }
        for unit in ["CosineDistance", "CosineSimilarity"] {
            if !code.contains(unit) {
                continue;
            }
            panic!(
                "a {unit} constant has appeared in diarize.rs — every threshold \
                 in this epic is an OUTPUT of the eval harness, never an input: \
                 {line}"
            );
        }
        // …and the same number wearing a bare `f32` is the same failure. A
        // `const DEFAULT_ENROLLMENT_BAND: f32 = 0.70;` types as a plain float
        // and would sail past the unit check above, which is exactly how
        // OpenWhispr's blog bands would arrive.
        let Some((name, value)) = code.split_once('=') else {
            continue;
        };
        let name = name
            .rsplit_once(':')
            .map(|(before, _)| before)
            .unwrap_or(name);
        let name = name.split_whitespace().next_back().unwrap_or_default();
        let value = value.trim().trim_end_matches(';').trim();
        let fractional = value.contains('.')
            && value
                .trim_end_matches("f32")
                .trim_end_matches("f64")
                .parse::<f64>()
                .is_ok();
        assert!(
            !fractional || SURFACING_FLOORS.contains(&name),
            "the fractional constant `{name} = {value}` has appeared in \
             diarize.rs. If it is an accuracy threshold it may not ship — it is \
             an OUTPUT of the eval harness. If it is a surfacing floor, name it \
             in SURFACING_FLOORS and say why."
        );
    }
}

/// **A rebase guard.** The caller's `min_embed` floor reaches the WIRE, and
/// nothing in this crate supplies one on the caller's behalf.
///
/// YV122 landed after this item's branch was cut and made the floor a mandatory
/// parameter of `DiarizePool::diarize` with no default anywhere in either
/// crate — deliberately, because its own truncation sweep found two tenths of a
/// second coming back as an ordinary-looking embedding that matched its own
/// speaker worse than an average stranger did. The failure this test exists for
/// is the cheap way to resolve that rebase: `cluster_track` accepting a floor
/// and then passing a convenient constant of its own to the child. No assertion
/// on `cluster_track`'s return value could see that, so the stub echoes back
/// the `min_embed_seconds` it received as a turn boundary and the assertion is
/// made on the number that crossed the pipe.
#[test]
fn the_callers_min_embed_floor_reaches_the_child_unchanged() {
    let mic = Path::new("/tmp/does-not-need-to-exist.wav");
    // Two different floors, so a stub echoing a constant of its own would fail
    // one of them.
    for seconds in [1.25f64, 3.5] {
        let pool = DiarizePool::new(stub_echoing_min_embed(), READY);
        let out = cluster_track(
            &pool,
            MeetingTracks {
                mic_wav: mic,
                system_wav: None,
            },
            MeetingKind::InPerson,
            CosineDistance::new(0.35),
            TargetMode::FullClustering,
            std::time::Duration::from_secs_f64(seconds),
        )
        .expect("the stub answers");
        pool.shutdown();
        assert_eq!(out.len(), 1);
        assert!(
            (out[0].end_seconds - seconds).abs() < 1e-6,
            "the child was sent min_embed_seconds {} but the caller asked for \
             {seconds} — a floor `cluster_track` substituted for the caller's is \
             a default with no measurement behind it",
            out[0].end_seconds
        );
    }

    // …and no such default exists to substitute: the SHIPPING half of
    // `diarize.rs` has no `Duration` constant a caller could be silently given
    // other than the three liveness budgets, none of which describes audio.
    //
    // The scan stops at the unit-test module, and it fails CLOSED if it cannot
    // find where that starts — YV128's lesson, that a source scanner anchored on
    // a substring its own documentation will one day contain scans the wrong
    // half and reports green. The `#[cfg(test)]` module's own `TEST_FLOOR` is a
    // fixture and is exactly what a scan of the whole file would false-positive
    // on.
    let whole = include_str!("../src/diarize.rs");
    let (src, _) = whole
        .split_once("\n#[cfg(test)]\nmod tests {")
        .expect("src/diarize.rs still has a `#[cfg(test)] mod tests` to stop at");
    assert!(
        src.contains("pub fn cluster_track("),
        "the scanned half no longer contains the shipping API — the split landed \
         in the wrong place and this guard is scanning nothing"
    );
    for line in src.lines() {
        let line = line.trim();
        if !line.starts_with("const ") && !line.starts_with("pub const ") {
            continue;
        }
        if !line.contains("Duration") {
            continue;
        }
        assert!(
            line.contains("DEFAULT_REQUEST_DEADLINE")
                || line.contains("READY_BUDGET")
                || line.contains("IDLE_UNLOAD"),
            "a new Duration constant appeared in diarize.rs: `{line}` — if it is \
             a min_embed floor it is a tuned audio threshold with no measurement \
             behind it, and YV122 spent an item making that impossible"
        );
    }
}
