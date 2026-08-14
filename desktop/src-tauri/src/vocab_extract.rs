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
// The corpus only ever needed the string literals; they are a fraction of that
// and contain no comments. Same guarantee, a fraction of the bytes, and nothing
// about how Yap is built is readable in what ships.

/// Every string literal in a Rust source file — and nothing else.
///
/// Comments are skipped deliberately. This codebase's comments are dense
/// English prose, and letting that prose into the corpus would let it vouch for
/// a sentence somebody spoke. Only text the compiler puts in the binary counts.
pub fn string_literals(src: &str) -> Vec<String> {
    let c: Vec<char> = src.chars().collect();
    let n = c.len();
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < n {
        match c[i] {
            '/' if i + 1 < n && c[i + 1] == '/' => {
                while i < n && c[i] != '\n' {
                    i += 1;
                }
            }
            '/' if i + 1 < n && c[i + 1] == '*' => {
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
            }
            // `'x'` is a char literal; `'a` is a lifetime. Telling them apart
            // matters: `'"'` would otherwise open a string that never closes.
            '\'' => i = char_literal_end(&c, i).unwrap_or(i + 1),
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

/// A `"…"` literal, with escapes flattened to spaces — an escape ends a phrase,
/// which is the safe direction.
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
                }
                buf.push(' ');
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

/// One literal, flattened to a single line: control characters (a raw string
/// can hold real newlines) become spaces, so the corpus file's line boundaries
/// are exactly its literal boundaries. Whitespace inside a literal is not
/// significant — `phrases_of` splits on it either way — so this changes nothing
/// the vocabulary can observe.
pub fn corpus_line(lit: &str) -> String {
    lit.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

/// A literal with no letter in it can contribute no word to any phrase, so it
/// never needs to ship.
pub fn carries_a_word(lit: &str) -> bool {
    lit.chars().any(|c| c.is_alphabetic())
}
