//! yap-1106-voiceprints — speaker profiles (saved voiceprints), fix for #151
//! and #152.
//!
//! ACCEPTANCE: the match bands with fixture embeddings, the digest mismatch
//! refusing to match, merge, and delete cascading. Every test drives the
//! SHIPPED matcher (`speaker_profiles::match_voice`) and the shipped
//! `Database` lifecycle on a temp DB.
//!
//! The fixture embeddings are exact: a probe at angle θ from a profile's axis
//! has cosine similarity cos θ to its centroid, so each band boundary is hit
//! on purpose, not by luck.

mod support;

use support::{open_db, seed_meeting, temp_dir};
use wilson_voice_lib::diarize_metrics::{cosine_similarity, CosineSimilarity};
use wilson_voice_lib::meeting_asr::SpeakerTurn;
use wilson_voice_lib::speaker_profiles::{
    self, embed_window, embed_window_bounds, match_voice, suggestion_text, MatchBands,
    SpeakerProfile, VoiceMatch, PROVISIONAL_ASK_SIMILARITY, PROVISIONAL_AUTO_SIMILARITY,
};

const DIM: usize = 8;
const DIGEST: &str = "c46fad10b5f81e1aa4a60c162714208577093655076c5450f8c469e522ec54ef";
const OTHER_DIGEST: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// The unit vector on axis `i`.
fn axis(i: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; DIM];
    v[i] = 1.0;
    v
}

/// A unit vector whose cosine similarity to `axis(on)` is exactly `sim`,
/// leaning toward `axis(away)` (orthogonal to both profile axes used below).
fn probe(on: usize, away: usize, sim: f32) -> Vec<f32> {
    let mut v = vec![0.0f32; DIM];
    v[on] = sim;
    v[away] = (1.0 - sim * sim).max(0.0).sqrt();
    v
}

fn profile(id: i64, name: &str, digest: &str, centroid: Vec<f32>) -> SpeakerProfile {
    SpeakerProfile {
        id,
        display_name: name.to_string(),
        is_me: false,
        created_at: "2026-11-07T00:00:00+00:00".into(),
        updated_at: "2026-11-07T00:00:00+00:00".into(),
        embedder_digest: digest.to_string(),
        centroid,
        n_samples: 3,
        last_heard_at: None,
    }
}

fn sim(v: f32) -> CosineSimilarity {
    CosineSimilarity::new(v)
}

#[test]
fn the_provisional_bands_are_the_one_named_070_and_ask_is_below_auto() {
    let b = MatchBands::provisional();
    assert_eq!(b.auto.get(), PROVISIONAL_AUTO_SIMILARITY);
    assert_eq!(b.ask.get(), PROVISIONAL_ASK_SIMILARITY);
    assert_eq!(PROVISIONAL_AUTO_SIMILARITY, 0.70);
    assert!(b.ask.get() < b.auto.get());
    assert!(
        MatchBands::new(sim(0.5), sim(0.6)).is_err(),
        "ask above auto is refused"
    );
}

#[test]
fn match_bands_with_fixture_embeddings_auto_ask_and_unknown() {
    let profiles = vec![
        profile(1, "Alex", DIGEST, axis(0)),
        profile(2, "Blair", DIGEST, axis(1)),
    ];
    let bands = MatchBands::provisional();

    // 0.90 to Alex -> auto.
    let m = match_voice(&probe(0, 3, 0.90), DIGEST, &profiles, bands);
    match &m {
        VoiceMatch::Auto {
            profile_id,
            display_name,
            similarity,
        } => {
            assert_eq!(*profile_id, 1);
            assert_eq!(display_name, "Alex");
            assert!((similarity.get() - 0.90).abs() < 1e-4);
        }
        other => panic!("0.90 must be auto, got {other:?}"),
    }
    assert_eq!(
        suggestion_text(&m).unwrap(),
        "Probably Alex — voice match 0.90"
    );

    // Just over the auto band is auto (accept at or above).
    let at_auto = match_voice(
        &probe(1, 3, PROVISIONAL_AUTO_SIMILARITY + 0.005),
        DIGEST,
        &profiles,
        bands,
    );
    assert_eq!(at_auto.outcome(), "auto", "{at_auto:?}");

    // 0.62 to Blair -> ask.
    let m = match_voice(&probe(1, 4, 0.62), DIGEST, &profiles, bands);
    match &m {
        VoiceMatch::Ask {
            profile_id,
            display_name,
            ..
        } => {
            assert_eq!(*profile_id, 2);
            assert_eq!(display_name, "Blair");
        }
        other => panic!("0.62 must be ask, got {other:?}"),
    }
    assert_eq!(
        suggestion_text(&m).unwrap(),
        "Is this Blair? Voice match 0.62"
    );

    // 0.30 -> a new unknown, carrying its best score as the reason.
    let m = match_voice(&probe(0, 5, 0.30), DIGEST, &profiles, bands);
    match m {
        VoiceMatch::Unknown { best: Some(b) } => assert!((b.get() - 0.30).abs() < 1e-4),
        other => panic!("0.30 must be unknown, got {other:?}"),
    }
    assert!(suggestion_text(&VoiceMatch::Unknown { best: None }).is_none());

    // Nobody enrolled -> unknown with no score.
    assert_eq!(
        match_voice(&axis(0), DIGEST, &[], bands),
        VoiceMatch::Unknown { best: None }
    );
}

#[test]
fn the_digest_mismatch_refuses_to_match() {
    // A perfect voice match from ANOTHER embedding model is never compared.
    let stale = vec![profile(7, "Alex", OTHER_DIGEST, axis(0))];
    let m = match_voice(&axis(0), DIGEST, &stale, MatchBands::provisional());
    assert_eq!(m, VoiceMatch::ReEnrollNeeded { stale_profiles: 1 });
    assert!(suggestion_text(&m).unwrap().contains("enrolled again"));

    // Mixed: the stale profile is skipped even though it would score 1.0; the
    // current-digest one decides.
    let mixed = vec![
        profile(7, "Alex (old model)", OTHER_DIGEST, axis(0)),
        profile(8, "Blair", DIGEST, axis(1)),
    ];
    let m = match_voice(&axis(0), DIGEST, &mixed, MatchBands::provisional());
    assert_eq!(m.outcome(), "unknown", "{m:?}");

    // A different dimension is a mismatch too, never a truncation.
    let short = vec![profile(9, "Casey", DIGEST, vec![1.0, 0.0, 0.0])];
    let m = match_voice(&axis(0), DIGEST, &short, MatchBands::provisional());
    assert_eq!(m, VoiceMatch::ReEnrollNeeded { stale_profiles: 1 });

    // Through the DB: a sample from another model is refused, and so is a
    // merge across models.
    let dir = temp_dir("voiceprint-digest");
    let db = open_db(&dir);
    let a = db
        .create_speaker_profile("Alex", false, DIGEST, &axis(0), 2.0, None)
        .unwrap();
    let err = db
        .add_speaker_sample(a.id, OTHER_DIGEST, &axis(0), 2.0, None)
        .unwrap_err();
    assert!(err.contains("Enroll it again"), "{err}");
    assert_eq!(db.speaker_sample_count(a.id).unwrap(), 1);
    let b = db
        .create_speaker_profile("Alex again", false, OTHER_DIGEST, &axis(0), 2.0, None)
        .unwrap();
    assert!(db.merge_speaker_profiles(a.id, b.id).is_err());
    let m = db
        .match_speaker_voice(&axis(0), DIGEST, MatchBands::provisional())
        .unwrap();
    match m {
        VoiceMatch::Auto { profile_id, .. } => assert_eq!(profile_id, a.id),
        other => panic!("only the current-digest profile may match: {other:?}"),
    }
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn merge_moves_samples_weights_the_centroid_and_renames_labelled_segments() {
    let dir = temp_dir("voiceprint-merge");
    let db = open_db(&dir);
    let meeting = seed_meeting(&db, &dir, "Standup", &["hi", "hello", "yo"]);
    let segs = db.list_meeting_segments(&meeting).unwrap();

    let keep = db
        .create_speaker_profile("Alex", false, DIGEST, &axis(0), 2.0, Some(meeting.as_str()))
        .unwrap();
    db.add_speaker_sample(keep.id, DIGEST, &axis(0), 2.0, Some(meeting.as_str()))
        .unwrap();
    db.add_speaker_sample(keep.id, DIGEST, &axis(0), 2.0, None)
        .unwrap();
    let absorb = db
        .create_speaker_profile(
            "Speaker 2",
            true,
            DIGEST,
            &axis(1),
            2.0,
            Some(meeting.as_str()),
        )
        .unwrap();
    db.label_segment_speaker(&segs[0].id, keep.id).unwrap();
    db.label_segment_speaker(&segs[1].id, absorb.id).unwrap();

    let merged = db.merge_speaker_profiles(keep.id, absorb.id).unwrap();
    assert_eq!(merged.id, keep.id);
    assert_eq!(merged.display_name, "Alex");
    assert_eq!(merged.n_samples, 4);
    assert!(merged.is_me, "merging the enrolled You keeps is_me");
    assert_eq!(db.speaker_sample_count(keep.id).unwrap(), 4);
    assert_eq!(db.speaker_sample_count(absorb.id).unwrap(), 0);
    assert!(db.get_speaker_profile(absorb.id).unwrap().is_none());

    // Weighted 3:1 toward axis 0.
    let expected = {
        let n = (9.0f32 + 1.0).sqrt();
        let mut v = vec![0.0f32; DIM];
        v[0] = 3.0 / n;
        v[1] = 1.0 / n;
        v
    };
    assert!(cosine_similarity(&merged.centroid, &expected).get() > 0.9999);

    assert_eq!(
        db.segment_speaker(&segs[1].id).unwrap(),
        (Some(keep.id), Some("Alex".to_string()))
    );
    assert!(db.merge_speaker_profiles(keep.id, keep.id).is_err());

    // Rename follows the labelled segments.
    db.rename_speaker_profile(keep.id, "  Alex Kim  ").unwrap();
    assert_eq!(
        db.segment_speaker(&segs[0].id).unwrap(),
        (Some(keep.id), Some("Alex Kim".to_string()))
    );
    assert!(db.rename_speaker_profile(keep.id, "   ").is_err());

    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_cascades_to_samples_and_past_meetings_keep_the_name() {
    let dir = temp_dir("voiceprint-delete");
    let db = open_db(&dir);
    let meeting = seed_meeting(&db, &dir, "1:1", &["one", "two"]);
    let segs = db.list_meeting_segments(&meeting).unwrap();

    let p = db
        .create_speaker_profile("Alex", false, DIGEST, &axis(0), 2.0, Some(meeting.as_str()))
        .unwrap();
    db.add_speaker_sample(
        p.id,
        DIGEST,
        &probe(0, 2, 0.95),
        3.0,
        Some(meeting.as_str()),
    )
    .unwrap();
    db.label_segment_speaker(&segs[0].id, p.id).unwrap();
    assert_eq!(db.speaker_sample_count(p.id).unwrap(), 2);

    assert!(db.delete_speaker_profile(p.id).unwrap());
    assert!(db.get_speaker_profile(p.id).unwrap().is_none());
    assert_eq!(db.speaker_sample_count(p.id).unwrap(), 0, "samples go too");
    assert_eq!(
        db.segment_speaker(&segs[0].id).unwrap(),
        (None, Some("Alex".to_string())),
        "the past meeting keeps the name as plain text"
    );
    assert!(
        !db.delete_speaker_profile(p.id).unwrap(),
        "second delete is a no-op"
    );

    // The cascade is also enforced by the schema: a raw DELETE of the profile
    // row leaves no orphaned sample.
    let q = db
        .create_speaker_profile("Blair", false, DIGEST, &axis(1), 2.0, None)
        .unwrap();
    drop(db);
    {
        let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute("DELETE FROM speaker_profiles WHERE id = ?1", [q.id])
            .unwrap();
        let orphans: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM speaker_samples WHERE profile_id = ?1",
                [q.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(orphans, 0);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn forget_all_voiceprints_removes_every_profile_and_clears_the_you_answer() {
    let dir = temp_dir("voiceprint-forget");
    let db = open_db(&dir);
    let meeting = seed_meeting(&db, &dir, "Call", &["hey"]);
    let segs = db.list_meeting_segments(&meeting).unwrap();
    assert_eq!(db.enroll_me_choice(), None, "never asked");
    db.set_enroll_me_choice(true).unwrap();
    assert_eq!(db.enroll_me_choice(), Some(true));

    let me = db
        .create_speaker_profile("You", true, DIGEST, &axis(0), 4.0, None)
        .unwrap();
    assert!(
        db.create_speaker_profile("You again", true, DIGEST, &axis(0), 4.0, None)
            .is_err(),
        "there is one You"
    );
    let other = db
        .create_speaker_profile("Alex", false, DIGEST, &axis(1), 2.0, Some(meeting.as_str()))
        .unwrap();
    db.label_segment_speaker(&segs[0].id, other.id).unwrap();

    assert_eq!(db.forget_all_voiceprints().unwrap(), 2);
    assert!(db.list_speaker_profiles().unwrap().is_empty());
    assert_eq!(db.speaker_sample_count(me.id).unwrap(), 0);
    assert_eq!(db.speaker_sample_count(other.id).unwrap(), 0);
    assert_eq!(
        db.segment_speaker(&segs[0].id).unwrap(),
        (None, Some("Alex".to_string()))
    );
    assert_eq!(db.enroll_me_choice(), None);
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bands_tune_on_a_scored_population() {
    let genuine: Vec<CosineSimilarity> = [0.92, 0.88, 0.85, 0.81, 0.78, 0.74]
        .iter()
        .map(|v| sim(*v))
        .collect();
    let impostor: Vec<CosineSimilarity> = [0.20, 0.31, 0.40, 0.48, 0.52, 0.60]
        .iter()
        .map(|v| sim(*v))
        .collect();
    let b = MatchBands::tuned(&genuine, &impostor, 0.0);
    assert!(b.ask.get() <= b.auto.get());
    assert!(
        b.auto.get() > 0.60,
        "no impostor reaches auto at FAR 0: {b:?}"
    );
    assert!(
        b.ask.get() > 0.60 && b.ask.get() <= 0.74,
        "EER sits between the clusters: {b:?}"
    );
}

#[test]
fn only_single_speaker_turns_are_embedded_and_boundaries_are_skipped() {
    let attributed = SpeakerTurn {
        track: 0,
        cluster_index: Some(1),
        start_seconds: 10.0,
        end_seconds: 13.0,
        words: 0..5,
        text: String::new(),
    };
    let (s, e) = embed_window(&attributed, 1.0).expect("a 3 s turn is embedded");
    assert!((s - 10.2).abs() < 1e-9 && (e - 12.8).abs() < 1e-9);
    assert!(
        embed_window(&attributed, 2.7).is_none(),
        "too short after the trim"
    );

    let unattributed = SpeakerTurn {
        cluster_index: None,
        ..attributed.clone()
    };
    assert!(embed_window(&unattributed, 0.0).is_none());
    assert!(embed_window_bounds(1.0, 1.3, 0.0).is_none());
}

#[test]
fn a_voiceprint_stores_the_pinned_embedding_model_digest() {
    assert_eq!(
        speaker_profiles::pinned_embedder_digest().as_deref(),
        Some(DIGEST),
        "embedder_digest = the catalog's pinned embedding sha256 (ANALYZER-RESEARCH.md)"
    );
}

#[test]
fn centroid_blobs_round_trip_and_the_webview_never_sees_a_voiceprint() {
    let v = probe(0, 1, 0.5);
    let back = speaker_profiles::decode_embedding(&speaker_profiles::encode_embedding(&v)).unwrap();
    assert_eq!(v, back);
    assert!(speaker_profiles::decode_embedding(&[0u8; 5]).is_err());
    let json = serde_json::to_value(profile(1, "Alex", DIGEST, axis(0))).unwrap();
    assert!(json.get("centroid").is_none(), "{json}");
    assert_eq!(json["displayName"], "Alex");
    assert_eq!(speaker_profiles::LEAVES_THIS_MAC, "Leaves this Mac: No");
}
