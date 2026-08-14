// The Rust string-literal extractor behind YV98's redaction corpus.
//
// This file is compiled TWICE and deliberately so: once as a module of the
// crate (see `vocab.rs`), and once by `build.rs`, which `include!`s it to
// extract the corpus at build time and write it to `OUT_DIR`. That is why it
// carries no `use` statements, no inner (`//!`) doc comments and no reference
// to anything outside `std` — an `include!`d file has to be legal where it is
// pasted, and it is pasted into a build script.
//
// Why extraction moved to build time at all: the first cut `include_str!`d all
// 33 source files into a `const SOURCES`, which put ~1.4 MB of this crate's own
// Rust — comments included, since only the parser skips comments, not the
// embedding — into the `.rodata` of the shipped, signed, notarised binary.
// `strings Yap` then handed anyone the complete offline-licensing and
// trial/revocation implementation, plus every internal comment in the codebase.
//
// Why TEST items are skipped: the review after that found the second half of
// the same mistake. This crate's `#[cfg(test)]` modules hold ~6,000 words of
// hand-written English — `dictation.rs` alone carries a ~1,070-token prose
// paragraph written to exercise the polish validators — and those literals were
// 37% of the corpus. A corpus is an ALLOWLIST: every word in it is a word the
// redactor will let out of a support bundle, so a test fixture full of ordinary
// English sentences is a hole with a test-shaped label on it. It is also
// `#[cfg(test)]`, which means it is not in the shipped binary at all, so
// embedding it broke the other half of the claim too ("what remains is text the
// compiler was going to put in the binary anyway"). Both halves are closed by
// extracting from non-test items only.

/// Every string literal in a Rust source file that the SHIPPED binary can
/// contain — and nothing else.
///
/// Comments are skipped deliberately: this codebase's comments are dense
/// English prose, and letting that prose into the corpus would let it vouch for
/// a sentence somebody spoke. `#[cfg(test)]` / `#[test]` items are skipped for
/// the same reason and one more — they are not compiled into a release build,
/// so their text is not "text the compiler was going to put in the binary
/// anyway". Only what ships counts.
pub fn string_literals(src: &str) -> Vec<String> {
    scan_literals(src, true)
}

/// Every string literal, test items included. Not what the corpus is built
/// from — it is what `no_cfg_test_fixture_text_is_in_the_shipped_corpus` needs
/// in order to know what the corpus must NOT contain.
pub fn string_literals_including_test_items(src: &str) -> Vec<String> {
    scan_literals(src, false)
}

fn scan_literals(src: &str, skip_test_items: bool) -> Vec<String> {
    let c: Vec<char> = src.chars().collect();
    let n = c.len();
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < n {
        match c[i] {
            '/' if i + 1 < n && c[i + 1] == '/' => i = skip_line_comment(&c, i),
            '/' if i + 1 < n && c[i + 1] == '*' => i = skip_block_comment(&c, i),
            // `'x'` is a char literal; `'a` is a lifetime. Telling them apart
            // matters: `'"'` would otherwise open a string that never closes.
            '\'' => i = char_literal_end(&c, i).unwrap_or(i + 1),
            '#' if attribute_start(&c, i).is_some() => {
                let (inner, end) = read_attribute(&c, i);
                i = if skip_test_items && attribute_gates_tests(&inner) {
                    skip_item(&c, end)
                } else {
                    end
                };
            }
            '"' => {
                let (lit, end) = read_quoted(&c, i);
                out.push(lit);
                i = end;
            }
            'r' | 'b' if !starts_inside_identifier(&c, i) => match read_prefixed(&c, i) {
                Some((lit, end)) => {
                    out.push(lit);
                    i = end;
                }
                None => i += 1,
            },
            _ => i += 1,
        }
    }
    out
}

/// `r` in `str` is part of a name, not a raw-string prefix.
fn starts_inside_identifier(c: &[char], i: usize) -> bool {
    i > 0 && (c[i - 1].is_alphanumeric() || c[i - 1] == '_')
}

fn skip_line_comment(c: &[char], mut i: usize) -> usize {
    let n = c.len();
    while i < n && c[i] != '\n' {
        i += 1;
    }
    i
}

fn skip_block_comment(c: &[char], mut i: usize) -> usize {
    let n = c.len();
    let mut depth = 1usize;
    i += 2;
    while i < n && depth > 0 {
        if c[i] == '/' && i + 1 < n && c[i + 1] == '*' {
            depth += 1;
            i += 2;
        } else if c[i] == '*' && i + 1 < n && c[i + 1] == '/' {
            depth -= 1;
            i += 2;
        } else {
            i += 1;
        }
    }
    i
}

/// The index of the `[` when `i` opens an attribute (`#[…]` or `#![…]`).
fn attribute_start(c: &[char], i: usize) -> Option<usize> {
    let n = c.len();
    if i + 1 < n && c[i + 1] == '[' {
        return Some(i + 1);
    }
    if i + 2 < n && c[i + 1] == '!' && c[i + 2] == '[' {
        return Some(i + 2);
    }
    None
}

/// The text between an attribute's brackets, and the index just past its `]`.
/// Bracket-balanced and lexer-aware, because an attribute can hold a string
/// (`#[doc = "…["]`) and nested brackets (`#[cfg(all(test, unix))]` does not,
/// but `#[serde(with = "…")]`-shaped ones can).
fn read_attribute(c: &[char], i: usize) -> (String, usize) {
    let n = c.len();
    let open = match attribute_start(c, i) {
        Some(o) => o,
        None => return (String::new(), i + 1),
    };
    let mut j = open + 1;
    let mut depth = 1usize;
    let mut inner = String::new();
    while j < n && depth > 0 {
        match c[j] {
            '[' => {
                depth += 1;
                inner.push('[');
                j += 1;
            }
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return (inner, j + 1);
                }
                inner.push(']');
                j += 1;
            }
            '"' => {
                let (_, end) = read_quoted(c, j);
                inner.push(' ');
                j = end;
            }
            other => {
                inner.push(other);
                j += 1;
            }
        }
    }
    (inner, n)
}

/// Does this attribute mean "only in a test build"?
///
/// `test` has to be a whole token: `#[cfg(feature = "testing")]` is not a test
/// gate, and `#[cfg(not(test))]` is the opposite of one — that item DOES ship,
/// so its literals belong in the corpus.
fn attribute_gates_tests(inner: &str) -> bool {
    let squashed: String = inner.chars().filter(|c| !c.is_whitespace()).collect();
    if squashed.contains("not(test") {
        return false;
    }
    let bytes: Vec<char> = inner.chars().collect();
    let mut i = 0usize;
    while i + 4 <= bytes.len() {
        if bytes[i..i + 4] == ['t', 'e', 's', 't'] {
            let before_ok = i == 0 || !(bytes[i - 1].is_alphanumeric() || bytes[i - 1] == '_');
            let after = bytes.get(i + 4);
            let after_ok = !matches!(after, Some(c) if c.is_alphanumeric() || *c == '_');
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Skip the item an attribute was attached to: any further attributes and doc
/// comments, then either a braced block or a statement ending in `;`.
fn skip_item(c: &[char], from: usize) -> usize {
    let n = c.len();
    let mut i = from;
    let mut depth = 0usize;
    let mut seen_brace = false;
    while i < n {
        match c[i] {
            '/' if i + 1 < n && c[i + 1] == '/' => i = skip_line_comment(c, i),
            '/' if i + 1 < n && c[i + 1] == '*' => i = skip_block_comment(c, i),
            '\'' => i = char_literal_end(c, i).unwrap_or(i + 1),
            '#' if attribute_start(c, i).is_some() => i = read_attribute(c, i).1,
            '"' => i = read_quoted(c, i).1,
            'r' | 'b' if !starts_inside_identifier(c, i) => {
                i = read_prefixed(c, i).map(|(_, e)| e).unwrap_or(i + 1)
            }
            '{' => {
                depth += 1;
                seen_brace = true;
                i += 1;
            }
            '}' => {
                i += 1;
                depth = depth.saturating_sub(1);
                if seen_brace && depth == 0 {
                    return i;
                }
            }
            ';' if depth == 0 => return i + 1,
            _ => i += 1,
        }
    }
    n
}

/// `b"…"`, `r"…"`, `r#"…"#`, `br#"…"#`.
fn read_prefixed(c: &[char], i: usize) -> Option<(String, usize)> {
    let n = c.len();
    let mut j = i;
    if c[j] == 'b' {
        j += 1;
    }
    if j < n && c[j] == '"' {
        return Some(read_quoted(c, j));
    }
    if j >= n || c[j] != 'r' {
        return None;
    }
    j += 1;
    let hashes = {
        let mut h = 0usize;
        while j + h < n && c[j + h] == '#' {
            h += 1;
        }
        h
    };
    j += hashes;
    if j >= n || c[j] != '"' {
        return None;
    }
    j += 1;
    let mut buf = String::new();
    while j < n {
        if c[j] == '"' && (j + hashes) < n && c[j + 1..=j + hashes].iter().all(|h| *h == '#') {
            return Some((buf, j + hashes + 1));
        }
        if c[j] == '"' && hashes == 0 {
            return Some((buf, j + 1));
        }
        buf.push(c[j]);
        j += 1;
    }
    Some((buf, n))
}

/// A `"…"` literal.
///
/// A `\n` or `\r` escape is kept as a real newline, because the corpus is a
/// list of MESSAGES and a `\n` inside a format string is where one message ends
/// and the next begins: the panic hook writes `"PANIC at {loc}: {msg}\n
/// backtrace:\n{bt}"` as one literal, and those three lines reach the redactor
/// as three separate log lines. Every other escape flattens to a space — an
/// escape ends a phrase, which is the safe direction.
fn read_quoted(c: &[char], i: usize) -> (String, usize) {
    let n = c.len();
    let mut j = i + 1;
    let mut buf = String::new();
    while j < n {
        match c[j] {
            '\\' => {
                j += 1;
                if j < n && c[j] == 'u' && j + 1 < n && c[j + 1] == '{' {
                    while j < n && c[j] != '}' {
                        j += 1;
                    }
                    buf.push(' ');
                } else if j < n && (c[j] == 'n' || c[j] == 'r') {
                    buf.push('\n');
                } else {
                    buf.push(' ');
                }
                j += 1;
            }
            '"' => return (buf, j + 1),
            other => {
                buf.push(other);
                j += 1;
            }
        }
    }
    (buf, n)
}

/// The index just past a char literal starting at `i`, or `None` when `'` began
/// a lifetime.
fn char_literal_end(c: &[char], i: usize) -> Option<usize> {
    let n = c.len();
    if i + 1 >= n {
        return None;
    }
    if c[i + 1] == '\\' {
        // `'\n'`, `'\''`, `'\u{2019}'` — scan to the closing quote, but not far.
        let mut j = i + 2;
        while j < n && j < i + 12 {
            if c[j] == '\'' {
                return Some(j + 1);
            }
            j += 1;
        }
        return None;
    }
    (i + 2 < n && c[i + 2] == '\'').then_some(i + 3)
}

/// One literal, as the corpus stores it: one MESSAGE per line.
///
/// A literal that spans lines is several messages (`"a\nb"`, or a raw string
/// with real newlines in it), and each of them reaches the redactor on its own,
/// so each is its own corpus entry. Remaining control characters become spaces
/// and runs of whitespace collapse, because a format string's alignment is not
/// something the redactor can observe in a log line.
pub fn corpus_lines(lit: &str) -> Vec<String> {
    lit.split('\n')
        .map(|line| {
            let flat: String = line
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            flat.split_whitespace().collect::<Vec<_>>().join(" ")
        })
        .filter(|l| !l.is_empty())
        .collect()
}

/// A literal with no letter in it can contribute no message to any line, so it
/// never needs to ship.
pub fn carries_a_word(lit: &str) -> bool {
    lit.chars().any(|c| c.is_alphabetic())
}
