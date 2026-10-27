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
//! Segment ids are chronological transcript-segment labels ([`seg_label`]) over
//! YV94's segments — that is 22-A's scope and YV133 does not change it.
//!
//! ## YV133 — speaker attribution rides the SAME rail, one step behind it
//!
//! An action item that says *"Jeisil will send the pricing doc"* is worth more
//! than one that says *"send the pricing doc"*, and there are exactly two ways
//! to get the name into it. One is to ask the model for it, which is finding
//! #18's hazard wearing a different field name: a grammar could constrain the
//! shape of a `speaker` field perfectly and the model would still be free to
//! invent whose name goes in it. The other is to look it up, which is what this
//! module does.
//!
//! So the mechanism is: the speaker a segment is ALREADY known to belong to is
//! carried into the chunk text beside its id ([`TranscriptLine::render`]) so the
//! model can read the conversation as a conversation, the grammar stays exactly
//! as it was (still an enum over `seg_NNNN` and nothing else — see
//! [`map_grammar`]), and [`SummaryItem::speaker`] is then filled in AFTER
//! parsing by looking up the speaker of the evidence id the model was
//! constrained to cite. The model is never asked who spoke and has nowhere to
//! answer: [`ExtractedItem`] — the type the model's own output deserializes
//! into — has no speaker field at all, so a `"speaker":"Priya"` key in a MAP
//! answer lands nowhere.
//!
//! Where the speaker comes from is one trait ([`SpeakerSource`]). The only
//! implementation this build can construct is [`TrackSpeakers`], which is
//! YV108/YV125's track-derived "Me"/"Them" — real information on a two-track
//! virtual call, and deliberately *nothing at all* where the mechanism does not
//! back a name (see [`TrackSpeakers`]'s own note). Enrolled names arrive when
//! YV128–130 build the profile table; nothing here needs to change when they
//! do, which is the point of the seam.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::meetings::{
    diarization_target, format_offset, is_two_track, speaker_label, DiarizationTarget, MeetingKind,
    MeetingSegment, UNCLUSTERED_SPEAKER_LABEL,
};
use crate::polish_protocol::{
    fit_to_budget, parse_ready, parse_response_for, PolishRequest, PolishResponse,
    SUMMARIZE_HEADLINE, SUMMARIZE_MAP, SUMMARIZE_REDUCE, SUMMARY_HEADLINE_MAX_OUT,
    summary_map_max_out, SUMMARY_REDUCE_MAX_OUT, TRUNCATION_MARKER,
};

/// Token budget for ONE map chunk's input, measured in the model's own
/// vocabulary.
///
/// Sized against the sidecar's 4096-token context with room for the system turn,
/// the chat template and up to [`SUMMARY_MAP_MAX_OUT_CEILING`](crate::polish_protocol::SUMMARY_MAP_MAX_OUT_CEILING) of output. It is a budget for
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
    /// Every pass ran and answered, and what came back carried nothing: no
    /// narrative, no actions, no decisions, no questions.
    ///
    /// Deliberately its own variant rather than an empty `Ok`. A summary with no
    /// content in it is not a thin summary, it is the absence of one, and the
    /// only thing a caller can do with it is overwrite a summary the user
    /// already had — data loss, in an app with no undo, reported as success.
    /// Kept distinct from [`SummaryError::Protocol`] because the two say
    /// different things to whoever is reading the log: `protocol` means the
    /// sidecar is broken, `empty` means it answered and had nothing to say.
    Empty,
    /// The person pressed Cancel (#250 round 3, B2). Nothing is stored: a
    /// cancelled run is never a partial summary passed off as the final one.
    Cancelled,
}

impl SummaryError {
    pub fn tag(self) -> &'static str {
        match self {
            Self::NoSegments => "no_segments",
            Self::Unavailable => "unavailable",
            Self::Protocol => "protocol",
            Self::Empty => "empty",
            Self::Cancelled => "cancelled",
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

// ── YV133 · where a speaker comes from, and where it may never come from ────

/// The speaker a stored segment is ALREADY known to belong to.
///
/// Two variants, because there are two mechanically different things a speaker
/// string can be, and collapsing them would let a placeholder be presented as
/// an attribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentSpeaker {
    /// A PERSON: an enrolled `speaker_profiles` name (YV129) or a correction
    /// the user made by hand (YV130). The only variant that can answer "who
    /// said this" with a name a reader recognises.
    Named(String),
    /// A voice told apart from other voices without being identified —
    /// YV108's `Me`/`Them`, YV126's `Speaker 2`. Real information (it separates
    /// two people) but not an identity, and it must never be rendered as one.
    Anonymous(String),
}

impl SegmentSpeaker {
    /// The string as a reader sees it.
    pub fn label(&self) -> &str {
        match self {
            SegmentSpeaker::Named(s) | SegmentSpeaker::Anonymous(s) => s,
        }
    }

    /// Is this a person's name, as opposed to a voice number?
    pub fn is_named(&self) -> bool {
        matches!(self, SegmentSpeaker::Named(_))
    }
}

/// Where a segment's speaker comes from. **Never the model** — that is the
/// whole contract of this trait, and the reason it exists as a seam rather than
/// as a field the parser could fill.
///
/// Implementations answer `None` for "I do not know who spoke", and are
/// required to answer `None` rather than a placeholder: a source that returned
/// `Anonymous("Speaker")` for every segment of a mic-only meeting would spend a
/// tag on every transcript line to say nothing, and would put the bare word
/// *Speaker* into the model's source text, where a summarizer can pick it up as
/// if it were somebody's name. [`TrackSpeakers`] is the worked example.
pub trait SpeakerSource {
    fn speaker_for(&self, segment: &MeetingSegment) -> Option<SegmentSpeaker>;
}

/// No speaker information at all: every meeting summarized before YV133, and
/// the behaviour [`summarize_segments`] keeps for callers that have none.
pub struct NoSpeakers;

impl SpeakerSource for NoSpeakers {
    fn speaker_for(&self, _segment: &MeetingSegment) -> Option<SegmentSpeaker> {
        None
    }
}

/// The one speaker source this build can actually construct: YV108's track
/// labels under YV125's diarization branch.
///
/// Built through [`TrackSpeakers::for_meeting`] so the target is computed from
/// the meeting's `kind` and its segments by the SAME two calls
/// `meetings::render_transcript` makes — the summary's attribution and the
/// transcript's speaker column are then two readings of one decision rather
/// than two decisions that can disagree.
///
/// **Why [`UNCLUSTERED_SPEAKER_LABEL`] becomes `None` here.** On the clustering
/// branch — every in-person meeting, every unknown one, every virtual one whose
/// tap never attached — `meetings::speaker_label` answers the honest `Speaker`,
/// which means *somebody spoke and this build cannot say who*. That is the
/// right word on a transcript row, where the reader can see the rows are
/// interleaved. It is the wrong thing to hand a summarizer: it is identical on
/// every line, so it separates nobody, it costs a tag per line out of a budget
/// finding #35 exists to protect, and it writes the word "Speaker" into the
/// source text that [`validate_item`]'s groundedness floor scores against. So
/// the source declines. It starts answering the moment there is something to
/// answer with — `Me`/`Them` on a live two-track call today, an enrolled name
/// once YV128–130 land.
pub struct TrackSpeakers {
    target: DiarizationTarget,
}

impl TrackSpeakers {
    /// The labelling this meeting's transcript is already rendered with.
    pub fn for_meeting(kind: MeetingKind, segments: &[MeetingSegment]) -> Self {
        Self {
            target: diarization_target(kind, is_two_track(segments)),
        }
    }

    /// The branch this source resolved to, for tests and diagnostics.
    pub fn target(&self) -> DiarizationTarget {
        self.target
    }
}

impl SpeakerSource for TrackSpeakers {
    fn speaker_for(&self, segment: &MeetingSegment) -> Option<SegmentSpeaker> {
        let label = speaker_label(segment.track, self.target);
        if label == UNCLUSTERED_SPEAKER_LABEL {
            return None;
        }
        // Never `Named`: a track index is a channel, not a person. Whether a
        // voice is Wilson's is a property of an enrolled profile matched by
        // embedding (YV128), and nothing here may promote a channel number to
        // an identity — the same rule `meetings::speaker_label`'s own header
        // states about `is_me`.
        Some(SegmentSpeaker::Anonymous(label.to_string()))
    }
}

/// One transcript line on its way into a MAP pass.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptLine {
    pub label: String,
    pub start_seconds: f64,
    pub text: String,
    /// YV133 — who this line belongs to, if anybody knows. Set from a
    /// [`SpeakerSource`] before the line is ever rendered, and read back after
    /// parsing to fill [`SummaryItem::speaker`]. `None` is the 22-A shape, and
    /// renders byte-identically to what 22-A rendered.
    pub speaker: Option<SegmentSpeaker>,
}

impl TranscriptLine {
    pub fn new(index: usize, start_seconds: f64, text: impl Into<String>) -> Self {
        Self {
            label: seg_label(index),
            start_seconds,
            text: text.into(),
            speaker: None,
        }
    }

    /// The same line, attributed. `None` and a blank label both mean "unknown",
    /// which is not the same thing as a speaker called `""`.
    pub fn with_speaker(mut self, speaker: Option<SegmentSpeaker>) -> Self {
        self.speaker = speaker.filter(|s| !s.label().trim().is_empty());
        self
    }

    /// Everything before the line's own words: `seg_0007: `, or
    /// `seg_0007 (Them): ` once a speaker is known.
    ///
    /// One function because two places need it to agree — [`Self::render`]
    /// writes it and [`plan_chunks`] strips it back off a truncated render, and
    /// a mismatch there doubles the label inside the model's prompt.
    pub fn prefix(&self) -> String {
        match &self.speaker {
            Some(speaker) => format!("{} ({}): ", self.label, speaker.label()),
            None => format!("{}: ", self.label),
        }
    }

    /// `seg_0007: we agreed to ship on friday` — one line, always, so the
    /// chunker can cut on line boundaries and the model can cite a whole line.
    ///
    /// With a speaker: `seg_0007 (Them): we agreed to ship on friday`. The id
    /// stays first and stays bare, because it is the id the grammar enumerates
    /// and the id the model must echo back.
    pub fn render(&self) -> String {
        format!("{}{}", self.prefix(), one_line(&self.text))
    }
}

/// YV94's stored segments, labelled in wall-clock order. Empty segments are
/// dropped: a blank line costs tokens and cites nothing.
pub fn transcript_lines(segments: &[MeetingSegment]) -> Vec<TranscriptLine> {
    transcript_lines_with(segments, &NoSpeakers)
}

/// The same, attributed by `speakers`.
///
/// The speaker is looked up per SEGMENT, not per line index, so the lookup
/// keeps working when empty segments drop out and the two numberings diverge.
pub fn transcript_lines_with(
    segments: &[MeetingSegment],
    speakers: &dyn SpeakerSource,
) -> Vec<TranscriptLine> {
    segments
        .iter()
        .filter(|s| !s.text.trim().is_empty())
        .enumerate()
        .map(|(i, s)| {
            TranscriptLine::new(i, s.start_seconds, s.text.trim())
                .with_speaker(speakers.speaker_for(s))
        })
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
            let prefix = line.prefix();
            line.text = format!(
                "{} {TRUNCATION_MARKER}",
                strip_prefix(kept.trim_end_matches(TRUNCATION_MARKER).trim_end(), &prefix)
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

/// Drop the `seg_NNNN: ` / `seg_NNNN (Them): ` prefix a rendered line carries,
/// so a truncated render can go back into a [`TranscriptLine`] without doubling
/// its label — or its speaker.
///
/// `prefix` comes from [`TranscriptLine::prefix`], never rebuilt here: the cut
/// may have landed anywhere, including inside the prefix itself, and a
/// hand-rolled second opinion about what the prefix looks like is exactly how
/// the two get out of step.
fn strip_prefix(rendered: &str, prefix: &str) -> String {
    rendered
        .strip_prefix(prefix)
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
///
/// **One rule per line, no continuations.** llama.cpp's GBNF parser ends a rule
/// at the newline (`parse_space(.., newline_ok = false)` inside a rule body) and
/// then demands the next `name ::=`, so the pretty-printed `root` this used to
/// emit — its four object fields wrapped across four indented lines — failed to
/// parse with *"expecting ::= at \"actions\"…"*. `LlamaSampler::grammar` then
/// returned `Err`, the sidecar answered `grammar`, and EVERY constrained MAP
/// pass failed. Nothing in CI could catch it: building a grammar sampler needs a
/// resident GGUF, so the only test that reaches this is the `#[ignore]`d
/// `summarize_e2e_real_sidecar`, which is now part of the merge gate.
///
/// **YV133 — what did NOT change here.** The chunk text handed to a MAP pass
/// now carries each line's speaker beside its id, and none of that reaches this
/// function: `labels` is [`Chunk::labels`], which is still bare `seg_NNNN`, the
/// `evid` rule is still an enum over exactly those, and there is still no
/// speaker field in the schema for a model to fill. Speaker is looked up after
/// the parse, from the id the model was constrained to cite — so this grammar's
/// hallucination guard is exactly as strict as YV97 shipped it, and
/// `tests/summarize_grammar_unchanged_shape.rs` fails the build if a speaker
/// ever leaks into it.
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
            "root ::= \"{{\" ws \"\\\"narrative\\\"\" ws \":\" ws string ws \",\"",
            " ws \"\\\"actions\\\"\" ws \":\" ws items ws \",\"",
            " ws \"\\\"decisions\\\"\" ws \":\" ws items ws \",\"",
            " ws \"\\\"questions\\\"\" ws \":\" ws items ws \"}}\"\n",
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
///
/// **This is the model's own output type, and YV133 deliberately did not add a
/// `speaker` field to it.** The backlog's file list names both this struct and
/// [`SummaryItem`]; only the second one got the field, for two reasons that
/// point the same way. Mechanically, the lookup cannot happen here —
/// [`parse_map_output`] is handed a raw answer, a label allowlist and the chunk
/// text, and has no access to any speaker, so a field here could only ever be
/// `None` until [`summarize_segments_with`] filled it in later. And in
/// principle, a `speaker` key on the type a model's JSON deserializes INTO is a
/// slot a model can aim at: `serde` ignores unknown keys, so today a MAP answer
/// carrying `"speaker":"Priya"` lands nowhere at all, which is a stronger
/// guarantee than a field that is filled elsewhere and merely *ought* not be
/// overwritten. `tests/summarize_grammar_unchanged_shape.rs` pins the absence.
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

/// Which list an extracted line goes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    Actions,
    Decisions,
    Questions,
}

// ── #250 round 2: the post-parse classifier ─────────────────────────────────

/// Words that open an imperative action item ("Send the deck to Priya").
const IMPERATIVE_VERBS: &[&str] = &[
    "send", "book", "schedule", "email", "call", "share", "draft", "write", "update",
    "review", "check", "prepare", "finish", "fix", "ship", "file", "create", "add",
    "confirm", "ask", "invite", "circulate", "post", "follow", "set", "reach", "get",
    "find", "look", "test", "submit", "sign", "order", "contact", "loop",
];

/// Owner-led commitments: `<owner> will …`, `<owner> to …`, `I'll …`.
const COMMIT_AFTER_OWNER: &[&str] = &[
    "will ", "to ", "should ", "must ", "needs to ", "has to ", "is going to ",
    "agreed to ", "offered to ", "promised to ", "owns ", "can ", "is on ",
];

/// Settled things. Checked first: "we also decided to drop X" carries a
/// `to <verb>` that would otherwise read as a commitment.
const DECISION_MARKERS: &[&str] = &[
    "decided", "decision", "we agreed", "agreed that", "agreed the", "settled",
    "will remain", "will stay", "stays at", "remains at", "stay at", "remain at",
    "going with", "go with", "approved", "signed off", "is final",
];

/// #250 round 3 (B4) — words that open a sentence capitalized only because
/// they open it: interjections, sentence adverbs, adjectives of small talk
/// ("Nice to meet you", "Happy to help", "Hard to say", "Honestly can we…").
/// None of them is ever an owner, whatever follows.
const NOT_A_NAME: &[&str] = &[
    "nice", "happy", "thanks", "thank", "good", "great", "hard", "easy", "yes", "no", "yeah",
    "yep", "nope", "honestly", "sure", "okay", "ok", "well", "glad", "fine", "right", "sorry",
    "hopefully", "actually", "basically", "really", "maybe", "perhaps", "probably",
    "definitely", "certainly", "clearly", "obviously", "please", "hi", "hello", "hey",
    "welcome", "cool", "awesome", "fair", "true", "important", "worth", "free", "ready",
    "next", "first", "last", "just", "now", "today", "tomorrow", "here", "excited", "safe",
    "likely", "unlikely", "able", "quick", "back", "lovely", "fun", "interesting", "funny",
    "weird", "strange", "sad", "tough", "impossible", "possible", "better", "best", "kind",
    "time", "nothing", "everybody", "anybody", "anyone", "somebody", "you", "he", "she",
    "his", "her", "their", "your", "let's", "lets", "oh", "um", "uh", "so", "and", "but",
    "also", "then", "quite", "very", "too", "exactly", "absolutely", "totally", "agreed",
];

/// #250 round 4 (R2-B2) — common English words that open a sentence and are
/// never a person: days, months, the indefinite pronouns, everyday nouns of
/// small talk ("Everything will be fine", "Monday will be busy", "Coffee will
/// help"). None of them is ever an owner, and none is ever a transcript name.
const COMMON_WORDS: &[&str] = &[
    "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday", "january",
    "february", "march", "april", "may", "june", "july", "august", "september", "october",
    "november", "december", "everything", "nothing", "something", "anything", "everywhere",
    "somewhere", "nowhere", "coffee", "tea", "lunch", "dinner", "breakfast", "traffic",
    "revenue", "sales", "budget", "money", "weather", "rain", "food", "water", "work",
    "life", "team", "morning", "afternoon", "evening", "tonight", "weekend", "week", "month",
    "year", "quarter", "noon", "midnight", "people", "things", "stuff", "nobody", "none",
    "each", "every", "all", "both", "some", "most", "many", "much", "more", "less", "other",
    "another", "such", "same", "which", "where", "while", "because", "although", "after",
    "before", "since", "until", "during", "without", "with", "for", "from", "into", "onto",
    "over", "under", "about", "again", "still", "even", "only", "never", "always", "often",
    "sometimes", "usually", "soon", "later", "earlier", "otherwise", "instead", "anyway",
    "meanwhile", "though", "however", "therefore", "plus", "or", "nor", "yet", "not",
    // review-opus2 N1: the everyday nouns of a work meeting are not people.
    "pricing", "price", "prices", "product", "design", "support", "finance", "legal",
    "customer", "customers", "client", "clients", "launch", "release", "deadline", "meeting",
    "call", "plan", "project", "demo", "deck", "doc", "report", "update", "marketing",
    "engineering", "hiring", "onboarding", "billing", "growth", "progress", "nobody",
];

/// Is `lower` (already lowercased) one of [`COMMON_WORDS`]?
fn is_common_word(lower: &str) -> bool {
    COMMON_WORDS.contains(&lower)
}

/// A capitalized single word that is not a pronoun, a determiner, a
/// small-talk opener or a common word — the only shape a NAME may have.
fn is_name_word(word: &str) -> bool {
    let w = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'');
    let lower = w.to_lowercase();
    let mut chars = w.chars();
    chars.next().is_some_and(char::is_uppercase)
        && chars.all(|c| c.is_lowercase())
        && !NOT_A_NAME.contains(&lower.as_str())
        && !is_common_word(&lower)
        && !matches!(
            lower.as_str(),
            "the" | "this" | "that" | "it" | "they" | "there" | "someone" | "nobody"
                | "everyone" | "whether" | "if" | "what" | "how" | "why" | "who" | "when"
                | "a" | "an" | "our" | "my" | "i" | "we" | "me" | "them" | "these" | "those"
        )
}

/// #250 round 4 (R2-B2) — the names this transcript itself uses: its NAMED
/// speakers (never `Me` / `Them` / `Speaker 2`), and every capitalized word
/// that sits INSIDE a sentence at least twice and is not a common word. A
/// sentence-initial word is no evidence of a name — every sentence starts
/// with a capital — and one mid-sentence capital can be a product, a place
/// or a typo. "Thanks Priya … ask Priya" names Priya; "Coffee will help"
/// names nobody.
pub fn transcript_names(lines: &[TranscriptLine]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let add = |names: &mut Vec<String>, n: &str| {
        if !n.is_empty() && !names.iter().any(|k| k.eq_ignore_ascii_case(n)) {
            names.push(n.to_string());
        }
    };
    let mut mid: Vec<(String, usize)> = Vec::new();
    for l in lines {
        if let Some(sp) = &l.speaker {
            if sp.is_named() {
                add(&mut names, sp.label().trim());
            }
        }
        let mut prev_ends_sentence = true;
        for raw in l.text.split_whitespace() {
            let w = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'');
            let w = w.strip_suffix("'s").or(w.strip_suffix("'ll")).unwrap_or(w);
            if !prev_ends_sentence && is_name_word(w) {
                match mid.iter_mut().find(|(k, _)| k == w) {
                    Some((_, n)) => *n += 1,
                    None => mid.push((w.to_string(), 1)),
                }
            }
            prev_ends_sentence = raw.ends_with(['.', '!', '?', ':']);
        }
    }
    for (w, n) in mid {
        if n >= 2 {
            add(&mut names, &w);
        }
    }
    names
}

fn is_owner_word(word: &str) -> bool {
    let w = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'');
    let lower = w.to_lowercase();
    if matches!(lower.as_str(), "i" | "we") {
        return true;
    }
    let mut chars = w.chars();
    chars.next().is_some_and(char::is_uppercase)
        && chars.all(|c| c.is_lowercase())
        && !NOT_A_NAME.contains(&lower.as_str())
        && !is_common_word(&lower)
        && !matches!(
            lower.as_str(),
            "the" | "this" | "that" | "it" | "they" | "there" | "someone" | "nobody"
                | "everyone" | "whether" | "if" | "what" | "how" | "why" | "who" | "when"
                | "a" | "an" | "our" | "my" | "also" | "and" | "so" | "then"
        )
}

/// `<owner> will/to/…` at the front, or a contracted `I'll` / `We'll` /
/// `Jeisil'll`. The owner + verb shape the action-item rule asks for.
fn owner_commitment(text: &str) -> bool {
    let t = text.trim();
    let Some((first, rest)) = t.split_once(char::is_whitespace) else {
        return false;
    };
    let rest = rest.trim_start().to_lowercase();
    let lower_first = first.to_lowercase();
    if let Some(owner) = lower_first.strip_suffix("'ll").or(lower_first.strip_suffix("\u{2019}ll")) {
        return !owner.is_empty();
    }
    if !is_owner_word(first) {
        return false;
    }
    // "We agreed to keep the price" is a decision, not a task.
    if lower_first == "we" && rest.starts_with("agreed") {
        return false;
    }
    COMMIT_AFTER_OWNER.iter().any(|c| rest.starts_with(c))
}

/// #250 round 4 (R2-B2) — the backfill's owner rule, stricter than
/// [`owner_commitment`] because a backfilled line is one the model did NOT
/// pick. The owner must be a name from `names` ([`transcript_names`]: named
/// speakers, and capitalized words said twice mid-sentence) followed by an
/// explicit commitment verb. There is no unknown-word branch: without a real
/// owner there is no backfilled action item at all. "Can", "owns", "is on", a
/// bare adjective + "to" — none of them commits anyone.
fn backfill_commitment(text: &str, names: &[String]) -> bool {
    let t = text.trim();
    let Some((first, rest)) = t.split_once(char::is_whitespace) else {
        return false;
    };
    let rest = rest.trim_start().to_lowercase();
    let bare = first.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '\u{2019}');
    let known = |w: &str| names.iter().any(|n| n.eq_ignore_ascii_case(w));
    if let Some(owner) = bare.strip_suffix("'ll").or(bare.strip_suffix("\u{2019}ll")) {
        return is_name_word(owner) && known(owner);
    }
    if !is_name_word(bare) {
        return false;
    }
    const EXPLICIT: &[&str] = &[
        "will ", "is going to ", "agreed to ", "offered to ", "promised to ", "needs to ",
        "has to ", "should ",
    ];
    known(bare) && (EXPLICIT.iter().any(|c| rest.starts_with(c)) || rest.starts_with("to "))
}

/// #250 round 3 (A2) — a line ABOUT whether something was decided is an open
/// question, not a decision: "Jeisil asked whether we decided", "Nobody
/// decided whether to keep it". Checked before the decision markers.
fn open_about_decision(lower: &str) -> bool {
    [
        "asked whether", "asked if", "decided whether", "decide whether", "nobody decided",
        "no one decided", "not decided", "haven't decided", "hasn't decided", "didn't decide",
        "did not decide", "not yet decided", "undecided", "still deciding",
    ]
    .iter()
    .any(|m| lower.contains(m))
}

fn starts_imperative(text: &str) -> bool {
    let first = text
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    IMPERATIVE_VERBS.contains(&first.as_str())
}

fn is_strong_question(lower: &str) -> bool {
    lower.trim_end().ends_with('?')
        || lower.contains("open question")
        || lower.contains("still open")
        || lower.contains("unresolved")
        || lower.contains("undecided")
        || lower.contains("not yet decided")
        || lower.contains("still unclear")
}

fn is_weak_question(lower: &str) -> bool {
    [
        "whether ", "should we ", "do we ", "does anyone ", "can we ", "are we ", "is it ",
        "how do we ", "what if ",
    ]
    .iter()
    .any(|q| lower.starts_with(q))
        || lower.contains(" is whether ")
        || lower.contains(" was whether ")
        || lower.contains(" asked whether ")
        || lower.contains(" asked if ")
}

fn generic_commitment(lower: &str) -> bool {
    [
        " will ", "'ll ", " need to ", " needs to ", " has to ", " have to ", " must ",
        " should ", " to do", "follow up", "going to ", " by friday", " by monday",
    ]
    .iter()
    .any(|c| format!(" {lower} ").contains(c))
}

/// Which list `text` belongs in, judged from its own words; `None` when it
/// was offered as an action item and carries no commitment at all.
///
/// The 1.5B MAP model buckets by topic, not by speech act — the real run on
/// PR #250 filed "The open question is whether annual billing gets a
/// discount" and "We also decided to drop the free trial" under Action
/// items. Grounding was right; the bucket was wrong. Order matters:
///
/// 1. a settled marker ("decided", "stays at", "will remain") → Decisions;
/// 2. an explicit open marker ("?", "open question") → Open questions;
/// 3. an owner + commitment ("Jeisil will send", "Wilson to book") or an
///    imperative ("Send the deck") → Action items;
/// 4. a softer question shape ("whether …", "should we …") → Open questions;
/// 5. otherwise the model's bucket stands — except Action items, which must
///    carry a commitment ("will", "need to", "follow up", …) or are dropped:
///    an action item without one is a statement the narrative already holds.
pub fn classify_item(from: Bucket, text: &str) -> Option<Bucket> {
    let lower = text.trim().to_lowercase();
    if open_about_decision(&lower) {
        return Some(Bucket::Questions);
    }
    if (DECISION_MARKERS.iter().any(|m| lower.contains(m)) || passive_settled(&lower))
        && !owner_commits_strongly(text)
    {
        return Some(Bucket::Decisions);
    }
    if is_strong_question(&lower) {
        return Some(Bucket::Questions);
    }
    // An imperative keeps an item the model already called an action; only an
    // owner + commitment moves one IN ("Drop the free trial" filed as a
    // decision is a decision).
    // Fix round 3: small talk is nobody's task, decision or question, in
    // any bucket ("Cheers everyone, see you all next week" is not an action
    // item). Every marker above has had its say, and `is_pleasantry` needs
    // EVERY clause to be a greeting, so a line with substance is kept.
    if is_pleasantry(text) {
        return None;
    }
    if owner_commitment(text) || (from == Bucket::Actions && starts_imperative(text)) {
        return Some(Bucket::Actions);
    }
    if is_weak_question(&lower) {
        return Some(Bucket::Questions);
    }
    match from {
        // A statement ("The pricing review is complete") is not a task: its
        // subject is a thing, and nothing in it commits anyone.
        Bucket::Actions => {
            let first = lower.split_whitespace().next().unwrap_or("");
            let first = first.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'');
            // B4: a small-talk opener ("Happy to help", "Honestly can we
            // move on") is no more a task than a statement is.
            let statement = matches!(
                first,
                "the" | "this" | "that" | "it" | "there" | "these" | "those" | "its" | "a" | "an"
            ) || NOT_A_NAME.contains(&first);
            // R2-B2: "Coffee will help", "Monday will be busy" — a common
            // word for a subject is small talk, whatever verb follows.
            if is_common_word(first) {
                return None;
            }
            (!statement || generic_commitment(&lower)).then_some(Bucket::Actions)
        }
        // Fix round 1 (review BLOCKING): a line the model filed as a decision
        // or a question that carries no marker of either is kept on the
        // model's word — small talk ("Thanks Jeisil, …", "Nice to meet you
        // all") was already dropped above.
        other => Some(other),
    }
}

/// Words that greet, thank or say goodbye. A clause made only of these, its
/// addressees and filler is small talk.
const GREETING_WORDS: &[&str] = &[
    "thanks", "thank", "hi", "hello", "hey", "cheers", "congrats", "congratulations", "bye",
    "goodbye", "nice", "great", "good", "glad", "happy", "welcome", "lovely", "cool", "awesome",
    "sorry", "appreciate", "appreciated", "pleasure", "morning", "afternoon", "evening", "night",
    "kudos",
];

/// Greeting words that start small talk outright; a capitalized word anywhere
/// after them in the same clause is an addressee ("Thanks Jeisil and Wilson",
/// "Hi everyone, hello Jeisil"). The ambiguous ones (good, great, morning,
/// welcome) also start proper nouns ("Good Shepherd Health", "Great Lakes"),
/// so a name after them counts only in the last slot after filler.
const UNAMBIGUOUS_GREETINGS: &[&str] = &[
    "thanks", "thank", "hi", "hello", "hey", "cheers", "congrats", "congratulations", "bye",
    "goodbye", "kudos",
];

/// Gratitude openers whose clause may name a reason after `for` / `on`
/// ("Thanks Wilson for pulling the numbers together", "Congrats on the
/// launch"); `appreciate` takes any object ("Appreciate everyone making the
/// time").
const GRATITUDE_OPENERS: &[&str] =
    &["thanks", "thank", "cheers", "congrats", "congratulations", "kudos", "appreciate", "appreciated"];

/// Addressees and filler that carry no decision and no topic. Fix round 3
/// (review BLOCKING): weekdays, "it", "is", "on", "board", "we're" are NOT
/// here — "Great, Monday it is." and "Good, so we're on board." are
/// decisions. A weekday counts only right after "happy" ("Happy Friday").
const PLEASANTRY_FILLER: &[&str] = &[
    "everyone", "everybody", "all", "y'all", "you", "guys", "folks", "team", "again", "so",
    "much", "very", "really", "too", "today", "here", "and", "to", "see", "meet", "have",
    "having", "be", "being", "back", "us", "for", "joining", "coming", "the", "time", "call",
    "chat", "day", "weekend", "week", "about", "late", "i", "i'm", "im", "a", "one",
    "of", "both", "lot", "oh", "well", "ok", "okay", "also",
];

/// Fix round 6 (review-opus2 BLOCKING on b3d048c): the words a courtesy reason
/// is made of. The reason after a thank-you is small talk only when EVERY word
/// is here or in `REASON_FILLER` — anything else ("approving", "vendor",
/// "dropping", "cancelling", a number, a name) keeps the line. Round 5 also
/// accepted all of `PLEASANTRY_FILLER` here, and its schedule words (back,
/// week, day, today, to, have) plus the two-sense verbs below were enough
/// vocabulary to spell a decision ("Thanks for setting the launch back a
/// week."). Those are now handled by `SCHEDULE_WORDS` and `TWO_SENSE_VERBS`.
const COURTESY_REASON: &[&str] = &[
    "it", "that", "this", "your", "everyone's", "everybody's", "my", "our", "me", "them",
    "him", "her", "his", "their", "help", "helping", "hosting", "host", "together", "numbers",
    "patience", "waiting", "flagging", "input", "feedback", "support", "effort", "efforts",
    "work", "hard", "great", "good", "nice", "sharing", "listening", "attending", "dialing",
    "dialling", "in", "showing", "up", "staying", "organizing", "organising", "update",
    "updates", "notes", "launch", "win", "promotion", "birthday", "anniversary", "news",
    "presentation", "demo", "thoughts", "questions", "everything", "invite", "invitation",
    "intro", "introduction", "quick", "turnaround",
];

/// The non-schedule filler a courtesy reason may carry ("for joining",
/// "for being here", "for all of your help"). Deliberately NOT all of
/// `PLEASANTRY_FILLER`: see `SCHEDULE_WORDS`.
const REASON_FILLER: &[&str] = &[
    "everyone", "everybody", "all", "y'all", "you", "guys", "folks", "team", "again", "so",
    "much", "very", "really", "too", "here", "and", "us", "the", "a", "of", "both", "lot", "i",
    "joining", "coming", "being", "be", "call", "chat", "also",
];

/// A schedule or time word in the reason means it can carry a decision
/// ("…back a week", "…for today", "…to have the launch…"), so the line is
/// kept. "time" is the one exception, and only as the object of a courtesy
/// ("making the time", "your time", "taking the time").
const SCHEDULE_WORDS: &[&str] = &[
    "back", "week", "weeks", "day", "days", "today", "tonight", "tomorrow", "to", "have",
    "having", "one", "late", "early", "earlier", "later", "weekend", "next", "until", "till",
    "by", "before", "after", "month", "months", "quarter", "year", "date", "deadline",
];

/// Verbs with a courtesy sense AND a decision sense ("pulling the numbers
/// together" vs "pulling the launch"). Each counts as courtesy only with its
/// courtesy completion somewhere later in the reason, and never alongside an
/// agenda noun from `AGENDA_NOUNS`.
const TWO_SENSE_VERBS: &[(&str, &[&str])] = &[
    ("pulling", &["together"]),
    ("putting", &["together"]),
    ("setting", &["up"]),
    ("taking", &["notes", "time"]),
    ("making", &["time"]),
    ("make", &["time"]),
];

/// Things a two-sense verb can act on as a decision ("pulling the launch",
/// "setting the demo for today").
const AGENDA_NOUNS: &[&str] = &[
    "launch", "demo", "presentation", "call", "meeting", "release", "pilot", "offsite",
    "deal", "contract", "price", "plan", "budget", "date", "deadline",
];

/// Is the reason a gratitude opener gives (`words[from..]`) only courtesy?
/// It is courtesy only when every word is lowercase (any capitalized word but
/// "I" is a name, a month or a weekday), no word is a schedule word, every
/// two-sense verb has its courtesy completion and no agenda noun, and every
/// other word is on `COURTESY_REASON` or `REASON_FILLER`. So "Thanks for
/// approving the budget increase", "Thanks for pulling the launch" and
/// "Thanks for setting the launch back a week" are kept, while "Appreciate
/// everyone making the time", "Thanks Wilson for pulling the numbers
/// together" and "Thank you all for joining" are small talk.
fn courtesy_reason(words: &[&str], lower: &[String], from: usize) -> bool {
    let reason: Vec<&str> = lower[from.min(lower.len())..].iter().map(String::as_str).collect();
    let has_agenda = reason.iter().any(|w| AGENDA_NOUNS.contains(w));
    (from..lower.len()).enumerate().all(|(k, j)| {
        let w = lower[j].as_str();
        let capitalized = words[j].chars().next().is_some_and(char::is_uppercase)
            && !matches!(w, "i" | "i'm" | "i've" | "i'll" | "i'd" | "im");
        if capitalized {
            return false;
        }
        if w == "time" {
            return k > 0
                && matches!(reason[k - 1], "the" | "your" | "making" | "taking" | "make" | "our");
        }
        if SCHEDULE_WORDS.contains(&w) {
            return false;
        }
        if let Some((_, completions)) = TWO_SENSE_VERBS.iter().find(|(v, _)| *v == w) {
            // "making the call" is a decision; "hosting the call" is not, so
            // `call` is an agenda noun only next to a two-sense verb.
            return !has_agenda && reason[k + 1..].iter().any(|r| completions.contains(r));
        }
        COURTESY_REASON.contains(&w) || REASON_FILLER.contains(&w)
    })
}

/// Lead-ins that may come before a greeting inside a clause ("and thanks
/// Wilson …", "oh hi").
const CLAUSE_LEAD_INS: &[&str] = &["and", "so", "oh", "well", "ok", "okay", "also"];

const WEEKDAYS: &[&str] =
    &["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];

/// Is `text` small talk rather than a statement of substance?
///
/// Fix round 3 (both reviewers BLOCKING on 0baec25): the line is split into
/// clauses at `, ; : . ! ?` and a spaced dash, and it is small talk only when
/// EVERY clause is a greeting, thanks or goodbye and at least one clause
/// carries a greeting word. So "Hey, the launch moves to May.", "Thanks
/// everyone, the launch moves to May." and "The pilot moves to May, thanks
/// for flagging it Jeisil." keep their substance clause and are kept, while
/// "Thanks Jeisil, and thanks Wilson for pulling the numbers together." is
/// dropped. Words are split on whitespace and `/` only, so "Hi-Fi Audio",
/// "Thank-you cards" and "Bye-laws" are single non-greeting words. A causal
/// "Thanks to <thing>" (not to an addressee) is not gratitude.
fn is_pleasantry(text: &str) -> bool {
    let normalized = text.replace(" - ", ",").replace(['\u{2014}', '\u{2013}'], ",");
    let clauses: Vec<Vec<&str>> = normalized
        .split([',', ';', ':', '.', '!', '?'])
        .map(|c| {
            c.split(|ch: char| ch.is_whitespace() || ch == '/')
                .map(|w| w.trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '\'' && ch != '-'))
                .map(|w| w.trim_matches('-'))
                .filter(|w| !w.is_empty())
                .collect::<Vec<&str>>()
        })
        .filter(|c| !c.is_empty())
        .collect();
    if clauses.is_empty() {
        return false;
    }
    let mut greeted = false;
    for clause in &clauses {
        match pleasantry_clause(clause) {
            Some(has_greeting) => greeted |= has_greeting,
            None => return false,
        }
    }
    greeted
}

/// `Some(has_greeting_word)` when the clause is nothing but small talk,
/// `None` when it carries anything else.
fn pleasantry_clause(words: &[&str]) -> Option<bool> {
    let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
    let is_name = |i: usize| {
        words[i].chars().next().is_some_and(char::is_uppercase)
            && !GREETING_WORDS.contains(&lower[i].as_str())
            && !PLEASANTRY_FILLER.contains(&lower[i].as_str())
    };
    // A clause of one to three capitalized names is an addressee: the
    // "Jeisil" in "Good morning, Jeisil."
    if words.len() <= 3 && (0..words.len()).all(is_name) {
        return Some(false);
    }
    let mut start = 0;
    while start < lower.len() && CLAUSE_LEAD_INS.contains(&lower[start].as_str()) {
        start += 1;
    }
    let first = lower.get(start).map(String::as_str)?;
    let unambiguous = UNAMBIGUOUS_GREETINGS.contains(&first);

    if GRATITUDE_OPENERS.contains(&first) {
        // Fix round 4 (review BLOCKING on cb35adf): "appreciate" goes through
        // the same addressee run and reason check as "thanks" — it no longer
        // decides on the first word alone.
        let appreciate = first.starts_with("appreciate");
        let mut i = start + 1;
        if first == "thank" {
            if lower.get(i).map(String::as_str) != Some("you") {
                return None; // "Thank the vendor …" is an instruction.
            }
            i += 1;
        }
        // "Thanks to everyone …" is gratitude; "Thanks to everyone's
        // feedback, …" and "Thanks to the new template, …" are causal.
        let causal_to = !appreciate && lower.get(i).map(String::as_str) == Some("to");
        if causal_to {
            i += 1;
        }
        let run_start = i;
        while i < lower.len()
            && (PLEASANTRY_FILLER.contains(&lower[i].as_str()) && !matches!(lower[i].as_str(), "for" | "the")
                || is_name(i))
        {
            i += 1;
        }
        if causal_to && i == run_start {
            return None;
        }
        // Fix round 4: reaching "for" / "on" is small talk only when the
        // reason after it is courtesy, not a date, a name or a number.
        let reason = match lower.get(i).map(String::as_str) {
            None => return Some(true),
            Some("for" | "on") => Some(i + 1),
            // "Appreciate everyone making the time": the object is the reason.
            Some(_) if appreciate => Some(i),
            Some(_) => None,
        };
        if let Some(from) = reason {
            return courtesy_reason(words, &lower, from).then_some(true);
        }
        if causal_to {
            return None;
        }
    }

    for i in start..lower.len() {
        let w = lower[i].as_str();
        if GREETING_WORDS.contains(&w) || PLEASANTRY_FILLER.contains(&w) {
            continue;
        }
        if WEEKDAYS.contains(&w) && i > 0 && lower[i - 1] == "happy" {
            continue;
        }
        // Fix round 4: "next" is not filler ("Good for next week" is a
        // decision) — only inside a goodbye ("see you all next week").
        if w == "next" && lower[start..i].windows(2).any(|p| p[0] == "see" && p[1] == "you") {
            continue;
        }
        // An addressee's name: anywhere after an unambiguous greeting, or as
        // the clause's last word after at least one filler word ("Good
        // morning Jeisil" — never "Great Lakes" or "Good Shepherd Health").
        if is_name(i) && (unambiguous || (i + 1 == lower.len() && i >= start + 2)) {
            continue;
        }
        return None;
    }
    let see_you = lower[start..].windows(2).any(|p| p[0] == "see" && p[1] == "you");
    Some(see_you || lower[start..].iter().any(|w| GREETING_WORDS.contains(&w.as_str())))
}

/// A named owner who will DO something outranks a decision word later in the
/// line: "Jeisil will send the doc we decided on" is Jeisil's task.
fn owner_commits_strongly(text: &str) -> bool {
    let first = text.split_whitespace().next().unwrap_or("");
    let lower = first.to_lowercase();
    owner_commitment(text) && lower != "we" && !lower.starts_with("we'")
        && !text.to_lowercase().contains(" will remain")
        && !text.to_lowercase().contains(" will stay")
}

/// "The free trial will be dropped" — a settled change in the passive voice,
/// which has no owner and so is a decision, not a task.
fn passive_settled(lower: &str) -> bool {
    lower
        .split(" will be ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .is_some_and(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).ends_with("ed"))
}

/// Function words that two unrelated lines share ("the", "will"); they say
/// nothing about which line an item came from.
const CITE_STOPWORDS: &[&str] = &[
    "the", "and", "for", "will", "that", "this", "with", "are", "was", "its", "our", "you",
    "has", "have", "from", "not", "but", "into", "about", "also", "then", "than", "they",
];

fn content_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| w.len() >= 3 && !CITE_STOPWORDS.contains(&w.as_str()))
        .collect()
}

fn overlap(item: &[String], line: &str) -> usize {
    let line = content_words(line);
    item.iter().filter(|w| line.contains(w)).count()
}

/// #250 round 2 — make one MAP extract's citations and coverage hold up.
///
/// The real 1.5B run cited the question's line for the "drop the free trial"
/// decision and vice versa (so both timestamps were wrong), and with any
/// prompt wording it skipped one of the plainly-said lines. Both are fixed
/// mechanically, from the chunk's own lines, so nothing here can invent text:
///
/// * **re-cite** — an item whose cited line shares none of its content words
///   moves to the chunk line that shares the most (two or more). The new id is
///   still one of THIS chunk's ids, so the provenance guarantee is unchanged.
/// * **backfill** — a line no item cites, whose own words are an unambiguous
///   commitment by a NAMED owner ("Jeisil will send …"), an explicit decision
///   ("we decided", "we agreed") or an explicit open question ("open
///   question", "…?"), is added verbatim, through the same [`validate_item`].
/// * each list is put back in meeting order.
pub fn ground_extract(extract: &mut MapExtract, lines: &[TranscriptLine], source: &str) {
    ground_extract_with(extract, lines, source, &transcript_names(lines));
}

/// [`ground_extract`] with the meeting-wide name list (B4): the backfill's
/// owner must be one of `names`, or a capitalized word followed by
/// `will`/`should`/`is going to` that is not a small-talk opener.
pub fn ground_extract_with(
    extract: &mut MapExtract,
    lines: &[TranscriptLine],
    source: &str,
    names: &[String],
) {
    for list in [&mut extract.actions, &mut extract.decisions, &mut extract.questions] {
        for item in list.iter_mut() {
            let words = content_words(&item.text);
            let score = |l: &TranscriptLine| overlap(&words, &l.text);
            let cited = lines.iter().find(|l| l.label == item.segment).map_or(0, score);
            // Only a citation that shares NO content word with its item moves,
            // and only to a line that clearly matches (two words or more): the
            // model's id stands whenever it is plausible at all.
            if cited > 0 {
                continue;
            }
            if let Some(best) = lines.iter().max_by_key(|l| (score(l), std::cmp::Reverse(l.label.clone()))) {
                if score(best) >= 2 {
                    item.segment = best.label.clone();
                }
            }
        }
    }
    for line in lines {
        let cited = [&extract.actions, &extract.decisions, &extract.questions]
            .iter()
            .any(|list| list.iter().any(|i| i.segment == line.label));
        if cited {
            continue;
        }
        let text = line.text.trim();
        let lower = text.to_lowercase();
        let first = text.split_whitespace().next().unwrap_or("").to_lowercase();
        let bucket = if open_about_decision(&lower) {
            Bucket::Questions
        } else if ["decided", "we agreed", "agreed that", "agreed the"]
            .iter()
            .any(|m| lower.contains(m))
        {
            Bucket::Decisions
        } else if is_strong_question(&lower) {
            Bucket::Questions
        } else if backfill_commitment(text, names)
            && !matches!(first.as_str(), "i" | "we" | "i'll" | "we'll")
        {
            Bucket::Actions
        } else {
            continue;
        };
        let Some(text) = validate_item(source, text.trim_end_matches('.')) else {
            continue;
        };
        let item = ExtractedItem { text, segment: line.label.clone() };
        match bucket {
            Bucket::Actions => extract.actions.push(item),
            Bucket::Decisions => extract.decisions.push(item),
            Bucket::Questions => extract.questions.push(item),
        }
    }
    for list in [&mut extract.actions, &mut extract.decisions, &mut extract.questions] {
        list.sort_by(|a, b| a.segment.cmp(&b.segment));
        let mut kept: Vec<ExtractedItem> = Vec::with_capacity(list.len());
        for item in list.drain(..) {
            if !kept.iter().any(|k| same_item(&k.text, &item.text)) {
                kept.push(item);
            }
        }
        *list = kept;
    }
}

/// Re-bucket one MAP extract with [`classify_item`]. Order inside each list is
/// kept; a dropped action counts toward [`MapExtract::dropped`].
pub fn classify_extract(extract: &mut MapExtract) {
    let all = [
        (Bucket::Actions, std::mem::take(&mut extract.actions)),
        (Bucket::Decisions, std::mem::take(&mut extract.decisions)),
        (Bucket::Questions, std::mem::take(&mut extract.questions)),
    ];
    for (from, items) in all {
        for item in items {
            match classify_item(from, &item.text) {
                Some(Bucket::Actions) => extract.actions.push(item),
                Some(Bucket::Decisions) => extract.decisions.push(item),
                Some(Bucket::Questions) => extract.questions.push(item),
                None => extract.dropped += 1,
            }
        }
    }
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
    let narrative = drop_unsaid_status(source, narrative.trim());
    let narrative = narrative.as_str();
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

/// #250 round 3 (A5) — status words a summary may only use when the meeting
/// did: "The pricing review … is complete" from a transcript that said "this
/// is the pricing review" claims more than was said.
const STATUS_CLAIMS: &[&str] = &[
    "is complete", "is completed", "was completed", "is done", "was done", "is finished",
    "was finished", "is final", "was finalized", "is finalized", "was a success",
    "is resolved", "was resolved", "went well", "is wrapped up", "was wrapped up",
];

/// Word stems that say something is finished, as a person would say it:
/// "we finished", "it's complete", "we're done with", "wrapped up", "resolved".
const COMPLETION_STEMS: &[&str] = &["finish", "complet", "final", "resolv", "wrap", "success"];

fn says_completion(lower_sentence: &str) -> bool {
    lower_sentence.contains("went well")
        || lower_sentence
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w == "done" || COMPLETION_STEMS.iter().any(|s| w.starts_with(s)))
}

/// Same word, allowing a suffix ("migration" / "migrations", "import" /
/// "imported") — a five-letter shared prefix when both are that long.
fn same_stem(a: &str, b: &str) -> bool {
    a == b || (a.len() >= 5 && b.len() >= 5 && a.as_bytes()[..5] == b.as_bytes()[..5])
}

/// #250 round 4 (R2-B3) — is a status claim in `sentence` something the
/// meeting said? A sentence with no [`STATUS_CLAIMS`] claim always is. One
/// with a claim is supported when SOME source sentence says a completion
/// (stem match: finish / complete / done / final / resolve / wrap / success)
/// about the same noun phrase — the content words before the claim ("The
/// migration is finished" ← "We finished the migration yesterday").
pub fn status_supported(source: &str, sentence: &str) -> bool {
    let l = sentence.to_lowercase();
    let Some(at) = STATUS_CLAIMS.iter().filter_map(|c| l.find(c)).min() else {
        return true;
    };
    let mut np = content_words(&l[..at]);
    if np.is_empty() {
        np = content_words(&l)
            .into_iter()
            .filter(|w| !says_completion(w) && !STATUS_CLAIMS.iter().any(|c| c.contains(w.as_str())))
            .collect();
    }
    split_sentences(source).iter().any(|src| {
        let src = src.to_lowercase();
        if !says_completion(&src) {
            return false;
        }
        let words = content_words(&src);
        np.iter().all(|w| words.iter().any(|sw| same_stem(sw, w)))
    })
}

/// Abbreviations whose period never ends a sentence.
const ABBREVIATIONS: &[&str] = &[
    "e.g", "i.e", "eg", "ie", "dr", "mr", "mrs", "ms", "prof", "sr", "jr", "st", "vs", "approx",
    "cf", "ca", "mt",
];

/// #250 round 4 (R2-B3) — split prose at REAL sentence ends only: a `.`/`!`/`?`
/// followed by whitespace or the end. Never between digits ("$49.99",
/// "3.30pm", "v1.2"), never after an abbreviation ("e.g.", "Dr."), and never
/// after "a.m." / "p.m." when the next word goes on in lower case.
pub fn split_sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    for (i, &ch) in chars.iter().enumerate() {
        cur.push(ch);
        if !matches!(ch, '.' | '!' | '?') {
            continue;
        }
        let next = chars.get(i + 1).copied();
        if next.is_some_and(|c| !c.is_whitespace()) {
            continue;
        }
        if ch == '.' {
            let last = cur
                .trim_end_matches('.')
                .rsplit(char::is_whitespace)
                .next()
                .unwrap_or("")
                .trim_start_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase();
            if ABBREVIATIONS.contains(&last.as_str()) {
                continue;
            }
            let next_word = chars[i + 1..]
                .iter()
                .find(|c| !c.is_whitespace())
                .copied();
            if matches!(last.as_str(), "a.m" | "p.m")
                && next_word.is_some_and(|c| c.is_lowercase() || c.is_ascii_digit())
            {
                continue;
            }
        }
        let s = cur.trim().to_string();
        if !s.is_empty() {
            out.push(s);
        }
        cur.clear();
    }
    let tail = cur.trim();
    if !tail.is_empty() {
        out.push(tail.to_string());
    }
    out
}

/// Remove each narrative sentence whose status claim the source does not
/// support ([`status_supported`]). Whole sentences only — never a reworded
/// half — split at real sentence ends ([`split_sentences`]) so "$49.99"
/// stays "$49.99". R2-B3: never empties a summary — when every sentence would
/// go, the narrative is returned as it was and the other gates decide.
fn drop_unsaid_status(source: &str, narrative: &str) -> String {
    let parts = split_sentences(narrative);
    if parts.iter().all(|s| status_supported(source, s)) {
        return narrative.to_string();
    }
    let kept: Vec<String> = parts
        .into_iter()
        .filter(|s| status_supported(source, s))
        .collect();
    if kept.is_empty() {
        log::warn!("summary: every sentence claims a status the meeting never stated — kept as written");
        return narrative.to_string();
    }
    kept.join(" ")
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
    content_gates(source, output)
}

/// V4–V7: the gates that hold for every piece of stored text, whatever its
/// groundedness floor (an item's, the narrative's, or a title's).
fn content_gates(source: &str, output: &str) -> Option<()> {
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
    /// Has this run been cancelled? Checked between stages so a cancel that
    /// landed inside a pass whose error was swallowed (REDUCE, HEADLINE)
    /// still stores nothing. A client with no cancel says `false`.
    fn cancelled(&self) -> bool {
        false
    }
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

impl MeetingSummary {
    /// Does this summary carry anything a reader could use?
    ///
    /// Content, deliberately — not `markdown.is_empty()`. `render_summary` adds
    /// a truncation note whenever a pass was cut, so a summary with nothing in
    /// it can still render as non-empty Markdown; storing THAT over a good
    /// summary loses exactly as much as storing the "_No summary could be
    /// produced_" placeholder did. One item and no narrative, on the other hand,
    /// is a real (if thin) summary and is worth keeping.
    pub fn is_empty(&self) -> bool {
        self.narrative.trim().is_empty()
            && self.actions.is_empty()
            && self.decisions.is_empty()
            && self.questions.is_empty()
    }
}

/// An item with its citation resolved back to a wall-clock offset — and, since
/// YV133, to a speaker.
#[derive(Debug, Clone, PartialEq)]
pub struct SummaryItem {
    pub text: String,
    pub segment: String,
    pub start_seconds: f64,
    /// YV133 — **who spoke the segment this item cites**, looked up from the
    /// evidence id after parsing. Never read from model output; see this
    /// module's header and [`ExtractedItem`]'s.
    ///
    /// Said precisely, because the difference is load-bearing: this is who
    /// *said* the line the item was extracted from, which is the strongest
    /// attribution a mechanical lookup can make. It is not a claim about who
    /// OWES an action — "could you send that over?" cites the person asking,
    /// not the person doing — which is why [`render_summary`] renders it as an
    /// attribution beside the timestamp rather than as an owner in front of the
    /// text.
    ///
    /// `None` whenever the [`SpeakerSource`] declined, which today is every
    /// meeting on the clustering branch.
    pub speaker: Option<String>,
}

/// Summarize a meeting's segments end to end: chunk, MAP, merge, REDUCE, render.
///
/// Every stage fails toward LESS output rather than wrong output — a chunk whose
/// MAP pass fails contributes nothing and does not stop the others, and a REDUCE
/// that fails or is rejected falls back to the chunk narratives it was given.
///
/// The floor under "less output" is `Err`, not an empty `Ok`, and it is checked
/// TWICE — once on the calls and once on what they produced, because those are
/// two different ways to end up with nothing:
///
/// * every chunk's MAP pass FAILED — the sidecar is down or wedged, and there is
///   nothing to summarize from (`extracts.is_empty()` below → [`SummaryError::Protocol`]);
/// * every chunk ANSWERED and the answers carried nothing — a grammar-legal
///   `{"narrative":"","actions":[],…}` from every pass, which is what a wedged
///   model or an overflowed context looks like from the outside
///   ([`MeetingSummary::is_empty`] below → [`SummaryError::Empty`]).
///
/// Both are `Err` for one reason: a caller handed an empty summary as `Ok`
/// writes it over the summary the user already had, in an app with no undo and
/// no version history, and reports success while doing it. The second check is
/// the one that matters most, because the first only catches the loud version.
pub fn summarize_segments(
    segments: &[MeetingSegment],
    client: &dyn SummaryClient,
) -> Result<MeetingSummary, SummaryError> {
    summarize_segments_with(segments, client, &NoSpeakers)
}

/// The same, with speakers.
///
/// `speakers` decides two things and nothing else: what each transcript line
/// says beside its id in the model's prompt, and what
/// [`SummaryItem::speaker`] is filled with afterwards. It cannot influence
/// which items survive, which ids are citable, or what any validator does —
/// the speaker is read out of the same table the line was rendered from, keyed
/// by the id the grammar already constrained the model to.
pub fn summarize_segments_with(
    segments: &[MeetingSegment],
    client: &dyn SummaryClient,
    speakers: &dyn SpeakerSource,
) -> Result<MeetingSummary, SummaryError> {
    let lines = transcript_lines_with(segments, speakers);
    if lines.is_empty() {
        return Err(SummaryError::NoSegments);
    }
    let chunks = plan_chunks(&lines, MAP_CHUNK_TOKENS, client)?;
    let names = transcript_names(&lines);
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
        // #250 round 2: the output budget is sized to THIS chunk (a fixed 288
        // ran out on a six-line meeting), and the deadline grows with it so a
        // bigger budget can never turn into a `deadline` failure instead.
        let max_out = summary_map_max_out(chunk.tokens);
        let req = PolishRequest::summarize(
            next_id(),
            SUMMARIZE_MAP,
            max_out,
            MAP_DEADLINE_MS.max(u64::from(max_out) * 80),
            source.clone(),
            Some(grammar),
        );
        match client.generate(&req) {
            // Parsed defensively: a MAP pass that hit its token budget
            // mid-array still contributes every item that closed.
            Ok(answer) => {
                truncated |= answer.truncated;
                let mut extract = parse_map_output(&answer.text, &labels, &source);
                // #250 round 2: the model's bucket is a suggestion; the line's
                // own words decide (a question is never an action item).
                classify_extract(&mut extract);
                ground_extract_with(&mut extract, &chunk.lines, &source, &names);
                // The chunk narrative goes through the same gate the items do.
                // `parse_map_output` leaves it raw on purpose — it is a parser,
                // and the source a narrative is grounded against is a decision
                // for the caller, not for the recovery scan.
                extract.narrative =
                    validate_narrative(&source, &extract.narrative).unwrap_or_default();
                extracts.push(extract);
            }
            // B2: a cancel is not a failed chunk to skip past — the run stops
            // and stores nothing.
            Err(SummaryError::Cancelled) => return Err(SummaryError::Cancelled),
            Err(e) => log::warn!("summary: a MAP pass failed ({}) — continuing", e.tag()),
        }
    }
    if client.cancelled() {
        return Err(SummaryError::Cancelled);
    }
    // One failed chunk is survivable — the others still carry the meeting. ZERO
    // surviving chunks is not a thin summary, it is no summary: the narrative
    // would render as the "_No summary could be produced_" placeholder, and the
    // caller would happily write that over a good summary the user already had,
    // in an app with no undo and no version history. A meeting that genuinely
    // had nothing to extract still produces an EMPTY extract per chunk here, so
    // this branch means the model calls failed, not that the meeting was quiet.
    if extracts.is_empty() && !chunks.is_empty() {
        log::warn!(
            "summary: every one of {} MAP pass(es) failed — refusing to overwrite with a placeholder",
            chunks.len()
        );
        return Err(SummaryError::Protocol);
    }
    let merged = merge_extracts(&extracts);

    // #250 round 4 (R2-B1): KEY POINTS FIRST, from the transcript, then the
    // Summary paragraph written from them (same call). Only when that
    // paragraph does not hold up does the MAP → REDUCE narrative stand in.
    let mut headline = headline_pass(client, &lines, &names, &merged.narratives);
    if client.cancelled() {
        return Err(SummaryError::Cancelled);
    }
    let narrative = if headline.summary.trim().is_empty() {
        let (narrative, reduce_truncated) =
            reduce_narrative(&merged.narratives, &full_text, client);
        truncated |= reduce_truncated;
        narrative
    } else {
        headline.summary.clone()
    };
    if client.cancelled() {
        return Err(SummaryError::Cancelled);
    }
    if headline.title.trim().is_empty() {
        headline.title = fallback_title(&narrative);
    }
    // The evidence table: for every citable id, the two facts about it that are
    // already known here — when it was said, and (YV133) who said it. Both are
    // looked up by the SAME key, the id the grammar constrained the model to
    // emit, which is what makes the speaker exactly as provenanced as the
    // timestamp already was.
    let evidence: Vec<(String, f64, Option<String>)> = lines
        .iter()
        .map(|l| {
            (
                l.label.clone(),
                l.start_seconds,
                l.speaker.as_ref().map(|s| s.label().to_string()),
            )
        })
        .collect();
    let resolve = |items: &[ExtractedItem]| -> Vec<SummaryItem> {
        items
            .iter()
            .map(|i| {
                let cited = evidence.iter().find(|(label, _, _)| label == &i.segment);
                SummaryItem {
                    text: i.text.clone(),
                    segment: i.segment.clone(),
                    start_seconds: cited.map(|(_, at, _)| *at).unwrap_or(0.0),
                    // Never `i.<anything>`: the model's answer supplies the id
                    // and nothing else about who spoke.
                    speaker: cited.and_then(|(_, _, who)| who.clone()),
                }
            })
            .collect()
    };
    let actions = resolve(&merged.actions);
    let decisions = resolve(&merged.decisions);
    let questions = resolve(&merged.questions);
    let markdown = render_notes(
        &headline,
        &names,
        &narrative,
        &actions,
        &decisions,
        &questions,
        truncated_chunks,
        chunks.len(),
        truncated,
    );
    let summary = MeetingSummary {
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
    };
    // The other half of the floor. Every pass answered, and between them they
    // said nothing: no narrative, no action, no decision, no question. What
    // `render_summary` produced from that is either the "_No summary could be
    // produced_" placeholder or a bare truncation note, and neither is a summary
    // — handing either back as `Ok` is how a good stored summary got overwritten
    // by a bad model run. A meeting that was genuinely quiet lands here too, and
    // that is the right answer for it as well: there is nothing to store, so the
    // row keeps whatever it had and the user is told nothing came back.
    if summary.is_empty() {
        log::warn!(
            "summary: {} MAP pass(es) answered with no content — refusing to store an empty summary",
            chunks.len()
        );
        return Err(SummaryError::Empty);
    }
    Ok(summary)
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
            let at = format_offset(item.start_seconds);
            // YV133 — the speaker rides WITH the citation, not in front of the
            // text. `- Jeisil: send the pricing doc` reads as an assignment,
            // and the lookup does not support one: it knows who spoke the cited
            // line, not who agreed to act on it. `(Jeisil, 00:04:12)` says
            // exactly what is known — this came from Jeisil, here.
            match &item.speaker {
                Some(who) => out.push_str(&format!("- {} ({who}, {at})\n", item.text)),
                None => out.push_str(&format!("- {} ({at})\n", item.text)),
            }
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

// ── focus-2026-10-19: zero-setup notes — headline, owners, model plan, cancel ─

/// The model a meeting summary uses when the user picked none: the catalog's
/// rank-1 polish model (Qwen2.5 1.5B Instruct Q4_K_M, 1.04 GiB GGUF, run by
/// the bundled llama.cpp sidecar). 0.5B was tried for the role and is too weak
/// on multi-speaker extraction; the bigger choice stays in Settings.
pub const DEFAULT_SUMMARY_MODEL: &str = "qwen2.5-1.5b-instruct-q4_k_m";

/// What the summary must do about its model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummaryModelPlan {
    /// This model is on disk — use it.
    Use(String),
    /// Nothing usable is on disk — fetch this one (managed, into the app's
    /// own models dir, with a progress row), then use it.
    Download(String),
}

/// PURE. The user's polish model wins when it is on disk; else the default
/// when it is; else download the default. Never "no model".
pub fn plan_summary_model(chosen: &str, on_disk: &dyn Fn(&str) -> bool) -> SummaryModelPlan {
    let chosen = chosen.trim();
    if !chosen.is_empty() && on_disk(chosen) {
        return SummaryModelPlan::Use(chosen.to_string());
    }
    if on_disk(DEFAULT_SUMMARY_MODEL) {
        return SummaryModelPlan::Use(DEFAULT_SUMMARY_MODEL.to_string());
    }
    SummaryModelPlan::Download(DEFAULT_SUMMARY_MODEL.to_string())
}

// #250 round 3 (B2, A4): the cancel is per job — a
// `summary_jobs::CancelToken` handed to the client — never a process-global
// flag that a new summary's start would wipe.

/// A meeting's title, key points and — #250 round 4 (R2-B1) — the Summary
/// paragraph written FROM those key points.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Headline {
    pub title: String,
    pub key_points: Vec<String>,
    /// The paragraph the model wrote from its own key points, validated
    /// against the transcript; empty when it wrote none that held up (the
    /// MAP → REDUCE narrative is used then).
    pub summary: String,
}

pub const MIN_KEY_POINTS: usize = 3;
pub const MAX_KEY_POINTS: usize = 6;
const MAX_TITLE_WORDS: usize = 10;

/// #250 round 4 (R2-B1) — two key points whose content words overlap this
/// much (shared ÷ the smaller set) are about the same thing; the later one
/// goes. Overlap with the Summary, a decision or an action item is NOT
/// checked: the Summary is written from the key points, so it is expected.
pub const KEY_POINT_OVERLAP_MAX: f64 = 0.6;

/// The HEADLINE grammar: exactly the JSON shape — the title, 3–6 key points,
/// then the summary paragraph. Key points come BEFORE the summary on
/// purpose: the model writes the facts first and the paragraph from them.
pub const HEADLINE_GRAMMAR: &str = r#"root ::= "{" ws "\"title\":" ws str "," ws "\"key_points\":" ws "[" ws str ws "," ws str ws "," ws str (ws "," ws str)? (ws "," ws str)? (ws "," ws str)? ws "]" ws "," ws "\"summary\":" ws str ws "}"
str ::= "\"" [^"\\\n]+ "\""
ws ::= [ \t\n]*"#;

#[derive(Deserialize)]
struct RawHeadline {
    #[serde(default)]
    title: String,
    #[serde(default)]
    key_points: Vec<String>,
    #[serde(default)]
    summary: String,
}

/// Share of a TITLE's content words that must be in the notes. Lower than an
/// item's [`GROUNDEDNESS_FLOOR`] on purpose: a title names the topic, so one
/// abstracting word ("review", "sync") per grounded one is the job, not an
/// invention — "Drivia consulting pricing review" over notes that say "reviewed
/// pricing for the Drivia consulting tier" is 3 of 4. V4–V7 still apply in
/// full: a title never carries a number, contact or template the notes lack.
pub const TITLE_GROUNDEDNESS_FLOOR: f64 = 0.5;

/// A title the notes support, or `None`.
pub fn validate_title(source: &str, title: &str) -> Option<String> {
    let title = title.trim();
    if title.is_empty() {
        return reject("v1_empty");
    }
    if title.split_whitespace().count() > MAX_TITLE_WORDS {
        return reject("v2_runaway");
    }
    if grounded(source, title) < TITLE_GROUNDEDNESS_FLOOR {
        return reject("v3_ungrounded");
    }
    content_gates(source, title)?;
    Some(title.to_string())
}

/// Our own section labels, which a 1.5B model sometimes echoes into a point.
fn strip_section_label(p: &str) -> String {
    ["Decisions:", "Action items:", "Key points:", "Summary:"]
        .iter()
        .find_map(|l| p.trim().strip_prefix(l))
        .map_or(p.trim(), str::trim_start)
        .to_string()
}

/// Parse + validate a HEADLINE answer against `source` (the transcript it
/// was written from). `None` when nothing usable survived.
pub fn parse_headline(raw: &str, source: &str) -> Option<Headline> {
    let cleaned = strip_fences(raw);
    let start = cleaned.find('{')?;
    let end = cleaned.rfind('}')?;
    let parsed: RawHeadline = serde_json::from_str(cleaned.get(start..=end)?).ok()?;
    let title = parsed
        .title
        .trim()
        .trim_matches('"')
        .trim_end_matches('.')
        .to_string();
    let title_ok = validate_title(source, &title).is_some();
    let valid: Vec<String> = parsed
        .key_points
        .iter()
        .map(|p| strip_section_label(p))
        // R2-B3: a key point is held to the same status rule as the Summary.
        .filter(|p| status_supported(source, p))
        .filter_map(|p| validate_item(source, &p))
        .collect();
    let mut key_points = dedupe_key_points(valid);
    key_points.truncate(MAX_KEY_POINTS);
    let summary = if parsed.summary.trim().is_empty() {
        String::new()
    } else {
        validate_narrative(source, &parsed.summary).unwrap_or_default()
    };
    if !title_ok && key_points.is_empty() && summary.is_empty() {
        return None;
    }
    Some(Headline {
        title: if title_ok { title } else { String::new() },
        key_points,
        summary,
    })
}

fn sentences(text: &str) -> Vec<String> {
    split_sentences(text)
        .into_iter()
        .filter(|s| s.split_whitespace().count() >= 3)
        .collect()
}

/// The mechanical title: the narrative's first sentence, cut to 8 words.
pub fn fallback_title(narrative: &str) -> String {
    sentences(narrative)
        .first()
        .map(|s| {
            s.trim_end_matches(['.', '!', '?'])
                .split_whitespace()
                .take(8)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_else(|| "Meeting notes".to_string())
}

/// What the HEADLINE pass reads: the transcript itself as `Speaker: text`
/// lines (no ids — a point never needs one), or, when the transcript does not
/// fit one pass, the chunk narratives MAP already wrote from it.
pub fn headline_source(
    lines: &[TranscriptLine],
    narratives: &[String],
    counter: &dyn TokenCounter,
) -> String {
    let plain = lines
        .iter()
        .map(|l| match &l.speaker {
            Some(sp) => format!("{}: {}", sp.label(), one_line(&l.text)),
            None => one_line(&l.text),
        })
        .collect::<Vec<_>>()
        .join("\n");
    match counter.count_tokens(&plain) {
        Ok(n) if n <= REDUCE_CHUNK_TOKENS => plain,
        _ => {
            let joined = narratives
                .iter()
                .filter(|n| !n.trim().is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join("\n\n");
            if joined.trim().is_empty() {
                clamp_words(&plain, MAX_NARRATIVE_WORDS * 3)
            } else {
                clamp_words(&joined, MAX_NARRATIVE_WORDS * 3)
            }
        }
    }
}

/// The HEADLINE pass — KEY POINTS FIRST, from the transcript (R2-B1).
///
/// One grammar-shaped call returns the title, 3–6 key points and the summary
/// paragraph written from those points. Points are validated against the
/// transcript, deduped ONLY against each other ([`dedupe_key_points`]), and
/// backfilled verbatim from the transcript's most informative sentences
/// ([`backfill_key_points`]) when fewer than [`MIN_KEY_POINTS`] survive. A
/// cancel or a failed call is the backfill alone (title and summary empty —
/// the caller fills them from the narrative).
pub fn headline_pass(
    client: &dyn SummaryClient,
    lines: &[TranscriptLine],
    names: &[String],
    narratives: &[String],
) -> Headline {
    let full_text = lines
        .iter()
        .map(TranscriptLine::render)
        .collect::<Vec<_>>()
        .join("\n");
    if lines.is_empty() {
        return Headline::default();
    }
    let source = headline_source(lines, narratives, client);
    let req = PolishRequest::summarize(
        next_id(),
        SUMMARIZE_HEADLINE,
        SUMMARY_HEADLINE_MAX_OUT,
        REDUCE_DEADLINE_MS,
        source,
        Some(HEADLINE_GRAMMAR.to_string()),
    );
    let got = if client.cancelled() {
        None
    } else {
        match client.generate(&req) {
            Ok(answer) => parse_headline(&answer.text, &full_text),
            Err(e) => {
                log::warn!(
                    "summary: headline pass failed ({}) — key points come from the transcript",
                    e.tag()
                );
                None
            }
        }
    };
    let mut h = got.unwrap_or_default();
    h.key_points = backfill_key_points(h.key_points, lines, names, &full_text);
    h
}

/// Content-word overlap of two points: shared ÷ the smaller set. 1.0 when
/// one point's facts are all in the other.
pub fn point_overlap(a: &str, b: &str) -> f64 {
    let set = |t: &str| {
        let mut w = content_words(t);
        w.sort();
        w.dedup();
        w
    };
    let (wa, wb) = (set(a), set(b));
    let small = wa.len().min(wb.len());
    if small == 0 {
        return if wa.is_empty() && wb.is_empty() { 1.0 } else { 0.0 };
    }
    let shared = wa.iter().filter(|w| wb.contains(w)).count();
    shared as f64 / small as f64
}

/// #250 round 4 (R2-B1) — key points that are each a different fact. A point
/// is dropped ONLY when it is empty or overlaps an earlier KEY POINT by
/// [`KEY_POINT_OVERLAP_MAX`] or more — never because the Summary, a decision
/// or an action item says the same thing (the Summary is written from these).
pub fn dedupe_key_points(points: Vec<String>) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for p in points {
        let p = strip_section_label(&p);
        if content_words(&p).is_empty() {
            continue;
        }
        if kept.iter().any(|k| point_overlap(k, &p) >= KEY_POINT_OVERLAP_MAX) {
            continue;
        }
        kept.push(p);
    }
    kept
}

/// Day, month and relative-date words: a sentence carrying one says WHEN.
const DATE_WORDS: &[&str] = &[
    "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday", "january",
    "february", "march", "april", "june", "july", "august", "september", "october",
    "november", "december", "today", "tomorrow", "tonight", "week", "month", "quarter", "year",
];

/// Number words count as numbers ("forty nine dollars").
const NUMBER_WORDS: &[&str] = &[
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven",
    "twelve", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    "hundred", "thousand", "million", "billion", "percent", "dollars", "dollar",
];

/// How much a transcript sentence says: numbers, names, dates, length.
fn information_score(sentence: &str, names: &[String]) -> usize {
    let lower = sentence.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric() && c != '$')
        .filter(|w| !w.is_empty())
        .collect();
    let numbers = crate::polish::digit_runs(sentence).len()
        + words.iter().filter(|w| NUMBER_WORDS.contains(w)).count().min(2);
    let dates = words.iter().filter(|w| DATE_WORDS.contains(w)).count().min(2);
    let named = names
        .iter()
        .filter(|n| sentence.split(|c: char| !c.is_alphanumeric()).any(|w| w == n.as_str()))
        .count()
        .min(2);
    3 * numbers + 2 * dates + 2 * named + usize::from(content_words(sentence).len() >= 5)
}

/// R2-B1 — top `points` up to [`MIN_KEY_POINTS`] with the transcript's
/// highest-information sentences (numbers, names, dates), VERBATIM, each one
/// distinct from every point already kept. Never a Summary sentence, never
/// a model rewording — only what was said, through [`validate_item`].
pub fn backfill_key_points(
    points: Vec<String>,
    lines: &[TranscriptLine],
    names: &[String],
    source: &str,
) -> Vec<String> {
    let mut kept = dedupe_key_points(points);
    if kept.len() >= MIN_KEY_POINTS {
        return kept;
    }
    let mut candidates: Vec<(usize, usize, String)> = Vec::new();
    for l in lines {
        for s in sentences(&l.text) {
            let s = s.trim().to_string();
            let lower = s.to_lowercase();
            if s.ends_with('?') || is_strong_question(&lower) {
                continue;
            }
            if content_words(&s).len() < 3 {
                continue;
            }
            let score = information_score(&s, names);
            candidates.push((score, candidates.len(), s));
        }
    }
    // Most informative first; the meeting's order breaks ties.
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for (_, _, s) in candidates {
        if kept.len() >= MIN_KEY_POINTS {
            break;
        }
        let Some(ok) = validate_item(source, &s) else { continue };
        if kept.iter().any(|k| point_overlap(k, &ok) >= KEY_POINT_OVERLAP_MAX) {
            continue;
        }
        kept.push(ok);
    }
    kept
}

/// "Owner" of an action item, ONLY when the item's own words name one at the
/// front ("Jeisil will send …", "Wilson to book …") AND (R2-B2) that word is
/// one of the transcript's `names` ([`transcript_names`]). Never inferred: a
/// capitalized first word nobody in the meeting is called is not a bold owner.
pub fn action_owner(text: &str, names: &[String]) -> Option<String> {
    let mut words = text.split_whitespace();
    let first = words.next()?.trim_matches(|c: char| !c.is_alphanumeric());
    let second = words.next()?.to_lowercase();
    let is_name = names.iter().any(|n| n == first)
        && is_name_word(first)
        && first.chars().next()?.is_uppercase()
        && first.chars().skip(1).all(|c| c.is_lowercase())
        && !NOT_A_NAME.contains(&first.to_lowercase().as_str())
        && !matches!(
            first,
            "We" | "They"
                | "The"
                | "Someone"
                | "Everyone"
                | "I"
                | "You"
                | "He"
                | "She"
                | "It"
                | "This"
                | "That"
        );
    let commits = matches!(
        second.as_str(),
        "will" | "to" | "should" | "needs" | "is" | "owns" | "agreed"
    );
    (is_name && commits).then(|| first.to_string())
}

/// The stored notes: title, one-paragraph summary, key points, decisions,
/// action items (owner when said), open questions. Markdown — what the card,
/// Open transcript and Export Markdown all show.
#[allow(clippy::too_many_arguments)]
pub fn render_notes(
    headline: &Headline,
    names: &[String],
    narrative: &str,
    actions: &[SummaryItem],
    decisions: &[SummaryItem],
    questions: &[SummaryItem],
    truncated_chunks: usize,
    chunks: usize,
    truncated: bool,
) -> String {
    let body = render_summary(
        narrative,
        &[],
        decisions,
        questions,
        truncated_chunks,
        chunks,
        truncated,
    );
    let mut out = String::new();
    if !headline.title.trim().is_empty() {
        out.push_str(&format!("## {}\n\n", headline.title.trim()));
    }
    let (summary_para, rest) = if narrative.trim().is_empty() {
        (String::new(), body.as_str())
    } else {
        (
            narrative.trim().to_string(),
            body.strip_prefix(narrative.trim()).unwrap_or(&body),
        )
    };
    if !summary_para.is_empty() {
        out.push_str("### Summary\n\n");
        out.push_str(&summary_para);
        out.push_str("\n\n");
    }
    // review-opus2: one lone point is not a section — fewer than two and
    // the Key points heading is left out.
    if headline.key_points.len() >= 2 {
        out.push_str("### Key points\n\n");
        for p in &headline.key_points {
            // #250 round 2: the HEADLINE input carries "Decisions:" /
            // "Action items:" labels, and a 1.5B model sometimes echoes one
            // into a point. The label is ours, never the meeting's.
            out.push_str(&format!("- {}\n", strip_section_label(p)));
        }
        out.push('\n');
    }
    if !actions.is_empty() {
        out.push_str("### Action items\n\n");
        for item in actions {
            let at = format_offset(item.start_seconds);
            let owner = action_owner(&item.text, names)
                .map(|o| format!("**{o}** — "))
                .unwrap_or_default();
            match &item.speaker {
                Some(who) => out.push_str(&format!("- {owner}{} ({who}, {at})\n", item.text)),
                None => out.push_str(&format!("- {owner}{} ({at})\n", item.text)),
            }
        }
        out.push('\n');
    }
    let rest = rest.trim();
    if !rest.is_empty() && !rest.starts_with("_No summary could be produced") {
        out.push_str(rest);
    }
    if out.trim().is_empty() {
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
/// review-opus2 A7 — the pids of every live summary sidecar, so app quit can
/// stop them explicitly rather than trust a Drop that a quit may never run.
fn live_sidecars() -> &'static std::sync::Mutex<Vec<u32>> {
    static LIVE: std::sync::OnceLock<std::sync::Mutex<Vec<u32>>> = std::sync::OnceLock::new();
    LIVE.get_or_init(Default::default)
}

/// Stop every live summary sidecar (app quit). Returns how many were signalled.
pub fn stop_all_sidecars() -> usize {
    let pids: Vec<u32> = std::mem::take(&mut *live_sidecars().lock().unwrap_or_else(|p| p.into_inner()));
    for pid in &pids {
        // SAFETY: plain kill(2) on a pid this process spawned and still owns.
        unsafe {
            libc::kill(*pid as libc::pid_t, libc::SIGKILL);
        }
    }
    pids.len()
}

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
        live_sidecars()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(child.id());
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

    /// One request/response. review-opus2 A4: the wait polls `cancel` every
    /// 100 ms and a cancel KILLS the sidecar, so Cancel ends a 60–90 s model
    /// pass at once instead of after it.
    fn exchange(
        &mut self,
        req: &PolishRequest,
        cancel: &crate::summary_jobs::CancelToken,
    ) -> Result<PolishResponse, SummaryError> {
        let line = serde_json::to_string(req).map_err(|_| SummaryError::Protocol)?;
        writeln!(self.stdin, "{line}").map_err(|_| SummaryError::Unavailable)?;
        self.stdin.flush().map_err(|_| SummaryError::Unavailable)?;
        let deadline = Instant::now() + Duration::from_millis(req.deadline_ms.max(1));
        loop {
            let left = deadline
                .checked_duration_since(Instant::now())
                .ok_or(SummaryError::Protocol)?;
            if cancel.is_cancelled() {
                let _ = self.child.kill();
                return Err(SummaryError::Cancelled);
            }
            match self.lines.recv_timeout(left.min(Duration::from_millis(100))) {
                Ok(line) => {
                    if let Some(response) = parse_response_for(&line, req.id) {
                        return Ok(response);
                    }
                }
                Err(RecvTimeoutError::Timeout) if left > Duration::from_millis(100) => {}
                Err(RecvTimeoutError::Timeout) => return Err(SummaryError::Protocol),
                Err(RecvTimeoutError::Disconnected) => return Err(SummaryError::Unavailable),
            }
        }
    }
}

impl Drop for SidecarSession {
    fn drop(&mut self) {
        let pid = self.child.id();
        live_sidecars()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .retain(|p| *p != pid);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The production client: one dedicated sidecar, for the length of one job.
pub struct SidecarSummaryClient {
    session: std::sync::Mutex<SidecarSession>,
    model: String,
    cancel: crate::summary_jobs::CancelToken,
}

impl SidecarSummaryClient {
    pub fn new(session: SidecarSession) -> Self {
        Self::with_cancel(session, crate::summary_jobs::CancelToken::new())
    }

    /// The client a notes job uses: every model call checks `cancel` first.
    pub fn with_cancel(session: SidecarSession, cancel: crate::summary_jobs::CancelToken) -> Self {
        let model = session.model.clone();
        Self {
            session: std::sync::Mutex::new(session),
            model,
            cancel,
        }
    }

    fn call(&self, req: &PolishRequest) -> Result<PolishResponse, SummaryError> {
        let mut session = self.session.lock().map_err(|_| SummaryError::Protocol)?;
        session.exchange(req, &self.cancel)
    }
}

impl TokenCounter for SidecarSummaryClient {
    fn count_tokens(&self, text: &str) -> Result<usize, SummaryError> {
        if self.cancel.is_cancelled() {
            return Err(SummaryError::Cancelled);
        }
        let req = PolishRequest::count_tokens(next_id(), COUNT_DEADLINE_MS, text.to_string());
        self.call(&req)?
            .token_count()
            .map(|n| n as usize)
            .ok_or(SummaryError::Protocol)
    }
}

impl SummaryClient for SidecarSummaryClient {
    fn generate(&self, req: &PolishRequest) -> Result<Generated, SummaryError> {
        // Cancel stops the summary at its next model call (B2: as Cancelled,
        // which the pipeline turns into "store nothing", not a skipped chunk).
        if self.cancel.is_cancelled() {
            return Err(SummaryError::Cancelled);
        }
        let response = self.call(req)?;
        let truncated = response.was_truncated();
        if truncated {
            log::warn!("summary: the sidecar truncated a pass to fit its context");
        }
        // #250 round 2: an answer that reached its output budget dropped none of
        // the INPUT, so it is not `truncated` and earns no reader-facing note —
        // the partial answer is parsed defensively and keeps every closed item.
        if response.was_out_capped() {
            log::warn!("summary: a pass reached its output budget; keeping the closed items");
        }
        response
            .into_text()
            .map(|text| Generated { text, truncated })
            .ok_or(SummaryError::Protocol)
    }

    fn model_id(&self) -> String {
        self.model.clone()
    }

    fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
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
