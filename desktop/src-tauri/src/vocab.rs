//! What a packed log line is allowed to say.
//!
//! # Three rules, and why only the third one holds
//!
//! YV98's first cut caught transcripts by *length*: a run of nine or more
//! word-like tokens was assumed to be prose somebody spoke. That was a
//! statistical guess, and it was wrong in the common case — one number in a
//! dictated sentence ("remind me to take the lithium at 8 tonight") split the
//! sentence into two short runs and the whole thing shipped verbatim.
//!
//! The second cut inverted it into an allowlist of Yap's own compiled-in
//! WORDS, matched as contiguous phrases. Better, and still not a guarantee.
//! The review that followed falsified it three ways, all of them the same
//! mistake in different clothes — *a rule about words says nothing about a
//! line*:
//!
//! * Single words are their own phrases. `her HIV+ result` tokenises to two
//!   one-word runs either side of a non-word, and `her` and `result` are both
//!   words Yap ships somewhere, so the line survived intact.
//! * Non-word tokens were never governed at all. `4111 1111 1111 1111`,
//!   `512 738 7951`, `20mg`, `9.4`, `A1C`, `COVID-19` and `P.O.` all shipped —
//!   a phrase allowlist has no opinion about a token that is not a phrase, and
//!   the test that was supposed to catch it iterated word runs, so it could not
//!   see that class either.
//! * The corpus itself was polluted: `build.rs` extracted literals from whole
//!   files, `#[cfg(test)]` modules included, and this crate's test fixtures
//!   carry ~6,000 words of ordinary English (`dictation.rs` alone has a
//!   1,070-token prose paragraph). 37% of the allowlist was text that never
//!   ships in a release binary and was written to look exactly like dictation.
//!
//! # The rule that ships
//!
//! A log line's body survives **only if it is, in full, one message this crate
//! compiled in** — a string literal from a non-test item — with each
//! interpolated value being value-shaped (a number, a marker a previous rule
//! left behind, a source location) or one of a fixed list of foreign
//! diagnostics Yap prints but did not author (`strerror(3)`, SQLite,
//! CoreAudio).
//!
//! That is a rule about the WHOLE line, so there are no gaps between rules for
//! a token class to fall through: every byte of the body is either inside a
//! literal segment the compiler put in the binary, or inside a hole whose
//! contents were checked. A sentence somebody spoke is never a whole compiled-in
//! message, however short it is, whichever module logged it, however many
//! digits or symbols are sitting in the middle of it — and a token that is not
//! a word gets no free pass, because it is not the words that are being
//! matched, it is the message.
//!
//! Two structural exceptions, both narrow and both tested: a Rust backtrace
//! frame (`  3: some::path::symbol`) and its location line (`at src/x.rs:8:1`).
//! They are the most useful lines in a crash report, they are written by
//! `std::backtrace`, and neither shape is reachable from speech.
//!
//! Lines that match nothing are not deleted — they collapse to a marker with a
//! count, keeping only tokens that are structural on their own (a marker, a
//! Rust path, a source location). Numbers do not survive an unexplained line,
//! which is the whole of finding #3.
//!
//! The honest cost, stated rather than hidden: a foreign string nobody listed
//! is lost even when it was useful — a Bluetooth device name, a window title, a
//! library's error text — and a log line assembled from two literals rather
//! than one is lost with it. A bundle that loses a device name is better than a
//! bundle that keeps somebody's dictation.

use std::collections::HashMap;
use std::sync::OnceLock;

/// This crate's own shipped string literals, extracted by `build.rs` at build
/// time, one MESSAGE per line. Nothing else from the source travels: no
/// comments, no identifiers, no code, and nothing from a `#[cfg(test)]` item.
const CORPUS: &str = include_str!(concat!(env!("OUT_DIR"), "/vocab-corpus.txt"));

/// The `src/*.rs` files `build.rs` globbed when it built [`CORPUS`], one per
/// line. Kept so `vocabulary_covers_every_source_file` can assert the glob saw
/// every module — a new module that logs must not silently escape the corpus,
/// because every line it wrote would then be unexplainable.
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
pub use crate::vocab_extract::{
    corpus_lines, string_literals, string_literals_including_test_items,
};

/// Text Yap prints but did not write: what `{e}` expands to when the error came
/// from the C library, SQLite or CoreAudio. Without these a log line reads
/// "could not remove ‹path›: ‹redacted›", which throws away the single most
/// useful half of a failure.
///
/// This is an allowlist of exact phrases from published, fixed vocabularies —
/// `strerror(3)`, SQLite's result strings, a handful of macOS ones. It is a
/// list rather than a heuristic precisely because it is the one place words
/// Yap did not compile in are allowed through.
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

/// Words that are values rather than prose: units, and the handful of tokens a
/// `{}` expands to when the value is an enum or a flag. Deliberately short —
/// every entry is a word the redactor will let out of a support bundle when it
/// stands inside a hole, so this list is curated, not derived.
const VALUE_WORDS: &[&str] = &[
    "ms",
    "us",
    "ns",
    "sec",
    "secs",
    "min",
    "mins",
    "hrs",
    "kb",
    "mb",
    "gb",
    "tb",
    "kib",
    "mib",
    "gib",
    "hz",
    "khz",
    "bps",
    "px",
    "pt",
    "true",
    "false",
    "none",
    "some",
    "null",
    "nil",
    "ok",
    "err",
    "nan",
    "inf",
    "utf",
    "macos",
    "darwin",
    "arm64",
    "aarch64",
    "x86",
    "amd64",
    "universal",
    "debug",
    "release",
    "info",
    "warn",
    "error",
    "trace",
];

/// File extensions a token may carry and still be a plain identifier rather
/// than something a person said.
const SAFE_EXTENSIONS: &[&str] = &[
    "rs",
    "ts",
    "tsx",
    "js",
    "jsx",
    "json",
    "toml",
    "lock",
    "plist",
    "log",
    "txt",
    "md",
    "html",
    "css",
    "wav",
    "pcm",
    "db",
    "sqlite",
    "bin",
    "gguf",
    "zip",
    "ips",
    "sh",
    "yml",
    "yaml",
    "png",
    "svg",
    "entitlements",
    "dylib",
    "app",
    "dmg",
];

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

/// One compiled-in message, split at its format holes.
///
/// `"hygiene: could not remove {}: {e}"` becomes the literal segments
/// `["hygiene: could not remove ", ": ", ""]` with a hole between each pair. A
/// body matches when every segment is present, in order, anchored at both ends,
/// and every hole's contents pass [`gap_is_safe`].
#[derive(Debug)]
pub struct Template {
    lits: Vec<String>,
}

impl Template {
    fn parse(text: &str) -> Self {
        let mut lits = Vec::new();
        let mut cur = String::new();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0usize;
        while i < chars.len() {
            match chars[i] {
                // `{{` and `}}` are escaped braces: literal text, not a hole.
                '{' if chars.get(i + 1) == Some(&'{') => {
                    cur.push('{');
                    i += 2;
                }
                '}' if chars.get(i + 1) == Some(&'}') => {
                    cur.push('}');
                    i += 2;
                }
                '{' => {
                    let mut j = i + 1;
                    while j < chars.len() && chars[j] != '}' {
                        j += 1;
                    }
                    if j >= chars.len() {
                        // An unclosed brace is not a hole; it is a character.
                        cur.push('{');
                        i += 1;
                        continue;
                    }
                    lits.push(std::mem::take(&mut cur).to_lowercase());
                    i = j + 1;
                }
                c => {
                    cur.push(c);
                    i += 1;
                }
            }
        }
        lits.push(cur.to_lowercase());
        Self { lits }
    }

    /// A message made only of holes vouches for nothing.
    ///
    /// `"{e}"` and `"{}: {}"` are real format strings in this crate, and
    /// `carries_a_word` let them into the corpus because the hole NAME has
    /// letters in it. As templates they are pure wildcards: they matched any
    /// body whose tokens were value-shaped, which is how `9.4`, `A1C` and
    /// `P.O.` came back "explained" by a message that says nothing. A template
    /// has to contribute text the compiler put in the binary, or it is not
    /// evidence of anything.
    fn says_something(&self) -> bool {
        self.lits.iter().any(|l| l.chars().any(char::is_alphabetic))
    }

    /// The first whitespace-delimited word of the leading literal, or `None`
    /// when the message opens with a hole. Used only to index.
    fn anchor(&self) -> Option<&str> {
        self.lits[0].split_whitespace().next()
    }

    fn matches(&self, body: &str, budget: &mut u32) -> bool {
        if !body.starts_with(self.lits[0].as_str()) {
            return false;
        }
        match_rest(&self.lits[1..], body, self.lits[0].len(), budget)
    }
}

/// Walk the remaining literal segments, trying every placement of each. A hole
/// precedes every segment in `rest`.
fn match_rest(rest: &[String], body: &str, pos: usize, budget: &mut u32) -> bool {
    if rest.is_empty() {
        return pos == body.len();
    }
    let lit = rest[0].as_str();
    let tail = &rest[1..];
    let mut search = pos;
    while search <= body.len() {
        if *budget == 0 {
            return false;
        }
        *budget -= 1;
        let Some(off) = body[search..].find(lit) else {
            return false;
        };
        let idx = search + off;
        let gap = &body[pos..idx];
        if gap_is_safe(gap) {
            let next = idx + lit.len();
            if tail.is_empty() {
                if next == body.len() {
                    return true;
                }
            } else if match_rest(tail, body, next, budget) {
                return true;
            }
        }
        // Advance one character, staying on a boundary.
        search = idx + body[idx..].chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// Every message this crate compiled in, indexed by first word.
pub struct Templates {
    all: Vec<Template>,
    by_anchor: HashMap<String, Vec<u32>>,
    /// Messages that open with a hole and so have no first word to index on.
    unanchored: Vec<u32>,
}

impl Templates {
    fn build(texts: impl Iterator<Item = String>) -> Self {
        let all: Vec<Template> = texts
            .map(|t| Template::parse(&t))
            .filter(Template::says_something)
            .collect();
        let mut by_anchor: HashMap<String, Vec<u32>> = HashMap::new();
        let mut unanchored = Vec::new();
        for (i, t) in all.iter().enumerate() {
            match t.anchor() {
                Some(word) => by_anchor
                    .entry(word.to_string())
                    .or_default()
                    .push(i as u32),
                None => unanchored.push(i as u32),
            }
        }
        Self {
            all,
            by_anchor,
            unanchored,
        }
    }

    /// Is this whole body one message Yap compiled in, with safe values in its
    /// holes?
    pub fn explains(&self, body: &str) -> bool {
        let norm = normalize(body).to_lowercase();
        if norm.is_empty() {
            return true;
        }
        let first = norm.split_whitespace().next().unwrap_or_default();
        let anchored = self
            .by_anchor
            .get(first)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for i in anchored.iter().chain(&self.unanchored) {
            // A fresh budget per template: a pathological message must not be
            // able to spend the budget of the one that would have matched.
            let mut budget = 20_000u32;
            if self.all[*i as usize].matches(&norm, &mut budget) {
                return true;
            }
        }
        false
    }

    /// The message that explains `body`, for diagnosing an over- or
    /// under-matching corpus. Not used by the redactor.
    pub fn explaining_message(&self, body: &str) -> Option<Vec<String>> {
        let norm = normalize(body).to_lowercase();
        let first = norm.split_whitespace().next().unwrap_or_default();
        let anchored = self
            .by_anchor
            .get(first)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for i in anchored.iter().chain(&self.unanchored) {
            let mut budget = 20_000u32;
            if self.all[*i as usize].matches(&norm, &mut budget) {
                return Some(self.all[*i as usize].lits.clone());
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.all.len()
    }

    pub fn is_empty(&self) -> bool {
        self.all.is_empty()
    }
}

/// The process-wide template set, built once on first use (a few tens of
/// milliseconds, on a path the user started by pressing a button).
pub fn templates() -> &'static Templates {
    static T: OnceLock<Templates> = OnceLock::new();
    T.get_or_init(|| {
        let messages = CORPUS
            .lines()
            .map(str::to_string)
            .chain(FOREIGN_DIAGNOSTICS.iter().map(|s| (*s).to_string()));
        Templates::build(messages)
    })
}

/// Is this body, in full, a message Yap compiled in — or one of the two
/// backtrace shapes `std` writes?
pub fn explains_body(body: &str) -> bool {
    let norm = normalize(body);
    norm.is_empty() || is_backtrace_frame(&norm) || templates().explains(&norm)
}

/// Runs of whitespace collapse and the ends are trimmed: a format string's
/// alignment is not something a log line preserves, and neither side of the
/// comparison can observe it.
pub fn normalize(body: &str) -> String {
    body.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// Holes
// ---------------------------------------------------------------------------

/// What a `{}` expanded to, checked token by token.
///
/// This is where the guarantee actually lives: the literal halves of a message
/// are text the compiler put in the binary, so the only bytes in a surviving
/// line that could have come from a person are the ones inside a hole. A hole
/// may hold values and it may hold a foreign diagnostic phrase. It may not hold
/// prose, and it may not hold a long number.
fn gap_is_safe(gap: &str) -> bool {
    let toks: Vec<&str> = gap.split_whitespace().collect();
    let mut i = 0usize;
    while i < toks.len() {
        if let Some(n) = foreign_prefix_len(&toks[i..]) {
            i += n;
            continue;
        }
        if token_is_value_shaped(toks[i]) {
            i += 1;
            continue;
        }
        return false;
    }
    true
}

/// The published diagnostics, tokenised once, longest first so
/// `"Connection timed out"` wins over `"timed out"`.
fn foreign_phrases() -> &'static Vec<Vec<String>> {
    static P: OnceLock<Vec<Vec<String>>> = OnceLock::new();
    P.get_or_init(|| {
        let mut v: Vec<Vec<String>> = FOREIGN_DIAGNOSTICS
            .iter()
            .map(|p| {
                p.split_whitespace()
                    .map(|w| word_core(w).to_lowercase())
                    .collect()
            })
            .collect();
        v.sort_by_key(|p| std::cmp::Reverse(p.len()));
        v
    })
}

/// How many tokens at the front of `toks` are one published diagnostic.
fn foreign_prefix_len(toks: &[&str]) -> Option<usize> {
    let lowered: Vec<String> = toks
        .iter()
        .take(12)
        .map(|t| word_core(t).to_lowercase())
        .collect();
    foreign_phrases()
        .iter()
        .find(|p| !p.is_empty() && p.len() <= lowered.len() && lowered[..p.len()] == p[..])
        .map(Vec::len)
}

/// A token that carries no prose: a marker a previous rule left, a URL whose
/// path is already gone, a Rust path, a source location, a number, or a
/// curated unit/flag word.
pub fn token_is_value_shaped(tok: &str) -> bool {
    if token_is_structural(tok) {
        return true;
    }
    let core = word_core(tok);
    if core.is_empty() {
        return true;
    }
    let lower = core.to_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return true;
    }
    // Every alphabetic run of two or more letters has to be a word from the
    // curated list. One letter on its own carries nothing.
    let mut run = String::new();
    for c in core.chars().chain(std::iter::once(' ')) {
        if c.is_alphabetic() {
            run.push(c.to_ascii_lowercase());
            continue;
        }
        if run.len() >= 2 && !VALUE_WORDS.contains(&run.as_str()) {
            return false;
        }
        run.clear();
    }
    true
}

/// A token that stands on its own even when nothing explains the line around
/// it: a redaction marker, a Rust path, a source location. Deliberately does
/// NOT include bare numbers — an unexplained line that keeps its numbers is how
/// `512 738 7951` shipped.
pub fn token_is_structural(tok: &str) -> bool {
    let core = word_core(tok);
    if core.is_empty() {
        return false;
    }
    if is_marker(core) {
        return true;
    }
    if is_rust_path(core) {
        return true;
    }
    is_source_location(core)
}

/// `‹path›`, `‹redacted›`, `‹redacted-prose›(12)` — something a rule above
/// already replaced. Markers are single tokens by construction: one that
/// tokenised into two pieces would be a marker that half of every later rule
/// could not recognise.
pub fn is_marker(tok: &str) -> bool {
    let t = word_core(tok);
    t.starts_with('\u{2039}') && t.ends_with('\u{203A}')
}

/// `wilson_voice_lib::dictation::finish_utterance`, `core::ops::function::FnOnce`.
fn is_rust_path(core: &str) -> bool {
    core.contains("::")
        && core.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | ':' | '<' | '>' | '#' | '.' | '$' | '{' | '}')
        })
}

/// `src/dictation.rs:88:17`, `Info.plist`, `yap.log.1` — a file this repo or
/// this app names, optionally with a line and column.
fn is_source_location(core: &str) -> bool {
    if !core.contains('.') {
        return false;
    }
    if !core
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '/' | '_' | '-' | ':'))
    {
        return false;
    }
    // Trailing `:12:9` is a position, not part of the name.
    let mut head = core;
    while let Some((rest, tail)) = head.rsplit_once(':') {
        if tail.is_empty() || !tail.chars().all(|c| c.is_ascii_digit()) {
            break;
        }
        head = rest;
    }
    let Some((_, ext)) = head.rsplit_once('.') else {
        return false;
    };
    SAFE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
}

/// The two shapes `std::backtrace` writes, and nothing else.
///
/// `   3: wilson_voice_lib::dictation::finish_utterance`
/// `             at src/dictation.rs:88:17`
///
/// These are the most useful lines in a crash report and neither is reachable
/// from speech: the first needs a decimal frame index followed by a `::` path,
/// the second the literal word `at` followed by a source location with a
/// position on it. A panic PAYLOAD that spans lines does not match either — it
/// is prose, and prose has no `::` in it.
pub fn is_backtrace_frame(norm: &str) -> bool {
    let t = norm.trim();
    if let Some((index, symbol)) = t.split_once(": ") {
        if !index.is_empty()
            && index.len() <= 4
            && index.chars().all(|c| c.is_ascii_digit())
            && is_rust_path(symbol.trim())
        {
            return true;
        }
    }
    if let Some(loc) = t.strip_prefix("at ") {
        let loc = loc.trim();
        return is_source_location(loc) && loc.contains(':');
    }
    false
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

/// A token is a *word* — something a person could have said. Kept public
/// because the redaction suite counts words for its markers, but note that
/// nothing in the redaction RULE turns on it any more: the rule matches whole
/// messages, so a token that is not a word gets no free pass. That asymmetry is
/// exactly what finding #3 was.
pub fn is_wordlike(tok: &str) -> bool {
    let core = word_core(tok);
    !core.is_empty()
        && core.chars().any(|c| c.is_alphabetic())
        && core
            .chars()
            .all(|c| c.is_alphabetic() || c == '-' || c == '\'' || c == '\u{2019}')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every assertion about what the corpus DOES and DOES NOT explain lives in
    /// `tests/support_bundle_redaction.rs`, and has to: a sentence written into
    /// a file under `src/` used to become a corpus entry. It no longer does —
    /// this module is `#[cfg(test)]` and the extractor skips test items — but
    /// the discipline is kept, because a test that proves the redactor allows
    /// the fixture it was written with proves nothing.
    ///
    /// `build.rs` globs `src/*.rs` rather than reading a hand-kept array, so
    /// this can no longer fall behind by hand — but it can fall behind by
    /// STALENESS: the glob ran at build time, and a module added without the
    /// build script rerunning would silently escape the corpus, after which
    /// every line it wrote would be unexplainable and redacted.
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
            CORPUS.len() * 4 < source_bytes,
            "the corpus is {} bytes of {source_bytes} bytes of source — that is \
             not shipped literals only",
            CORPUS.len()
        );
        // And it is not empty, which would pass every "does not contain" test
        // in the suite while making the redactor delete the entire log.
        assert!(
            CORPUS.len() > 30_000,
            "corpus is only {} bytes",
            CORPUS.len()
        );
        assert!(templates().len() > 1_000, "{} templates", templates().len());
    }

    #[test]
    fn a_hole_takes_a_value_and_refuses_a_sentence() {
        let t = Template::parse("hygiene: could not remove {}: {e}");
        let mut b = 20_000;
        assert!(t.matches(
            "hygiene: could not remove ‹path› ‹path›: no such file or directory",
            &mut b
        ));
        let mut b = 20_000;
        assert!(!t.matches(
            "hygiene: could not remove the memo about the acquisition: no such file or directory",
            &mut b
        ));
    }

    #[test]
    fn a_backtrace_frame_is_recognised_and_a_sentence_is_not() {
        assert!(is_backtrace_frame("0: wilson_voice_lib::dictation::finish"));
        assert!(is_backtrace_frame("at src/dictation.rs:88:17"));
        assert!(!is_backtrace_frame(
            "1: she asked me not to put it in the notes"
        ));
        assert!(!is_backtrace_frame("at the clinic on friday"));
    }

    #[test]
    fn a_number_is_not_structural() {
        assert!(!token_is_structural("512"));
        assert!(!token_is_structural("9.4"));
        assert!(token_is_structural("‹path›"));
        assert!(token_is_structural("src/dictation.rs:88:17"));
        assert!(token_is_structural("wilson_voice_lib::db"));
    }

    #[test]
    fn value_shape_refuses_a_dose_and_a_lab_value_word() {
        assert!(token_is_value_shaped("300"));
        assert!(token_is_value_shaped("0.8.0"));
        assert!(!token_is_value_shaped("20mg"));
        assert!(!token_is_value_shaped("lithium"));
    }
}
