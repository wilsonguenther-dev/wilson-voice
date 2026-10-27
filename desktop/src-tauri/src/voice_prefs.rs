//! Y9-C — spoken preference rules: say a rule once, it applies where it matches.
//!
//! The user says (or types) one sentence — "never use exclamation marks in
//! email". The polish sidecar, on this Mac, turns it into a structured
//! `{filter, rule}` with a dedicated prompt ([`crate::polish_protocol::KIND_DERIVE_PREFERENCE`]).
//! The answer is validated here against an ALLOWLIST of filter shapes:
//!
//! * `mode:<one of the six polish modes>` — `email` `document` `notes` `chat`
//!   `plain` `list` (`code` is absent on purpose: code never reaches a model);
//! * `app:<bundle-id>` — reverse-DNS, e.g. `app:com.apple.mail`;
//! * `lang:<code>` — an ISO 639 primary subtag, e.g. `lang:de`;
//! * `*` — everywhere.
//!
//! Anything else — prose, a refusal, a filter outside the allowlist, a rule that
//! is empty or too long — is REJECTED and shown back to the user, never stored
//! half-derived.
//!
//! A derived rule is stored PENDING in the append-only `voice_preferences`
//! table (`db.rs`) and changes nothing until the user presses **Apply**. Retract
//! flips it off and stamps `retracted_at`; the row — what the user said, what
//! Yap heard — stays, so the history can be read back instead of trusted. The
//! table refuses DELETE and refuses edits to what was said at the SQL level.
//!
//! Matching rules ride the polish request ([`crate::polish_protocol::PolishRequest::preferences`])
//! for takes whose mode / app / language match, bounded by
//! [`crate::polish_protocol::bound_preferences`]. Zero matching rules ⇒ the
//! byte-identical request Yap sent before this module existed.

use std::panic::AssertUnwindSafe;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::dictation::DictationMode;
use crate::polish::{self, mode_tag, PolishClient};
use crate::polish_protocol::{bound_preferences, PolishRequest, MAX_PREFERENCE_RULE_CHARS};

/// The six modes a `mode:` filter may name — every polish mode, never `code`.
pub const PREFERENCE_MODES: [&str; 6] = ["email", "document", "notes", "chat", "plain", "list"];

/// Longest spoken sentence a derivation will consider. A rule is one sentence;
/// a paragraph is a dictation, not a preference.
pub const MAX_SPOKEN_CHARS: usize = 400;

/// Deadline for one derivation. Generous next to a dictation's polish deadline:
/// the user is waiting on a Settings screen, and the sidecar may be cold.
pub const DERIVE_DEADLINE_MS: u64 = 15_000;

/// GBNF for the derivation: the `{filter, rule}` object with the filter
/// constrained to the allowlist's SHAPES, or the explicit refusal. The parent
/// still re-validates every byte — the grammar makes a small model reliable, the
/// allowlist below is what is trusted.
pub const DERIVE_GRAMMAR: &str = r#"root ::= ruleobj | refusal
ruleobj ::= "{\"filter\":\"" filter "\",\"rule\":\"" text "\"}"
refusal ::= "{\"error\":\"unclear\"}"
filter ::= "*" | "mode:" mode | "app:" bundle | "lang:" lang
mode ::= "email" | "document" | "notes" | "chat" | "plain" | "list"
bundle ::= label ("." label)+
label ::= [A-Za-z0-9-]+
lang ::= [a-z] [a-z] [a-z]?
text ::= [^"\\\x7F\x00-\x1F]+
"#;

/// Why a spoken rule did not become a stored rule. Every variant is shown back
/// to the user; none of them stores anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreferenceError {
    /// Nothing was said.
    Empty,
    /// The sentence is longer than one rule.
    TooLong,
    /// The model's answer was not a `{filter, rule}` object (or it said
    /// `{"error":"unclear"}`). Carries the sentence that was heard.
    Unparseable(String),
    /// The filter is outside the allowlist. Carries the offending filter.
    FilterNotAllowed(String),
    /// No polish model is installed, so nothing on this Mac can read the rule.
    NoModel,
    /// The sidecar failed (deadline, crash, protocol). Carries its reason tag.
    Model(&'static str),
    /// The rule id is unknown, or it is not in a state that allows the action.
    NotFound,
    /// SQLite refused the write.
    Storage(String),
}

impl std::fmt::Display for PreferenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "Say or type a rule first, like “never use exclamation marks in email”."),
            Self::TooLong => write!(f, "That's longer than one rule. Say one rule at a time."),
            Self::Unparseable(heard) => write!(
                f,
                "I couldn't turn “{heard}” into a rule, so nothing was saved. Try something like “never use exclamation marks in email”."
            ),
            Self::FilterNotAllowed(filter) => write!(
                f,
                "I understood a place I can't apply rules to (“{filter}”), so nothing was saved. Rules can apply to a kind of writing (email, chat…), one app, one language, or everywhere."
            ),
            Self::NoModel => write!(
                f,
                "Rules are understood on this Mac by the polish model. Install one in Settings → Dictation first."
            ),
            Self::Model(reason) => write!(f, "The polish model couldn't read that rule ({reason}). Nothing was saved — try again."),
            Self::NotFound => write!(f, "That rule can't be changed from its current state."),
            Self::Storage(e) => write!(f, "Couldn't save the rule: {e}"),
        }
    }
}

/// One parsed, allowlisted filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreferenceFilter {
    /// `*` — every take.
    Any,
    /// `mode:<mode>` — one of [`PREFERENCE_MODES`].
    Mode(String),
    /// `app:<bundle-id>` — the paste target's bundle id, case-insensitive.
    App(String),
    /// `lang:<code>` — the take's language primary subtag.
    Lang(String),
}

fn is_bundle_id(v: &str) -> bool {
    let labels: Vec<&str> = v.split('.').collect();
    v.len() <= 155
        && labels.len() >= 2
        && labels
            .iter()
            .all(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
}

fn is_lang(v: &str) -> bool {
    (2..=3).contains(&v.len()) && v.chars().all(|c| c.is_ascii_lowercase())
}

impl PreferenceFilter {
    /// Parse a filter string through the allowlist. This is THE gate: a filter
    /// that does not come back `Ok` here is never stored and never reaches a
    /// prompt.
    pub fn parse(raw: &str) -> Result<Self, PreferenceError> {
        let f = raw.trim();
        if f == "*" {
            return Ok(Self::Any);
        }
        let reject = || PreferenceError::FilterNotAllowed(f.to_string());
        let (kind, value) = f.split_once(':').ok_or_else(reject)?;
        let value = value.trim();
        match kind.trim() {
            "mode" => {
                let v = value.to_ascii_lowercase();
                if PREFERENCE_MODES.contains(&v.as_str()) {
                    Ok(Self::Mode(v))
                } else {
                    Err(reject())
                }
            }
            "app" if is_bundle_id(value) => Ok(Self::App(value.to_string())),
            "lang" => {
                let v = value.to_ascii_lowercase();
                if is_lang(&v) {
                    Ok(Self::Lang(v))
                } else {
                    Err(reject())
                }
            }
            _ => Err(reject()),
        }
    }

    /// The canonical stored form.
    pub fn canonical(&self) -> String {
        match self {
            Self::Any => "*".to_string(),
            Self::Mode(m) => format!("mode:{m}"),
            Self::App(b) => format!("app:{b}"),
            Self::Lang(l) => format!("lang:{l}"),
        }
    }

    /// Does this filter select `take`?
    pub fn matches(&self, take: &TakeContext<'_>) -> bool {
        match self {
            Self::Any => true,
            Self::Mode(m) => mode_tag(take.mode) == m,
            Self::App(b) => {
                !take.bundle_id.trim().is_empty() && take.bundle_id.trim().eq_ignore_ascii_case(b)
            }
            Self::Lang(l) => take
                .language
                .trim()
                .split(['-', '_'])
                .next()
                .is_some_and(|primary| primary.eq_ignore_ascii_case(l)),
        }
    }
}

/// What a take is, for matching: its resolved mode, its paste target's bundle
/// id (`""` when unknown) and the language the user dictates in.
#[derive(Debug, Clone, Copy)]
pub struct TakeContext<'a> {
    pub mode: DictationMode,
    pub bundle_id: &'a str,
    pub language: &'a str,
}

/// A validated derivation, before it is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedPreference {
    pub filter: String,
    pub rule: String,
}

/// One row of the append-only `voice_preferences` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoicePreference {
    pub id: String,
    /// The sentence exactly as the user said it.
    pub spoken: String,
    /// The derived, allowlisted filter (`mode:email`, `app:…`, `lang:…`, `*`).
    pub filter: String,
    /// The derived rule text that reaches the prompt.
    pub rule: String,
    pub created_at: DateTime<Utc>,
    /// In effect: applied and not retracted.
    pub active: bool,
    /// When the user pressed Apply. `None` = still pending (no effect).
    pub applied_at: Option<DateTime<Utc>>,
    /// When the user retracted (or discarded) it. The row is never deleted.
    pub retracted_at: Option<DateTime<Utc>>,
}

/// Validate a rule's text: one line, at least two words, within the budget a
/// single rule may spend.
fn clean_rule(raw: &str) -> Option<String> {
    let rule = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let words = rule.split(' ').filter(|w| !w.is_empty()).count();
    (words >= 2
        && rule.chars().count() <= MAX_PREFERENCE_RULE_CHARS
        && !rule.chars().any(char::is_control))
    .then_some(rule)
}

/// Words that carry no CONTENT for the grounding check: polarity, filler and
/// the verbs every rule shares. "never" alone must not ground a rule.
const UNGROUNDING_WORDS: [&str; 24] = [
    "never", "always", "please", "with", "when", "that", "this", "from", "into", "your", "about",
    "should", "would", "could", "dont", "don't", "want", "like", "make", "sure", "every", "write",
    "speak", "using",
];

/// Is `rule` about what was said? At least one content word of the spoken
/// sentence (4+ letters, not in [`UNGROUNDING_WORDS`]) must appear in the rule.
/// Measured against the real 1.5B polish model: asked to derive a rule from
/// "banana telephone purple" it answered the prompt's own example ("Never use
/// exclamation marks.") — a well-formed object about nothing the user said.
/// That is unparseable as far as the user is concerned, and is rejected as such.
fn is_grounded(spoken: &str, rule: &str) -> bool {
    let rule = rule.to_lowercase();
    spoken
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| w.chars().count() >= 4 && !UNGROUNDING_WORDS.contains(w))
        .any(|w| rule.contains(w.trim_end_matches('s')))
}

/// Parse the sidecar's answer for `spoken` into a [`DerivedPreference`].
///
/// Tolerates a markdown fence or a sentence around the object (a model that
/// ignores "output ONLY"), nothing else: the object must carry a string
/// `filter` that passes [`PreferenceFilter::parse`] and a string `rule` that
/// passes [`clean_rule`]. `{"error": …}` is the model saying it could not read
/// the sentence, and is reported as such.
pub fn parse_derivation(spoken: &str, raw: &str) -> Result<DerivedPreference, PreferenceError> {
    let unparseable = || PreferenceError::Unparseable(spoken.trim().to_string());
    let start = raw.find('{').ok_or_else(unparseable)?;
    let end = raw.rfind('}').ok_or_else(unparseable)?;
    if end < start {
        return Err(unparseable());
    }
    let value: serde_json::Value =
        serde_json::from_str(&raw[start..=end]).map_err(|_| unparseable())?;
    let obj = value.as_object().ok_or_else(unparseable)?;
    if obj.contains_key("error") {
        return Err(unparseable());
    }
    let filter = obj
        .get("filter")
        .and_then(|v| v.as_str())
        .ok_or_else(unparseable)?;
    let rule = obj
        .get("rule")
        .and_then(|v| v.as_str())
        .ok_or_else(unparseable)?;
    let rule = clean_rule(rule).ok_or_else(unparseable)?;
    if !is_grounded(spoken, &rule) {
        return Err(unparseable());
    }
    let filter = PreferenceFilter::parse(filter)?;
    Ok(DerivedPreference {
        filter: filter.canonical(),
        rule,
    })
}

/// Ask `client` to derive a rule from `spoken`, then validate the answer.
/// Stores nothing.
pub fn derive_with(
    spoken: &str,
    client: &dyn PolishClient,
    deadline_ms: u64,
) -> Result<DerivedPreference, PreferenceError> {
    let spoken = spoken.trim();
    if spoken.is_empty() {
        return Err(PreferenceError::Empty);
    }
    if spoken.chars().count() > MAX_SPOKEN_CHARS {
        return Err(PreferenceError::TooLong);
    }
    let req = PolishRequest::derive_preference(
        polish::next_request_id(),
        deadline_ms,
        spoken.to_string(),
        Some(DERIVE_GRAMMAR.to_string()),
    );
    let answered = std::panic::catch_unwind(AssertUnwindSafe(|| client.rewrite(&req)));
    let raw = match answered {
        Ok(Ok(raw)) => raw,
        Ok(Err(err)) => return Err(PreferenceError::Model(err.reason())),
        Err(_) => return Err(PreferenceError::Model("client_panic")),
    };
    parse_derivation(spoken, &raw)
}

/// Derive and, ONLY on success, store the rule PENDING. It has no effect until
/// [`Database::apply_voice_preference`]. A rejection stores nothing.
pub fn propose(
    db: &Database,
    spoken: &str,
    client: &dyn PolishClient,
    deadline_ms: u64,
) -> Result<VoicePreference, PreferenceError> {
    let derived = derive_with(spoken, client, deadline_ms)?;
    db.add_voice_preference(spoken.trim(), &derived)
}

/// [`propose`] against the installed polish model `model_id` (the app's
/// `polish_model` setting) on the warm sidecar.
pub fn propose_installed(
    db: &Database,
    model_id: &str,
    spoken: &str,
) -> Result<VoicePreference, PreferenceError> {
    let path = crate::models::polish_model(model_id.trim())
        .filter(|m| crate::models::is_polish_downloaded(m))
        .map(crate::models::polish_model_path)
        .ok_or(PreferenceError::NoModel)?;
    propose(
        db,
        spoken,
        &polish::SidecarClient::new(path),
        DERIVE_DEADLINE_MS,
    )
}

/// The rule texts that apply to `take`: APPLIED, not retracted, filter passes
/// the allowlist AGAIN (a row is re-checked at the prompt boundary, not only at
/// insert) and matches. Most recently applied first, then bounded.
pub fn matching_rules(rows: &[VoicePreference], take: &TakeContext<'_>) -> Vec<String> {
    let mut live: Vec<&VoicePreference> = rows
        .iter()
        .filter(|p| p.active && p.applied_at.is_some() && p.retracted_at.is_none())
        .filter(|p| PreferenceFilter::parse(&p.filter).is_ok_and(|f| f.matches(take)))
        .collect();
    live.sort_by(|a, b| b.applied_at.cmp(&a.applied_at));
    let rules: Vec<String> = live.into_iter().map(|p| p.rule.clone()).collect();
    bound_preferences(&rules)
}
