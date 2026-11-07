//! yap-1106-auto-label-and-correct — known voices are labelled automatically,
//! and any label can be corrected.
//!
//! ACCEPTANCE: the relabel / merge / split reducers are unit-tested next to
//! them (`speaker_profiles::correction_tests`); this file drives the SHIPPED
//! `Database` path on a temp DB: the export reflects the corrections, undo
//! asks the lock (YV130), corrections feed the voiceprint as positive and
//! negative samples, and a future meeting is auto-labelled with the name.

mod support;

use support::{open_db, seed_meeting, temp_dir};
use wilson_voice_lib::speaker_profiles::{MatchBands, NameTarget, SpeakerCorrection};

const DIGEST: &str = "c46fad10b5f81e1aa4a60c162714208577093655076c5450f8c469e522ec54ef";

fn voice_a() -> Vec<f32> {
    vec![1.0, 0.0, 0.0, 0.0]
}

fn voice_b() -> Vec<f32> {
    vec![0.0, 1.0, 0.0, 0.0]
}

/// A meeting whose lines alternate A (cluster 0) / B (cluster 1), with each
/// cluster's voice kept the way the diarization pass keeps it.
fn two_voice_meeting(
    db: &wilson_voice_lib::db::Database,
    dir: &std::path::Path,
) -> (String, Vec<String>) {
    let id = seed_meeting(
        db,
        dir,
        "Planning",
        &["a one", "b one", "a two", "b two", "a three"],
    );
    let ids: Vec<String> = db
        .list_meeting_segments(&id)
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    let clusters: Vec<(String, Option<i64>)> = ids
        .iter()
        .enumerate()
        .map(|(i, s)| (s.clone(), Some((i % 2) as i64)))
        .collect();
    db.set_segment_clusters(&id, &clusters).unwrap();
    db.save_meeting_cluster_voices(
        &id,
        0,
        Some(DIGEST),
        &[(0, voice_a(), 30.0), (1, voice_b(), 25.0)],
    )
    .unwrap();
    (id, ids)
}

fn rename(cluster: i64, name: &str) -> SpeakerCorrection {
    SpeakerCorrection::Rename {
        track: 0,
        cluster_index: cluster,
        target: NameTarget::NewName { name: name.into() },
    }
}

fn markdown(db: &wilson_voice_lib::db::Database, id: &str) -> String {
    db.meeting_markdown(id).unwrap().1
}

#[test]
fn the_export_reflects_a_rename_and_its_undo() {
    let dir = temp_dir("correct-export");
    let db = open_db(&dir);
    let (id, _) = two_voice_meeting(&db, &dir);
    let before = markdown(&db, &id);
    assert!(!before.contains("Alex"));

    let applied = db
        .correct_meeting_speakers(&id, &rename(0, "Alex"))
        .unwrap();
    assert_eq!(
        applied.rows.len(),
        3,
        "every line of the speaker, whole turns"
    );
    let md = markdown(&db, &id);
    for text in ["a one", "a two", "a three"] {
        assert!(md.contains(&format!("Alex:** {text}")), "{md}");
    }
    assert!(
        !md.contains("Alex:** b one"),
        "the other speaker keeps its label"
    );

    let undone = db.undo_speaker_correction(&id, &applied.undo).unwrap();
    assert_eq!(undone.rows.len(), 3);
    assert!(undone.skipped_changed.is_empty());
    assert_eq!(
        markdown(&db, &id),
        before,
        "undo puts the export back exactly"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_export_reflects_merge_and_split() {
    let dir = temp_dir("correct-merge-split");
    let db = open_db(&dir);
    let (id, ids) = two_voice_meeting(&db, &dir);
    db.correct_meeting_speakers(&id, &rename(0, "Alex"))
        .unwrap();
    db.correct_meeting_speakers(
        &id,
        &SpeakerCorrection::Merge {
            track: 0,
            from_cluster: 1,
            into_cluster: 0,
        },
    )
    .unwrap();
    let md = markdown(&db, &id);
    assert!(
        md.contains("Alex:** b one") && md.contains("Alex:** b two"),
        "{md}"
    );

    db.correct_meeting_speakers(
        &id,
        &SpeakerCorrection::Split {
            track: 0,
            cluster_index: 0,
            segment_ids: vec![ids[4].clone()],
        },
    )
    .unwrap();
    let md = markdown(&db, &id);
    assert!(
        !md.contains("Alex:** a three"),
        "the split line left Alex: {md}"
    );
    assert!(md.contains("Alex:** a two"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn undo_asks_the_lock_not_only_the_value() {
    let dir = temp_dir("correct-undo-lock");
    let db = open_db(&dir);
    let (id, _) = two_voice_meeting(&db, &dir);
    let first = db
        .correct_meeting_speakers(&id, &rename(0, "Alex"))
        .unwrap();
    // A later correction writes the SAME name: only the lock tells them apart.
    db.correct_meeting_speakers(&id, &rename(0, "Alex"))
        .unwrap();
    let undone = db.undo_speaker_correction(&id, &first.undo).unwrap();
    assert!(undone.rows.is_empty());
    assert_eq!(undone.skipped_changed.len(), 3);
    assert!(
        markdown(&db, &id).contains("Alex:** a one"),
        "the later decision stands"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_expired_undo_is_refused() {
    let dir = temp_dir("correct-undo-expired");
    let db = open_db(&dir);
    let (id, _) = two_voice_meeting(&db, &dir);
    let mut applied = db
        .correct_meeting_speakers(&id, &rename(0, "Alex"))
        .unwrap();
    applied.undo.expires_at_ms -= 60_000;
    assert!(db.undo_speaker_correction(&id, &applied.undo).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn corrections_feed_samples_and_future_meetings_are_auto_labelled() {
    let dir = temp_dir("correct-auto-label");
    let db = open_db(&dir);
    // Meeting 1: a person names A "Alex" — a positive sample.
    let (m1, _) = two_voice_meeting(&db, &dir);
    let applied = db
        .correct_meeting_speakers(&m1, &rename(0, "Alex"))
        .unwrap();
    assert_eq!(applied.undo.positive_sample_ids.len(), 1);
    let alex = applied.rows[0].profile_id.unwrap();
    assert_eq!(db.speaker_sample_count(alex).unwrap(), 1);

    // Meeting 2: the same voice is labelled on its own; B stays "Speaker N".
    let (m2, _) = two_voice_meeting(&db, &dir);
    let labelled = db
        .auto_label_meeting(&m2, MatchBands::provisional())
        .unwrap();
    assert_eq!(labelled, 3);
    let md = markdown(&db, &m2);
    assert!(
        md.contains("Alex:** a one") && !md.contains("Alex:** b one"),
        "{md}"
    );

    // "This wasn't Alex" on meeting 2 — a negative sample for Alex, a positive for Kim.
    let wrong = db.correct_meeting_speakers(&m2, &rename(0, "Kim")).unwrap();
    assert_eq!(wrong.undo.negative_sample_ids.len(), 1);
    assert_eq!(wrong.undo.positive_sample_ids.len(), 1);
    assert!(markdown(&db, &m2).contains("Kim:** a one"));

    // Meeting 3: the negative now sits as close to the voice as Alex's own
    // centroid, so the match is not trusted and the voice is not labelled Alex.
    let (m3, _) = two_voice_meeting(&db, &dir);
    db.auto_label_meeting(&m3, MatchBands::provisional())
        .unwrap();
    assert!(!markdown(&db, &m3).contains("Alex:**"));

    // A locked (corrected) line is never overwritten by auto-labelling.
    db.auto_label_meeting(&m2, MatchBands::provisional())
        .unwrap();
    assert!(markdown(&db, &m2).contains("Kim:** a one"));
    let _ = std::fs::remove_dir_all(&dir);
}
