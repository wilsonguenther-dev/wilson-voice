//! One definition of "this text is an echo of that text".
//!
//! A call's far side leaks into the microphone: the mic track's decode then
//! contains the far side's words, a moment late and a little mangled. Two
//! places need to say whether a stretch of text is that leak, and they must
//! never disagree about it:
//!
//! 1. the eval harness (`tests/support/eval_metrics.rs::echo_dup_rate`), which
//!    measures how much leak survives into a rendered transcript, and
//! 2. the notes echo-owner guard (yap-audit-notes-guards), which refuses to
//!    credit a commitment to "Me" when the sentence is the far side's.
//!
//! So it is one pure function in the library, public, and both call it. The
//! measure is containment, not similarity: of the word trigrams in the
//! WINDOW being judged, what share also occur in the REFERENCE text. Containment
//! is asymmetric on purpose — a short echoed phrase inside a long mic window is
//! still an echo of the phrase, and a similarity measure normalised by the
//! longer side would dilute it away.
//!
//! No audio, no clock, no I/O: text in, a number out.

use std::collections::HashSet;

/// The share of a window's word trigrams that must also appear in the other
/// side's text for the window to count as an echo. 30% rather than a majority
/// because an echo is decoded from an attenuated, reverberant copy: a third of
/// its trigrams surviving intact is already far above what two unrelated
/// sentences share by chance (which is zero on mundane speech).
pub const ECHO_CONTAINMENT_THRESHOLD: f64 = 0.30;

/// Words for comparison: ASCII alphanumerics and word-internal apostrophes,
/// lowercased; everything else separates. Plain on purpose — it must give the
/// same tokens for the decoder's punctuated text and for a reference
/// transcript's.
pub fn echo_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else if (ch == '\'' || ch == '\u{2019}') && !current.is_empty() {
            current.push('\'');
        } else if !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    for w in &mut words {
        while w.ends_with('\'') {
            w.pop();
        }
    }
    words.retain(|w| !w.is_empty());
    words
}

/// The distinct word n-grams of `text`, each joined by a single space.
fn ngram_set(text: &str, n: usize) -> HashSet<String> {
    let words = echo_words(text);
    if n == 0 || words.len() < n {
        return HashSet::new();
    }
    words.windows(n).map(|w| w.join(" ")).collect()
}

/// The share of `window`'s distinct word `n`-grams that also occur in
/// `reference`, in `0.0..=1.0`. `0.0` when `window` has fewer than `n` words:
/// a window with no n-gram has nothing to be contained.
pub fn word_ngram_containment(window: &str, reference: &str, n: usize) -> f64 {
    let own = ngram_set(window, n);
    if own.is_empty() {
        return 0.0;
    }
    let other = ngram_set(reference, n);
    let shared = own.iter().filter(|g| other.contains(*g)).count();
    shared as f64 / own.len() as f64
}

/// [`word_ngram_containment`] at `n = 3` — the echo measure. `window` is the
/// text being judged, `reference` the text it may be an echo of.
pub fn word_trigram_containment(window: &str, reference: &str) -> f64 {
    word_ngram_containment(window, reference, 3)
}

/// Whether `window` is an echo of `reference` at [`ECHO_CONTAINMENT_THRESHOLD`].
pub fn is_echo_of(window: &str, reference: &str) -> bool {
    word_trigram_containment(window, reference) >= ECHO_CONTAINMENT_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_is_fully_contained() {
        let t = "we will cap each session at thirty people";
        assert_eq!(word_trigram_containment(t, t), 1.0);
    }

    #[test]
    fn unrelated_text_shares_nothing() {
        let a = "the hall has about twenty chairs";
        let b = "please send the budget outline on friday";
        assert_eq!(word_trigram_containment(a, b), 0.0);
    }

    #[test]
    fn punctuation_and_case_do_not_matter() {
        let a = "Then it is decided, we cap each session.";
        let b = "then it is decided we cap each session";
        assert_eq!(word_trigram_containment(a, b), 1.0);
    }

    #[test]
    fn fewer_than_three_words_is_never_an_echo() {
        assert_eq!(word_trigram_containment("good point", "good point"), 0.0);
        assert!(!is_echo_of("good point", "good point"));
    }

    #[test]
    fn containment_is_asymmetric() {
        let short = "call the caterer and confirm";
        let long = "i will call the caterer and confirm the headcount before the end of the week";
        assert_eq!(word_trigram_containment(short, long), 1.0);
        assert!(word_trigram_containment(long, short) < ECHO_CONTAINMENT_THRESHOLD + 0.2);
        assert!(word_trigram_containment(long, short) < 1.0);
    }

    #[test]
    fn a_third_of_the_trigrams_is_the_threshold() {
        // 6 words -> 4 trigrams; sharing the first 3 trigrams = 0.75, sharing 1 = 0.25.
        let window = "alpha bravo charlie delta echo foxtrot";
        let one_shared = "alpha bravo charlie unrelated words here";
        let two_shared = "alpha bravo charlie delta unrelated words";
        assert!(!is_echo_of(window, one_shared));
        assert!(is_echo_of(window, two_shared));
    }

    #[test]
    fn four_grams_are_the_verbatim_measure() {
        let summary = "cap each session at thirty";
        let transcript = "then we cap each session at thirty people";
        assert_eq!(word_ngram_containment(summary, transcript, 4), 1.0);
        assert_eq!(word_ngram_containment("one two three", transcript, 4), 0.0);
    }
}
