//! YV98 review — what the redaction corpus puts inside the shipped binary.
//!
//! `vocab`'s guarantee is that a word survives a packed log line only if Yap
//! compiled that word into itself. Its first cut bought that guarantee by
//! `include_str!`ing all 33 files under `src/` into a `const SOURCES` — which
//! meant ~1.4 MB of this crate's own Rust, comments and all, sat in the
//! `.rodata` of a signed, notarised, commercial binary. Only the *parser*
//! skipped comments; the *embedding* did not. `strings Yap.app/Contents/MacOS/
//! Yap` printed the offline Ed25519 licensing implementation, the trial and
//! revocation logic, and every internal note in the codebase — to anyone who
//! downloaded it.
//!
//! The corpus only ever needed the string literals. `build.rs` extracts them at
//! build time now and writes only those to `OUT_DIR`. This file is the standing
//! proof of that, and it lives OUTSIDE `src/` because it has to: a comment
//! sentinel written into a source file would become part of the very corpus it
//! is asserting about.

use std::path::{Path, PathBuf};

use wilson_voice_lib::vocab;

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn source_files() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(src_dir()).expect("read src/") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        out.push((name, std::fs::read_to_string(&path).expect("read source")));
    }
    out.sort();
    out
}

/// The exact strings the review found in the shipped binary with `strings`.
/// Kept literal so a regression is named rather than inferred, and asserted
/// present in the source first so the test cannot pass by the comment having
/// been deleted.
const IMPLEMENTATION_COMMENTS: &[(&str, &str)] = &[
    (
        "license.rs",
        "PUBLIC key: it is safe in the repo, safe in a shipped binary",
    ),
    (
        "license.rs",
        "Rotating it is a breaking change for every copy of Yap already installed",
    ),
];

#[test]
fn the_licensing_implementation_comments_are_not_in_the_corpus() {
    let corpus = vocab::corpus_text();
    for (file, needle) in IMPLEMENTATION_COMMENTS {
        let text = std::fs::read_to_string(src_dir().join(file)).expect("read source");
        assert!(
            text.contains(needle),
            "sentinel is no longer in src/{file} — this test proves nothing: {needle:?}"
        );
        assert!(
            !corpus.contains(needle),
            "src/{file}'s implementation comment ships in the binary: {needle:?}"
        );
    }
}

/// The general form: not one comment in `src/` travels in the corpus.
///
/// A hit is not automatically a leak — a comment-shaped line can be text
/// *inside* a string literal (this codebase quotes its own log lines in test
/// fixtures), and a literal is exactly what the corpus is for. So a hit is
/// checked against the extracted literals before it is called a failure; that
/// second pass is the slow one and it only runs on the rare hit.
#[test]
fn the_shipped_corpus_carries_no_source_comments() {
    let corpus = vocab::corpus_text();
    let sources = source_files();
    let mut checked = 0usize;
    for (name, text) in &sources {
        for line in text.lines() {
            let Some(rest) = line.trim_start().strip_prefix("//") else {
                continue;
            };
            let comment = rest.trim_start_matches(['/', '!']).trim();
            // Long enough that a collision with a compiled-in message would be
            // remarkable rather than plausible.
            if comment.split_whitespace().count() < 8 {
                continue;
            }
            checked += 1;
            if !corpus.contains(comment) {
                continue;
            }
            let is_a_literal = sources
                .iter()
                .flat_map(|(_, t)| vocab::string_literals(t))
                .any(|l| l.contains(comment));
            assert!(
                is_a_literal,
                "a comment from src/{name} shipped inside the binary: {comment:?}"
            );
        }
    }
    assert!(
        checked > 500,
        "only {checked} comments were checked — this codebase has more than that, \
         so the scan is broken"
    );
}

/// The other direction, because a corpus that shipped nothing would pass every
/// assertion above and would make the redactor delete every log line.
#[test]
fn the_corpus_still_carries_yaps_own_messages() {
    let corpus = vocab::corpus_text();
    for message in [
        "polish sidecar starting",
        "file logging",
        "recording cancelled",
    ] {
        assert!(
            corpus.contains(message),
            "the corpus lost one of Yap's own messages: {message:?}"
        );
    }
    let sources = vocab::corpus_source_files();
    assert!(
        sources.contains(&"license.rs") && sources.len() > 20,
        "the build-time glob did not see the crate: {sources:?}"
    );
}

/// Size is the visible half of the fix: literals are a fraction of source, and
/// that fraction is what ships.
#[test]
fn the_corpus_is_a_fraction_of_the_source_it_came_from() {
    let source_bytes: usize = source_files().iter().map(|(_, t)| t.len()).sum();
    let corpus = vocab::corpus_text().len();
    assert!(
        corpus * 3 < source_bytes,
        "corpus is {corpus} bytes against {source_bytes} bytes of source — the \
         source itself is still being embedded"
    );
}

/// Review round 3, and the reason the second cut's allowlist did not hold.
///
/// The corpus is an ALLOWLIST: every message in it is a message the redactor
/// will let out of a support bundle. `build.rs` extracted literals from whole
/// files, so this crate's `#[cfg(test)]` fixtures — ~6,000 words of ordinary
/// English written *specifically to look like dictation*, including the
/// 1,070-token prose paragraph in `dictation.rs` — were 37% of it. They are
/// also not in a release build at all, so embedding them broke the other half
/// of the claim: that what ships in the corpus is text the compiler was going
/// to put in the binary anyway.
///
/// This is also the seam where the rest of the phase meets this item. The
/// extractor GLOBS `src/*.rs`, so every module YV90–YV97 added (`summarize.rs`,
/// `rtring.rs`, `meeting*.rs`, `vad.rs`, `shortcuts.rs`…) feeds this corpus and
/// brings its own test fixtures with it. A guard that named one paragraph in
/// one file would rot on the next merge, so the assertion is over every test
/// module in the tree.
#[test]
fn no_cfg_test_fixture_text_is_in_the_shipped_corpus() {
    let corpus = vocab::corpus_text();
    let files = source_files();
    // What the build script put in the corpus, from the whole tree — a test
    // module often asserts on a message some OTHER module ships, and that
    // message is in the corpus on its own merits.
    let shipped: std::collections::HashSet<String> = files
        .iter()
        .flat_map(|(_, text)| vocab::string_literals(text))
        .flat_map(|l| vocab::corpus_lines(&l))
        .collect();
    let mut test_only_lines = 0usize;
    for (name, text) in &files {
        // Everything the file's literals say, test items included. The
        // difference is text that exists ONLY inside a `#[cfg(test)]` item.
        for lit in vocab::string_literals_including_test_items(text) {
            for line in vocab::corpus_lines(&lit) {
                if shipped.contains(&line) {
                    continue;
                }
                // Only prose is interesting: a short literal is as likely to be
                // punctuation or an identifier as a sentence.
                if line.split_whitespace().count() < 6 {
                    continue;
                }
                test_only_lines += 1;
                assert!(
                    !corpus.contains(&line),
                    "a #[cfg(test)] fixture is in the redaction allowlist \
                     (src/{name}): {line:?}"
                );
            }
        }
    }
    assert!(
        test_only_lines > 200,
        "only {test_only_lines} test-only prose literals were found — the scan \
         is not seeing the fixtures it exists to exclude"
    );
}

/// The other direction, so the test above cannot pass by the extractor having
/// been broken into returning nothing: the fixture prose really is still there
/// in the source it was excluded from.
#[test]
fn the_test_fixture_prose_still_exists_to_be_excluded() {
    let dictation = std::fs::read_to_string(src_dir().join("dictation.rs")).expect("read source");
    let sentinel = "okay so the thing I keep running into when I dictate for a long stretch";
    assert!(
        dictation.contains(sentinel),
        "the fixture this test is about has moved; re-point it"
    );
    assert!(
        !vocab::corpus_text().contains(sentinel),
        "the dictation test paragraph is in the shipped corpus"
    );
}
