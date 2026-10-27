//! Y9-B — writing samples become a local style profile.
//!
//! The item's acceptance, as tests:
//!   * ZERO samples ⇒ the built polish request is BYTE-IDENTICAL to the
//!     pre-Y9-B request, asserted against the serialized wire line, so the
//!     whole formatting corpus sees exactly the prompt it always saw;
//!   * ONE sample ⇒ it is in the request, and in the system turn the sidecar
//!     builds from it;
//!   * over-budget samples are truncated DETERMINISTICALLY, oldest dropped;
//!   * samples never appear in a support bundle.
//!
//! Every sample in this file is hand-written synthetic text that lives OUTSIDE
//! `src/` on purpose: the redaction allowlist is this crate's own string
//! literals, so a sample written into a source file would become text the
//! redactor is taught to let through.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use wilson_voice_lib::db::Database;
use wilson_voice_lib::dictation::DictationMode;
use wilson_voice_lib::polish::{build_request, PolishConfig, StyleProfile};
use wilson_voice_lib::polish_protocol::{
    fit_style_samples, max_out_for, style_block, style_profile_tokens, PolishRequest,
    MAX_CUSTOM_RULES_WORDS, MAX_STYLE_SAMPLES, MAX_STYLE_SAMPLE_WORDS_TOTAL,
    MIN_STYLE_SAMPLE_WORDS,
};
use wilson_voice_lib::support::{self, BundleInputs};

const TEXT: &str = "hey jordan quick update the build is green and we ship friday";

/// A sample long enough to pass the save-time guard, with words no source
/// literal carries — so finding any of them in a bundle means it leaked.
const SAMPLE: &str = "Hi Priya, attaching the revised quarterly forecast for the Lisbon \
                      office. Shout if the numbers look off before Thursday's board call, \
                      otherwise I'll circulate it Monday. Best, Wilhelmina";
const SAMPLE_MARKERS: &[&str] = &["Priya", "Lisbon", "quarterly forecast", "Wilhelmina"];

fn cfg() -> PolishConfig {
    PolishConfig {
        model: "qwen2.5-1.5b-instruct-q4_k_m".to_string(),
        ..PolishConfig::default()
    }
}

/// The wire line with the per-process request id pinned, so two requests can
/// be compared byte for byte.
fn wire(mut req: PolishRequest) -> String {
    req.id = 0;
    serde_json::to_string(&req).expect("request serializes")
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "yap-y9b-{tag}-{}-{}",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn open_db(dir: &Path) -> Database {
    Database::open(dir.join("history.db")).expect("db opens")
}

fn words(tag: &str, n: usize) -> String {
    (0..n)
        .map(|i| format!("{tag}{i}"))
        .collect::<Vec<_>>()
        .join(" ")
}

// ── the regression guard ────────────────────────────────────────────────────

/// The exact pre-Y9-B line for this input, written out by hand. If a profile
/// field ever leaks onto the wire with no profile set — a `null`, an empty
/// array, a reordered key — this literal stops matching.
#[test]
fn zero_samples_yields_a_byte_identical_request() {
    let expected = format!(
        r#"{{"id":0,"kind":"polish","mode":"email","style":"default","max_out":{},"deadline_ms":1200,"text":"{TEXT}","topic":null}}"#,
        max_out_for(TEXT)
    );
    let today = wire(build_request(TEXT, DictationMode::Email, &cfg()).expect("in band"));
    assert_eq!(today, expected);

    // An explicitly empty profile — no samples, blank rules, blank samples —
    // is the same bytes, not merely "equivalent".
    for profile in [
        StyleProfile::default(),
        StyleProfile::new(&[], ""),
        StyleProfile::new(&["   ".to_string(), String::new()], "  \n "),
    ] {
        assert!(profile.is_empty());
        let req = build_request(TEXT, DictationMode::Email, &cfg().with_profile(profile))
            .expect("in band");
        assert_eq!(wire(req), expected);
    }

    // And for every mode the corpus covers, with and without an empty profile.
    for mode in [
        DictationMode::Email,
        DictationMode::Document,
        DictationMode::Notes,
        DictationMode::Chat,
        DictationMode::Plain,
        DictationMode::List,
    ] {
        let a = wire(build_request(TEXT, mode, &cfg()).unwrap());
        let b = wire(
            build_request(TEXT, mode, &cfg().with_profile(StyleProfile::new(&[], ""))).unwrap(),
        );
        assert_eq!(a, b, "{mode:?}");
        assert!(!a.contains("samples") && !a.contains("rules"), "{a}");
    }
}

#[test]
fn one_sample_appears_in_the_request_and_the_system_turn() {
    let profile = StyleProfile::new(&[SAMPLE.to_string()], "Never use exclamation marks.");
    let req =
        build_request(TEXT, DictationMode::Email, &cfg().with_profile(profile)).expect("in band");
    assert_eq!(req.samples, vec![SAMPLE.to_string()]);
    assert_eq!(req.rules.as_deref(), Some("Never use exclamation marks."));
    let line = wire(req.clone());
    assert!(line.contains("Lisbon"), "{line}");

    // The sidecar appends exactly this to the system turn (yap-polish's
    // `writing_samples_reach_the_system_turn_and_only_when_present` asserts
    // the other end of the same wire).
    let block = style_block(&req.samples, req.rules.as_deref()).expect("a profile");
    assert!(block.contains(SAMPLE), "{block}");
    assert!(block.contains("Never use exclamation marks."));
    assert!(block.contains("Never copy a sample's content"));

    // The parsed line round-trips with the profile intact (the sidecar parses
    // the same struct from the same file).
    let back: PolishRequest = serde_json::from_str(&line).unwrap();
    assert_eq!(back.samples, vec![SAMPLE.to_string()]);

    // `Code` still never reaches a model, profile or not.
    assert!(build_request(
        TEXT,
        DictationMode::Code,
        &cfg().with_profile(StyleProfile::new(&[SAMPLE.to_string()], ""))
    )
    .is_none());
}

// ── the budget ──────────────────────────────────────────────────────────────

#[test]
fn over_budget_samples_are_truncated_deterministically_oldest_first() {
    // Five samples, oldest first, 100 words each: the count cap (3) and the
    // word budget (320) both say "keep the newest three".
    let five: Vec<String> = (0..5).map(|i| words(&format!("s{i}w"), 100)).collect();
    let kept = fit_style_samples(&five);
    assert_eq!(kept, five[2..].to_vec(), "the NEWEST three, oldest first");
    assert_eq!(kept, fit_style_samples(&five), "deterministic");

    // The walk STOPS at the first sample that does not fit: a short OLDER
    // sample is never kept in place of the one that was dropped.
    let big_newest = vec![
        words("tiny", 5),
        words("mid", 30),
        words("newest", MAX_STYLE_SAMPLE_WORDS_TOTAL - 10),
    ];
    assert_eq!(fit_style_samples(&big_newest), vec![big_newest[2].clone()]);

    // A newest sample that alone exceeds the budget drops everything: the
    // prompt never grows past the budget, and nothing is cut mid-sample.
    let too_big = vec![
        words("old", 40),
        words("huge", MAX_STYLE_SAMPLE_WORDS_TOTAL + 1),
    ];
    assert!(fit_style_samples(&too_big).is_empty());

    // Through the profile the request is built from — same answer.
    let profile = StyleProfile::new(&five, "");
    assert_eq!(profile.samples(), &five[2..]);
    let req = build_request(TEXT, DictationMode::Notes, &cfg().with_profile(profile)).unwrap();
    assert_eq!(req.samples.len(), MAX_STYLE_SAMPLES);
    assert!(!wire(req).contains("s0w0 "), "the oldest is gone");

    // Rules past the cap are dropped whole, never cut mid-sentence.
    let long_rules = words("rule", MAX_CUSTOM_RULES_WORDS + 1);
    assert_eq!(StyleProfile::new(&[], &long_rules).rules(), None);
}

/// The worst-case profile still fits the sidecar's default 4096-token context
/// beside the largest chunk the polish stage sends, on the same words × 1.3
/// proxy `max_out_for` uses — with half the context to spare for the base
/// rules and the chat template. The sidecar re-measures with its real
/// tokenizer and refuses (rules text kept) rather than cutting, so this is the
/// guard that keeps that refusal from being the common case.
#[test]
fn the_worst_case_profile_fits_the_polish_context() {
    let samples = vec![words("w", MAX_STYLE_SAMPLE_WORDS_TOTAL)];
    let rules = words("r", MAX_CUSTOM_RULES_WORDS);
    let chunk = words("c", wilson_voice_lib::polish::POLISH_CHUNK_WORDS + 24);
    let estimate = style_profile_tokens(&samples, Some(&rules))
        + (chunk.split_whitespace().count() as f32 * 1.3).ceil() as u32
        + max_out_for(&words("c", wilson_voice_lib::polish::POLISH_CHUNK_WORDS));
    assert!(estimate <= 4096 / 2, "{estimate} tokens");
}

// ── storage ─────────────────────────────────────────────────────────────────

#[test]
fn the_user_context_table_is_a_migration_and_reopening_is_idempotent() {
    let dir = temp_dir("migrate");
    {
        let db = open_db(&dir);
        assert_eq!(
            db.schema_version().unwrap(),
            wilson_voice_lib::meetings::SCHEMA_VERSION
        );
        db.add_writing_sample(SAMPLE).unwrap();
    }
    let db = open_db(&dir);
    assert_eq!(db.schema_version().unwrap(), wilson_voice_lib::meetings::SCHEMA_VERSION);
    let view = db.user_context().unwrap();
    assert_eq!(view.writing_samples.len(), 1, "the row survived a reopen");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_save_guards_refuse_noise_and_overflow() {
    let dir = temp_dir("guards");
    let db = open_db(&dir);
    let err = db.add_writing_sample("thanks, bye").unwrap_err();
    assert!(err.contains(&MIN_STYLE_SAMPLE_WORDS.to_string()), "{err}");
    assert!(db.user_context().unwrap().writing_samples.is_empty());

    for i in 0..MAX_STYLE_SAMPLES {
        db.add_writing_sample(&words(&format!("k{i}w"), MIN_STYLE_SAMPLE_WORDS))
            .unwrap();
    }
    assert!(db
        .add_writing_sample(&words("extra", MIN_STYLE_SAMPLE_WORDS))
        .is_err());
    let view = db.user_context().unwrap();
    assert!(view.writing_samples.iter().all(|s| s.in_use));
    assert_eq!(
        view.writing_samples[0].body,
        words("k0w", MIN_STYLE_SAMPLE_WORDS)
    );

    // Delete one; a sample over the remaining word budget is refused.
    db.delete_writing_sample(&view.writing_samples[0].id)
        .unwrap();
    assert!(db
        .add_writing_sample(&words("big", MAX_STYLE_SAMPLE_WORDS_TOTAL))
        .is_err());

    // Rules: set, read back, cap refused, empty clears.
    db.set_custom_rules("  Sign off with just my initial.  ")
        .unwrap();
    assert_eq!(db.custom_rules().unwrap(), "Sign off with just my initial.");
    assert!(db
        .set_custom_rules(&words("r", MAX_CUSTOM_RULES_WORDS + 1))
        .is_err());
    assert_eq!(db.custom_rules().unwrap(), "Sign off with just my initial.");

    // The profile a take is built with is the stored one, oldest first.
    let profile = db.style_profile();
    assert_eq!(profile.samples().len(), MAX_STYLE_SAMPLES - 1);
    assert_eq!(profile.samples()[0], words("k1w", MIN_STYLE_SAMPLE_WORDS));
    assert_eq!(profile.rules(), Some("Sign off with just my initial."));

    db.set_custom_rules("").unwrap();
    assert_eq!(db.custom_rules().unwrap(), "");
    let _ = std::fs::remove_dir_all(&dir);
}

// ── privacy ─────────────────────────────────────────────────────────────────

fn at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-27T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

/// The bundle is assembled the way `lib.rs` assembles it — logs read from a
/// logs dir with `support::read_logs`, the environment block from the real
/// builder — beside a database that HOLDS a sample. The bundle never reads the
/// database, and the log here is the worst case: a line that echoed a sample
/// verbatim and a line carrying a whole polish request with its samples. The
/// packer has to drop both.
#[test]
fn samples_never_appear_in_a_support_bundle() {
    let dir = temp_dir("bundle");
    let db = open_db(&dir);
    db.add_writing_sample(SAMPLE).unwrap();
    assert!(db.style_profile().samples()[0].contains("Lisbon"));

    let mut req = PolishRequest::polish(9, "email", "default", 48, 1200, TEXT.to_string());
    req.samples = vec![SAMPLE.to_string()];
    let logs_dir = dir.join("logs");
    std::fs::create_dir_all(&logs_dir).unwrap();
    std::fs::write(
        logs_dir.join("yap.log"),
        format!(
            "[2026-10-27T12:00:00Z INFO  wilson_voice_lib::db] style profile: writing sample saved (31 words)\n\
             [2026-10-27T12:00:01Z DEBUG wilson_voice_lib::polish] {SAMPLE}\n\
             [2026-10-27T12:00:01Z DEBUG wilson_voice_lib::polish] sent \"{SAMPLE}\"\n\
             [2026-10-27T12:00:02Z DEBUG wilson_voice_lib::polish] {}\n",
            serde_json::to_string(&req).unwrap()
        ),
    )
    .unwrap();
    let logs = support::read_logs(&logs_dir);
    assert!(
        logs.iter().any(|(_, raw)| raw.contains("Lisbon")),
        "the fixture really does leak before packing"
    );

    let entries = support::build_entries(&BundleInputs {
        crash_summary: "No crashes recorded.\n".into(),
        logs,
        environment: support::environment_block("macOS 26.5.2 (25F84)", "aarch64"),
        permissions: "accessibility: true\n".into(),
        models: support::models_block("small.en"),
        username: "wilsonguenther".into(),
        generated_at: at(),
    });
    for entry in &entries {
        for marker in SAMPLE_MARKERS {
            assert!(
                !entry.text.contains(marker),
                "{marker:?} survived into {}:\n{}",
                entry.name,
                entry.text
            );
        }
    }
    // Not vacuous: the packed log is there and its operational line survived.
    let log = entries
        .iter()
        .find(|e| e.name == "logs/yap.log")
        .expect("the log is packed");
    assert!(log.text.contains("writing sample saved"), "{}", log.text);
    let _ = std::fs::remove_dir_all(&dir);
}
