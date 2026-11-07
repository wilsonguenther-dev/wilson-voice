//! yap-1106-diarize-wire acceptance — the diarization pass a finished meeting
//! gets, driven end to end through the shipping `meeting_diarize::diarize_meeting`
//! against a STUB PROCESS (the `tests/diarize_sidecar_pool.rs` pattern: a
//! `/bin/sh` script `DiarizePool` cannot tell from the real sidecar).
//!
//! ```sh
//! cargo test --test meeting_diarize_wire
//! ```
//!
//! The ground truth is a two-speaker RTTM (the YV120 RTTM shape,
//! `diarize_metrics::RttmTurn::parse_rttm`) over the speaker-turns fixture's
//! word times: A/B alternating every 2–4 s, a one-word interjection ("yeah"),
//! two switches INSIDE a word ("absolutely", "okay") and a trailing word past
//! the last turn ("bye"). The stub answers those turns with one orthogonal
//! embedding per speaker, so the parent's own clustering — at the shipped
//! provisional distance — is what turns them into two clusters.

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use support::diarize_stub::{stub_returning, StubTurn};
use support::{open_db, temp_dir};
use wilson_voice_lib::diarize::DiarizePool;
use wilson_voice_lib::diarize_metrics::RttmTurn;
use wilson_voice_lib::meeting_diarize::{diarize_meeting, DiarizeBackend, DiarizeResult};
use wilson_voice_lib::meetings::{
    render_transcript, MeetingKind, MeetingState, NewMeetingSegment, SYSTEM_TRACK,
};

const READY: Duration = Duration::from_secs(10);
const FIXTURE: &str = include_str!("fixtures/speaker-turns/two-voice-interleave.json");
const RTTM: &str = include_str!("fixtures/speaker-turns/two-voice-interleave.rttm");

#[derive(Clone)]
struct Word {
    start: f64,
    end: f64,
    text: String,
}

fn words() -> Vec<Word> {
    let v: serde_json::Value = serde_json::from_str(FIXTURE).expect("fixture parses");
    v["words"]
        .as_array()
        .expect("words")
        .iter()
        .map(|w| Word {
            start: w["startSeconds"].as_f64().unwrap(),
            end: w["endSeconds"].as_f64().unwrap(),
            text: w["text"].as_str().unwrap().to_string(),
        })
        .collect()
}

/// The speaker each word belongs to under the rule ENGINEERING MUST (a)
/// states: the RTTM turn covering the word's midpoint, else the nearest turn
/// (the trailing "bye"). Cross-checked against the fixture's hand-written turn
/// sequence so the rule and the fixture cannot drift apart silently.
fn expected_speakers(words: &[Word]) -> Vec<String> {
    let turns = RttmTurn::parse_rttm(RTTM);
    assert_eq!(turns.len(), 6, "the RTTM fixture has six turns");
    let out: Vec<String> = words
        .iter()
        .map(|w| {
            let m = mid(w);
            let covering = turns
                .iter()
                .find(|t| m >= t.start_seconds && m < t.end_seconds);
            let nearest = || {
                turns
                    .iter()
                    .min_by(|a, b| {
                        let da = (a.start_seconds - m).abs().min((a.end_seconds - m).abs());
                        let db = (b.start_seconds - m).abs().min((b.end_seconds - m).abs());
                        da.total_cmp(&db)
                    })
                    .expect("turns")
            };
            covering.unwrap_or_else(nearest).speaker_id.clone()
        })
        .collect();
    let v: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    let hand: Vec<String> = v["expect"]["turnSpeakers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    let mut runs: Vec<String> = Vec::new();
    for s in &out {
        if runs.last() != Some(s) {
            runs.push(s.clone());
        }
    }
    assert_eq!(
        runs, hand,
        "midpoint rule over the RTTM reproduces the fixture's turns"
    );
    out
}

fn rttm_stub_turns() -> Vec<StubTurn> {
    RttmTurn::parse_rttm(RTTM)
        .into_iter()
        .map(|t| {
            let embedding = if t.speaker_id == "A" {
                vec![1.0f32, 0.0]
            } else {
                vec![0.0f32, 1.0]
            };
            StubTurn::new(t.start_seconds, t.end_seconds, 0, embedding)
        })
        .collect()
}

/// The resume ledger the pipeline leaves for track `track`: one finished
/// window with word times — exactly what `ledger_words` reads back.
fn write_ledger(dir: &Path, meeting_id: &str, track: i64, words: &[Word]) {
    let spans: Vec<serde_json::Value> = words
        .iter()
        .map(|w| serde_json::json!({"startSeconds": w.start, "endSeconds": w.end, "text": w.text}))
        .collect();
    let text = words
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let ledger = serde_json::json!({
        "meetingId": format!("{meeting_id}.t{track}"),
        "processedThroughSeconds": 20.0,
        "chunks": [{
            "index": 0,
            "audioStartSeconds": 0.0,
            "contentStartSeconds": 0.0,
            "contentEndSeconds": 20.0,
            "startBoundary": "edge",
            "endBoundary": "edge",
            "status": "done",
            "text": text,
            "spans": spans,
            "timestampKind": "word"
        }]
    });
    std::fs::write(
        dir.join(format!("{meeting_id}.t{track}.asr-progress.json")),
        serde_json::to_vec_pretty(&ledger).unwrap(),
    )
    .unwrap();
}

/// Rows of `per_row` words each, deliberately cut WITHOUT regard to who is
/// speaking — so several straddle a switch and the pass has to split them.
fn rows_of(words: &[Word], per_row: usize, track: i64) -> Vec<NewMeetingSegment> {
    words
        .chunks(per_row)
        .map(|c| {
            let text = c
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            NewMeetingSegment::new(c[0].start, c[c.len() - 1].end, text).on_track(track)
        })
        .collect()
}

fn mid(w: &Word) -> f64 {
    (w.start + w.end) / 2.0
}

#[test]
fn an_in_person_meeting_gets_two_clusters_split_at_word_times_and_two_labels() {
    let dir = temp_dir("diarize-wire-in-person");
    let db = open_db(&dir);
    let meeting = db
        .create_meeting_with_kind("Two voices, one room", "manual", MeetingKind::InPerson)
        .unwrap();
    let words = words();
    let expected = expected_speakers(&words);
    db.append_meeting_segments(&meeting.id, &rows_of(&words, 8, 0))
        .unwrap();
    let wav = dir.join("mic.wav");
    std::fs::write(&wav, b"RIFF....WAVEfake").unwrap();
    db.finish_meeting(&meeting.id, 20.0, Some(&wav)).unwrap();
    db.set_meeting_state(&meeting.id, MeetingState::Complete, None)
        .unwrap();
    write_ledger(&dir, &meeting.id, 0, &words);

    let pool = Arc::new(DiarizePool::new(stub_returning(rttm_stub_turns()), READY));
    let outcome = diarize_meeting(&db, &meeting.id, &dir, &DiarizeBackend::Pool(pool.clone()));
    pool.shutdown();

    assert_eq!(
        outcome.result,
        DiarizeResult::Ok,
        "{}",
        outcome.log_line(&meeting.id)
    );
    assert_eq!(outcome.clusters, 2);
    assert_eq!(
        outcome.track,
        Some(0),
        "an in-person meeting clusters the mic"
    );
    assert!(outcome.rows_split > 0, "rows straddling a switch are split");
    assert_eq!(
        outcome.boundary_words_reassigned, 2,
        "the two switches inside a word (absolutely, okay)"
    );
    assert!(outcome
        .log_line(&meeting.id)
        .starts_with("diarize result=ok clusters=2 speech_s="));
    assert!(outcome.labels.marker().is_none());

    // Clusters reach meeting_segments.cluster_index.
    let rows = db.list_meeting_segments(&meeting.id).unwrap();
    let clusters: BTreeSet<i64> = rows.iter().filter_map(|r| r.cluster_index).collect();
    assert_eq!(clusters.len(), 2, "two clusters stored: {clusters:?}");
    assert!(rows.iter().all(|r| r.cluster_index.is_some()));

    // ZERO cross-attributed words, and every row's bounds are its first and
    // last word's times exactly.
    let mut cluster_to_speaker: BTreeMap<i64, String> = BTreeMap::new();
    let mut seen_words = 0usize;
    for row in &rows {
        let inside: Vec<usize> = (0..words.len())
            .filter(|i| {
                let m = mid(&words[*i]);
                m >= row.start_seconds - 1e-9 && m <= row.end_seconds + 1e-9
            })
            .collect();
        assert!(!inside.is_empty(), "row {:?} holds no word", row.text);
        seen_words += inside.len();
        let first = &words[inside[0]];
        let last = &words[*inside.last().unwrap()];
        assert_eq!(
            row.start_seconds, first.start,
            "row starts on its first word"
        );
        assert_eq!(row.end_seconds, last.end, "row ends on its last word");
        let cluster = row.cluster_index.unwrap();
        for i in inside {
            let speaker = &expected[i];
            let mapped = cluster_to_speaker
                .entry(cluster)
                .or_insert_with(|| speaker.clone());
            assert_eq!(
                mapped, speaker,
                "word {:?} at {:.2}s is cross-attributed",
                words[i].text, words[i].start
            );
        }
    }
    assert_eq!(
        seen_words,
        words.len(),
        "no word lost or duplicated by a split"
    );
    let both: BTreeSet<&String> = cluster_to_speaker.values().collect();
    assert_eq!(both.len(), 2, "the two clusters are the two speakers");

    // The rendered transcript shows exactly two labels.
    let labels: BTreeSet<String> = render_transcript(&rows, MeetingKind::InPerson)
        .iter()
        .map(|l| l.label())
        .collect();
    assert_eq!(
        labels,
        BTreeSet::from(["Speaker 1".to_string(), "Speaker 2".to_string()])
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_virtual_meeting_clusters_the_call_track_and_the_mic_stays_you() {
    let dir = temp_dir("diarize-wire-virtual");
    let db = open_db(&dir);
    let meeting = db
        .create_meeting_with_kind("Two remote voices", "manual", MeetingKind::Virtual)
        .unwrap();
    let words = words();
    let mut rows = rows_of(&words, 8, SYSTEM_TRACK);
    rows.push(NewMeetingSegment::new(15.0, 16.0, "my own words").on_track(0));
    db.append_meeting_segments(&meeting.id, &rows).unwrap();
    let mic = dir.join("mic.wav");
    let sys = dir.join("sys.wav");
    std::fs::write(&mic, b"RIFF....WAVEfake").unwrap();
    std::fs::write(&sys, b"RIFF....WAVEfake").unwrap();
    db.finish_meeting(&meeting.id, 20.0, Some(&mic)).unwrap();
    db.set_meeting_sys_wav_path(&meeting.id, Some(&sys))
        .unwrap();
    db.set_meeting_state(&meeting.id, MeetingState::Complete, None)
        .unwrap();
    write_ledger(&dir, &meeting.id, SYSTEM_TRACK, &words);

    let pool = Arc::new(DiarizePool::new(stub_returning(rttm_stub_turns()), READY));
    let outcome = diarize_meeting(&db, &meeting.id, &dir, &DiarizeBackend::Pool(pool.clone()));
    pool.shutdown();
    assert_eq!(outcome.result, DiarizeResult::Ok);
    assert_eq!(
        outcome.track,
        Some(SYSTEM_TRACK),
        "a virtual call clusters the call track"
    );

    let stored = db.list_meeting_segments(&meeting.id).unwrap();
    assert!(stored
        .iter()
        .filter(|r| r.track == 0)
        .all(|r| r.cluster_index.is_none()));
    let labels: BTreeSet<String> = render_transcript(&stored, MeetingKind::Virtual)
        .iter()
        .map(|l| l.label())
        .collect();
    assert_eq!(
        labels,
        BTreeSet::from([
            "You".to_string(),
            "Call · Speaker 1".to_string(),
            "Call · Speaker 2".to_string()
        ])
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn off_is_a_logged_skip_that_touches_nothing() {
    let dir = temp_dir("diarize-wire-off");
    let db = open_db(&dir);
    let meeting = db
        .create_meeting_with_kind("Off", "manual", MeetingKind::InPerson)
        .unwrap();
    db.append_meeting_segments(&meeting.id, &rows_of(&words(), 8, 0))
        .unwrap();
    let outcome = diarize_meeting(&db, &meeting.id, &dir, &DiarizeBackend::Off);
    assert_eq!(outcome.result, DiarizeResult::Skipped("off"));
    assert!(outcome
        .log_line(&meeting.id)
        .starts_with("diarize result=skipped clusters=0"));
    assert!(db
        .list_meeting_segments(&meeting.id)
        .unwrap()
        .iter()
        .all(|r| r.cluster_index.is_none()));
    let _ = std::fs::remove_dir_all(&dir);
}
