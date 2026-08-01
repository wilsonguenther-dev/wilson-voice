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

/// Dictation context inferred from the frontmost application, or picked by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationMode {
    Email,
    Document,
    Notes,
    Code,
    Chat,
    Plain,
    /// User-forced list formatting (never inferred from an app).
    List,
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
    // Backtrack cleanup (YV6): drop fillers + apply spoken self-corrections BEFORE list
    // detection. `clean_backtrack` is itself guarded against emptying non-empty input,
    // but we re-guard here to keep the "never lose text" contract explicit at the seam.
    let cleaned = clean_backtrack(trimmed);
    let cleaned = if cleaned.trim().is_empty() {
        trimmed.to_string()
    } else {
        cleaned
    };
    if let Some(list) = detect_and_format_list(&cleaned) {
        return list;
    }
    cleaned.trim().to_string()
}

/// Resolve the effective dictation mode for a transcription. A user-picked fixed mode
/// (the `dictation_mode` setting) always wins; the sentinel `"auto"` — or any value we
/// don't recognize — defers to app detection via [`mode_for_app`].
///
/// Setting values mirror the Settings picker: `auto | plain | list | email | code | notes`.
pub fn resolve_mode(setting: &str, app_name: &str) -> DictationMode {
    match setting.trim().to_lowercase().as_str() {
        "plain" => DictationMode::Plain,
        "list" => DictationMode::List,
        "email" => DictationMode::Email,
        "code" => DictationMode::Code,
        "notes" => DictationMode::Notes,
        _ => mode_for_app(app_name),
    }
}

/// Whether [`format_dictation`] should run for a given mode. `Code` and `Plain` stay
/// verbatim — we never reflow identifiers or plain prose — everything else is formatted.
pub fn should_format(mode: DictationMode) -> bool {
    !matches!(mode, DictationMode::Code | DictationMode::Plain)
}

// ---------------------------------------------------------------------------
// Context awareness v1 (YV50) — the text ALREADY before the caret steers how the
// dictated text is joined onto it (Wispr Flow parity).
//
// The pipeline reads at most `focus::CONTEXT_CHAR_LIMIT` characters before the
// caret via Accessibility (`focus::text_before_cursor`) and hands them here as a
// borrowed `&str`. Three decisions come out of it:
//   1. casing  — continuing a sentence lowercases the lead word; a fresh
//                sentence (after . ? ! / a new line) or an empty field
//                capitalises it.
//   2. spacing — a leading space is added only when the character before the
//                caret needs one.
//   3. mode    — an email surface (greeting / sign-off / header line) is a HINT
//                for mode detection when the app alone resolved nothing.
//
// PRIVACY (hard rule): everything here is PURE and takes the context by
// reference. No function stores it, returns it, or emits it — only decisions
// derived from it leave this module. `lib.rs` never logs the binding either
// (enforced by `cursor_context_is_never_logged_or_persisted`).
// ---------------------------------------------------------------------------

/// What to do with the first letter of the dictated text given the context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeadCase {
    /// Start of a sentence, a new line, or an empty field.
    Capitalize,
    /// Continuing a sentence already in progress.
    Lowercase,
    /// No context signal (Accessibility denied, secure field, unknown caret) —
    /// leave the ASR casing exactly as dictated.
    Leave,
}

/// Sentence-final punctuation: what precedes a fresh, capitalised sentence.
const SENTENCE_ENDERS: &[char] = &['.', '?', '!'];

/// Characters that already "own" the gap after them — no space is inserted when
/// the caret sits right behind one (openers, quotes, and joiners like `/` `@`).
const NO_SPACE_AFTER: &[char] = &['(', '[', '{', '"', '\'', '“', '‘', '/', '@', '#', '-', '_'];

/// Leading characters on the dictated side that must hug the previous word —
/// punctuation and closers never get a space pushed in front of them.
const NO_SPACE_BEFORE: &[char] = &[
    ',', '.', ';', ':', '!', '?', ')', ']', '}', '"', '\'', '”', '’', '%',
];

/// Casing decision for the lead word, from the text before the caret.
pub fn lead_case_for_context(context: Option<&str>) -> LeadCase {
    let Some(context) = context else {
        // No signal at all — never touch what the model produced.
        return LeadCase::Leave;
    };
    if context.trim().is_empty() {
        // Empty field (or only whitespace behind the caret) → a fresh sentence.
        return LeadCase::Capitalize;
    }
    // A new line is a sentence boundary too (bullet lists, chat drafts, notes).
    if context.ends_with('\n') || context.ends_with('\r') {
        return LeadCase::Capitalize;
    }
    match context.trim_end().chars().last() {
        Some(c) if SENTENCE_ENDERS.contains(&c) => LeadCase::Capitalize,
        // Mid-sentence: the model always capitalises the first word of a take,
        // which is wrong when the user is continuing a sentence they started.
        _ => LeadCase::Lowercase,
    }
}

/// Whether a single leading space is needed to join `text` onto the context.
pub fn needs_leading_space(context: Option<&str>, text: &str) -> bool {
    let Some(context) = context else {
        return false;
    };
    // Empty field or the caret already sits after whitespace/a newline.
    if context.is_empty() || context.ends_with(|c: char| c.is_whitespace()) {
        return false;
    }
    if context.ends_with(|c: char| NO_SPACE_AFTER.contains(&c)) {
        return false;
    }
    match text.chars().next() {
        None => false,
        Some(c) if c.is_whitespace() => false,
        Some(c) if NO_SPACE_BEFORE.contains(&c) => false,
        Some(_) => true,
    }
}

/// Join the dictated text onto the text before the caret: apply the casing
/// decision, then the spacing decision. Never removes characters, so the
/// "never lose text" contract holds (`None` context ⇒ verbatim passthrough).
pub fn join_with_context(text: &str, context: Option<&str>) -> String {
    let cased = apply_lead_case(text, lead_case_for_context(context));
    if needs_leading_space(context, &cased) {
        format!(" {cased}")
    } else {
        cased
    }
}

/// Apply a [`LeadCase`] to the first word of `text`.
fn apply_lead_case(text: &str, case: LeadCase) -> String {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    match case {
        LeadCase::Leave => text.to_string(),
        LeadCase::Capitalize => {
            if first.is_lowercase() {
                first.to_uppercase().collect::<String>() + chars.as_str()
            } else {
                text.to_string()
            }
        }
        LeadCase::Lowercase => {
            if first.is_uppercase() && lead_word_is_safe_to_lowercase(text) {
                first.to_lowercase().collect::<String>() + chars.as_str()
            } else {
                text.to_string()
            }
        }
    }
}

/// Conservative guard on the mid-sentence lowercase: it must not damage words
/// that are capitalised for a REASON. Refuses on the pronoun "I" (and "I'm",
/// "I'll", …), on acronyms ("API"), and on internally-capitalised names
/// ("GitHub", "McDonald") — everything else is the model's sentence-initial
/// capital and is safe to fold back down.
fn lead_word_is_safe_to_lowercase(text: &str) -> bool {
    let Some(word) = text.split_whitespace().next() else {
        return false;
    };
    let core: String = word
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '\'' || *c == '’')
        .collect();
    let letters: String = core.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return false;
    }
    if letters == "I" || core.starts_with("I'") || core.starts_with("I’") {
        return false;
    }
    // Any capital past the first letter ⇒ acronym or CamelCase name.
    !letters.chars().skip(1).any(|c| c.is_uppercase())
}

/// Line prefixes that mark an email compose surface.
const EMAIL_GREETINGS: &[&str] = &[
    "dear ",
    "hi ",
    "hello ",
    "hey ",
    "good morning",
    "good afternoon",
    "good evening",
];

/// Sign-off / header lines that mark an email compose surface.
const EMAIL_SIGNOFFS: &[&str] = &[
    "best,",
    "best regards",
    "kind regards",
    "regards,",
    "thanks,",
    "thank you,",
    "sincerely,",
    "cheers,",
];

/// Header lines of a compose window.
const EMAIL_HEADERS: &[&str] = &["subject:", "to:", "cc:", "bcc:"];

/// Mode HINT read out of the context: an email greeting, sign-off or header line
/// before the caret means the user is writing an email even when the app name
/// says nothing (a browser tab, a webmail Electron shell, a generic client).
/// `None` whenever the context shows no such signal.
pub fn mode_hint_from_context(context: Option<&str>) -> Option<DictationMode> {
    let context = context?;
    for line in context.lines() {
        let line = line.trim().to_lowercase();
        if line.is_empty() {
            continue;
        }
        if EMAIL_HEADERS.iter().any(|h| line.starts_with(h)) {
            return Some(DictationMode::Email);
        }
        if EMAIL_SIGNOFFS.iter().any(|s| line.starts_with(s)) {
            return Some(DictationMode::Email);
        }
        // A greeting only counts when it's punctuated like one ("Hi Sarah," /
        // "Hey!") so ordinary prose starting with "hi" can't trip it.
        if EMAIL_GREETINGS.iter().any(|g| line.starts_with(g))
            && line.ends_with(|c: char| c == ',' || c == '!')
        {
            return Some(DictationMode::Email);
        }
    }
    None
}

/// [`resolve_mode`] plus the YV50 context hint.
///
/// Precedence is deliberate and narrow: a user-picked fixed mode always wins,
/// then the app match, and the context hint only fills the gap left when `auto`
/// detection found nothing (`Plain`). Context steers; it never overrides.
pub fn resolve_mode_with_context(
    setting: &str,
    app_name: &str,
    context: Option<&str>,
) -> DictationMode {
    let mode = resolve_mode(setting, app_name);
    let user_picked = matches!(
        setting.trim().to_lowercase().as_str(),
        "plain" | "list" | "email" | "code" | "notes"
    );
    if mode == DictationMode::Plain && !user_picked {
        if let Some(hint) = mode_hint_from_context(context) {
            return hint;
        }
    }
    mode
}

// ---------------------------------------------------------------------------
// No-speech + hallucination guards (YV16) — stop Whisper pasting phantom text.
//
// A near-silent / sub-second tap makes Whisper hallucinate degenerate repetitive
// tokens ("WPM-SERV-SERV-SERV…") which then get pasted into the focused app. Two
// pure, conservative guards close that hole: (1) gate ASR on TRUE voiced time so
// silence never reaches the model, and (2) reject degenerate repetition loops in
// the raw ASR output before paste. Both are side-effect-free and unit-tested.
// ---------------------------------------------------------------------------

/// Minimum TRUE voiced seconds (energy VAD) required to bother transcribing.
/// Below this a clip is treated as no-speech — a fumbled or near-silent tap — and
/// ASR is skipped so Whisper can't hallucinate text on silence.
const MIN_SPEECH_SECONDS: f64 = 0.35;

/// True when the clip holds enough real speech to transcribe. Fed the TRUE
/// energy-VAD value (`record::RecordingResult.voiced_seconds`, which is 0.0 on
/// silence with NO clip-length fallback), so a silent / sub-second tap gates out
/// before ASR runs.
pub fn has_enough_speech(voiced_seconds: f64) -> bool {
    voiced_seconds >= MIN_SPEECH_SECONDS
}

/// Reject degenerate Whisper "hallucination loops" — the phantom repetitive
/// tokens the model emits when handed near-silence — BEFORE they get pasted into
/// the user's focused app. Pure and deliberately CONSERVATIVE: it must stay quiet
/// for normal prose, a genuine numbered list, and short spoken emphatics
/// ("no no no", "very very good").
///
/// Fires on any of:
///   1. ≥4 consecutive identical whitespace tokens ("the the the the …").
///   2. unique/total whitespace-token ratio < 0.3 when total ≥ 6 — a clip that is
///      overwhelmingly one repeated token (the 6-token floor keeps a short
///      emphatic like "no no no" from tripping it).
///   3. a single whitespace token that, split on `-`/`_`, is a short unit repeated
///      ≥4× ("WPM-SERV-SERV-SERV-SERV" — Whisper emits the whole loop as ONE token).
pub fn is_hallucinated_repetition(text: &str) -> bool {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.is_empty() {
        return false;
    }
    // Rule 1 — ≥4 identical tokens in a row.
    if max_consecutive_identical(&tokens) >= 4 {
        return true;
    }
    // Rule 2 — overwhelmingly one repeated token, on a long-enough clip only.
    let total = tokens.len();
    if total >= 6 {
        let mut uniq: Vec<&str> = tokens.clone();
        uniq.sort_unstable();
        uniq.dedup();
        if (uniq.len() as f64 / total as f64) < 0.3 {
            return true;
        }
    }
    // Rule 3 — a single glued token that is a short unit repeated ≥4× on `-`/`_`.
    for tok in &tokens {
        let parts: Vec<&str> = tok
            .split(|c| c == '-' || c == '_')
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() >= 4 && has_short_repeated_unit(&parts) {
            return true;
        }
    }
    false
}

/// Longest run of identical adjacent items. Helper for the repetition guard.
fn max_consecutive_identical<T: PartialEq>(items: &[T]) -> usize {
    let mut best = 0usize;
    let mut run = 0usize;
    let mut prev: Option<&T> = None;
    for item in items {
        if prev == Some(item) {
            run += 1;
        } else {
            run = 1;
            prev = Some(item);
        }
        if run > best {
            best = run;
        }
    }
    best
}

/// True when `parts` contains a SHORT unit (≤10 chars) repeated ≥4× in a row —
/// the "SERV-SERV-SERV-SERV" signature. The length bound keeps genuinely
/// hyphenated content from being mistaken for a hallucination loop.
fn has_short_repeated_unit(parts: &[&str]) -> bool {
    let mut run = 0usize;
    let mut prev: Option<&str> = None;
    for &p in parts {
        if prev == Some(p) {
            run += 1;
        } else {
            run = 1;
            prev = Some(p);
        }
        if run >= 4 && p.chars().count() <= 10 {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Cleanup pipeline (YV10) — the Wispr-style "polish" pass, architecture only.
//
// Wispr Flow is not a better transcriber; it's ASR + a fine-tuned "cleanup" LLM
// second pass fed rich context (see docs/research/wispr-parity.md §0-1). This
// module lays that architecture WITHOUT downloading a model: an ordered stage
// pipeline gated by an Auto-Cleanup level, with the LLM stage present as a
// guarded NO-OP stub that falls back to its input. Every stage is guarded so a
// non-empty transcript can never become empty ("never lose text").
// ---------------------------------------------------------------------------

/// Auto-Cleanup intensity — Wispr Flow parity (Settings → Style → Auto Cleanup:
/// None / Light / Medium / High). Gates which pipeline stages run. `None` is a
/// pure raw passthrough (nothing is applied — the raw transcript is returned
/// verbatim); higher levels enable progressively more of the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupLevel {
    /// Raw passthrough — no cleanup at all.
    None,
    /// Dictionary/vocabulary replacement + backtrack (filler + self-correction).
    Light,
    /// Light + smart formatting (list detection, etc.).
    Medium,
    /// Medium + the local-LLM polish stage.
    High,
}

impl CleanupLevel {
    /// Parse the `cleanup_level` setting. Unknown/empty values fall back to the
    /// default (`Light`), mirroring the Settings picker: `none | light | medium | high`.
    pub fn from_setting(setting: &str) -> Self {
        match setting.trim().to_lowercase().as_str() {
            "none" => CleanupLevel::None,
            "medium" => CleanupLevel::Medium,
            "high" => CleanupLevel::High,
            _ => CleanupLevel::Light,
        }
    }

    /// Vocabulary/dictionary replacement runs at every level except `None`.
    fn runs_dictionary(self) -> bool {
        self != CleanupLevel::None
    }
    /// Backtrack (filler removal + spoken self-correction) runs from `Light` up.
    fn runs_backtrack(self) -> bool {
        matches!(self, CleanupLevel::Light | CleanupLevel::Medium | CleanupLevel::High)
    }
    /// Smart formatting (list detection) runs from `Medium` up.
    fn runs_format(self) -> bool {
        matches!(self, CleanupLevel::Medium | CleanupLevel::High)
    }
    /// The local-LLM polish stage runs only at `High`.
    fn runs_llm(self) -> bool {
        self == CleanupLevel::High
    }
}

/// LLM polish stage — **guarded NO-OP STUB** (YV10 architecture only).
///
/// The eventual implementation will call a small local instruct model (MLX-LM;
/// Qwen2.5-1.5B/3B-Instruct or Llama-3.2-3B-Instruct, 4-bit) to "recreate exactly
/// what the user would have typed" — Wispr Flow's second pass (see
/// docs/research/wispr-parity.md §1, P0). Downloading/wiring that model is a
/// later runtime setup step; **no model is fetched here**.
///
/// Contract for the future implementation: it MUST be fallible and map any
/// error/timeout to `None` so [`run_cleanup`] falls back to its input — and it
/// must NEVER return `Some("")`. Returning `None` today makes the stage a safe
/// no-op that leaves the transcript untouched.
pub fn polish_llm(_text: &str) -> Option<String> {
    // No local model yet → no-op. When wired: run inference under a timeout and
    // return `None` on any failure so the pipeline keeps the input text.
    None
}

/// Run the ordered cleanup pipeline over a raw transcript.
///
/// Stages, in order (each gated by `level`, each guarded so it can never empty a
/// non-empty transcript):
///   1. `apply_dictionary` — user vocabulary/replacement (supplied as a closure so
///      this stays pure/testable; production passes `Database::apply_dictionary`).
///   2. backtrack cleanup — [`clean_backtrack`] (fillers + spoken self-corrections).
///   3. `format_dictation` — smart formatting (list detection), respecting `mode`.
///   4. LLM polish — `polish` (production passes [`polish_llm`], a guarded no-op stub
///      that falls back to the input on any error/timeout).
///
/// `level == None` short-circuits to a verbatim raw passthrough.
pub fn run_cleanup<D, P>(
    raw: &str,
    level: CleanupLevel,
    mode: DictationMode,
    apply_dictionary: D,
    polish: P,
) -> String
where
    D: Fn(&str) -> String,
    P: Fn(&str) -> Option<String>,
{
    // `None` = raw passthrough: return the transcript exactly as dictated.
    if level == CleanupLevel::None {
        return raw.to_string();
    }

    // Guarded assignment: only accept a stage's output when it's non-empty, so
    // no stage can ever erase a non-empty transcript.
    let mut text = raw.to_string();

    // Stage 1 — dictionary/vocabulary replacement.
    if level.runs_dictionary() {
        let out = apply_dictionary(&text);
        if !out.trim().is_empty() {
            text = out;
        }
    }
    // Stage 2 — backtrack cleanup (fillers + self-correction).
    if level.runs_backtrack() {
        let out = clean_backtrack(&text);
        if !out.trim().is_empty() {
            text = out;
        }
    }
    // Stage 3 — smart formatting. `format_dictation` also runs a backtrack pass
    // internally, so at Medium/High the standalone Stage 2 above is idempotent.
    if level.runs_format() && should_format(mode) {
        let out = format_dictation(&text);
        if !out.trim().is_empty() {
            text = out;
        }
    }
    // Stage 4 — local-LLM polish (guarded). `None`/empty ⇒ keep current text.
    if level.runs_llm() {
        if let Some(out) = polish(&text) {
            if !out.trim().is_empty() {
                text = out;
            }
        }
    }

    // Final "never lose text" backstop.
    if text.trim().is_empty() && !raw.trim().is_empty() {
        return raw.to_string();
    }
    text
}

// ---------------------------------------------------------------------------
// Backtrack v1 (YV6) — rule-based filler + self-correction cleanup (on-device).
//
// A conservative, PURE first pass that mirrors Wispr Flow's "Backtrack" for the
// obvious, low-risk cases while a local-LLM pass (P0 backlog) does the rest. It
// (a) drops standalone filler tokens, (b) applies spoken self-corrections via a
// small set of unambiguous trigger phrases, and (c) NEVER empties the string —
// if a rule would leave nothing, the original text is returned unchanged. Kept
// intentionally narrow: it only fires where intent is clear, never rewriting
// meaning or touching ordinary prose.
// ---------------------------------------------------------------------------

/// Standalone, meaning-free filler tokens that are always safe to drop.
const FILLER_TOKENS: &[&str] = &["um", "umm", "uh", "uhh", "uhm", "er", "erm", "hmm"];

/// Comma-parenthetical discourse fillers — removed ONLY when clearly non-semantic,
/// i.e. bounded by commas on both sides (", like," → ","), which is the signal that
/// they're a hedge rather than a real word (the verb "like", "you know" as content).
const DISCOURSE_FILLERS: &[&str] = &[", like,", ", you know,", ", i mean,"];

/// Conservative rule-based backtrack cleanup: apply spoken self-corrections, then
/// strip fillers. Guarded so a non-empty input can never become empty.
pub fn clean_backtrack(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let corrected = apply_self_correction(trimmed);
    let deflated = remove_fillers(&corrected);
    let cleaned = deflated.trim();
    // Never lose text: if the rules stripped everything (e.g. an all-filler
    // utterance), fall back to the original transcript.
    if cleaned.is_empty() {
        return trimmed.to_string();
    }
    cleaned.to_string()
}

/// Remove filler words: comma-bounded discourse hedges first, then standalone
/// pure-filler tokens ("um", "uh", …) anywhere in the utterance.
fn remove_fillers(text: &str) -> String {
    let mut s = text.to_string();
    for phrase in DISCOURSE_FILLERS {
        s = replace_ci_ascii(&s, phrase, ",");
    }
    s.split_whitespace()
        .filter(|tok| !is_pure_filler(tok))
        .collect::<Vec<_>>()
        .join(" ")
}

/// True when a whitespace token is a standalone pure filler ("um", "uh,", "Er.").
fn is_pure_filler(token: &str) -> bool {
    FILLER_TOKENS.contains(&token_core(token).as_str())
}

/// Apply the unambiguous self-correction triggers: clause-retraction markers
/// ("scratch that", "no wait") drop the retracted clause; a numeric "actually"
/// restatement ("coffee at 2 actually 3" → "coffee at 3") swaps the value.
fn apply_self_correction(text: &str) -> String {
    let mut s = text.to_string();
    for marker in ["scratch that", "no wait"] {
        if let Some(corrected) = apply_clause_correction(&s, marker) {
            s = corrected;
        }
    }
    if let Some(corrected) = apply_numeric_actually(&s) {
        s = corrected;
    }
    s
}

/// Handle a clause-retraction marker: keep whatever follows the marker (the
/// correction), drop the retracted clause that precedes it, and preserve any
/// earlier clauses. Fires ONLY when the marker clearly begins a new clause
/// (preceded by a boundary), so it can never chop content out of plain prose.
/// Returns `None` (no change) when the pattern isn't a safe, obvious correction.
fn apply_clause_correction(text: &str, marker: &str) -> Option<String> {
    let pos = find_ci_ascii(text.as_bytes(), marker.as_bytes(), 0)?;
    let before = &text[..pos];
    let after = text[pos + marker.len()..]
        .trim_start_matches(|c: char| matches!(c, ',' | '.' | ';' | ':' | '!' | '?') || c.is_whitespace())
        .trim();
    if after.is_empty() {
        // Nothing to correct with — leave the utterance untouched.
        return None;
    }
    let before_trim = before.trim_end();
    if !before_trim.ends_with(|c: char| matches!(c, ',' | '.' | ';' | '\n')) {
        // Marker isn't clause-delimited → too ambiguous to fire safely.
        return None;
    }
    // Everything up to (and including) the boundary before the retracted clause.
    let core_before =
        before_trim.trim_end_matches(|c: char| matches!(c, ',' | '.' | ';' | '\n') || c.is_whitespace());
    let head = match core_before.rfind(|c: char| matches!(c, ',' | '.' | ';' | '\n')) {
        Some(i) => core_before[..=i].trim().to_string(),
        None => String::new(),
    };
    let combined = if head.is_empty() {
        after.to_string()
    } else {
        format!("{} {}", head, after)
    };
    if combined.trim().is_empty() {
        None
    } else {
        Some(combined)
    }
}

/// Numeric restatement via "actually": when the tokens flanking "actually" are
/// both plain numbers, replace the first with the second and drop "actually"
/// ("coffee at 2 actually 3" → "coffee at 3"). Restricting to numbers keeps it
/// safe — it never fires on the adverb ("I actually think …").
fn apply_numeric_actually(text: &str) -> Option<String> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    for i in 1..tokens.len().saturating_sub(1) {
        if token_core(tokens[i]) == "actually"
            && is_numeric_token(tokens[i - 1])
            && is_numeric_token(tokens[i + 1])
        {
            let mut out: Vec<&str> = Vec::with_capacity(tokens.len() - 2);
            out.extend_from_slice(&tokens[..i - 1]);
            out.extend_from_slice(&tokens[i + 1..]);
            return Some(out.join(" "));
        }
    }
    None
}

/// Lowercased alphanumeric core of a token, with surrounding punctuation stripped.
fn token_core(token: &str) -> String {
    token
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

/// True when a token is a plain number once surrounding punctuation is stripped.
fn is_numeric_token(token: &str) -> bool {
    let core = token.trim_matches(|c: char| !c.is_alphanumeric());
    !core.is_empty() && core.chars().all(|c| c.is_ascii_digit())
}

/// Case-insensitive ASCII substring search over bytes, returning the byte offset
/// of the first match at/after `from`. Byte-based (never builds a resized
/// lowercased string) so it's safe on UTF-8 input — an ASCII needle only ever
/// matches ASCII bytes, which are always char boundaries.
fn find_ci_ascii(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| {
        needle
            .iter()
            .enumerate()
            .all(|(j, &nb)| hay[i + j].eq_ignore_ascii_case(&nb))
    })
}

/// Case-insensitive ASCII substring replacement built on [`find_ci_ascii`]. Only
/// ASCII needles/replacements — matches land on char boundaries so slicing is safe.
fn replace_ci_ascii(haystack: &str, needle: &str, replacement: &str) -> String {
    let (hay, need) = (haystack.as_bytes(), needle.as_bytes());
    let mut out = String::with_capacity(haystack.len());
    let mut last = 0;
    while let Some(pos) = find_ci_ascii(hay, need, last) {
        out.push_str(&haystack[last..pos]);
        out.push_str(replacement);
        last = pos + need.len();
    }
    out.push_str(&haystack[last..]);
    out
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

    // --- No-speech + hallucination guards (YV16) --------------------------

    #[test]
    fn has_enough_speech_thresholds() {
        // Silence and a sub-0.35s tap are NOT enough speech to transcribe…
        assert!(!has_enough_speech(0.0));
        assert!(!has_enough_speech(0.2));
        // …a half-second of real voiced time is.
        assert!(has_enough_speech(0.5));
    }

    #[test]
    fn hallucinated_repetition_flags_degenerate_loops() {
        // The reported "WPM-SERV" spam, both spaced and glued into one token.
        assert!(is_hallucinated_repetition("WPM SERV SERV SERV SERV"));
        assert!(is_hallucinated_repetition("WPM-SERV-SERV-SERV-SERV"));
        // A classic Whisper stuck-token loop.
        assert!(is_hallucinated_repetition("the the the the the the"));
    }

    #[test]
    fn hallucinated_repetition_spares_normal_text() {
        // Ordinary prose is never flagged.
        assert!(!is_hallucinated_repetition(
            "The weather today is nice and I think we should go for a walk."
        ));
        // A real numbered list survives (repeated "Buy" is not a degenerate loop).
        assert!(!is_hallucinated_repetition(
            "1. Buy milk\n2. Buy eggs\n3. Buy bread"
        ));
        // A short spoken emphatic (<4 repeats) is legitimate speech.
        assert!(!is_hallucinated_repetition("no no no"));
    }

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

    #[test]
    fn fixed_mode_overrides_detected_mode() {
        // Slack detects as Chat via app inference…
        assert_eq!(mode_for_app("Slack"), DictationMode::Chat);
        // …but a user-picked fixed mode always wins over detection.
        assert_eq!(resolve_mode("email", "Slack"), DictationMode::Email);
        assert_eq!(resolve_mode("plain", "Gmail"), DictationMode::Plain);
        assert_eq!(resolve_mode("list", "Xcode"), DictationMode::List);
        // "auto" (and unknown values) defer back to app detection.
        assert_eq!(resolve_mode("auto", "Slack"), DictationMode::Chat);
        assert_eq!(resolve_mode("auto", "Google Docs"), DictationMode::Document);
    }

    /// Mirror of the pipeline wiring in `lib.rs`: pick the mode, then format (guarded so
    /// non-empty input can never yield empty output).
    fn pipeline_format(setting: &str, app: &str, raw: &str) -> String {
        let mode = resolve_mode(setting, app);
        if should_format(mode) {
            let formatted = format_dictation(raw);
            if formatted.trim().is_empty() && !raw.trim().is_empty() {
                raw.to_string()
            } else {
                formatted
            }
        } else {
            raw.to_string()
        }
    }

    #[test]
    fn should_format_skips_code_and_plain() {
        assert!(!should_format(DictationMode::Code));
        assert!(!should_format(DictationMode::Plain));
        assert!(should_format(DictationMode::List));
        assert!(should_format(DictationMode::Notes));
        assert!(should_format(DictationMode::Email));
    }

    // --- Backtrack v1 (YV6) ----------------------------------------------

    #[test]
    fn backtrack_removes_standalone_fillers() {
        assert_eq!(
            clean_backtrack("um so I think we should uh go home"),
            "so I think we should go home"
        );
        assert_eq!(clean_backtrack("Er, hello there"), "hello there");
        assert_eq!(clean_backtrack("that is hmm interesting"), "that is interesting");
    }

    #[test]
    fn backtrack_removes_comma_bounded_discourse_fillers() {
        // Comma-bounded hedges are dropped; the real words survive.
        assert_eq!(
            clean_backtrack("we need milk, like, and eggs"),
            "we need milk, and eggs"
        );
        assert_eq!(
            clean_backtrack("it was, you know, really good"),
            "it was, really good"
        );
        // The verb "like" (not comma-bounded) is left alone.
        assert_eq!(clean_backtrack("I like the plan"), "I like the plan");
    }

    #[test]
    fn backtrack_applies_actually_restatement() {
        // The documented Wispr example.
        assert_eq!(
            clean_backtrack("Let's do coffee at 2 actually 3"),
            "Let's do coffee at 3"
        );
        assert_eq!(clean_backtrack("coffee at 2 actually 3"), "coffee at 3");
    }

    #[test]
    fn backtrack_applies_clause_retraction_markers() {
        assert_eq!(
            clean_backtrack("meet at noon, scratch that, meet at one"),
            "meet at one"
        );
        // Earlier, non-retracted clauses are preserved.
        assert_eq!(
            clean_backtrack("I'm free today, let's do noon, scratch that, let's do one"),
            "I'm free today, let's do one"
        );
        assert_eq!(
            clean_backtrack("book the blue one, no wait, the red one"),
            "the red one"
        );
    }

    #[test]
    fn backtrack_preserves_content_and_never_empties() {
        // "actually" as an adverb is NOT a numeric restatement — text is untouched.
        let prose = "I actually think we should meet at the office tomorrow.";
        assert_eq!(clean_backtrack(prose), prose);
        // Plain prose with no triggers is returned verbatim.
        let plain = "The quarterly report is ready for your review.";
        assert_eq!(clean_backtrack(plain), plain);
        // An all-filler utterance can never be emptied — original is preserved.
        assert_eq!(clean_backtrack("um uh er"), "um uh er");
        // A dangling marker with no correction is left untouched.
        assert_eq!(clean_backtrack("meet at noon scratch that"), "meet at noon scratch that");
    }

    #[test]
    fn format_dictation_runs_backtrack_cleanup() {
        // Fillers are stripped inside the polish path, prose stays prose.
        assert_eq!(
            format_dictation("um the report is uh done"),
            "the report is done"
        );
    }

    #[test]
    fn pipeline_applies_format_dictation_and_never_loses_text() {
        let raw = "first, buy milk, second, buy eggs, third, buy bread";
        // format_dictation IS applied for detected/document-style modes.
        assert_eq!(
            pipeline_format("auto", "Obsidian", raw),
            "1. Buy milk\n2. Buy eggs\n3. Buy bread"
        );
        // A user-forced Code/Plain mode leaves the transcript verbatim.
        assert_eq!(pipeline_format("code", "Obsidian", raw), raw);
        assert_eq!(pipeline_format("plain", "Obsidian", raw), raw);
        // Guard: non-empty input never yields empty output.
        assert!(!pipeline_format("notes", "Obsidian", "hello world").is_empty());
    }

    // --- Cleanup pipeline (YV10) -----------------------------------------

    /// Identity dictionary stage (no vocabulary changes) for pipeline tests.
    fn no_dict(t: &str) -> String {
        t.to_string()
    }

    #[test]
    fn cleanup_level_parses_with_light_default() {
        assert_eq!(CleanupLevel::from_setting("none"), CleanupLevel::None);
        assert_eq!(CleanupLevel::from_setting("light"), CleanupLevel::Light);
        assert_eq!(CleanupLevel::from_setting("MEDIUM"), CleanupLevel::Medium);
        assert_eq!(CleanupLevel::from_setting("High"), CleanupLevel::High);
        // Unknown/empty values fall back to the default (Light).
        assert_eq!(CleanupLevel::from_setting(""), CleanupLevel::Light);
        assert_eq!(CleanupLevel::from_setting("bogus"), CleanupLevel::Light);
    }

    #[test]
    fn cleanup_level_none_returns_raw_unchanged() {
        // Even with a dictionary that WOULD rewrite the text and an LLM stub that
        // WOULD replace it, `None` short-circuits to a verbatim raw passthrough.
        let raw = "um first, buy milk, second, buy eggs";
        let out = run_cleanup(
            raw,
            CleanupLevel::None,
            DictationMode::Notes,
            |_t| "REWRITTEN".to_string(),
            |_t| Some("POLISHED".to_string()),
        );
        assert_eq!(out, raw);
    }

    #[test]
    fn cleanup_pipeline_runs_stages_in_order() {
        use std::cell::RefCell;
        // Record the ORDER of the two closure-backed stages (dictionary, LLM).
        // The two pure middle stages (backtrack, format) are proven to have run,
        // in position, by their visible effect on the output.
        let order = RefCell::new(Vec::<&'static str>::new());
        let dict = |t: &str| {
            order.borrow_mut().push("dictionary");
            // Vocabulary fix that only the FIRST stage could apply.
            t.replace("Drivea", "Drivia")
        };
        let llm = |t: &str| {
            order.borrow_mut().push("llm");
            Some(t.to_string())
        };
        // Raw has: a dictionary miss (Drivea), a filler (um), and list intent.
        let raw = "um first, ship Drivea, second, buy eggs";
        let out = run_cleanup(raw, CleanupLevel::High, DictationMode::Notes, dict, llm);

        // Dictionary ran before the LLM stage (stages 1 → 4 ordering).
        assert_eq!(*order.borrow(), vec!["dictionary", "llm"]);
        // Stage 1 (dictionary) applied: "Drivea" → "Drivia".
        assert!(out.contains("Drivia"), "dictionary stage output missing: {out}");
        // Stage 2 (backtrack) applied: the "um" filler is gone.
        assert!(!out.contains("um "), "backtrack stage did not run: {out}");
        // Stage 3 (format) applied: enumerated list intent became a numbered list.
        assert_eq!(out, "1. Ship Drivia\n2. Buy eggs");
    }

    #[test]
    fn cleanup_level_gates_which_stages_run() {
        let raw = "um first, buy milk, second, buy eggs";
        // Light: backtrack runs (filler removed) but NOT list formatting.
        let light = run_cleanup(raw, CleanupLevel::Light, DictationMode::Notes, no_dict, polish_llm);
        assert_eq!(light, "first, buy milk, second, buy eggs");
        // Medium: formatting also runs → numbered list.
        let medium = run_cleanup(raw, CleanupLevel::Medium, DictationMode::Notes, no_dict, polish_llm);
        assert_eq!(medium, "1. Buy milk\n2. Buy eggs");
    }

    // --- Context awareness v1 (YV50) --------------------------------------

    #[test]
    fn continuing_a_sentence_lowercases_the_lead_word() {
        // The model always capitalises the first word of a take; mid-sentence
        // that is wrong, and the context before the caret is what proves it.
        assert_eq!(lead_case_for_context(Some("we should ")), LeadCase::Lowercase);
        assert_eq!(join_with_context("Ship it on Friday", Some("we should ")), "ship it on Friday");
        assert_eq!(join_with_context("Ship it", Some("we should")), " ship it");

        // A fresh sentence after . ? ! keeps (and forces) the capital.
        for ender in ["That works.", "Does it?", "Ship it!"] {
            assert_eq!(lead_case_for_context(Some(ender)), LeadCase::Capitalize, "{ender}");
        }
        assert_eq!(join_with_context("ship it", Some("That works. ")), "Ship it");
        // An empty field is the start of a sentence too.
        assert_eq!(join_with_context("ship it", Some("")), "Ship it");
        // …and so is a new line.
        assert_eq!(join_with_context("ship it", Some("Notes:\n")), "Ship it");

        // No context (AX denied / secure field) → verbatim, never re-cased.
        assert_eq!(lead_case_for_context(None), LeadCase::Leave);
        assert_eq!(join_with_context("Ship it", None), "Ship it");
    }

    #[test]
    fn lowercasing_never_damages_i_acronyms_or_names() {
        // Capitalised for a reason: the pronoun, acronyms, CamelCase names.
        for text in ["I think so", "I'll ship it", "API keys rotate", "GitHub is down"] {
            assert_eq!(join_with_context(text, Some("she said ")), text.to_string(), "{text}");
        }
        // …but an ordinary word does fold down.
        assert_eq!(join_with_context("They shipped", Some("she said ")), "they shipped");
    }

    #[test]
    fn spacing_follows_the_character_before_the_caret() {
        // Mid-word/after a word with no trailing space → one space is added.
        assert!(needs_leading_space(Some("we should"), "ship it"));
        assert_eq!(join_with_context("Ship it", Some("we should")), " ship it");
        // The caret already sits after whitespace → no double space.
        assert!(!needs_leading_space(Some("we should "), "ship it"));
        assert!(!needs_leading_space(Some("Notes:\n"), "ship it"));
        // Empty field and no context → never a stray leading space.
        assert!(!needs_leading_space(Some(""), "ship it"));
        assert!(!needs_leading_space(None, "ship it"));
        // Openers and joiners own the gap after them.
        for ctx in ["(", "\"", "wilson@", "https://"] {
            assert!(!needs_leading_space(Some(ctx), "ship it"), "{ctx}");
        }
        // Punctuation on the dictated side hugs the previous word.
        assert_eq!(join_with_context(", and then some", Some("done")), ", and then some");
    }

    #[test]
    fn email_context_hints_the_mode_without_overriding_app_or_setting() {
        let greeting = "Hi Sarah,\n\nthanks for sending the deck over. We are";
        assert_eq!(mode_hint_from_context(Some(greeting)), Some(DictationMode::Email));
        assert_eq!(
            mode_hint_from_context(Some("Subject: Q3 pricing\n\n")),
            Some(DictationMode::Email)
        );
        assert_eq!(
            mode_hint_from_context(Some("Best regards,\nWilson")),
            Some(DictationMode::Email)
        );
        // Ordinary prose (and no context at all) hints nothing.
        assert_eq!(mode_hint_from_context(Some("hi there is a bug in the parser")), None);
        assert_eq!(mode_hint_from_context(None), None);

        // The hint only fills the gap an unrecognised app leaves.
        assert_eq!(
            resolve_mode_with_context("auto", "Arc", Some(greeting)),
            DictationMode::Email
        );
        // A recognised app still wins over the hint…
        assert_eq!(
            resolve_mode_with_context("auto", "Visual Studio Code", Some(greeting)),
            DictationMode::Code
        );
        // …and a user-picked mode wins over everything.
        assert_eq!(
            resolve_mode_with_context("plain", "Arc", Some(greeting)),
            DictationMode::Plain
        );
        // No context = exactly the pre-YV50 behaviour.
        assert_eq!(
            resolve_mode_with_context("auto", "Arc", None),
            resolve_mode("auto", "Arc")
        );
    }

    #[test]
    fn context_join_never_loses_text() {
        for (text, ctx) in [
            ("Ship it", Some("we should")),
            ("ship it", Some("")),
            ("Ship it", None),
            ("🚀 ship", Some("go ")),
            ("", Some("we should")),
        ] {
            // Only the lead capital and a joining space may ever differ —
            // no word is added, reordered or dropped.
            let out = join_with_context(text, ctx);
            assert_eq!(
                out.trim().to_lowercase(),
                text.trim().to_lowercase(),
                "{text:?} → {out:?}"
            );
        }
    }

    #[test]
    fn llm_polish_stub_is_a_guarded_noop() {
        // The stub never rewrites text today.
        assert_eq!(polish_llm("hello world"), None);
        // A failing/empty LLM result must never lose text: High level with a stub
        // that returns None keeps the pre-LLM (formatted) result.
        let raw = "first, buy milk, second, buy eggs";
        let out = run_cleanup(raw, CleanupLevel::High, DictationMode::Notes, no_dict, polish_llm);
        assert_eq!(out, "1. Buy milk\n2. Buy eggs");
        // An LLM stage that erroneously returns empty is ignored (guarded).
        let out2 = run_cleanup(
            raw,
            CleanupLevel::High,
            DictationMode::Notes,
            no_dict,
            |_t| Some("   ".to_string()),
        );
        assert_eq!(out2, "1. Buy milk\n2. Buy eggs");
    }
}
