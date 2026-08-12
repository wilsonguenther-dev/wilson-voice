//! YV97 acceptance — "on a YV90 fixture, `cargo test --test summarize_e2e --
//! --nocapture` produces a narrative summary ≤250 words and a flat action list,
//! end to end."
//!
//! The fixture is YV90's `lecture-15min`: ~900 seconds of synthetic single-
//! speaker audio whose reference transcript is EXACT by construction (it was
//! rendered from those sentences by `say`), so its `meta.json` utterances are
//! the same segments YV93 would hand YV94. The corpus lives at
//! `~/yap-eval-corpus/meetings/` — durable, outside the repo — so with it absent
//! (CI, a fresh clone) this file prints one line and passes, exactly like
//! `tests/meeting_eval.rs`.
//!
//! **What "end to end" means here, precisely.** Chunking, grammar generation,
//! the constrained request, defensive parsing, the ported gate, the dedup merge,
//! REDUCE and the rendered Markdown all run for real over a real 15-minute
//! transcript. The MODEL is a scripted stand-in that answers out of the chunk it
//! was handed — because the properties under test are properties of the
//! pipeline, and a 1.5B GGUF is neither installed in CI nor deterministic. The
//! same path against the real sidecar is one `#[ignore]`d test below
//! (`summarize_e2e_real_sidecar`), which is also what YV99's on-camera
//! Wi-Fi-off demo drives.
//!
//! Nothing here opens a socket. The whole pipeline is a local process talking to
//! a local child over stdio — there is no network code on this path to disable.

mod support;

use std::path::PathBuf;

use serde::Deserialize;
use support::{bodies_in, labels_in, map_answer, segments_from, StubModel};
use wilson_voice_lib::summarize::{summarize_segments, MAX_NARRATIVE_WORDS};

const CORPUS_ENV: &str = "YAP_EVAL_CORPUS";
const CORPUS_HOME_RELATIVE: &str = "yap-eval-corpus/meetings";
const CORPUS_ABSENT: &str = "meeting eval corpus not found at ~/yap-eval-corpus/meetings, skipping";
const LECTURE: &str = "lecture-15min";

#[derive(Debug, Deserialize)]
struct Utterance {
    text: String,
}

#[derive(Debug, Deserialize)]
struct FixtureMeta {
    utterances: Vec<Utterance>,
}

fn corpus() -> Option<PathBuf> {
    let root = std::env::var(CORPUS_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .expect("a home directory")
                .join(CORPUS_HOME_RELATIVE)
        });
    if root.join(LECTURE).join("meta.json").is_file() {
        return Some(root);
    }
    println!("{CORPUS_ABSENT}");
    None
}

/// A scripted model that answers ONLY out of the chunk it was handed: the
/// narrative is the chunk's first line, each item is a clause from a line in the
/// same chunk, and every citation is that line's own id. Grounded by
/// construction — which is the point, since the gate would strip anything else
/// and the test would then be measuring the stub rather than the pipeline.
fn scripted() -> StubModel {
    StubModel::new(|req| {
        let labels = labels_in(&req.text);
        let bodies = bodies_in(&req.text);
        if req.mode == "reduce" {
            // REDUCE sees the chunk narratives and nothing else; stitch them
            // into one paragraph, which is the merge a model would be asked for.
            let head: Vec<&str> = req
                .text
                .lines()
                .filter(|l| !l.trim().is_empty())
                .collect();
            return Ok(head.join(" "));
        }
        let clause = |i: usize| -> String {
            bodies
                .get(i)
                .map(|b| b.split_whitespace().take(9).collect::<Vec<_>>().join(" "))
                .unwrap_or_default()
        };
        Ok(map_answer(
            &clause(0),
            &[(clause(1).as_str(), labels[1 % labels.len()].as_str())],
            &[(clause(2).as_str(), labels[2 % labels.len()].as_str())],
            &[],
        ))
    })
}

#[test]
fn summarize_e2e_over_the_yv90_lecture_fixture() {
    let Some(root) = corpus() else { return };
    let meta: FixtureMeta = serde_json::from_str(
        &std::fs::read_to_string(root.join(LECTURE).join("meta.json")).expect("fixture meta"),
    )
    .expect("fixture meta parses");

    let texts: Vec<&str> = meta.utterances.iter().map(|u| u.text.as_str()).collect();
    let segments = segments_from(&texts);
    let words: usize = texts.iter().map(|t| t.split_whitespace().count()).sum();
    println!(
        "summarize_e2e: {LECTURE} — {} segments, {words} words",
        segments.len()
    );

    let model = scripted();
    let summary = summarize_segments(&segments, &model).expect("the fixture summarizes");

    // The acceptance, stated as numbers.
    let narrative_words = summary.narrative.split_whitespace().count();
    assert!(
        narrative_words <= MAX_NARRATIVE_WORDS,
        "narrative is {narrative_words} words, over the {MAX_NARRATIVE_WORDS} cap"
    );
    assert!(narrative_words > 0, "a 15-minute lecture gets a narrative");
    assert!(!summary.actions.is_empty(), "a flat action list, not empty");
    assert!(
        summary.chunks > 1,
        "15 minutes needs more than one MAP pass"
    );
    assert_eq!(summary.truncated_chunks, 0, "nothing had to be truncated");
    assert_eq!(summary.model, "stub-1.5b");

    // Flat: one level of list, every item citing a real offset in the meeting.
    let last = segments.last().expect("segments").start_seconds;
    for item in summary.actions.iter().chain(summary.decisions.iter()) {
        assert!(!item.text.trim().is_empty());
        assert!((0.0..=last).contains(&item.start_seconds));
    }

    // The stored Markdown is what the app writes to `meetings.summary`.
    assert!(summary.markdown.contains("### Action items"));
    assert!(!summary.markdown.contains("###### "), "flat, not nested");

    println!(
        "---- stored summary ----\n{}\n------------------------",
        summary.markdown
    );
    println!(
        "summarize_e2e: {} chunks → {} actions, {} decisions, {} questions, {} items dropped",
        summary.chunks,
        summary.actions.len(),
        summary.decisions.len(),
        summary.questions.len(),
        summary.dropped_items
    );
}

/// The same path against the REAL sidecar and a real GGUF — the one YV99's
/// on-camera demo drives. Ignored by default: it needs a polish model installed
/// and takes minutes on a 15-minute transcript.
///
/// ```sh
/// cargo test --test summarize_e2e summarize_e2e_real_sidecar -- --ignored --nocapture
/// ```
#[test]
#[ignore]
fn summarize_e2e_real_sidecar() {
    let Some(root) = corpus() else { return };
    let binary = std::env::var("YAP_POLISH_BIN")
        .map(PathBuf::from)
        .expect("set YAP_POLISH_BIN to the built yap-polish binary");
    let model = std::env::var("YAP_POLISH_MODEL")
        .map(PathBuf::from)
        .expect("set YAP_POLISH_MODEL to a .gguf");
    let meta: FixtureMeta = serde_json::from_str(
        &std::fs::read_to_string(root.join(LECTURE).join("meta.json")).expect("fixture meta"),
    )
    .expect("fixture meta parses");
    let texts: Vec<&str> = meta.utterances.iter().map(|u| u.text.as_str()).collect();
    let segments = segments_from(&texts);

    let session = wilson_voice_lib::summarize::SidecarSession::spawn(&binary, &model, "real")
        .expect("the sidecar starts and loads its model");
    let client = wilson_voice_lib::summarize::SidecarSummaryClient::new(session);
    let started = std::time::Instant::now();
    let summary = summarize_segments(&segments, &client).expect("a real summary");
    println!(
        "summarize_e2e real: {} chunks in {:?}\n{}",
        summary.chunks,
        started.elapsed(),
        summary.markdown
    );
    assert!(summary.narrative.split_whitespace().count() <= MAX_NARRATIVE_WORDS);
}
