//! YV97 — meeting summary v1: MAP-stage extraction, token-based chunking, and
//! the polish stage's V1–V7 validators ported onto the summary path.
//!
//! Deliberately "basic" (O2 closed: no 7B for v1). What it produces is a
//! narrative "what was this about" plus a flat action list — not a
//! Granola-style multi-section template, which is deferred (finding #33).
//!
//! ## Two corrections from the audit, both load-bearing
//!
//! **Extraction happens in MAP, not REDUCE (finding #34).** The original plan
//! extracted actions and decisions only after log₂(n) lossy free-text REDUCE
//! merges had already discarded the evidence, with "preserve every quoted
//! commitment" repeated to a 1.5B model at every merge level and nothing
//! enforcing it. Here every chunk is extracted ONCE, where its segment ids are
//! local and enumerable, and REDUCE only merges narratives. Action lists are
//! merged by DEDUP, never re-generated, so a merge cannot reword or drop a
//! commitment.
//!
//! **Chunking is measured in tokens, not words (finding #35).** Meeting-shaped
//! text — proper nouns, disfluencies, ASR spellings — runs well above the
//! 1.3-tokens/word proxy the rewriter assumes, so the sidecar's own tokenizer is
//! asked (`count_tokens`), summarize gets its own fixed output budget rather
//! than `max_out_for`, and an oversized unit is truncated and flagged instead of
//! hard-erroring.
//!
//! ## Provenance is mechanical, not a model behaviour (finding #18)
//!
//! A grammar constrains a field's SHAPE, never its truth: nothing stops a model
//! emitting a plausible but invented segment id. So the GBNF is generated per
//! chunk with the evidence field as a literal enum over the ids actually in that
//! chunk ([`map_grammar`]), and [`parse_map_output`] drops any item whose id
//! falls outside it anyway — belt and braces, because a grammar this build
//! failed to apply must not silently become an unconstrained decode.
//!
//! Scoped to 22-A: segment ids are chronological transcript-segment labels
//! ([`seg_label`]) over YV94's segments. Evidence against real speaker turns is
//! a 23+ refinement that needs diarization.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::meetings::{format_offset, MeetingSegment};
use crate::polish_protocol::{
    fit_to_budget, parse_ready, parse_response_for, PolishRequest, PolishResponse, SUMMARIZE_MAP,
    SUMMARIZE_REDUCE, SUMMARY_MAP_MAX_OUT, SUMMARY_REDUCE_MAX_OUT, TRUNCATION_MARKER,
};

/// Token budget for ONE map chunk's input, measured in the model's own
/// vocabulary.
///
/// Sized against the sidecar's 4096-token context with room for the system turn,
/// the chat template and [`SUMMARY_MAP_MAX_OUT`] of output. It is a budget for
/// the transcript body only — the sidecar re-checks the assembled prompt and
/// truncates-and-warns if it still does not fit, so a wrong guess here degrades
/// rather than fails.
pub const MAP_CHUNK_TOKENS: usize = 1400;

/// Token budget for ONE reduce pass's input, measured the same way.
///
/// REDUCE is a model call like any other and gets the same treatment as MAP: its
/// input is measured against the real vocabulary and packed to fit. Joining
/// every chunk narrative into one request instead — the obvious implementation —
/// is unbounded in meeting length, and a three-hour meeting (YV91's hard cap) is
/// ~27 chunks of up to [`MAX_NARRATIVE_WORDS`] each, well past anything a 4k
/// context holds. The sidecar would take that prompt, cut its tail to fit, and
/// answer `truncated:true`; the stored "summary of the meeting" would in fact be
/// a summary of the meeting's first half. So narratives are folded
/// hierarchically instead (see [`reduce_narrative`]), and whatever truncation
/// still happens is carried out to the reader.
pub const REDUCE_CHUNK_TOKENS: usize = 1400;

/// How many fold levels [`reduce_narrative`] may run before it stops asking.
///
/// A 3-way fold reaches one narrative from 6,561 in eight levels, so this bounds
/// nothing a real meeting reaches — it exists so a counter that answers
/// nonsensically (every group holding one narrative) cannot spin forever.
const MAX_FOLD_LEVELS: usize = 8;

/// Deadlines. These are background work on a finished meeting, not a keystroke
/// path, so they are seconds rather than the rewriter's 1.2 s.
const COUNT_DEADLINE_MS: u64 = 5_000;
const MAP_DEADLINE_MS: u64 = 60_000;
const REDUCE_DEADLINE_MS: u64 = 90_000;
/// How long a freshly spawned summarize sidecar may take to load its model.
const READY_BUDGET: Duration = Duration::from_secs(30);

/// Hard word cap on the narrative. The prompt asks for it; this enforces it.
pub const MAX_NARRATIVE_WORDS: usize = 250;
/// Past this a "summary" is a re-transcript, not a summary — rejected outright
/// rather than trimmed, because trimming a runaway keeps its first half.
const NARRATIVE_RUNAWAY_WORDS: usize = MAX_NARRATIVE_WORDS * 2;
/// One extracted item is one sentence. Past this the model is narrating inside
/// a list.
const MAX_ITEM_WORDS: usize = 40;
/// Share of a summary's content words that must actually appear in the source
/// (the V3 retention floor, inverted for a summary — see [`grounded`]).
const GROUNDEDNESS_FLOOR: f64 = 0.80;

/// Rejections so far, by the ported gate. Kept SEPARATE from
/// `polish_rejected_total`: mixing summary rejections into a dictation metric
/// would make both numbers mean nothing.
static SUMMARY_REJECTED_TOTAL: AtomicU64 = AtomicU64::new(0);

/// How many summary outputs the gate has thrown away.
pub fn rejected_total() -> u64 {
    SUMMARY_REJECTED_TOTAL.load(Ordering::Relaxed)
}

fn reject(reason: &'static str) -> Option<String> {
    SUMMARY_REJECTED_TOTAL.fetch_add(1, Ordering::Relaxed);
    log::info!("summary rejected: {reason}");
    None
}

/// Anything that can go wrong on the way to a summary. Tags only — never text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryError {
    /// The meeting has no transcript to summarize.
    NoSegments,
    /// No polish model installed, or the sidecar could not be started.
    Unavailable,
    /// The sidecar answered something unusable, or not in time.
    Protocol,
}

impl SummaryError {
    pub fn tag(self) -> &'static str {
        match self {
            Self::NoSegments => "no_segments",
            Self::Unavailable => "unavailable",
            Self::Protocol => "protocol",
        }
    }
}

// ── the transcript, as the model sees it ────────────────────────────────────

/// `seg_0001` — the chronological label for the nth segment of a meeting.
///
/// Not the database id: a UUID spends tokens, is unenumerable in a grammar, and
/// means nothing to a 1.5B model. The label is positional, the position maps
/// back to the row, and the mapping never leaves this process.
pub fn seg_label(index: usize) -> String {
    format!("seg_{:04}", index + 1)
}

/// One transcript line on its way into a MAP pass.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptLine {
    pub label: String,
    pub start_seconds: f64,
    pub text: String,
}

impl TranscriptLine {
    pub fn new(index: usize, start_seconds: f64, text: impl Into<String>) -> Self {
        Self {
            label: seg_label(index),
            start_seconds,
            text: text.into(),
        }
    }

    /// `seg_0007: we agreed to ship on friday` — one line, always, so the
    /// chunker can cut on line boundaries and the model can cite a whole line.
    pub fn render(&self) -> String {
        format!("{}: {}", self.label, one_line(&self.text))
    }
}

/// YV94's stored segments, labelled in wall-clock order. Empty segments are
/// dropped: a blank line costs tokens and cites nothing.
pub fn transcript_lines(segments: &[MeetingSegment]) -> Vec<TranscriptLine> {
    segments
        .iter()
        .filter(|s| !s.text.trim().is_empty())
        .enumerate()
        .map(|(i, s)| TranscriptLine::new(i, s.start_seconds, s.text.trim()))
        .collect()
}

/// Collapse a segment onto one physical line — the line IS the unit here.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ── token-based chunking (finding #35) ──────────────────────────────────────

/// One MAP unit: consecutive transcript lines that fit one pass.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub lines: Vec<TranscriptLine>,
    /// A line in here was too long to fit a whole pass on its own and was cut.
    pub truncated: bool,
    /// The measured size of [`Chunk::text`] in model tokens, as counted by the
    /// sidecar — never a word-count proxy.
    pub tokens: usize,
}

impl Chunk {
    /// The chunk exactly as it is sent.
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(TranscriptLine::render)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The segment labels this chunk contains — the enum the grammar is built
    /// over, and the allowlist the parser enforces.
    pub fn labels(&self) -> Vec<String> {
        self.lines.iter().map(|l| l.label.clone()).collect()
    }
}

/// Something that can measure text in the model's vocabulary.
pub trait TokenCounter {
    fn count_tokens(&self, text: &str) -> Result<usize, SummaryError>;
}

/// Pack `lines` into chunks of at most `budget` tokens, measured by `counter`.
///
/// Each line is measured ONCE (n counts, not n² — a cumulative re-count per
/// added line would re-tokenize the whole meeting for every line in it), and the
/// pack allows one token per newline join, which is the conservative direction:
/// BPE merges across a join can only ever produce fewer tokens than the parts.
///
/// A single line that cannot fit a whole pass is truncated with a marker and the
/// chunk is flagged — never dropped, and never an error (finding #35).
pub fn plan_chunks(
    lines: &[TranscriptLine],
    budget: usize,
    counter: &dyn TokenCounter,
) -> Result<Vec<Chunk>, SummaryError> {
    if lines.is_empty() {
        return Err(SummaryError::NoSegments);
    }
    let budget = budget.max(32);
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut current: Vec<TranscriptLine> = Vec::new();
    let mut current_tokens = 0usize;
    let mut current_truncated = false;

    for line in lines {
        let mut line = line.clone();
        let mut cost = counter.count_tokens(&line.render())? + 1;
        let mut cut = false;
        if cost > budget {
            // One transcript line longer than a whole pass. Cut it against the
            // real tokenizer and say so IN THE TEXT, rather than refusing the
            // meeting or shortening it silently. Room for the marker is taken
            // out of the budget first, so saying so cannot itself overflow.
            let marker = counter.count_tokens(TRUNCATION_MARKER)? + 1;
            let mut count = |candidate: &str| {
                counter
                    .count_tokens(candidate)
                    .map_or(usize::MAX, |n| n + 1)
            };
            let (kept, was_cut) =
                fit_to_budget(&line.render(), budget.saturating_sub(marker), &mut count);
            line.text = format!(
                "{} {TRUNCATION_MARKER}",
                strip_label(
                    kept.trim_end_matches(TRUNCATION_MARKER).trim_end(),
                    &line.label
                )
            );
            cost = counter.count_tokens(&line.render())? + 1;
            cut = was_cut;
            log::warn!("summary: one transcript line exceeded a whole MAP pass and was truncated");
        }
        if !current.is_empty() && current_tokens + cost > budget {
            chunks.push(Chunk {
                lines: std::mem::take(&mut current),
                truncated: current_truncated,
                tokens: current_tokens,
            });
            current_tokens = 0;
            current_truncated = false;
        }
        current_tokens += cost;
        current_truncated |= cut;
        current.push(line);
    }
    if !current.is_empty() {
        chunks.push(Chunk {
            lines: current,
            truncated: current_truncated,
            tokens: current_tokens,
        });
    }
    Ok(chunks)
}

/// Drop the `seg_NNNN: ` prefix a rendered line carries, so a truncated render
/// can go back into a [`TranscriptLine`] without doubling its label.
fn strip_label(rendered: &str, label: &str) -> String {
    rendered
        .strip_prefix(label)
        .and_then(|rest| rest.strip_prefix(": "))
        .unwrap_or(rendered)
        .to_string()
}

// ── the grammar (findings #17 + #18) ────────────────────────────────────────

/// GBNF for one MAP pass over `labels`.
///
/// The `evid` rule is a literal enum over the segment ids present in THIS chunk.
/// That is the whole point (finding #18): a grammar constrains a field's shape,
/// never its truth, so constraining the shape to *exactly the ids that exist
/// here* is what turns provenance from a model behaviour into a mechanical
/// guarantee. `None` when there is nothing to cite — an empty alternation is not
/// a grammar, and a request carrying one would be refused by the sidecar.
pub fn map_grammar(labels: &[String]) -> Option<String> {
    if labels.is_empty() {
        return None;
    }
    let evid = labels
        .iter()
        .map(|l| format!("\"\\\"{l}\\\"\""))
        .collect::<Vec<_>>()
        .join(" | ");
    Some(format!(
        concat!(
            "root ::= \"{{\" ws \"\\\"narrative\\\"\" ws \":\" ws string ws \",\"\n",
            "         ws \"\\\"actions\\\"\" ws \":\" ws items ws \",\"\n",
            "         ws \"\\\"decisions\\\"\" ws \":\" ws items ws \",\"\n",
            "         ws \"\\\"questions\\\"\" ws \":\" ws items ws \"}}\"\n",
            "items ::= \"[\" ws \"]\" | \"[\" ws item (ws \",\" ws item)* ws \"]\"\n",
            "item ::= \"{{\" ws \"\\\"text\\\"\" ws \":\" ws string ws \",\" ws \"\\\"segment\\\"\" ws \":\" ws evid ws \"}}\"\n",
            "string ::= \"\\\"\" char* \"\\\"\"\n",
            "char ::= [^\"\\\\] | \"\\\\\" [\"\\\\/bfnrt]\n",
            "evid ::= {evid}\n",
            "ws ::= [ \\t\\n]*\n",
        ),
        evid = evid
    ))
}

// ── defensive parsing of the MAP answer ─────────────────────────────────────

/// One extracted item and the segment it came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractedItem {
    pub text: String,
    pub segment: String,
}

/// What one MAP pass produced, after the grammar, the id allowlist and the
/// ported validators have all had their say.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MapExtract {
    pub narrative: String,
    pub actions: Vec<ExtractedItem>,
    pub decisions: Vec<ExtractedItem>,
    pub questions: Vec<ExtractedItem>,
    /// Items thrown away: an invented segment id, or a failed validator.
    pub dropped: usize,
}

/// Parse one MAP answer.
///
/// Never panics, never trusts, and never requires the answer to be complete: a
/// decode that hit its token budget mid-array still yields every item that
/// closed. Items are kept only when the cited id is in `labels` — the chunk's
/// own ids — and only when the text survives [`validate_item`] against `source`,
/// the chunk text it claims to summarize.
pub fn parse_map_output(raw: &str, labels: &[String], source: &str) -> MapExtract {
    let raw = strip_fences(raw);
    let mut out = MapExtract {
        narrative: recover_narrative(&raw).unwrap_or_default(),
        ..MapExtract::default()
    };
    for (bucket, item) in recover_items(&raw) {
        if !labels.iter().any(|l| l == &item.segment) {
            // A plausible id that does not exist in this chunk. The grammar
            // should have made this impossible; the drop is what makes it true
            // even when the grammar did not apply.
            out.dropped += 1;
            continue;
        }
        let Some(text) = validate_item(source, &item.text) else {
            out.dropped += 1;
            continue;
        };
        let item = ExtractedItem {
            text,
            segment: item.segment,
        };
        match bucket {
            Bucket::Actions => out.actions.push(item),
            Bucket::Decisions => out.decisions.push(item),
            Bucket::Questions => out.questions.push(item),
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bucket {
    Actions,
    Decisions,
    Questions,
}

/// Strip a markdown fence a model wrapped its JSON in. The grammar forbids one;
/// this is for the path where the grammar did not apply.
fn strip_fences(raw: &str) -> String {
    let trimmed = raw.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed.to_string();
    };
    let body = rest.split_once('\n').map(|(_, b)| b).unwrap_or("");
    body.trim_end().trim_end_matches("```").trim().to_string()
}

/// The `narrative` value, if a complete string for it is present.
fn recover_narrative(raw: &str) -> Option<String> {
    let key = "\"narrative\"";
    let at = raw.find(key)? + key.len();
    let rest = raw[at..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let bytes: Vec<char> = rest.chars().collect();
    if bytes.first() != Some(&'"') {
        return None;
    }
    let mut value = String::new();
    let mut escaped = false;
    for c in bytes.iter().skip(1) {
        if escaped {
            value.push(match c {
                'n' => '\n',
                't' => '\t',
                other => *other,
            });
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            // Closed properly: this is a complete value.
            '"' => return Some(value),
            other => value.push(*other),
        }
    }
    // Truncated mid-string. Keep what closed cleanly at a sentence, or nothing.
    let cut = value.rfind(['.', '!', '?'])?;
    Some(value[..=cut].to_string())
}

/// Every complete `{…}` item in the answer, tagged with the array it appeared
/// in.
///
/// A linear scan rather than a JSON parse, because the answer may be truncated:
/// `serde_json` on a half-written array yields nothing at all, while the array
/// KEY ordering is fixed by the grammar, so the last key seen before an object
/// tells us which list it belongs to. Objects that never close are dropped —
/// that is exactly the half-written item nobody should trust.
fn recover_items(raw: &str) -> Vec<(Bucket, ExtractedItem)> {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = Vec::new();
    let mut bucket = None;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut start: Option<usize> = None;

    for i in 0..chars.len() {
        let c = chars[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                // Only key positions matter, and only outside any item — an
                // item's own `"text"` value can say the word "actions".
                if depth <= 1 {
                    for (name, which) in [
                        ("\"actions\"", Bucket::Actions),
                        ("\"decisions\"", Bucket::Decisions),
                        ("\"questions\"", Bucket::Questions),
                    ] {
                        if chars[i..].starts_with(&name.chars().collect::<Vec<_>>()[..]) {
                            bucket = Some(which);
                        }
                    }
                }
                in_string = true;
            }
            '{' => {
                depth += 1;
                if depth == 2 {
                    start = Some(i);
                }
            }
            '}' => {
                if depth == 2 {
                    if let (Some(from), Some(which)) = (start.take(), bucket) {
                        let text: String = chars[from..=i].iter().collect();
                        if let Ok(item) = serde_json::from_str::<ExtractedItem>(&text) {
                            out.push((which, item));
                        }
                    }
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    out
}

// ── the ported gate (V1–V7) ─────────────────────────────────────────────────

/// The polish stage's V1–V7, ported to one extracted item.
///
/// Reused verbatim where the check means the same thing for a summary — V4
/// invented digits, V5 invented `@`-address/URL, V6 template leak and assistant
/// preamble, V7 script drift are all the same failure. V2 and V3 are REINTERPRETED,
/// because a summary is supposed to be shorter than its input and is supposed to
/// drop content: the length band becomes a per-item sentence cap, and the ≥80%
/// content-word RETENTION floor becomes a ≥80% GROUNDEDNESS floor — the share of
/// the summary's own content words that actually occur in the source.
pub fn validate_item(source: &str, item: &str) -> Option<String> {
    let item = item.trim();
    // V1 — never store nothing.
    if item.is_empty() {
        return reject("v1_empty");
    }
    // V2 — one item is one sentence.
    if item.split_whitespace().count() > MAX_ITEM_WORDS {
        return reject("v2_runaway");
    }
    common_gates(source, item)?;
    Some(item.to_string())
}

/// The same gate over the narrative, plus the summary-shaped length checks.
pub fn validate_narrative(source: &str, narrative: &str) -> Option<String> {
    let narrative = narrative.trim();
    if narrative.is_empty() {
        return reject("v1_empty");
    }
    let words = narrative.split_whitespace().count();
    // V2 — a "summary" this long is a re-transcript. Rejected rather than
    // trimmed: trimming a runaway keeps its first half and hides the failure.
    if words > NARRATIVE_RUNAWAY_WORDS {
        return reject("v2_runaway");
    }
    let source_words = source.split_whitespace().count();
    if source_words > MAX_NARRATIVE_WORDS && words >= source_words {
        return reject("v2_not_a_summary");
    }
    common_gates(source, narrative)?;
    Some(clamp_words(narrative, MAX_NARRATIVE_WORDS))
}

/// V3–V7, shared by both paths.
fn common_gates(source: &str, output: &str) -> Option<()> {
    // V3 (inverted) — groundedness. The polish validator asks how much of the
    // INPUT survived; a summary is allowed to drop almost all of it, so the
    // honest question is the other one: how much of what the summary SAYS is
    // actually in the source.
    if grounded(source, output) < GROUNDEDNESS_FLOOR {
        return reject("v3_ungrounded").map(|_| ());
    }
    // V4 — invented numbers, times, dates, prices.
    let known_digits = crate::polish::digit_runs(source);
    if crate::polish::digit_runs(output)
        .iter()
        .any(|d| !known_digits.contains(d))
    {
        return reject("v4_invented_digits").map(|_| ());
    }
    // V5 — invented contact details.
    let known_contacts = crate::polish::contact_tokens(source);
    if crate::polish::contact_tokens(output)
        .iter()
        .any(|c| !known_contacts.contains(c))
    {
        return reject("v5_invented_contact").map(|_| ());
    }
    // V6 — template leak or assistant behaviour.
    if crate::polish::TEMPLATE_MARKERS
        .iter()
        .any(|m| output.contains(m))
    {
        return reject("v6_template_leak").map(|_| ());
    }
    let opening = output.trim_start().to_lowercase();
    if crate::polish::ASSISTANT_PREAMBLES
        .iter()
        .any(|p| opening.starts_with(p))
    {
        return reject("v6_preamble").map(|_| ());
    }
    // V7 — language / script drift.
    if crate::polish::non_ascii_ratio(output) - crate::polish::non_ascii_ratio(source)
        > crate::polish::MAX_NON_ASCII_DRIFT
    {
        return reject("v7_script_drift").map(|_| ());
    }
    Some(())
}

/// Share of the OUTPUT's DISTINCT content words that occur in the source — V3,
/// inverted for a summary.
///
/// Distinct, not a multiset (which is how the polish stage counts retention):
/// repetition is normal in prose — a two-paragraph summary naming the same
/// project four times says nothing false about a transcript that named it once —
/// so counting repeats would punish ordinary writing while catching no
/// fabrication at all. What this asks is the only question that matters here:
/// is there a word in this summary that the meeting never contained?
fn grounded(source: &str, output: &str) -> f64 {
    let mut claimed = crate::polish::content_words(output);
    claimed.sort();
    claimed.dedup();
    if claimed.is_empty() {
        return 1.0;
    }
    // A set, not the source vector: the hierarchical fold grounds every level
    // against the WHOLE meeting, so a linear scan per claimed word would make the
    // gate quadratic in a three-hour transcript.
    let have: std::collections::HashSet<String> =
        crate::polish::content_words(source).into_iter().collect();
    let found = claimed.iter().filter(|w| have.contains(*w)).count();
    found as f64 / claimed.len() as f64
}

/// Cut to at most `cap` words, at the last sentence end inside the cap when
/// there is one — a summary that stops mid-clause reads as a bug.
pub fn clamp_words(text: &str, cap: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= cap {
        return words.join(" ");
    }
    let head = words[..cap].join(" ");
    match head.rfind(['.', '!', '?']) {
        Some(at) => head[..=at].to_string(),
        None => format!("{head}…"),
    }
}

// ── merge (dedup, never re-generation — finding #34) ────────────────────────

/// The whole meeting's extraction, merged.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MergedExtract {
    pub actions: Vec<ExtractedItem>,
    pub decisions: Vec<ExtractedItem>,
    pub questions: Vec<ExtractedItem>,
    /// Chunk narratives, in order — the ONLY input REDUCE gets.
    pub narratives: Vec<String>,
    pub dropped: usize,
    pub duplicates: usize,
}

/// Merge per-chunk extractions into one meeting-level list.
///
/// By DEDUP, not re-generation (finding #34): the merge never asks a model
/// anything, so it cannot reword a commitment, drop one, or invent one. The
/// first occurrence wins, which keeps the earliest citation in the meeting.
pub fn merge_extracts(extracts: &[MapExtract]) -> MergedExtract {
    let mut merged = MergedExtract::default();
    for extract in extracts {
        merged.dropped += extract.dropped;
        if !extract.narrative.trim().is_empty() {
            merged.narratives.push(extract.narrative.trim().to_string());
        }
        for (src, dst) in [
            (&extract.actions, &mut merged.actions),
            (&extract.decisions, &mut merged.decisions),
            (&extract.questions, &mut merged.questions),
        ] {
            for item in src {
                if dst.iter().any(|kept| same_item(&kept.text, &item.text)) {
                    merged.duplicates += 1;
                    continue;
                }
                dst.push(item.clone());
            }
        }
    }
    merged
}

/// Two items are the same commitment when their content words match. Deliberately
/// conservative — punctuation and casing differ across chunks, wording does not.
fn same_item(a: &str, b: &str) -> bool {
    fn key(text: &str) -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect()
    }
    key(a) == key(b)
}

// ── the pipeline ────────────────────────────────────────────────────────────

/// One answer from the model, and whether the sidecar had to cut the request to
/// make it fit.
///
/// The flag is on the answer rather than in a log line on purpose. The sidecar
/// truncates an oversized prompt and reports `truncated:true` precisely so the
/// caller can say so; a `log::warn!` and nothing else means the user is handed a
/// summary that quietly omits part of their meeting, which is the failure this
/// whole path exists to avoid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    pub text: String,
    /// The sidecar cut this request's input to fit its context.
    pub truncated: bool,
}

impl Generated {
    /// An answer to a request that fitted whole.
    pub fn whole(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            truncated: false,
        }
    }
}

/// A model that can answer a summarize request and count tokens.
pub trait SummaryClient: TokenCounter {
    /// Run one request and return its text, plus whether the sidecar had to
    /// truncate the input to fit. `Err` for anything else at all.
    fn generate(&self, req: &PolishRequest) -> Result<Generated, SummaryError>;
    /// A stable id for what produced the summary, stored beside it.
    fn model_id(&self) -> String;
}

/// One finished summary.
#[derive(Debug, Clone, PartialEq)]
pub struct MeetingSummary {
    /// What gets written to `meetings.summary` — narrative plus the flat lists.
    pub markdown: String,
    pub narrative: String,
    pub actions: Vec<SummaryItem>,
    pub decisions: Vec<SummaryItem>,
    pub questions: Vec<SummaryItem>,
    pub chunks: usize,
    pub truncated_chunks: usize,
    /// A pass — MAP or REDUCE — was handed more than its context could hold and
    /// the input was cut. Distinct from [`MeetingSummary::truncated_chunks`],
    /// which counts a single transcript LINE too long for a whole pass: this one
    /// says the model never saw part of what it was asked to summarize, so the
    /// reader has to be told (see [`render_summary`]).
    pub truncated: bool,
    pub dropped_items: usize,
    pub model: String,
}

/// An item with its citation resolved back to a wall-clock offset.
#[derive(Debug, Clone, PartialEq)]
pub struct SummaryItem {
    pub text: String,
    pub segment: String,
    pub start_seconds: f64,
}

/// Summarize a meeting's segments end to end: chunk, MAP, merge, REDUCE, render.
///
/// Every stage fails toward LESS output rather than wrong output — a chunk whose
/// MAP pass fails contributes nothing and does not stop the others, and a REDUCE
/// that fails or is rejected falls back to the chunk narratives it was given.
pub fn summarize_segments(
    segments: &[MeetingSegment],
    client: &dyn SummaryClient,
) -> Result<MeetingSummary, SummaryError> {
    let lines = transcript_lines(segments);
    if lines.is_empty() {
        return Err(SummaryError::NoSegments);
    }
    let chunks = plan_chunks(&lines, MAP_CHUNK_TOKENS, client)?;
    let full_text = lines
        .iter()
        .map(TranscriptLine::render)
        .collect::<Vec<_>>()
        .join("\n");

    let mut extracts = Vec::with_capacity(chunks.len());
    let mut truncated_chunks = 0usize;
    let mut truncated = false;
    for chunk in &chunks {
        if chunk.truncated {
            truncated_chunks += 1;
        }
        let labels = chunk.labels();
        let source = chunk.text();
        let Some(grammar) = map_grammar(&labels) else {
            continue;
        };
        let req = PolishRequest::summarize(
            next_id(),
            SUMMARIZE_MAP,
            SUMMARY_MAP_MAX_OUT,
            MAP_DEADLINE_MS,
            source.clone(),
            Some(grammar),
        );
        match client.generate(&req) {
            // Parsed defensively: a MAP pass that hit its token budget
            // mid-array still contributes every item that closed.
            Ok(answer) => {
                truncated |= answer.truncated;
                let mut extract = parse_map_output(&answer.text, &labels, &source);
                // The chunk narrative goes through the same gate the items do.
                // `parse_map_output` leaves it raw on purpose — it is a parser,
                // and the source a narrative is grounded against is a decision
                // for the caller, not for the recovery scan.
                extract.narrative =
                    validate_narrative(&source, &extract.narrative).unwrap_or_default();
                extracts.push(extract);
            }
            Err(e) => log::warn!("summary: a MAP pass failed ({}) — continuing", e.tag()),
        }
    }
    let merged = merge_extracts(&extracts);

    let (narrative, reduce_truncated) = reduce_narrative(&merged.narratives, &full_text, client);
    truncated |= reduce_truncated;
    let offsets: Vec<(String, f64)> = lines
        .iter()
        .map(|l| (l.label.clone(), l.start_seconds))
        .collect();
    let resolve = |items: &[ExtractedItem]| -> Vec<SummaryItem> {
        items
            .iter()
            .map(|i| SummaryItem {
                text: i.text.clone(),
                segment: i.segment.clone(),
                start_seconds: offsets
                    .iter()
                    .find(|(label, _)| label == &i.segment)
                    .map(|(_, at)| *at)
                    .unwrap_or(0.0),
            })
            .collect()
    };
    let actions = resolve(&merged.actions);
    let decisions = resolve(&merged.decisions);
    let questions = resolve(&merged.questions);
    let markdown = render_summary(
        &narrative,
        &actions,
        &decisions,
        &questions,
        truncated_chunks,
        chunks.len(),
        truncated,
    );
    Ok(MeetingSummary {
        markdown,
        narrative,
        actions,
        decisions,
        questions,
        chunks: chunks.len(),
        truncated_chunks,
        truncated,
        dropped_items: merged.dropped,
        model: client.model_id(),
    })
}

/// Pack `narratives` into fold groups that each fit `budget` measured tokens.
///
/// The same shape as [`plan_chunks`] and for the same reason: each narrative is
/// measured ONCE, and the blank-line join between two of them is charged two
/// tokens, which is the conservative direction. A single narrative that will not
/// fit a whole pass on its own is cut with [`TRUNCATION_MARKER`] rather than
/// dropped, and the caller is told — that is finding #35's "truncate AND warn",
/// applied to REDUCE instead of left to the sidecar to do silently.
fn plan_fold(
    narratives: &[String],
    budget: usize,
    counter: &dyn TokenCounter,
) -> Result<(Vec<Vec<String>>, bool), SummaryError> {
    let budget = budget.max(32);
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut current_tokens = 0usize;
    let mut cut_any = false;

    for narrative in narratives {
        let mut narrative = narrative.clone();
        let mut cost = counter.count_tokens(&narrative)? + 2;
        if cost > budget {
            let marker = counter.count_tokens(TRUNCATION_MARKER)? + 2;
            let mut count = |candidate: &str| {
                counter
                    .count_tokens(candidate)
                    .map_or(usize::MAX, |n| n + 2)
            };
            let (kept, was_cut) =
                fit_to_budget(&narrative, budget.saturating_sub(marker), &mut count);
            narrative = format!(
                "{} {TRUNCATION_MARKER}",
                kept.trim_end_matches(TRUNCATION_MARKER).trim_end()
            );
            cost = counter.count_tokens(&narrative)? + 2;
            cut_any |= was_cut;
            log::warn!("summary: one chunk narrative exceeded a whole REDUCE pass and was cut");
        }
        if !current.is_empty() && current_tokens + cost > budget {
            groups.push(std::mem::take(&mut current));
            current_tokens = 0;
        }
        current_tokens += cost;
        current.push(narrative);
    }
    if !current.is_empty() {
        groups.push(current);
    }
    Ok((groups, cut_any))
}

/// REDUCE: fold the chunk narratives down to one, a budget's worth at a time.
///
/// A single-chunk meeting skips the model entirely — there is nothing to merge,
/// and asking a 1.5B model to rewrite a summary it just wrote only invites drift.
///
/// Everything else is folded HIERARCHICALLY rather than joined into one request
/// (the finding this function exists to fix). Joining is unbounded in meeting
/// length: at YV91's three-hour cap a meeting is ~27 chunks, each contributing up
/// to [`MAX_NARRATIVE_WORDS`], and the sidecar's own REDUCE budget is `n_ctx`
/// minus the output reservation. Handed that, the sidecar cuts the tail and
/// answers `truncated:true` — so the narrative would describe the FRONT of the
/// meeting while being stored as the summary of all of it. Folding keeps every
/// pass inside [`REDUCE_CHUNK_TOKENS`], measured with the model's own tokenizer,
/// so the tail is merged rather than dropped.
///
/// Returns the narrative and whether anything was truncated on the way — which
/// the caller renders, because a shortened input the reader never hears about is
/// exactly the silent omission this is fixing.
///
/// Every pass fails toward LESS output rather than wrong output: a rejected or
/// failed REDUCE keeps the narratives it was given, which are already validated.
fn reduce_narrative(
    narratives: &[String],
    source: &str,
    client: &dyn SummaryClient,
) -> (String, bool) {
    let mut level: Vec<String> = narratives
        .iter()
        .filter(|n| !n.trim().is_empty())
        .cloned()
        .collect();
    match level.len() {
        0 => return (String::new(), false),
        1 => return (clamp_words(&level[0], MAX_NARRATIVE_WORDS), false),
        _ => {}
    }
    let mut truncated = false;

    for _ in 0..MAX_FOLD_LEVELS {
        let (groups, cut) = match plan_fold(&level, REDUCE_CHUNK_TOKENS, client) {
            Ok(planned) => planned,
            Err(e) => {
                // The tokenizer is the only thing that can say what fits. Without
                // it, refuse to guess at a prompt size and keep the MAP output.
                log::warn!(
                    "summary: could not measure the REDUCE input ({}) — keeping the MAP narratives",
                    e.tag()
                );
                return (clamp_words(&level.join("\n\n"), MAX_NARRATIVE_WORDS), true);
            }
        };
        truncated |= cut;
        if groups.len() >= level.len() && level.len() > 1 {
            // Every group holds one narrative, so another level would merge
            // nothing and this would spin. Stop, keep what fits one narrative's
            // cap, and say that it is not all of it.
            break;
        }
        let mut next: Vec<String> = Vec::with_capacity(groups.len());
        for group in &groups {
            let joined = group.join("\n\n");
            let fallback = clamp_words(&joined, MAX_NARRATIVE_WORDS);
            if group.len() < 2 {
                // Nothing to merge in this group — pass it through untouched
                // rather than spend a pass letting a 1.5B model reword it.
                next.push(joined);
                continue;
            }
            let req = PolishRequest::summarize(
                next_id(),
                SUMMARIZE_REDUCE,
                SUMMARY_REDUCE_MAX_OUT,
                REDUCE_DEADLINE_MS,
                joined,
                None,
            );
            match client.generate(&req) {
                Ok(answer) => {
                    // Belt and braces: the fold is sized so this cannot happen,
                    // but if the sidecar still had to cut, the reader hears it.
                    truncated |= answer.truncated;
                    next.push(validate_narrative(source, &answer.text).unwrap_or(fallback));
                }
                Err(e) => {
                    log::warn!(
                        "summary: a REDUCE pass failed ({}) — keeping its MAP narratives",
                        e.tag()
                    );
                    next.push(fallback);
                }
            }
        }
        if next.len() == 1 {
            return (clamp_words(&next[0], MAX_NARRATIVE_WORDS), truncated);
        }
        level = next;
    }
    // Levels exhausted, or a fold that could not converge. Keep the front of what
    // we have and flag it — the flag is the whole difference between a short
    // summary and a summary that lies about its coverage.
    (clamp_words(&level.join("\n\n"), MAX_NARRATIVE_WORDS), true)
}

/// The stored summary: a narrative, then flat lists with citations.
///
/// Markdown, because that is what `meetings.summary` renders as in the app and
/// in YV94's export. Deliberately not a multi-section template (finding #33).
pub fn render_summary(
    narrative: &str,
    actions: &[SummaryItem],
    decisions: &[SummaryItem],
    questions: &[SummaryItem],
    truncated_chunks: usize,
    chunks: usize,
    truncated: bool,
) -> String {
    let mut out = String::new();
    if !narrative.trim().is_empty() {
        out.push_str(narrative.trim());
        out.push_str("\n\n");
    }
    for (heading, items) in [
        ("### Action items", actions),
        ("### Decisions", decisions),
        ("### Open questions", questions),
    ] {
        if items.is_empty() {
            continue;
        }
        out.push_str(heading);
        out.push_str("\n\n");
        for item in items {
            out.push_str(&format!(
                "- {} ({})\n",
                item.text,
                format_offset(item.start_seconds)
            ));
        }
        out.push('\n');
    }
    if truncated_chunks > 0 {
        // Said out loud, because a silently shortened input is a summary that
        // quietly omits part of a meeting.
        out.push_str(&format!(
            "_{truncated_chunks} of {chunks} parts of this meeting were too long for one pass and were truncated._\n"
        ));
    }
    if truncated {
        // The other half of the same principle. The line above counts transcript
        // LINES the chunker had to cut; this one is the model saying it never saw
        // all of what it was asked to summarize. A `log::warn!` reaches nobody who
        // reads the summary, so the flag has to land here.
        out.push_str(
            "_Part of this meeting did not fit the model in one pass and was shortened before summarizing, so the summary above may not cover all of it._\n",
        );
    }
    if out.trim().is_empty() {
        // An honest empty answer beats an invented one.
        return "_No summary could be produced for this meeting._".to_string();
    }
    out.trim_end().to_string()
}

// ── the sidecar session ─────────────────────────────────────────────────────

/// Request ids for this process's summarize sessions.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// A `yap-polish` process dedicated to ONE summarization job.
///
/// Deliberately NOT the warm pool the dictation path uses, for two reasons that
/// are both about the dictation path rather than about this one:
///
/// * the pool serializes on one mutex, and a MAP pass runs for seconds — a
///   dictation that landed mid-summary would block on that lock long past its
///   own deadline, which is exactly the livelock YV75 removed;
/// * the pool's whole latency story is the KV prefix cache holding the static
///   polish system prompt. Interleaving a summarize prompt evicts it, so every
///   dictation after a summary would pay a full prefill.
///
/// So the summarizer brings its own process, and takes it down when it is done.
pub struct SidecarSession {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    model: String,
}

impl SidecarSession {
    /// Spawn the staged sidecar against `model_path` and wait for its readiness
    /// line. Unlike the dictation path this DOES wait — a summary is background
    /// work with no keystroke behind it.
    pub fn spawn(binary: &Path, model_path: &Path, model_id: &str) -> Result<Self, SummaryError> {
        let mut child = Command::new(binary)
            .arg("--model")
            .arg(model_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| SummaryError::Unavailable)?;
        let stdin = child.stdin.take().ok_or(SummaryError::Unavailable)?;
        let stdout = child.stdout.take().ok_or(SummaryError::Unavailable)?;
        let stderr = child.stderr.take().ok_or(SummaryError::Unavailable)?;
        let (tx, lines) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(hello) = parse_ready(&line) {
                    let _ = ready_tx.send(hello.model_loaded);
                    continue;
                }
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        // Drained, not discarded: an unread stderr pipe fills and blocks the
        // child mid-write.
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                log::debug!(
                    "yap-polish(summary): {}",
                    line.chars().take(300).collect::<String>()
                );
            }
        });
        let session = Self {
            child,
            stdin,
            lines,
            model: model_id.to_string(),
        };
        match ready_rx.recv_timeout(READY_BUDGET) {
            Ok(true) => Ok(session),
            _ => Err(SummaryError::Unavailable),
        }
    }

    /// The staged sidecar for the installed polish model, or `None` when the
    /// stage is off or the model is not fully downloaded.
    pub fn for_installed_model(model_id: &str) -> Result<Self, SummaryError> {
        let model = crate::models::polish_model(model_id.trim())
            .filter(|m| crate::models::is_polish_downloaded(m))
            .ok_or(SummaryError::Unavailable)?;
        let path: PathBuf = crate::models::polish_model_path(model);
        let exe = std::env::current_exe().map_err(|_| SummaryError::Unavailable)?;
        let binary = exe
            .parent()
            .ok_or(SummaryError::Unavailable)?
            .join("yap-polish");
        if !binary.is_file() {
            return Err(SummaryError::Unavailable);
        }
        Self::spawn(&binary, &path, &model.id)
    }

    fn exchange(&mut self, req: &PolishRequest) -> Result<PolishResponse, SummaryError> {
        let line = serde_json::to_string(req).map_err(|_| SummaryError::Protocol)?;
        writeln!(self.stdin, "{line}").map_err(|_| SummaryError::Unavailable)?;
        self.stdin.flush().map_err(|_| SummaryError::Unavailable)?;
        let deadline = Instant::now() + Duration::from_millis(req.deadline_ms.max(1));
        loop {
            let left = deadline
                .checked_duration_since(Instant::now())
                .ok_or(SummaryError::Protocol)?;
            match self.lines.recv_timeout(left) {
                Ok(line) => {
                    if let Some(response) = parse_response_for(&line, req.id) {
                        return Ok(response);
                    }
                }
                Err(RecvTimeoutError::Timeout) => return Err(SummaryError::Protocol),
                Err(RecvTimeoutError::Disconnected) => return Err(SummaryError::Unavailable),
            }
        }
    }
}

impl Drop for SidecarSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The production client: one dedicated sidecar, for the length of one job.
pub struct SidecarSummaryClient {
    session: std::sync::Mutex<SidecarSession>,
    model: String,
}

impl SidecarSummaryClient {
    pub fn new(session: SidecarSession) -> Self {
        let model = session.model.clone();
        Self {
            session: std::sync::Mutex::new(session),
            model,
        }
    }

    fn call(&self, req: &PolishRequest) -> Result<PolishResponse, SummaryError> {
        let mut session = self.session.lock().map_err(|_| SummaryError::Protocol)?;
        session.exchange(req)
    }
}

impl TokenCounter for SidecarSummaryClient {
    fn count_tokens(&self, text: &str) -> Result<usize, SummaryError> {
        let req = PolishRequest::count_tokens(next_id(), COUNT_DEADLINE_MS, text.to_string());
        self.call(&req)?
            .token_count()
            .map(|n| n as usize)
            .ok_or(SummaryError::Protocol)
    }
}

impl SummaryClient for SidecarSummaryClient {
    fn generate(&self, req: &PolishRequest) -> Result<Generated, SummaryError> {
        let response = self.call(req)?;
        let truncated = response.was_truncated();
        if truncated {
            log::warn!("summary: the sidecar truncated a pass to fit its context");
        }
        response
            .into_text()
            .map(|text| Generated { text, truncated })
            .ok_or(SummaryError::Protocol)
    }

    fn model_id(&self) -> String {
        self.model.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A counter with a meeting-shaped vocabulary: one token per word, plus one
    /// per capital letter (proper nouns) and one per hyphen/apostrophe — i.e.
    /// well above the rewriter's 1.3-tokens/word proxy, which is the whole
    /// reason finding #35 exists.
    struct FakeCounter;

    impl TokenCounter for FakeCounter {
        fn count_tokens(&self, text: &str) -> Result<usize, SummaryError> {
            Ok(text.split_whitespace().count()
                + text.chars().filter(|c| c.is_uppercase()).count()
                + text.chars().filter(|c| *c == '\'' || *c == '-').count())
        }
    }

    fn lines(n: usize) -> Vec<TranscriptLine> {
        (0..n)
            .map(|i| TranscriptLine::new(i, i as f64 * 5.0, format!("line number {i} of the talk")))
            .collect()
    }

    #[test]
    fn seg_labels_are_chronological_and_fixed_width() {
        assert_eq!(seg_label(0), "seg_0001");
        assert_eq!(seg_label(41), "seg_0042");
        // Fixed width so the labels sort as text and cost the same tokens.
        assert_eq!(seg_label(0).len(), seg_label(999).len());
    }

    #[test]
    fn chunks_never_exceed_the_measured_budget() {
        let all = lines(50);
        let chunks = plan_chunks(&all, 60, &FakeCounter).expect("lines chunk");
        assert!(chunks.len() > 1, "50 lines do not fit one 60-token pass");
        for chunk in &chunks {
            let real = FakeCounter.count_tokens(&chunk.text()).expect("counted");
            assert!(real <= 60, "chunk of {real} tokens over a 60 budget");
        }
        // Every line survives, once, in order.
        let seen: Vec<String> = chunks
            .iter()
            .flat_map(|c| c.lines.iter().map(|l| l.label.clone()))
            .collect();
        assert_eq!(seen.len(), all.len());
        assert_eq!(seen[0], "seg_0001");
    }

    #[test]
    fn an_oversized_line_is_truncated_and_flagged_not_dropped() {
        let long = TranscriptLine::new(0, 0.0, "word ".repeat(400));
        let chunks = plan_chunks(&[long], 40, &FakeCounter).expect("one line chunks");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].truncated, "a cut line must be flagged");
        assert!(
            FakeCounter
                .count_tokens(&chunks[0].text())
                .expect("counted")
                <= 40
        );
    }

    #[test]
    fn the_grammar_enumerates_only_this_chunks_ids() {
        let grammar = map_grammar(&["seg_0001".to_string(), "seg_0002".to_string()])
            .expect("labels make a grammar");
        assert!(grammar.contains(r#"evid ::= "\"seg_0001\"" | "\"seg_0002\"""#));
        assert!(!grammar.contains("seg_0003"));
        assert!(grammar.starts_with("root ::="));
        assert_eq!(map_grammar(&[]), None);
    }

    #[test]
    fn merge_dedups_and_keeps_the_first_citation() {
        let a = MapExtract {
            actions: vec![ExtractedItem {
                text: "Send the deck.".into(),
                segment: "seg_0001".into(),
            }],
            ..MapExtract::default()
        };
        let b = MapExtract {
            actions: vec![
                ExtractedItem {
                    text: "send the deck".into(),
                    segment: "seg_0009".into(),
                },
                ExtractedItem {
                    text: "Book the room.".into(),
                    segment: "seg_0010".into(),
                },
            ],
            ..MapExtract::default()
        };
        let merged = merge_extracts(&[a, b]);
        assert_eq!(merged.actions.len(), 2);
        assert_eq!(merged.actions[0].segment, "seg_0001");
        assert_eq!(merged.duplicates, 1);
    }

    #[test]
    fn clamp_words_cuts_at_a_sentence() {
        let text = "One two three. Four five six seven eight nine ten.";
        assert_eq!(clamp_words(text, 100), text);
        assert_eq!(clamp_words(text, 5), "One two three.");
    }
}
