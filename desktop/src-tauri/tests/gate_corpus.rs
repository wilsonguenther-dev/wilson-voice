//! YV76 — the public-safe corpus for the hallucination gate (YV66 × YV67).
//!
//! YV66 was specified with a regression fixture built from the real `transcripts`
//! table. That corpus is a verbatim record of everything its owner has ever
//! dictated, this repository is public, and no regex scrub makes free-form
//! dictation safe to publish — so the decision (documented in
//! `tests/fixtures/README.md`) is that it stays local, permanently.
//!
//! What lands here instead is a SYNTHETIC corpus that reproduces the same
//! failure classes without carrying a single word of anybody's dictation:
//!
//! * [`Class::RealSpeech`] — long, unpunctuated run-on cadence. This is the YV66
//!   false-positive class: whole-take type/token ratio falls under the old 0.3
//!   cliff purely because the take is long (`global_ttr_below_cliff`), while
//!   every 150-token window stays far above it.
//! * [`Class::LegitRepetition`] — emphatics, "very very long", chorus-like
//!   repetition. Real speech repeats itself; the gate must not care.
//! * [`Class::Degenerate`] — stuck-decoder output: identical-token runs, looped
//!   phrases, and the glued `SERV-SERV-SERV` token Whisper emits as ONE token.
//! * [`Class::Mixed`] — a degenerate tail after real speech, which must be CUT,
//!   never discarded: the prose in front of the loop is dictation the user
//!   cannot reproduce.
//!
//! Two halves, both committed under `tests/fixtures/gate/`:
//! `generated-classes.jsonl` comes from [`generated_cases`] below — a
//! deterministic generator, so the corpus can be regrown and diffed
//! (`gate_corpus_generated_fixture_is_reproducible` fails if the committed file
//! and the generator ever disagree) — and `handwritten-innocuous.jsonl` is
//! hand-written prose covering the shapes a generator makes stilted.
//!
//! The assertions are PROPERTIES over the whole corpus, not per-case goldens:
//! no real-speech case is ever discarded, and every degenerate case is either
//! truncated or rejected — and a rejection keeps its audio, which is YV67's
//! whole point and is pinned against the live gate's source here.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use wilson_voice_lib::dictation::{self, DEGEN_MIN_KEEP_TOKENS};

// ---------------------------------------------------------------------------
// Case model
// ---------------------------------------------------------------------------

/// Which failure class the case belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Class {
    /// Ordinary dictation, including the long run-on takes YV66 exists for.
    RealSpeech,
    /// Ordinary dictation that repeats itself on purpose.
    LegitRepetition,
    /// Stuck-decoder output with nothing worth keeping in front of it.
    Degenerate,
    /// Real speech followed by a degenerate tail.
    Mixed,
}

impl Class {
    /// True for the two classes that are a HUMAN talking — the classes the gate
    /// is never allowed to discard.
    fn is_real_speech(self) -> bool {
        matches!(self, Class::RealSpeech | Class::LegitRepetition)
    }
}

/// What the gate must do with the case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Expect {
    /// `degenerate_cutoff` stays quiet — the take reaches the user whole.
    Keep,
    /// A cutoff past the [`DEGEN_MIN_KEEP_TOKENS`] floor: the prefix survives.
    Truncate,
    /// Cutoff 0 — degenerate from the first token, so the take is rejected and
    /// (YV67) its audio is kept as a retryable row.
    Reject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Case {
    id: String,
    class: Class,
    expect: Expect,
    /// `truncate` cases only: the real-speech part of the take. Whatever
    /// survives the cut has to be a prefix of THIS — never a word of the tail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    clean_prefix: Option<String>,
    /// Set when the take reproduces the YV66 false positive by construction:
    /// its whole-take type/token ratio is under the old 0.3 cliff.
    #[serde(default, skip_serializing_if = "is_false")]
    global_ttr_below_cliff: bool,
    note: String,
    text: String,
    /// Fixture file + line the case came from (filled in by the loader).
    #[serde(skip)]
    source: String,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn case(id: &str, class: Class, expect: Expect, note: &str, text: String) -> Case {
    Case {
        id: id.to_string(),
        class,
        expect,
        clean_prefix: None,
        global_ttr_below_cliff: false,
        note: note.to_string(),
        text,
        source: String::new(),
    }
}

// ---------------------------------------------------------------------------
// Generator
// ---------------------------------------------------------------------------

/// Synthetic dictation clauses. Deliberately mundane and about nobody: no
/// names, no addresses, no numbers, nothing that could be mistaken for a real
/// person's speech. Unpunctuated, because run-on cadence is the shape that made
/// the old whole-take ratio test misfire.
const CLAUSES: [&str; 28] = [
    "so the first thing I want to do is write the outline",
    "and then we can look at the numbers one more time",
    "I think the second option is probably the safer one here",
    "the weather turned cold again so the walk was pretty short",
    "we should probably test that on a fresh machine before shipping",
    "there is a small chance the whole thing needs another pass",
    "let me read that part back to make sure it is right",
    "the notes from the last review already cover most of this ground",
    "it would be good to have a diagram somewhere near the top",
    "nothing here is urgent but it would be nice to finish it",
    "the sample file is tiny so loading it is basically instant",
    "I keep forgetting which folder the exported copies end up in",
    "maybe we start with the smallest change and measure from there",
    "the coffee machine in the hallway is broken again which is annoying",
    "a short walk around the block usually clears my head enough",
    "we could split the work into three chunks and review each one",
    "the second paragraph repeats itself a little bit too much",
    "please remind me to check the queue before the demo starts",
    "the garden needs water and the tomatoes are finally turning red",
    "I would much rather ship something small than ship nothing at all",
    "the test suite runs in well under a minute which is fine",
    "somebody should write down why we picked this approach originally",
    "the printer is out of paper and nobody wants to refill it",
    "we can circle back to that once the first pass has landed",
    "my handwriting is bad enough that dictation is genuinely faster",
    "the recipe calls for butter but olive oil works just as well",
    "there are two bugs left and neither of them is scary",
    "let us keep the scope tight and finish what is already open",
];

/// `count` clauses of run-on dictation, picked by a fixed stride so the take is
/// deterministic, consecutive clauses always differ, and a 150-token window
/// never sees the same clause twice (the stride is coprime with the pool).
fn speech(seed: usize, count: usize) -> String {
    (0..count)
        .map(|i| CLAUSES[(i * 5 + seed) % CLAUSES.len()])
        .collect::<Vec<_>>()
        .join(" ")
}

/// `unit` repeated `times`, space-joined — one stuck-decoder loop.
fn looped(unit: &str, times: usize) -> String {
    vec![unit; times].join(" ")
}

/// Whole-take unique/total token ratio — the measure the pre-YV66 gate used.
fn global_ttr(text: &str) -> f64 {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let unique: BTreeSet<&str> = tokens.iter().copied().collect();
    unique.len() as f64 / tokens.len() as f64
}

/// The generated half of the corpus, in file order. Pure and deterministic —
/// same input, same bytes, every run on every machine.
fn generated_cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();

    // Real speech: the same cadence at six lengths, from a two-sentence aside to
    // a take long enough that its whole-take ratio collapses on its own.
    for (n, (seed, clauses)) in [
        (0, 4),
        (1, 10),
        (2, 26),
        (3, 64),
        (4, 90),
        (5, 110),
        (6, 170),
    ]
    .into_iter()
    .enumerate()
    {
        let text = speech(seed, clauses);
        let mut c = case(
            &format!("gen-real-{:02}", n + 1),
            Class::RealSpeech,
            Expect::Keep,
            "unpunctuated run-on dictation — the class YV66 must never cut",
            text,
        );
        c.global_ttr_below_cliff = global_ttr(&c.text) < 0.3;
        out.push(c);
    }

    // Legitimate repetition: emphatics inside a long take (rule 1 relaxes to 8
    // raw / 12 normalised there, precisely so a person saying a word four times
    // is not read as a stuck decoder).
    for (n, (seed, word, run)) in [(6, "no", 4), (7, "yes", 6), (8, "very", 7)]
        .into_iter()
        .enumerate()
    {
        let text = format!(
            "{} {} {}",
            speech(seed, 12),
            looped(word, run),
            speech(seed + 1, 12)
        );
        out.push(case(
            &format!("gen-emphatic-{:02}", n + 1),
            Class::LegitRepetition,
            Expect::Keep,
            "an emphatic run inside a long take — a person, not a decoder",
            text,
        ));
    }
    out.push(case(
        "gen-emphatic-04",
        Class::LegitRepetition,
        Expect::Keep,
        "emphatics that only line up after normalisation (drifting case and marks)",
        format!(
            "{} No, no, No. no, no! No, no {}",
            speech(9, 12),
            speech(10, 12)
        ),
    ));
    let chorus = "hold on hold on we are almost home";
    out.push(case(
        "gen-chorus-01",
        Class::LegitRepetition,
        Expect::Keep,
        "a chorus quoted three times across a take — repetition with prose between it",
        format!(
            "{} {} {} {} {} {}",
            speech(11, 6),
            chorus,
            speech(12, 6),
            chorus,
            speech(13, 6),
            chorus
        ),
    ));
    out.push(case(
        "gen-doubled-word-01",
        Class::LegitRepetition,
        Expect::Keep,
        "\"very very long\" — the doubled intensifier people actually say",
        format!(
            "{} it was a very very long stretch of work and the room stayed very very quiet {}",
            speech(14, 8),
            speech(15, 8)
        ),
    ));

    // Degenerate from the first token: nothing to keep, so the gate rejects and
    // YV67 keeps the audio.
    for (n, (unit, times)) in [("the", 20), ("the", 200), ("okay", 12)]
        .into_iter()
        .enumerate()
    {
        out.push(case(
            &format!("gen-degen-run-{:02}", n + 1),
            Class::Degenerate,
            Expect::Reject,
            "the decoder stuck on one token for the whole take",
            looped(unit, times),
        ));
    }
    for (n, (unit, times)) in [("and then we can", 40), ("thank you for watching", 12)]
        .into_iter()
        .enumerate()
    {
        out.push(case(
            &format!("gen-degen-phrase-{:02}", n + 1),
            Class::Degenerate,
            Expect::Reject,
            "a short phrase looped for the whole take — rule 2's window collapse",
            looped(unit, times),
        ));
    }
    for (n, glued) in ["SERV-SERV-SERV-SERV", "WPM-SERV-SERV-SERV-SERV-SERV"]
        .into_iter()
        .enumerate()
    {
        out.push(case(
            &format!("gen-degen-glued-{:02}", n + 1),
            Class::Degenerate,
            Expect::Reject,
            "the whole loop emitted as ONE hyphen-glued token (rule 3)",
            glued.to_string(),
        ));
    }
    out.push(case(
        "gen-degen-drift-01",
        Class::Degenerate,
        Expect::Reject,
        "one token looped while its case and sentence marks drift",
        looped("the The, the. the!", 5),
    ));

    // Mixed: real speech, then the decoder sticks. The prefix is dictation the
    // user cannot say again — it must survive.
    let mixed: [(&str, usize, usize, String, &str); 6] = [
        (
            "gen-mixed-run-01",
            20,
            30,
            looped("the", 40),
            "a one-token loop after 30 clauses of dictation",
        ),
        (
            "gen-mixed-run-02",
            23,
            15,
            looped("okay", 30),
            "a shorter take with the same stuck-token ending",
        ),
        (
            "gen-mixed-phrase-01",
            21,
            20,
            looped("and she was", 60),
            "a looped phrase tail — the cut lands on the collapsed window",
        ),
        (
            "gen-mixed-phrase-02",
            24,
            40,
            looped("thank you for watching", 50),
            "the long-take version of the same phrase loop",
        ),
        (
            "gen-mixed-glued-01",
            22,
            25,
            "WPM-SERV-SERV-SERV-SERV".to_string(),
            "a glued loop token appended to a clean take (rule 3)",
        ),
        (
            "gen-mixed-drift-01",
            25,
            18,
            looped("the The, the. the!", 6),
            "a drifting loop tail — normalised run, long-take limit",
        ),
    ];
    for (id, seed, clauses, tail, note) in mixed {
        let prefix = speech(seed, clauses);
        let text = format!("{prefix} {tail}");
        let mut c = case(id, Class::Mixed, Expect::Truncate, note, text);
        c.clean_prefix = Some(prefix);
        out.push(c);
    }

    out
}

// ---------------------------------------------------------------------------
// Corpus loading
// ---------------------------------------------------------------------------

/// The file [`generated_cases`] writes, and the one
/// `gate_corpus_generated_fixture_is_reproducible` holds it to.
const GENERATED_FIXTURE: &str = "generated-classes.jsonl";

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gate")
}

/// One JSON case per line, the same shape the formatting corpus uses.
fn serialize(cases: &[Case]) -> String {
    let mut out = String::new();
    for c in cases {
        out.push_str(&serde_json::to_string(c).expect("a case serialises"));
        out.push('\n');
    }
    out
}

/// Read every `*.jsonl` in the fixture directory — reading the DIRECTORY, not a
/// hand-listed set, so a new fixture file is picked up by every test below the
/// moment it is added.
fn corpus() -> Vec<Case> {
    let dir = fixtures_dir();
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|e| e.expect("readable dir entry").path())
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no fixtures in {}", dir.display());

    let mut cases = Vec::new();
    for file in files {
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            file.extension().is_some_and(|x| x == "jsonl"),
            "{name}: the gate corpus holds *.jsonl cases only — never a raw transcript dump"
        );
        let body = fs::read_to_string(&file).expect("fixture file is readable");
        for (n, line) in body.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let mut c: Case = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("{name}:{} is not a valid case: {e}", n + 1));
            c.source = format!("{name}:{}", n + 1);
            cases.push(c);
        }
    }
    cases
}

// ---------------------------------------------------------------------------
// Shape of the corpus
// ---------------------------------------------------------------------------

/// Floor from the spec: 40 committed cases. It is a floor, not a target — the
/// classes below are what actually matters.
const MIN_CASES: usize = 40;

#[test]
fn gate_corpus_covers_every_failure_class() {
    let cases = corpus();
    assert!(
        cases.len() >= MIN_CASES,
        "the corpus must hold at least {MIN_CASES} cases, found {}",
        cases.len()
    );

    let mut ids: BTreeSet<&str> = BTreeSet::new();
    for c in &cases {
        assert!(
            ids.insert(c.id.as_str()),
            "duplicate case id {:?} ({})",
            c.id,
            c.source
        );
        assert!(!c.note.trim().is_empty(), "{}: every case says why", c.id);
        assert!(!c.text.trim().is_empty(), "{}: empty take", c.id);
    }

    for class in [
        Class::RealSpeech,
        Class::LegitRepetition,
        Class::Degenerate,
        Class::Mixed,
    ] {
        let n = cases.iter().filter(|c| c.class == class).count();
        assert!(n >= 6, "{class:?} needs at least 6 cases, found {n}");
    }
    // The YV66 false positive is the reason this corpus exists: takes that only
    // look degenerate because they are LONG. Prove some are in here.
    let long_class = cases.iter().filter(|c| c.global_ttr_below_cliff).count();
    assert!(
        long_class >= 3,
        "at least 3 cases must reproduce the whole-take-ratio false positive, found {long_class}"
    );
}

#[test]
fn gate_corpus_generated_fixture_is_reproducible() {
    let path = fixtures_dir().join(GENERATED_FIXTURE);
    let committed = fs::read_to_string(&path).expect("the generated fixture is committed");
    assert_eq!(
        committed,
        serialize(&generated_cases()),
        "{} is out of date — regenerate it with \
         `cargo test gate_corpus_regenerate_generated_fixture -- --ignored`",
        path.display()
    );
}

/// Not a test — the writer for the generated half. Run it after touching
/// [`generated_cases`], then commit the diff.
#[test]
#[ignore = "writer, not a check: rewrites the committed fixture"]
fn gate_corpus_regenerate_generated_fixture() {
    let path = fixtures_dir().join(GENERATED_FIXTURE);
    fs::create_dir_all(fixtures_dir()).expect("fixture dir");
    fs::write(&path, serialize(&generated_cases())).expect("fixture is writable");
    eprintln!("wrote {}", path.display());
}

// ---------------------------------------------------------------------------
// The properties
// ---------------------------------------------------------------------------

#[test]
fn gate_corpus_never_discards_real_speech_class() {
    let cases = corpus();
    let mut checked = 0usize;
    for c in cases.iter().filter(|c| c.class.is_real_speech()) {
        assert_eq!(
            c.expect,
            Expect::Keep,
            "{} ({}): a real-speech case can only expect `keep`",
            c.id,
            c.source
        );
        assert_eq!(
            dictation::degenerate_cutoff(&c.text),
            None,
            "{} ({}): the gate cut real speech",
            c.id,
            c.source
        );
        assert!(
            !dictation::is_hallucinated_repetition(&c.text),
            "{} ({}): the gate would discard real speech — and with it the take",
            c.id,
            c.source
        );
        if c.global_ttr_below_cliff {
            let ratio = global_ttr(&c.text);
            assert!(
                ratio < 0.3,
                "{}: declared as the long-take false positive but its ratio is {ratio}",
                c.id
            );
        }
        checked += 1;
    }
    assert!(
        checked >= 12,
        "only {checked} real-speech cases ran — the property is weaker than it looks"
    );
}

#[test]
fn gate_corpus_degenerate_class_truncated_or_rejected_with_audio() {
    // YV67 semantics, pinned where they live: the arm that rejects a whole take
    // must return `TakeOutcome::Rejected`, which is what hands the wav to
    // `keep_failed_take` and gives the user a Retry row. A `Soft` there compiles
    // and quietly restores the bug — the audio is unlinked and the take is gone.
    const LIB: &str = include_str!("../src/lib.rs");
    let gate = LIB
        .split_once("match dictation::degenerate_cutoff(&raw_text) {")
        .expect("the live gate must run the cutoff")
        .1
        .split_once("// YV49 command mode")
        .expect("the live gate ends where command mode begins")
        .0;
    assert!(
        gate.contains("return Ok(TakeOutcome::Rejected {")
            && gate.contains("GATE_REPETITION_REASON"),
        "a whole-take rejection must keep the wav as a retryable row (YV67)"
    );
    assert!(
        !gate.contains("TakeOutcome::Soft("),
        "a Soft outcome discards the wav — a rejected take would be unrecoverable"
    );

    let cases = corpus();
    let mut rejected = 0usize;
    let mut truncated = 0usize;
    for c in cases
        .iter()
        .filter(|c| matches!(c.class, Class::Degenerate | Class::Mixed))
    {
        let cutoff = dictation::degenerate_cutoff(&c.text).unwrap_or_else(|| {
            panic!(
                "{} ({}): degenerate output reached the user untouched",
                c.id, c.source
            )
        });
        match c.expect {
            Expect::Reject => {
                assert_eq!(
                    cutoff, 0,
                    "{} ({}): nothing is salvageable, so the cutoff must be 0",
                    c.id, c.source
                );
                // Cutoff 0 is exactly the arm pinned above: rejected, audio kept.
                assert!(
                    dictation::is_hallucinated_repetition(&c.text),
                    "{} ({}): must be a whole-take rejection",
                    c.id,
                    c.source
                );
                rejected += 1;
            }
            Expect::Truncate => {
                assert!(
                    cutoff > 0,
                    "{} ({}): real speech in front of the loop was discarded",
                    c.id,
                    c.source
                );
                let kept = &c.text[..cutoff];
                let kept_tokens = kept.split_whitespace().count();
                assert!(
                    kept_tokens >= DEGEN_MIN_KEEP_TOKENS,
                    "{} ({}): kept only {kept_tokens} tokens — the caller rejects that as a whole take",
                    c.id,
                    c.source
                );
                let clean = c.clean_prefix.as_deref().unwrap_or_else(|| {
                    panic!("{}: a truncate case declares its clean prefix", c.id)
                });
                assert!(
                    clean.starts_with(kept.trim_end()),
                    "{} ({}): the surviving text is not a prefix of the real speech — \
                     the loop leaked through",
                    c.id,
                    c.source
                );
                assert!(
                    !dictation::is_hallucinated_repetition(&c.text),
                    "{} ({}): a take with a salvageable prefix must be cut, not discarded",
                    c.id,
                    c.source
                );
                truncated += 1;
            }
            Expect::Keep => panic!("{}: a degenerate-class case cannot expect `keep`", c.id),
        }
    }
    assert!(
        rejected >= 6 && truncated >= 6,
        "thin coverage: {rejected} rejections, {truncated} truncations"
    );
}

#[test]
fn gate_corpus_holds_no_private_dictation() {
    // The privacy decision, enforced rather than promised: the committed corpus
    // is synthetic, so none of the shapes that make free-form dictation unsafe
    // to publish can appear in it.
    for c in corpus() {
        let text = format!("{} {}", c.text, c.clean_prefix.unwrap_or_default());
        assert!(
            text.is_ascii(),
            "{} ({}): synthetic cases are plain ASCII",
            c.id,
            c.source
        );
        assert!(
            !text.contains('@'),
            "{} ({}): an address-shaped token has no business in a public fixture",
            c.id,
            c.source
        );
        assert!(
            !text.contains("http"),
            "{} ({}): no links in the corpus",
            c.id,
            c.source
        );
        let longest_digit_run = text
            .split(|c: char| !c.is_ascii_digit())
            .map(str::len)
            .max()
            .unwrap_or(0);
        assert!(
            longest_digit_run < 4,
            "{} ({}): a {longest_digit_run}-digit run reads like a real number",
            c.id,
            c.source
        );
    }
    assert!(
        !fixtures_dir().join("transcripts_corpus.txt").exists(),
        "the private corpus stays local — see tests/fixtures/README.md"
    );
}

/// The complementary LOCAL-ONLY check. The synthetic corpus above proves the
/// gate's behaviour on every failure class; only the real transcripts table
/// proves it does not misfire on how one particular person actually talks. That
/// file can never be committed (see `tests/fixtures/README.md`), so the check is
/// opt-in and reads it from wherever the operator exported it:
///
/// ```sh
/// sqlite3 "$HOME/Library/Application Support/WilsonVoice/wilson_voice.db" \
///   "select replace(replace(raw_text, char(10), ' '), char(13), ' ')
///      from transcripts
///     where raw_text is not null and length(trim(raw_text)) > 0;" \
///   > /tmp/transcripts_corpus.txt
/// YAP_PRIVATE_CORPUS=/tmp/transcripts_corpus.txt \
///   cargo test gate_corpus_private_corpus -- --ignored --nocapture
/// ```
///
/// It prints a line number, a token count and a cutoff per firing line — never a
/// word of the transcript itself (YV20/M2: the body never leaves the machine and
/// never enters a log). Reading the report is the human half of the check.
#[test]
#[ignore = "local-only: needs a private corpus export that is never committed"]
fn gate_corpus_private_corpus_report() {
    let Ok(path) = std::env::var("YAP_PRIVATE_CORPUS") else {
        panic!("set YAP_PRIVATE_CORPUS to an exported corpus — see this test's doc comment");
    };
    let body = fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    let mut lines = 0usize;
    let mut fired = 0usize;
    eprintln!("line\ttokens\tcutoff\tverdict");
    for (n, line) in body.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        lines += 1;
        let Some(cutoff) = dictation::degenerate_cutoff(line) else {
            continue;
        };
        fired += 1;
        let verdict = if cutoff == 0 { "rejected" } else { "truncated" };
        eprintln!(
            "{}\t{}\t{cutoff}\t{verdict}",
            n + 1,
            line.split_whitespace().count()
        );
    }
    eprintln!("{fired} of {lines} lines fired the gate");
}
