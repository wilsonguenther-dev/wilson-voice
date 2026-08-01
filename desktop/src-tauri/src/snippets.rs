//! Snippets (YV48) — spoken trigger phrases expand to saved text.
//!
//! Wispr Flow parity (docs/research/wispr-parity.md — "text personalization"),
//! fully local: the user stores `trigger phrase → expansion text` rows in the
//! `snippets` table (see `db.rs`) and this module is the PURE matcher that
//! rewrites a transcript with them.
//!
//! Matching rules:
//!   * case-insensitive, at WORD BOUNDARIES only ("addy" never fires inside
//!     "paddy"),
//!   * LONGEST match first, so "my work email" beats "my email",
//!   * left-to-right, single pass — an expansion is never re-scanned, so a
//!     snippet whose expansion contains another trigger cannot cascade.
//!
//! Scope is a user toggle ([`SnippetScope`]): `Inline` rewrites matches anywhere
//! in the transcript; `WholeUtterance` fires ONLY when the entire utterance is
//! the trigger (the conservative "say the shortcut and nothing else" mode).
//!
//! Guard: this stage runs on the dictation paste path ONLY, as its own stage
//! AFTER the cleanup pipeline (`dictation::run_cleanup`). It is deliberately not
//! reachable from the dictionary path (`Database::apply_dictionary`, which only
//! ever rewrites single tokens) nor from the command paths (`copy_entry` /
//! `paste_entry` / `cli.rs`), so a stored transcript can never be re-expanded.
//! The matcher is total — it cannot fail — and the DB read that feeds it is
//! guarded at the call site so a snippet error pastes the raw text unchanged.

/// One `trigger → expansion` rule, already filtered to the enabled rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnippetRule {
    pub trigger: String,
    pub expansion: String,
}

/// Where a trigger is allowed to match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnippetScope {
    /// Rewrite every match anywhere in the transcript.
    Inline,
    /// Fire only when the WHOLE utterance is the trigger.
    WholeUtterance,
}

impl SnippetScope {
    /// Parse the `snippet_scope` setting. Unknown/empty values fall back to the
    /// default (`Inline`), mirroring the Settings toggle: `inline | utterance`.
    pub fn from_setting(setting: &str) -> Self {
        match setting.trim().to_lowercase().as_str() {
            "utterance" | "whole" | "whole_utterance" => SnippetScope::WholeUtterance,
            _ => SnippetScope::Inline,
        }
    }
}

/// A char that can be part of a trigger's "word" — used for the boundary test.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\''
}

/// Case-insensitive prefix test. Returns the number of BYTES of `hay` consumed
/// when it starts with `needle`, else `None`. Compared char-by-char so a
/// multi-byte lowercase mapping can never desync the byte offsets.
fn ci_prefix_len(hay: &str, needle: &str) -> Option<usize> {
    let mut h = hay.chars();
    let mut used = 0usize;
    for n in needle.chars() {
        let c = h.next()?;
        if !c.to_lowercase().eq(n.to_lowercase()) {
            return None;
        }
        used += c.len_utf8();
    }
    Some(used)
}

/// Trim the trailing sentence punctuation ASR likes to append, so
/// "my address." still matches the trigger "my address" in whole-utterance mode.
fn trim_utterance(text: &str) -> &str {
    text.trim()
        .trim_end_matches(['.', ',', '!', '?', ';', ':'])
        .trim()
}

/// Rules that can actually match, ordered LONGEST trigger first so a longer
/// phrase always wins over a shorter one that prefixes it.
fn ranked(rules: &[SnippetRule]) -> Vec<&SnippetRule> {
    let mut out: Vec<&SnippetRule> = rules
        .iter()
        .filter(|r| !r.trigger.trim().is_empty() && !r.expansion.is_empty())
        .collect();
    // Ties broken by trigger text so the result is deterministic.
    out.sort_by(|a, b| {
        b.trigger
            .chars()
            .count()
            .cmp(&a.trigger.chars().count())
            .then_with(|| a.trigger.cmp(&b.trigger))
    });
    out
}

/// Expand snippet triggers in `text`.
///
/// Pure and total: with no rules, no matches, or an empty transcript the input
/// is returned unchanged. The final guard mirrors the cleanup pipeline's "never
/// lose text" contract — an expansion can never turn non-empty text into empty.
pub fn expand_snippets(text: &str, rules: &[SnippetRule], scope: SnippetScope) -> String {
    let ranked = ranked(rules);
    if ranked.is_empty() || text.trim().is_empty() {
        return text.to_string();
    }

    let out = match scope {
        SnippetScope::WholeUtterance => {
            let body = trim_utterance(text);
            match ranked
                .iter()
                .find(|r| ci_prefix_len(body, r.trigger.trim()) == Some(body.len()))
            {
                Some(r) => r.expansion.clone(),
                None => return text.to_string(),
            }
        }
        SnippetScope::Inline => expand_inline(text, &ranked),
    };

    if out.trim().is_empty() {
        return text.to_string();
    }
    out
}

/// Single left-to-right pass: at every word start, try the longest trigger that
/// matches AND ends on a word boundary. Matched spans are copied from the
/// expansion and skipped, so expansions are never re-scanned.
fn expand_inline(text: &str, ranked: &[&SnippetRule]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    // Whether the previous char allows a word to START here.
    let mut at_boundary = true;

    while !rest.is_empty() {
        if at_boundary {
            let hit = ranked.iter().find_map(|r| {
                let used = ci_prefix_len(rest, r.trigger.trim())?;
                // The char AFTER the match must not continue the word.
                match rest[used..].chars().next() {
                    Some(c) if is_word_char(c) => None,
                    _ => Some((used, *r)),
                }
            });
            if let Some((used, rule)) = hit {
                out.push_str(&rule.expansion);
                rest = &rest[used..];
                at_boundary = true;
                continue;
            }
        }
        let c = rest.chars().next().expect("non-empty");
        out.push(c);
        at_boundary = !is_word_char(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(trigger: &str, expansion: &str) -> SnippetRule {
        SnippetRule {
            trigger: trigger.into(),
            expansion: expansion.into(),
        }
    }

    fn rules() -> Vec<SnippetRule> {
        vec![
            rule("my email", "wilson@drivia.consulting"),
            rule("my work email", "wilson@drivia.consulting (work)"),
            rule("sign off", "Thanks,\nWilson Guenther\nFounder, Drivia"),
        ]
    }

    /// Inline scope rewrites a trigger sitting inside a longer sentence.
    #[test]
    fn inline_match_expands_mid_sentence() {
        let out = expand_snippets("send it to my email please", &rules(), SnippetScope::Inline);
        assert_eq!(out, "send it to wilson@drivia.consulting please");
    }

    /// Whole-utterance scope fires only when the trigger IS the utterance —
    /// the same trigger inside a sentence must be left alone.
    #[test]
    fn whole_utterance_match_only_fires_alone() {
        let alone = expand_snippets("My email.", &rules(), SnippetScope::WholeUtterance);
        assert_eq!(alone, "wilson@drivia.consulting");

        let embedded = expand_snippets(
            "send it to my email please",
            &rules(),
            SnippetScope::WholeUtterance,
        );
        assert_eq!(
            embedded, "send it to my email please",
            "inline fired in whole-utterance mode"
        );
    }

    /// The longer trigger wins over the shorter one it contains.
    #[test]
    fn longest_trigger_wins() {
        let out = expand_snippets("use my work email today", &rules(), SnippetScope::Inline);
        assert_eq!(out, "use wilson@drivia.consulting (work) today");

        let whole = expand_snippets("my work email", &rules(), SnippetScope::WholeUtterance);
        assert_eq!(whole, "wilson@drivia.consulting (work)");
    }

    /// No trigger, no rules, and empty input all pass straight through.
    #[test]
    fn no_match_passes_through_unchanged() {
        let raw = "ship the release notes tonight";
        assert_eq!(expand_snippets(raw, &rules(), SnippetScope::Inline), raw);
        assert_eq!(
            expand_snippets(raw, &rules(), SnippetScope::WholeUtterance),
            raw
        );
        assert_eq!(expand_snippets(raw, &[], SnippetScope::Inline), raw);
        assert_eq!(expand_snippets("", &rules(), SnippetScope::Inline), "");
    }

    /// Word-boundary matching: a trigger inside a bigger word never fires, and
    /// matching is case-insensitive.
    #[test]
    fn matches_on_word_boundaries_case_insensitively() {
        let r = vec![rule("addy", "1 Main St")];
        assert_eq!(
            expand_snippets("my paddy field", &r, SnippetScope::Inline),
            "my paddy field",
            "matched inside a longer word"
        );
        assert_eq!(
            expand_snippets("my Addy, thanks", &r, SnippetScope::Inline),
            "my 1 Main St, thanks"
        );
    }

    /// An expansion is never re-scanned, so a snippet that emits another
    /// trigger cannot cascade (or loop).
    #[test]
    fn expansion_is_not_rescanned() {
        let r = vec![rule("a b", "a b c"), rule("c", "boom")];
        assert_eq!(expand_snippets("a b", &r, SnippetScope::Inline), "a b c");
    }

    /// Blank triggers/expansions are inert, and the setting parses to the
    /// documented default.
    #[test]
    fn blank_rules_are_inert_and_scope_defaults_to_inline() {
        let r = vec![rule("   ", "nope"), rule("hello", "")];
        assert_eq!(
            expand_snippets("hello there", &r, SnippetScope::Inline),
            "hello there"
        );
        assert_eq!(
            SnippetScope::from_setting("utterance"),
            SnippetScope::WholeUtterance
        );
        assert_eq!(SnippetScope::from_setting(""), SnippetScope::Inline);
        assert_eq!(SnippetScope::from_setting("bogus"), SnippetScope::Inline);
    }
}
