//! The vocabulary a packed log line is allowed to use.
//!
//! YV98's first cut caught transcripts by *length*: a run of nine or more
//! word-like tokens was assumed to be prose somebody spoke. That was a
//! statistical guess, and it was wrong in the common case — `is_wordlike`
//! rejects any token carrying a digit or a symbol, so one number in a dictated
//! sentence ("remind me to take the lithium at 8 tonight") split the sentence
//! into two short runs and the whole thing shipped verbatim. Times, amounts and
//! invoice numbers are not an edge case in dictation; they are most of it.
//!
//! So the rule is inverted here, and with it the burden of proof. A word in a
//! log line survives only if Yap **compiled that word in itself**: the corpus
//! is every string literal in this crate's own source, plus a fixed list of
//! foreign diagnostic strings (POSIX `strerror`, SQLite, CoreAudio) that Yap
//! interpolates but does not author. Matching is by contiguous phrase, not by
//! word, so a transcript cannot be reassembled out of common words that happen
//! to appear somewhere in the source.
//!
//! What the user says is never compiled into the binary, so it cannot be in the
//! corpus, so it cannot survive — regardless of how long it is, which module
//! logged it, or how many digits it contains. That is the structural property
//! the length rule never had, and it holds for a four-word fragment as firmly
//! as for a paragraph.
//!
//! The corpus is extracted at BUILD time, not embedded as source. The first
//! cut `include_str!`d all 33 files under `src/`, which put ~1.4 MB of this
//! crate's own Rust — comments included, since only the parser skipped
//! comments, not the embedding — into the `.rodata` of a signed, notarised,
//! commercial binary. `strings Yap` then printed the offline Ed25519 licensing
//! implementation, the trial and revocation logic, and every internal note in
//! the codebase. `build.rs` now runs the same extractor over `src/*.rs` and
//! writes only the literals to `OUT_DIR`; this module embeds that. Identical
//! guarantee, no comments, no non-literal source.
//!
//! The honest limits, stated rather than hidden:
//! * Matching keeps the **longest prefix** of a run that the corpus accounts
//!   for, because a log line is usually a compiled-in message with a runtime
//!   value spliced into it. If a sentence somebody spoke happens to *begin*
//!   with a phrase Yap also ships ("the app is …"), that overlap survives. The
//!   remainder never does.
//! * Numbers, identifiers, versions and error codes are not words and are not
//!   governed here — [`crate::support::redact_line`]'s path, digit-run, token
//!   and quoted-span rules own those, and they run first.
//! * The cost is real and deliberate: a foreign string that is *not* in the
//!   corpus is lost even when it is useful — a Bluetooth device name, a window
//!   title, an error text from a library nobody listed below. A diagnostic
//!   bundle that keeps somebody's dictation is worse than a bundle that lost a
//!   device name.

use std::collections::HashMap;
use std::sync::OnceLock;

/// This crate's own string literals, extracted by `build.rs` at build time and
/// written one literal per line. Nothing else from the source ships: no
/// comments, no identifiers, no code.
const CORPUS: &str = include_str!(concat!(env!("OUT_DIR"), "/vocab-corpus.txt"));

/// The `src/*.rs` files `build.rs` globbed when it built [`CORPUS`], one per
/// line. Kept so `vocabulary_covers_every_source_file` can assert the glob saw
/// every module — a new module that logs must not silently escape the corpus,
/// because every word it logged would then be redacted.
const CORPUS_SOURCES: &str = include_str!(concat!(env!("OUT_DIR"), "/vocab-sources.txt"));

/// The literal corpus exactly as it ships, for tests that assert what does NOT
/// travel in the binary.
pub fn corpus_text() -> &'static str {
    CORPUS
}

/// The source files [`CORPUS`] was built from.
pub fn corpus_source_files() -> Vec<&'static str> {
    CORPUS_SOURCES
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect()
}

/// The extractor itself lives in `vocab_extract.rs` because `build.rs`
/// `include!`s that file too — one implementation, compiled on both sides, so
/// what is embedded can never drift from what this module thinks it embedded.
pub use crate::vocab_extract::string_literals;

/// Text Yap prints but did not write: what `{e}` expands to when the error came
/// from the C library, SQLite or CoreAudio. Without these a log line reads
/// "could not remove ‹path›: ‹redacted 5 words›", which throws away the single
/// most useful half of a failure.
///
/// This is an allowlist of exact phrases from published, fixed vocabularies —
/// `strerror(3)`, SQLite's result strings, a handful of macOS ones. Nothing a
/// user dictates matches an entry unless they dictate an errno string.
const FOREIGN_DIAGNOSTICS: &[&str] = &[
    // strerror(3) — the errnos an app like this actually hits.
    "Operation not permitted",
    "No such file or directory",
    "No such process",
    "Interrupted system call",
    "Input/output error",
    "Device not configured",
    "Argument list too long",
    "Exec format error",
    "Bad file descriptor",
    "No child processes",
    "Resource deadlock avoided",
    "Cannot allocate memory",
    "Permission denied",
    "Bad address",
    "Device busy",
    "Resource busy",
    "File exists",
    "Cross-device link",
    "Operation not supported by device",
    "Not a directory",
    "Is a directory",
    "Invalid argument",
    "Too many open files in system",
    "Too many open files",
    "Inappropriate ioctl for device",
    "Text file busy",
    "File too large",
    "No space left on device",
    "Illegal seek",
    "Read-only file system",
    "Too many links",
    "Broken pipe",
    "Resource temporarily unavailable",
    "Operation now in progress",
    "Operation already in progress",
    "Socket operation on non-socket",
    "Network is down",
    "Network is unreachable",
    "Connection reset by peer",
    "No buffer space available",
    "Connection timed out",
    "Connection refused",
    "Host is down",
    "No route to host",
    "Directory not empty",
    "Too many levels of symbolic links",
    "File name too long",
    "Function not implemented",
    "Operation canceled",
    "Operation timed out",
    "Operation not supported",
    "Undefined error",
    "unexpected end of file",
    "stream did not contain valid UTF-8",
    "invalid utf-8 sequence",
    "entity not found",
    "early eof",
    "failed to fill whole buffer",
    "timed out",
    // SQLite result strings.
    "database is locked",
    "database table is locked",
    "database or disk is full",
    "database disk image is malformed",
    "disk I/O error",
    "attempt to write a readonly database",
    "no such table",
    "no such column",
    "unable to open database file",
    "out of memory",
    "constraint failed",
    "UNIQUE constraint failed",
    "FOREIGN KEY constraint failed",
    "NOT NULL constraint failed",
    "interrupted",
    "not an error",
    // macOS / CoreAudio / AppKit, the ones that reach a log line here.
    "The operation couldn't be completed",
    "The file doesn't exist",
    "The operation was cancelled",
    "not authorized",
    "not authorised",
    "user denied access",
    "unknown error",
];

/// A phrase: the lowercased word tokens of one uninterrupted run.
type Phrase = Vec<String>;

/// Yap's own words, indexed for prefix matching.
pub struct Vocabulary {
    phrases: Vec<Phrase>,
    /// word → every `(phrase, position)` it occurs at, so a run can be matched
    /// starting anywhere inside a compiled-in message, not only at its start.
    index: HashMap<String, Vec<(u32, u32)>>,
}

impl Vocabulary {
    fn build(texts: impl Iterator<Item = String>) -> Self {
        let mut phrases: Vec<Phrase> = Vec::new();
        for text in texts {
            phrases.extend(phrases_of(&text));
        }
        let mut index: HashMap<String, Vec<(u32, u32)>> = HashMap::new();
        for (p, phrase) in phrases.iter().enumerate() {
            for (pos, word) in phrase.iter().enumerate() {
                index
                    .entry(word.clone())
                    .or_default()
                    .push((p as u32, pos as u32));
            }
        }
        Self { phrases, index }
    }

    /// How many of `words`, counted from the front, Yap's own vocabulary can
    /// account for as one contiguous phrase. `0` means "not a word of this came
    /// from Yap".
    pub fn accounted_prefix(&self, words: &[String]) -> usize {
        let Some(first) = words.first() else {
            return 0;
        };
        let Some(candidates) = self.index.get(first.as_str()) else {
            return 0;
        };
        let mut best = 0usize;
        for &(p, pos) in candidates {
            let phrase = &self.phrases[p as usize];
            let start = pos as usize;
            let mut k = 0usize;
            while k < words.len() && start + k < phrase.len() && phrase[start + k] == words[k] {
                k += 1;
            }
            if k > best {
                best = k;
                if best == words.len() {
                    break;
                }
            }
        }
        best
    }

    /// The whole run is Yap's own.
    pub fn accounts_for_all(&self, words: &[String]) -> bool {
        !words.is_empty() && self.accounted_prefix(words) == words.len()
    }
}

/// The process-wide vocabulary, built once on first use (a few tens of
/// milliseconds, on a path the user started by pressing a button).
pub fn vocabulary() -> &'static Vocabulary {
    static V: OnceLock<Vocabulary> = OnceLock::new();
    V.get_or_init(|| {
        let literals = CORPUS
            .lines()
            .map(str::to_string)
            .chain(FOREIGN_DIAGNOSTICS.iter().map(|s| (*s).to_string()));
        Vocabulary::build(literals)
    })
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

/// Punctuation a log line wraps a word in. Trimmed from both ends before a
/// token is judged, so `(release)` and `starting:` are the words they look
/// like.
const EDGE_PUNCT: &[char] = &[
    '(', ')', '[', ']', '{', '}', '<', '>', '"', '`', ',', '.', ';', ':', '!', '?', '*', '|', '~',
    '\'', '\u{2019}', '\u{2018}', '\u{201C}', '\u{201D}', '\u{2026}', '\u{2014}', '\u{2013}',
];

/// The word inside a token, without the punctuation around it.
pub fn word_core(tok: &str) -> &str {
    tok.trim_matches(|c| EDGE_PUNCT.contains(&c))
}

/// A token is a *word* — something a person could have said, and therefore
/// something that has to be accounted for. Anything carrying a digit, a slash,
/// an underscore or a redaction marker is not a word: it is an identifier, a
/// number or an already-redacted span, and the rules that run before this one
/// own it.
pub fn is_wordlike(tok: &str) -> bool {
    let core = word_core(tok);
    !core.is_empty()
        && core.chars().any(|c| c.is_alphabetic())
        && core
            .chars()
            .all(|c| c.is_alphabetic() || c == '-' || c == '\'' || c == '\u{2019}')
}

/// Split text into phrases: maximal runs of word tokens, lowercased. A
/// non-word token — a number, a `{placeholder}`, a path, a marker — ends the
/// run, exactly as it does on the log-line side, so both sides of a comparison
/// are tokenised by the same rule.
pub fn phrases_of(text: &str) -> Vec<Phrase> {
    let mut out = Vec::new();
    let mut cur: Phrase = Vec::new();
    for tok in text.split_whitespace() {
        // A format placeholder is where a runtime value goes; a phrase must
        // never be matched across one, or the corpus would vouch for the value.
        let placeholder = tok.contains('{') || tok.contains('}');
        if !placeholder && is_wordlike(tok) {
            cur.push(word_core(tok).to_lowercase());
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_are_extracted_and_comments_are_not() {
        let src = r####"
            // a comment saying "not a literal"
            /* block "also not" /* nested */ still comment */
            fn f<'a>(x: &'a str) -> &'static str {
                let c = '"';       // a char literal, not an opener
                let _ = c;
                log::info!("polish sidecar starting");
                let raw = r#"a "raw" literal"#;
                let byte = b"bytes here";
                let esc = "line\nbreak";
                raw
            }
        "####;
        let lits = string_literals(src);
        assert!(
            lits.iter().any(|l| l == "polish sidecar starting"),
            "{lits:?}"
        );
        assert!(lits.iter().any(|l| l == r#"a "raw" literal"#), "{lits:?}");
        assert!(lits.iter().any(|l| l == "bytes here"), "{lits:?}");
        assert!(lits.iter().any(|l| l == "line break"), "{lits:?}");
        for l in &lits {
            assert!(!l.contains("not a literal"), "a comment leaked in: {l}");
            assert!(!l.contains("also not"), "a block comment leaked in: {l}");
            assert!(
                !l.contains("still comment"),
                "a nested comment leaked in: {l}"
            );
        }
    }

    #[test]
    fn phrases_break_at_placeholders_and_numbers() {
        let p = phrases_of("unloading ASR model after {}s idle");
        assert_eq!(
            p,
            vec![
                vec![
                    "unloading".to_string(),
                    "asr".to_string(),
                    "model".to_string(),
                    "after".to_string()
                ],
                vec!["idle".to_string()]
            ]
        );
    }

    /// Every assertion about what the vocabulary DOES and DOES NOT account for
    /// lives in `tests/support_bundle_redaction.rs`, and has to: a sentence
    /// written into a file under `src/` becomes a string literal in this
    /// crate's own source, which is to say it becomes part of the corpus. A
    /// transcript fixture kept here would be a transcript the redactor is
    /// taught to allow, and a test of Yap's own messages kept here would pass
    /// on the strength of its own text rather than on the module it came from.
    ///
    /// `build.rs` globs `src/*.rs` rather than reading a hand-kept array, so
    /// this can no longer fall behind by hand — but it can fall behind by
    /// STALENESS: the glob ran at build time, and a module added without the
    /// build script rerunning would silently escape the corpus, after which
    /// every word it logged would be redacted out of a support bundle. So the
    /// check compares what shipped against the directory as it is right now.
    #[test]
    fn vocabulary_covers_every_source_file() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let shipped = corpus_source_files();
        let mut missing = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("read src") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if !shipped.contains(&name.as_str()) {
                missing.push(name);
            }
        }
        assert!(
            missing.is_empty(),
            "not in the vocabulary corpus: {missing:?} (shipped: {shipped:?})"
        );
        assert!(
            shipped.len() > 20,
            "the build-time glob found almost nothing: {shipped:?}"
        );
    }

    /// The corpus is literals, and only literals. A `strings` on the shipped
    /// binary must not print this codebase's commentary — see
    /// `tests/vocab_corpus_is_literals_only.rs` for the assertion against every
    /// comment in `src/`, which has to live outside `src/` to mean anything.
    #[test]
    fn the_corpus_is_smaller_than_the_source_it_came_from() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut source_bytes = 0usize;
        for entry in std::fs::read_dir(&dir).expect("read src") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                source_bytes += std::fs::metadata(&path).expect("metadata").len() as usize;
            }
        }
        assert!(
            CORPUS.len() * 2 < source_bytes,
            "the corpus is {} bytes of {source_bytes} bytes of source — that is \
             not literals only",
            CORPUS.len()
        );
        // And it is not empty, which would pass every "does not contain" test
        // in the suite while making the redactor delete the entire log.
        assert!(
            CORPUS.len() > 50_000,
            "corpus is only {} bytes",
            CORPUS.len()
        );
    }
}
