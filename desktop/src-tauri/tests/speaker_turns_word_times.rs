//! ENGINEERING MUST (a), yap-audit-utterance-segments — EXACT TURN TIMESTAMPS,
//! NO BLEED.
//!
//! A speaker turn is a run of whole ASR WORD rows (never window or chunk
//! times). Each word goes to the diarization segment covering its midpoint;
//! a switch that falls inside a word gives the word to the larger overlap. No
//! turn may contain a word whose midpoint lies in another speaker's segment,
//! and every turn's start/end is its first/last word's time, exactly.
//!
//! Always-on over a synthetic fixture (word times + segments). The real-audio
//! variant is `#[ignore]`d: it needs a diarization run with the models.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use wilson_voice_lib::asr_engine::TimedSpan;
use wilson_voice_lib::diarize::DiarizedSegment;
use wilson_voice_lib::meeting_asr::{
    build_speaker_turns, MeetingProgress, SpeakerTurns, TURN_EMBED_GUARD_SECONDS,
};
use wilson_voice_lib::meetings::{MIC_TRACK, SYSTEM_TRACK};

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/speaker-turns/two-voice-interleave.json")
}

struct Fixture {
    words: Vec<TimedSpan>,
    segments: Vec<DiarizedSegment>,
    /// speaker letter -> cluster index
    clusters: BTreeMap<String, i64>,
    expect: serde_json::Value,
}

fn load_fixture(track: i64) -> Fixture {
    let text = fs::read_to_string(fixture_path())
        .unwrap_or_else(|e| panic!("{}: {e}", fixture_path().display()));
    let v: serde_json::Value = serde_json::from_str(&text).expect("fixture is JSON");
    let words: Vec<TimedSpan> =
        serde_json::from_value(v["words"].clone()).expect("words are TimedSpan rows");
    let mut clusters = BTreeMap::new();
    let mut segments = Vec::new();
    for s in v["segments"].as_array().expect("segments array") {
        let speaker = s["speaker"].as_str().expect("speaker letter").to_string();
        let next = clusters.len() as i64;
        let cluster = *clusters.entry(speaker).or_insert(next);
        segments.push(DiarizedSegment::new(
            track,
            s["start"].as_f64().expect("start"),
            s["end"].as_f64().expect("end"),
            Some(cluster),
        ));
    }
    Fixture {
        words,
        segments,
        clusters,
        expect: v["expect"].clone(),
    }
}

fn mid(w: &TimedSpan) -> f64 {
    (w.start_seconds + w.end_seconds.max(w.start_seconds)) / 2.0
}

/// The invariants every build must hold, whatever the input.
fn assert_no_bleed(
    track: i64,
    words: &[TimedSpan],
    segments: &[DiarizedSegment],
    built: &SpeakerTurns,
) {
    // Every word in exactly one turn, in order, none dropped.
    let mut next = 0usize;
    for t in &built.turns {
        assert_eq!(t.words.start, next, "turns tile the words in order: {t:?}");
        assert!(t.words.end > t.words.start, "no empty turn: {t:?}");
        next = t.words.end;
    }
    assert_eq!(next, words.len(), "no word dropped");
    assert_eq!(built.word_segment.len(), words.len());

    for t in &built.turns {
        // Exact bounds: the first word's start and the last word's end.
        assert_eq!(t.start_seconds, words[t.words.start].start_seconds, "{t:?}");
        assert_eq!(t.end_seconds, words[t.words.end - 1].end_seconds, "{t:?}");
        // Zero cross-attributed words: no word's midpoint lies in a segment
        // of another speaker on this track.
        for w in &words[t.words.clone()] {
            let m = mid(w);
            for s in segments.iter().filter(|s| s.track == track) {
                let covers = s.start_seconds <= m && m < s.end_seconds;
                if covers && s.cluster_index.is_some() {
                    assert_eq!(
                        s.cluster_index,
                        t.cluster_index,
                        "word at {:.2}-{:.2} (mid {m:.2}) lies in cluster {:?}'s segment \
                         {:.2}-{:.2} but sits in a cluster {:?} turn",
                        w.start_seconds,
                        w.end_seconds,
                        s.cluster_index,
                        s.start_seconds,
                        s.end_seconds,
                        t.cluster_index
                    );
                }
            }
        }
        // Embeddings skip the guard nearest each boundary.
        if let Some((a, b)) = t.embedding_window() {
            assert!((a - (t.start_seconds + TURN_EMBED_GUARD_SECONDS)).abs() < 1e-9);
            assert!((b - (t.end_seconds - TURN_EMBED_GUARD_SECONDS)).abs() < 1e-9);
            assert!(b > a);
        } else {
            assert!(
                t.seconds() <= 2.0 * TURN_EMBED_GUARD_SECONDS + 1e-9,
                "{t:?}"
            );
        }
    }
}

#[test]
fn a_two_voice_interleave_has_zero_cross_attributed_words_and_exact_turn_bounds() {
    let f = load_fixture(SYSTEM_TRACK);
    let built = build_speaker_turns(SYSTEM_TRACK, &f.words, &f.segments);
    assert_no_bleed(SYSTEM_TRACK, &f.words, &f.segments, &built);

    let speakers: Vec<i64> = f.expect["turnSpeakers"]
        .as_array()
        .expect("turnSpeakers")
        .iter()
        .map(|s| f.clusters[s.as_str().expect("letter")])
        .collect();
    let got: Vec<i64> = built
        .turns
        .iter()
        .map(|t| t.cluster_index.expect("every fixture turn is attributed"))
        .collect();
    assert_eq!(got, speakers, "A/B alternate turn by turn");

    let first_words: Vec<&str> = f.expect["turnFirstWords"]
        .as_array()
        .expect("turnFirstWords")
        .iter()
        .map(|s| s.as_str().expect("word"))
        .collect();
    let got_first: Vec<&str> = built
        .turns
        .iter()
        .map(|t| f.words[t.words.start].text.as_str())
        .collect();
    assert_eq!(got_first, first_words);

    assert_eq!(
        built.boundary_words_reassigned as u64,
        f.expect["boundaryWordsReassigned"].as_u64().expect("count"),
        "the two mid-word switches are the only straddling words"
    );
}

#[test]
fn a_switch_inside_a_word_gives_the_word_to_the_midpoint_side_never_splits_it() {
    let f = load_fixture(SYSTEM_TRACK);
    let built = build_speaker_turns(SYSTEM_TRACK, &f.words, &f.segments);
    let a = f.clusters["A"];
    let b = f.clusters["B"];
    let turn_of = |word: &str| {
        let wi = f
            .words
            .iter()
            .position(|w| w.text == word)
            .expect("word in fixture");
        built
            .turns
            .iter()
            .find(|t| t.words.contains(&wi))
            .expect("every word is in a turn")
            .clone()
    };
    // "absolutely" 6.05-6.65 straddles the B->A switch at 6.2: midpoint 6.35 is A's.
    let t = turn_of("absolutely");
    assert_eq!(t.cluster_index, Some(a));
    assert_eq!(
        t.start_seconds, 6.05,
        "the turn starts on the word, not on the 6.2 switch"
    );
    // "okay" 12.25-12.75 straddles the A->B switch at 12.4: midpoint 12.5 is B's.
    let t = turn_of("okay");
    assert_eq!(t.cluster_index, Some(b));
    assert_eq!(t.start_seconds, 12.25);
    // The A turn before it ends on "tonight", not on the 12.4 switch.
    let before = turn_of("tonight");
    assert_eq!(before.end_seconds, 11.40);
    // The one-word interjection is its own turn, word-exact.
    let t = turn_of("yeah");
    assert_eq!(
        (t.cluster_index, t.start_seconds, t.end_seconds),
        (Some(b), 8.85, 9.20)
    );
    assert_eq!(t.words.len(), 1);
    assert_eq!(
        t.embedding_window(),
        None,
        "0.35 s minus 0.2 s each side leaves nothing"
    );
    // A word past the last segment goes to the nearest one, and stays whole.
    let t = turn_of("bye");
    assert_eq!(t.cluster_index, Some(b));
    assert_eq!(t.end_seconds, 14.40);
}

#[test]
fn segments_of_another_track_never_attribute_this_tracks_words() {
    let f = load_fixture(MIC_TRACK);
    // Words of SYSTEM_TRACK against segments that all belong to MIC_TRACK.
    let built = build_speaker_turns(SYSTEM_TRACK, &f.words, &f.segments);
    assert_eq!(
        built.turns.len(),
        1,
        "no segment on this track: one unattributed run"
    );
    assert_eq!(built.turns[0].cluster_index, None);
    assert!(built.word_segment.iter().all(Option::is_none));
    assert_eq!(built.boundary_words_reassigned, 0);
    assert_no_bleed(SYSTEM_TRACK, &f.words, &f.segments, &built);
}

#[test]
fn neighbouring_unattributed_segments_are_two_turns_not_one() {
    let w = |s: f64, e: f64, t: &str| TimedSpan {
        start_seconds: s,
        end_seconds: e,
        text: t.to_string(),
    };
    let words = vec![w(0.1, 0.5, "one"), w(0.6, 0.9, "two"), w(1.1, 1.5, "three")];
    let segments = vec![
        DiarizedSegment::new(SYSTEM_TRACK, 0.0, 1.0, None),
        DiarizedSegment::new(SYSTEM_TRACK, 1.0, 2.0, None),
    ];
    let built = build_speaker_turns(SYSTEM_TRACK, &words, &segments);
    assert_eq!(built.turns.len(), 2, "two unknowns may be two people");
    assert_eq!(built.turns[0].text, "one two");
    assert_eq!(built.turns[1].text, "three");
    assert_no_bleed(SYSTEM_TRACK, &words, &segments, &built);
}

#[test]
fn no_words_build_no_turns() {
    let built = build_speaker_turns(SYSTEM_TRACK, &[], &[]);
    assert!(built.turns.is_empty());
    assert!(built.word_segment.is_empty());
}

/// Real audio: the word rows of a real decode (the committed timings-only
/// ledger of the NT0 `call-bleed-2voice` system track) against a REAL
/// diarization of the same audio. Needs the yap-diarize sidecar and its
/// models, so it is ignored by default. Run it with
/// `YAP_TURNS_DIARIZE_SEGMENTS=<json array of {start,end,cluster}>` — the
/// `segments` of the sidecar's response for `call-bleed-2voice.t1.wav` —
/// and `cargo test --test speaker_turns_word_times -- --ignored`.
#[test]
#[ignore = "needs a real diarization of call-bleed-2voice (sidecar + models)"]
fn real_audio_turns_have_zero_cross_attributed_words() {
    let ledger_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/meeting-utterances/call-bleed-2voice.t1.asr-progress.json");
    let ledger: MeetingProgress =
        serde_json::from_str(&fs::read_to_string(&ledger_path).expect("ledger"))
            .expect("a resume ledger");
    let words: Vec<TimedSpan> = ledger
        .chunks
        .iter()
        .flat_map(|c| c.spans.iter().cloned())
        .collect();
    assert!(!words.is_empty(), "the ledger carries word rows");
    let seg_path = std::env::var("YAP_TURNS_DIARIZE_SEGMENTS")
        .expect("set YAP_TURNS_DIARIZE_SEGMENTS to the sidecar's segments JSON");
    let raw: Vec<wilson_voice_lib::diarize_protocol::DiarizeSegment> =
        serde_json::from_str(&fs::read_to_string(&seg_path).expect("segments file"))
            .expect("an array of {start,end,cluster}");
    let segments: Vec<DiarizedSegment> = raw
        .iter()
        .map(|s| DiarizedSegment::new(SYSTEM_TRACK, s.start, s.end, Some(i64::from(s.cluster))))
        .collect();
    let built = build_speaker_turns(SYSTEM_TRACK, &words, &segments);
    assert!(!built.turns.is_empty());
    assert_no_bleed(SYSTEM_TRACK, &words, &segments, &built);
}
