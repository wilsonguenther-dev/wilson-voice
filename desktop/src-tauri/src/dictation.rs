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
///   3. rules formatting — `format_dictation` (list detection), respecting `mode`,
///      then [`apply_spoken_marks`] (punctuation-by-name + line/paragraph commands).
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
    // Stage 3 — rules formatting. `format_dictation` also runs a backtrack pass
    // internally, so at Medium/High the standalone Stage 2 above is idempotent.
    if level.runs_format() {
        if should_format(mode) {
            let out = format_dictation(&text);
            if !out.trim().is_empty() {
                text = out;
            }
        }
        // Spoken marks (R1/R2) run LAST of the rules: they are the only stage that
        // introduces line breaks, and the ones before it normalise whitespace.
        // `apply_spoken_marks` carries its own mode gate (inert in `Code`), so
        // punctuation dictated by name still lands in `Plain`.
        let out = apply_spoken_marks(&text, mode);
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
// Undo AI edit (YV51) — re-paste the raw take over the polished one.
// ---------------------------------------------------------------------------

/// Pick the text for "Undo AI edit": the raw ASR transcript of a take, but ONLY
/// when it actually differs from the polished text that was pasted.
///
/// Returns `None` — i.e. the action is inert and must be disabled — when:
///   * the row stores no raw at all (legacy rows written before the column), or
///   * the raw is blank, so re-pasting it would replace the user's text with
///     nothing (the pipeline's "never lose text" rule applies here too), or
///   * raw and polished match once trimmed — Auto-Cleanup was `None`, or every
///     enabled stage was a no-op, so there is no AI edit to undo.
///
/// The comparison is on the TRIMMED strings on purpose: the only difference the
/// context join (YV50) can leave on its own is a leading space, and re-pasting a
/// whole take just to drop one space is not an "undo".
pub fn undo_ai_edit_text<'a>(polished: &str, raw: Option<&'a str>) -> Option<&'a str> {
    let raw = raw?;
    if raw.trim().is_empty() || raw.trim() == polished.trim() {
        return None;
    }
    Some(raw)
}

// ---------------------------------------------------------------------------
// Backtrack v2 (YV58) — rule-based filler + self-correction cleanup (on-device).
//
// A conservative, PURE first pass that mirrors Wispr Flow's "Backtrack" for the
// obvious, low-risk cases while a local-LLM pass (P0 backlog) does the rest. It
// (a) drops standalone filler tokens, (b) applies spoken self-corrections via a
// small set of unambiguous trigger phrases, and (c) NEVER empties the string —
// if a rule would leave nothing, the original text is returned unchanged. Kept
// intentionally narrow: it only fires where intent is clear, never rewriting
// meaning or touching ordinary prose.
//
// v2 widens the closed vocabulary without widening the risk: markers are
// order-insensitive ("wait no" as well as "no wait") and a one-word correction of
// the SAME category swaps that word instead of dropping the clause ("Tuesday,
// wait no, Friday" → "… Friday"); the hedge list grows to `like`, `you know`,
// `sort of`, `kind of`, `i guess`, `basically`, `literally`, each removed only
// where it is a discourse particle and never inside a quoted span.
// ---------------------------------------------------------------------------

/// Standalone, meaning-free filler tokens that are always safe to drop.
const FILLER_TOKENS: &[&str] = &["um", "umm", "uh", "uhh", "uhm", "er", "erm", "hmm"];

/// Discourse particles (YV58) — hedges that carry no meaning when they are used as
/// PARTICLES and full content words otherwise ("I like the plan", "sort of blue").
/// [`is_discourse_particle`] decides which occurrence is which; the word alone is
/// never enough. "i mean" is NOT here: it retracts a clause, so it lives in
/// [`CORRECTION_MARKERS`].
const DISCOURSE_PARTICLES: &[&str] = &[
    "like",
    "you know",
    "sort of",
    "kind of",
    "i guess",
    "basically",
    "literally",
];

/// Clause-retraction markers (YV58) — order-insensitive: both "wait no" and its
/// reversal "no wait" retract. Listed longest-first so a specific phrase is tried
/// before a shorter one it contains.
const CORRECTION_MARKERS: &[&str] = &[
    "let me rephrase",
    "sorry i meant",
    "scratch that",
    "wait no",
    "no wait",
    "i mean",
];

/// Weekday names — one of the categories a one-word restatement can swap: in
/// "Tuesday, wait no, Friday" the day is replaced, the clause is not dropped.
const WEEKDAYS: &[&str] = &[
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

/// Month names — the second swappable category. "may" and "march" are left out on
/// purpose: as a modal and a verb they are ordinary prose far more often than they
/// are dates, and a wrong swap rewrites the sentence ("we may meet" → "we June meet").
const MONTHS: &[&str] = &[
    "january",
    "february",
    "april",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

/// The kind of thing a token names. Two tokens of the SAME category are
/// interchangeable, which is what makes a one-word restatement a swap rather than
/// a clause retraction. Deliberately tiny: a wrong swap silently changes meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenCategory {
    Weekday,
    Month,
    Number,
}

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

/// Remove filler words: discourse particles first, then standalone pure-filler
/// tokens ("um", "uh", …) anywhere in the utterance.
fn remove_fillers(text: &str) -> String {
    remove_discourse_particles(text)
        .split_whitespace()
        .filter(|tok| !is_pure_filler(tok))
        .collect::<Vec<_>>()
        .join(" ")
}

/// True when a whitespace token is a standalone pure filler ("um", "uh,", "Er.").
fn is_pure_filler(token: &str) -> bool {
    FILLER_TOKENS.contains(&token_core(token).as_str())
}

/// Drop every occurrence of a [`DISCOURSE_PARTICLES`] phrase that
/// [`is_discourse_particle`] accepts. One removal shifts every later offset (and
/// can expose the next particle: ", like, you know,"), so the scan restarts after
/// each edit; it always terminates because the string strictly shrinks.
fn remove_discourse_particles(text: &str) -> String {
    let mut s = text.to_string();
    'restart: loop {
        for particle in DISCOURSE_PARTICLES {
            let mut from = 0;
            while let Some(pos) = find_ci_ascii(s.as_bytes(), particle.as_bytes(), from) {
                let end = pos + particle.len();
                if is_discourse_particle(&s, pos, end) {
                    s = strip_particle(&s, pos, end);
                    continue 'restart;
                }
                from = pos + 1;
            }
        }
        break;
    }
    s
}

/// Whether the occurrence at `start..end` is a discourse particle — i.e. safe to
/// delete — rather than a content word. True ONLY when it is a whole word that is
/// either comma-bounded (", like," — the spoken form of a parenthetical hedge) or
/// sentence-initial with a following comma ("Basically, we should ship"), and never
/// inside a quoted span, where the speaker is reporting words rather than hedging.
fn is_discourse_particle(text: &str, start: usize, end: usize) -> bool {
    // Whole-word only: "like" but not "likely", "unlike".
    if text[..start].chars().next_back().is_some_and(char::is_alphanumeric)
        || text[end..].chars().next().is_some_and(char::is_alphanumeric)
    {
        return false;
    }
    if is_inside_quotes(text, start) {
        return false;
    }
    let before = text[..start].trim_end();
    let after = text[end..].trim_start();
    // The hedge always closes with a comma; what precedes it decides which shape
    // this is — a mid-sentence parenthetical or the opener of a sentence.
    if !after.starts_with(',') {
        return false;
    }
    before.ends_with(',')
        || before.is_empty()
        || before.ends_with(SENTENCE_ENDERS)
        || before.ends_with('\n')
}

/// Delete the particle at `start..end` plus the comma that closed it, re-joining
/// the two sides. When the particle opened a sentence its capitalization moves to
/// the word that now starts it ("Basically, we ship" → "We ship").
fn strip_particle(text: &str, start: usize, end: usize) -> String {
    let head = text[..start].trim_end();
    let mut tail = text[end..].trim_start();
    if let Some(rest) = tail.strip_prefix(',') {
        tail = rest.trim_start();
    }
    let opened_sentence =
        head.is_empty() || head.ends_with(SENTENCE_ENDERS) || head.ends_with('\n');
    let particle_was_capitalized = text[start..end].starts_with(char::is_uppercase);
    let tail = if opened_sentence && particle_was_capitalized {
        capitalize(tail)
    } else {
        tail.to_string()
    };
    match (head.is_empty(), tail.is_empty()) {
        (true, _) => tail,
        (false, true) => head.to_string(),
        (false, false) => format!("{head} {tail}"),
    }
}

/// True when the byte offset `at` sits inside a quoted span. Straight quotes toggle;
/// curly quotes open and close explicitly.
fn is_inside_quotes(text: &str, at: usize) -> bool {
    let mut open = false;
    for c in text[..at].chars() {
        match c {
            '"' => open = !open,
            '\u{201c}' => open = true,
            '\u{201d}' => open = false,
            _ => {}
        }
    }
    open
}

/// Apply the unambiguous self-correction triggers: every [`CORRECTION_MARKERS`]
/// phrase retracts what precedes it (or swaps a single same-category word), and an
/// "actually" restatement ("coffee at 2 actually 3" → "coffee at 3") swaps the value.
fn apply_self_correction(text: &str) -> String {
    let mut s = text.to_string();
    for marker in CORRECTION_MARKERS {
        if let Some(corrected) = apply_clause_correction(&s, marker) {
            s = corrected;
        }
    }
    if let Some(corrected) = apply_actually_restatement(&s) {
        s = corrected;
    }
    s
}

/// Handle a clause-retraction marker: keep whatever follows the marker (the
/// correction), drop the retracted clause that precedes it, and preserve any
/// earlier clauses. Fires ONLY when the marker clearly begins a new clause
/// (preceded by a boundary), so it can never chop content out of plain prose.
/// A one-word correction that names the same KIND of thing as a word in the
/// retracted clause is spliced in place instead ("Tuesday, wait no, Friday").
/// Returns `None` (no change) when the pattern isn't a safe, obvious correction.
fn apply_clause_correction(text: &str, marker: &str) -> Option<String> {
    let pos = find_marker(text, marker)?;
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
    // Same-category restatement first: the speaker replaced ONE word, not the
    // clause, so keep the clause and swap that word.
    if let Some(spliced) = splice_same_category(core_before, after) {
        return Some(spliced);
    }
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

/// Byte offset of the first WHOLE-WORD occurrence of `marker`, so "i mean" never
/// matches inside "sorry i meant" and "dash" never matches inside "dashboard".
fn find_marker(text: &str, marker: &str) -> Option<usize> {
    let mut from = 0;
    loop {
        let pos = find_ci_ascii(text.as_bytes(), marker.as_bytes(), from)?;
        let end = pos + marker.len();
        let bounded = !text[..pos].chars().next_back().is_some_and(char::is_alphanumeric)
            && !text[end..].chars().next().is_some_and(char::is_alphanumeric);
        if bounded {
            return Some(pos);
        }
        from = pos + 1;
    }
}

/// Splice a ONE-WORD correction into the retracted clause when it names the same
/// category as a word already in it ("let's meet Tuesday" + "Friday" → "let's meet
/// Friday"). Anything longer is a restatement of the whole clause, which the caller
/// handles by retraction — so this stays a swap of a single, interchangeable word.
fn splice_same_category(retracted: &str, correction: &str) -> Option<String> {
    let mut words = correction.split_whitespace();
    let word = words.next()?;
    if words.next().is_some() {
        return None;
    }
    let category = token_category(word)?;
    let mut tokens: Vec<&str> = retracted.split_whitespace().collect();
    let target = tokens.iter().rposition(|t| token_category(t) == Some(category))?;
    tokens[target] = word;
    let out = tokens.join(" ");
    (!out.trim().is_empty()).then_some(out)
}

/// The category a token belongs to, if it is one of the few kinds of word a spoken
/// restatement can swap outright. Everything else is `None` — no category, no swap.
fn token_category(token: &str) -> Option<TokenCategory> {
    let core = token_core(token);
    if core.is_empty() {
        return None;
    }
    if WEEKDAYS.contains(&core.as_str()) {
        return Some(TokenCategory::Weekday);
    }
    if MONTHS.contains(&core.as_str()) {
        return Some(TokenCategory::Month);
    }
    if is_numeric_token(token) {
        return Some(TokenCategory::Number);
    }
    None
}

/// Restatement via "actually": when the tokens flanking "actually" name the same
/// category (two numbers, two weekdays, …), replace the first with the second and
/// drop "actually" ("coffee at 2 actually 3" → "coffee at 3"). Requiring a shared
/// category keeps it safe — it never fires on the adverb ("I actually enjoyed …").
fn apply_actually_restatement(text: &str) -> Option<String> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    for i in 1..tokens.len().saturating_sub(1) {
        let category = token_category(tokens[i - 1]);
        if token_core(tokens[i]) == "actually"
            && category.is_some()
            && category == token_category(tokens[i + 1])
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

// ---------------------------------------------------------------------------
// Spoken marks (YV58) — rules R1/R2 of
// `docs/research/wispr-formatting-deep-dive.md`.
//
// Punctuation dictated BY NAME becomes the glyph and the words disappear
// ("see you exclamation point" → "see you!"), and the line/paragraph commands
// become real breaks ("new line" → `\n`, "new paragraph" → `\n\n`).
//
// Everything here is deterministic and pure. Three guards keep it off ordinary
// prose: the name must be a standalone token, it must not follow a determiner
// (that is "the Jurassic period", not a command), and a terminal mark is dropped
// rather than doubled when the text already ends in one. `Code` mode is inert —
// identifiers and string literals are full of these words.
//
// The pass runs per LINE, so breaks it (or the list renderer) inserted survive.
// ---------------------------------------------------------------------------

/// How a glyph attaches to the words around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkKind {
    /// Attaches to the word BEFORE it, space after: `you!`, `7.`, `50%`.
    Trailing,
    /// Attaches to the word AFTER it, space before: `(note`, `#ship`.
    Leading,
    /// No space on either side: `R&D`, `and/or`, `wilson@example.com`.
    Tight,
    /// Alternates leading → trailing so a pair wraps the words between them.
    Quote,
    /// A line break (R2) — `\n` or `\n\n`.
    Break,
}

/// Spoken punctuation names → glyph (R1), plus the line/paragraph commands (R2).
/// The set is Wispr's documented one; matching is greedy over the token stream, so
/// a name never has to be listed shorter-first.
const SPOKEN_MARKS: &[(&str, &str, MarkKind)] = &[
    ("start a new paragraph", "\n\n", MarkKind::Break),
    ("new paragraph", "\n\n", MarkKind::Break),
    ("skip a line", "\n\n", MarkKind::Break),
    ("new line", "\n", MarkKind::Break),
    ("period", ".", MarkKind::Trailing),
    ("comma", ",", MarkKind::Trailing),
    ("question mark", "?", MarkKind::Trailing),
    ("exclamation point", "!", MarkKind::Trailing),
    ("exclamation mark", "!", MarkKind::Trailing),
    ("colon", ":", MarkKind::Trailing),
    ("semicolon", ";", MarkKind::Trailing),
    ("ellipsis", "\u{2026}", MarkKind::Trailing),
    ("percent sign", "%", MarkKind::Trailing),
    ("degree symbol", "\u{b0}", MarkKind::Trailing),
    ("quotation mark", "\"", MarkKind::Quote),
    ("apostrophe", "'", MarkKind::Tight),
    ("dash", "-", MarkKind::Tight),
    ("em dash", "\u{2014}", MarkKind::Tight),
    ("asterisk", "*", MarkKind::Tight),
    ("ampersand", "&", MarkKind::Tight),
    ("slash", "/", MarkKind::Tight),
    ("forward slash", "/", MarkKind::Tight),
    ("backslash", "\\", MarkKind::Tight),
    ("underscore", "_", MarkKind::Tight),
    ("tilde", "~", MarkKind::Tight),
    ("plus sign", "+", MarkKind::Tight),
    ("minus sign", "-", MarkKind::Tight),
    ("equals sign", "=", MarkKind::Tight),
    ("at symbol", "@", MarkKind::Tight),
    ("at sign", "@", MarkKind::Tight),
    ("hashtag", "#", MarkKind::Leading),
    ("open parenthesis", "(", MarkKind::Leading),
    ("open paren", "(", MarkKind::Leading),
    ("close parenthesis", ")", MarkKind::Trailing),
    ("close paren", ")", MarkKind::Trailing),
    ("open angle bracket", "<", MarkKind::Leading),
    ("close angle bracket", ">", MarkKind::Trailing),
];

/// Longest name in [`SPOKEN_MARKS`], in whitespace tokens ("start a new paragraph").
const MAX_MARK_TOKENS: usize = 4;

/// Words that turn a following mark name back into content: a determiner makes it
/// the NOUN the speaker is talking about ("a DASH of salt", "the NEW LINE of
/// products"), and "to" makes it the VERB ("I need to DASH to the store"). Nobody
/// dictates a punctuation command straight after one, so this gates every name.
const MARK_BLOCKING_LEAD_INS: &[&str] = &[
    "a", "an", "the", "this", "that", "these", "those", "each", "every", "another", "my", "your",
    "our", "their", "his", "her", "its", "no", "to",
];

/// A mark name followed by "of" heads a noun phrase ("PERIOD of time", "DASH of
/// salt") — the one word that never follows a dictated punctuation command.
const MARK_BLOCKING_FOLLOWER: &str = "of";

/// Apply spoken punctuation names (R1) and line/paragraph commands (R2).
///
/// Pure, and inert in `Code` mode — dictated identifiers really do contain
/// "underscore" and "slash" as words. Guarded like every other stage: a non-empty
/// transcript can never come back empty.
pub fn apply_spoken_marks(text: &str, mode: DictationMode) -> String {
    if mode == DictationMode::Code {
        return text.to_string();
    }
    let out = text
        .split('\n')
        .map(apply_marks_to_line)
        .collect::<Vec<_>>()
        .join("\n");
    if out.trim().is_empty() && !text.trim().is_empty() {
        return text.to_string();
    }
    out
}

/// One line of [`apply_spoken_marks`]: walk the tokens, emitting glyphs for the
/// mark names and the words verbatim for everything else.
fn apply_marks_to_line(line: &str) -> String {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let mut out = String::with_capacity(line.len());
    let mut space_before_next = false;
    let mut quote_open = true;
    let mut i = 0;
    while i < tokens.len() {
        if let Some((len, glyph, kind)) = match_spoken_mark(&tokens, i) {
            if emit_mark(&mut out, glyph, kind, &mut quote_open, &mut space_before_next) {
                i += len;
                continue;
            }
        }
        if space_before_next {
            out.push(' ');
        }
        out.push_str(tokens[i]);
        space_before_next = true;
        i += 1;
    }
    out
}

/// Longest mark name starting at token `i`, as `(token count, glyph, kind)`.
/// `None` when the tokens are ordinary words — including a real name that a
/// determiner turned back into content.
fn match_spoken_mark(tokens: &[&str], i: usize) -> Option<(usize, &'static str, MarkKind)> {
    for len in (1..=MAX_MARK_TOKENS.min(tokens.len() - i)).rev() {
        let key = tokens[i..i + len]
            .iter()
            .map(|t| mark_token_key(t))
            .collect::<Vec<_>>()
            .join(" ");
        let Some(&(_, glyph, kind)) = SPOKEN_MARKS.iter().find(|(name, _, _)| *name == key) else {
            continue;
        };
        let blocked_before =
            i > 0 && MARK_BLOCKING_LEAD_INS.contains(&mark_token_key(tokens[i - 1]).as_str());
        let blocked_after = tokens
            .get(i + len)
            .is_some_and(|t| mark_token_key(t) == MARK_BLOCKING_FOLLOWER);
        if blocked_before || blocked_after {
            return None;
        }
        return Some((len, glyph, kind));
    }
    None
}

/// Comparable form of a token for mark matching: lowercased, outer punctuation
/// stripped, an inner hyphen normalised to a space ("Em-Dash." → "em dash").
fn mark_token_key(token: &str) -> String {
    token_core(token).replace('-', " ")
}

/// Write one mark into `out`. Returns `false` when it cannot be applied — nothing
/// precedes a trailing mark, or a break would open the line — and the caller then
/// keeps the spoken words as literal text rather than losing them.
fn emit_mark(
    out: &mut String,
    glyph: &str,
    kind: MarkKind,
    quote_open: &mut bool,
    space_before_next: &mut bool,
) -> bool {
    let kind = match kind {
        MarkKind::Quote => {
            let resolved = if *quote_open { MarkKind::Leading } else { MarkKind::Trailing };
            *quote_open = !*quote_open;
            resolved
        }
        other => other,
    };
    match kind {
        MarkKind::Break => {
            while out.ends_with(' ') {
                out.pop();
            }
            if out.is_empty() {
                return false;
            }
            // Consecutive breaks widen the gap instead of stacking blank lines.
            let existing = out.chars().rev().take_while(|&c| c == '\n').count();
            for _ in existing..glyph.len() {
                out.push('\n');
            }
            *space_before_next = false;
        }
        MarkKind::Leading => {
            if *space_before_next && !out.is_empty() {
                out.push(' ');
            }
            out.push_str(glyph);
            *space_before_next = false;
        }
        MarkKind::Trailing | MarkKind::Tight => {
            if out.is_empty() || out.ends_with('\n') {
                // Nothing to attach to — treat the words as content.
                return false;
            }
            // Never double a mark: "see you exclamation point period" ends "you!".
            let doubled = out.ends_with(glyph)
                || (SENTENCE_ENDERS.iter().any(|c| glyph.ends_with(*c))
                    && out.ends_with(SENTENCE_ENDERS));
            if !doubled {
                out.push_str(glyph);
            }
            *space_before_next = kind == MarkKind::Trailing;
        }
        MarkKind::Quote => unreachable!("Quote is resolved to Leading/Trailing above"),
    }
    true
}

// ---------------------------------------------------------------------------
// List formatting v2 (YV57) — rules R7–R11 of
// `docs/research/wispr-formatting-deep-dive.md`.
//
// v1 split the utterance on punctuation and only inspected the FIRST word of each
// clause, so it fired on NONE of Wispr's documented examples: real ASR output has
// no commas (one clause ⇒ instant bail) and the cue table had no numeric words.
//
// v2 scans the TOKEN stream instead (R8) and takes its safety from a monotone cue
// sequence — "one" is only a cue when a later "two" resolves — which is what keeps
// "I want one coffee and a bagel" prose. Text BEFORE the first cue is preserved as
// a lead-in line (R9); v1 silently dropped it, which would have been a text-loss
// bug the moment detection started working. Everything here stays pure, and mode
// gating stays with `should_format` (R11): `Code`/`Plain` never reach this code.
// ---------------------------------------------------------------------------

/// Spoken cardinals that can open a list item ("… are one finish the report two …"),
/// paired with their position in the enumeration (R7).
const NUMERIC_CUES: &[(&str, usize)] = &[
    ("one", 1),
    ("two", 2),
    ("three", 3),
    ("four", 4),
    ("five", 5),
    ("six", 6),
    ("seven", 7),
    ("eight", 8),
    ("nine", 9),
    ("ten", 10),
];

/// Ordinal cues — v1's table, completed through "tenth" (R7).
const ORDINAL_CUES: &[(&str, usize)] = &[
    ("first", 1),
    ("firstly", 1),
    ("second", 2),
    ("secondly", 2),
    ("third", 3),
    ("thirdly", 3),
    ("fourth", 4),
    ("fourthly", 4),
    ("fifth", 5),
    ("sixth", 6),
    ("seventh", 7),
    ("eighth", 8),
    ("ninth", 9),
    ("tenth", 10),
];

/// Cues that continue a sequence without naming their own position. They may only
/// EXTEND a chain that already resolved 1 and 2 — on their own they are far too
/// common in prose ("… and finally we shipped it").
const CONTINUATION_CUES: &[&str] = &["next", "finally", "lastly"];

/// Non-sequential enumeration cues (R10). Nothing about them is monotone, so they
/// only fire at a clause boundary and only when at least three of them line up.
const ENUM_CUES: &[&str] = &["also", "plus", "and then", "another thing"];

/// Function words that can never OPEN a list item. "one of the things I noticed …
/// two of the servers were down" satisfies the monotone rule by accident; the token
/// right after the cue is the cheapest place to tell a quantity phrase from an
/// enumeration. Deliberately excludes articles ("first, the report goes out" is a
/// real item) — only words that cannot begin a dictated item at all.
const NON_ITEM_OPENERS: &[&str] = &[
    "of", "or", "and", "but", "that", "which", "who", "whom", "than", "as", "because", "if",
    "though", "although", "while", "whether",
];

/// Conjunctions that can never CLOSE a list item. An item trailing off in "and"/"or"
/// means the clause kept going, which is the tell of counted quantities rather than
/// a dictated enumeration: "one dog two cats AND three birds", "number one in the
/// league AND number two in scoring". Real items end on their own content.
const DANGLING_CONJUNCTIONS: &[&str] = &["and", "or"];

/// Nouns that ANNOUNCE an enumeration when they appear in the lead-in ("my top
/// GOALS are one … two …", "grocery LIST one … two …"). Positive evidence that the
/// speaker set up a list, which is what separates listing from counting.
const LIST_ANNOUNCING_NOUNS: &[&str] = &[
    "list", "lists", "checklist", "things", "goals", "steps", "reasons", "items", "tasks",
    "priorities", "options", "ideas", "plan", "plans", "agenda", "questions", "topics",
];

/// Present-tense listing verbs. Directly ahead of the first cue they are the spoken
/// form of a colon: "the plan IS one … two …", "my goals ARE one … two …". Past
/// tense is narration ("he WAS number one in the league"), so it is not here.
const LIST_ANNOUNCING_VERBS: &[&str] = &["is", "are", "include", "includes"];

/// Words that turn a bullet noun into a shape instruction: "write it IN bullets",
/// "USE bullet points". Without one, a bare "bullets" is a thing being talked about.
const BULLET_INSTRUCTION_MARKERS: &[&str] =
    &["in", "as", "use", "using", "with", "into", "make", "format", "formatted"];

/// Determiners that make "bullet points" a referring noun phrase — something the
/// speaker is talking ABOUT ("THE bullet points are one … two …"), never shape.
const BULLET_DETERMINERS: &[&str] =
    &["the", "these", "those", "my", "your", "our", "their", "his", "her", "its"];

/// One enumeration cue found in the token stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cue {
    /// Index of the cue's first token.
    start: usize,
    /// How many whitespace tokens the cue spans ("number one" spans 2).
    len: usize,
    /// Position in the enumeration; `None` for a continuation cue.
    value: Option<usize>,
}

/// If the text is clearly an enumerated list, render it (lead-in line + items).
/// Otherwise `None` so the caller keeps it as prose.
fn detect_and_format_list(text: &str) -> Option<String> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.len() < 4 {
        return None;
    }

    // Path 1 — a monotone numeric/ordinal chain (R7/R8): numbered by default.
    if let Some(chain) = resolve_chain(&scan_cues(&tokens)) {
        let mut items: Vec<&[&str]> = Vec::with_capacity(chain.len());
        for (i, cue) in chain.iter().enumerate() {
            let end = chain.get(i + 1).map_or(tokens.len(), |next| next.start);
            items.push(&tokens[cue.start + cue.len..end]);
        }
        // Two-item lists are the easiest to hallucinate out of prose ("one coffee
        // and two bagels"), so they additionally need real content per item.
        if items.len() == 2 && items.iter().any(|it| it.len() < 2) {
            return None;
        }
        // A cue that opens no item is a quantity, not an enumerator ("one OF the
        // things …"), so the whole chain is a misread.
        if items
            .iter()
            .any(|it| it.first().is_some_and(|t| NON_ITEM_OPENERS.contains(&token_core(t).as_str())))
        {
            return None;
        }
        // An item that trails off in a conjunction is a clause still running, not an
        // item that ended — quantity prose ("I have one dog two cats AND three
        // birds") rather than an enumeration.
        if items.iter().any(|it| {
            it.last().is_some_and(|t| DANGLING_CONJUNCTIONS.contains(&token_core(t).as_str()))
        }) {
            return None;
        }
        // An explicit "bullet point(s)" instruction picks the marker. It is shape,
        // not content, so it is only honored — and only consumed — as the tail of
        // the LEAD-IN, ahead of the first cue. Everywhere else, including earlier in
        // the lead-in, those words are ordinary content and must survive.
        let head = &tokens[..chain[0].start];
        let (bulleted, lead_in) = take_bullet_cue(head);
        // A monotone chain is necessary but nowhere near sufficient: ordinary
        // counting prose ("I ate one apple two oranges three bananas") satisfies it
        // too. The speaker has to have marked the enumeration somehow.
        if !has_enumeration_evidence(&tokens, &chain, head, bulleted) {
            return None;
        }
        return render_list(&lead_in, &items, bulleted);
    }

    // Path 2 — three or more clause-boundary enumeration cues (R10): bulleted.
    let enums = scan_enum_cues(&tokens);
    if enums.len() >= 3 {
        let mut items: Vec<&[&str]> = vec![&tokens[..enums[0].start]];
        for (i, cue) in enums.iter().enumerate() {
            let end = enums.get(i + 1).map_or(tokens.len(), |next| next.start);
            items.push(&tokens[cue.start + cue.len..end]);
        }
        if items.iter().any(|it| it.len() < 2) {
            return None;
        }
        return render_list(&[], &items, true);
    }

    None
}

/// Scan the token stream for numeric/ordinal/digit/continuation cues (R8). A cue
/// must be a whole token — never a substring — so "someone" and "seventh-inning"
/// are inert.
fn scan_cues(tokens: &[&str]) -> Vec<Cue> {
    let mut cues = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let core = token_core(tokens[i]);
        // "number one", "number two" … consume both tokens so the cardinal inside
        // can't also register as a cue of its own.
        if core == "number" {
            if let Some(value) = tokens.get(i + 1).and_then(|t| cue_value(&token_core(t))) {
                cues.push(Cue { start: i, len: 2, value: Some(value) });
                i += 2;
                continue;
            }
        }
        if let Some(value) = digit_cue_value(tokens[i]).or_else(|| cue_value(&core)) {
            cues.push(Cue { start: i, len: 1, value: Some(value) });
        } else if CONTINUATION_CUES.contains(&core.as_str()) {
            cues.push(Cue { start: i, len: 1, value: None });
        }
        i += 1;
    }
    cues
}

/// Scan for non-sequential enumeration cues (R10). Boundary-gated: with no monotone
/// sequence to lean on, only a preceding `,` `.` `;` `:` (or the utterance start)
/// separates "also" the enumerator from "also" the adverb.
fn scan_enum_cues(tokens: &[&str]) -> Vec<Cue> {
    let mut cues = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if at_clause_boundary(tokens, i) {
            if let Some(len) = enum_cue_len(&tokens[i..]) {
                cues.push(Cue { start: i, len, value: None });
                i += len;
                continue;
            }
        }
        i += 1;
    }
    cues
}

/// Resolve the longest monotone cue chain: an anchor at position 1, then each next
/// position in turn (R8). Continuation cues only extend a chain that already holds
/// 1 and 2. Returns `None` unless at least two cues resolve.
fn resolve_chain(cues: &[Cue]) -> Option<Vec<Cue>> {
    for (i, anchor) in cues.iter().enumerate() {
        if anchor.value != Some(1) {
            continue;
        }
        let mut chain = vec![*anchor];
        let mut want = 2;
        for cue in &cues[i + 1..] {
            match cue.value {
                Some(v) if v == want => {
                    chain.push(*cue);
                    want += 1;
                }
                None if chain.len() >= 2 => chain.push(*cue),
                _ => {}
            }
        }
        if chain.len() >= 2 {
            return Some(chain);
        }
    }
    None
}

/// Render the lead-in (R9) and the items (R10). `None` when any item is empty —
/// an empty item means we misread prose, so prose is what the caller keeps.
fn render_list(lead_in: &[&str], items: &[&[&str]], bulleted: bool) -> Option<String> {
    let items: Vec<String> = items.iter().map(|it| clean_item(&it.join(" "))).collect();
    if items.iter().any(|item| item.is_empty()) {
        return None;
    }
    let mut out = clean_lead_in(&lead_in.join(" "));
    for (i, item) in items.iter().enumerate() {
        if i > 0 || !out.is_empty() {
            out.push('\n');
        }
        if bulleted {
            out.push_str("- ");
        } else {
            out.push_str(&format!("{}. ", i + 1));
        }
        out.push_str(item);
    }
    Some(out)
}

/// Lead-in line (R9): the text before the first cue, capitalized and given a `:`
/// when it carries no terminal punctuation of its own. Empty ⇒ no lead-in line.
fn clean_lead_in(text: &str) -> String {
    let trimmed = text
        .trim()
        .trim_end_matches(|c: char| matches!(c, ',' | ';' | ':' | '-') || c.is_whitespace());
    if trimmed.is_empty() {
        return String::new();
    }
    let mut lead = capitalize(trimmed);
    if !lead.ends_with(|c: char| matches!(c, '.' | '!' | '?')) {
        lead.push(':');
    }
    lead
}

/// Item text (R10): separators trimmed, capitalized, and stripped of a trailing
/// period unless the item is more than one sentence.
fn clean_item(text: &str) -> String {
    let mut item = text
        .trim()
        .trim_start_matches(|c: char| matches!(c, ',' | ';' | ':' | '.') || c.is_whitespace())
        .trim_end_matches(|c: char| matches!(c, ',' | ';' | ':') || c.is_whitespace())
        .to_string();
    if let Some(head) = item.strip_suffix('.') {
        if !head.contains(|c: char| matches!(c, '.' | '!' | '?')) {
            item = head.trim_end().to_string();
        }
    }
    capitalize(item.trim())
}

/// Positive evidence that a resolved chain is an enumeration and not counted
/// quantities. A bare cardinal is how an ASR renders EVERY spoken number, so a chain
/// of them proves nothing on its own — the speaker has to have marked the list:
/// an explicit shape instruction, a lead-in that announces one, cues sitting on
/// clause boundaries, or a cue form that is itself an enumerator. Ordinals and
/// dictated digit markers are that last case: nobody counts apples in "firsts".
fn has_enumeration_evidence(
    tokens: &[&str],
    chain: &[Cue],
    lead_in: &[&str],
    bulleted: bool,
) -> bool {
    bulleted
        || chain.first().is_some_and(|cue| is_enumerative_form(tokens[cue.start]))
        || chain.iter().all(|cue| at_clause_boundary(tokens, cue.start))
        || lead_in.iter().any(|t| LIST_ANNOUNCING_NOUNS.contains(&token_core(t).as_str()))
        || lead_in.last().is_some_and(|t| LIST_ANNOUNCING_VERBS.contains(&token_core(t).as_str()))
}

/// True when the cue token marks an enumeration by its own form — an ordinal word
/// ("first") or a dictated digit marker ("1.") — rather than a bare cardinal.
fn is_enumerative_form(token: &str) -> bool {
    digit_cue_value(token).is_some()
        || ORDINAL_CUES.iter().any(|(cue, _)| *cue == token_core(token))
}

/// Strip an explicit "bullet point(s)" / "bullets" instruction from a LEAD-IN,
/// reporting whether one was spoken (R10). Numbered stays the default; bullets are
/// opt-in.
///
/// The instruction is only recognized as the TAIL of the lead-in — directly ahead of
/// the first cue, where "… in bullet points one X two Y" is unambiguously shape — and
/// only in a form no speaker uses for content: "bullet point(s)" without a determiner
/// in front of it ("THE bullet points are one …" is a noun phrase), or a bare
/// "bullets" behind an instruction marker ("in bullets", never "buy bullets").
/// Anything else is left alone, in the lead-in exactly as in an item body.
fn take_bullet_cue<'a>(tokens: &[&'a str]) -> (bool, Vec<&'a str>) {
    let mut end = tokens.len();
    let core = |i: usize| token_core(tokens[i]);
    if end >= 2 && core(end - 2) == "bullet" && matches!(core(end - 1).as_str(), "point" | "points")
    {
        // "the/my/those bullet points" is something being referred to, not shape.
        if end >= 3 && BULLET_DETERMINERS.contains(&core(end - 3).as_str()) {
            return (false, tokens.to_vec());
        }
        end -= 2;
    } else if end >= 2
        && core(end - 1) == "bullets"
        && BULLET_INSTRUCTION_MARKERS.contains(&core(end - 2).as_str())
    {
        end -= 1;
    } else {
        return (false, tokens.to_vec());
    }
    // The marker that introduced the instruction is shape too ("write it IN bullet
    // points" leaves "write it", not "write it in").
    if end > 0 && BULLET_INSTRUCTION_MARKERS.contains(&core(end - 1).as_str()) {
        end -= 1;
    }
    (true, tokens[..end].to_vec())
}

/// Enumeration position of a cue word, cardinals first then ordinals.
fn cue_value(word: &str) -> Option<usize> {
    NUMERIC_CUES
        .iter()
        .chain(ORDINAL_CUES.iter())
        .find(|(cue, _)| *cue == word)
        .map(|(_, value)| *value)
}

/// Enumeration position of a dictated digit marker — `1.` … `10.` (or `1)`). The
/// trailing mark is required: a bare "2" is a quantity far more often than a cue.
fn digit_cue_value(token: &str) -> Option<usize> {
    let digits = token.strip_suffix('.').or_else(|| token.strip_suffix(')'))?;
    let value: usize = digits.parse().ok()?;
    (1..=10).contains(&value).then_some(value)
}

/// Length (in tokens) of the enumeration cue starting at `rest`, if any.
fn enum_cue_len(rest: &[&str]) -> Option<usize> {
    ENUM_CUES.iter().find_map(|phrase| {
        let words: Vec<&str> = phrase.split(' ').collect();
        (rest.len() >= words.len()
            && words.iter().enumerate().all(|(i, w)| token_core(rest[i]) == *w))
        .then_some(words.len())
    })
}

/// True when the token at `i` opens a clause: the utterance start, or right after
/// a token that ended one.
fn at_clause_boundary(tokens: &[&str], i: usize) -> bool {
    i == 0 || tokens[i - 1].ends_with(|c: char| matches!(c, ',' | '.' | ';' | ':'))
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

    // --- List formatting v2 (YV57, rules R7–R11) --------------------------
    //
    // The three inputs below are Wispr's own documented examples — measured
    // failures #1–#3 of the deep-dive (`docs/research/wispr-formatting-deep-dive.md`
    // §1.1), on which the v1 detector fired exactly zero times.

    #[test]
    fn list_from_unpunctuated_numeric_cues() {
        // Real ASR output: no punctuation anywhere, cues mid-utterance (R7/R8).
        let out = format_dictation(
            "My top goals this week are one finish the report two send the presentation",
        );
        assert_eq!(
            out,
            "My top goals this week are:\n1. Finish the report\n2. Send the presentation"
        );
    }

    #[test]
    fn list_from_comma_punctuated_numeric_cues() {
        // Same utterance with the commas an ASR sometimes emits → same list.
        let out = format_dictation(
            "My top goals this week are, one, finish the report, two, send the presentation.",
        );
        assert_eq!(
            out,
            "My top goals this week are:\n1. Finish the report\n2. Send the presentation"
        );
    }

    #[test]
    fn list_from_ordinal_cues_preserves_lead_in() {
        // The lead-in is kept as its own line ending in ':' — v1 dropped it (R9).
        let out = format_dictation(
            "we need to do three things first ship the build second email the client third update the docs",
        );
        assert_eq!(
            out,
            "We need to do three things:\n1. Ship the build\n2. Email the client\n3. Update the docs"
        );
        let lead_in = out.lines().next().unwrap();
        assert_eq!(lead_in, "We need to do three things:");
        assert!(lead_in.ends_with(':'));
    }

    #[test]
    fn list_requires_monotone_cue_sequence() {
        // "one" with no later "two" is a quantity, not a cue — prose is preserved.
        let prose = "I want one coffee and a bagel";
        assert_eq!(format_dictation(prose), prose);
        // A cue chain that never starts at 1 is not a list either.
        let prose = "we shipped it second time around and third parties noticed";
        assert_eq!(format_dictation(prose), prose);
    }

    #[test]
    fn list_ignores_cues_that_open_no_item() {
        // A monotone "one … two" satisfied by accident: both are quantities, and
        // neither opens an item ("of the things", "of the servers").
        let prose = "one of the things I noticed was that two of the servers were down";
        assert_eq!(format_dictation(prose), prose);
    }

    #[test]
    fn list_bullets_only_on_explicit_cue() {
        // Numbered is the default for a numeric/ordinal chain…
        assert_eq!(
            format_dictation("grocery list one whole milk two brown eggs three sourdough bread"),
            "Grocery list:\n1. Whole milk\n2. Brown eggs\n3. Sourdough bread"
        );
        // …and the marker only flips on an explicit spoken instruction, which is
        // itself removed from the text (it's shape, not content).
        assert_eq!(
            format_dictation(
                "grocery list bullet points one whole milk two brown eggs three sourdough bread"
            ),
            "Grocery list:\n- Whole milk\n- Brown eggs\n- Sourdough bread"
        );
        // The instruction alone never manufactures a list out of prose.
        let prose = "I filled in the bullet points on the slide before lunch";
        assert_eq!(format_dictation(prose), prose);
    }

    #[test]
    fn spoken_bullet_words_inside_items_are_content_not_shape() {
        // "bullets" here is a thing being bought, not a marker instruction: it must
        // survive verbatim AND leave the list numbered.
        assert_eq!(
            format_dictation(
                "my top goals this week are one buy more bullets two clean the gun three go home"
            ),
            "My top goals this week are:\n1. Buy more bullets\n2. Clean the gun\n3. Go home"
        );
        // Same for "bullet points" as the object of an item.
        assert_eq!(
            format_dictation(
                "the plan is one order bullet points from supply two label them three ship"
            ),
            "The plan is:\n1. Order bullet points from supply\n2. Label them\n3. Ship"
        );
    }

    #[test]
    fn spoken_bullet_words_in_the_lead_in_are_content_not_shape() {
        // "the bullet points" is the SUBJECT of the lead-in, not a marker request:
        // both words survive and the list stays numbered.
        assert_eq!(
            format_dictation("the bullet points are one ship it two email them"),
            "The bullet points are:\n1. Ship it\n2. Email them"
        );
        // "buy bullets" is a purchase. Nothing may be consumed, nothing flipped to a
        // bullet marker — with no evidence of an enumeration this stays prose.
        let prose = "I need to buy bullets one nine millimeter two forty five three shotgun shells";
        assert_eq!(format_dictation(prose), prose);
        // Shape is still honored where it IS an instruction: on the tail of the
        // lead-in, and behind a marker for the bare "bullets" form.
        assert_eq!(
            format_dictation("write it in bullets one ship the build two email the client"),
            "Write it:\n- Ship the build\n- Email the client"
        );
    }

    #[test]
    fn counting_prose_without_enumeration_evidence_stays_prose() {
        // Ascending cardinals are how an ASR writes down ordinary counting, and the
        // speaker marked no list in any of these: no shape instruction, no
        // announcing lead-in, no clause boundaries, no enumerator cue form. Every
        // one of them used to be rewritten (and word-scrambled) into a list.
        for prose in [
            "I ate one apple two oranges three bananas",
            "chapter one was slow chapter two was better",
            "she scored one goal in the first half two goals in the second half",
            "I gave one dollar to my brother two days later he gave it back",
            "on the one hand it rained on the other hand two people still showed up",
        ] {
            assert_eq!(format_dictation(prose), prose, "{prose:?} was mangled into a list");
        }
    }

    #[test]
    fn quantity_prose_with_ascending_cardinals_stays_prose() {
        // Three ascending cardinals satisfy the monotone rule by accident; each of
        // these is one clause counting things, and every one of them leaves an item
        // dangling on a conjunction — the tell that the clause never ended.
        for prose in [
            "I have one dog two cats and three birds",
            "the recipe needs one egg two cups of flour and three tablespoons of sugar",
            "one hundred and two people came to the show and three hundred left",
            "he was number one in the league and number two in scoring",
        ] {
            assert_eq!(format_dictation(prose), prose, "{prose:?} was mangled into a list");
        }
    }

    #[test]
    fn list_never_drops_input_words() {
        // Property: every input token formatting is not allowed to consume survives,
        // as many times as it was spoken. The ONLY exemption is the enumeration cue
        // vocabulary — a spoken "one"/"first" BECOMES the "1." marker. No length
        // filter, so dropping a one- or two-character word ("a", "at") fails too.
        //
        // The one other exemption is the shape-instruction span, and it is read back
        // from the implementation's OWN span for each input (see
        // `content_token_counts`) rather than from a word list — so a bullet-word the
        // pipeline eats anywhere it did not contract to fails the property.
        const FIXED: &[&str] = &[
            "My top goals this week are one finish the report two send the presentation",
            "My top goals this week are, one, finish the report, two, send the presentation.",
            "we need to do three things first ship the build second email the client third update the docs",
            "first, buy milk, second, buy eggs, third, buy bread",
            "1. finish the report 2. send the presentation 3. book the room",
            "I want one coffee and a bagel",
            "The weather today is nice and I think we should go for a walk before it gets dark.",
            "my top goals this week are one buy more bullets two clean the gun three go home",
            "the plan is one order bullet points from supply two label them three ship",
            "I have one dog two cats and three birds",
            "he was number one in the league and number two in scoring",
            "the bullet points are one ship it two email them",
            "I need to buy bullets one nine millimeter two forty five three shotgun shells",
            "write it in bullets one ship the build two email the client",
            "I ate one apple two oranges three bananas",
            "chapter one was slow chapter two was better",
        ];
        let generated = generated_list_utterances();
        for input in FIXED.iter().map(|s| (*s).to_string()).chain(generated.iter().cloned()) {
            let out = format_dictation(&input);
            for (word, spoken) in content_token_counts(&input) {
                let kept = count_tokens(&out, &word);
                assert!(
                    kept >= spoken,
                    "content word {word:?} spoken {spoken}× but kept {kept}× in {input:?} → {out:?}"
                );
            }
        }
        // …and the generated corpus has to actually reach the list path, or the
        // property above would be satisfied by never formatting anything at all.
        let listed = generated.iter().filter(|i| format_dictation(i).contains('\n')).count();
        assert!(
            listed * 2 > generated.len(),
            "only {listed}/{} generated utterances formatted as lists",
            generated.len()
        );
    }

    /// Multiset of the input tokens formatting must preserve — everything but the
    /// enumeration cue vocabulary (at every length) and the shape-instruction span
    /// the detector actually recognizes in THIS input.
    ///
    /// That span is computed by asking the implementation, not by excluding words
    /// from a curated list: whatever `take_bullet_cue` declines to consume stays
    /// under the property, so widening it later re-breaks this test rather than
    /// silently passing. (The corpus carries no fillers or self-corrections, so the
    /// token stream `clean_backtrack` hands the detector is the input stream.)
    fn content_token_counts(text: &str) -> Vec<(String, usize)> {
        let tokens: Vec<&str> = text.split_whitespace().collect();
        let mut counts: Vec<(String, usize)> = Vec::new();
        let mut bump = |word: String, delta: isize| {
            if word.is_empty() || is_cue_word(&word) {
                return;
            }
            match counts.iter_mut().find(|(w, _)| *w == word) {
                Some((_, n)) => *n = n.saturating_add_signed(delta),
                None if delta > 0 => counts.push((word, delta as usize)),
                None => {}
            }
        };
        for token in &tokens {
            bump(token_core(token), 1);
        }
        if let Some(chain) = resolve_chain(&scan_cues(&tokens)) {
            let lead_in = &tokens[..chain[0].start];
            let (spoken, kept) = take_bullet_cue(lead_in);
            if spoken {
                for token in &lead_in[kept.len()..] {
                    bump(token_core(token), -1);
                }
            }
        }
        counts.retain(|(_, n)| *n > 0);
        counts
    }

    /// How many whitespace tokens of `text` have `word` as their core.
    fn count_tokens(text: &str, word: &str) -> usize {
        text.split_whitespace().filter(|t| token_core(t) == word).count()
    }

    fn is_cue_word(word: &str) -> bool {
        cue_value(word).is_some()
            || CONTINUATION_CUES.contains(&word)
            || ENUM_CUES.iter().any(|c| c.split(' ').any(|w| w == word))
            // "number one" consumes both of its tokens.
            || word == "number"
    }

    /// Interleave enumeration cues with random content tokens: a 0–4 word lead-in,
    /// then 2–4 items of 1–4 words each, cued by cardinals, ordinals or dictated
    /// digit markers. Deterministic (fixed seed), so a failure is always replayable.
    fn generated_list_utterances() -> Vec<String> {
        // Ordinary words only — nothing the pipeline is allowed to consume: no cue
        // words, no fillers, no self-correction markers. Short words are in on
        // purpose: a dropped "a" or "at" has to fail the property too. Lead-ins draw
        // from this same vocabulary as item bodies — including the bullet words, so
        // the generator can build the very lead-ins that used to lose them.
        const CONTENT: &[&str] = &[
            "a", "at", "id", "go", "my", "the", "buy", "milk", "eggs", "report", "client",
            "docs", "gun", "bullet", "bullets", "point", "points", "ship", "email", "call",
            "mom", "list", "goals", "things", "plan", "steps",
        ];
        const CUE_FORMS: [[&str; 4]; 3] = [
            ["one", "two", "three", "four"],
            ["first", "second", "third", "fourth"],
            ["1.", "2.", "3.", "4."],
        ];
        let mut state = 0x5EED_1357_2468_ACE1_u64;
        let pick = |state: &mut u64, n: usize| (next_rand(state) % n as u64) as usize;
        let mut out = Vec::with_capacity(200);
        for _ in 0..200 {
            let forms = CUE_FORMS[pick(&mut state, CUE_FORMS.len())];
            let item_count = 2 + pick(&mut state, 3);
            let mut words: Vec<&str> = Vec::new();
            for _ in 0..pick(&mut state, 5) {
                words.push(CONTENT[pick(&mut state, CONTENT.len())]);
            }
            for form in forms.iter().take(item_count) {
                words.push(form);
                for _ in 0..=pick(&mut state, 4) {
                    words.push(CONTENT[pick(&mut state, CONTENT.len())]);
                }
            }
            out.push(words.join(" "));
        }
        out
    }

    /// Deterministic xorshift64 — no dev-dependency, and the same corpus every run.
    fn next_rand(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
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

    // --- Backtrack v2 (YV58) ----------------------------------------------

    #[test]
    fn backtrack_wait_no_reversed_marker() {
        // Measured failure #5 of the deep-dive: the reversed marker now retracts,
        // and a one-word same-category correction swaps the DAY, not the clause.
        assert_eq!(
            clean_backtrack("let's meet Tuesday, wait no, Friday"),
            "let's meet Friday"
        );
        assert_eq!(
            clean_backtrack("let's meet Tuesday, no wait, Friday"),
            "let's meet Friday"
        );
        // The other new markers retract their clause the same way v1 ones did.
        assert_eq!(
            clean_backtrack("ship it Monday, sorry I meant, ship it Thursday"),
            "ship it Thursday"
        );
        assert_eq!(
            clean_backtrack("send the deck today, let me rephrase, send the deck tonight"),
            "send the deck tonight"
        );
        assert_eq!(
            clean_backtrack("the invoice is due April, i mean, June"),
            "the invoice is due June"
        );
    }

    #[test]
    fn backtrack_keeps_i_actually_enjoyed_the_movie() {
        // [DOC] negative case: "actually" as an adverb has no category on either
        // side, so the restatement rule must not fire.
        let doc = "I actually enjoyed the movie";
        assert_eq!(clean_backtrack(doc), doc);
        // The categorised restatement still fires either side of it.
        assert_eq!(clean_backtrack("coffee at 2 actually 3"), "coffee at 3");
        assert_eq!(
            clean_backtrack("standup is Tuesday actually Thursday"),
            "standup is Thursday"
        );
    }

    #[test]
    fn backtrack_drops_hedges_only_where_they_are_particles() {
        // Comma-bounded and sentence-initial hedges go…
        assert_eq!(
            clean_backtrack("we should, sort of, rewrite the intro"),
            "we should, rewrite the intro"
        );
        assert_eq!(
            clean_backtrack("Basically, the migration is done"),
            "The migration is done"
        );
        assert_eq!(
            clean_backtrack("it was, kind of, slow and, i guess, expensive"),
            "it was, slow and, expensive"
        );
        // …the same words as content stay.
        let content = "I like the plan and it is sort of blue";
        assert_eq!(clean_backtrack(content), content);
        let literal = "the file is literally 4 gigabytes";
        assert_eq!(clean_backtrack(literal), literal);
        // Never inside a quoted span — there the speaker is reporting words.
        let quoted = "he said \"well, like, whatever\" and left";
        assert_eq!(clean_backtrack(quoted), quoted);
    }

    #[test]
    fn backtrack_never_empties_non_empty_input() {
        // Property: over utterances built from every trigger vocabulary v2 knows —
        // fillers, hedges, markers, mark names — cleanup always returns text.
        for input in generated_backtrack_utterances() {
            let cleaned = clean_backtrack(&input);
            assert!(
                !cleaned.trim().is_empty(),
                "clean_backtrack emptied {input:?}"
            );
            for mode in [DictationMode::Notes, DictationMode::Plain, DictationMode::Code] {
                let marked = apply_spoken_marks(&cleaned, mode);
                assert!(
                    !marked.trim().is_empty(),
                    "apply_spoken_marks emptied {cleaned:?} in {mode:?}"
                );
            }
        }
    }

    /// Deterministic pseudo-random utterances mixing content words with every
    /// trigger vocabulary the backtrack/marks rules recognize.
    fn generated_backtrack_utterances() -> Vec<String> {
        const CONTENT: &[&str] = &["ship", "the", "report", "Friday", "at", "7", "to", "Jeisil"];
        const TRIGGERS: &[&str] = &[
            "um", "uh", "like,", ",", "you know,", "scratch that,", "wait no,", "no wait,",
            "i mean,", "basically,", "period", "comma", "new line", "new paragraph",
            "exclamation point", "quotation mark", "at symbol",
        ];
        let mut state = 0x5eed_1958_u64;
        let mut out = Vec::with_capacity(240);
        for len in 1..=12usize {
            for _ in 0..20 {
                let words: Vec<&str> = (0..len)
                    .map(|_| {
                        let r = next_rand(&mut state);
                        if r.is_multiple_of(3) {
                            TRIGGERS[(r >> 8) as usize % TRIGGERS.len()]
                        } else {
                            CONTENT[(r >> 8) as usize % CONTENT.len()]
                        }
                    })
                    .collect();
                out.push(words.join(" "));
            }
        }
        out
    }

    // --- Spoken marks (YV58, R1/R2) ---------------------------------------

    #[test]
    fn spoken_marks_exclamation_and_period() {
        // Wispr's verbatim example. ("seven" → "7" is R6 digit normalization, a
        // separate rule — R1 owns the glyphs and the removal of their names.)
        assert_eq!(
            apply_spoken_marks(
                "I can't wait to see you exclamation point Let's meet at 7 period",
                DictationMode::Chat
            ),
            "I can't wait to see you! Let's meet at 7."
        );
        // A terminal mark is never doubled onto another one.
        assert_eq!(
            apply_spoken_marks("see you question mark period", DictationMode::Notes),
            "see you?"
        );
        // Tight and paired glyphs attach the way they are typed.
        assert_eq!(
            apply_spoken_marks("email wilson at symbol drivia dot dev", DictationMode::Notes),
            "email wilson@drivia dot dev"
        );
        assert_eq!(
            apply_spoken_marks(
                "she said quotation mark ship it quotation mark today",
                DictationMode::Notes
            ),
            "she said \"ship it\" today"
        );
        // A determiner in front (noun), "to" in front (verb) or "of" behind (noun
        // phrase) all mean the speaker is TALKING about the mark, not dictating it.
        for content in [
            "the period was long and a dash of salt",
            "the new line of products ships in June",
            "I need to dash to the store",
            "the Jurassic period of the dinosaurs",
        ] {
            assert_eq!(apply_spoken_marks(content, DictationMode::Notes), content);
        }
    }

    #[test]
    fn spoken_marks_new_line_and_new_paragraph() {
        // R2: the command owns the break only — casing/`?` are other rules' work.
        assert_eq!(
            apply_spoken_marks(
                "When is reading club new line should be tomorrow",
                DictationMode::Chat
            ),
            "When is reading club\nshould be tomorrow"
        );
        for command in ["new paragraph", "start a new paragraph", "skip a line"] {
            let spoken = format!("first thought {command} second thought");
            assert_eq!(
                apply_spoken_marks(&spoken, DictationMode::Notes),
                "first thought\n\nsecond thought",
                "{command} did not open a paragraph"
            );
        }
        // Breaks the list renderer already made survive the pass.
        assert_eq!(
            apply_spoken_marks("1. Buy milk\n2. Buy eggs", DictationMode::Notes),
            "1. Buy milk\n2. Buy eggs"
        );
        // A command with nothing before it stays as spoken text — never lose words.
        assert_eq!(
            apply_spoken_marks("new line ship it", DictationMode::Notes),
            "new line ship it"
        );
    }

    #[test]
    fn spoken_marks_are_inert_in_code_mode() {
        // Identifiers and paths are full of these words — `Code` never rewrites.
        let code = "let new line equals sign underscore period value";
        assert_eq!(apply_spoken_marks(code, DictationMode::Code), code);
        // …and the pipeline honours that gate end to end.
        let raw = "print open paren value close paren period";
        assert_eq!(
            run_cleanup(raw, CleanupLevel::High, DictationMode::Code, no_dict, polish_llm),
            raw
        );
        assert_eq!(
            run_cleanup(raw, CleanupLevel::High, DictationMode::Notes, no_dict, polish_llm),
            "print (value)."
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

    // --- Undo AI edit (YV51) ----------------------------------------------

    #[test]
    fn undo_offers_the_raw_take_only_when_cleanup_changed_it() {
        // Light cleanup dropped the filler → raw differs → undo is offered, and
        // it hands back the VERBATIM raw (fillers included), not a re-clean.
        let raw = "um the report is uh done";
        let polished = run_cleanup(raw, CleanupLevel::Light, DictationMode::Notes, no_dict, polish_llm);
        assert_ne!(polished, raw);
        assert_eq!(undo_ai_edit_text(&polished, Some(raw)), Some(raw));
    }

    #[test]
    fn undo_is_inert_when_raw_equals_polished() {
        // Auto-Cleanup = None is a verbatim passthrough, so there is nothing to
        // undo — the tray item / shortcut / history button must stay disabled.
        let raw = "the report is done";
        let polished = run_cleanup(raw, CleanupLevel::None, DictationMode::Notes, no_dict, polish_llm);
        assert_eq!(polished, raw);
        assert_eq!(undo_ai_edit_text(&polished, Some(raw)), None);
        // Same when every enabled stage was a no-op on already-clean text.
        let clean = run_cleanup(raw, CleanupLevel::Light, DictationMode::Notes, no_dict, polish_llm);
        assert_eq!(undo_ai_edit_text(&clean, Some(raw)), None);
    }

    #[test]
    fn undo_is_inert_without_a_usable_raw() {
        // Legacy rows predate the raw_text column → nothing to fall back to.
        assert_eq!(undo_ai_edit_text("polished text", None), None);
        // A blank raw must never be pasted over the user's text.
        assert_eq!(undo_ai_edit_text("polished text", Some("")), None);
        assert_eq!(undo_ai_edit_text("polished text", Some("   \n ")), None);
    }

    #[test]
    fn undo_ignores_a_context_join_that_only_added_surrounding_space() {
        // YV50 joins the take onto the caret text by adding a leading space.
        // That alone is not an AI edit, so undo stays inert...
        let raw = "buy milk";
        let joined = join_with_context(raw, Some("I need to"));
        assert_eq!(joined, " buy milk");
        assert_eq!(undo_ai_edit_text(&joined, Some(raw)), None);
        // ...but a casing change from the same join IS a real edit to undo.
        let recased = join_with_context("Buy milk", Some("I need to"));
        assert_eq!(recased, " buy milk");
        assert_eq!(
            undo_ai_edit_text(&recased, Some("Buy milk")),
            Some("Buy milk")
        );
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

