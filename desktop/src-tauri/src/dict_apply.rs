//! yap-1106-vocab-bias — how a Dictionary rewrite rule is applied to a
//! transcript, for dictation AND meetings (one implementation, two callers).
//!
//! A Dictionary row is `term` (what the recogniser wrote) → `preferred` (what
//! the user meant). The old applier matched whitespace tokens case-blind, which
//! had two defects this module fixes:
//!
//! * Multi-word misheard forms ("Plus Lane", "Whisper Fill", "St. Marcos")
//!   could never match: no single token equals a phrase.
//! * Case-blind matching turns ordinary English into a brand name: a rule
//!   `Trivia → Drivia` rewrote "trivia night".
//!
//! DECISION (item prompt): by default replace only CAPITALISED matches of the
//! exact misheard form, with word boundaries on both sides.
//!
//! * A term that carries an upper-case letter is matched case-SENSITIVELY. A
//!   lower-case "trivia" never matches `Trivia`.
//! * Capitalisation is only a signal away from a sentence start (the recogniser
//!   capitalises every sentence's first word). So a single Capitalised word at
//!   a sentence start ("Trivia night was fun.") is replaced only when the
//!   preferred spelling already appears in the same text — the speaker's
//!   context. Forms with a capital past their first letter ("ClassLank",
//!   "TechServe", "Plus Lane") are never ordinary sentence starts and need no
//!   context.
//! * A term typed all lower-case keeps the old case-blind behaviour: the user
//!   asked for "any case" by typing it that way.

/// The misheard list from Wilson's 10-29 handoff: `(misheard, meant)`.
/// Seeded once into Wilson's install and offered to everyone through the
/// Dictionary's "Add suggested words" button.
pub const SUGGESTED_MISHEARD: &[(&str, &str)] = &[
    ("Trivia", "Drivia"),
    ("Gunther", "Guenther"),
    ("Plus Lane", "Postline"),
    ("ClassLank", "ClassLink"),
    ("ClassPlank", "ClassLink"),
    ("St. Marcos", "San Marcos"),
    ("Whisper Fill", "Wispr Flow"),
    ("Henshake", "Handshake"),
    ("TechServe", "TechServ"),
];

/// Result of one application: the rewritten text and, per input rule (same
/// index), how many times it fired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub text: String,
    pub hits: Vec<usize>,
}

impl Applied {
    pub fn replacements(&self) -> usize {
        self.hits.iter().sum()
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

/// Collapse internal whitespace so "Plus  Lane" and "Plus Lane" are one rule.
fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One Capitalised word with no other capital ("Trivia", "Gunther") — the only
/// shape that is ambiguous at a sentence start.
fn is_single_capitalised_word(term: &str) -> bool {
    let mut chars = term.chars();
    match chars.next() {
        Some(first) if first.is_uppercase() => {}
        _ => return false,
    }
    !term.chars().any(char::is_whitespace) && !chars.any(char::is_uppercase)
}

/// Is byte offset `at` the first word of a sentence? Looks back past spaces
/// and opening quotes/brackets: start of text, a newline, or sentence-ending
/// punctuation means yes.
fn at_sentence_start(text: &str, at: usize) -> bool {
    for c in text[..at].chars().rev() {
        match c {
            '\n' | '\r' => return true,
            c if c.is_whitespace() => continue,
            '"' | '\'' | '(' | '[' | '\u{201C}' | '\u{2018}' => continue,
            '.' | '!' | '?' | '\u{2026}' => return true,
            _ => return false,
        }
    }
    true
}

/// Byte offsets of every whole-word occurrence of `needle` in `hay`.
fn find_whole(hay: &str, needle: &str, case_sensitive: bool) -> Vec<usize> {
    let mut found = Vec::new();
    if needle.is_empty() || needle.len() > hay.len() {
        return found;
    }
    let mut i = 0usize;
    while i + needle.len() <= hay.len() {
        if !hay.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let end = i + needle.len();
        if hay.is_char_boundary(end) {
            let window = &hay[i..end];
            let hit = if case_sensitive {
                window == needle
            } else {
                window.eq_ignore_ascii_case(needle)
            };
            let left_ok = !matches!(hay[..i].chars().next_back(), Some(c) if is_word_char(c));
            let right_ok = !matches!(hay[end..].chars().next(), Some(c) if is_word_char(c));
            if hit && left_ok && right_ok {
                found.push(i);
                i = end;
                continue;
            }
        }
        i += 1;
    }
    found
}

/// Apply `rules` (`(term, preferred)`, in order) to `text`. Pure: no DB, no
/// logging — callers log `vocab applied track=… replacements=…`.
pub fn apply_rules(text: &str, rules: &[(String, String)]) -> Applied {
    let context = text;
    let mut out = text.to_string();
    let mut hits = vec![0usize; rules.len()];
    for (idx, (term, preferred)) in rules.iter().enumerate() {
        let term = normalize_ws(term);
        let preferred = preferred.trim();
        if term.is_empty() || preferred.is_empty() || term == preferred {
            continue;
        }
        let case_sensitive = term.chars().any(char::is_uppercase);
        let gated = case_sensitive && is_single_capitalised_word(&term);
        let preferred_in_context = gated && !find_whole(context, preferred, false).is_empty();
        let starts = find_whole(&out, &term, case_sensitive);
        if starts.is_empty() {
            continue;
        }
        let mut next = String::with_capacity(out.len());
        let mut cursor = 0usize;
        for start in starts {
            if gated && !preferred_in_context && at_sentence_start(&out, start) {
                continue;
            }
            next.push_str(&out[cursor..start]);
            next.push_str(preferred);
            cursor = start + term.len();
            hits[idx] += 1;
        }
        next.push_str(&out[cursor..]);
        out = next;
    }
    Applied { text: out, hits }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suggested() -> Vec<(String, String)> {
        SUGGESTED_MISHEARD
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    /// The 8 pairs (ClassLink has two misheard forms) on a meeting-segment
    /// fixture: every misheard form, mid-sentence and capitalised, comes out
    /// spelled right.
    #[test]
    fn eight_pairs_fixed_on_a_meeting_segment() {
        let segment = "so I told them Trivia ships Friday, and Gunther signs. \
                       We pitched Plus Lane to ClassLank and ClassPlank in St. Marcos, \
                       then compared it with Whisper Fill, Henshake and TechServe.";
        let got = apply_rules(segment, &suggested());
        assert_eq!(
            got.text,
            "so I told them Drivia ships Friday, and Guenther signs. \
             We pitched Postline to ClassLink and ClassLink in San Marcos, \
             then compared it with Wispr Flow, Handshake and TechServ."
        );
        assert_eq!(got.replacements(), 9);
        assert!(
            got.hits.iter().all(|&h| h == 1),
            "every rule fired once: {:?}",
            got.hits
        );
    }

    /// No false positives: "trivia night" is ordinary English and stays, at a
    /// sentence start too, when Drivia is not in the speaker's context.
    #[test]
    fn trivia_night_stays_without_drivia_in_context() {
        let rules = suggested();
        let lower = apply_rules("we went to trivia night at the pub.", &rules);
        assert_eq!(lower.text, "we went to trivia night at the pub.");
        assert_eq!(lower.replacements(), 0);

        let start = apply_rules("Trivia night was fun. Then Gunther won it.", &rules);
        assert_eq!(start.text, "Trivia night was fun. Then Guenther won it.");

        // Inside a word, never: "Trivial", "TechServes", "Gunthers".
        let inside = apply_rules("Trivial TechServes and Gunthers", &rules);
        assert_eq!(inside.text, "Trivial TechServes and Gunthers");
    }

    /// With Drivia already in the speaker's context, a sentence-start
    /// "Trivia" is the misheard brand.
    #[test]
    fn sentence_start_replaced_when_preferred_in_context() {
        let got = apply_rules("Trivia is growing. Drivia raised a round.", &suggested());
        assert_eq!(got.text, "Drivia is growing. Drivia raised a round.");
    }

    /// A lower-case user term keeps the old case-blind behaviour.
    #[test]
    fn lowercase_term_is_case_blind() {
        let rules = vec![("divya".to_string(), "Drivia".to_string())];
        assert_eq!(
            apply_rules("meet Divya, divya!", &rules).text,
            "meet Drivia, Drivia!"
        );
        let cap = vec![("Divya".to_string(), "Drivia".to_string())];
        assert_eq!(apply_rules("meet Divya", &cap).text, "meet Drivia");
    }
}
