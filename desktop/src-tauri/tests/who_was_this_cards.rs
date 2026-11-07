//! yap-1106-fb-22b-who-was-this-cards — the "Who was this?" cards after the
//! meeting, the default (approved design "Meetings: who said this?" §B, §C).
//!
//! ACCEPTANCE pinned here, every test on the shipped `Database` over a temp DB
//! (or the shipped pure picker):
//! - a 5-speaker meeting (you + 4 others) gives at most 5 cards and exactly 4:
//!   you are never carded;
//! - naming one card relabels every turn of that voice and adds ONE sample;
//! - a second meeting with the same (now named) voices gives zero cards;
//! - 8 new voices give exactly 5 cards, most speaking time first;
//! - a voice at 62% to a profile gives a "Same person as John?" card and
//!   "Yes, John" increments John's n_samples;
//! - "This wasn't John" drops what John's voiceprint learned in this meeting;
//! - clips are 3.0 s ± 50 ms and never cross a turn boundary;
//! - Settings → Voices: Delete removes a profile and its samples, "Forget all
//!   voices" empties both tables.
//!
//! Plus an #[ignore]d variant on real audio when the diarize models are present.

mod support;

#[path = "support/diarize_models.rs"]
mod models;

use support::{open_db, seed_meeting, temp_dir};
use wilson_voice_lib::db::Database;
use wilson_voice_lib::diarize_protocol::DiarizeRequest;
use wilson_voice_lib::meetings::MeetingSegment;
use wilson_voice_lib::speaker_profiles::{
    clearest_clip, ClusterLine, MatchBands, NameTarget, SpeakerCorrection, MAX_VOICE_CARDS,
    PROVISIONAL_AUTO_SIMILARITY, VOICE_CLIP_SECONDS,
};

const DIM: usize = 16;
const DIGEST: &str = "c46fad10b5f81e1aa4a60c162714208577093655076c5450f8c469e522ec54ef";

fn axis(i: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; DIM];
    v[i] = 1.0;
    v
}

/// A finished meeting whose voice `k` speaks `turns[k]` turns (3.5 s each,
/// round-robin, so every turn is one word-exact single-speaker row), with the
/// cluster voices stored as the diarize pass would store them.
fn meeting_with_voices(
    db: &Database,
    dir: &std::path::Path,
    title: &str,
    turns: &[usize],
    voices: &[Vec<f32>],
) -> (String, i64, Vec<MeetingSegment>) {
    let mut left: Vec<usize> = turns.to_vec();
    let mut order: Vec<i64> = Vec::new();
    while left.iter().any(|n| *n > 0) {
        for (k, n) in left.iter_mut().enumerate() {
            if *n > 0 {
                *n -= 1;
                order.push(k as i64);
            }
        }
    }
    let texts: Vec<String> = order
        .iter()
        .enumerate()
        .map(|(i, k)| format!("voice {k} line {i}"))
        .collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let meeting = seed_meeting(db, dir, title, &refs);
    let mut segs = db.list_meeting_segments(&meeting).unwrap();
    segs.sort_by(|a, b| a.start_seconds.partial_cmp(&b.start_seconds).unwrap());
    assert_eq!(segs.len(), order.len());
    let assignments: Vec<(String, Option<i64>)> = segs
        .iter()
        .zip(&order)
        .map(|(s, k)| (s.id.clone(), Some(*k)))
        .collect();
    db.set_segment_clusters(&meeting, &assignments).unwrap();
    let track = segs[0].track;
    let stored: Vec<(i64, Vec<f32>, f64)> = voices
        .iter()
        .enumerate()
        .map(|(k, v)| (k as i64, v.clone(), turns[k] as f64 * 3.5))
        .collect();
    db.save_meeting_cluster_voices(&meeting, track, Some(DIGEST), &stored)
        .unwrap();
    let segs = db.list_meeting_segments(&meeting).unwrap();
    (meeting, track, segs)
}

fn enroll_me(db: &Database) -> i64 {
    db.create_speaker_profile("You", true, DIGEST, &axis(0), 30.0, None)
        .unwrap()
        .id
}

#[test]
fn five_speakers_give_four_cards_and_you_are_never_carded() {
    let dir = temp_dir("fb22b-five");
    let db = open_db(&dir);
    enroll_me(&db);
    let voices: Vec<Vec<f32>> = (0..5).map(axis).collect();
    let (meeting, track, segs) =
        meeting_with_voices(&db, &dir, "Launch sync", &[6, 6, 6, 6, 6], &voices);
    let cards = db
        .meeting_voice_cards(&meeting, MatchBands::provisional())
        .unwrap();
    assert!(cards.len() <= MAX_VOICE_CARDS);
    assert_eq!(cards.len(), 4, "you + 4 others: exactly the 4 others");
    assert!(
        cards.iter().all(|c| c.cluster_index != 0),
        "you are never asked about yourself"
    );
    assert!(cards.iter().all(|c| c.suggestion.is_none()));

    // Naming one card relabels EVERY turn of that voice at once and adds one sample.
    let named = db
        .name_meeting_cluster(
            &meeting,
            track,
            1,
            &NameTarget::NewName {
                name: "John".into(),
            },
        )
        .unwrap();
    assert_eq!(named.relabelled, 6, "John ✓ · 6 lines relabeled");
    assert!(named.sample_added);
    assert_eq!(db.speaker_sample_count(named.profile_id).unwrap(), 1);
    assert_eq!(
        db.get_speaker_profile(named.profile_id)
            .unwrap()
            .unwrap()
            .n_samples,
        1
    );
    for s in segs.iter().filter(|s| s.cluster_index == Some(1)) {
        assert_eq!(
            db.segment_speaker(&s.id).unwrap().1.as_deref(),
            Some("John")
        );
    }
    let after = db
        .meeting_voice_cards(&meeting, MatchBands::provisional())
        .unwrap();
    assert_eq!(after.len(), 3, "the named voice leaves the cards");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_second_meeting_with_the_same_voices_gives_zero_cards() {
    let dir = temp_dir("fb22b-second");
    let db = open_db(&dir);
    enroll_me(&db);
    let voices: Vec<Vec<f32>> = (0..5).map(axis).collect();
    let (m1, track, _) = meeting_with_voices(&db, &dir, "Monday", &[6, 6, 6, 6, 6], &voices);
    for (k, name) in [(1, "John"), (2, "Ana"), (3, "Ben"), (4, "Cy")] {
        db.name_meeting_cluster(&m1, track, k, &NameTarget::NewName { name: name.into() })
            .unwrap();
    }
    let (m2, _, _) = meeting_with_voices(&db, &dir, "Tuesday", &[6, 7, 8, 6, 9], &voices);
    let cards = db
        .meeting_voice_cards(&m2, MatchBands::provisional())
        .unwrap();
    assert!(
        cards.is_empty(),
        "every voice matches a saved profile at {PROVISIONAL_AUTO_SIMILARITY} or more: {cards:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn eight_new_voices_give_exactly_five_cards_by_speaking_time() {
    let dir = temp_dir("fb22b-eight");
    let db = open_db(&dir);
    enroll_me(&db);
    // Voices 1..=8 (none is you); voice k speaks 5 + k turns.
    let voices: Vec<Vec<f32>> = (1..=8).map(axis).collect();
    let turns: Vec<usize> = (1..=8).map(|k| 5 + k).collect();
    let (meeting, _, _) = meeting_with_voices(&db, &dir, "All hands", &turns, &voices);
    let cards = db
        .meeting_voice_cards(&meeting, MatchBands::provisional())
        .unwrap();
    assert_eq!(cards.len(), MAX_VOICE_CARDS);
    let order: Vec<i64> = cards.iter().map(|c| c.cluster_index).collect();
    assert_eq!(order, vec![7, 6, 5, 4, 3], "most speaking time first");
    assert!(cards.windows(2).all(|w| w[0].seconds >= w[1].seconds));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_voice_at_62_percent_asks_same_person_and_yes_adds_a_sample() {
    let dir = temp_dir("fb22b-62");
    let db = open_db(&dir);
    enroll_me(&db);
    let john = db
        .create_speaker_profile("John", false, DIGEST, &axis(1), 20.0, None)
        .unwrap();
    let before = john.n_samples;
    let mut v = vec![0.0f32; DIM];
    v[1] = 0.62;
    v[2] = (1.0f32 - 0.62 * 0.62).sqrt();
    let (meeting, track, _) =
        meeting_with_voices(&db, &dir, "Design review", &[6, 6], &[axis(0), v]);
    let cards = db
        .meeting_voice_cards(&meeting, MatchBands::provisional())
        .unwrap();
    assert_eq!(cards.len(), 1);
    let s = cards[0]
        .suggestion
        .as_ref()
        .expect("\"Same person as John? 62% sure\"");
    assert_eq!(s.profile_id, john.id);
    assert_eq!(s.display_name, "John");
    assert!(
        (s.similarity.get() - 0.62).abs() < 0.01,
        "{}",
        s.similarity.get()
    );
    // "Yes, John": the new sound joins John's voiceprint.
    let yes = db
        .name_meeting_cluster(
            &meeting,
            track,
            cards[0].cluster_index,
            &NameTarget::Profile {
                profile_id: john.id,
            },
        )
        .unwrap();
    assert!(yes.sample_added);
    let after = db.get_speaker_profile(john.id).unwrap().unwrap();
    assert_eq!(after.n_samples, before + 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn this_wasnt_john_drops_what_johns_voiceprint_learned_in_this_meeting() {
    let dir = temp_dir("fb22b-wasnt");
    let db = open_db(&dir);
    let (meeting, track, _) = meeting_with_voices(&db, &dir, "Retro", &[6, 6], &[axis(3), axis(4)]);
    let john = db
        .name_meeting_cluster(
            &meeting,
            track,
            0,
            &NameTarget::NewName {
                name: "John".into(),
            },
        )
        .unwrap();
    assert_eq!(db.speaker_sample_count(john.profile_id).unwrap(), 1);
    let wrong = db
        .correct_meeting_speakers(
            &meeting,
            &SpeakerCorrection::Rename {
                track,
                cluster_index: 0,
                target: NameTarget::NewName { name: "Kim".into() },
            },
        )
        .unwrap();
    assert_eq!(db.speaker_sample_count(john.profile_id).unwrap(), 0);
    assert_eq!(
        db.get_speaker_profile(john.profile_id)
            .unwrap()
            .unwrap()
            .n_samples,
        0,
        "John's voiceprint forgot the wrong sound"
    );
    assert_eq!(wrong.undo.unlearned.len(), 1);
    assert_eq!(wrong.undo.unlearned[0].samples, 1);
    // Undo puts it back.
    db.undo_speaker_correction(&meeting, &wrong.undo).unwrap();
    assert_eq!(db.speaker_sample_count(john.profile_id).unwrap(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_removes_the_profile_and_its_samples_and_forget_all_empties_both() {
    let dir = temp_dir("fb22b-voices");
    let db = open_db(&dir);
    let (meeting, track, _) = meeting_with_voices(
        &db,
        &dir,
        "Voices",
        &[6, 6, 6],
        &[axis(5), axis(6), axis(7)],
    );
    let mut ids = Vec::new();
    for (k, name) in [(0, "Ana"), (1, "Ben"), (2, "Cy")] {
        ids.push(
            db.name_meeting_cluster(
                &meeting,
                track,
                k,
                &NameTarget::NewName { name: name.into() },
            )
            .unwrap()
            .profile_id,
        );
    }
    assert!(ids
        .iter()
        .all(|id| db.speaker_sample_count(*id).unwrap() == 1));
    assert!(db.delete_speaker_profile(ids[0]).unwrap());
    assert!(db.get_speaker_profile(ids[0]).unwrap().is_none());
    assert_eq!(
        db.speaker_sample_count(ids[0]).unwrap(),
        0,
        "Delete cascades to the samples"
    );
    assert_eq!(db.list_speaker_profiles().unwrap().len(), 2);
    assert_eq!(db.forget_all_voiceprints().unwrap(), 2);
    assert!(db.list_speaker_profiles().unwrap().is_empty());
    for id in &ids {
        assert_eq!(db.speaker_sample_count(*id).unwrap(), 0);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn xorshift(state: &mut u64) -> f64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state % 10_000) as f64 / 1_000.0
}

#[test]
fn clips_are_three_seconds_and_never_cross_a_turn_boundary() {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    for round in 0..500 {
        let n = 1 + (round % 7);
        let mut at = 0.0;
        let mut turns = Vec::new();
        for i in 0..n {
            let len = 0.2 + xorshift(&mut seed);
            let gap = 0.1 + xorshift(&mut seed) / 4.0;
            turns.push(ClusterLine {
                segment_id: format!("t{i}"),
                start_seconds: at,
                end_seconds: at + len,
                text: String::new(),
                labelled: false,
            });
            at += len + gap;
        }
        let (start, end, i) = clearest_clip(&turns).unwrap();
        let turn = &turns[i];
        assert!(
            start >= turn.start_seconds && end <= turn.end_seconds + 1e-9,
            "the clip stays inside one turn (round {round})"
        );
        let longest = turns
            .iter()
            .map(|t| t.end_seconds - t.start_seconds)
            .fold(0.0f64, f64::max);
        if longest >= VOICE_CLIP_SECONDS {
            assert!(
                ((end - start) - VOICE_CLIP_SECONDS).abs() <= 0.05,
                "3.0 s ± 50 ms, got {} (round {round})",
                end - start
            );
        } else {
            assert!(end - start <= VOICE_CLIP_SECONDS);
        }
    }
}

/// The same 5-speaker check on real audio: five macOS voices embedded by the
/// shipped diarize sidecar; "you" is enrolled from a different utterance.
#[test]
#[ignore = "needs the diarize models (~/yap-diarize-models) and macOS `say`"]
fn five_real_voices_never_card_you() {
    let Some((segmentation, embedding_model)) = models::models() else {
        return;
    };
    const VOICES: [&str; 5] = ["Samantha", "Daniel", "Karen", "Rishi", "Fred"];
    const LINES: [&str; 2] = [
        "The quarterly numbers came in this morning and the growth line finally crossed the mark we set in January.",
        "Let me pull up the dashboard and walk through the retention curve with everyone who is on this call today.",
    ];
    let scratch = models::scratch("fb22b-real");
    models::assert_voices_are_distinct(&scratch, &VOICES);
    let mut sidecar = models::Spawned::start();
    sidecar
        .ask(&DiarizeRequest::load_models(
            1,
            &segmentation,
            &embedding_model,
        ))
        .into_embedding_dim()
        .expect("both models load");
    let mut id = 2u64;
    let mut embed = |voice: &str, n: usize| {
        let wav = models::synthesize(&scratch, voice, &n.to_string(), LINES[n], 16_000);
        id += 1;
        sidecar
            .ask(&DiarizeRequest::embed(id, &wav, 2.0))
            .into_embedding()
            .expect("an embedding")
    };
    let me = embed(VOICES[0], 1);
    let voices: Vec<Vec<f32>> = VOICES.iter().map(|v| embed(v, 0)).collect();
    let dir = temp_dir("fb22b-real-db");
    let db = open_db(&dir);
    db.create_speaker_profile("You", true, DIGEST, &me, 30.0, None)
        .unwrap();
    let (meeting, _, _) = meeting_with_voices(&db, &dir, "Real", &[6, 6, 6, 6, 6], &voices);
    let cards = db
        .meeting_voice_cards(&meeting, MatchBands::provisional())
        .unwrap();
    assert!(cards.len() <= MAX_VOICE_CARDS);
    assert!(
        cards.iter().all(|c| c.cluster_index != 0),
        "you are never carded"
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&scratch);
}
