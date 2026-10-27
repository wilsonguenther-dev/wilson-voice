//! yap-format-structure — the structure he SPOKE becomes the structure he would
//! have TYPED (Wispr Flow parity): enumerations become lists, "this thing:
//! verdict" lines become bullets, and a structured take is laid out in blocks.
//!
//! Wilson, 2026-10-16: "That's what Yap needs to be able to do. It knew I was
//! doing a list of things." Fixture 01 under
//! `yap-polish/fixtures/structure/` is that take, with Wispr's output as the
//! ground truth.
//!
//! # Contract
//!
//! * **PURE and rules-only.** No LLM, no clock in the output path, no
//!   randomness, no hash-map iteration order. Same transcript + pauses + target
//!   ⇒ byte-identical output (`structure_is_deterministic_three_runs`). Y4-D is
//!   law: list detection never depends on a polish model.
//! * **std only.** This file is compiled into BOTH crates — the app (`mod
//!   structure` in `lib.rs`) and the `yap-polish` sidecar
//!   (`#[path = "../../src-tauri/src/structure.rs"] mod structure;`, the same
//!   pattern as `polish_protocol.rs`). So: no serde, no `crate::` paths.
//! * **Never his words, only their layout.** No new content, no summaries, no
//!   reordering. The only words that may disappear are the spoken cue words in
//!   [`STRUCTURE_CUE_WORDS`], and only when they were consumed as a marker.
//!   "heading", "title", "bullet" and the sequence openers are ordinary
//!   words, so they count as a marker only when SET OFF by a comma or colon
//!   (`strip_marker`): "Heading home now" and "First of all" stay his words.
//!   `structure_never_invents_content_words` checks both directions over every
//!   fixture and over seeded shuffles of them.
//! * **Conservative.** A take where no list or heading forms comes back
//!   BYTE-FOR-BYTE as it went in — pause-only paragraphing of plain prose stays
//!   with `dictation::insert_paragraphs`, which runs after this stage and is
//!   skipped only when this stage actually structured the take (so it can never
//!   cut a paragraph through a list this stage produced). A text that already
//!   carries a line break (a spoken "new line", `format_dictation`'s own list,
//!   an email shape) is left alone too.
//!
//! # Signals
//!
//! Speech has no markup; it has punctuation the recogniser guessed and the
//! silences the speaker actually left. Both are used:
//!
//! | signal | effect |
//! |---|---|
//! | pause ≥ [`BLOCK_PAUSE_SECONDS`] at a sentence end | a new block (line) |
//! | pause ≥ [`BEAT_PAUSE_SECONDS`] at a sentence end | an item boundary — 2+ of them in one block make that block a bulleted list |
//! | pause ≥ [`BEAT_PAUSE_SECONDS`] at a comma after a short topic | `Topic, verdict` → `Topic: verdict` (the colon-like pause) |
//! | "first … then … finally" / "step one …" opening 3+ sentences | a numbered list, cue words consumed |
//! | 3+ sentences with the same two-word opener ("make sure X. make sure Y.") | a bulleted list |
//! | 3+ short comma-separated phrases with one shared opener and NO closing "and/or" ("built, the security, the data, the telemetry") | the lead-in gets a colon, the phrases become bullets |
//! | "heading …" / "title …" opening a sentence | a heading, never invented |
//!
//! A list always has at least [`MIN_LIST_ITEMS`] items (the Y4-D single-item
//! rule), and a series that closes on "and"/"or" ("very good, very great, and
//! easily understood") is prose, not a list. A narrative "and then we drove,
//! and then we ate" never lists: conjunction-led phrases are never items.

/// What the paste target can render, and therefore which syntax this stage
/// may emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureFormat {
    /// `- item`, `1. item`, `## Heading` — editors, notes, Slack, Notion-like.
    Markdown,
    /// `• item`, `1. item`, a heading as a plain line — never `#` or `**`.
    /// Mail, Messages, Outlook and every unknown text field.
    PlainBullets,
    /// Leave the text exactly as it is — code editors and terminals.
    Off,
}

/// One silence the speaker left, positioned the same way
/// `dictation::PauseSpan` is: as a share of the take's voiced time, projected
/// onto the characters of the text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StructurePause {
    pub seconds: f64,
    pub at_fraction: f64,
}

/// A silence at or over this, at a sentence end, starts a new block. The same
/// value as `dictation::PARAGRAPH_PAUSE_SECONDS` (pinned by a test in the app
/// crate) — one notion of "he finished that thought" across both stages.
pub const BLOCK_PAUSE_SECONDS: f64 = 0.9;

/// A silence at or over this (and under a block pause) is a beat: between two
/// sentences it separates list items, at a comma after a short topic it is the
/// spoken colon. Above the 0.2–0.5 s of connected speech on purpose.
pub const BEAT_PAUSE_SECONDS: f64 = 0.6;

/// Fewer items than this is never a list (Y4-D `single_item`).
pub const MIN_LIST_ITEMS: usize = 3;

/// The spoken cue words this stage may CONSUME when, and only when, it turns
/// them into a marker. The ONE list: the content-word test treats exactly these
/// words as droppable and every other word as untouchable.
pub const STRUCTURE_CUE_WORDS: &[&str] = &[
    "first",
    "firstly",
    "second",
    "secondly",
    "third",
    "thirdly",
    "fourth",
    "fifth",
    "next",
    "then",
    "after that",
    "finally",
    "lastly",
    "step one",
    "step two",
    "step three",
    "step four",
    "step five",
    "number one",
    "number two",
    "number three",
    "number four",
    "number five",
    "bullet",
    "heading",
    "title",
];

/// Cues that can OPEN a numbered sequence (a subset of [`STRUCTURE_CUE_WORDS`]).
const START_CUES: &[&str] = &["first", "firstly", "step one", "number one"];
/// Cues that END a numbered sequence once they have been consumed.
const END_CUES: &[&str] = &["finally", "lastly"];
/// Cues that announce a heading.
const HEADING_CUES: &[&str] = &["heading", "title"];
/// The explicit item cue.
const BULLET_CUE: &str = "bullet";
/// Continuation cues that are only ever a sequence adverb at the head of a
/// sentence, so they are consumed bare ("Then wait."). Every OTHER continuation
/// cue is also an ordinary word there — "Next week", "Second hand", "Third
/// quarter", "After that meeting", "Step two is" — and is consumed only when
/// he set it off with a comma or a colon, exactly like the start cues (PR #213
/// re-review: "First, book the venue. Next week, we send the invites." lost
/// "Next").
const BARE_CONTINUE_CUES: &[&str] = &["then", "finally", "lastly", "secondly", "thirdly"];

/// A phrase led by one of these is a clause or a continuation, never a list
/// item and never a topic: "and then we drove" is a story, "when I got home,"
/// is a dependent clause, "I think," is a speaker, not a thing being judged.
const CLAUSE_OPENERS: &[&str] = &[
    "and", "or", "but", "nor", "so", "then", "when", "if", "because", "after", "before", "while",
    "since", "although", "though", "as", "once", "until", "unless", "whenever", "where", "i", "we",
    "you", "he", "she", "they", "it",
];

const MAX_INLINE_ITEM_WORDS: usize = 4;
const MAX_OPENER_SENTENCE_WORDS: usize = 25;
const MAX_PAUSE_ITEM_WORDS: usize = 100;
/// A dictated heading is a short label, never a sentence.
const MAX_HEADING_WORDS: usize = 8;
const MAX_LEAD_IN_WORDS: usize = 12;
const MAX_TOPIC_WORDS: usize = 20;

/// Pick the format for a paste target. `setting` is the per-app override the
/// user chose (`markdown` | `plain` | `off`); anything else (`auto`, empty)
/// defers to the table below. A browser is ambiguous (the Y4-F conservative
/// rule), so it is plain unless the user says otherwise.
pub fn structure_format_for(bundle_id: &str, setting: &str) -> StructureFormat {
    match setting.trim().to_ascii_lowercase().as_str() {
        "markdown" => return StructureFormat::Markdown,
        "plain" => return StructureFormat::PlainBullets,
        "off" => return StructureFormat::Off,
        _ => {}
    }
    let id = bundle_id.trim().to_ascii_lowercase();
    const OFF: &[&str] = &[
        "com.microsoft.vscode",
        "com.apple.terminal",
        "com.googlecode.iterm2",
        "dev.warp.warp-stable",
        "com.apple.dt.xcode",
        // Cursor's real bundle id.
        "com.todesktop.230313mzl4w4u92",
    ];
    const MARKDOWN: &[&str] = &[
        "com.tinyspeck.slackmacgap",
        "notion.id",
        "md.obsidian",
        "com.apple.notes",
        "com.microsoft.word",
    ];
    if OFF.contains(&id.as_str()) || id.contains("cursor") {
        return StructureFormat::Off;
    }
    if MARKDOWN.contains(&id.as_str()) {
        return StructureFormat::Markdown;
    }
    // com.apple.mail, com.apple.MobileSMS, com.microsoft.Outlook, browsers and
    // every unknown field: plain bullets, real newlines, no markup.
    StructureFormat::PlainBullets
}

/// Structure one take. Returns `text` unchanged (byte for byte) when the
/// format is `Off`, when the text already has line breaks, or when no list or
/// heading forms.
pub fn structure_text(text: &str, pauses: &[StructurePause], format: StructureFormat) -> String {
    if format == StructureFormat::Off {
        return text.to_string();
    }
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.contains('\n') {
        return text.to_string();
    }
    let flat = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    let sentences = split_sentences(&flat);
    let marks = place_pauses(&flat, &sentences, pauses);

    // Sentence texts with the spoken colons applied.
    let texts: Vec<String> = sentences
        .iter()
        .enumerate()
        .map(|(i, &(s, e))| apply_topic_colon(&flat[s..e], marks.topic_comma[i]))
        .collect();

    let mut out = Builder::default();
    let mut block_start = 0;
    for i in 0..texts.len() {
        let last = i + 1 == texts.len();
        if last || marks.after[i] == Some(Mark::Block) {
            out.block(
                &texts[block_start..=i],
                &marks.after[block_start..=i],
                !last,
            );
            block_start = i + 1;
        }
    }
    if !out.structured {
        return text.to_string();
    }
    render(&out.lines, format)
}

/// The sidecar's guard (yap-polish): a rewrite that dropped list lines the
/// rules text had is a flattened list, and the rules text wins. Returns the
/// text to keep.
pub fn restore_if_flattened<'a>(rules_text: &'a str, rewrite: &'a str) -> &'a str {
    if list_line_count(rewrite) < list_line_count(rules_text) {
        rules_text
    } else {
        rewrite
    }
}

/// How many lines of `text` are list items or headings this stage can emit.
pub fn list_line_count(text: &str) -> usize {
    text.lines().filter(|l| marker_len(l) > 0).count()
}

// ── internals ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Block,
    Beat,
}

struct Marks {
    /// The pause after sentence `i`, if one landed on that boundary.
    after: Vec<Option<Mark>>,
    /// Byte offset (within sentence `i`) of the comma a beat landed on.
    topic_comma: Vec<Option<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    Prose(String),
    Bullet(String),
    Numbered(String),
    Heading(String),
}

/// Byte ranges of the sentences of a whitespace-collapsed text: a sentence ends
/// after `.`, `?` or `!` that is followed by a space or the end.
fn split_sentences(flat: &str) -> Vec<(usize, usize)> {
    let bytes = flat.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if matches!(b, b'.' | b'?' | b'!') && (i + 1 == bytes.len() || bytes[i + 1] == b' ') {
            out.push((start, i + 1));
            start = (i + 2).min(bytes.len());
        }
    }
    if start < bytes.len() {
        out.push((start, bytes.len()));
    }
    out
}

/// Where a pause can land: `Ok(sentence)` is the boundary after that
/// sentence, `Err((sentence, byte offset in sentence))` is a comma.
type Candidate = Result<usize, (usize, usize)>;

/// Snap every pause to the nearest sentence boundary or comma.
fn place_pauses(flat: &str, sentences: &[(usize, usize)], pauses: &[StructurePause]) -> Marks {
    let n = sentences.len();
    let mut marks = Marks {
        after: vec![None; n],
        topic_comma: vec![None; n],
    };
    if pauses.is_empty() || n == 0 {
        return marks;
    }
    // Candidates as (char offset, kind): kind = Ok(sentence) for the boundary
    // after that sentence, Err((sentence, byte offset in sentence)) for a comma.
    let char_at = |byte: usize| flat[..byte].chars().count();
    let mut cands: Vec<(usize, Candidate)> = Vec::new();
    for (si, &(s, e)) in sentences.iter().enumerate() {
        for (bi, ch) in flat[s..e].char_indices() {
            if ch == ',' {
                cands.push((char_at(s + bi + 1), Err((si, bi))));
            }
        }
        if si + 1 < n {
            cands.push((char_at(e), Ok(si)));
        }
    }
    let total = flat.chars().count() as f64;
    let window = ((total / 60.0) as usize).max(12);
    for p in pauses {
        if p.seconds.is_nan() || p.seconds < BEAT_PAUSE_SECONDS || !p.at_fraction.is_finite() {
            continue;
        }
        let pos = (p.at_fraction.clamp(0.0, 1.0) * total).round() as usize;
        // Nearest candidate; ties go to the sentence boundary, then the earlier.
        let best = cands
            .iter()
            .filter(|(c, _)| c.abs_diff(pos) <= window)
            .min_by_key(|(c, k)| (c.abs_diff(pos), k.is_err(), *c));
        match best {
            Some(&(_, Ok(si))) => {
                let m = if p.seconds >= BLOCK_PAUSE_SECONDS {
                    Mark::Block
                } else {
                    Mark::Beat
                };
                // The stronger mark wins if two pauses snap to one boundary.
                if marks.after[si] != Some(Mark::Block) {
                    marks.after[si] = Some(m);
                }
            }
            // The FIRST spoken colon of a sentence is the topic's.
            Some(&(_, Err((si, bi)))) if marks.topic_comma[si].is_none_or(|old| bi < old) => {
                marks.topic_comma[si] = Some(bi);
            }
            Some(&(_, Err(_))) | None => {}
        }
    }
    marks
}

/// "Account and licence, perfect." with a beat at the comma →
/// "Account and licence: perfect." — when the part before the comma is a short
/// topic, not a clause.
fn apply_topic_colon(sentence: &str, comma: Option<usize>) -> String {
    let Some(c) = comma else {
        return sentence.to_string();
    };
    let topic = &sentence[..c];
    let words = topic.split_whitespace().count();
    let first = first_word(topic);
    if words == 0 || words > MAX_TOPIC_WORDS || CLAUSE_OPENERS.contains(&first.as_str()) {
        return sentence.to_string();
    }
    format!("{}:{}", topic, &sentence[c + 1..])
}

/// The lowercase first word of a phrase, punctuation trimmed.
fn first_word(s: &str) -> String {
    s.split_whitespace()
        .next()
        .unwrap_or("")
        .replace('\u{2019}', "'")
        .trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
        .to_lowercase()
}

/// The first `n` lowercase words of a phrase, punctuation trimmed.
fn lead_words(s: &str, n: usize) -> Vec<String> {
    s.split_whitespace()
        .take(n)
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
                .to_lowercase()
        })
        .collect()
}

/// If `sentence` opens with one of `cues`, the cue and the rest (leading comma
/// and space trimmed).
fn strip_cue<'a>(sentence: &'a str, cues: &[&'static str]) -> Option<(&'static str, &'a str)> {
    strip_cue_inner(sentence, cues, false)
}

/// Like [`strip_cue`], but only when the speaker SET THE CUE OFF as a marker:
/// the cue's last word carries a comma or a colon ("Heading, weekly update.",
/// "First, unplug it.", "Bullet: eggs."). "heading", "title", "bullet" and
/// "first" are ordinary words — "Heading home now", "Title insurance",
/// "Bullet points", "First of all" — and consuming them deletes his words.
fn strip_marker<'a>(sentence: &'a str, cues: &[&'static str]) -> Option<(&'static str, &'a str)> {
    strip_cue_inner(sentence, cues, true)
}

fn strip_cue_inner<'a>(
    sentence: &'a str,
    cues: &[&'static str],
    need_sep: bool,
) -> Option<(&'static str, &'a str)> {
    for &cue in cues {
        let n = cue.split(' ').count();
        if lead_words(sentence, n).join(" ") != cue {
            continue;
        }
        // Byte offset just past the n-th word.
        let mut rest = sentence;
        let mut last_tok = "";
        for _ in 0..n {
            rest = rest.trim_start();
            let end = rest.find(' ').unwrap_or(rest.len());
            last_tok = &rest[..end];
            rest = &rest[end..];
        }
        if need_sep && !last_tok.ends_with([',', ':']) {
            continue;
        }
        let rest = rest
            .trim_start()
            .trim_start_matches([',', ':'])
            .trim_start();
        if rest.is_empty() {
            return None;
        }
        return Some((cue, rest));
    }
    None
}

/// A continuation cue inside a numbered sequence: a bare sequence adverb, or
/// any other cue only when set off as a marker (see [`BARE_CONTINUE_CUES`]).
fn strip_continue_cue(sentence: &str) -> Option<(&'static str, &str)> {
    strip_cue(sentence, BARE_CONTINUE_CUES).or_else(|| strip_marker(sentence, STRUCTURE_CUE_WORDS))
}

/// Words after a bare "then" that make it a fixed phrase, not a sequence
/// adverb: "Then again, maybe not." is "on the other hand"; "Then and there".
const THEN_IDIOM_WORDS: &[&str] = &["again", "and"];
/// A subject right after a bare "then" reads as narration ("Then I drove
/// home", "Then there's the budget"), not as a step.
const NARRATIVE_SUBJECTS: &[&str] = &[
    "i", "we", "you", "he", "she", "they", "it", "there", "i'm", "i've", "i'd", "we're", "we've",
    "we'd", "you're", "he's", "she's", "they're", "it's", "there's",
];
/// A determiner or possessive right after a bare "then" opens a noun phrase
/// that is the subject of a story ("Then my wife called", "Then the meeting
/// ran long"), not a step — unless the list itself is told that way ("First,
/// the venue. Then the invites.").
const NARRATIVE_DETERMINERS: &[&str] = &["my", "the", "our", "his", "her", "their", "a", "an"];
/// Words after a bare "finally"/"lastly" that make it mean "at last" ("Finally
/// home after a long day", "Finally done", "Finally there") — a state, not a
/// step.
const AT_LAST_WORDS: &[&str] = &[
    "home", "here", "there", "done", "over", "finished", "free", "back", "ready", "safe", "gone",
    "awake", "asleep", "alone", "together", "now", "time", "peace", "quiet",
];
/// "at" after "finally" means "at last" only with one of these: "Finally at
/// home", "Finally at peace". "Finally at checkout, pay." is a step.
const AT_LAST_AFTER_AT: &[&str] = &["home", "peace", "last", "rest", "ease"];
/// Dictated names whose casing is the name: capitalising a list item never
/// touches them. Names shaped like "iPhone" / "eBay" (lowercase then
/// uppercase) are kept by rule and need no entry here.
const NEVER_RECASE: &[&str] = &["macOS", "watchOS", "tvOS", "visionOS", "npm", "pnpm"];

/// PR #217 re-review: a continuation cue that matched BARE (not set off) is
/// still prose when the rest is not a plausible list item, and his words are
/// kept: "Then again, maybe not." (idiom), "Then I drove home." / "Then my
/// wife called." after a list of steps (narration), "Finally home after a
/// long day." / "Finally I'm home." ("at last").
/// `items` is the sequence so far, its first item at index 0.
fn continue_cue_is_prose(cue: &str, sentence: &str, rest: &str, items: &[String]) -> bool {
    let next = first_word(rest);
    let next = next.as_str();
    let set_off = sentence
        .split_whitespace()
        .next()
        .is_some_and(|t| t.ends_with([',', ':']));
    let first_item = items.first().map(|f| first_word(f)).unwrap_or_default();
    let complete = items.len() >= MIN_LIST_ITEMS;
    match cue {
        "then" => {
            if THEN_IDIOM_WORDS.contains(&next) {
                return true;
            }
            !set_off
                && (subject_is_narration(next, &first_item, complete)
                    || (NARRATIVE_DETERMINERS.contains(&next)
                        && (complete || !NARRATIVE_DETERMINERS.contains(&first_item.as_str()))))
        }
        "finally" | "lastly" => {
            let at_last = AT_LAST_WORDS.contains(&next)
                || (next == "at"
                    && lead_words(rest, 2)
                        .get(1)
                        .is_some_and(|w| AT_LAST_AFTER_AT.contains(&w.as_str())));
            // "Finally I'm home": a subject contraction is a state reached.
            let contraction = next.contains('\'') && NARRATIVE_SUBJECTS.contains(&next);
            !set_off
                && (at_last || contraction || subject_is_narration(next, &first_item, complete))
        }
        _ => false,
    }
}

/// A subject right after a bare continuation cue: narration, or a step of a
/// list told that way? `first_item` is the list's first word, lowercase.
fn subject_is_narration(next: &str, first_item: &str, complete: bool) -> bool {
    if !NARRATIVE_SUBJECTS.contains(&next) {
        return false;
    }
    if NARRATIVE_SUBJECTS.contains(&first_item) {
        // A list told with a subject ("First, we design it. Then we build
        // it.") takes any subject until it is a list, then runs on for as
        // long as THAT subject leads: "Then we ship it." is step four, "Then
        // I went home." is the story after it.
        return complete && next != first_item;
    }
    // An imperative list: "Then you tap General." is a step addressed to the
    // reader; any other subject ("Then I drove home.") is narration.
    next != "you"
}

fn is_continue_cue(c: &str) -> bool {
    !START_CUES.contains(&c) && !HEADING_CUES.contains(&c) && c != BULLET_CUE
}

/// Capitalise a list item's first letter — unless its first word is a name
/// whose casing is the name: "iPhone", "eBay" (lowercase then uppercase) or
/// one of [`NEVER_RECASE`] ("macOS"). Those stay exactly as dictated.
fn capitalize(s: &str) -> String {
    let word = s
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches(|c: char| !c.is_alphanumeric());
    let mut lead = word.chars();
    if matches!((lead.next(), lead.next()), (Some(a), Some(b)) if a.is_lowercase() && b.is_uppercase())
        || NEVER_RECASE.contains(&word)
    {
        return s.to_string();
    }
    let mut chars = s.chars();
    match chars.next() {
        Some(f) => f.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[derive(Default)]
struct Builder {
    lines: Vec<Line>,
    prose: Vec<String>,
    structured: bool,
}

impl Builder {
    fn flush(&mut self) {
        if !self.prose.is_empty() {
            self.lines.push(Line::Prose(self.prose.join(" ")));
            self.prose.clear();
        }
    }

    /// Start a list: the prose sentence right before it is its lead-in when it
    /// is short and ends in a period — "Everything is good." → "…good:".
    fn open_list(&mut self) {
        self.flush();
        self.structured = true;
        if let Some(Line::Prose(p)) = self.lines.last_mut() {
            let last_start = split_sentences(p).last().map(|&(s, _)| s).unwrap_or(0);
            let last = &p[last_start..];
            if p.ends_with('.') && last.split_whitespace().count() <= MAX_LEAD_IN_WORDS {
                p.pop();
                p.push(':');
            }
        }
    }

    /// `more_after`: another block of this take follows this one.
    fn block(&mut self, sents: &[String], after: &[Option<Mark>], more_after: bool) {
        // 1. A block the speaker beat out item by item is a list.
        let beats = after[..after.len() - 1]
            .iter()
            .filter(|m| **m == Some(Mark::Beat))
            .count();
        if beats + 1 >= MIN_LIST_ITEMS {
            let mut items: Vec<String> = Vec::new();
            let mut cur: Vec<&str> = Vec::new();
            for (i, s) in sents.iter().enumerate() {
                cur.push(s);
                if i + 1 == sents.len() || after[i] == Some(Mark::Beat) {
                    items.push(cur.join(" "));
                    cur.clear();
                }
            }
            if items
                .iter()
                .all(|it| it.split_whitespace().count() <= MAX_PAUSE_ITEM_WORDS)
            {
                self.open_list();
                self.lines.extend(items.into_iter().map(Line::Bullet));
                self.flush();
                return;
            }
        }

        // 2. Sentence-level rules.
        let mut i = 0;
        while i < sents.len() {
            let s = &sents[i];
            // Heading — only when he said so, set the cue off, kept it short,
            // and something follows it: a heading never takes the take alone.
            let heading = strip_marker(s, HEADING_CUES).filter(|(_, rest)| {
                rest.split_whitespace().count() <= MAX_HEADING_WORDS
                    && (i + 1 < sents.len() || more_after)
            });
            if let Some((_, rest)) = heading {
                self.flush();
                self.structured = true;
                let h = rest.trim_end_matches(['.', ',', ';', ':']);
                self.lines.push(Line::Heading(capitalize(h)));
                i += 1;
                continue;
            }
            // Numbered sequence: first … then … finally.
            if let Some((_, first)) = strip_marker(s, START_CUES) {
                let mut items = vec![capitalize(first)];
                let mut j = i + 1;
                while j < sents.len() {
                    match strip_continue_cue(&sents[j]) {
                        Some((cue, rest))
                            if is_continue_cue(cue)
                                && !continue_cue_is_prose(cue, &sents[j], rest, &items) =>
                        {
                            items.push(capitalize(rest));
                            j += 1;
                            if END_CUES.contains(&cue) {
                                break;
                            }
                        }
                        _ => break,
                    }
                }
                if items.len() >= MIN_LIST_ITEMS {
                    self.open_list();
                    self.lines.extend(items.into_iter().map(Line::Numbered));
                    i = j;
                    continue;
                }
            }
            // Explicit "bullet …" items: he said the marker, two is enough.
            if strip_marker(s, &[BULLET_CUE]).is_some() {
                let mut items = Vec::new();
                let mut j = i;
                while j < sents.len() {
                    match strip_marker(&sents[j], &[BULLET_CUE]) {
                        Some((_, rest)) => items.push(capitalize(rest)),
                        None => break,
                    }
                    j += 1;
                }
                if items.len() >= 2 {
                    self.open_list();
                    self.lines.extend(items.into_iter().map(Line::Bullet));
                    i = j;
                    continue;
                }
            }
            // Parallel openers: "make sure X. make sure Y. make sure Z."
            let key = lead_words(s, 2);
            if key.len() == 2
                && !CLAUSE_OPENERS.contains(&key[0].as_str())
                && s.split_whitespace().count() <= MAX_OPENER_SENTENCE_WORDS
            {
                let mut j = i + 1;
                while j < sents.len()
                    && lead_words(&sents[j], 2) == key
                    && sents[j].split_whitespace().count() <= MAX_OPENER_SENTENCE_WORDS
                {
                    j += 1;
                }
                if j - i >= MIN_LIST_ITEMS {
                    self.open_list();
                    self.lines
                        .extend(sents[i..j].iter().cloned().map(Line::Bullet));
                    i = j;
                    continue;
                }
            }
            // An inline run: "built, the security, the data, the telemetry".
            if let Some((lead, items)) = inline_run(s) {
                self.prose.push(lead);
                self.flush();
                self.structured = true;
                self.lines.extend(items.into_iter().map(Line::Bullet));
                i += 1;
                continue;
            }
            self.prose.push(s.clone());
            i += 1;
        }
        self.flush();
    }
}

/// Split a sentence on ", " and look for a run of 3+ short phrases sharing one
/// opener, after a lead-in, NOT closed by "and"/"or".
fn inline_run(sentence: &str) -> Option<(String, Vec<String>)> {
    let segs: Vec<&str> = sentence.split(", ").collect();
    let item_like = |seg: &str| {
        let w = first_word(seg);
        !w.is_empty()
            && w.chars().all(|c| c.is_alphabetic())
            && !CLAUSE_OPENERS.contains(&w.as_str())
            && seg.split_whitespace().count() <= MAX_INLINE_ITEM_WORDS
    };
    let mut k = 1;
    while k < segs.len() {
        if !item_like(segs[k]) {
            k += 1;
            continue;
        }
        let opener = first_word(segs[k]);
        let mut end = k + 1;
        while end < segs.len() && item_like(segs[end]) && first_word(segs[end]) == opener {
            end += 1;
        }
        let closes_on_conjunction =
            end < segs.len() && matches!(first_word(segs[end]).as_str(), "and" | "or");
        if end - k >= MIN_LIST_ITEMS && !closes_on_conjunction {
            let lead = format!("{}:", segs[..k].join(", "));
            let mut items: Vec<String> = segs[k..end].iter().map(|s| s.to_string()).collect();
            if end < segs.len() {
                items.push(segs[end..].join(", "));
            }
            return Some((lead, items));
        }
        k = end;
    }
    None
}

fn render(lines: &[Line], format: StructureFormat) -> String {
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut n = 0;
    for line in lines {
        if !matches!(line, Line::Numbered(_)) {
            n = 0;
        }
        out.push(match (line, format) {
            (Line::Prose(p), _) => p.clone(),
            (Line::Bullet(b), StructureFormat::Markdown) => format!("- {b}"),
            (Line::Bullet(b), _) => format!("• {b}"),
            (Line::Numbered(t), _) => {
                n += 1;
                format!("{n}. {t}")
            }
            (Line::Heading(h), StructureFormat::Markdown) => format!("## {h}"),
            (Line::Heading(h), _) => h.clone(),
        });
    }
    out.join("\n")
}

/// Length of the list/heading marker at the start of a line, 0 for none.
fn marker_len(line: &str) -> usize {
    for m in ["- ", "• ", "## "] {
        if line.starts_with(m) {
            return m.len();
        }
    }
    let digits = line.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 && line[digits..].starts_with(". ") {
        return digits + 2;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const FIXTURES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../yap-polish/fixtures/structure"
    );

    struct Fixture {
        name: String,
        input: String,
        target: String,
        expected: String,
        pauses: Vec<StructurePause>,
    }

    fn read(path: &std::path::Path) -> String {
        let s = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        s.strip_suffix('\n').unwrap_or(&s).to_string()
    }

    /// `pauses.json` is `[{"seconds": f, "at_fraction": f}, …]`. std-only
    /// crate, so a tiny reader for exactly that shape.
    fn parse_pauses(json: &str) -> Vec<StructurePause> {
        let num_after = |obj: &str, key: &str| -> f64 {
            let at = obj
                .find(key)
                .unwrap_or_else(|| panic!("{key} missing in {obj}"));
            let rest = &obj[at + key.len()..];
            let rest = rest.trim_start_matches(|c: char| c == '"' || c == ':' || c.is_whitespace());
            let end = rest
                .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == 'e'))
                .unwrap_or(rest.len());
            rest[..end].parse().expect("a number")
        };
        json.split('}')
            .filter(|o| o.contains("seconds"))
            .map(|o| StructurePause {
                seconds: num_after(o, "\"seconds\""),
                at_fraction: num_after(o, "\"at_fraction\""),
            })
            .collect()
    }

    fn fixtures() -> Vec<Fixture> {
        let mut dirs: Vec<_> = std::fs::read_dir(FIXTURES)
            .expect("fixtures dir")
            .map(|e| e.expect("entry").path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        dirs.into_iter()
            .map(|d| {
                let pauses_path = d.join("pauses.json");
                Fixture {
                    name: d.file_name().unwrap().to_string_lossy().into_owned(),
                    input: read(&d.join("input.txt")),
                    target: read(&d.join("target.txt")).trim().to_string(),
                    expected: read(&d.join("expected.md")),
                    pauses: if pauses_path.exists() {
                        parse_pauses(&read(&pauses_path))
                    } else {
                        Vec::new()
                    },
                }
            })
            .collect()
    }

    fn run(f: &Fixture) -> String {
        structure_text(&f.input, &f.pauses, structure_format_for(&f.target, "auto"))
    }

    fn words(s: &str) -> Vec<String> {
        s.split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(|w| w.to_lowercase())
            .collect()
    }

    fn output_words(out: &str) -> Vec<String> {
        out.lines()
            .flat_map(|l| words(&l[marker_len(l)..]))
            .collect()
    }

    fn count(ws: &[String], w: &str) -> usize {
        ws.iter().filter(|x| x.as_str() == w).count()
    }

    /// Both directions: nothing new appears, and nothing but a cue word goes.
    fn assert_words_kept(input: &str, out: &str, ctx: &str) {
        let cue: Vec<String> = STRUCTURE_CUE_WORDS
            .iter()
            .flat_map(|c| c.split(' '))
            .map(str::to_string)
            .collect();
        let inw = words(input);
        let outw = output_words(out);
        for w in &outw {
            assert!(
                count(&outw, w) <= count(&inw, w),
                "{ctx}: output invents or repeats {w:?}\n{out}"
            );
        }
        for w in &inw {
            if !cue.contains(w) {
                assert!(
                    count(&outw, w) >= count(&inw, w),
                    "{ctx}: output dropped {w:?}\n{out}"
                );
            }
        }
    }

    #[test]
    fn structure_fixtures_are_all_present() {
        let names: Vec<String> = fixtures().into_iter().map(|f| f.name).collect();
        assert!(names.len() >= 8, "{names:?}");
        assert!(names.iter().any(|n| n == "01-wilson-review-2026-10-16"));
    }

    #[test]
    fn structure_fixture_01_two_lists_and_topic_verdict_bullets() {
        let f = fixtures()
            .into_iter()
            .find(|f| f.name == "01-wilson-review-2026-10-16")
            .expect("fixture 01");
        assert!(
            !f.input.contains('\n') && !f.input.contains("- "),
            "input is flat speech"
        );
        let out = run(&f);
        let lines: Vec<&str> = out.lines().collect();
        let list_starts = lines
            .windows(2)
            .filter(|w| !w[0].starts_with("- ") && w[1].starts_with("- "))
            .count();
        assert!(list_starts >= 2, "at least two lists:\n{out}");
        for want in [
            "- Account and licence: perfect.",
            "- Guided tour: it looks good.",
            "- Library: make sure it looks smooth.",
            "- the security",
            "Everything is good:",
        ] {
            assert!(out.contains(want), "missing {want:?}:\n{out}");
        }
        // Prose stays prose: the inline series in the opening is not a list.
        assert!(lines[0].starts_with("Okay, I really, really liked the slideshow."));
        assert!(lines[0].contains("very good, very great, very intuitive, and easily understood."));
    }

    #[test]
    fn structure_golden_fixtures_match() {
        for f in fixtures() {
            let out = run(&f);
            assert_eq!(out, f.expected, "fixture {} differs", f.name);
        }
    }

    #[test]
    fn structure_is_deterministic_three_runs() {
        for f in fixtures() {
            let a = run(&f);
            let b = run(&f);
            let c = run(&f);
            assert_eq!(a, b, "{}", f.name);
            assert_eq!(b, c, "{}", f.name);
        }
    }

    #[test]
    fn structure_latency_p95_under_150ms() {
        let f = fixtures()
            .into_iter()
            .find(|f| f.name.starts_with("06-"))
            .expect("the 400-word fixture");
        assert!(f.input.split_whitespace().count() >= 400);
        let mut ms: Vec<f64> = (0..50)
            .map(|_| {
                let t = Instant::now();
                std::hint::black_box(run(&f));
                t.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p95 = ms[(ms.len() * 95).div_ceil(100) - 1];
        println!("structure p95_ms={p95:.3}");
        assert!(p95 <= 150.0, "structure p95_ms={p95:.3} > 150");
    }

    #[test]
    fn structure_never_invents_content_words() {
        for f in fixtures() {
            let out = run(&f);
            assert_words_kept(&f.input, &out, &f.name);
            // Seeded shuffles of the sentences: new adjacencies, same rule.
            let sents: Vec<String> = split_sentences(&f.input)
                .into_iter()
                .map(|(s, e)| f.input[s..e].to_string())
                .collect();
            let mut seed: u64 = 0x5eed_0000_0000_0001;
            for round in 0..8 {
                let mut v = sents.clone();
                for i in (1..v.len()).rev() {
                    seed = seed
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    let j = (seed >> 33) as usize % (i + 1);
                    v.swap(i, j);
                }
                let input = v.join(" ");
                let fmt = structure_format_for(&f.target, "auto");
                let out = structure_text(&input, &[], fmt);
                assert_words_kept(&input, &out, &format!("{} shuffle {round}", f.name));
            }
        }
    }

    #[test]
    fn structure_format_for_bundle_id() {
        use StructureFormat::*;
        for id in [
            "com.tinyspeck.slackmacgap",
            "notion.id",
            "md.obsidian",
            "com.apple.Notes",
            "com.microsoft.Word",
        ] {
            assert_eq!(structure_format_for(id, "auto"), Markdown, "{id}");
        }
        for id in [
            "com.apple.mail",
            "com.apple.MobileSMS",
            "com.microsoft.Outlook",
            "com.example.unknown",
            "",
            // A browser is ambiguous: plain unless the user says otherwise.
            "com.google.Chrome",
            "com.apple.Safari",
        ] {
            assert_eq!(structure_format_for(id, "auto"), PlainBullets, "{id}");
        }
        for id in [
            "com.microsoft.VSCode",
            "com.apple.Terminal",
            "com.googlecode.iterm2",
            "dev.warp.Warp-Stable",
            "com.todesktop.230313mzl4w4u92",
            "com.apple.dt.Xcode",
        ] {
            assert_eq!(structure_format_for(id, "auto"), Off, "{id}");
            assert_eq!(structure_format_for(id, ""), Off, "{id}");
        }
        // The per-app override wins, both ways.
        assert_eq!(
            structure_format_for("com.google.Chrome", "markdown"),
            Markdown
        );
        assert_eq!(
            structure_format_for("com.microsoft.VSCode", "plain"),
            PlainBullets
        );
        assert_eq!(structure_format_for("notion.id", "off"), Off);
    }

    #[test]
    fn structure_chat_paste_has_no_blank_lines() {
        // Paste is clipboard + Cmd-V: it does not send, so Slack gets the list.
        // But never a blank line in chat.
        for name in ["01-", "03-"] {
            let f = fixtures()
                .into_iter()
                .find(|f| f.name.starts_with(name))
                .unwrap();
            let fmt = structure_format_for("com.tinyspeck.slackmacgap", "auto");
            let out = structure_text(&f.input, &f.pauses, fmt);
            assert!(!out.contains("\n\n"), "blank line in chat:\n{out}");
            assert!(
                out.contains("\n- "),
                "chat keeps single-newline list items:\n{out}"
            );
        }
    }

    #[test]
    fn structure_single_item_is_not_a_list() {
        let md = StructureFormat::Markdown;
        for s in [
            "First, unplug the router.",
            "Make sure the door is locked.",
            "Make sure the door is locked. Make sure the oven is off.",
            "We need one thing, the keys.",
            "First, unplug the router. Then wait a minute.",
        ] {
            assert_eq!(structure_text(s, &[], md), s, "{s}");
        }
        // One beat is two segments, not a list.
        let s = "The kitchen is great. The rooms are small.";
        let p = [StructurePause {
            seconds: 0.7,
            at_fraction: 21.0 / s.len() as f64,
        }];
        assert_eq!(structure_text(s, &p, md), s);
    }

    #[test]
    fn structure_narrative_and_then_is_not_a_list() {
        let md = StructureFormat::Markdown;
        for s in [
            "Last weekend we packed the car early, and then we drove to the lake, and then we ate lunch on the dock, and then we swam until it got dark.",
            "We drove, and then we ate, and then we slept.",
            "It was very good, very great, very intuitive, and easily understood.",
            "We think about scrolling, pads, error states, handling, and stuff like that.",
        ] {
            assert_eq!(structure_text(s, &[], md), s, "{s}");
        }
    }

    #[test]
    fn structure_off_and_existing_line_breaks_are_untouched() {
        let s = "Make sure A works. Make sure B works. Make sure C works.";
        assert_eq!(structure_text(s, &[], StructureFormat::Off), s);
        let lines = "1. one\n2. two";
        assert_eq!(structure_text(lines, &[], StructureFormat::Markdown), lines);
        assert_eq!(
            structure_text(s, &[], StructureFormat::PlainBullets),
            "• Make sure A works.\n• Make sure B works.\n• Make sure C works."
        );
    }

    /// PR #213 review: "heading", "title", "bullet" and "first" are ordinary
    /// words. They are a dictated marker only when the speaker set them off
    /// ("Heading, weekly update." / "First, unplug it."), never when they open
    /// a sentence as a verb or an idiom — and a heading never takes a take on
    /// its own. Every one of these comes back byte for byte.
    #[test]
    fn structure_common_words_are_not_consumed_as_cues() {
        for s in [
            "Heading home now, see you in twenty.",
            "Heading into the meeting now.",
            "Heading out. Be back soon.",
            "Title insurance is expensive in Texas.",
            "Bullet points are fine. Bullet trains are fast.",
            "First of all, great job on the launch. Then we can talk pricing. Finally, let's grab lunch.",
            "First off, thanks. Then we can talk pricing. Finally, let's grab lunch.",
            "First time I went there it rained. Then it snowed. Finally it cleared up.",
            "Heading, weekly update.",
            "Title: the plan.",
            "Notes for today. Heading, next steps.",
            "Heading, this is a whole long sentence that keeps going and going well past any heading length.",
        ] {
            for fmt in [StructureFormat::Markdown, StructureFormat::PlainBullets] {
                assert_eq!(structure_text(s, &[], fmt), s, "{fmt:?}: {s}");
            }
        }
    }

    /// PR #213 re-review: an ordinary word that happens to be a continuation
    /// cue ("Next week", "After that meeting", "Third quarter", "Second hand",
    /// "Step two is") must not be eaten just because a set-off "First," came
    /// before it. Each take must come back byte for byte.
    #[test]
    fn structure_ordinary_words_after_a_set_off_first_are_not_consumed() {
        for s in [
            "First, book the venue. Next week, we send the invites. Finally, we pick the menu.",
            "First, we had lunch. After that meeting ended, we left. Finally, we got home.",
            "First, revenue is up. Third quarter numbers look great. Then we hire. Finally, we celebrate.",
            "First, open the box. Second hand stores sell these cheap. Lastly, recycle the box.",
            "First, pick a plan. Step two is the hard part. Finally, ship it.",
            // PR #217 re-review (rr1 BLOCKING): "then again" is an idiom.
            "First, buy milk. Second, buy eggs. Then again, maybe not.",
            "First, buy milk. Then again, maybe not. Finally, buy eggs.",
            // rr1 advisory 2: "finally" meaning "at last".
            "First, we packed. Then we drove ten hours. Finally home after a long day.",
            "First, buy milk. Second, buy eggs. Finally done.",
            // rr1 advisory 1: narration after a list of steps.
            "First, buy milk. Second, buy eggs. Then there's the budget, which nobody wants to talk about.",
            "First, buy milk. Second, buy eggs. Then I drove home.",
        ] {
            for fmt in [StructureFormat::Markdown, StructureFormat::PlainBullets] {
                assert_eq!(structure_text(s, &[], fmt), s, "{fmt:?}: {s}");
            }
        }
        // The same cues SET OFF are still a sequence.
        assert_eq!(
            structure_text(
                "First, book the venue. Next, send the invites. After that, pick the menu.",
                &[],
                StructureFormat::Markdown
            ),
            "1. Book the venue.\n2. Send the invites.\n3. Pick the menu."
        );
        assert_eq!(
            structure_text(
                "First, revenue. Second, hiring. Third, the launch.",
                &[],
                StructureFormat::Markdown
            ),
            "1. Revenue.\n2. Hiring.\n3. The launch."
        );
    }

    /// PR #217 re-review: a list followed by an idiom or narration keeps the
    /// list and leaves the following sentence as prose, every word kept.
    #[test]
    fn structure_idiom_and_narration_after_a_list_stay_prose() {
        let md = StructureFormat::Markdown;
        for (s, want) in [
            (
                "First, book the venue. Second, send the invites. Third, pick the menu. Then I drove home and took a nap.",
                "1. Book the venue.\n2. Send the invites.\n3. Pick the menu.\nThen I drove home and took a nap.",
            ),
            (
                "First, book the venue. Second, send the invites. Third, pick the menu. Then again, maybe not.",
                "1. Book the venue.\n2. Send the invites.\n3. Pick the menu.\nThen again, maybe not.",
            ),
            (
                "First, book the venue. Second, send the invites. Third, pick the menu. Then we went home and slept.",
                "1. Book the venue.\n2. Send the invites.\n3. Pick the menu.\nThen we went home and slept.",
            ),
        ] {
            let out = structure_text(s, &[], md);
            assert_eq!(out, want, "{s}");
            assert_words_kept(s, &out, s);
        }
        // Still a sequence: steps told with a subject, a bare imperative
        // "Then", a bare "Lastly", and set-off cues.
        assert_eq!(
            structure_text(
                "First, we design it. Then we build it. Finally we ship it.",
                &[],
                md
            ),
            "1. We design it.\n2. We build it.\n3. We ship it."
        );
        assert_eq!(
            structure_text("First, wash it. Then dry it. Lastly fold it.", &[], md),
            "1. Wash it.\n2. Dry it.\n3. Fold it."
        );
        assert_eq!(
            structure_text(
                "First, book the venue. Next, send the invites. Then pick the menu. Lastly, pay.",
                &[],
                md
            ),
            "1. Book the venue.\n2. Send the invites.\n3. Pick the menu.\n4. Pay."
        );
        // "Finally!" as an exclamation stays prose.
        let s = "First, we packed. Then we drove. Finally!";
        assert_eq!(structure_text(s, &[], md), s);
    }

    /// yap-structure-followups (PR #217 re-review advisories): one case per
    /// advisory, the exact input and the exact output, and every input word
    /// kept. The guards after each fix are the lists the fix must not break.
    #[test]
    fn structure_followups_keep_every_word_and_finish_every_list() {
        let md = StructureFormat::Markdown;
        let mut wrong = Vec::new();
        for (s, want) in [
            // 1. Narration after a list keeps "Then": a determiner or a
            //    possessive after a bare "Then" is a story, not a step.
            (
                "First, book the venue. Second, send the invites. Third, pick the menu. Then my wife called and we talked for an hour.",
                "1. Book the venue.\n2. Send the invites.\n3. Pick the menu.\nThen my wife called and we talked for an hour.",
            ),
            (
                "First, book the venue. Second, send the invites. Third, pick the menu. Then the meeting ran long.",
                "1. Book the venue.\n2. Send the invites.\n3. Pick the menu.\nThen the meeting ran long.",
            ),
            // Guard: a list whose items are told with a determiner still forms.
            (
                "First, the venue. Then the invites. Finally, the menu.",
                "1. The venue.\n2. The invites.\n3. The menu.",
            ),
            // 2. "Finally" + a pronoun contraction is "at last", not a step.
            (
                "First, book the venue. Second, send the invites. Third, pick the menu. Finally I'm home.",
                "1. Book the venue.\n2. Send the invites.\n3. Pick the menu.\nFinally I'm home.",
            ),
            (
                "First, we packed. Then we drove ten hours. Finally I'm home.",
                "First, we packed. Then we drove ten hours. Finally I'm home.",
            ),
            // 3. A list told with one subject runs as long as that subject
            //    keeps leading a cued sentence: four steps, one list.
            (
                "First, we design it. Then we build it. Then we test it. Then we ship it.",
                "1. We design it.\n2. We build it.\n3. We test it.\n4. We ship it.",
            ),
            // Guard: a different subject after the list is still narration.
            (
                "First, we design it. Then we build it. Then we ship it. Then I went home and slept.",
                "1. We design it.\n2. We build it.\n3. We ship it.\nThen I went home and slept.",
            ),
            // 4. An imperative step told to "you" inside a list is a step.
            (
                "First, open Settings. Then you tap General. Finally, tap Reset.",
                "1. Open Settings.\n2. You tap General.\n3. Tap Reset.",
            ),
            // 5. "Finally at checkout" is a step; only "at home" / "at last" /
            //    "at peace" mean "at last".
            (
                "First, add the shoes to your cart. Then enter your address. Finally at checkout, pay with your card.",
                "1. Add the shoes to your cart.\n2. Enter your address.\n3. At checkout, pay with your card.",
            ),
            (
                "First, we packed. Then we drove ten hours. Finally at home, we slept.",
                "First, we packed. Then we drove ten hours. Finally at home, we slept.",
            ),
            // 6. Product names keep their dictated casing; an ordinary
            //    lowercase first letter is still capitalised.
            (
                "First, iPhone. Second, eBay. Third, macOS.",
                "1. iPhone.\n2. eBay.\n3. macOS.",
            ),
            (
                "Bullet, iPhone cases. Bullet, eBay listings. Bullet, macOS updates. Bullet, laptop stands.",
                "- iPhone cases.\n- eBay listings.\n- macOS updates.\n- Laptop stands.",
            ),
        ] {
            let out = structure_text(s, &[], md);
            assert_words_kept(s, &out, s);
            if out != want {
                wrong.push(format!("IN:   {s}\nGOT:  {out:?}\nWANT: {want:?}"));
            }
        }
        assert!(wrong.is_empty(), "\n{}", wrong.join("\n\n"));
        let out = structure_text(
            "First, iPhone. Second, eBay. Third, macOS.",
            &[],
            StructureFormat::PlainBullets,
        );
        for name in ["iPhone", "eBay", "macOS"] {
            assert!(out.contains(name), "{name} recased:\n{out}");
        }
    }

    #[test]
    fn structure_set_off_cues_still_work() {
        let md = StructureFormat::Markdown;
        assert_eq!(
            structure_text("Heading: weekly update. The billing page shipped.", &[], md),
            "## Weekly update\nThe billing page shipped."
        );
        assert_eq!(
            structure_text(
                "Title, launch plan. We ship Friday.",
                &[],
                StructureFormat::PlainBullets
            ),
            "Launch plan\nWe ship Friday."
        );
        assert_eq!(
            structure_text("Bullet, eggs. Bullet, milk.", &[], md),
            "- Eggs.\n- Milk."
        );
        assert_eq!(
            structure_text("First, unplug it. Then wait. Finally, plug it in.", &[], md),
            "1. Unplug it.\n2. Wait.\n3. Plug it in."
        );
        // A heading followed by a block pause still has content after it.
        let s = "Heading, weekly update. The billing page shipped.";
        let p = [StructurePause {
            seconds: 1.2,
            at_fraction: 23.0 / s.len() as f64,
        }];
        assert_eq!(
            structure_text(s, &p, md),
            "## Weekly update\nThe billing page shipped."
        );
    }

    #[test]
    fn structure_cue_tables_are_subsets_of_the_one_list() {
        for c in START_CUES
            .iter()
            .chain(END_CUES)
            .chain(HEADING_CUES)
            .chain(BARE_CONTINUE_CUES)
            .chain([BULLET_CUE].iter())
        {
            assert!(STRUCTURE_CUE_WORDS.contains(c), "{c}");
        }
    }

    #[test]
    fn structure_restore_if_flattened_keeps_the_rules_list() {
        let rules = "Things:\n- a\n- b\n- c";
        assert_eq!(restore_if_flattened(rules, "Things: a, b, c."), rules);
        assert_eq!(
            restore_if_flattened(rules, "Things:\n- A\n- B\n- C"),
            "Things:\n- A\n- B\n- C"
        );
        assert_eq!(restore_if_flattened("plain", "Plain."), "Plain.");
    }
}
