//! Y10-A — multi-language dictation.
//!
//! Whisper-family models are multilingual, but an UNCONSTRAINED auto-detect over
//! ~99 languages mis-detects: a short English take with an accent comes back as
//! Welsh. So the user picks the few languages they actually speak (the language
//! SET), and auto-detect is only ever allowed to choose among those.
//!
//! Everything here is driven off what the loaded model PROBED
//! ([`crate::asr_engine::ModelCapabilities`]), never a hardcoded belief about
//! the model: a model without language detection falls back to the single
//! primary language and says so in [`DecodePlan::notice`], which the Settings
//! control and the bar's picker both show. A picker that silently does nothing
//! is the defect this file exists to prevent.
//!
//! The cleanup pipeline is English-shaped in places (spoken punctuation names,
//! the email greeting/sign-off shape, filler words). [`CleanupStage`] states,
//! per stage, whether it is language-agnostic; `dictation::cleanup_pipeline`
//! skips every stage that is not for a non-English take instead of applying
//! English rules to German.
//!
//! Dictation only. The meeting path is English-only by design
//! (`tests/meeting_english_only_gate.rs`) and never reads these settings.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::transcribe_progress::TakeObserver;
use crate::transcription::{DictationTake, TranscriptionManager};

/// The language every install starts with, and the one an install written
/// before the language set existed keeps.
pub const DEFAULT_LANGUAGE: &str = "en";

/// The most languages a set may hold. Auto-detect gets worse with every
/// language it may choose from; four covers every real multilingual speaker.
pub const MAX_LANGUAGE_SET: usize = 4;

/// What the picker offers, as `(ISO 639-1 code, English name)`. Deliberately
/// short — NOT the model's ~99 — and further narrowed at runtime to the
/// languages the installed model reports it can transcribe.
pub const PICKER_LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("ht", "Haitian Creole"),
    ("pt", "Portuguese"),
    ("de", "German"),
    ("it", "Italian"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("uk", "Ukrainian"),
    ("ru", "Russian"),
    ("ar", "Arabic"),
    ("hi", "Hindi"),
    ("zh", "Chinese"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
];

/// The English name of a picker code, or the code itself.
pub fn language_name(code: &str) -> String {
    PICKER_LANGUAGES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, n)| (*n).to_string())
        .unwrap_or_else(|| code.to_string())
}

fn normalize_code(code: &str) -> Option<String> {
    let c = code.trim().to_ascii_lowercase();
    PICKER_LANGUAGES.iter().any(|(k, _)| *k == c).then_some(c)
}

/// True for an English take. A blank code is treated as English: it is what
/// every pre-Y10 settings file meant by "the default".
pub fn is_english(code: &str) -> bool {
    let c = code.trim().to_ascii_lowercase();
    c.is_empty() || c == "en" || c.starts_with("en-") || c.starts_with("en_")
}

/// What the loaded model said about languages (from its probed capabilities).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageSupport {
    /// `supports_language_detect` from the probe.
    pub supports_detect: bool,
    /// Codes the model can transcribe. Empty = the model did not advertise a
    /// list, which is NOT a licence to assume all of them: see [`Self::can_decode`].
    pub languages: Vec<String>,
}

impl LanguageSupport {
    /// What an engine that reports nothing is treated as: no detection, no
    /// list. The plan then decodes the primary language only.
    pub fn unknown() -> Self {
        Self {
            supports_detect: false,
            languages: Vec::new(),
        }
    }

    pub fn from_capabilities(caps: &crate::asr_engine::ModelCapabilities) -> Self {
        Self {
            supports_detect: caps.supports_language_detect,
            languages: caps
                .languages
                .iter()
                .map(|l| l.trim().to_ascii_lowercase())
                .collect(),
        }
    }

    /// Whether `code` may be handed to this model as a hint. A model with no
    /// advertised list is given the benefit of the doubt (the engine itself
    /// rejects a hint it cannot use, loudly).
    pub fn can_decode(&self, code: &str) -> bool {
        self.languages.is_empty() || self.languages.iter().any(|l| l == code)
    }
}

/// The user's language preferences, normalized.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguagePrefs {
    /// The language used when only one can be: a single-language set, auto
    /// off, or a model that cannot detect. Always a member of `set`.
    pub primary: String,
    /// The languages the user speaks, primary first, at most
    /// [`MAX_LANGUAGE_SET`], every one a [`PICKER_LANGUAGES`] code.
    pub set: Vec<String>,
    /// Auto-detect among `set`. Meaningless for a one-language set.
    pub auto: bool,
}

impl LanguagePrefs {
    /// Build from the three settings fields. An install written before the set
    /// existed has `language_set == []` and keeps exactly its old single
    /// language — English for every default install.
    pub fn from_settings(language: &str, language_set: &[String], auto: bool) -> Self {
        let mut set: Vec<String> = Vec::new();
        for code in language_set {
            if let Some(c) = normalize_code(code) {
                if !set.contains(&c) {
                    set.push(c);
                }
            }
        }
        let primary = normalize_code(language)
            .or_else(|| set.first().cloned())
            .unwrap_or_else(|| DEFAULT_LANGUAGE.to_string());
        set.retain(|c| *c != primary);
        set.insert(0, primary.clone());
        set.truncate(MAX_LANGUAGE_SET);
        Self { primary, set, auto }
    }
}

/// How a take is decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DecodeMode {
    /// One language, handed to the engine as a hint.
    Single,
    /// No hint: the model detects, and the result is held to the set.
    AutoWithinSet,
    /// The user asked for auto-detect, the model cannot do it: primary only,
    /// and [`DecodePlan::notice`] says so.
    FallbackNoDetect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodePlan {
    pub mode: DecodeMode,
    /// The engine's language hint. `None` only in [`DecodeMode::AutoWithinSet`].
    pub hint: Option<String>,
    /// The languages auto-detect may choose from (the set ∩ what the model
    /// can transcribe). One element outside auto mode.
    pub candidates: Vec<String>,
    /// The primary language a mis-detection is re-decoded as.
    pub primary: String,
    /// What the user must be told, in plain words, when the plan is not what
    /// they picked. `None` when it is.
    pub notice: Option<String>,
}

fn join_names(codes: &[String]) -> String {
    let names: Vec<String> = codes.iter().map(|c| language_name(c)).collect();
    match names.len() {
        0 => String::new(),
        1 => names[0].clone(),
        n => format!("{} and {}", names[..n - 1].join(", "), names[n - 1]),
    }
}

/// Decide how the next take is decoded, from the prefs and the PROBED support.
pub fn plan_decode(prefs: &LanguagePrefs, support: &LanguageSupport) -> DecodePlan {
    let usable: Vec<String> = prefs
        .set
        .iter()
        .filter(|c| support.can_decode(c))
        .cloned()
        .collect();
    let dropped: Vec<String> = prefs
        .set
        .iter()
        .filter(|c| !support.can_decode(c))
        .cloned()
        .collect();
    // The language a single-language decode uses: the primary when the model
    // can do it, else the first usable member, else the model's own first
    // language (an English-only model asked for French decodes English and
    // says so, rather than failing every take).
    let single = if support.can_decode(&prefs.primary) {
        prefs.primary.clone()
    } else {
        usable
            .first()
            .cloned()
            .or_else(|| support.languages.first().cloned())
            .unwrap_or_else(|| prefs.primary.clone())
    };
    let unsupported_notice = (!dropped.is_empty()).then(|| {
        format!(
            "This speech model can't transcribe {}, so Yap is using {}.",
            join_names(&dropped),
            language_name(&single)
        )
    });

    if !prefs.auto || usable.len() <= 1 {
        return DecodePlan {
            mode: DecodeMode::Single,
            hint: Some(single.clone()),
            candidates: vec![single.clone()],
            primary: single,
            notice: unsupported_notice,
        };
    }
    if !support.supports_detect {
        return DecodePlan {
            mode: DecodeMode::FallbackNoDetect,
            hint: Some(single.clone()),
            candidates: vec![single.clone()],
            notice: Some(format!(
                "This speech model can't detect languages, so Yap uses {} only — not {}. Pick a multilingual (Whisper) model in Settings to switch automatically.",
                language_name(&single),
                join_names(&usable),
            )),
            primary: single,
        };
    }
    DecodePlan {
        mode: DecodeMode::AutoWithinSet,
        hint: None,
        candidates: usable,
        primary: single,
        notice: unsupported_notice,
    }
}

/// What to do with the language the model detected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Keep the decode; the take is in this language.
    Accept(String),
    /// The model chose outside the set (or would not say): decode again with
    /// this language as the hint.
    Redecode(String),
}

/// Hold a detection to the set. Auto-detect may ONLY choose a candidate: a
/// detection outside the set, or no detection at all, is re-decoded as the
/// primary — never accepted.
pub fn resolve_detected(plan: &DecodePlan, detected: Option<&str>) -> Resolution {
    if plan.mode != DecodeMode::AutoWithinSet {
        return Resolution::Accept(plan.hint.clone().unwrap_or_else(|| plan.primary.clone()));
    }
    match detected.map(|d| d.trim().to_ascii_lowercase()) {
        Some(d) if plan.candidates.contains(&d) => Resolution::Accept(d),
        _ => Resolution::Redecode(plan.primary.clone()),
    }
}

/// A finished take plus the language it was decoded in.
#[derive(Debug, Clone, PartialEq)]
pub struct LanguageTake {
    pub take: DictationTake,
    /// ISO code of the take, as decided by [`resolve_detected`].
    pub language: String,
    pub mode: DecodeMode,
    pub notice: Option<String>,
    /// True when auto-detect chose outside the set and the take was decoded a
    /// second time as the primary.
    pub redecoded: bool,
    /// WF-12 — a picker language the model heard that is NOT in the user's set
    /// (see [`outside_language`]). The pill turns it into the "Sounded like
    /// Spanish — Add Spanish?" card, rate-limited by [`LanguageOfferLedger`].
    pub outside: Option<String>,
}

/// Decode one dictation take under the user's language prefs: plan off the
/// model's probed support, decode, hold the detection to the set, and decode
/// again as the primary when it fell outside.
#[allow(clippy::too_many_arguments)]
pub fn decode_in_set(
    manager: &TranscriptionManager,
    model_id: &str,
    model_path: &Path,
    samples_16k_mono: Vec<f32>,
    prefs: &LanguagePrefs,
    bias_prompt: Option<String>,
    observer: &mut dyn TakeObserver,
) -> Result<LanguageTake, String> {
    let support = manager
        .language_support_on(model_id, model_path)?
        .unwrap_or_else(LanguageSupport::unknown);
    let plan = plan_decode(prefs, &support);
    if let Some(notice) = &plan.notice {
        log::info!("dictation language: {notice}");
    }
    let keep = (plan.mode == DecodeMode::AutoWithinSet).then(|| samples_16k_mono.clone());
    let take = manager.transcribe_take_observed_on(
        model_id,
        model_path,
        samples_16k_mono,
        plan.hint.clone(),
        bias_prompt.clone(),
        observer,
    )?;
    let Some(samples) = keep else {
        // WF-12: a hinted decode may still report what it heard. A failed read
        // only costs the offer, never the take.
        let heard = manager
            .last_detected_language_on(model_id, model_path)
            .ok()
            .flatten();
        // The hint itself is not a different language: an English-only model
        // decoding a French-only set as English must not offer "Add English?".
        let heard =
            heard.filter(|h| plan.hint.as_deref() != Some(h.trim().to_ascii_lowercase().as_str()));
        return Ok(LanguageTake {
            take,
            language: plan.primary.clone(),
            mode: plan.mode,
            notice: plan.notice,
            redecoded: false,
            outside: outside_language(prefs, heard.as_deref()),
        });
    };
    let detected = manager.last_detected_language_on(model_id, model_path)?;
    let outside = outside_language(prefs, detected.as_deref());
    match resolve_detected(&plan, detected.as_deref()) {
        Resolution::Accept(language) => Ok(LanguageTake {
            take,
            language,
            mode: plan.mode,
            notice: plan.notice,
            redecoded: false,
            outside,
        }),
        Resolution::Redecode(language) => {
            log::info!(
                "dictation language: detected {:?} is outside {:?} — decoding again as {language}",
                detected,
                plan.candidates
            );
            let take = manager.transcribe_take_observed_on(
                model_id,
                model_path,
                samples,
                Some(language.clone()),
                bias_prompt,
                observer,
            )?;
            Ok(LanguageTake {
                take,
                language,
                mode: plan.mode,
                notice: plan.notice,
                redecoded: true,
                outside,
            })
        }
    }
}

/// The rules stages of the cleanup pipeline, in pipeline order, each with a
/// stated answer to "does this work on a language that is not English?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupStage {
    /// The user's own vocabulary/replacements — their words, any language.
    Dictionary,
    /// `clean_backtrack`: English fillers ("um", "uh") and English
    /// self-corrections ("scratch that", "I mean").
    Backtrack,
    /// `format_dictation`: list detection keyed on "first / second / next".
    ListFormat,
    /// `apply_spoken_marks`: the English spoken-punctuation table ("comma",
    /// "new paragraph").
    SpokenMarks,
    /// `format_email_shape`: English greetings and sign-offs ("hi", "thanks").
    EmailShape,
    /// `structure_text`: spoken-list cues are English words.
    Structure,
    /// `insert_paragraphs`: breaks at the speaker's own pauses — timing only.
    Paragraphs,
    /// `strip_trailing_period`: the tone dial's trailing-period rule —
    /// punctuation only.
    TrailingPeriod,
    /// The local-LLM polish: its prompt and validator are English, and a small
    /// model told to "tidy" German in English is liable to translate it.
    Polish,
}

impl CleanupStage {
    pub const ALL: [CleanupStage; 9] = [
        CleanupStage::Dictionary,
        CleanupStage::Backtrack,
        CleanupStage::ListFormat,
        CleanupStage::SpokenMarks,
        CleanupStage::EmailShape,
        CleanupStage::Structure,
        CleanupStage::Paragraphs,
        CleanupStage::TrailingPeriod,
        CleanupStage::Polish,
    ];

    /// True when the stage has nothing English-specific in it.
    pub fn language_agnostic(self) -> bool {
        matches!(
            self,
            CleanupStage::Dictionary | CleanupStage::Paragraphs | CleanupStage::TrailingPeriod
        )
    }

    /// Whether the stage runs for a take in `language`.
    pub fn runs_for(self, language: &str) -> bool {
        self.language_agnostic() || is_english(language)
    }
}

/// The picker's view: prefs, what the model offers, and what to tell the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageOptions {
    pub prefs: LanguagePrefs,
    /// `(code, name)` pairs the picker offers: [`PICKER_LANGUAGES`] narrowed to
    /// what the model can transcribe.
    pub offered: Vec<LanguageChoice>,
    /// `None` when no model is loaded or downloaded, so the probe has not run.
    pub supports_detect: Option<bool>,
    pub mode: DecodeMode,
    pub notice: Option<String>,
    pub max_set: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageChoice {
    pub code: String,
    pub name: String,
}

/// Build the picker's view from the prefs and the probe (`None` = no model).
pub fn language_options(
    prefs: &LanguagePrefs,
    support: Option<&LanguageSupport>,
) -> LanguageOptions {
    let probe = support.cloned().unwrap_or_else(LanguageSupport::unknown);
    let offered = PICKER_LANGUAGES
        .iter()
        .filter(|(c, _)| probe.can_decode(c))
        .map(|(c, n)| LanguageChoice {
            code: (*c).to_string(),
            name: (*n).to_string(),
        })
        .collect();
    let plan = plan_decode(prefs, &probe);
    let notice = match support {
        Some(_) => plan.notice,
        None if prefs.auto && prefs.set.len() > 1 => Some(
            "Yap checks whether your speech model can detect languages once it is downloaded."
                .to_string(),
        ),
        None => None,
    };
    LanguageOptions {
        prefs: prefs.clone(),
        offered,
        supports_detect: support.map(|s| s.supports_detect),
        mode: plan.mode,
        notice,
        max_set: MAX_LANGUAGE_SET,
    }
}

// ---------------------------------------------------------------------------
// WF-12 — "Speaking a different language?"
// ---------------------------------------------------------------------------
//
// When a take is heard in a picker language the user has not selected, the
// pill offers it: "Sounded like Spanish — Add Spanish?" with Add and Not now.
// The DISCIPLINE is the feature: a language is offered at most once per
// [`LANGUAGE_OFFER_QUIET_DAYS`], whatever the answer, and the ledger that
// enforces it is one `settings_kv` row so it survives a relaunch. PRIVACY: the
// ledger holds language codes and timestamps only, never text.

/// The pill event that raises the card (payload: [`LanguageOffer`]).
pub const LANGUAGE_OFFER_EVENT: &str = "language_offer";

/// The `settings_kv` key of the offer ledger.
pub const LANGUAGE_OFFER_LEDGER_KEY: &str = "language_offer_ledger_v1";

/// A language is offered at most once in this many days; Not now silences it
/// for the same span.
pub const LANGUAGE_OFFER_QUIET_DAYS: i64 = 7;

/// [`LANGUAGE_OFFER_QUIET_DAYS`] in milliseconds.
pub const LANGUAGE_OFFER_QUIET_MS: i64 = LANGUAGE_OFFER_QUIET_DAYS * 24 * 60 * 60 * 1000;

/// The picker language a take was heard in, when it is NOT in the user's set.
/// `None` for no detection, a code outside [`PICKER_LANGUAGES`] (the picker
/// could not add it anyway), or a language already in the set.
pub fn outside_language(prefs: &LanguagePrefs, detected: Option<&str>) -> Option<String> {
    let code = normalize_code(detected?)?;
    (!prefs.set.contains(&code)).then_some(code)
}

/// When each language was last offered (or silenced), epoch ms by code.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageOfferLedger {
    #[serde(default)]
    pub last_offered_ms: std::collections::BTreeMap<String, i64>,
}

impl LanguageOfferLedger {
    /// Parse the stored row; a missing or unreadable row is an empty ledger.
    pub fn load(raw: Option<&str>) -> Self {
        raw.and_then(|r| serde_json::from_str::<Self>(r).ok())
            .unwrap_or_default()
    }

    /// True while `code` is inside its quiet window at `now_ms`.
    pub fn is_quiet(&self, code: &str, now_ms: i64) -> bool {
        self.last_offered_ms
            .get(code)
            .is_some_and(|at| now_ms.saturating_sub(*at) < LANGUAGE_OFFER_QUIET_MS)
    }

    /// Start (or restart) `code`'s quiet window at `now_ms`.
    pub fn record(&mut self, code: &str, now_ms: i64) {
        self.last_offered_ms.insert(code.to_string(), now_ms);
    }
}

/// What the pill's card shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageOffer {
    /// ISO 639-1 code, a [`PICKER_LANGUAGES`] member.
    pub code: String,
    /// Its English name ("Spanish").
    pub name: String,
    /// "Sounded like Spanish — Add Spanish?"
    pub headline: String,
}

impl LanguageOffer {
    pub fn new(code: &str) -> Self {
        let name = language_name(code);
        Self {
            code: code.to_string(),
            headline: format!("Sounded like {name} — Add {name}?"),
            name,
        }
    }
}

/// Decide whether a take's outside language is offered now. Never when the
/// set is full (Add could not keep its word), never inside the quiet window.
/// Records the offer in the ledger when it returns `Some`, so a second take in
/// the same language within the window offers nothing.
pub fn offer_for(
    prefs: &LanguagePrefs,
    outside: Option<&str>,
    ledger: &mut LanguageOfferLedger,
    now_ms: i64,
) -> Option<LanguageOffer> {
    let code = normalize_code(outside?)?;
    if prefs.set.contains(&code) || prefs.set.len() >= MAX_LANGUAGE_SET {
        return None;
    }
    if ledger.is_quiet(&code, now_ms) {
        return None;
    }
    ledger.record(&code, now_ms);
    Some(LanguageOffer::new(&code))
}

/// The prefs after Add: `code` joins the set (after the existing members, the
/// primary unchanged) and auto-detect turns on, because a two-language set
/// without it would never decode the new language.
pub fn prefs_with_added(prefs: &LanguagePrefs, code: &str) -> LanguagePrefs {
    let mut set = prefs.set.clone();
    if let Some(c) = normalize_code(code) {
        if !set.contains(&c) {
            set.push(c);
        }
    }
    LanguagePrefs::from_settings(&prefs.primary, &set, true)
}

/// [`offer_for`] against the persisted ledger. A ledger write failure is
/// logged and the offer withheld: an offer whose quiet window cannot be kept
/// would come back on every take.
pub fn take_offer(
    db: &crate::db::Database,
    prefs: &LanguagePrefs,
    outside: Option<&str>,
    now_ms: i64,
) -> Option<LanguageOffer> {
    outside?;
    let mut ledger =
        LanguageOfferLedger::load(db.setting_get(LANGUAGE_OFFER_LEDGER_KEY).as_deref());
    let offer = offer_for(prefs, outside, &mut ledger, now_ms)?;
    let json = serde_json::to_string(&ledger).ok()?;
    if let Err(e) = db.setting_set(LANGUAGE_OFFER_LEDGER_KEY, &json) {
        log::warn!(
            "languages offer_ledger_write_failed code={} err={e}",
            offer.code
        );
        return None;
    }
    log::info!("languages offer_shown code={}", offer.code);
    Some(offer)
}

/// Not now: restart `code`'s quiet window from the click.
pub fn snooze_offer(db: &crate::db::Database, code: &str, now_ms: i64) -> Result<(), String> {
    let code = normalize_code(code).ok_or_else(|| format!("unknown language {code:?}"))?;
    let mut ledger =
        LanguageOfferLedger::load(db.setting_get(LANGUAGE_OFFER_LEDGER_KEY).as_deref());
    ledger.record(&code, now_ms);
    let json = serde_json::to_string(&ledger).map_err(|e| e.to_string())?;
    db.setting_set(LANGUAGE_OFFER_LEDGER_KEY, &json)
}

#[cfg(test)]
mod wf12_offer_tests {
    use super::*;

    const DAY: i64 = 24 * 60 * 60 * 1000;

    fn english_only() -> LanguagePrefs {
        LanguagePrefs::from_settings("en", &[], false)
    }

    #[test]
    fn outside_language_is_a_picker_code_not_in_the_set() {
        let prefs = english_only();
        assert_eq!(outside_language(&prefs, Some("ES")), Some("es".to_string()));
        assert_eq!(outside_language(&prefs, Some("en")), None);
        assert_eq!(
            outside_language(&prefs, Some("cy")),
            None,
            "not in the picker"
        );
        assert_eq!(outside_language(&prefs, None), None);
    }

    #[test]
    fn offer_headline_names_the_language_twice() {
        let o = LanguageOffer::new("es");
        assert_eq!(o.name, "Spanish");
        assert_eq!(o.headline, "Sounded like Spanish — Add Spanish?");
    }

    #[test]
    fn offered_at_most_once_per_language_per_seven_days() {
        let prefs = english_only();
        let mut ledger = LanguageOfferLedger::default();
        let t0 = 1_000_000;
        assert!(offer_for(&prefs, Some("es"), &mut ledger, t0).is_some());
        // Every take in the next 7 days: nothing.
        for at in [
            t0 + 1,
            t0 + DAY,
            t0 + 6 * DAY,
            t0 + LANGUAGE_OFFER_QUIET_MS - 1,
        ] {
            assert!(
                offer_for(&prefs, Some("es"), &mut ledger, at).is_none(),
                "at {at}"
            );
        }
        // A different language has its own window.
        assert!(offer_for(&prefs, Some("fr"), &mut ledger, t0 + DAY).is_some());
        // Seven days on, Spanish may be offered again — once.
        let t7 = t0 + LANGUAGE_OFFER_QUIET_MS;
        assert!(offer_for(&prefs, Some("es"), &mut ledger, t7).is_some());
        assert!(offer_for(&prefs, Some("es"), &mut ledger, t7 + 1).is_none());
    }

    #[test]
    fn not_now_silences_the_language_for_seven_days_from_the_click() {
        let prefs = english_only();
        let mut ledger = LanguageOfferLedger::default();
        assert!(offer_for(&prefs, Some("es"), &mut ledger, 0).is_some());
        // The user answers Not now three days later: the window restarts.
        ledger.record("es", 3 * DAY);
        assert!(offer_for(&prefs, Some("es"), &mut ledger, 9 * DAY).is_none());
        assert!(offer_for(&prefs, Some("es"), &mut ledger, 10 * DAY).is_some());
    }

    #[test]
    fn the_ledger_survives_a_round_trip_and_bad_rows_are_empty() {
        let mut ledger = LanguageOfferLedger::default();
        ledger.record("es", 42);
        let json = serde_json::to_string(&ledger).unwrap();
        let back = LanguageOfferLedger::load(Some(&json));
        assert_eq!(back, ledger);
        assert!(back.is_quiet("es", 43));
        assert_eq!(
            LanguageOfferLedger::load(Some("not json")),
            LanguageOfferLedger::default()
        );
        assert_eq!(
            LanguageOfferLedger::load(None),
            LanguageOfferLedger::default()
        );
    }

    #[test]
    fn no_offer_for_a_language_in_the_set_or_a_full_set() {
        let mut ledger = LanguageOfferLedger::default();
        let two = LanguagePrefs::from_settings("en", &["en".into(), "es".into()], true);
        assert!(offer_for(&two, Some("es"), &mut ledger, 0).is_none());
        let full = LanguagePrefs::from_settings(
            "en",
            &["en".into(), "es".into(), "fr".into(), "de".into()],
            true,
        );
        assert_eq!(full.set.len(), MAX_LANGUAGE_SET);
        assert!(offer_for(&full, Some("it"), &mut ledger, 0).is_none());
        assert!(
            ledger.last_offered_ms.is_empty(),
            "a withheld offer records nothing"
        );
    }

    #[test]
    fn add_appends_the_language_and_turns_auto_on() {
        let prefs = english_only();
        let next = prefs_with_added(&prefs, "es");
        assert_eq!(next.primary, "en");
        assert_eq!(next.set, vec!["en".to_string(), "es".to_string()]);
        assert!(next.auto);
        // The picker then offers a choice, and auto-detect may pick Spanish.
        let support = LanguageSupport {
            supports_detect: true,
            languages: Vec::new(),
        };
        let plan = plan_decode(&next, &support);
        assert_eq!(plan.mode, DecodeMode::AutoWithinSet);
        assert!(plan.candidates.contains(&"es".to_string()));
        let opts = language_options(&next, Some(&support));
        assert_eq!(opts.prefs.set, next.set);
        // Adding twice is a no-op.
        assert_eq!(prefs_with_added(&next, "es").set, next.set);
    }
}
