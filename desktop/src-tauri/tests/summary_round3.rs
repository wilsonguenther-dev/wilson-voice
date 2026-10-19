//! #250 round 3 — the review-opus findings, as fixtures.
//!
//! B4: small talk never becomes an action item and never gets a bold owner.
//! A1: key points never echo the Summary paragraph or a decision.
//! A2: a line ABOUT whether something was decided is an open question.
//! A5: the Summary never claims a status nobody stated.
//! B2: a cancelled summary stores nothing; a cancelled download stops
//!     between chunks and keeps its `.partial` for the resume.

mod support;

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use support::{segments_from, StubModel};
use wilson_voice_lib::polish_protocol::PolishRequest;
use wilson_voice_lib::summarize::{
    action_owner, classify_item, dedupe_key_points, ground_extract, summarize_segments,
    transcript_lines, transcript_names, validate_narrative, Bucket, Generated, MapExtract,
    SummaryClient, SummaryError, SummaryItem, TokenCounter,
};
use wilson_voice_lib::summary_jobs::CancelToken;

/// The reviewer's probe lines plus one more: openers that appear in nearly
/// every real meeting.
const SMALL_TALK: &[&str] = &[
    "Nice to meet you everyone",
    "Happy to help with the rollout",
    "Thanks to Priya the import works now",
    "Good to see the numbers moving",
    "Hard to say how long the migration takes",
    "Yes to the annual plan from my side",
    "Honestly can we move on",
    "Great to have everyone here today",
];

#[test]
fn small_talk_never_becomes_an_action_item() {
    let segments = segments_from(SMALL_TALK);
    let lines = transcript_lines(&segments);
    let source = lines.iter().map(|l| l.render()).collect::<Vec<_>>().join("\n");
    let mut extract = MapExtract::default();
    ground_extract(&mut extract, &lines, &source);
    assert!(extract.actions.is_empty(), "backfilled small talk: {:?}", extract.actions);
    for line in SMALL_TALK {
        // Filed by the model as an action, the classifier still drops it.
        assert_ne!(classify_item(Bucket::Actions, line), Some(Bucket::Actions), "{line}");
        assert_eq!(action_owner(line), None, "fake owner for {line}");
    }
    // "Thanks to Priya" names Priya — mid-sentence — and never "Thanks".
    let names = transcript_names(&lines);
    assert!(names.iter().any(|n| n == "Priya"), "{names:?}");
    assert!(!names.iter().any(|n| n == "Thanks" || n == "Nice" || n == "Honestly"), "{names:?}");
}

#[test]
fn backfill_still_takes_named_commitments() {
    let segments = segments_from(&[
        "Thanks to Priya the import works now.",
        "Priya to file the migration report.",
        "Jeisil will send the updated deck by Friday.",
        "Happy to help with the rollout.",
    ]);
    let lines = transcript_lines(&segments);
    let source = lines.iter().map(|l| l.render()).collect::<Vec<_>>().join("\n");
    let mut extract = MapExtract::default();
    ground_extract(&mut extract, &lines, &source);
    let texts: Vec<&str> = extract.actions.iter().map(|a| a.text.as_str()).collect();
    assert_eq!(
        texts,
        vec!["Priya to file the migration report", "Jeisil will send the updated deck by Friday"],
    );
    assert_eq!(action_owner(texts[0]), Some("Priya".into()));
}

#[test]
fn asking_about_a_decision_is_an_open_question() {
    for line in [
        "Jeisil asked whether we decided.",
        "Nobody decided whether to keep it.",
        "We haven't decided on the annual discount.",
    ] {
        assert_eq!(classify_item(Bucket::Decisions, line), Some(Bucket::Questions), "{line}");
        assert_eq!(classify_item(Bucket::Actions, line), Some(Bucket::Questions), "{line}");
    }
    assert_eq!(
        classify_item(Bucket::Actions, "We decided to drop the free trial."),
        Some(Bucket::Decisions)
    );
}

fn item(text: &str) -> SummaryItem {
    SummaryItem { text: text.into(), segment: "seg_0001".into(), start_seconds: 0.0, speaker: None }
}

#[test]
fn key_points_never_echo_the_summary_or_a_decision() {
    // The round-2 real run, verbatim: points 1–5 were Summary sentences and
    // point 6 the price decision again.
    let narrative = "The pricing review for the Drivia consulting tier is complete. The starter package will remain at forty-nine dollars a month. A pricing update document will be sent to the team by Friday. Wilson will book a demo with Liberty University next week. The open question is whether annual billing gets a discount. The free trial for enterprise accounts will be dropped.";
    let points: Vec<String> = [
        "The pricing review for the Drivia consulting tier is complete.",
        "The starter package will remain at forty-nine dollars a month.",
        "A pricing update document will be sent to the team by Friday.",
        "Wilson will book a demo with Liberty University next week.",
        "The free trial for enterprise accounts will be dropped.",
        "We agreed the starter package stays at forty-nine dollars a month.",
        "Annual billing discount is still being weighed.",
        "Annual billing discount is still being weighed for now.",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let decisions = [item("We agreed the starter package stays at forty nine dollars a month")];
    let kept = dedupe_key_points(points, narrative, &decisions);
    assert_eq!(kept, vec!["Annual billing discount is still being weighed."]);
}

#[test]
fn summary_never_claims_a_status_nobody_stated() {
    let source = "seg_0001: Okay, this is the pricing review for the Drivia consulting tier.\nseg_0002: We agreed the starter package stays at forty nine dollars a month.";
    let got = validate_narrative(
        source,
        "The pricing review for the Drivia consulting tier is complete. The starter package stays at forty nine dollars a month.",
    )
    .expect("the supported sentence survives");
    assert_eq!(got, "The starter package stays at forty nine dollars a month.");
    // Said in the meeting → allowed.
    let said = "seg_0001: The pricing review is complete and the starter package stays at forty nine dollars.";
    assert!(validate_narrative(said, "The pricing review is complete.").is_some());
}

/// A client that cancels its own job during the first model call — the shape
/// of a Cancel pressed while MAP runs.
struct CancelDuring {
    inner: StubModel,
    token: CancelToken,
    calls: AtomicUsize,
}

impl TokenCounter for CancelDuring {
    fn count_tokens(&self, text: &str) -> Result<usize, SummaryError> {
        self.inner.count_tokens(text)
    }
}

impl SummaryClient for CancelDuring {
    fn generate(&self, req: &PolishRequest) -> Result<Generated, SummaryError> {
        if self.token.is_cancelled() {
            return Err(SummaryError::Cancelled);
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.token.cancel();
        self.inner.generate(req)
    }
    fn model_id(&self) -> String {
        "stub".into()
    }
    fn cancelled(&self) -> bool {
        self.token.is_cancelled()
    }
}

#[test]
fn a_cancelled_summary_is_never_returned_as_final() {
    let segments = segments_from(&[
        "Jeisil will send the updated pricing doc to the team by Friday.",
        "We agreed the starter package stays at forty nine dollars a month.",
    ]);
    let client = CancelDuring {
        inner: StubModel::new(|req| {
            Ok(match req.mode.as_str() {
                "map" => r#"{"narrative":"Jeisil will send the pricing doc.","actions":[],"decisions":[],"questions":[]}"#.to_string(),
                _ => "Jeisil will send the pricing doc.".to_string(),
            })
        }),
        token: CancelToken::new(),
        calls: AtomicUsize::new(0),
    };
    assert_eq!(summarize_segments(&segments, &client).unwrap_err(), SummaryError::Cancelled);
    assert_eq!(client.calls.load(Ordering::SeqCst), 1, "no model call after the cancel");
}

#[test]
fn a_cancelled_download_stops_between_chunks_and_keeps_the_partial() {
    const CHUNK: usize = 16 * 1024;
    const CHUNKS: usize = 64;
    let total = (CHUNK * CHUNKS) as u64;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Some(Ok(mut stream)) = listener.incoming().next() {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n"
            );
            if stream.write_all(head.as_bytes()).is_err() {
                return;
            }
            for _ in 0..CHUNKS {
                if stream.write_all(&[7u8; CHUNK]).is_err() || stream.flush().is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
        }
    });
    let dir = support::temp_dir("dl-cancel");
    let dest = dir.join("model.gguf");
    let stop = Arc::new(AtomicBool::new(false));
    let seen = Arc::new(AtomicU64::new(0));
    let (s2, seen2) = (stop.clone(), seen.clone());
    let cancel = move || s2.load(Ordering::SeqCst);
    let started = Instant::now();
    let got = tauri::async_runtime::block_on(wilson_voice_lib::models::download_file_cancellable(
        &[format!("http://127.0.0.1:{port}/model.gguf")],
        &dest,
        total,
        "0000",
        |done, _| {
            seen2.store(done, Ordering::SeqCst);
            if done >= (2 * CHUNK) as u64 {
                stop.store(true, Ordering::SeqCst);
            }
        },
        &cancel,
    ));
    assert_eq!(got.unwrap_err(), wilson_voice_lib::models::DOWNLOAD_CANCELLED);
    let last = seen.load(Ordering::SeqCst);
    assert!(last < total / 2, "kept downloading after the cancel: {last} of {total}");
    assert!(started.elapsed() < Duration::from_secs(2), "no retry/backoff after a cancel");
    assert!(!dest.exists(), "nothing at the final name");
    let partial = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .any(|e| e.file_name().to_string_lossy().contains("partial"));
    assert!(partial, "the .partial stays for the resume");
    let _ = std::fs::remove_dir_all(&dir);
}
