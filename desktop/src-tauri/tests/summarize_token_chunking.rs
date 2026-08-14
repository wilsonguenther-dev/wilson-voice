//! YV97 acceptance — "a synthetic 9,000-word meeting-shaped transcript (proper
//! nouns, disfluencies) is chunked using `count_tokens`, not the word-count
//! proxy, and no chunk overflows its budget without a truncate-and-warn marker."
//!
//! The falsifiable part is the middle clause, and it is stated here as a
//! DIFFERENTIAL rather than an assertion about implementation: the same
//! transcript is packed twice, once against the tokenizer and once against the
//! 1.3-tokens/word proxy the rewriter uses (`max_out_for`'s ratio), and the
//! proxy's chunks are shown to overflow the budget when measured for real. A
//! chunker that had quietly kept the proxy would fail this test, which is the
//! only way "it uses count_tokens" means anything (finding #35).

mod support;

use support::{meeting_token_count, StubModel};
use wilson_voice_lib::polish_protocol::TRUNCATION_MARKER;
use wilson_voice_lib::summarize::{plan_chunks, TranscriptLine};

/// The proxy the rewriter's `max_out_for` is written on: 1.3 tokens per word.
const WORD_PROXY: f64 = 1.3;

/// Meeting-shaped, on purpose: capitalized proper nouns and disfluencies, which
/// is exactly the vocabulary the proxy under-counts. Deterministic — no RNG, so
/// a failure is reproducible from the seed constants alone.
fn meeting_transcript(words_wanted: usize) -> Vec<TranscriptLine> {
    const NAMES: [&str; 6] = [
        "Priya",
        "Okonkwo",
        "Delacroix-Bell",
        "Yusuf",
        "Ratnayake",
        "O'Shaughnessy",
    ];
    const FILLERS: [&str; 5] = ["um", "you know", "I mean", "like", "sort of"];
    const BODY: [&str; 6] = [
        "we should move the onboarding review before the release",
        "the staging rollout is blocked on the migration",
        "nobody owns the escalation path yet",
        "the customer wants the export in markdown",
        "we are not shipping the calendar integration this cycle",
        "that number came from the wrong dashboard",
    ];
    let mut lines = Vec::new();
    let mut words = 0usize;
    let mut i = 0usize;
    while words < words_wanted {
        // Shaped like real multi-speaker ASR output: a speaker tag, proper
        // nouns, disfluencies, em dashes. This is the text finding #35 is about.
        let text = format!(
            "{}: {}, {} — I mean {} — {} {}",
            NAMES[i % NAMES.len()],
            FILLERS[i % FILLERS.len()],
            BODY[i % BODY.len()],
            NAMES[(i + 3) % NAMES.len()],
            FILLERS[(i + 2) % FILLERS.len()],
            BODY[(i + 3) % BODY.len()],
        );
        words += text.split_whitespace().count();
        lines.push(TranscriptLine::new(lines.len(), i as f64 * 6.0, text));
        i += 1;
    }
    lines
}

/// Pack by the 1.3-tokens/word proxy — the chunker this item replaces.
fn proxy_chunks(lines: &[TranscriptLine], budget: usize) -> Vec<Vec<TranscriptLine>> {
    let mut out: Vec<Vec<TranscriptLine>> = Vec::new();
    let mut current: Vec<TranscriptLine> = Vec::new();
    let mut estimate = 0.0f64;
    for line in lines {
        let cost = line.render().split_whitespace().count() as f64 * WORD_PROXY;
        if !current.is_empty() && estimate + cost > budget as f64 {
            out.push(std::mem::take(&mut current));
            estimate = 0.0;
        }
        estimate += cost;
        current.push(line.clone());
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn render(lines: &[TranscriptLine]) -> String {
    lines
        .iter()
        .map(TranscriptLine::render)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn summarize_chunks_a_9000_word_meeting_against_real_tokens() {
    let lines = meeting_transcript(9_000);
    let total_words: usize = lines
        .iter()
        .map(|l| l.text.split_whitespace().count())
        .sum();
    assert!(
        total_words >= 9_000,
        "the fixture is a 9,000-word meeting, not {total_words}"
    );

    let budget = 600usize;
    let model = StubModel::new(|_| Ok(String::new()));
    let chunks = plan_chunks(&lines, budget, &model).expect("a transcript chunks");

    assert!(
        model.token_count_calls() as usize >= lines.len(),
        "every line must be measured by the tokenizer, not estimated: {} calls for {} lines",
        model.token_count_calls(),
        lines.len()
    );
    assert!(
        chunks.len() > 1,
        "9,000 words do not fit one 600-token pass"
    );

    // The property: measured for real, no chunk is over budget.
    for (i, chunk) in chunks.iter().enumerate() {
        let real = meeting_token_count(&chunk.text());
        assert!(
            real <= budget,
            "chunk {i} is {real} tokens against a {budget} budget"
        );
    }

    // Nothing is lost and nothing is reordered by the packing.
    let packed: Vec<String> = chunks
        .iter()
        .flat_map(|c| c.lines.iter().map(|l| l.label.clone()))
        .collect();
    let expected: Vec<String> = lines.iter().map(|l| l.label.clone()).collect();
    assert_eq!(packed, expected, "chunking must not drop or reorder lines");

    // The differential: the same transcript packed by the word proxy overflows.
    let proxied = proxy_chunks(&lines, budget);
    let worst = proxied
        .iter()
        .map(|c| meeting_token_count(&render(c)))
        .max()
        .expect("chunks exist");
    assert!(
        worst > budget,
        "the 1.3-tokens/word proxy would have fit inside {budget} (worst {worst}) — \
         this fixture no longer distinguishes the two chunkers, which is the whole test"
    );
    println!(
        "summarize_token_chunking: {} lines / {total_words} words → {} token-sized chunks \
         (proxy-sized worst chunk {worst} tokens vs {budget} budget)",
        lines.len(),
        chunks.len()
    );
}

#[test]
fn summarize_truncates_and_warns_instead_of_overflowing() {
    // One transcript line longer than an entire pass — a 3-hour meeting with a
    // 20-minute uninterrupted monologue produces exactly this.
    let monster = TranscriptLine::new(0, 0.0, "Priya said the same thing again ".repeat(200));
    let short = TranscriptLine::new(1, 900.0, "we agreed to stop there");
    let budget = 80usize;
    let model = StubModel::new(|_| Ok(String::new()));
    let chunks = plan_chunks(&[monster, short], budget, &model).expect("chunks");

    let cut: Vec<_> = chunks.iter().filter(|c| c.truncated).collect();
    assert_eq!(
        cut.len(),
        1,
        "exactly the oversized line's chunk is flagged"
    );
    assert!(
        cut[0].text().contains(TRUNCATION_MARKER),
        "a truncated chunk says so IN THE TEXT: {:?}",
        cut[0].text()
    );
    for chunk in &chunks {
        let real = meeting_token_count(&chunk.text());
        assert!(
            real <= budget,
            "even the truncated chunk stays inside its budget ({real} > {budget})"
        );
    }
    // Truncate-and-WARN, not truncate-and-drop: the rest of the meeting is here.
    let labels: Vec<String> = chunks
        .iter()
        .flat_map(|c| c.lines.iter().map(|l| l.label.clone()))
        .collect();
    assert_eq!(labels, vec!["seg_0001".to_string(), "seg_0002".to_string()]);
}

#[test]
fn an_empty_transcript_is_an_error_not_an_empty_summary() {
    let model = StubModel::new(|_| Ok(String::new()));
    assert!(plan_chunks(&[], 600, &model).is_err());
}
