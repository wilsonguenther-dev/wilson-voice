//! Smart dictation v1 (YV3) — context→mode mapping + list formatting (Wispr Flow parity).
//!
//! Two pure, side-effect-free helpers:
//!   1. `mode_for_app` — turn the frontmost app name/bundle (resolved by
//!      `focus::frontmost_app_name` and stored as `source_app`) into a dictation MODE.
//!   2. `format_dictation` — detect LIST intent in a raw transcript and render it as a
//!      numbered list, otherwise preserve the text as prose.
//!
//! Kept pure so they're trivially testable and safe to call anywhere in the pipeline
//! without touching the "never lose text" paste path.

/// Dictation context inferred from the frontmost application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationMode {
    Email,
    Document,
    Notes,
    Code,
    Chat,
    Plain,
}

/// Map a frontmost app name (or bundle id) to a dictation mode via keyword match.
///
/// Matching is substring-based and case-insensitive so it works on both human titles
/// ("Google Chrome") and bundle ids ("com.google.Chrome"). Order matters where keyword
/// sets could overlap; the most specific product families are checked first.
pub fn mode_for_app(app_name: &str) -> DictationMode {
    let a = app_name.to_lowercase();
    let has = |kw: &str| a.contains(kw);

    // Email clients.
    if has("gmail") || has("outlook") || has("superhuman") || has("airmail") || has("mail") {
        return DictationMode::Email;
    }
    // Long-form documents.
    if has("docs") || has("word") || has("pages") || has("notion") {
        return DictationMode::Document;
    }
    // Note-takers.
    if has("notes") || has("bear") || has("obsidian") {
        return DictationMode::Notes;
    }
    // Editors / terminals — code or plain technical text.
    if has("terminal") || has("iterm") || has("code") || has("xcode") || has("warp") {
        return DictationMode::Code;
    }
    // Chat surfaces.
    if has("slack") || has("discord") || has("messages") || has("telegram") || has("whatsapp") {
        return DictationMode::Chat;
    }
    DictationMode::Plain
}

/// Format a raw transcript: render clear list intent as a numbered list, otherwise
/// return the trimmed prose unchanged. Pure — never mutates global state.
pub fn format_dictation(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(list) = detect_and_format_list(trimmed) {
        return list;
    }
    trimmed.to_string()
}

/// Enumerator cue words that, at the start of a clause, mark a new list item.
/// Deliberately excludes weak connectors ("then", "also") to avoid firing on prose.
const CUES: &[&str] = &[
    "first", "firstly", "second", "secondly", "third", "thirdly", "fourth", "fourthly",
    "fifth", "sixth", "seventh", "next", "finally", "lastly",
];

/// If the text is clearly an enumerated list (>= 2 cue-led clauses), return it as a
/// numbered list. Otherwise `None` so the caller keeps it as prose.
fn detect_and_format_list(text: &str) -> Option<String> {
    let clauses: Vec<String> = text
        .split(|c| c == ',' || c == '.' || c == ';' || c == '\n')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if clauses.len() < 2 {
        return None;
    }

    let mut items: Vec<String> = Vec::new();
    let mut started = false;
    for clause in &clauses {
        let (is_cue, rest) = strip_leading_cue(clause);
        if is_cue {
            started = true;
            items.push(rest);
        } else if started {
            // Continuation of the current item (e.g. "first," then the content clause).
            if let Some(last) = items.last_mut() {
                if last.is_empty() {
                    *last = clause.clone();
                } else {
                    last.push_str(", ");
                    last.push_str(clause);
                }
            }
        }
    }

    let items: Vec<String> = items
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    // Require at least two real, cue-led items before calling it a list.
    if items.len() < 2 {
        return None;
    }

    let mut out = String::new();
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format!("{}. {}", i + 1, capitalize(item)));
    }
    Some(out)
}

/// If `clause` begins with an enumerator cue, return `(true, remainder)` with the cue
/// stripped; otherwise `(false, clause)`.
fn strip_leading_cue(clause: &str) -> (bool, String) {
    let mut parts = clause.splitn(2, char::is_whitespace);
    let first = parts.next().unwrap_or("");
    let first_norm = first
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    if CUES.contains(&first_norm.as_str()) {
        (true, parts.next().unwrap_or("").trim().to_string())
    } else {
        (false, clause.to_string())
    }
}

/// Capitalize the first alphabetic character; leaves the rest untouched.
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_maps_to_expected_mode() {
        assert_eq!(mode_for_app("Gmail"), DictationMode::Email);
        assert_eq!(mode_for_app("Microsoft Outlook"), DictationMode::Email);
        assert_eq!(mode_for_app("Google Docs"), DictationMode::Document);
        assert_eq!(mode_for_app("Obsidian"), DictationMode::Notes);
        assert_eq!(mode_for_app("Visual Studio Code"), DictationMode::Code);
        assert_eq!(mode_for_app("Xcode"), DictationMode::Code);
        assert_eq!(mode_for_app("Slack"), DictationMode::Chat);
        assert_eq!(mode_for_app("Roblox"), DictationMode::Plain);
    }

    #[test]
    fn unknown_app_falls_back_to_plain() {
        assert_eq!(mode_for_app(""), DictationMode::Plain);
        assert_eq!(mode_for_app("SomeRandomGame"), DictationMode::Plain);
    }

    #[test]
    fn list_like_input_formats_to_a_list() {
        let out = format_dictation("first, buy milk, second, buy eggs, third, buy bread");
        let expected = "1. Buy milk\n2. Buy eggs\n3. Buy bread";
        assert_eq!(out, expected);
    }

    #[test]
    fn inline_enumerators_also_format_to_a_list() {
        // Cue and content in the same clause (period-separated).
        let out = format_dictation("first get the milk. second get the eggs. third get bread");
        assert_eq!(out, "1. Get the milk\n2. Get the eggs\n3. Get bread");
    }

    #[test]
    fn prose_stays_prose() {
        let prose =
            "The weather today is nice and I think we should go for a walk before it gets dark.";
        assert_eq!(format_dictation(prose), prose.trim());
    }

    #[test]
    fn single_stray_cue_in_prose_does_not_become_a_list() {
        // Only one cue-led clause → not a list; prose is preserved.
        let prose = "I went to the store, and I bought some food, then I came home.";
        assert_eq!(format_dictation(prose), prose.trim());
    }

    #[test]
    fn empty_input_is_empty() {
        assert_eq!(format_dictation("   "), "");
    }
}
