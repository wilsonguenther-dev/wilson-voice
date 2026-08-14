//! YV98 / finding #36 — the log packer's redaction test.
//!
//! `crash_events` rows have had an allowlist test since YV64. The LOG bundle
//! never had one: PRIVACY.md asserted that diagnostic logs "are designed to
//! contain no transcript text", and an assertion is not a guarantee. This item
//! makes the log bundle the first user content that can ever leave a Yap
//! install, so the assertion has to become a test.
//!
//! The fixture (`tests/fixtures/support/dirty-yap.log`) is HAND-AUTHORED
//! synthetic text — no line of it came from a real dictation or a real machine
//! — and it leaks on purpose, in every shape a real log can: a quoted transcript, an UNQUOTED
//! transcript, a panic whose payload is somebody's medical detail, absolute
//! paths (including one with a space in it, which tokenises awkwardly), the
//! account name both inside a path and bare, an email address, a license key, a
//! URL with a token in the query, and a phone number.
//!
//! Three of its lines are here because the first cut of this redactor SHIPPED
//! them verbatim. It caught transcripts by length — nine word-like tokens in a
//! row — and `is_wordlike` rejects any token carrying a digit or a symbol, so
//! `at 8 tonight` and `Baker & Sons` split a sentence into two short runs and
//! the whole utterance survived byte-identical. Times, amounts and invoice
//! numbers are what dictation is made of, so that was the common case, not the
//! edge. The rule is an allowlist now (`wilson_voice_lib::vocab`): a word
//! survives only if Yap compiled it in, which no sentence somebody spoke ever
//! is — at any length, with any punctuation, from any module.
//!
//! Everything asserted here lives OUTSIDE `src/`, and has to: the corpus is
//! this crate's own string literals, so a transcript fixture written into a
//! source file would become a transcript the redactor is taught to allow.
//!
//! Two directions are tested, and both matter:
//!   * nothing on that list survives the packer, and
//!   * the packer did not "pass" by emptying the file — the operational lines
//!     that make a bundle worth sending are still readable.

use chrono::{DateTime, Utc};
use wilson_voice_lib::support::{self, BundleEntry, BundleInputs};

const DIRTY: &str = include_str!("fixtures/support/dirty-yap.log");
const USERNAME: &str = "wilsonguenther";

/// The three categories finding #36 names, spelled out as literals from the
/// fixture. `(label, needle)` so a failure says WHICH category leaked.
const TRANSCRIPT_SHAPED: &[(&str, &str)] = &[
    (
        "quoted transcript",
        "so the plan for thursday is that we tell the board about the fundraise",
    ),
    (
        "unquoted transcript",
        "please call me back once you have read the memo about the acquisition",
    ),
    (
        "panic payload transcript",
        "her doctor said the results came back clean but she should come in again",
    ),
    // The three the length rule shipped verbatim. A digit in the middle of a
    // sentence, a symbol in the middle of a sentence, and an utterance too
    // short to have ever tripped a threshold at all.
    (
        "transcript split by a digit",
        "remind me to take the lithium",
    ),
    (
        "transcript split by a digit, tail",
        "tonight and again tomorrow",
    ),
    (
        "transcript split by an ampersand",
        "my social is on file at Baker",
    ),
    (
        "transcript split by an ampersand, tail",
        "Sons please shred the copy you have",
    ),
    ("four-word utterance", "call mom"),
    // A log RECORD is not always a log LINE: `logging`'s panic hook writes
    // `PANIC at {loc}: {msg}\nbacktrace:\n{bt}`, so a payload's continuation
    // lines reach the redactor as whole lines of their own. One that merely
    // OPENS with a bracket used to be trusted as an `env_logger` header and
    // shipped verbatim up to the first `]`.
    (
        "transcript masquerading as a log header",
        "her HIV result came back positive",
    ),
    (
        "the prose after a fake header",
        "and I have not told anyone at work",
    ),
    (
        "a fake header inside a backtrace",
        "she asked me not to put it in the notes",
    ),
    // Review round 3. The word allowlist let all of these out, and each is a
    // different way of making the same point: a rule about WORDS says nothing
    // about a LINE.
    //
    // A sentence broken into one-word runs by non-word tokens, where each lone
    // word is a word Yap ships somewhere:
    (
        "a sentence split into single words",
        "her HIV+ result came back positive",
    ),
    // A sentence made only of words that are ordinary enough to be common:
    ("an ordinary short sentence", "she said the thing"),
    // A sentence that happens to be a phrase Yap also ships — the fallback
    // message in this very module says "The file is on your Desktop". Under a
    // phrase allowlist that made it Yap's sentence. It is not; it is the
    // user's, and only whole-message matching can tell the difference.
    (
        "a sentence that collides with one of Yap's own",
        "the file is on your Desktop",
    ),
];

/// Finding #3's class, spelled out: PII that is not word-shaped, so a rule that
/// only governs words never saw it. Every one of these shipped.
const NOT_WORD_SHAPED: &[(&str, &str)] = &[
    ("a card number as spoken", "4111 1111 1111 1111"),
    ("a phone number as spoken", "512 738 7951"),
    ("a dose", "20mg"),
    ("a lab value", "9.4"),
    ("a test name", "A1C"),
    ("a diagnosis", "COVID-19"),
    ("an address fragment", "P.O."),
];

const FILESYSTEM_PATHS: &[(&str, &str)] = &[
    (
        "app support path",
        "/Users/wilsonguenther/Library/Application",
    ),
    (
        "models path",
        "Support/WilsonVoice/models/ggml-small.en.bin",
    ),
    (
        "recordings path",
        "WilsonVoice/recordings/2026-08-12-091244.wav",
    ),
    ("desktop folder name", "Desktop/BoardSyncNotes"),
];

const HOME_USERNAME: &[(&str, &str)] = &[
    ("bare account name", "wilsonguenther"),
    ("account name in an email", "wilsonguenther@gmail.com"),
];

fn at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-08-12T14:15:30Z")
        .unwrap()
        .with_timezone(&Utc)
}

/// Pack the fixture through the REAL builder — not `redact` directly — because
/// the thing under test is the door out of the module, and a builder that
/// forgot to call the redactor would pass a test of the redactor.
fn packed_logs() -> Vec<BundleEntry> {
    let entries = support::build_entries(&BundleInputs {
        crash_summary: "No crashes recorded.\n".into(),
        // Two files, because rotation means the redactor has to run on every
        // one of them, not just the first.
        logs: vec![
            ("yap.log".into(), DIRTY.to_string()),
            ("yap.log.1".into(), DIRTY.to_string()),
        ],
        environment: support::environment_block("macOS 26.5.2 (25F84)", "aarch64"),
        permissions: "accessibility: true\n".into(),
        models: "selected_asr_model: small.en\n".into(),
        username: USERNAME.into(),
        generated_at: at(),
    });
    entries
        .into_iter()
        .filter(|e| e.name.starts_with("logs/"))
        .collect()
}

/// The fixture has to actually contain what it claims to, or every assertion
/// below is vacuous.
#[test]
fn the_fixture_really_is_a_leak() {
    for (label, needle) in TRANSCRIPT_SHAPED
        .iter()
        .chain(NOT_WORD_SHAPED)
        .chain(FILESYSTEM_PATHS)
        .chain(HOME_USERNAME)
    {
        assert!(
            DIRTY.contains(needle),
            "fixture no longer contains its own {label} sentinel"
        );
    }
}

#[test]
fn transcript_shaped_text_does_not_survive_the_bundle() {
    for entry in packed_logs() {
        for (label, needle) in TRANSCRIPT_SHAPED {
            assert!(
                !entry.text.contains(needle),
                "{label} survived into {}:\n{}",
                entry.name,
                entry.text
            );
        }
    }
}

#[test]
fn filesystem_paths_do_not_survive_the_bundle() {
    for entry in packed_logs() {
        for (label, needle) in FILESYSTEM_PATHS {
            assert!(
                !entry.text.contains(needle),
                "{label} survived into {}:\n{}",
                entry.name,
                entry.text
            );
        }
        // Nothing that still looks like an absolute path, anywhere.
        for token in entry.text.split_whitespace() {
            assert!(
                !token.starts_with('/') && !token.starts_with("~/"),
                "an absolute path survived into {}: {token}",
                entry.name
            );
        }
    }
}

#[test]
fn the_home_directory_username_does_not_survive_the_bundle() {
    for entry in packed_logs() {
        let lower = entry.text.to_lowercase();
        for (label, needle) in HOME_USERNAME {
            assert!(
                !lower.contains(&needle.to_lowercase()),
                "{label} survived into {}:\n{}",
                entry.name,
                entry.text
            );
        }
    }
}

/// Finding #3, as its own test: the leaks that are not words.
///
/// A card number, a phone number, a dose, a lab value, a test name, a diagnosis
/// and an address fragment are not word-shaped, so the word allowlist had no
/// opinion about any of them and all seven shipped. `redact_opaque` needed
/// seven digits inside ONE token, and a person says a card number in blocks of
/// four.
#[test]
fn pii_that_is_not_word_shaped_does_not_survive_the_bundle() {
    for entry in packed_logs() {
        for (label, needle) in NOT_WORD_SHAPED {
            assert!(
                !entry.text.contains(needle),
                "{label} survived into {}:\n{}",
                entry.name,
                entry.text
            );
        }
    }
}

/// The general form of the guarantee, and the whole point of the rewrite: it
/// does not depend on knowing which log call leaked the sentence, on the
/// sentence being long, on it being free of digits, or on the leak being made
/// of words at all.
///
/// The previous version of this test walked WORD RUNS, which is the same blind
/// spot the redactor had — it could not see a token that is not a word, so it
/// was green while `4111 1111 1111 1111` shipped. This one walks every token of
/// every line of the packed bundle and asks the shipped rule to account for it.
///
/// It restates the implementation's own predicate, which on its own would be a
/// tautology, so it is deliberately paired with two independent checks: the
/// sentinel lists above (concrete strings, asserted present in the fixture by
/// `the_fixture_really_is_a_leak` and absent from the output here), and
/// `the_general_rule_is_not_vacuous` below, which proves the predicate can say
/// no.
#[test]
fn every_line_that_survives_is_a_message_yap_compiled_in() {
    for entry in packed_logs() {
        for line in entry.text.lines() {
            // The `[timestamp LEVEL module::path]` header is kept verbatim and
            // is not user-writable, so it is not part of the claim — but only
            // a REAL header is exempt, and the split is the shipped rule
            // itself, not a looser restatement of it.
            let (_, body) = support::split_header(line);
            let left = support::unexplained_tokens(body);
            assert!(
                left.is_empty(),
                "unaccounted-for tokens survived into {}: {left:?}\n{line}",
                entry.name
            );
        }
    }
}

/// The predicate the test above leans on has to be able to say NO, or that test
/// is an elaborate way of asserting `true`.
#[test]
fn the_general_rule_is_not_vacuous() {
    for (label, needle) in TRANSCRIPT_SHAPED.iter().chain(NOT_WORD_SHAPED) {
        assert!(
            !support::unexplained_tokens(needle).is_empty(),
            "{label}: the rule accounts for {needle:?}, which is user content"
        );
    }
    // And it says yes to the app's own messages, or the bundle would be empty.
    for msg in [
        "polish sidecar starting",
        "wal_checkpoint failed: database is locked",
        "0: wilson_voice_lib::dictation::finish_utterance",
        "at src/dictation.rs:88:17",
    ] {
        assert!(
            support::unexplained_tokens(msg).is_empty(),
            "over-redacted: {msg:?}"
        );
    }
}

/// The two probes that falsified the length rule, run against the shipped
/// `redact_line` on the exact line shapes they were found on. One digit and one
/// `&` used to be enough to ship the whole utterance byte-identical.
#[test]
fn a_digit_or_a_symbol_inside_a_sentence_no_longer_saves_it() {
    for (label, line, gone) in [
        (
            "a time in the middle of the sentence",
            "[2026-08-12T09:12:45Z INFO  wilson_voice_lib::dictation] polished text = remind me to take the lithium at 8 tonight and again tomorrow when i wake up",
            // Down to the single words: a line carrying text Yap cannot
            // account for loses the lone words it could, so the tail does not
            // come back as "‹redacted› 8 tonight".
            &["remind", "lithium", "tonight", "tomorrow", "wake"][..],
        ),
        (
            "an ampersand in a company name",
            "[2026-08-12T09:12:46Z INFO  wilson_voice_lib::dictation] transcript stored: my social is on file at Baker & Sons please shred the copy you have",
            &["social", "Baker", "Sons", "shred", "copy"][..],
        ),
        (
            "an utterance too short for any threshold",
            "[2026-08-12T09:12:47Z INFO  wilson_voice_lib::dictation] transcript stored: call mom at 5",
            &["call", "mom"][..],
        ),
    ] {
        let out = support::redact_line(line, USERNAME);
        assert_ne!(out, line, "{label}: the line shipped byte-identical");
        for needle in gone {
            assert!(
                !out.contains(needle),
                "{label}: {needle:?} survived\n  in: {line}\n out: {out}"
            );
        }
        // The header is still there — a redacted log is still a log.
        assert!(out.starts_with("[2026-08-12T09:12:4"), "{out}");
    }
}

/// A line is not a header because it starts with `[`.
///
/// This is the shape the multi-line panic record produces — `logging.rs` writes
/// `PANIC at {loc}: {msg}\nbacktrace:\n{bt}` as ONE record, and every line after
/// the first arrives at the redactor on its own. The first cut trusted any
/// leading `[` and emitted everything up to the first `]` unredacted, so a
/// sentence that happened to open with a bracket shipped its opening words
/// verbatim out of the one module whose whole claim is that it does not do that.
#[test]
fn a_line_that_only_looks_like_a_log_header_is_redacted_as_body() {
    for (label, line, gone) in [
        (
            "a bracketed clause opening the line",
            "[her HIV result came back positive] and I have not told anyone at work yet",
            &["HIV", "positive", "told", "anyone"][..],
        ),
        (
            "a bracket that never closes",
            "[her cousin needs the deposit wired before the closing on friday",
            &["cousin", "deposit", "wired", "closing"][..],
        ),
        (
            "a level-shaped word but no timestamp",
            "[INFO wilson_voice_lib::db] she said the biopsy came back benign",
            &["biopsy", "benign"][..],
        ),
        (
            "a timestamp-shaped word that is not a timestamp",
            "[yesterday ERROR wilson_voice_lib::db] he owes the clinic four hundred dollars",
            &["clinic", "owes", "hundred"][..],
        ),
    ] {
        let out = support::redact_line(line, USERNAME);
        for needle in gone {
            assert!(
                !out.contains(needle),
                "{label}: {needle:?} survived\n  in: {line}\n out: {out}"
            );
        }
        // And the header rule agrees: none of these is a header at all.
        let (head, body) = support::split_header(line);
        assert_eq!(head, "", "{label}: treated as a header");
        assert_eq!(
            body, line,
            "{label}: part of the line escaped the body rules"
        );
    }
}

/// The other side of that rule: the header `env_logger` really does write is
/// still recognised, still kept, and still not confused for user content.
#[test]
fn a_real_env_logger_header_is_still_a_header() {
    for line in [
        "[2026-08-12T09:13:01Z WARN  wilson_voice_lib::db] wal_checkpoint failed: database is locked",
        "[2026-08-12T09:15:11Z INFO  wilson_voice_lib] recording cancelled",
        "[2026-08-12T09:14:00Z ERROR wilson_voice_lib::logging] polish sidecar starting",
    ] {
        let (head, body) = support::split_header(line);
        assert!(head.starts_with("[2026-08-12T09:1"), "not split: {line}");
        assert!(head.ends_with(']'), "not split at the bracket: {head}");
        assert!(!body.is_empty(), "the whole line became a header: {line}");
        assert_eq!(format!("{head}{body}"), line);
    }
}

/// Secrets that are not prose: a license key, an API token in a URL query, a
/// phone number. Redacting the sentence and shipping the key would be a strange
/// kind of privacy.
#[test]
fn opaque_secrets_and_long_numbers_do_not_survive() {
    for entry in packed_logs() {
        for needle in [
            "YAPL-7f3a9d1c4b8e2a60f5d7c93b81e4a2f6",
            "hf_ZzQq11223344556677889900aabbccdd",
            "5127387951",
        ] {
            assert!(
                !entry.text.contains(needle),
                "{needle} survived into {}",
                entry.name
            );
        }
    }
}

/// The other direction. A redactor that returned the empty string would pass
/// every test above, and would make the bundle worthless.
#[test]
fn the_bundle_is_still_worth_sending() {
    let packed = packed_logs();
    let text = &packed[0].text;
    for kept in [
        // The header env_logger writes: timestamp, level, module path.
        "[2026-08-12T09:13:01Z WARN  wilson_voice_lib::db]",
        // The operational messages support actually reads.
        "wal_checkpoint failed: database is locked",
        "polish sidecar starting",
        "polish skipped: sidecar is still loading its model",
        "recording cancelled",
        "meeting delete: removed",
        // A foreign diagnostic — SQLite's and the C library's own words, which
        // Yap interpolates but did not write. They are on a fixed allowlist for
        // exactly this reason: "could not remove it" without "No such file or
        // directory" is half a support case.
        "database is locked",
        "No such file or directory",
        // A panic's LOCATION is the most useful line in a crash log. Only its
        // payload is dropped — the same split `crash.rs` already makes.
        "PANIC at src/transcription.rs:212:9",
        // And the host of a failing model download, so "why won't it install"
        // is still answerable.
        "https://huggingface.co/",
    ] {
        assert!(
            text.contains(kept),
            "over-redacted — lost {kept:?}:\n{text}"
        );
    }
    assert_eq!(packed.len(), 2, "both rotations are packed");
}

/// Not an assertion — a way to SEE the redacted log in a PR review.
/// `cargo test --test support_bundle_redaction -- --ignored --nocapture`
#[test]
#[ignore = "prints the redacted fixture for review"]
fn show_the_redacted_log() {
    println!("{}", packed_logs()[0].text);
}
