//! Y9-C — spoken preference rules, with an explicit Apply step.
//!
//! P1 #10 acceptance (L): "rule with `mode:email` alters the email fixture
//! output and leaves the chat fixture unchanged." Plus the item's guards: an
//! unparseable rule is rejected and not stored, a filter outside the allowlist
//! is rejected, nothing takes effect before Apply, retracting stops the effect
//! and keeps the append-only history, and zero active rules sends the
//! byte-identical polish request.
//!
//! The model is a scripted [`PolishClient`]: for a derivation it returns the
//! answer under test; for a rewrite it FOLLOWS the rules it was sent (it turns
//! `!` into `.` only when a rule about exclamation marks is on the request). So
//! every assertion about output is really an assertion about which rules reached
//! which request — the real seam this item adds — with no model on the machine.

use std::path::PathBuf;
use std::sync::Mutex;

use wilson_voice_lib::db::Database;
use wilson_voice_lib::dictation::DictationMode;
use wilson_voice_lib::polish::{self, PolishClient, PolishConfig, PolishError};
use wilson_voice_lib::polish_protocol::{
    max_out_for, PolishRequest, KIND_DERIVE_PREFERENCE, KIND_POLISH, MAX_PREFERENCE_RULES,
    MAX_PREFERENCE_TOTAL_CHARS,
};
use wilson_voice_lib::voice_prefs::{
    self, PreferenceError, PreferenceFilter, TakeContext, VoicePreference, DERIVE_GRAMMAR,
};

/// The email fixture: an email-mode take with the exclamation marks a
/// "never use exclamation marks in email" rule exists to remove.
const EMAIL_FIXTURE: &str =
    "Hi Sam! Thanks so much for sending the deck over this morning! It looks great.";
/// The chat fixture: the same punctuation, in chat — where that rule must NOT apply.
const CHAT_FIXTURE: &str = "that is amazing! see you at the coffee place around five then!";

const SPOKEN: &str = "never use exclamation marks in email";

/// A scripted model. Derivations answer `derive_answer`; rewrites follow the
/// rules carried on the request. Every request is recorded.
struct RuleFollowingModel {
    derive_answer: String,
    seen: Mutex<Vec<PolishRequest>>,
}

impl RuleFollowingModel {
    fn answering(derive_answer: &str) -> Self {
        Self {
            derive_answer: derive_answer.to_string(),
            seen: Mutex::new(Vec::new()),
        }
    }
    fn last_polish(&self) -> PolishRequest {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|r| r.kind == KIND_POLISH)
            .cloned()
            .expect("a polish request was sent")
    }
}

impl PolishClient for RuleFollowingModel {
    fn rewrite(&self, req: &PolishRequest) -> Result<String, PolishError> {
        self.seen.lock().unwrap().push(req.clone());
        if req.kind == KIND_DERIVE_PREFERENCE {
            return Ok(self.derive_answer.clone());
        }
        let no_bangs = req
            .preferences
            .iter()
            .any(|r| r.to_lowercase().contains("exclamation"));
        Ok(if no_bangs {
            req.text.replace('!', ".")
        } else {
            req.text.clone()
        })
    }
}

fn email_rule_model() -> RuleFollowingModel {
    RuleFollowingModel::answering(
        r#"{"filter":"mode:email","rule":"Never use exclamation marks."}"#,
    )
}

fn cfg() -> PolishConfig {
    PolishConfig {
        model: "qwen2.5-0.5b-instruct-q4_k_m".to_string(),
        ..PolishConfig::default()
    }
}

fn take(mode: DictationMode) -> TakeContext<'static> {
    TakeContext {
        mode,
        bundle_id: "com.apple.mail",
        language: "en",
    }
}

/// The polish stage for one fixture, exactly as the live path wires it: the
/// DB's matching rules onto the config, then the stage against the model.
fn polish_take(
    db: &Database,
    model: &RuleFollowingModel,
    text: &str,
    mode: DictationMode,
) -> String {
    let cfg = cfg().with_preferences(db.matching_voice_preferences(&take(mode)));
    polish::polish_stage(text, mode, &cfg, model).unwrap_or_else(|| text.to_string())
}

fn history(db: &Database) -> Vec<VoicePreference> {
    db.list_voice_preferences().expect("history reads")
}

fn temp_db_path(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "yap-y9c-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("history.db")
}

// ── The acceptance, verbatim ────────────────────────────────────────────────

#[test]
fn a_mode_email_rule_alters_email_and_leaves_chat_unchanged() {
    let db = Database::open_in_memory().unwrap();
    let model = email_rule_model();

    let email_before = polish_take(&db, &model, EMAIL_FIXTURE, DictationMode::Email);
    let chat_before = polish_take(&db, &model, CHAT_FIXTURE, DictationMode::Chat);
    assert_eq!(
        email_before, EMAIL_FIXTURE,
        "no rule yet: the email fixture is untouched"
    );

    let rule = voice_prefs::propose(&db, SPOKEN, &model, 5_000).expect("derives");
    assert_eq!(rule.filter, "mode:email");
    assert_eq!(rule.rule, "Never use exclamation marks.");
    db.apply_voice_preference(&rule.id).expect("applies");

    let email_after = polish_take(&db, &model, EMAIL_FIXTURE, DictationMode::Email);
    assert_ne!(
        email_after, email_before,
        "a mode:email rule must alter the email fixture"
    );
    assert!(!email_after.contains('!'), "{email_after:?}");
    assert_eq!(
        model.last_polish().preferences,
        vec!["Never use exclamation marks.".to_string()]
    );

    let chat_after = polish_take(&db, &model, CHAT_FIXTURE, DictationMode::Chat);
    assert_eq!(
        chat_after, chat_before,
        "the chat fixture must be unchanged"
    );
    assert!(chat_after.contains('!'));
    assert!(
        model.last_polish().preferences.is_empty(),
        "the email rule must not even reach the chat request"
    );
}

// ── Derivation gates ────────────────────────────────────────────────────────

#[test]
fn an_unparseable_rule_is_rejected_and_not_stored() {
    let db = Database::open_in_memory().unwrap();
    for answer in [
        "Sure! I'll remember that you don't like exclamation marks.",
        r#"{"error":"unclear"}"#,
        r#"{"filter":"mode:email"}"#,
        r#"{"filter":"mode:email","rule":""}"#,
        r#"{"filter":"mode:email","rule":"no"}"#,
        r#"{"filter":"mode:email","rule":42}"#,
        "```json\n{\"filter\": \"mode:email\", \"rule\": \n```",
        // Well-formed but about nothing that was said — the real 1.5B model's
        // answer to "banana telephone purple" was the prompt's own example.
        r#"{"filter":"mode:email","rule":"Never use exclamation marks."}"#,
    ] {
        let model = RuleFollowingModel::answering(answer);
        let err = voice_prefs::propose(&db, "banana telephone", &model, 5_000)
            .expect_err(&format!("{answer:?} must be rejected"));
        assert_eq!(
            err,
            PreferenceError::Unparseable("banana telephone".to_string()),
            "{answer:?}"
        );
        // Shown back to the user: the message names what was heard.
        assert!(err.to_string().contains("banana telephone"));
    }
    assert!(
        history(&db).is_empty(),
        "a rejected derivation must store nothing"
    );

    // Empty input never reaches the model at all.
    let model = email_rule_model();
    assert_eq!(
        voice_prefs::propose(&db, "   ", &model, 5_000),
        Err(PreferenceError::Empty)
    );
    assert!(model.seen.lock().unwrap().is_empty());

    // A fenced but well-formed answer is fine — the model ignoring "ONLY" is not a reason to lose the rule.
    let fenced = RuleFollowingModel::answering(
        "```json\n{\"filter\": \"mode:email\", \"rule\": \"Never use exclamation marks.\"}\n```",
    );
    assert!(voice_prefs::propose(&db, SPOKEN, &fenced, 5_000).is_ok());
    // The derivation request is the dedicated kind, carries the grammar, and is never a rewrite.
    let sent = fenced.seen.lock().unwrap()[0].clone();
    assert_eq!(sent.kind, KIND_DERIVE_PREFERENCE);
    assert_eq!(sent.text, SPOKEN);
    assert_eq!(sent.grammar.as_deref(), Some(DERIVE_GRAMMAR));
}

#[test]
fn a_filter_outside_the_allowlist_is_rejected() {
    let db = Database::open_in_memory().unwrap();
    for filter in [
        "site:gmail.com",
        "mode:code",
        "mode:poetry",
        "app:gmail",
        "app:../../etc/passwd",
        "app:com.apple.mail; DROP TABLE voice_preferences",
        "lang:english",
        "lang:",
        "everywhere",
        "",
    ] {
        let answer = format!(
            r#"{{"filter":{},"rule":"Never use exclamation marks."}}"#,
            serde_json::json!(filter)
        );
        let model = RuleFollowingModel::answering(&answer);
        let err = voice_prefs::propose(&db, SPOKEN, &model, 5_000).expect_err(filter);
        assert!(
            matches!(err, PreferenceError::FilterNotAllowed(_)),
            "{filter:?} -> {err:?}"
        );
        // The storage boundary refuses it too, whoever calls it.
        let derived = voice_prefs::DerivedPreference {
            filter: filter.to_string(),
            rule: "Never use exclamation marks.".to_string(),
        };
        assert!(
            db.add_voice_preference(SPOKEN, &derived).is_err(),
            "{filter:?}"
        );
    }
    assert!(
        history(&db).is_empty(),
        "nothing outside the allowlist is stored"
    );

    // Every allowlisted shape parses, canonically.
    for (raw, canonical) in [
        ("*", "*"),
        ("mode:email", "mode:email"),
        ("mode:LIST", "mode:list"),
        (
            "app:com.tinyspeck.slackmacgap",
            "app:com.tinyspeck.slackmacgap",
        ),
        ("lang:DE", "lang:de"),
    ] {
        assert_eq!(PreferenceFilter::parse(raw).unwrap().canonical(), canonical);
    }

    // A row that somehow carries a bad filter never reaches a prompt: the
    // allowlist is re-checked at the prompt boundary, not only at insert.
    let smuggled = VoicePreference {
        id: "x".into(),
        spoken: SPOKEN.into(),
        filter: "site:gmail.com".into(),
        rule: "Never use exclamation marks.".into(),
        created_at: chrono::Utc::now(),
        active: true,
        applied_at: Some(chrono::Utc::now()),
        retracted_at: None,
    };
    assert!(voice_prefs::matching_rules(&[smuggled], &take(DictationMode::Email)).is_empty());
}

// ── Apply / Retract ─────────────────────────────────────────────────────────

#[test]
fn no_rule_takes_effect_before_apply() {
    let db = Database::open_in_memory().unwrap();
    let model = email_rule_model();
    let pending = voice_prefs::propose(&db, SPOKEN, &model, 5_000).expect("derives");
    assert!(!pending.active);
    assert!(pending.applied_at.is_none());
    // Stored — the user can see what Yap heard — but inert.
    assert_eq!(history(&db), vec![pending.clone()]);
    assert!(db
        .matching_voice_preferences(&take(DictationMode::Email))
        .is_empty());
    assert_eq!(
        polish_take(&db, &model, EMAIL_FIXTURE, DictationMode::Email),
        EMAIL_FIXTURE
    );
    assert!(model.last_polish().preferences.is_empty());

    let applied = db.apply_voice_preference(&pending.id).expect("Apply");
    assert!(applied.active && applied.applied_at.is_some());
    assert_ne!(
        polish_take(&db, &model, EMAIL_FIXTURE, DictationMode::Email),
        EMAIL_FIXTURE
    );
    // Apply is one-shot.
    assert_eq!(
        db.apply_voice_preference(&pending.id),
        Err(PreferenceError::NotFound)
    );
    assert_eq!(
        db.apply_voice_preference("no-such-id"),
        Err(PreferenceError::NotFound)
    );
}

#[test]
fn retract_leaves_the_append_only_history() {
    let path = temp_db_path("retract");
    let db = Database::open(path.clone()).unwrap();
    let model = email_rule_model();
    let rule = voice_prefs::propose(&db, SPOKEN, &model, 5_000).unwrap();
    db.apply_voice_preference(&rule.id).unwrap();
    assert_ne!(
        polish_take(&db, &model, EMAIL_FIXTURE, DictationMode::Email),
        EMAIL_FIXTURE
    );

    let retracted = db.retract_voice_preference(&rule.id).expect("Retract");
    assert!(!retracted.active);
    assert!(retracted.retracted_at.is_some());
    assert_eq!(
        retracted.applied_at,
        db.list_voice_preferences().unwrap()[0].applied_at
    );

    // The effect stops…
    assert_eq!(
        polish_take(&db, &model, EMAIL_FIXTURE, DictationMode::Email),
        EMAIL_FIXTURE
    );
    assert!(model.last_polish().preferences.is_empty());
    // …and the history stays: same row, what was said intact.
    let rows = history(&db);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, rule.id);
    assert_eq!(rows[0].spoken, SPOKEN);
    assert_eq!(rows[0].rule, "Never use exclamation marks.");
    // A retracted rule stays retracted; saying it again appends a NEW row.
    assert_eq!(
        db.apply_voice_preference(&rule.id),
        Err(PreferenceError::NotFound)
    );
    assert_eq!(
        db.retract_voice_preference(&rule.id),
        Err(PreferenceError::NotFound)
    );
    let again = voice_prefs::propose(&db, SPOKEN, &model, 5_000).unwrap();
    assert_ne!(again.id, rule.id);
    assert_eq!(history(&db).len(), 2);

    // Append-only is enforced by SQLite itself, not only by the methods above.
    drop(db);
    let raw = rusqlite::Connection::open(&path).unwrap();
    assert!(
        raw.execute("DELETE FROM voice_preferences", []).is_err(),
        "DELETE must be refused"
    );
    assert!(
        raw.execute("UPDATE voice_preferences SET rule = 'Always shout.'", [])
            .is_err(),
        "rewriting what was derived must be refused"
    );
    assert!(
        raw.execute("UPDATE voice_preferences SET spoken = 'something else'", [])
            .is_err(),
        "rewriting what was said must be refused"
    );
    assert!(
        raw.execute(
            "UPDATE voice_preferences SET active = 1, retracted_at = NULL WHERE id = ?1",
            [&rule.id],
        )
        .is_err(),
        "a retracted rule must not be switched back on behind the history's back"
    );
    let count: i64 = raw
        .query_row("SELECT COUNT(*) FROM voice_preferences", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2);
    drop(raw);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

// ── The Y9-B guard: no rules ⇒ the byte-identical request ───────────────────

#[test]
fn zero_active_rules_is_a_byte_identical_request() {
    let text = "hey sam just checking in on the deck for thursday";
    let line = |cfg: &PolishConfig| {
        let mut req = polish::build_request(text, DictationMode::Email, cfg).expect("stage on");
        req.id = 0;
        serde_json::to_string(&req).unwrap()
    };
    // The exact pre-Y9-C wire line — no `preferences` key at all.
    let golden = format!(
        r#"{{"id":0,"kind":"polish","mode":"email","style":"default","max_out":{},"deadline_ms":{},"text":"{text}","topic":null}}"#,
        max_out_for(text),
        polish::DEFAULT_POLISH_DEADLINE_MS,
    );
    assert_eq!(line(&cfg()), golden);
    assert_eq!(line(&cfg().with_preferences(Vec::new())), golden);

    // Pending, retracted and non-matching rules are all "zero active rules".
    let db = Database::open_in_memory().unwrap();
    let model = email_rule_model();
    let pending = voice_prefs::propose(&db, SPOKEN, &model, 5_000).unwrap();
    assert_eq!(
        line(&cfg().with_preferences(db.matching_voice_preferences(&take(DictationMode::Email)))),
        golden
    );
    db.apply_voice_preference(&pending.id).unwrap();
    let chat = db.matching_voice_preferences(&take(DictationMode::Chat));
    let mut chat_req =
        polish::build_request(text, DictationMode::Chat, &cfg().with_preferences(chat)).unwrap();
    chat_req.id = 0;
    let mut chat_plain = polish::build_request(text, DictationMode::Chat, &cfg()).unwrap();
    chat_plain.id = 0;
    assert_eq!(
        serde_json::to_string(&chat_req).unwrap(),
        serde_json::to_string(&chat_plain).unwrap()
    );
    db.retract_voice_preference(&pending.id).unwrap();
    assert_eq!(
        line(&cfg().with_preferences(db.matching_voice_preferences(&take(DictationMode::Email)))),
        golden
    );

    // And an active matching rule DOES change the line — the guard is not vacuous.
    let live = voice_prefs::propose(&db, SPOKEN, &model, 5_000).unwrap();
    db.apply_voice_preference(&live.id).unwrap();
    let with =
        line(&cfg().with_preferences(db.matching_voice_preferences(&take(DictationMode::Email))));
    assert_ne!(with, golden);
    assert!(
        with.contains(r#""preferences":["Never use exclamation marks."]"#),
        "{with}"
    );
}

// ── Matching and the budget ─────────────────────────────────────────────────

#[test]
fn rules_match_by_app_and_language_and_stay_inside_the_budget() {
    let db = Database::open_in_memory().unwrap();
    let apply = |answer: &str| {
        let model = RuleFollowingModel::answering(answer);
        // Spoken = the rule itself, so the grounding check holds by construction.
        let value: serde_json::Value = serde_json::from_str(answer).unwrap();
        let spoken = value["rule"].as_str().unwrap().to_string();
        let row = voice_prefs::propose(&db, &spoken, &model, 5_000).unwrap();
        db.apply_voice_preference(&row.id).unwrap();
    };
    apply(r#"{"filter":"app:com.tinyspeck.slackmacgap","rule":"Sound relaxed in Slack."}"#);
    apply(r#"{"filter":"lang:de","rule":"Never use the word awesome."}"#);
    apply(r#"{"filter":"*","rule":"Spell it colour, not color."}"#);

    let slack_en = TakeContext {
        mode: DictationMode::Chat,
        bundle_id: "com.tinyspeck.slackmacgap",
        language: "en",
    };
    let mail_de = TakeContext {
        mode: DictationMode::Email,
        bundle_id: "com.apple.mail",
        language: "de-DE",
    };
    let unknown = TakeContext {
        mode: DictationMode::Notes,
        bundle_id: "",
        language: "en",
    };
    let got = |t: &TakeContext<'_>| {
        let mut v = db.matching_voice_preferences(t);
        v.sort();
        v
    };
    assert_eq!(
        got(&slack_en),
        vec!["Sound relaxed in Slack.", "Spell it colour, not color."]
    );
    assert_eq!(
        got(&mail_de),
        vec!["Never use the word awesome.", "Spell it colour, not color."]
    );
    assert_eq!(got(&unknown), vec!["Spell it colour, not color."]);

    // Forty everywhere-rules: the request carries at most the budget.
    for i in 0..40 {
        apply(&format!(
            r#"{{"filter":"*","rule":"Rule number {i} is a fairly long rule that the speaker set once and forgot."}}"#
        ));
    }
    let rules = db.matching_voice_preferences(&unknown);
    assert!(rules.len() <= MAX_PREFERENCE_RULES, "{}", rules.len());
    assert!(rules.iter().map(|r| r.chars().count()).sum::<usize>() <= MAX_PREFERENCE_TOTAL_CHARS);
    let req = polish::build_request(
        "hey sam just checking in on the deck for thursday",
        DictationMode::Notes,
        &cfg().with_preferences(rules.clone()),
    )
    .unwrap();
    assert_eq!(req.preferences, rules);
    // The most recently applied rule always makes the cut.
    assert!(
        rules.iter().any(|r| r.starts_with("Rule number 39 ")),
        "{rules:?}"
    );
}
