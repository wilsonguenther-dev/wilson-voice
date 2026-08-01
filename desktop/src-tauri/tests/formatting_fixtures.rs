//! YV59 golden formatting corpus + the polish-latency harness.
//!
//! `tests/fixtures/formatting/*.jsonl` is the scoreboard the LLM stage lands
//! against (`docs/research/wispr-formatting-deep-dive.md` §3, spec item YV58 —
//! "fixtures land before the LLM so the LLM has a scoreboard"). One JSON object
//! per line, one case per object, covering every rule R1–R14 and every measured
//! failure of §1.1 (`F1`–`F6`).
//!
//! Two kinds of assertion, deliberately different in strength:
//!
//! * **rules cases** assert an EXACT string. The rules stage is deterministic,
//!   so a golden is a byte-for-byte regression net.
//! * **LLM cases** assert SHAPE predicates only — a greeting line exists, a blank
//!   line exists, no trailing period, content words survived. A model bump
//!   changes wording; it must not be able to flake the corpus.
//!
//! `expect_rules` is therefore the output of the pipeline with the LLM stage
//! injected as a no-op, for EVERY case: it stays meaningful once a model is
//! installed, and it records — honestly, with a note — what the rules stage does
//! today for the rules that are specified but not yet built (R3/R4/R6, and the
//! email/tone work of the next item). Those cases carry `owner: "pending"`, and a
//! rules change that closes the gap fails `fixtures_rules_corpus_matches_expected`
//! loudly, which is exactly what a golden corpus is for.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::Deserialize;
use wilson_voice_lib::dictation::{self, CleanupLevel, DictationMode};
use wilson_voice_lib::polish;

// ---------------------------------------------------------------------------
// Corpus
// ---------------------------------------------------------------------------

/// Who is responsible for the case's FINAL, specified behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Owner {
    /// The rules stage already delivers it — `expect_rules` is the spec'd output
    /// and the shape predicates are asserted against it on every run.
    Rules,
    /// Needs whole-utterance semantics (§1 "rule/LLM split") — the rules stage
    /// leaves the input and the predicates are the target for the model.
    Llm,
    /// Specified as rules work but not built yet (R3/R4/R6/R13/R14). Same
    /// treatment as `Llm`; the distinction is documentation, so nobody reads the
    /// golden as "this is what the spec wants".
    Pending,
}

#[derive(Debug, Deserialize)]
struct Case {
    id: String,
    /// Rule ids (`R1`…`R14`) and measured-failure ids (`F1`…`F6`) this case covers.
    rules: Vec<String>,
    owner: Owner,
    mode: String,
    style: String,
    level: String,
    /// Text already before the caret (YV50), when the case exercises R5.
    #[serde(default)]
    context: Option<String>,
    #[serde(rename = "in")]
    input: String,
    expect_rules: String,
    /// Words the case RETRACTS on purpose (self-correction / restatement), which
    /// are therefore not required to survive. For `owner: "rules"` cases they are
    /// additionally asserted absent, so a stale declaration cannot hide.
    #[serde(default)]
    expect_drops: Vec<String>,
    expect_llm_shape: Vec<String>,
    #[serde(default)]
    note: String,
    /// Fixture file the case came from (filled in by the loader).
    #[serde(default)]
    source: String,
}

/// Every rule and measured failure the corpus has to cover.
const REQUIRED_COVERAGE: &[&str] = &[
    "R1", "R2", "R3", "R4", "R5", "R6", "R7", "R8", "R9", "R10", "R11", "R12", "R13", "R14", "F1",
    "F2", "F3", "F4", "F5", "F6",
];

const STYLES: &[&str] = &["very_casual", "casual", "default", "formal"];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/formatting")
}

/// Read every `*.jsonl` in the fixture directory. Reading the DIRECTORY (rather
/// than a hand-listed set of `include_str!`s) is deliberate: a new fixture file
/// is picked up by every test below the moment it is added.
fn corpus() -> Vec<Case> {
    let dir = fixtures_dir();
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|e| e.expect("readable dir entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no *.jsonl fixtures in {}", dir.display());

    let mut cases = Vec::new();
    for file in files {
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        let body = fs::read_to_string(&file).expect("fixture file is readable");
        for (n, line) in body.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let mut case: Case = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("{name}:{} is not a valid case: {e}", n + 1));
            case.source = format!("{name}:{}", n + 1);
            cases.push(case);
        }
    }
    cases
}

fn mode_of(case: &Case) -> DictationMode {
    match case.mode.as_str() {
        "email" => DictationMode::Email,
        "document" => DictationMode::Document,
        "notes" => DictationMode::Notes,
        "code" => DictationMode::Code,
        "chat" => DictationMode::Chat,
        "plain" => DictationMode::Plain,
        "list" => DictationMode::List,
        other => panic!("{}: unknown mode {other:?}", case.id),
    }
}

fn level_of(case: &Case) -> CleanupLevel {
    assert!(
        matches!(case.level.as_str(), "none" | "light" | "medium" | "high"),
        "{}: unknown cleanup level {:?}",
        case.id,
        case.level
    );
    CleanupLevel::from_setting(&case.level)
}

/// The pipeline exactly as `lib.rs` wires it — dictionary, backtrack, rules
/// formatting, polish, then the R5 context join — with the polish stage supplied
/// by the caller. The dictionary is the identity: user vocabulary is per-install
/// and is not what this corpus measures.
fn pipeline(case: &Case, polish: impl Fn(&str) -> Option<String>) -> String {
    let text = dictation::run_cleanup(
        &case.input,
        level_of(case),
        mode_of(case),
        |t| t.to_string(),
        polish,
    );
    dictation::join_with_context(&text, case.context.as_deref())
}

/// Rules-only output: the LLM stage injected as a no-op, which is what
/// `expect_rules` records for every case.
fn rules_output(case: &Case) -> String {
    pipeline(case, |_| None)
}

/// The production pipeline, including whatever the polish stage does on this
/// machine: YV61 wires `polish::polish_llm` to the take's mode and the polish
/// settings, so this is a no-op until a polish model is installed and selected
/// (`PolishConfig::default().model == ""`).
fn full_output(case: &Case) -> String {
    let mode = mode_of(case);
    let config = polish::PolishConfig::default();
    pipeline(case, |text| polish::polish_llm(text, mode, &config))
}

// ---------------------------------------------------------------------------
// Content-word retention (§2.5 V3) — the "never lose text" metric
// ---------------------------------------------------------------------------

/// Retention floor the pipeline must hold on every case, per §2.5 V3.
const RETENTION_FLOOR: f64 = 0.80;

/// Vocabulary the formatter is LICENSED to consume, and which is therefore not
/// required to survive: fillers and hedges (backtrack), self-correction markers,
/// the enumeration cue words that BECOME a list marker, the shape instruction,
/// and the spoken names of punctuation and line commands, which become glyphs.
/// Only words of ≥ 4 characters matter to the metric, but the list is written in
/// full so its provenance is readable.
const CONSUMABLE: &[&str] = &[
    // fillers / hedges (R "backtrack")
    "um", "uh", "er", "erm", "hmm", "like", "you", "know", "sort", "kind", "guess", "basically",
    "literally", "mean", "well",
    // self-correction markers
    "scratch", "that", "wait", "meant", "sorry", "rephrase", "actually", "instead",
    // enumeration cues (R7) — a spoken "one"/"first" becomes the "1." marker
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "first",
    "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth",
    "number", "next", "finally", "lastly",
    // shape instruction (R10)
    "bullet", "bullets", "point", "points",
    // spoken mark and line-command names (R1/R2)
    "period", "comma", "question", "mark", "exclamation", "colon", "semicolon", "ellipsis",
    "percent", "degree", "symbol", "sign", "quotation", "apostrophe", "dash", "asterisk",
    "ampersand", "slash", "backslash", "underscore", "tilde", "plus", "minus", "equals",
    "hashtag", "paren", "parenthesis", "angle", "bracket", "open", "close", "line", "paragraph",
    "skip", "start", "new",
];

/// Alphanumeric words of a string, lowercased. Splitting on everything else (not
/// just whitespace) is what makes the metric survive the glyphs the rules attach:
/// `notes_final` yields `notes` + `final`, `you!` yields `you`.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '\u{2019}')
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

fn count_of(words: &[String], needle: &str) -> usize {
    words.iter().filter(|w| *w == needle).count()
}

/// Content words the case REQUIRES to survive: ≥ 4 characters, case-folded, with
/// the consumable vocabulary and the case's own declared retractions removed.
fn required_words(case: &Case) -> Vec<String> {
    let dropped: Vec<String> = case.expect_drops.iter().map(|d| d.to_lowercase()).collect();
    let mut required: Vec<String> = words(&case.input)
        .into_iter()
        .filter(|w| w.chars().count() >= 4)
        .filter(|w| !CONSUMABLE.contains(&w.as_str()))
        .filter(|w| !dropped.contains(w))
        .collect();
    required.sort();
    required.dedup();
    required
}

/// Share of the required content words (counted with multiplicity, so a word
/// spoken twice must be kept twice) that survive into `output`.
fn retention(case: &Case, output: &str) -> f64 {
    let required = required_words(case);
    if required.is_empty() {
        return 1.0;
    }
    let spoken = words(&case.input);
    let kept = words(output);
    let (mut want, mut have) = (0usize, 0usize);
    for word in &required {
        let n = count_of(&spoken, word);
        want += n;
        have += count_of(&kept, word).min(n);
    }
    have as f64 / want as f64
}

// ---------------------------------------------------------------------------
// Shape predicates — the only thing an LLM case is allowed to assert
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    ContentWordsPreserved,
    EndsWithTerminalMark,
    NoTrailingPeriod,
    StartsCapitalized,
    HasLineBreak,
    HasBlankLine,
    NoBlankLine,
    SingleLine,
    GreetingLine,
    SignoffLine,
    NoInventedDigits,
}

const ALL_SHAPES: &[Shape] = &[
    Shape::ContentWordsPreserved,
    Shape::EndsWithTerminalMark,
    Shape::NoTrailingPeriod,
    Shape::StartsCapitalized,
    Shape::HasLineBreak,
    Shape::HasBlankLine,
    Shape::NoBlankLine,
    Shape::SingleLine,
    Shape::GreetingLine,
    Shape::SignoffLine,
    Shape::NoInventedDigits,
];

const GREETINGS: &[&str] = &["hi", "hey", "hello", "dear", "good morning", "good afternoon"];
const SIGNOFFS: &[&str] = &["thanks", "thank you", "best", "cheers", "regards", "sincerely"];

impl Shape {
    fn parse(name: &str) -> Option<Shape> {
        ALL_SHAPES.iter().copied().find(|s| s.name() == name)
    }

    fn name(self) -> &'static str {
        match self {
            Shape::ContentWordsPreserved => "content_words_preserved",
            Shape::EndsWithTerminalMark => "ends_with_terminal_mark",
            Shape::NoTrailingPeriod => "no_trailing_period",
            Shape::StartsCapitalized => "starts_capitalized",
            Shape::HasLineBreak => "has_line_break",
            Shape::HasBlankLine => "has_blank_line",
            Shape::NoBlankLine => "no_blank_line",
            Shape::SingleLine => "single_line",
            Shape::GreetingLine => "greeting_line",
            Shape::SignoffLine => "signoff_line",
            Shape::NoInventedDigits => "no_invented_digits",
        }
    }

    /// Evaluate the predicate over `output`, with `case` supplying the input side
    /// for the comparative ones.
    fn holds(self, case: &Case, output: &str) -> bool {
        let trimmed = output.trim_end();
        match self {
            Shape::ContentWordsPreserved => retention(case, output) >= RETENTION_FLOOR,
            Shape::EndsWithTerminalMark => trimmed.ends_with(['.', '?', '!']),
            Shape::NoTrailingPeriod => !trimmed.ends_with('.'),
            Shape::StartsCapitalized => output
                .chars()
                .find(|c| c.is_alphabetic())
                .is_some_and(char::is_uppercase),
            Shape::HasLineBreak => output.contains('\n'),
            Shape::HasBlankLine => output.contains("\n\n"),
            Shape::NoBlankLine => !output.contains("\n\n"),
            Shape::SingleLine => !output.contains('\n'),
            Shape::GreetingLine => output.lines().next().is_some_and(|first| {
                let lower = first.trim().to_lowercase();
                first.trim_end().ends_with(',') && GREETINGS.iter().any(|g| lower.starts_with(g))
            }),
            Shape::SignoffLine => output.lines().any(|l| {
                let lower = l.trim().to_lowercase();
                l.trim_end().ends_with(',') && SIGNOFFS.iter().any(|s| lower.starts_with(s))
            }),
            Shape::NoInventedDigits => {
                let spoken = digit_runs(&case.input);
                digit_runs(output).iter().all(|d| spoken.contains(d))
            }
        }
    }
}

/// Maximal runs of digits in a string (§2.5 V4's unit).
fn digit_runs(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_digit())
        .filter(|r| !r.is_empty())
        .map(str::to_string)
        .collect()
}

fn shapes_of(case: &Case) -> Vec<Shape> {
    case.expect_llm_shape
        .iter()
        .map(|name| {
            Shape::parse(name).unwrap_or_else(|| {
                panic!("{}: unknown shape predicate {name:?} ({})", case.id, case.source)
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The corpus tests
// ---------------------------------------------------------------------------

#[test]
fn fixtures_corpus_is_well_formed() {
    let cases = corpus();
    assert!(
        cases.len() >= 30,
        "the corpus is the LLM's scoreboard: ≥ 30 cases required, found {}",
        cases.len()
    );
    let mut ids = BTreeSet::new();
    for case in &cases {
        assert!(ids.insert(case.id.clone()), "duplicate case id {:?}", case.id);
        assert!(!case.input.trim().is_empty(), "{}: empty input", case.id);
        assert!(!case.rules.is_empty(), "{}: covers no rule", case.id);
        assert!(!case.note.trim().is_empty(), "{}: a case has to say why it exists", case.id);
        assert!(
            STYLES.contains(&case.style.as_str()),
            "{}: unknown style {:?}",
            case.id,
            case.style
        );
        // Parsing is the assertion for these two.
        let _ = (mode_of(case), level_of(case));
        assert!(!shapes_of(case).is_empty(), "{}: declares no shape predicate", case.id);
        // A case whose every word is consumable or declared-dropped would make
        // `fixtures_never_lose_text` vacuously true for it.
        assert!(
            !required_words(case).is_empty(),
            "{}: no content word is required to survive — the retention metric would be vacuous",
            case.id
        );
    }
}

#[test]
fn fixtures_cover_every_rule_and_measured_failure() {
    let cases = corpus();
    let covered: BTreeSet<&str> =
        cases.iter().flat_map(|c| c.rules.iter().map(String::as_str)).collect();
    for id in REQUIRED_COVERAGE {
        assert!(
            covered.contains(id),
            "no fixture covers {id} — see docs/research/wispr-formatting-deep-dive.md §1"
        );
    }
    for id in &covered {
        assert!(
            REQUIRED_COVERAGE.contains(id),
            "fixture claims unknown rule id {id:?}"
        );
    }
    // Every predicate has to be exercised by at least one case, or the evaluator
    // below is asserting something nothing uses.
    let used: BTreeSet<&str> =
        cases.iter().flat_map(|c| c.expect_llm_shape.iter().map(String::as_str)).collect();
    for shape in ALL_SHAPES {
        assert!(used.contains(shape.name()), "no fixture uses `{}`", shape.name());
    }
}

#[test]
fn fixtures_rules_corpus_matches_expected() {
    for case in corpus() {
        let got = rules_output(&case);
        assert_eq!(
            got, case.expect_rules,
            "\n{} ({})\n  in     : {:?}\n  want   : {:?}\n  got    : {:?}\n  note   : {}\n",
            case.id, case.source, case.input, case.expect_rules, got, case.note
        );
    }
}

#[test]
fn fixtures_never_lose_text() {
    for case in corpus() {
        for (label, out) in [("rules", rules_output(&case)), ("full", full_output(&case))] {
            let kept = retention(&case, &out);
            assert!(
                kept >= RETENTION_FLOOR,
                "{} ({label}): content-word retention {kept:.2} < {RETENTION_FLOOR:.2}\n  \
                 in : {:?}\n  out: {:?}\n  required: {:?}",
                case.id,
                case.input,
                out,
                required_words(&case)
            );
        }
        // A declared retraction that the rules stage owns has to have actually
        // happened, so a stale `expect_drops` cannot quietly widen the metric.
        if case.owner == Owner::Rules {
            let kept = words(&rules_output(&case));
            for drop in &case.expect_drops {
                assert!(
                    !kept.contains(&drop.to_lowercase()),
                    "{}: declares it drops {drop:?}, but the word is still in the output",
                    case.id
                );
            }
        }
    }
}

#[test]
fn fixtures_llm_shape_predicates_hold() {
    for case in corpus() {
        let rules = rules_output(&case);
        let full = full_output(&case);
        // Rules cases are asserted on every run. LLM/pending cases describe what a
        // stage that does not exist yet must produce, so they are asserted only
        // once that stage actually changes the text — i.e. the day a polish model
        // is installed, this corpus starts grading it.
        if case.owner != Owner::Rules && full == rules {
            continue;
        }
        for shape in shapes_of(&case) {
            assert!(
                shape.holds(&case, &full),
                "{} ({:?}): `{}` does not hold for {:?}",
                case.id,
                case.owner,
                shape.name(),
                full
            );
        }
    }
}

#[test]
fn fixtures_tone_pairs_differ_only_in_shape() {
    let cases = corpus();
    let mut pairs = 0;
    for (i, a) in cases.iter().enumerate() {
        for b in &cases[i + 1..] {
            if a.input != b.input || a.mode != b.mode || a.style == b.style {
                continue;
            }
            pairs += 1;
            // R14: the tone dial adjusts capitalization and punctuation density
            // only — never word choice, grammar or content.
            assert_eq!(
                words(&a.expect_rules),
                words(&b.expect_rules),
                "tone pair {} / {} differs in content words, not just in caps and marks",
                a.id,
                b.id
            );
        }
    }
    assert!(pairs >= 2, "the corpus needs tone pairs to make R14 falsifiable, found {pairs}");
}

#[test]
fn fixtures_shape_predicates_discriminate() {
    // The evaluator itself, on strings written here rather than in the corpus: a
    // predicate that cannot fail would make every LLM case vacuous.
    let case = Case {
        id: "probe".into(),
        rules: vec!["R1".into()],
        owner: Owner::Rules,
        mode: "email".into(),
        style: "default".into(),
        level: "medium".into(),
        context: None,
        input: "hey jordan the build is green i pushed the fix at 7".into(),
        expect_rules: String::new(),
        expect_drops: Vec::new(),
        expect_llm_shape: Vec::new(),
        note: String::new(),
        source: "probe".into(),
    };
    // The shape R13 describes: greeting line, blank line, body, sign-off line.
    let good = "Hey Jordan,\n\nThe build is green and I pushed the fix at 7.\n\nThanks,\nWilson.";
    for (shape, bad) in [
        (Shape::ContentWordsPreserved, "Hey Jordan."),
        (Shape::EndsWithTerminalMark, "the build is green"),
        (Shape::NoTrailingPeriod, "sounds good."),
        (Shape::StartsCapitalized, "hey jordan,"),
        (Shape::HasLineBreak, "Hey Jordan, the build is green"),
        (Shape::HasBlankLine, "Hey Jordan,\nThe build is green"),
        (Shape::NoBlankLine, "Hey Jordan,\n\nThe build is green"),
        (Shape::SingleLine, "Hey Jordan,\nThe build is green"),
        (Shape::GreetingLine, "The build is green"),
        (Shape::SignoffLine, "The build is green"),
        (Shape::NoInventedDigits, "The build is green, call me at 30"),
    ] {
        assert!(!shape.holds(&case, bad), "`{}` accepted {bad:?}", shape.name());
    }
    for shape in [
        Shape::ContentWordsPreserved,
        Shape::EndsWithTerminalMark,
        Shape::StartsCapitalized,
        Shape::HasLineBreak,
        Shape::HasBlankLine,
        Shape::GreetingLine,
        Shape::SignoffLine,
        Shape::NoInventedDigits,
    ] {
        assert!(shape.holds(&case, good), "`{}` rejected {good:?}", shape.name());
    }
    for shape in [Shape::NoTrailingPeriod, Shape::NoBlankLine, Shape::SingleLine] {
        let clean = "sounds good to me";
        assert!(shape.holds(&case, clean), "`{}` rejected {clean:?}", shape.name());
    }
}

// ---------------------------------------------------------------------------
// Polish latency harness (§2.3) — ignored by default
// ---------------------------------------------------------------------------

/// p50 budget for one polish pass over a typical dictation (§2.3).
const POLISH_P50_BUDGET_MS: u128 = 600;
/// The hard deadline: past this the sidecar is abandoned and the rules text is
/// pasted, so p95 above it means the stage is losing its own output.
const POLISH_P95_BUDGET_MS: u128 = 1200;
/// Samples per run. Small enough to stay a test, large enough for a p95.
const POLISH_SAMPLES: usize = 20;

/// The 40-word dictation §2.3's budget is written against (≈ 55 in / 60 out
/// tokens). Its length is asserted, so the numbers in the report always describe
/// the same workload.
const LATENCY_INPUT: &str = "so I wanted to give a quick update on where the migration landed \
last night and what is left before we can close it out because the read path is going through \
the new index and the dashboard is quicker";

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    let idx = (((sorted.len() - 1) as f64) * p).round() as usize;
    sorted[idx]
}

/// Where the measured numbers land. `../..` from the crate is the repo root.
fn latency_report_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/research/polish-latency.md")
}

/// Render the report. Split out of the harness so the formatting — the part that
/// only ever runs on a machine with a model installed — is exercised on every
/// run by `fixtures_latency_report_renders`.
fn latency_report(samples: &[Duration], polished: usize) -> String {
    let p50 = percentile(samples, 0.50).as_millis();
    let p95 = percentile(samples, 0.95).as_millis();
    let min = samples[0].as_millis();
    let max = samples[samples.len() - 1].as_millis();
    let mut report = String::new();
    let _ = write!(
        report,
        "# Polish latency — measured\n\n\
         Written by `polish_latency_p50_under_budget` \
         (`desktop/src-tauri/tests/formatting_fixtures.rs`), the ignored harness from spec item \
         YV58 of `wispr-formatting-deep-dive.md`. Re-run it with:\n\n\
         ```bash\n\
         cd desktop/src-tauri && cargo test --test formatting_fixtures -- --ignored --nocapture\n\
         ```\n\n\
         | | |\n|---|---|\n\
         | machine | {arch} / {os} |\n\
         | input | {words} words, one polish pass per sample |\n\
         | samples | {count} ({polished} returned a rewrite) |\n\
         | p50 | **{p50} ms** (budget {p50_budget} ms) |\n\
         | p95 | **{p95} ms** (hard deadline {p95_budget} ms) |\n\
         | min / max | {min} ms / {max} ms |\n\n\
         The p50 budget and the {p95_budget} ms hard deadline come from §2.3 of the \
         deep-dive; the estimates there are extrapolations from llama.cpp's Apple-Silicon \
         table, and the numbers above are the only measured ones.\n",
        arch = std::env::consts::ARCH,
        os = std::env::consts::OS,
        words = LATENCY_INPUT.split_whitespace().count(),
        count = samples.len(),
        polished = polished,
        p50 = p50,
        p50_budget = POLISH_P50_BUDGET_MS,
        p95 = p95,
        p95_budget = POLISH_P95_BUDGET_MS,
        min = min,
        max = max,
    );
    report
}

/// The report path and the rendering both have to be right on the ONE run that
/// has a model installed, so neither waits for that run to be exercised.
#[test]
fn fixtures_latency_report_renders() {
    assert_eq!(
        LATENCY_INPUT.split_whitespace().count(),
        40,
        "the budget in §2.3 is for a 40-word dictation"
    );
    let dir = latency_report_path().parent().expect("report has a parent dir").to_path_buf();
    assert!(dir.is_dir(), "{} does not exist — the harness could not write", dir.display());

    let samples: Vec<Duration> =
        (1..=POLISH_SAMPLES).map(|n| Duration::from_millis(n as u64)).collect();
    assert_eq!(percentile(&samples, 0.50).as_millis(), 11);
    assert_eq!(percentile(&samples, 0.95).as_millis(), 19);
    let report = latency_report(&samples, POLISH_SAMPLES);
    for fragment in [
        "| p50 | **11 ms** (budget 600 ms) |",
        "| p95 | **19 ms** (hard deadline 1200 ms) |",
        "| min / max | 1 ms / 20 ms |",
        "| input | 40 words, one polish pass per sample |",
    ] {
        assert!(report.contains(fragment), "report is missing {fragment:?}:\n{report}");
    }
}

/// The polish stage as this machine would run it: the best-ranked polish model
/// that is actually downloaded (none ⇒ the stage is off and every sample is a
/// no-op, which is why the harness below is `#[ignore]`d).
fn latency_config() -> polish::PolishConfig {
    let settings = wilson_voice_lib::AppSettings {
        polish_model: polish::installed_model_id().unwrap_or_default(),
        ..Default::default()
    };
    polish::PolishConfig::from_settings(&settings, DictationMode::Notes)
}

/// Measure the real polish stage and write `docs/research/polish-latency.md`.
///
/// Ignored by default because it needs a downloaded polish model: with none
/// installed `polish_llm` is a no-op, and a benchmark of a no-op is noise. Run it
/// with `cargo test --test formatting_fixtures -- --ignored --nocapture`.
#[test]
#[ignore = "measures real polish latency; needs an installed polish model (deep-dive §2.3)"]
fn polish_latency_p50_under_budget() {
    assert_eq!(
        LATENCY_INPUT.split_whitespace().count(),
        40,
        "the budget in §2.3 is for a 40-word dictation"
    );

    // Warm-up: model load and the static prompt's KV cache are one-time costs and
    // are explicitly OFF the dictation path in §2.3's budget.
    let config = latency_config();
    let _ = polish::polish_llm(LATENCY_INPUT, DictationMode::Notes, &config);

    let mut samples = Vec::with_capacity(POLISH_SAMPLES);
    let mut polished = 0usize;
    for _ in 0..POLISH_SAMPLES {
        let start = Instant::now();
        let out = polish::polish_llm(LATENCY_INPUT, DictationMode::Notes, &config);
        samples.push(start.elapsed());
        if out.is_some() {
            polished += 1;
        }
    }

    if polished == 0 {
        println!(
            "polish_latency: SKIPPED — no polish model installed, `polish_llm` returned None for \
             all {POLISH_SAMPLES} samples. Install one and re-run to write the report."
        );
        return;
    }

    samples.sort();
    let p50 = percentile(&samples, 0.50).as_millis();
    let p95 = percentile(&samples, 0.95).as_millis();
    let report = latency_report(&samples, polished);
    let path = latency_report_path();
    fs::write(&path, &report).unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
    println!("{report}\nwrote {}", path.display());

    assert!(p50 <= POLISH_P50_BUDGET_MS, "polish p50 {p50} ms > {POLISH_P50_BUDGET_MS} ms");
    assert!(p95 <= POLISH_P95_BUDGET_MS, "polish p95 {p95} ms > {POLISH_P95_BUDGET_MS} ms");
}
