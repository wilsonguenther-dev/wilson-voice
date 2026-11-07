//! yap-1106-who-is-this — the "Who was this?" cards (spec §9, design
//! meetings.html §B).
//!
//! ACCEPTANCE: naming relabels every line of the cluster AND adds a voice
//! sample; a meeting whose audio was purged (no voice kept) can still be named
//! from text; "Not sure" / "Ignore this voice" keep nothing; the card picker
//! asks about at most five unknown voices with ≥ 20 s of speech, most speaking
//! time first, never about you, with a ≤ 3 s clip that stays inside ONE
//! word-exact turn. Every test drives the shipped `Database` on a temp DB or
//! the shipped pure functions.

mod support;

use support::{open_db, seed_meeting, temp_dir};
use wilson_voice_lib::diarize::cluster_voices;
use wilson_voice_lib::diarize_metrics::cosine_similarity;
use wilson_voice_lib::diarize_protocol::DiarizeSegment;
use wilson_voice_lib::meetings::MeetingSegment;
use wilson_voice_lib::speaker_profiles::{
    clearest_clip, pick_voice_cards, ClusterCandidate, ClusterLine, ClusterVoiceState, NameTarget,
    VoiceMatch, MAX_VOICE_CARDS, UNKNOWN_VOICE_MIN_SECONDS, VOICE_CLIP_SECONDS,
};

const DIM: usize = 8;
const DIGEST: &str = "c46fad10b5f81e1aa4a60c162714208577093655076c5450f8c469e522ec54ef";

fn axis(i: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; DIM];
    v[i] = 1.0;
    v
}

/// Six lines alternating between two clusters: 0, 2, 4 → cluster 0; 1, 3, 5 → cluster 1.
fn two_voice_meeting(
    db: &wilson_voice_lib::db::Database,
    dir: &std::path::Path,
) -> (String, Vec<MeetingSegment>) {
    let meeting = seed_meeting(
        db,
        dir,
        "Launch sync",
        &["a0", "b0", "a1", "b1", "a2", "b2"],
    );
    let mut segs = db.list_meeting_segments(&meeting).unwrap();
    segs.sort_by(|a, b| a.start_seconds.partial_cmp(&b.start_seconds).unwrap());
    let assignments: Vec<(String, Option<i64>)> = segs
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id.clone(), Some((i % 2) as i64)))
        .collect();
    db.set_segment_clusters(&meeting, &assignments).unwrap();
    (meeting, segs)
}

#[test]
fn naming_a_cluster_relabels_every_line_and_adds_a_sample() {
    let dir = temp_dir("who-name");
    let db = open_db(&dir);
    let (meeting, segs) = two_voice_meeting(&db, &dir);
    let track = segs[0].track;
    db.save_meeting_cluster_voices(
        &meeting,
        track,
        Some(DIGEST),
        &[(0, axis(0), 30.0), (1, axis(1), 25.0)],
    )
    .unwrap();

    let named = db
        .name_meeting_cluster(
            &meeting,
            track,
            0,
            &NameTarget::NewName {
                name: "  John ".into(),
            },
        )
        .unwrap();
    assert_eq!(named.display_name, "John");
    assert_eq!(
        named.relabelled, 3,
        "all three of John's lines move at once"
    );
    assert!(
        named.sample_added,
        "the cluster's voice feeds the voiceprint"
    );
    assert_eq!(db.speaker_sample_count(named.profile_id).unwrap(), 1);
    for (i, s) in segs.iter().enumerate() {
        let (pid, name) = db.segment_speaker(&s.id).unwrap();
        if i % 2 == 0 {
            assert_eq!(pid, Some(named.profile_id));
            assert_eq!(name.as_deref(), Some("John"));
        } else {
            assert_eq!(pid, None, "the other voice's lines are never touched");
            assert_eq!(name, None);
        }
    }
    let profile = db.get_speaker_profile(named.profile_id).unwrap().unwrap();
    assert_eq!(profile.n_samples, 1);
    assert_eq!(profile.embedder_digest, DIGEST);

    // "Yes, same person": the second voice joins the existing profile and
    // its lines take the name too.
    let merged = db
        .name_meeting_cluster(
            &meeting,
            track,
            1,
            &NameTarget::Profile {
                profile_id: named.profile_id,
            },
        )
        .unwrap();
    assert_eq!(merged.profile_id, named.profile_id);
    assert_eq!(merged.relabelled, 3);
    assert!(merged.sample_added);
    assert_eq!(db.speaker_sample_count(named.profile_id).unwrap(), 2);
    assert_eq!(db.meeting_speaker_names(&meeting).unwrap().len(), 6);

    let states: Vec<ClusterVoiceState> = db
        .meeting_cluster_voices(&meeting)
        .unwrap()
        .into_iter()
        .map(|v| v.state)
        .collect();
    assert_eq!(
        states,
        vec![ClusterVoiceState::Named, ClusterVoiceState::Named]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn typing_an_existing_name_updates_that_profile_instead_of_a_duplicate() {
    let dir = temp_dir("who-same-name");
    let db = open_db(&dir);
    let (meeting, segs) = two_voice_meeting(&db, &dir);
    let track = segs[0].track;
    db.save_meeting_cluster_voices(
        &meeting,
        track,
        Some(DIGEST),
        &[(0, axis(0), 30.0), (1, axis(0), 25.0)],
    )
    .unwrap();
    let first = db
        .name_meeting_cluster(
            &meeting,
            track,
            0,
            &NameTarget::NewName {
                name: "Maya".into(),
            },
        )
        .unwrap();
    let second = db
        .name_meeting_cluster(
            &meeting,
            track,
            1,
            &NameTarget::NewName {
                name: "maya".into(),
            },
        )
        .unwrap();
    assert_eq!(first.profile_id, second.profile_id);
    assert_eq!(db.list_speaker_profiles().unwrap().len(), 1);
    assert_eq!(db.speaker_sample_count(first.profile_id).unwrap(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn purged_audio_still_allows_naming_from_text() {
    let dir = temp_dir("who-purged");
    let db = open_db(&dir);
    let (meeting, segs) = two_voice_meeting(&db, &dir);
    let track = segs[0].track;
    // The pass kept no voice (digest unknown / audio gone before it ran).
    db.save_meeting_cluster_voices(&meeting, track, None, &[(0, axis(0), 30.0)])
        .unwrap();
    let named = db
        .name_meeting_cluster(
            &meeting,
            track,
            0,
            &NameTarget::NewName {
                name: "Priya".into(),
            },
        )
        .unwrap();
    assert_eq!(named.relabelled, 3);
    assert!(
        !named.sample_added,
        "no voice was kept, so none is invented"
    );
    let profile = db.get_speaker_profile(named.profile_id).unwrap().unwrap();
    assert_eq!(
        profile.n_samples, 0,
        "a name with no voiceprint is never matched"
    );
    assert_eq!(db.speaker_sample_count(named.profile_id).unwrap(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn thats_me_labels_the_lines_you_and_marks_the_profile_as_yours() {
    let dir = temp_dir("who-me");
    let db = open_db(&dir);
    let (meeting, segs) = two_voice_meeting(&db, &dir);
    let track = segs[0].track;
    db.save_meeting_cluster_voices(&meeting, track, Some(DIGEST), &[(1, axis(3), 22.0)])
        .unwrap();
    let me = db
        .name_meeting_cluster(&meeting, track, 1, &NameTarget::Me)
        .unwrap();
    assert_eq!(me.display_name, "You");
    assert!(me.sample_added);
    let profile = db.get_speaker_profile(me.profile_id).unwrap().unwrap();
    assert!(profile.is_me);
    let state = db
        .meeting_cluster_voices(&meeting)
        .unwrap()
        .into_iter()
        .find(|v| v.cluster_index == 1)
        .unwrap()
        .state;
    assert_eq!(state, ClusterVoiceState::Me);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn not_sure_and_ignore_keep_nothing_and_leave_speaker_n() {
    let dir = temp_dir("who-dismiss");
    let db = open_db(&dir);
    let (meeting, segs) = two_voice_meeting(&db, &dir);
    let track = segs[0].track;
    db.save_meeting_cluster_voices(
        &meeting,
        track,
        Some(DIGEST),
        &[(0, axis(0), 30.0), (1, axis(1), 25.0)],
    )
    .unwrap();
    db.dismiss_meeting_cluster(&meeting, track, 0, ClusterVoiceState::NotSure)
        .unwrap();
    db.dismiss_meeting_cluster(&meeting, track, 1, ClusterVoiceState::Ignored)
        .unwrap();
    for v in db.meeting_cluster_voices(&meeting).unwrap() {
        assert!(v.embedding.is_none(), "nothing of the voice is kept");
        assert!(v.embedder_digest.is_none());
    }
    assert!(db.list_speaker_profiles().unwrap().is_empty());
    assert!(db.meeting_speaker_names(&meeting).unwrap().is_empty());
    assert!(db
        .dismiss_meeting_cluster(&meeting, track, 0, ClusterVoiceState::Named)
        .is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

fn line(id: &str, start: f64, end: f64) -> ClusterLine {
    ClusterLine {
        segment_id: id.into(),
        start_seconds: start,
        end_seconds: end,
        text: format!("line {id}"),
        labelled: false,
    }
}

/// `n` turns of `each` seconds, back to back from `at`.
fn lines(tag: &str, at: f64, n: usize, each: f64) -> Vec<ClusterLine> {
    (0..n)
        .map(|i| {
            let s = at + i as f64 * (each + 1.0);
            line(&format!("{tag}{i}"), s, s + each)
        })
        .collect()
}

fn unknown(cluster: i64, lines: Vec<ClusterLine>) -> ClusterCandidate {
    ClusterCandidate {
        track: 0,
        cluster_index: cluster,
        state: ClusterVoiceState::Unknown,
        lines,
        voice_match: Some(VoiceMatch::Unknown { best: None }),
        me_profile_id: Some(1),
    }
}

#[test]
fn cards_are_unknown_voices_over_twenty_seconds_five_at_most_loudest_first() {
    let mut cands: Vec<ClusterCandidate> = (0..8)
        .map(|c| unknown(c, lines("c", 0.0, 5, 5.0 + c as f64)))
        .collect();
    // Under the 20 s floor: never a card.
    cands.push(unknown(20, lines("short", 0.0, 3, 5.0)));
    let cards = pick_voice_cards(&cands);
    assert_eq!(cards.len(), MAX_VOICE_CARDS);
    let order: Vec<i64> = cards.iter().map(|c| c.cluster_index).collect();
    assert_eq!(order, vec![7, 6, 5, 4, 3], "most speaking time first");
    assert!(cards.iter().all(|c| c.seconds >= UNKNOWN_VOICE_MIN_SECONDS));
}

#[test]
fn answered_labelled_or_your_voice_is_never_asked_about() {
    let mut named = unknown(0, lines("n", 0.0, 5, 6.0));
    named.state = ClusterVoiceState::Named;
    let mut dismissed = unknown(1, lines("d", 0.0, 5, 6.0));
    dismissed.state = ClusterVoiceState::NotSure;
    let mut labelled = unknown(2, lines("l", 0.0, 5, 6.0));
    labelled.lines[0].labelled = true;
    let mut me = unknown(3, lines("m", 0.0, 5, 6.0));
    me.voice_match = Some(VoiceMatch::Ask {
        profile_id: 1,
        display_name: "You".into(),
        similarity: cosine_similarity(&axis(0), &axis(0)),
    });
    let mut alex = unknown(4, lines("a", 0.0, 5, 6.0));
    alex.voice_match = Some(VoiceMatch::Ask {
        profile_id: 9,
        display_name: "Alex".into(),
        similarity: cosine_similarity(&axis(0), &axis(0)),
    });
    let cards = pick_voice_cards(&[named, dismissed, labelled, me, alex]);
    assert_eq!(cards.len(), 1);
    let s = cards[0]
        .suggestion
        .as_ref()
        .expect("an ask-band match reads 'Same person as Alex?'");
    assert_eq!(s.display_name, "Alex");
    assert_eq!(s.profile_id, 9);
}

#[test]
fn the_clip_is_three_seconds_inside_the_longest_turn_and_never_crosses_a_switch() {
    let turns = vec![
        line("a", 0.0, 2.0),
        line("b", 10.0, 17.5),
        line("c", 20.0, 21.0),
    ];
    let (start, end, i) = clearest_clip(&turns).unwrap();
    assert_eq!(i, 1, "the longest turn is the clearest");
    assert_eq!(start, 10.0, "starts on the turn's first word");
    assert!((end - start - VOICE_CLIP_SECONDS).abs() < 1e-9);
    assert!(end <= turns[1].end_seconds, "never leaves the turn");
    // A turn shorter than the clip is played whole, and still never past its end.
    let short = vec![line("x", 4.0, 5.2)];
    let (s, e, _) = clearest_clip(&short).unwrap();
    assert_eq!((s, e), (4.0, 5.2));
    assert!(clearest_clip(&[]).is_none());
}

#[test]
fn a_cluster_voice_is_the_normalised_mean_of_its_turns() {
    let seg = |start: f64, end: f64, emb: Vec<f32>| DiarizeSegment {
        start,
        end,
        cluster: 0,
        embedding: emb,
    };
    let raw = vec![
        seg(0.0, 2.0, vec![2.0, 0.0, 0.0]),
        seg(2.0, 5.0, vec![0.0, 3.0, 0.0]),
        seg(5.0, 6.0, vec![0.0, 0.0, 1.0]),
        // Under the embedding floor: no evidence, still talk time.
        seg(6.0, 6.5, Vec::new()),
    ];
    let clusters = vec![Some(4), Some(4), Some(7), Some(7)];
    let voices = cluster_voices(&raw, &clusters);
    assert_eq!(voices.len(), 2);
    let a = &voices[0];
    assert_eq!(a.cluster_index, 4);
    assert!((a.seconds - 5.0).abs() < 1e-9);
    let h = std::f32::consts::FRAC_1_SQRT_2;
    assert!((a.embedding[0] - h).abs() < 1e-6 && (a.embedding[1] - h).abs() < 1e-6);
    let b = &voices[1];
    assert!((b.seconds - 1.5).abs() < 1e-9);
    assert_eq!(b.embedding, vec![0.0, 0.0, 1.0]);
    // An unattributed turn feeds no voice.
    assert!(cluster_voices(&raw, &[None, None, None, None]).is_empty());
}
