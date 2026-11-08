//! yap-1106-fb-23 — "How many people are in this meeting?" A speaker-count cap.
//!
//! The cap lives in the merge step that runs AFTER the threshold path
//! (`assign_clusters`): while there are more clusters than the count, the
//! closest pair by centroid cosine merges. It never splits a cluster and it
//! never hands sherpa-onnx `num_clusters` a forced exact count (that would
//! split a real speaker when fewer people talk).
//!
//! The always-on tests run over FIXTURE EMBEDDINGS that reproduce the drift a
//! phone band-pass (300–3400 Hz) or a pitch shift causes: speaker D's second
//! half sits far enough from D's first half that the tuned threshold
//! (0.75 cosine distance, `PROVISIONAL_CLUSTER_DISTANCE`) splits them into a
//! fifth "speaker", yet still nearer to D than to anyone else. The real-audio
//! variant is `#[ignore]`d and skips when the diarize models are absent; its
//! fixture comes from `scripts/make-drift-fixture.sh` (licensed clips only).

mod support;

use std::time::Duration;

use wilson_voice_lib::diarize::{assign_clusters, cap_turn_clusters, merge_to_cap, voice_pool};
use wilson_voice_lib::diarize_metrics::CosineDistance;
use wilson_voice_lib::diarize_protocol::DiarizeSegment;

const DIM: usize = 12;
const THRESHOLD: f32 = 0.75;

fn unit(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.iter_mut().for_each(|x| *x /= n);
    v
}

/// A deterministic jitter so turns of one voice are close, not identical.
fn jitter(seed: u64, axis: usize) -> f32 {
    let mut s = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add((axis as u64).wrapping_mul(1442695040888963407));
    s ^= s >> 33;
    ((s % 1000) as f32 / 1000.0 - 0.5) * 0.06
}

fn voice(axis: usize, seed: u64) -> Vec<f32> {
    let mut v: Vec<f32> = (0..DIM).map(|a| jitter(seed, a)).collect();
    v[axis] += 1.0;
    unit(v)
}

/// Speaker D after the tone change: mostly a new direction, a little of D.
/// cos(D, D') ≈ 0.2 (distance ≈ 0.8 > 0.75, so the threshold splits it) and
/// ≈ 0 to everyone else, so D is its nearest voice.
fn drifted(seed: u64) -> Vec<f32> {
    let mut v: Vec<f32> = (0..DIM).map(|a| jitter(seed, a)).collect();
    v[3] += 0.2;
    v[4] += 0.98;
    unit(v)
}

fn seg(i: usize, embedding: Vec<f32>) -> DiarizeSegment {
    DiarizeSegment {
        start: i as f64 * 4.0,
        end: i as f64 * 4.0 + 3.5,
        cluster: 0,
        embedding,
    }
}

/// Four real voices (A, B, C, D), D changes tone halfway: 24 turns.
fn four_voices_one_drifts() -> Vec<DiarizeSegment> {
    let mut out = Vec::new();
    for round in 0..6u64 {
        for axis in 0..4usize {
            let seed = round * 10 + axis as u64;
            let e = if axis == 3 && round >= 3 {
                drifted(seed)
            } else {
                voice(axis, seed)
            };
            out.push(seg(out.len(), e));
        }
    }
    out
}

/// Three voices, nobody drifts.
fn three_voices() -> Vec<DiarizeSegment> {
    let mut out = Vec::new();
    for round in 0..5u64 {
        for axis in 0..3usize {
            out.push(seg(out.len(), voice(axis, round * 10 + axis as u64 + 500)));
        }
    }
    out
}

fn speakers(clusters: &[Option<i64>]) -> usize {
    let mut s: Vec<i64> = clusters.iter().flatten().copied().collect();
    s.sort_unstable();
    s.dedup();
    s.len()
}

fn threshold() -> CosineDistance {
    CosineDistance::new(THRESHOLD)
}

#[test]
fn four_voices_with_a_drift_are_four_with_count_four_and_five_on_auto() {
    let raw = four_voices_one_drifts();
    let auto = assign_clusters(&raw, threshold());
    assert_eq!(speakers(&auto), 5, "the drift alone makes a fifth speaker on Auto");
    assert_eq!(cap_turn_clusters(&raw, &auto, None), auto, "Auto is today's behaviour");
    let capped = cap_turn_clusters(&raw, &auto, voice_pool(Some(4), false));
    assert_eq!(speakers(&capped), 4, "count = 4 yields exactly 4 speakers");
    // The drifted half went to D, not to A, B or C.
    let d_first = capped[3];
    for (i, c) in capped.iter().enumerate() {
        if i % 4 == 3 {
            assert_eq!(*c, d_first, "turn {i}: D's drifted half is D");
        } else {
            assert_ne!(*c, d_first, "turn {i}: nobody else joins D");
        }
    }
}

#[test]
fn every_count_from_one_to_six_is_a_ceiling_and_never_splits_a_cluster() {
    let raw = four_voices_one_drifts();
    let auto = assign_clusters(&raw, threshold());
    for count in 1u8..=6 {
        let capped = cap_turn_clusters(&raw, &auto, voice_pool(Some(count), false));
        let n = speakers(&capped);
        assert!(n <= usize::from(count), "count {count}: {n} speakers");
        assert_eq!(n, speakers(&auto).min(usize::from(count)), "count {count}: merges only as far as needed");
        for i in 0..raw.len() {
            for j in 0..raw.len() {
                if auto[i] == auto[j] {
                    assert_eq!(capped[i], capped[j], "count {count}: turns {i},{j} were split");
                }
            }
        }
    }
}

#[test]
fn three_voices_with_count_four_stay_three() {
    let raw = three_voices();
    let auto = assign_clusters(&raw, threshold());
    assert_eq!(speakers(&auto), 3);
    let capped = cap_turn_clusters(&raw, &auto, voice_pool(Some(4), false));
    assert_eq!(speakers(&capped), 3, "a cap is a ceiling, never a target");
}

#[test]
fn the_pool_is_count_minus_one_when_you_have_the_mic_track() {
    assert_eq!(voice_pool(None, true), None);
    assert_eq!(voice_pool(Some(4), true), Some(3));
    assert_eq!(voice_pool(Some(4), false), Some(4));
    assert_eq!(voice_pool(Some(1), true), Some(1));
}

#[test]
fn merge_to_cap_keeps_the_larger_member_and_merges_voiceless_clusters_last() {
    let a = vec![1.0, 0.0, 0.0];
    let a2 = unit(vec![0.9, 0.3, 0.0]);
    let b = vec![0.0, 0.0, 1.0];
    let reps = merge_to_cap(&[a.clone(), a2, b.clone(), Vec::new()], &[10.0, 30.0, 5.0, 1.0], Some(3));
    assert_eq!(reps[0], 1, "the larger member represents the merged pair");
    assert_eq!(reps[1], 1);
    assert_eq!(reps[2], 2);
    assert_eq!(reps[3], 3, "a voiceless cluster merges only when the count demands it");
    let reps = merge_to_cap(&[a, b, Vec::new()], &[10.0, 5.0, 1.0], Some(1));
    assert!(reps.iter().all(|r| *r == 0), "count 1 is one speaker: {reps:?}");
}

/// The real-audio variant: 4 licensed voices, one band-passed halfway, made by
/// `scripts/make-drift-fixture.sh` into `$YAP_DRIFT_FIXTURE_WAV`.
#[test]
#[ignore = "needs the diarize models and the drift fixture (scripts/make-drift-fixture.sh)"]
fn real_audio_four_voices_with_a_phone_band_drift_are_four_with_count_four() {
    let Some(wav) = std::env::var_os("YAP_DRIFT_FIXTURE_WAV").map(std::path::PathBuf::from) else {
        eprintln!("skip: YAP_DRIFT_FIXTURE_WAV is not set (run scripts/make-drift-fixture.sh)");
        return;
    };
    let embedder = support::diarize::embedder();
    if let Some(reason) = embedder.skip_reason() {
        eprintln!("skip: {reason}");
        return;
    }
    let support::diarize::Embedder::Ready { pool, .. } = embedder else {
        return;
    };
    let raw = pool
        .diarize(&wav, threshold(), Duration::from_secs(2))
        .expect("the drift fixture diarizes");
    let auto = assign_clusters(&raw, threshold());
    let capped = cap_turn_clusters(&raw, &auto, voice_pool(Some(4), false));
    assert_eq!(speakers(&capped), 4, "count = 4 yields 4 on real audio");
    assert!(speakers(&auto) >= 4, "Auto never finds fewer than the cap result");
    pool.shutdown();
}

// ── after the meeting: 5 → 4 re-runs ONLY the merge ──────────────────────────

use wilson_voice_lib::meetings::RecapOutcome;

/// A finished mic-only meeting diarized on Auto into five voices: A, B, C,
/// John (cluster 3, 30 s) and John's phone voice (cluster 4, 10 s). `phone`
/// names cluster 4's line, or leaves it unnamed.
fn five_voice_meeting(tag: &str, phone: Option<&str>) -> (std::path::PathBuf, String) {
    let dir = support::temp_dir(tag);
    let db = support::open_db(&dir);
    let id = support::seed_meeting(&db, &dir, "Standup", &["a", "b", "c", "john", "phone"]);
    let voices: Vec<(i64, Vec<f32>, f64)> = vec![
        (0, voice(0, 1), 12.0),
        (1, voice(1, 2), 11.0),
        (2, voice(2, 3), 9.0),
        (3, voice(3, 4), 30.0),
        (4, drifted(5), 10.0),
    ];
    db.save_meeting_cluster_voices(&id, 0, Some("fixture-digest"), &voices)
        .expect("save voices");
    drop(db);
    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).unwrap();
    conn.execute(
        "UPDATE meeting_segments SET track = 0,
            cluster_index = CAST(round(start_seconds / 4.0) AS INTEGER)
          WHERE meeting_id = ?1",
        rusqlite::params![id],
    )
    .unwrap();
    conn.execute(
        "UPDATE meeting_segments SET speaker_name = 'John' WHERE meeting_id = ?1 AND cluster_index = 3",
        rusqlite::params![id],
    )
    .unwrap();
    if let Some(name) = phone {
        conn.execute(
            "UPDATE meeting_segments SET speaker_name = ?2 WHERE meeting_id = ?1 AND cluster_index = 4",
            rusqlite::params![id, name],
        )
        .unwrap();
    }
    (dir, id)
}

fn lines(dir: &std::path::Path, id: &str) -> Vec<(Option<i64>, Option<String>)> {
    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).unwrap();
    let mut stmt = conn
        .prepare("SELECT cluster_index, speaker_name FROM meeting_segments WHERE meeting_id = ?1 ORDER BY start_seconds")
        .unwrap();
    let rows = stmt
        .query_map(rusqlite::params![id], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    rows
}

#[test]
fn after_the_meeting_five_to_four_merges_without_asr_or_embedding_and_keeps_the_name() {
    let (dir, id) = five_voice_meeting("recap-keep", None);
    let db = support::open_db(&dir);
    let sidecar_before = wilson_voice_lib::diarize::sidecar_requests();
    let out = db.recap_meeting_speakers(&id, Some(4), false).unwrap();
    assert_eq!(out, RecapOutcome::Applied { voices: 4, merged: 1 });
    assert_eq!(
        wilson_voice_lib::diarize::sidecar_requests(),
        sidecar_before,
        "the merge re-runs on stored voices: no diarize/embed request"
    );
    let l = lines(&dir, &id);
    assert_eq!(l[4], (Some(3), Some("John".to_string())), "the phone voice is John now");
    assert_eq!(l[3], (Some(3), Some("John".to_string())));
    assert_eq!(db.meeting_expected_speakers(&id).unwrap(), Some(4));
    assert_eq!(db.meeting_cluster_voices(&id).unwrap().len(), 4);
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn merging_two_named_people_asks_once_and_writes_nothing_until_confirmed() {
    let (dir, id) = five_voice_meeting("recap-ask", Some("Maya"));
    let db = support::open_db(&dir);
    let sidecar_before = wilson_voice_lib::diarize::sidecar_requests();
    let out = db.recap_meeting_speakers(&id, Some(4), false).unwrap();
    assert_eq!(
        out,
        RecapOutcome::NeedsConfirm {
            from: "Maya".into(),
            into: "John".into()
        },
        "\"Merge Maya into John?\" — the larger named voice keeps its name"
    );
    let l = lines(&dir, &id);
    assert_eq!(l[4], (Some(4), Some("Maya".to_string())), "nothing written before the answer");
    assert_eq!(db.meeting_expected_speakers(&id).unwrap(), None);
    let out = db.recap_meeting_speakers(&id, Some(4), true).unwrap();
    assert_eq!(out, RecapOutcome::Applied { voices: 4, merged: 1 });
    let l = lines(&dir, &id);
    assert_eq!(l[4], (Some(3), Some("John".to_string())));
    assert_eq!(wilson_voice_lib::diarize::sidecar_requests(), sidecar_before);
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_higher_count_after_the_meeting_never_splits_anyone() {
    let (dir, id) = five_voice_meeting("recap-up", None);
    let db = support::open_db(&dir);
    db.recap_meeting_speakers(&id, Some(4), false).unwrap();
    let out = db.recap_meeting_speakers(&id, Some(6), false).unwrap();
    assert_eq!(out, RecapOutcome::Applied { voices: 4, merged: 0 });
    assert_eq!(db.meeting_expected_speakers(&id).unwrap(), Some(6));
    assert!(db.set_meeting_expected_speakers(&id, Some(13)).is_err(), "1..=12 only");
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}
