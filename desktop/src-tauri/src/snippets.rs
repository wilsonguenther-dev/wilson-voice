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

// ---------------------------------------------------------------------------
// Signature (YV62) — R13's opt-in, byte-exact sign-off block.
//
// It lives HERE, in the snippet stage, and not in the cleanup pipeline, because
// R13 is explicit about it: "Yap appends the user's configured sign-off block …
// The signature is inserted AFTER the LLM stage, by `snippets.rs`, so the model
// can never mangle it." A model that invents a signature is a correctness bug —
// `polish::validate_polish` rejects one (V3/V5) — and the only block that ever
// reaches the pasteboard is the configured string, copied verbatim.
//
// Default is `Off`: nothing is ever appended until the user asks for it.
// ---------------------------------------------------------------------------

/// When the configured signature block is appended (R13).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SignatureMode {
    /// Never — the default, and what an install that never opens this setting has.
    #[default]
    Off,
    /// Only when the user asks for it in the take ("sign it").
    Cue,
    /// On an explicit cue, and on any email that ends with a sign-off line.
    Auto,
}

impl SignatureMode {
    /// Parse the `signature_mode` setting. Unknown/empty values fall back to the
    /// default (`Off`), mirroring the Settings control: `off | cue | auto`.
    pub fn from_setting(setting: &str) -> Self {
        match setting.trim().to_lowercase().as_str() {
            "cue" => SignatureMode::Cue,
            "auto" => SignatureMode::Auto,
            _ => SignatureMode::Off,
        }
    }
}

/// Spoken phrases that ask for the signature explicitly (R13's "sign it").
/// Recognised only as the TAIL of a take, so "sign off on the budget" is content.
const SIGNATURE_CUES: &[&str] = &["sign me off", "sign it off", "sign it", "sign off"];

/// Append the configured signature block, or leave the text exactly as it is.
///
/// Pure, and the LAST thing that touches a take. The block is copied BYTE FOR
/// BYTE — it is never reflowed, re-cased or regenerated — which is the whole
/// point of running it here rather than asking the model for a sign-off.
///
/// It is appended only when:
///   * `sig_mode` is not `Off` and a signature is actually configured, AND
///   * the take ends with a spoken cue ("sign it"), which is consumed, or
///   * `sig_mode` is `Auto`, the take is an email, and it already ends with a
///     sign-off line (`dictation::ends_with_signoff_line` — what R13's shape rule
///     leaves behind).
pub fn append_signature(
    text: &str,
    signature: &str,
    sig_mode: SignatureMode,
    mode: crate::dictation::DictationMode,
) -> String {
    if sig_mode == SignatureMode::Off || signature.trim().is_empty() {
        return text.to_string();
    }
    let (body, cued) = take_signature_cue(text);
    let wanted = cued
        || (sig_mode == SignatureMode::Auto
            && mode == crate::dictation::DictationMode::Email
            && crate::dictation::ends_with_signoff_line(&body));
    // "Never lose text" holds here too: a take that is nothing but the cue is
    // left alone rather than replaced by a signature.
    if !wanted || body.trim().is_empty() {
        return text.to_string();
    }
    format!("{}\n{signature}", body.trim_end())
}

/// Strip a trailing signature cue, reporting whether one was there. The body
/// keeps its own whitespace — the email shape's blank lines survive this.
fn take_signature_cue(text: &str) -> (String, bool) {
    let trimmed = text.trim_end().trim_end_matches(['.', ',', '!', '?']);
    for cue in SIGNATURE_CUES {
        let Some(at) = ci_suffix_start(trimmed, cue) else {
            continue;
        };
        let head = &trimmed[..at];
        // Word boundary ("resign it" is not "sign it"), and a take that is
        // nothing BUT the cue keeps its words rather than becoming a signature.
        if head.ends_with(is_word_char) || head.trim().is_empty() {
            continue;
        }
        return (head.trim_end().to_string(), true);
    }
    (text.to_string(), false)
}

/// Case-insensitive suffix test — the mirror of [`ci_prefix_len`]. Returns the
/// BYTE offset in `hay` where `needle` starts, else `None`. Compared char by char
/// so a multi-byte lowercase mapping can never desync the offsets.
fn ci_suffix_start(hay: &str, needle: &str) -> Option<usize> {
    let mut chars = hay.char_indices().rev();
    let mut at = hay.len();
    for n in needle.chars().rev() {
        let (i, c) = chars.next()?;
        if !c.to_lowercase().eq(n.to_lowercase()) {
            return None;
        }
        at = i;
    }
    Some(at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictation::DictationMode;

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

    // --- Signature (YV62, R13) -------------------------------------------

    /// A signature with the two things a model most likes to invent in one: a
    /// name it was never given and an address (which is also what makes an
    /// invented copy of it fail `validate_polish`'s V5).
    const SIGNATURE: &str = "Wilson Guenther\nwilson@drivia.consulting";

    #[test]
    fn signature_off_by_default_never_appended() {
        // The shipped default is OFF, in the settings and in the parser.
        let settings = crate::AppSettings::default();
        assert_eq!(settings.signature, "");
        assert_eq!(settings.signature_mode, "off");
        assert_eq!(
            SignatureMode::from_setting(&settings.signature_mode),
            SignatureMode::Off
        );
        for unset in ["", "   ", "bogus"] {
            assert_eq!(SignatureMode::from_setting(unset), SignatureMode::Off);
        }
        // Off means off: neither an email that ends on a sign-off line nor an
        // explicit cue appends anything.
        let signed_off = "The build is green\n\nThanks,";
        assert_eq!(
            append_signature(
                signed_off,
                SIGNATURE,
                SignatureMode::Off,
                DictationMode::Email
            ),
            signed_off
        );
        let cued = "the deck is attached sign it";
        assert_eq!(
            append_signature(cued, SIGNATURE, SignatureMode::Off, DictationMode::Email),
            cued
        );
        // …and with the mode ON but nothing configured there is still no block.
        assert_eq!(
            append_signature(signed_off, "  ", SignatureMode::Auto, DictationMode::Email),
            signed_off
        );
    }

    #[test]
    fn signature_auto_appends_after_a_signoff_line_only() {
        // R13: `auto` fires on an email that closed itself with a sign-off.
        assert_eq!(
            append_signature(
                "The build is green\n\nThanks,",
                SIGNATURE,
                SignatureMode::Auto,
                DictationMode::Email
            ),
            format!("The build is green\n\nThanks,\n{SIGNATURE}")
        );
        // Never outside email, and never without the sign-off cue.
        assert_eq!(
            append_signature(
                "The build is green\n\nThanks,",
                SIGNATURE,
                SignatureMode::Auto,
                DictationMode::Chat
            ),
            "The build is green\n\nThanks,"
        );
        assert_eq!(
            append_signature(
                "The build is green",
                SIGNATURE,
                SignatureMode::Auto,
                DictationMode::Email
            ),
            "The build is green"
        );
    }

    #[test]
    fn signature_cue_is_consumed_and_never_fires_on_content() {
        // The spoken cue is shape, not words: it leaves the take.
        assert_eq!(
            append_signature(
                "the deck is attached sign it.",
                SIGNATURE,
                SignatureMode::Cue,
                DictationMode::Notes
            ),
            format!("the deck is attached\n{SIGNATURE}")
        );
        // A cue mid-sentence, a cue inside a longer word, and a take that is
        // ONLY the cue all leave the text exactly as dictated.
        for content in [
            "sign off on the budget before Friday",
            "they had to resign it after the audit",
            "sign it",
        ] {
            assert_eq!(
                append_signature(content, SIGNATURE, SignatureMode::Cue, DictationMode::Email),
                content,
                "{content:?} was treated as a cue"
            );
        }
        // `cue` does NOT widen to auto's sign-off line.
        let signed_off = "The build is green\n\nThanks,";
        assert_eq!(
            append_signature(
                signed_off,
                SIGNATURE,
                SignatureMode::Cue,
                DictationMode::Email
            ),
            signed_off
        );
    }
}
