//! The call-bleed metrics (yap-audit-eval-call-bleed). Pure functions: text and
//! timings in, a number out. No audio, no corpus, no model — which is why they
//! are unit-tested with negative controls in `tests/meeting_eval_metrics.rs`
//! and merely PRINTED for the real fixture by `tests/meeting_eval.rs`.
//!
//! Gates are deliberately absent. Each metric gets its threshold from the item
//! that fixes the defect it measures: NT11 sets `echo_dup_rate <= 0.10`,
//! yap-audit-utterance-segments sets the median segment under 12 s, NT4 sets
//! owner accuracy. A threshold picked before the fix exists is a number from
//! nowhere (the lesson of YV90's placeholder WER gate).

use wilson_voice_lib::meeting_echo::{echo_words, is_echo_of, word_ngram_containment};

/// A stretch of transcript text with its place on the call's clock, in seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct TimedText {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

impl TimedText {
    pub fn new(start: f64, end: f64, text: &str) -> TimedText {
        TimedText {
            start,
            end,
            text: text.to_string(),
        }
    }

    pub fn seconds(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }
}

/// How far either side of a call window the mic is searched for its echo. An
/// echo arrives late (the 60 ms bleed delay) and a decoder's segment edges
/// wander by about a second, so the window is the call segment's span widened
/// by this much.
pub const ECHO_SEARCH_SECONDS: f64 = 1.5;

/// The share of word 4-grams of a summary that occur verbatim in the
/// transcript. 1.0 means the summary is a copy.
pub const VERBATIM_N: usize = 4;

/// Share of CALL windows (the far side's decoded segments) whose word trigrams
/// are at least 30% contained in the mic text within +-[`ECHO_SEARCH_SECONDS`].
///
/// This is the leak measured at the transcript: of everything the far side said,
/// how much of it the mic track ALSO transcribed. It calls the production
/// predicate [`is_echo_of`] (word trigram containment), so the notes echo-owner
/// guard and this metric cannot drift apart.
///
/// A call window with fewer than three words has no trigram and is left out of
/// the denominator, not counted as "not an echo"; with nothing left to judge the
/// rate is 0.0.
pub fn echo_dup_rate(call: &[TimedText], mic: &[TimedText]) -> f64 {
    let mut judged = 0usize;
    let mut echoed = 0usize;
    for window in call {
        if echo_words(&window.text).len() < 3 {
            continue;
        }
        judged += 1;
        let lo = window.start - ECHO_SEARCH_SECONDS;
        let hi = window.end + ECHO_SEARCH_SECONDS;
        let nearby: String = mic
            .iter()
            .filter(|m| m.end >= lo && m.start <= hi)
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if is_echo_of(&window.text, &nearby) {
            echoed += 1;
        }
    }
    if judged == 0 {
        0.0
    } else {
        echoed as f64 / judged as f64
    }
}

/// Share of segments that stop in the middle of a sentence: non-empty text whose
/// last visible character is not sentence-final punctuation. A segmenter that
/// cuts on a fixed clock instead of on a pause produces these; one that cuts on
/// utterances does not. Empty segments are not judged; no segments gives 0.0.
pub fn mid_sentence_end_rate(segments: &[TimedText]) -> f64 {
    let mut judged = 0usize;
    let mut mid = 0usize;
    for s in segments {
        let trimmed = s.text.trim_end_matches(|c: char| {
            c.is_whitespace() || matches!(c, '"' | '\'' | ')' | '\u{201D}' | '\u{2019}')
        });
        let Some(last) = trimmed.chars().last() else {
            continue;
        };
        judged += 1;
        if !matches!(last, '.' | '?' | '!' | '\u{2026}') {
            mid += 1;
        }
    }
    if judged == 0 {
        0.0
    } else {
        mid as f64 / judged as f64
    }
}

/// Median segment length in seconds (the mean of the middle two when the count
/// is even); 0.0 for no segments.
pub fn median_segment_seconds(segments: &[TimedText]) -> f64 {
    let mut lengths: Vec<f64> = segments.iter().map(TimedText::seconds).collect();
    if lengths.is_empty() {
        return 0.0;
    }
    lengths.sort_by(|a, b| a.total_cmp(b));
    let mid = lengths.len() / 2;
    if lengths.len() % 2 == 1 {
        lengths[mid]
    } else {
        (lengths[mid - 1] + lengths[mid]) / 2.0
    }
}

/// One commitment the fixture planted: what it is about (the `keywords`, which
/// are words of the spoken sentence) and who owns it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlantedItem {
    pub owner: String,
    pub keywords: Vec<String>,
}

/// One action item the notes produced, with the owner label the notes gave it
/// (`Me`, `Them`, `Someone here`, or empty for none).
#[derive(Debug, Clone, PartialEq)]
pub struct NoteItem {
    pub text: String,
    pub owner: String,
}

/// A note item matches a planted one when at least this share of the planted
/// item's keywords appear among the note item's words.
pub const NOTE_MATCH_SHARE: f64 = 0.6;

/// Share of the planted commitments the notes found AND attributed to the right
/// owner. A planted item with no matching note item counts as wrong, as does a
/// match with a blank or different owner; no planted items gives 0.0.
pub fn notes_owner_accuracy(planted: &[PlantedItem], notes: &[NoteItem]) -> f64 {
    if planted.is_empty() {
        return 0.0;
    }
    let right = planted
        .iter()
        .filter(|p| {
            let wanted: Vec<String> = p.keywords.iter().flat_map(|k| echo_words(k)).collect();
            if wanted.is_empty() {
                return false;
            }
            let best = notes
                .iter()
                .map(|n| {
                    let have = echo_words(&n.text);
                    let hit = wanted.iter().filter(|w| have.contains(w)).count();
                    (hit as f64 / wanted.len() as f64, n)
                })
                .max_by(|a, b| a.0.total_cmp(&b.0));
            match best {
                Some((share, note)) if share >= NOTE_MATCH_SHARE => {
                    !note.owner.trim().is_empty()
                        && note.owner.trim().eq_ignore_ascii_case(p.owner.trim())
                }
                _ => false,
            }
        })
        .count();
    right as f64 / planted.len() as f64
}

/// Share of the summary's word 4-grams that occur verbatim in the transcript.
/// A summary that is a paste of the transcript scores 1.0; a rewrite scores low.
/// Containment is taken from the summary's side, so a long transcript does not
/// dilute it. Fewer than four words in the summary gives 0.0.
pub fn summary_verbatim_ratio(summary: &str, transcript: &str) -> f64 {
    word_ngram_containment(summary, transcript, VERBATIM_N)
}
