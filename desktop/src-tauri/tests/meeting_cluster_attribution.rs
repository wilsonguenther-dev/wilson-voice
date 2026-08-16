//! YV126 — the write path: an `in_person` meeting comes out of clustering with
//! TWO speakers stored on its transcript rows, not one "Me".
//!
//! ```sh
//! cargo test --test meeting_cluster_attribution
//! ```
//!
//! This is the second half of YV125's manual acceptance criterion ("a synthetic
//! two-speaker `in_person`-kind recording produces two distinct, unnamed speaker
//! clusters on Track A — not one 'Me' label swallowing both voices"). YV125
//! shipped the first half (the branch stops calling the room "Me") and
//! instrumented the gap with an expiring test, because on that base there was no
//! clusterer to produce two clusters with. This file closes it at the mechanism
//! level: real `cluster_track`, real `attribute_clusters`, real migration-5
//! column, real SQLite round trip.
//!
//! The embeddings here come from a stub process on purpose: this file's subject
//! is the WRITE path — which turn's cluster lands on which transcript row, in
//! which column, on which track — and a real model would make every assertion
//! about that path depend on what the model happened to think. The same
//! criterion on real AUDIO is `meeting_eval::fixture_e_der_gate`, which since
//! YV122 (#137) landed does run and does produce a measured DER;
//! `the_sidecar_has_a_real_backend_and_may_not_regress_to_refusing` at the
//! bottom of this file is what keeps that true.

mod support;

use support::diarize_stub::{stub_returning, StubTurn, STUB_MIN_EMBED};
use support::{open_db, temp_dir};
use wilson_voice_lib::diarize::{
    attribute_clusters, cluster_track, rank_and_floor, DiarizePool, DiarizedSegment, MeetingTracks,
    TargetMode,
};
use wilson_voice_lib::diarize_metrics::CosineDistance;
use wilson_voice_lib::meetings::{
    render_transcript, MeetingKind, NewMeetingSegment, MIC_SPEAKER_LABEL, MIC_TRACK, SYSTEM_TRACK,
};

const READY: std::time::Duration = std::time::Duration::from_secs(10);

/// Two voices, alternating, four turns each — the "two people round one
/// microphone" case. One dimension per voice, so the pair is 1.0 apart in
/// cosine distance and no threshold in the plausible range confuses them.
fn two_voices() -> Vec<StubTurn> {
    (0..8u32)
        .map(|i| {
            let voice = (i % 2) as usize;
            let mut embedding = vec![0.0f32; 2];
            embedding[voice] = 1.0;
            let start = i as f64 * 20.0;
            // The child insists everything is cluster 0 — the "one Me swallowed
            // the room" answer. The parent's threshold is what disagrees.
            StubTurn::new(start, start + 18.0, 0, embedding)
        })
        .collect()
}

/// The criterion, end to end: an `in_person` meeting's Track A rows come back
/// carrying two distinct cluster indices.
#[test]
fn an_in_person_meeting_stores_two_distinct_clusters_on_track_a() {
    let dir = temp_dir("cluster-attribution");
    let db = open_db(&dir);
    let meeting = db
        .create_meeting_with_kind(
            "Design review, in the room",
            "manual",
            MeetingKind::InPerson,
        )
        .unwrap();

    // Eight transcript rows on the mic track, one per spoken turn.
    let texts = [
        "let us start with the part everyone said was confusing",
        "i read it twice and the second half still does not follow",
        "then we lead with the example",
        "i would rather shorten it than rewrite it",
        "fine, we shorten it",
        "someone still has to check the numbers",
        "take it, and tell us if anything looks wrong",
        "then we finish here",
    ];
    let rows: Vec<NewMeetingSegment> = texts
        .iter()
        .enumerate()
        .map(|(i, t)| NewMeetingSegment::new(i as f64 * 20.0, i as f64 * 20.0 + 18.0, *t))
        .collect();
    db.append_meeting_segments(&meeting.id, &rows).unwrap();

    let wav = dir.join("mic.wav");
    std::fs::write(&wav, b"RIFF....WAVEfake").unwrap();
    db.finish_meeting(&meeting.id, 160.0, Some(&wav)).unwrap();

    // Cluster the track the kind branch picks, attribute the rows, store it.
    let pool = DiarizePool::new(stub_returning(two_voices()), READY);
    let diarized = cluster_track(
        &pool,
        MeetingTracks {
            mic_wav: &wav,
            system_wav: None,
        },
        MeetingKind::InPerson,
        CosineDistance::new(0.35),
        TargetMode::FullClustering,
        STUB_MIN_EMBED,
    )
    .expect("the stub answers");
    pool.shutdown();

    let stored = db.list_meeting_segments(&meeting.id).unwrap();
    let assignments = attribute_clusters(&stored, &diarized);
    assert_eq!(assignments.len(), 8, "every mic row is attributed");
    assert_eq!(
        db.set_segment_clusters(&meeting.id, &assignments).unwrap(),
        8
    );

    let after = db.list_meeting_segments(&meeting.id).unwrap();
    let clusters: Vec<Option<i64>> = after.iter().map(|s| s.cluster_index).collect();
    assert_eq!(
        clusters,
        vec![
            Some(0),
            Some(1),
            Some(0),
            Some(1),
            Some(0),
            Some(1),
            Some(0),
            Some(1)
        ],
        "two alternating voices, stored as two distinct clusters — the second \
         half of YV125's criterion"
    );

    // …and the transcript still refuses to call any of it "Me", because a
    // microphone in a room is not a person (YV125's first half, unchanged).
    let m = db.get_meeting(&meeting.id).unwrap().unwrap();
    let lines = render_transcript(&after, m.kind());
    assert!(
        lines.iter().all(|l| l.speaker != MIC_SPEAKER_LABEL),
        "an in-person meeting has no 'Me' track"
    );

    // The two clusters are what a surface would show, if this meeting were long
    // enough to clear the floor: 4 turns × 18 s = 72 s each.
    let ranking = rank_and_floor(&diarized);
    assert_eq!(ranking.chips(), vec!["Speaker 1", "Speaker 2"]);
}

/// Attribution is by OVERLAP, not by start time.
///
/// A transcript row's first word routinely lands inside the previous speaker's
/// trailing frames — that is what a turn boundary looks like when two people
/// talk in sequence. "Which turn does this row start inside" mis-attributes
/// every one of those; "which turn does it share the most time with" does not.
#[test]
fn a_row_takes_the_cluster_it_shares_the_most_time_with() {
    let diarized = vec![
        DiarizedSegment::new(MIC_TRACK, 0.0, 10.0, Some(0)),
        DiarizedSegment::new(MIC_TRACK, 10.0, 30.0, Some(1)),
    ];
    let dir = temp_dir("attribution-overlap");
    let db = open_db(&dir);
    let meeting = db
        .create_meeting_with_kind("Overlapping boundary", "manual", MeetingKind::InPerson)
        .unwrap();
    db.append_meeting_segments(
        &meeting.id,
        &[
            // Starts inside cluster 0's turn, but 80% of it is cluster 1's.
            NewMeetingSegment::new(9.0, 29.0, "so as i was saying"),
            // No clustered turn covers this at all.
            NewMeetingSegment::new(60.0, 65.0, "a stray span"),
        ],
    )
    .unwrap();

    let stored = db.list_meeting_segments(&meeting.id).unwrap();
    let assignments = attribute_clusters(&stored, &diarized);
    assert_eq!(
        assignments.iter().map(|(_, c)| *c).collect::<Vec<_>>(),
        vec![Some(1), None],
        "the straddling row belongs to whoever spoke most of it; the row nobody \
         clustered is unattributed rather than filed as speaker 0"
    );
}

/// One pass clusters ONE track. Rows on the other track are not returned at all
/// — not even as `None` — so a virtual meeting's mic rows, which the YV125
/// branch files as "Me" by mechanism rather than by clustering, cannot be
/// cleared by a pass that never looked at them.
#[test]
fn attribution_never_touches_a_track_the_pass_did_not_cluster() {
    let dir = temp_dir("attribution-track-scope");
    let db = open_db(&dir);
    let meeting = db
        .create_meeting_with_kind("Virtual standup", "manual", MeetingKind::Virtual)
        .unwrap();
    db.append_meeting_segments(
        &meeting.id,
        &[
            NewMeetingSegment::new(0.0, 5.0, "morning everyone"),
            NewMeetingSegment::new(5.0, 10.0, "morning, can you hear me").on_track(SYSTEM_TRACK),
            NewMeetingSegment::new(10.0, 15.0, "loud and clear").on_track(SYSTEM_TRACK),
        ],
    )
    .unwrap();

    // A pass over Track B (which is what `MicIsMe` clusters).
    let diarized = vec![
        DiarizedSegment::new(SYSTEM_TRACK, 5.0, 10.0, Some(0)),
        DiarizedSegment::new(SYSTEM_TRACK, 10.0, 15.0, Some(1)),
    ];
    let stored = db.list_meeting_segments(&meeting.id).unwrap();
    let assignments = attribute_clusters(&stored, &diarized);
    assert_eq!(
        assignments.len(),
        2,
        "only the clustered track's rows are in the write set: {assignments:?}"
    );
    db.set_segment_clusters(&meeting.id, &assignments).unwrap();

    let after = db.list_meeting_segments(&meeting.id).unwrap();
    assert_eq!(after[0].track, MIC_TRACK);
    assert!(
        after[0].cluster_index.is_none(),
        "the mic row is 'Me' by mechanism, and this pass never looked at it"
    );
    assert_eq!(after[1].cluster_index, Some(0));
    assert_eq!(after[2].cluster_index, Some(1));

    // Its transcript still says Me/Them, unchanged by clustering the far side.
    let m = db.get_meeting(&meeting.id).unwrap().unwrap();
    let lines = render_transcript(&after, m.kind());
    assert_eq!(lines[0].speaker, MIC_SPEAKER_LABEL);
}

/// **The tripwire that expired, and the guard that replaced it.**
///
/// This test used to assert the OPPOSITE: that `yap-diarize`'s `load_backend`
/// was an unconditional `Err(ERR_NO_BACKEND)`, so that nothing in the tree could
/// turn audio into an embedding and the eval gates in `meeting_eval.rs` had to
/// stay `None`. Its own instructions said how to close it — measure fixture (e),
/// record the tuned threshold and its DER/JER, THEN delete it — and YV122
/// (#137) landing while this branch was open is what made that possible.
/// `ROOM_3_DER_GATE` and `CLASSROOM_6_DER_GATE` now carry measured numbers, so
/// the tripwire is spent.
///
/// What replaces it is the standing version of the same claim: the sidecar has
/// a real backend, and it may not quietly go back to refusing. A regression
/// there would not fail loudly — the eval arms would print a skip and the
/// suite would stay green while measuring nothing, which is the exact posture
/// this file spent two items getting out of.
#[test]
fn the_sidecar_has_a_real_backend_and_may_not_regress_to_refusing() {
    let manifest = include_str!("../../yap-diarize/Cargo.toml");
    assert!(
        manifest
            .lines()
            .any(|l| l.trim_start().starts_with("sherpa-onnx")),
        "yap-diarize no longer depends on sherpa-onnx — without an inference \
         backend every diarization measurement in meeting_eval.rs degrades to a \
         printed skip, and the DER gates would be checking nothing while staying \
         green"
    );

    let sidecar = include_str!("../../yap-diarize/src/main.rs");
    let load = sidecar
        .split("fn load_backend")
        .nth(1)
        .expect("yap-diarize still has a load_backend");
    let body = load.split("\nfn ").next().unwrap_or_default();
    assert!(
        !body.contains("ERR_NO_BACKEND"),
        "`load_backend` refuses with a no-backend tag again. YV122 retired that \
         tag: a machine either has the catalog's two models or it does not, and \
         those are `model_not_found` / `model_load_failed`."
    );
    assert!(
        body.contains("SpeakerEmbeddingExtractor::create"),
        "`load_backend` no longer builds an embedding extractor — the thing that \
         makes a cluster come from audio rather than from a stub"
    );
}
