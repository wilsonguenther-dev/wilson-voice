//! Command Mode v1 (YV49) — deterministic voice EDITS on the current selection.
//!
//! Normal dictation TYPES what you said. Command mode EDITS what you already
//! selected: hold the PTT combo with ⌘ (fn⌃⌘ by default — see
//! [`crate::ptt_macos::CommandBinding`]), say an instruction, and the selection
//! is replaced by the transformed text.
//!
//! v1 is deliberately **rule-based, not a model**. Every instruction is matched
//! against a fixed phrase table and every edit is a pure function of the
//! selection, so the same words always produce the same edit and nothing is
//! ever sent anywhere. The hard rule that shapes the whole module:
//!
//! > **An instruction we do not recognise NEVER edits the user's text.**
//!
//! [`parse_command`] returns `None` for anything outside the table and the
//! caller shows [`UNKNOWN_COMMAND_MESSAGE`] as a transient toast. Guessing at a
//! half-understood instruction would silently mangle text the user cannot get
//! back — the same "never lose text" contract the paste path is built on.

/// Toast shown when the spoken instruction is not in the command table. The
/// selection is left EXACTLY as it was.
pub const UNKNOWN_COMMAND_MESSAGE: &str = "Didn't catch a command";

/// Toast shown when a command-mode press starts with nothing selected. Command
/// mode edits a selection; without one there is nothing to act on, so the take
/// never starts.
pub const NO_SELECTION_MESSAGE: &str = "Select some text first — command mode edits your selection";

/// One recognised instruction. Constructed ONLY by [`parse_command`], so an
/// unrecognised utterance cannot reach [`apply`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// "delete that" — remove the selection.
    Delete,
    /// "make it a list" — one bullet per line/clause.
    BulletList,
    /// "make it uppercase".
    UpperCase,
    /// "make it lowercase".
    LowerCase,
    /// "make it title case".
    TitleCase,
    /// "wrap in quotes".
    Quote,
    /// "replace X with Y" — the only parameterised command.
    Replace { from: String, to: String },
    /// "add punctuation".
    Punctuate,
    /// "make it one paragraph" — the inverse of `BulletList`.
    OneParagraph,
}

impl Command {
    /// Short past-tense label for the confirmation toast ("Made it uppercase").
    pub fn label(&self) -> String {
        match self {
            Self::Delete => "Deleted the selection".into(),
            Self::BulletList => "Made it a list".into(),
            Self::UpperCase => "Made it uppercase".into(),
            Self::LowerCase => "Made it lowercase".into(),
            Self::TitleCase => "Made it title case".into(),
            Self::Quote => "Wrapped it in quotes".into(),
            Self::Replace { from, to } => format!("Replaced “{from}” with “{to}”"),
            Self::Punctuate => "Added punctuation".into(),
            Self::OneParagraph => "Made it one paragraph".into(),
        }
    }
}

/// Apply a recognised command to the selection. Pure — the whole edit surface
/// of command mode is this one dispatch over the transform functions below.
pub fn apply(command: &Command, selection: &str) -> String {
    match command {
        Command::Delete => delete_selection(selection),
        Command::BulletList => to_bullet_list(selection),
        Command::UpperCase => to_upper_case(selection),
        Command::LowerCase => to_lower_case(selection),
        Command::TitleCase => to_title_case(selection),
        Command::Quote => wrap_in_quotes(selection),
        Command::Replace { from, to } => replace_phrase(selection, from, to),
        Command::Punctuate => add_punctuation(selection),
        Command::OneParagraph => to_one_paragraph(selection),
    }
}

/// Match a spoken instruction against the command table.
///
/// `None` means "not a command" — the caller must toast and leave the text
/// alone. Matching is on the FULLY normalized utterance (lowercased, trimmed of
/// edge punctuation, whitespace collapsed, leading "yap"/"please"/"can you"
/// stripped) and is EXACT against the table: a partial or fuzzy match would be
/// a guess, and guesses edit text the user cannot recover.
pub fn parse_command(instruction: &str) -> Option<Command> {
    // "replace X with Y" carries operands, so it is parsed from the (case
    // preserving) utterance before the fixed-phrase table.
    if let Some(replace) = parse_replace(instruction) {
        return Some(replace);
    }
    let normalized = strip_filler(&normalize(instruction));
    match normalized.as_str() {
        "delete that"
        | "delete this"
        | "delete it"
        | "delete the selection"
        | "scratch that"
        | "remove that" => Some(Command::Delete),

        "make it a list"
        | "make this a list"
        | "make that a list"
        | "turn it into a list"
        | "turn this into a list"
        | "make it a bullet list"
        | "make it bullets"
        | "bullet list"
        | "list it" => Some(Command::BulletList),

        "make it uppercase"
        | "make this uppercase"
        | "make that uppercase"
        | "make it upper case"
        | "make this upper case"
        | "make that upper case"
        | "uppercase"
        | "upper case"
        | "uppercase that"
        | "all caps"
        | "make it all caps" => Some(Command::UpperCase),

        "make it lowercase"
        | "make this lowercase"
        | "make that lowercase"
        | "make it lower case"
        | "make this lower case"
        | "make that lower case"
        | "lowercase"
        | "lower case"
        | "lowercase that" => Some(Command::LowerCase),

        "make it title case"
        | "make this title case"
        | "make that title case"
        | "title case"
        | "title case that"
        | "make it a title" => Some(Command::TitleCase),

        "wrap in quotes"
        | "wrap it in quotes"
        | "wrap this in quotes"
        | "wrap that in quotes"
        | "put it in quotes"
        | "put that in quotes"
        | "quote that" => Some(Command::Quote),

        "add punctuation"
        | "add the punctuation"
        | "add punctuation to that"
        | "punctuate that"
        | "punctuate it"
        | "fix the punctuation" => Some(Command::Punctuate),

        "make it one paragraph"
        | "make this one paragraph"
        | "make that one paragraph"
        | "one paragraph"
        | "make it a paragraph"
        | "join the lines" => Some(Command::OneParagraph),

        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Transforms — one pure function per command, each independently unit-tested.
// None of them can panic on arbitrary input, and each is idempotent or
// naturally re-appliable, so a user who repeats a command sees no surprise.
// ---------------------------------------------------------------------------

/// "delete that" — the selection becomes nothing.
///
/// Takes the selection it discards so every command shares one signature (see
/// [`apply`]). The caller does NOT paste this empty string: an empty pasteboard
/// is a no-op in several apps, so `Command::Delete` is carried out with a real
/// Delete keystroke (`paste::delete_selection`), which also leaves the user's
/// clipboard untouched.
pub fn delete_selection(_selection: &str) -> String {
    String::new()
}

/// "make it a list" — one `- ` bullet per line, or per clause when the
/// selection is a single run-on line. Existing bullet/number markers are
/// stripped first so re-running never stacks `- - item`.
pub fn to_bullet_list(text: &str) -> String {
    let mut items: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if items.len() < 2 {
        // Single line: fall back to spoken clause boundaries.
        items = text
            .split(['.', ';', ',', '\n'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
    }
    if items.is_empty() {
        return text.trim().to_string();
    }
    items
        .iter()
        .map(|item| format!("- {}", strip_bullet(item)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// "make it uppercase".
pub fn to_upper_case(text: &str) -> String {
    text.to_uppercase()
}

/// "make it lowercase".
pub fn to_lower_case(text: &str) -> String {
    text.to_lowercase()
}

/// "make it title case" — capitalize the first character of every whitespace
/// separated word, lowercase the rest. Line breaks and spacing are preserved.
pub fn to_title_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at_word_start = true;
    for ch in text.chars() {
        if ch.is_whitespace() {
            at_word_start = true;
            out.push(ch);
        } else if at_word_start {
            at_word_start = false;
            out.extend(ch.to_uppercase());
        } else {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

/// "wrap in quotes" — straight double quotes around the trimmed selection.
/// Idempotent: already-quoted text is returned unchanged.
pub fn wrap_in_quotes(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return text.to_string();
    }
    let already = trimmed.len() > 1 && trimmed.starts_with('"') && trimmed.ends_with('"');
    if already {
        return trimmed.to_string();
    }
    format!("\"{trimmed}\"")
}

/// "replace X with Y" — every ASCII-case-insensitive occurrence of `from`
/// becomes `to`, exactly as spoken. An empty `from` is a no-op (never rewrite
/// the whole selection on a mis-parse).
pub fn replace_phrase(text: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(hit) = find_ignore_ascii_case(&text[cursor..], from) {
        out.push_str(&text[cursor..cursor + hit]);
        out.push_str(to);
        cursor += hit + from.len();
    }
    out.push_str(&text[cursor..]);
    out
}

/// "add punctuation" — capitalize the first letter and guarantee the selection
/// ends in terminal punctuation. Conservative by design: it never re-punctuates
/// the interior, because splitting someone's sentences is a guess.
pub fn add_punctuation(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return text.to_string();
    }
    let mut out = capitalize_first(trimmed);
    if !out.ends_with(['.', '!', '?', ':', ';']) {
        out.push('.');
    }
    out
}

/// "make it one paragraph" — the inverse of [`to_bullet_list`]: drop bullet /
/// number markers, join every line with a single space, collapse whitespace.
pub fn to_one_paragraph(text: &str) -> String {
    text.lines()
        .map(|line| strip_bullet(line.trim()))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Lowercase, drop sentence punctuation, and collapse whitespace, so "Make it
/// uppercase.", "make  it uppercase" and "Yap, make it uppercase" all reduce to
/// the same table key. Only punctuation that ASR sprinkles between words is
/// removed — no table phrase contains any of it.
fn normalize(instruction: &str) -> String {
    instruction
        .to_lowercase()
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | '.' | '!' | '?' | ';' | ':'))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Politeness and wake-word noise people put in front of an instruction.
const LEADING_FILLERS: &[&str] = &[
    "hey yap",
    "ok yap",
    "okay yap",
    "yap",
    "please",
    "can you",
    "could you",
];

/// Strip leading fillers (repeatedly — "yap please make it a list") and a
/// trailing "please". Everything else is left alone, so a sentence that merely
/// *starts* politely still has to match the table exactly.
fn strip_filler(normalized: &str) -> String {
    let mut rest = normalized;
    loop {
        let before = rest;
        for filler in LEADING_FILLERS {
            if let Some(tail) = rest.strip_prefix(filler) {
                // Only a whole-word prefix — "yapping" is not "yap".
                if tail.is_empty() || tail.starts_with(' ') {
                    rest = tail.trim_start();
                    break;
                }
            }
        }
        if rest == before {
            break;
        }
    }
    rest.trim_end_matches(" please").trim().to_string()
}

/// Parse "replace X with Y" off the ORIGINAL utterance so the operands keep the
/// casing the user spoke. The keywords are matched ASCII-case-insensitively;
/// the LAST " with " wins so "replace sign off with best, Wilson" splits where
/// the user meant. Either operand empty → not a command.
fn parse_replace(instruction: &str) -> Option<Command> {
    let trimmed = instruction
        .trim()
        .trim_matches(|c: char| c.is_ascii_punctuation() || c.is_whitespace());
    const PREFIX: &str = "replace ";
    let head = trimmed.get(..PREFIX.len())?;
    if !head.eq_ignore_ascii_case(PREFIX) {
        return None;
    }
    let rest = &trimmed[PREFIX.len()..];
    let at = rfind_ignore_ascii_case(rest, " with ")?;
    let from = rest[..at].trim();
    let to = rest[at + " with ".len()..].trim();
    if from.is_empty() || to.is_empty() {
        return None;
    }
    Some(Command::Replace {
        from: from.to_string(),
        to: to.to_string(),
    })
}

/// First ASCII-case-insensitive occurrence of `needle`, as a byte offset on a
/// char boundary. Byte-wise on purpose: the matched slice is always exactly
/// `needle.len()` bytes long, so callers can splice without re-measuring.
fn find_ignore_ascii_case(haystack: &str, needle: &str) -> Option<usize> {
    ascii_case_positions(haystack, needle).next()
}

/// Last ASCII-case-insensitive occurrence of `needle`.
fn rfind_ignore_ascii_case(haystack: &str, needle: &str) -> Option<usize> {
    ascii_case_positions(haystack, needle).last()
}

fn ascii_case_positions<'a>(
    haystack: &'a str,
    needle: &'a str,
) -> impl Iterator<Item = usize> + 'a {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    // Exclusive bound: 0 for an empty needle, or a needle longer than the
    // haystack — both mean "no positions to test".
    let limit = match h.len().checked_sub(n.len()) {
        Some(last) if !n.is_empty() => last + 1,
        _ => 0,
    };
    (0..limit)
        .filter(move |&i| haystack.is_char_boundary(i) && h[i..i + n.len()].eq_ignore_ascii_case(n))
}

/// Drop a leading `- `, `* `, `• `, `– `, `1. ` or `1) ` marker.
fn strip_bullet(line: &str) -> &str {
    let trimmed = line.trim_start();
    for marker in ["- ", "* ", "• ", "– "] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            return rest.trim_start();
        }
    }
    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let rest = &trimmed[digits..];
        for marker in [". ", ") "] {
            if let Some(tail) = rest.strip_prefix(marker) {
                return tail.trim_start();
            }
        }
    }
    trimmed
}

fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- one test per transform ------------------------------------------

    #[test]
    fn delete_empties_the_selection() {
        assert_eq!(delete_selection("throw this away"), "");
        assert_eq!(delete_selection(""), "");
    }

    #[test]
    fn bullet_list_from_lines_and_from_clauses() {
        // Multi-line selection → one bullet per line.
        assert_eq!(
            to_bullet_list("milk\neggs\nbread"),
            "- milk\n- eggs\n- bread"
        );
        // Single run-on line → split on spoken clause boundaries.
        assert_eq!(
            to_bullet_list("milk, eggs, bread"),
            "- milk\n- eggs\n- bread"
        );
        // Idempotent: existing markers are stripped, never stacked.
        assert_eq!(to_bullet_list("- milk\n2. eggs"), "- milk\n- eggs");
        // Degenerate input is returned intact, never emptied.
        assert_eq!(to_bullet_list("solo"), "- solo");
        assert_eq!(to_bullet_list("   "), "");
    }

    #[test]
    fn upper_case_transform() {
        assert_eq!(to_upper_case("ship it today"), "SHIP IT TODAY");
        assert_eq!(to_upper_case(""), "");
    }

    #[test]
    fn lower_case_transform() {
        assert_eq!(to_lower_case("SHIP IT Today"), "ship it today");
        assert_eq!(to_lower_case(""), "");
    }

    #[test]
    fn title_case_transform() {
        assert_eq!(to_title_case("ship it today"), "Ship It Today");
        assert_eq!(to_title_case("SHIP IT TODAY"), "Ship It Today");
        // Line structure survives.
        assert_eq!(to_title_case("one two\nthree"), "One Two\nThree");
    }

    #[test]
    fn wrap_in_quotes_transform() {
        assert_eq!(wrap_in_quotes("ship it"), "\"ship it\"");
        assert_eq!(wrap_in_quotes("  ship it  "), "\"ship it\"");
        // Idempotent — a second "wrap in quotes" does not double-quote.
        assert_eq!(wrap_in_quotes("\"ship it\""), "\"ship it\"");
        assert_eq!(wrap_in_quotes(""), "");
    }

    #[test]
    fn replace_phrase_transform() {
        assert_eq!(
            replace_phrase("call Aidan and Aidan again", "Aidan", "Jeisil"),
            "call Jeisil and Jeisil again"
        );
        // Case-insensitive match, replacement keeps the casing as spoken.
        assert_eq!(replace_phrase("Ship IT", "ship", "send"), "send IT");
        // No match → untouched. Empty needle → untouched (never rewrite all).
        assert_eq!(replace_phrase("ship it", "zzz", "x"), "ship it");
        assert_eq!(replace_phrase("ship it", "", "x"), "ship it");
    }

    #[test]
    fn add_punctuation_transform() {
        assert_eq!(add_punctuation("ship it today"), "Ship it today.");
        // Already terminated → only the capital is added.
        assert_eq!(add_punctuation("ship it today!"), "Ship it today!");
        assert_eq!(add_punctuation("  "), "  ");
    }

    #[test]
    fn one_paragraph_transform() {
        assert_eq!(
            to_one_paragraph("- milk\n- eggs\n- bread"),
            "milk eggs bread"
        );
        assert_eq!(
            to_one_paragraph("first line\n\n   second line  "),
            "first line second line"
        );
        // Round-trips with to_bullet_list.
        assert_eq!(to_one_paragraph(&to_bullet_list("a\nb")), "a b");
    }

    // --- parsing ----------------------------------------------------------

    #[test]
    fn parses_every_command_in_the_table() {
        assert_eq!(parse_command("delete that"), Some(Command::Delete));
        assert_eq!(parse_command("Make it a list."), Some(Command::BulletList));
        assert_eq!(parse_command("make it uppercase"), Some(Command::UpperCase));
        assert_eq!(parse_command("all caps"), Some(Command::UpperCase));
        assert_eq!(parse_command("make it lowercase"), Some(Command::LowerCase));
        assert_eq!(parse_command("title case"), Some(Command::TitleCase));
        assert_eq!(parse_command("wrap in quotes"), Some(Command::Quote));
        assert_eq!(parse_command("add punctuation"), Some(Command::Punctuate));
        assert_eq!(
            parse_command("make it one paragraph"),
            Some(Command::OneParagraph)
        );
        assert_eq!(
            parse_command("replace Aidan with Jeisil"),
            Some(Command::Replace {
                from: "Aidan".into(),
                to: "Jeisil".into()
            })
        );
    }

    #[test]
    fn parsing_tolerates_filler_casing_and_punctuation() {
        assert_eq!(
            parse_command("  Yap, please make it a list!  "),
            Some(Command::BulletList)
        );
        assert_eq!(
            parse_command("can you make it uppercase please"),
            Some(Command::UpperCase)
        );
        // "yap" only strips as a whole word.
        assert_eq!(parse_command("yapping about that"), None);
    }

    #[test]
    fn replace_uses_the_last_with_and_keeps_spoken_casing() {
        assert_eq!(
            parse_command("replace sign off with Best, Wilson"),
            Some(Command::Replace {
                from: "sign off".into(),
                to: "Best, Wilson".into()
            })
        );
        // Missing operands are NOT a command.
        assert_eq!(parse_command("replace with"), None);
        assert_eq!(parse_command("replace Aidan"), None);
    }

    /// The guard that protects the user's text: anything outside the table is
    /// rejected outright so the caller toasts instead of guess-editing.
    #[test]
    fn unknown_instruction_is_never_a_command() {
        for instruction in [
            "",
            "   ",
            "make it sound more professional",
            "rewrite this as a haiku",
            "so anyway I was thinking we should ship on Friday",
            "delete",
            "list",
            "make it",
            "hello there",
        ] {
            assert_eq!(
                parse_command(instruction),
                None,
                "must not be treated as a command: {instruction:?}"
            );
        }
    }

    #[test]
    fn apply_dispatches_to_the_right_transform() {
        assert_eq!(apply(&Command::UpperCase, "ship it"), "SHIP IT");
        assert_eq!(apply(&Command::Delete, "ship it"), "");
        assert_eq!(
            apply(
                &Command::Replace {
                    from: "ship".into(),
                    to: "send".into()
                },
                "ship it"
            ),
            "send it"
        );
    }
}
