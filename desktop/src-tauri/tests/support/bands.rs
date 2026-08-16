//! YV129 — the two things that have to be *scanned* rather than asserted:
//! whether a tuned enrollment band has appeared anywhere in the shipping crate,
//! and what `docs/yap23-eer-status.md` currently says about OS-8's EER.
//!
//! Shared by `enrollment_threshold_from_harness.rs` (which asserts the absence
//! is consistent with the pinned golden value) and
//! `enrollment_thresholds_refuse_an_unmeasured_eer.rs` (which asserts the
//! absence is *required* while the EER is unmeasured). Two different questions
//! over one scanner, so the two tests cannot disagree about what a "tuned band"
//! is.
//!
//! # Why this file was rewritten: the first scanner was a name-based grep
//!
//! Its predicate fired on a `const`/`static` line carrying one of six hardcoded
//! names, or on a literal sitting inline in a constructor call. A review probe
//! walked straight through it — [`PROBE_SOURCE`], committed below as a
//! permanent mutation case — by renaming the constants and putting one function
//! between the literals and the constructor. Both gates stayed green while the
//! crate carried the exact vendor pair (`0.70` / `0.55`) this item exists to
//! refuse.
//!
//! So the scan is now by TYPE, in four independent nets, plus a compile-time
//! seal that makes the probe fail to build at all:
//!
//! 1. **Provenance** ([`construction_sites`]): every construction of an
//!    `EnrollmentBands` or a `ChipFloor` in `src/**/*.rs` must sit inside a
//!    producer named in [`BAND_PRODUCERS`]. A `pub fn shipped_bands() ->
//!    EnrollmentBands` is a hit whatever it contains, because the type can only
//!    come from a measurement.
//! 2. **Literals through one level of indirection** ([`expand_consts`]): a
//!    constructor argument is resolved against the crate's `const` table before
//!    being called literal-free, so `CosineSimilarity::new(OPENWHISPR_HI)`
//!    reads as `CosineSimilarity::new(0.70)`.
//! 3. **Any tuned float constant in the band module** ([`numeric_const_sites`]):
//!    in `speaker_profiles.rs`, ANY non-endpoint decimal in an `f32`/`f64`
//!    `const`/`static` is a hit regardless of its name.
//! 4. **Deserialization** ([`DESERIALIZERS`]): `EnrollmentBands` derives
//!    `Deserialize`, so data is the one remaining path a shipping band could
//!    arrive by. Naming the type next to a deserializer in `src/` is a hit that
//!    has to be argued for rather than a hole nobody wrote down.
//!
//! The name-based net is kept as a fifth, cheapest one — it catches the naive
//! case with a good error message — but nothing rests on it any more.

#![allow(dead_code)] // each test binary uses a different subset

use std::path::PathBuf;

#[path = "callsite.rs"]
pub mod callsite;

/// The repository root, from this crate's manifest directory
/// (`<repo>/desktop/src-tauri`).
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

/// The in-repo mirror of YV124's `**MEASURED**` block. See its own header for
/// why the gate reads a copy rather than the Obsidian note it came from.
pub fn status_doc() -> PathBuf {
    repo_root().join("docs/yap23-eer-status.md")
}

pub const BLOCK_BEGIN: &str = "<!-- BEGIN YV124 MEASURED BLOCK";
pub const BLOCK_END: &str = "<!-- END YV124 MEASURED BLOCK -->";

/// The literal string YV124 left behind to say the EER was instrumented and not
/// taken. Its presence is what forbids tuning.
pub const UNMEASURED_MARKER: &str = "EER: UNMEASURED";

/// The mirrored block, between the two markers, exclusive of the marker lines.
///
/// `None` when the markers are missing or out of order — which a caller must
/// treat as a failure, never as "no block, therefore nothing to check". A gate
/// that passes when its input disappeared is not a gate.
pub fn extract_block(text: &str) -> Option<String> {
    let start = text.find(BLOCK_BEGIN)?;
    let after_begin = start + text[start..].find("-->")? + "-->".len();
    let end = text.find(BLOCK_END)?;
    if end <= after_begin {
        return None;
    }
    Some(text[after_begin..end].trim().to_string())
}

/// The mirrored block as it stands in this checkout.
///
/// # Panics
/// If the file or its markers are gone. Deleting the block is exactly how a
/// gate like this gets defeated, so it is a failure and not a skip.
pub fn eer_status_block() -> String {
    let path = status_doc();
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{} is the input to YV129's tuning gate and it could not be read ({e}). \
             It is not optional: without it there is nothing holding the enrollment \
             bands to OS-8's ordering requirement.",
            path.display()
        )
    });
    extract_block(&text).unwrap_or_else(|| {
        panic!(
            "{} has lost its `{BLOCK_BEGIN} … {BLOCK_END}` markers. Restore them by \
             re-copying YV124's `**MEASURED**` block from the backlog note; do not \
             delete this test.",
            path.display()
        )
    })
}

pub fn block_says_unmeasured(block: &str) -> bool {
    block.contains(UNMEASURED_MARKER)
}

/// The SSOT the mirror was copied from, when this machine has it.
///
/// `None` on CI and on any fresh clone — the note lives in an Obsidian vault, not
/// in this repository.
///
/// **An absent SSOT is only tolerable while the mirror still says
/// `EER: UNMEASURED`.** The moment it claims a measurement, the caller must fail
/// rather than skip: a status file inside the repo, editable in the same diff
/// that tunes a band, certifies nothing on its own. That is
/// `enrollment_thresholds_refuse_an_unmeasured_eer`'s measured branch, and it is
/// the hole a review finding opened in the first version of this gate.
pub fn ssot_backlog() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("YAP23_BACKLOG_PATH") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let home = std::env::var("HOME").ok()?;
    let p = PathBuf::from(home)
        .join("Obsidian/Wilson-Brain/Notes/Yap-23-Diarization-Backlog-2026-08-15.md");
    p.is_file().then_some(p)
}

/// The sha256 of the mirrored block as committed, so a hand-edit of the mirror
/// is visible in a diff as a second, deliberate line rather than as prose that
/// changed. Recomputed and compared on every machine, SSOT or not.
///
/// Update it ONLY by re-copying the SSOT block and pasting the digest the
/// failure message prints.
pub const MIRROR_SHA256: &str = "8eaf250389c5266ab66513f488f390cfdeee2415109e84d96c57b709225a97d0";

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------
// Masking: comments and string literals are not code
// ---------------------------------------------------------------------------

/// `body` with every comment and every string/char literal blanked to spaces,
/// **byte for byte**, newlines preserved.
///
/// Length-preserving on purpose: every offset a net computes over the mask is
/// an offset into the original file, so a hit can be reported with the source
/// text around it. Blanking rather than deleting is also what makes brace
/// matching safe — `format!("{{")` no longer unbalances a scan, which is how a
/// naive version of this swallowed the rest of a file.
pub fn mask_code(body: &str) -> String {
    let b = body.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0usize;
    let blank = |out: &mut Vec<u8>, i: usize| {
        if out[i] != b'\n' {
            out[i] = b' ';
        }
    };
    while i < b.len() {
        // line comment
        if b[i] == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                blank(&mut out, i);
                i += 1;
            }
            continue;
        }
        // block comment, nested
        if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
            let mut depth = 1usize;
            blank(&mut out, i);
            blank(&mut out, i + 1);
            i += 2;
            while i < b.len() && depth > 0 {
                if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    blank(&mut out, i);
                    blank(&mut out, i + 1);
                    i += 2;
                    continue;
                }
                if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    blank(&mut out, i);
                    blank(&mut out, i + 1);
                    i += 2;
                    continue;
                }
                blank(&mut out, i);
                i += 1;
            }
            continue;
        }
        // raw string: r"…", r#"…"#
        if b[i] == b'r' && (i == 0 || !is_ident_byte(b[i - 1])) {
            let mut j = i + 1;
            let mut hashes = 0usize;
            while b.get(j) == Some(&b'#') {
                hashes += 1;
                j += 1;
            }
            if b.get(j) == Some(&b'"') {
                while i <= j {
                    blank(&mut out, i);
                    i += 1;
                }
                'raw: while i < b.len() {
                    if b[i] == b'"' {
                        let closes = (1..=hashes).all(|k| b.get(i + k) == Some(&b'#'));
                        if closes {
                            for k in 0..=hashes {
                                if i + k < b.len() {
                                    blank(&mut out, i + k);
                                }
                            }
                            i += hashes + 1;
                            break 'raw;
                        }
                    }
                    blank(&mut out, i);
                    i += 1;
                }
                continue;
            }
        }
        // string literal
        if b[i] == b'"' {
            blank(&mut out, i);
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    blank(&mut out, i);
                    if i + 1 < b.len() {
                        blank(&mut out, i + 1);
                    }
                    i += 2;
                    continue;
                }
                let done = b[i] == b'"';
                blank(&mut out, i);
                i += 1;
                if done {
                    break;
                }
            }
            continue;
        }
        // char literal — distinguished from a lifetime by its closing quote
        if b[i] == b'\'' {
            let is_char = b.get(i + 1) == Some(&b'\\')
                || (b.get(i + 2) == Some(&b'\'') && b.get(i + 1).is_some());
            if is_char {
                blank(&mut out, i);
                i += 1;
                while i < b.len() {
                    if b[i] == b'\\' {
                        blank(&mut out, i);
                        if i + 1 < b.len() {
                            blank(&mut out, i + 1);
                        }
                        i += 2;
                        continue;
                    }
                    let done = b[i] == b'\'';
                    blank(&mut out, i);
                    i += 1;
                    if done {
                        break;
                    }
                }
                continue;
            }
        }
        i += 1;
    }
    String::from_utf8(out).expect("blanking ASCII bytes preserves UTF-8")
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

// ---------------------------------------------------------------------------
// The shipping half: everything that is NOT compiled out of a release build
// ---------------------------------------------------------------------------

/// `cfg` predicates whose item is compiled out of `cargo build --release`.
///
/// An explicit allowlist of two exact forms, whitespace-insensitive, rather than
/// "any predicate mentioning `test`": `#[cfg(any(test, unix))]` IS shipping code
/// on unix, and a scanner that skipped it would hand back the hole it just
/// closed.
pub const TEST_ONLY_CFGS: [&str; 2] = ["test", "any(test,feature=\"test-bands\")"];

/// The byte ranges of items that only exist in a test build.
pub fn test_only_item_ranges(body: &str) -> Vec<(usize, usize)> {
    let mask = mask_code(body);
    let m = mask.as_bytes();
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = mask[from..].find("#[cfg(") {
        let at = from + rel;
        let open = at + "#[cfg".len();
        let Some(close) = balanced_close(m, open) else {
            break;
        };
        // Read the predicate from the SOURCE, not the mask: the mask blanks
        // string literals, and `feature = "test-bands"` is one. (The mask is
        // length-preserving, so these offsets are the same in both.)
        let predicate: String = body[open + 1..close]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        from = close + 1;
        if !TEST_ONLY_CFGS.contains(&predicate.as_str()) {
            continue;
        }
        // Past the attribute's `]`, then the item: `{ … }` or `… ;`.
        let Some(bracket) = mask[close..].find(']').map(|k| close + k) else {
            break;
        };
        let mut i = bracket + 1;
        let mut end = None;
        while i < m.len() {
            match m[i] {
                b'{' => {
                    end = brace_close(m, i).map(|e| e + 1);
                    break;
                }
                b';' => {
                    end = Some(i + 1);
                    break;
                }
                _ => i += 1,
            }
        }
        if let Some(end) = end {
            out.push((at, end));
            from = end;
        }
    }
    out
}

/// Index of the `)` matching the `(` at `open`.
fn balanced_close(m: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in m.iter().enumerate().skip(open) {
        match c {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Index of the `}` matching the `{` at `open`.
fn brace_close(m: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in m.iter().enumerate().skip(open) {
        match c {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// A file's shipping half: the source with every test-only item blanked out,
/// line numbers preserved.
///
/// Unit tests inside `src/` are not shipping code, and they legitimately
/// construct bands from literals — that is how the band logic is exercised at
/// all. Scanning them would make this gate impossible to satisfy for the wrong
/// reason. Blanking rather than truncating at the first `#[cfg(test)]` matters
/// now that a test-only item (`EnrollmentBands::for_test`) sits in the MIDDLE
/// of the file: truncating there would have stopped scanning everything below
/// it, which is a hole shaped exactly like the one this rewrite closes.
pub fn shipping_half(body: &str) -> String {
    let mut out = body.as_bytes().to_vec();
    for (start, end) in test_only_item_ranges(body) {
        for byte in out.iter_mut().take(end.min(body.len())).skip(start) {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    }
    String::from_utf8(out).expect("blanking ASCII bytes preserves UTF-8")
}

/// The file's shipping half, masked and flattened to one line, so a
/// paren-balanced scan crosses line breaks. Byte offsets are preserved.
pub fn shipping_code(body: &str) -> String {
    mask_code(&shipping_half(body)).replace('\n', " ")
}

// ---------------------------------------------------------------------------
// Net 5 (cheapest, kept for its error message): "is this line a tuned band?"
// ---------------------------------------------------------------------------

/// Names that would make a constant an enrollment band whatever its value.
pub const BAND_NAMES: [&str; 6] = [
    "AUTO_CONFIRM",
    "NEW_VOICE_FLOOR",
    "ENROLLMENT_BAND",
    "ENROLLMENT_THRESHOLD",
    "SUGGEST_FLOOR",
    "SPEAKER_MATCH_THRESHOLD",
];

/// Every decimal literal in `s`: a `.` with a digit on each side, extended in
/// both directions. `1e-4` is deliberately not one — an epsilon is not a
/// threshold.
pub fn decimal_literals(s: &str) -> Vec<String> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'.' && i > 0 && b[i - 1].is_ascii_digit() && b.get(i + 1).is_some_and(u8::is_ascii_digit)
        {
            let mut start = i - 1;
            while start > 0 && b[start - 1].is_ascii_digit() {
                start -= 1;
            }
            if start > 0 && b[start - 1] == b'-' {
                start -= 1;
            }
            let mut end = i + 1;
            while end < b.len() && b[end].is_ascii_digit() {
                end += 1;
            }
            out.push(s[start..end].to_string());
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

/// The endpoints and origin of the cosine-similarity line. A `0.0` or a `1.0`
/// is the unit's own bound or its identity element — `cosine_similarity`
/// returns `0.0` for a zero-norm embedding, `CosineDistance::from_similarity`
/// is `1.0 - x` — and neither is a band anybody tuned. Excluded so the scanner
/// reports thresholds rather than arithmetic.
pub fn is_unit_endpoint(literal: &str) -> bool {
    matches!(literal, "0.0" | "1.0" | "-1.0" | "2.0")
}

/// A tuned literal: a decimal that is not one of the unit's own endpoints.
pub fn has_tuned_literal(s: &str) -> bool {
    decimal_literals(s).iter().any(|l| !is_unit_endpoint(l))
}

/// Does this line of SHIPPING code declare a tuned enrollment threshold by
/// NAME, or install one as a `Default`?
///
/// The weakest of the nets, and the one the review probe defeated: a rename is
/// all it takes. Kept because when it DOES fire it says exactly what is wrong,
/// and because a naive reintroduction of the vendor pair should fail on the
/// clearest message rather than on a provenance walk.
pub fn is_tuned_band_line(code: &str) -> bool {
    let declares = code.contains("const ") || code.contains("static ");
    if declares && BAND_NAMES.iter().any(|n| code.contains(n)) {
        return true;
    }
    if code.contains("impl Default for EnrollmentBands")
        || code.contains("impl Default for ChipFloor")
    {
        return true;
    }
    // A one-line construction from a literal, whatever it is called.
    if code.contains("CosineSimilarity::new(") && has_tuned_literal(code) {
        return true;
    }
    false
}

/// The argument list of every `<ctor>` call in `code`, paren-balanced.
///
/// Paren-matched rather than "the next N characters" so a construction split
/// across lines is still read as one call and a following unrelated constant
/// is not folded into it. Both mistakes were available; both would have made
/// this gate lie in a different direction.
pub fn call_arguments(code: &str, ctor: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(at) = code[from..].find(ctor) {
        let open = from + at + ctor.len();
        let b = code.as_bytes();
        let mut depth = 1usize;
        let mut i = open;
        while i < b.len() && depth > 0 {
            match b[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        out.push(code[open..i.saturating_sub(1).max(open)].to_string());
        from = open;
    }
    out
}

// ---------------------------------------------------------------------------
// Net 2: one level of `const` indirection
// ---------------------------------------------------------------------------

/// Every `const`/`static` in `code` as `(name, initializer)`.
///
/// Deliberately one level deep and no further: a second level would need a real
/// resolver, and the point is not to evaluate Rust — it is that
/// `CosineSimilarity::new(OPENWHISPR_HI)` must not read as literal-free.
pub fn const_table(code: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for keyword in ["const ", "static "] {
        let mut from = 0usize;
        while let Some(rel) = code[from..].find(keyword) {
            let at = from + rel;
            from = at + keyword.len();
            let rest = &code[from..];
            let name: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if name.is_empty() {
                continue;
            }
            // The initializer has to be inside THIS declaration: a trait's
            // `const FOO: u8;` has none, and taking the next `=` in the file
            // would pair a name with a stranger's value.
            let Some(semi) = rest.find(';') else { continue };
            let decl = &rest[..semi];
            let Some(eq) = decl.find('=') else { continue };
            out.push((name, decl[eq + 1..].trim().to_string()));
        }
    }
    out
}

/// `text` with each known constant name, as a whole token, replaced by its
/// initializer — once, never recursively.
pub fn expand_consts(text: &str, table: &[(String, String)]) -> String {
    let mut out = text.to_string();
    for (name, value) in table {
        if name.len() < 2 || !out.contains(name.as_str()) {
            continue;
        }
        let mut next = String::with_capacity(out.len());
        let mut rest = out.as_str();
        while let Some(at) = rest.find(name.as_str()) {
            let before_ok = at == 0 || !is_ident_byte(rest.as_bytes()[at - 1]);
            let after = at + name.len();
            let after_ok = after >= rest.len() || !is_ident_byte(rest.as_bytes()[after]);
            next.push_str(&rest[..at]);
            if before_ok && after_ok {
                next.push_str(&format!("({value})"));
            } else {
                next.push_str(name.as_str());
            }
            rest = &rest[after..];
        }
        next.push_str(rest);
        out = next;
    }
    out
}

// ---------------------------------------------------------------------------
// Net 1: provenance — who is allowed to construct a band
// ---------------------------------------------------------------------------

/// The types that may only come from a measurement.
pub const BAND_TYPES: [&str; 2] = ["EnrollmentBands", "ChipFloor"];

/// The only functions in `src/**/*.rs` allowed to construct each band type.
///
/// `bands_from_distribution` is the measured producer; `from_measured_edges` is
/// the private checked constructor it calls (private precisely so nothing else
/// can). `ChipFloor::new` stores the two ranking parameters its caller supplies
/// and holds no number of its own — the literal net covers its call sites.
///
/// Qualified (`Type::fn`) wherever the producer is an inherent method, so a
/// band built inside some OTHER type's `fn new` is still a hit.
pub const BAND_PRODUCERS: [(&str, &[&str]); 2] = [
    (
        "EnrollmentBands",
        &[
            "bands_from_distribution",
            "EnrollmentBands::from_measured_edges",
        ],
    ),
    ("ChipFloor", &["ChipFloor::new"]),
];

/// Constructors whose arguments would be a tuned band if they were literals.
pub const BAND_CONSTRUCTORS: [&str; 4] = [
    "EnrollmentBands::new(",
    "EnrollmentBands::for_test(",
    "ChipFloor::new(",
    "TargetFar::new(",
];

/// Ways a band could arrive as DATA rather than as a literal. `EnrollmentBands`
/// derives `Deserialize` because the UI is handed one; that same derive is a
/// path by which an unmeasured band could enter the app from a file.
pub const DESERIALIZERS: [&str; 4] = ["from_str", "from_slice", "from_value", "deserialize"];

/// The modules whose float constants are all suspect by location.
pub const BAND_MODULES: [&str; 1] = ["speaker_profiles.rs"];

/// `(name, body_start, body_end)` for every `fn` with a body in `code`.
pub fn fn_ranges(code: &str) -> Vec<(String, usize, usize)> {
    let m = code.as_bytes();
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = code[from..].find("fn ") {
        let at = from + rel;
        from = at + 3;
        if at > 0 && is_ident_byte(m[at - 1]) {
            continue;
        }
        let name: String = code[at + 3..]
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let mut i = at + 3;
        let mut body = None;
        while i < m.len() {
            match m[i] {
                b'{' => {
                    body = brace_close(m, i).map(|e| (i, e));
                    break;
                }
                b';' => break,
                _ => i += 1,
            }
        }
        if let Some((start, end)) = body {
            out.push((name, start, end));
            from = start + 1;
        }
    }
    out
}

/// The innermost `fn` containing `index`, qualified with the impl type when it
/// sits inside one, or `<module level>`.
///
/// Qualified because the allowlist is `ChipFloor::new`, not `new`: a `ChipFloor`
/// built inside some other type's `fn new` is exactly the smuggling this net
/// exists to see, and an unqualified name would have waved it through.
fn enclosing_fn(
    ranges: &[(String, usize, usize)],
    self_impls: &[(usize, usize)],
    ty: &str,
    index: usize,
) -> String {
    let name = ranges
        .iter()
        .filter(|(_, s, e)| index > *s && index < *e)
        .max_by_key(|(_, s, _)| *s)
        .map(|(n, _, _)| n.clone())
        .unwrap_or_else(|| "<module level>".to_string());
    if self_impls.iter().any(|(s, e)| index > *s && index < *e) {
        format!("{ty}::{name}")
    } else {
        name
    }
}

/// The byte ranges of `impl <ty>` / `impl … for <ty>` blocks, where `Self` is
/// `ty` and a `Self { … }` is a construction of it.
fn impl_ranges(code: &str, ty: &str) -> Vec<(usize, usize)> {
    let m = code.as_bytes();
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = code[from..].find("impl ") {
        let at = from + rel;
        from = at + 5;
        let Some(open) = code[at..].find('{').map(|k| at + k) else {
            break;
        };
        let header = &code[at + 5..open];
        let last_word = header
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .rfind(|w| !w.is_empty())
            .unwrap_or("");
        if last_word == ty {
            if let Some(end) = brace_close(m, open) {
                out.push((open, end));
            }
        }
    }
    out
}

/// Every construction of `ty` in `code`, as `(enclosing_fn, snippet)`.
///
/// A construction is an associated call `Ty::something(`, a struct literal
/// `Ty {`, or — inside an `impl Ty` block — `Self {` / `Self::something(`. The
/// type's own definition (`struct Ty {`), its impl headers and `-> Self {`
/// return positions are not.
pub fn construction_sites(code: &str, ty: &str) -> Vec<(String, String)> {
    let fns = fn_ranges(code);
    let self_impls = impl_ranges(code, ty);
    let mut hits: Vec<(usize, String)> = Vec::new();

    let assoc = format!("{ty}::");
    let mut from = 0usize;
    while let Some(rel) = code[from..].find(&assoc) {
        let at = from + rel;
        from = at + assoc.len();
        let tail = &code[at + assoc.len()..];
        let ident: String = tail
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !ident.is_empty() && tail[ident.len()..].starts_with('(') {
            hits.push((at, format!("{assoc}{ident}(")));
        }
    }

    let literal = format!("{ty} ");
    for pattern in [literal.as_str(), ty] {
        let mut from = 0usize;
        while let Some(rel) = code[from..].find(pattern) {
            let at = from + rel;
            from = at + pattern.len();
            let after = code[at + pattern.len()..].trim_start();
            if !after.starts_with('{') {
                continue;
            }
            let before = code[..at].trim_end();
            if ["struct", "impl", "enum", "union", "trait", "for"]
                .iter()
                .any(|k| before.ends_with(k))
            {
                continue;
            }
            hits.push((at, format!("{ty} {{")));
        }
    }

    for (open, close) in self_impls.iter().copied() {
        let block = &code[open..close];
        for (pattern, label) in [("Self {", "Self { … }"), ("Self::", "Self::…(")] {
            let mut from = 0usize;
            while let Some(rel) = block[from..].find(pattern) {
                let at = open + from + rel;
                from += rel + pattern.len();
                if pattern == "Self {" {
                    // `-> Self {` is a RETURN TYPE followed by the body brace,
                    // not a construction. Missing this made `ChipFloor::new`'s
                    // own signature look like a smuggled band.
                    let before = code[..at].trim_end();
                    if before.ends_with("->") || before.ends_with(':') || before.ends_with("for") {
                        continue;
                    }
                }
                if pattern == "Self::" {
                    let tail = &code[at + pattern.len()..];
                    let ident: String = tail
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect();
                    if ident.is_empty() || !tail[ident.len()..].starts_with('(') {
                        continue;
                    }
                }
                hits.push((at, format!("{ty}: {label}")));
            }
        }
    }

    hits.sort_by_key(|(at, _)| *at);
    hits.dedup_by_key(|(at, _)| *at);
    hits.into_iter()
        .map(|(at, snippet)| (enclosing_fn(&fns, &self_impls, ty, at), snippet))
        .collect()
}

// ---------------------------------------------------------------------------
// Net 3: any tuned float constant in the band module
// ---------------------------------------------------------------------------

/// Every `f32`/`f64` `const`/`static` in `code` whose value carries a
/// non-endpoint decimal — regardless of what it is called.
///
/// This is the net the review probe's `const OPENWHISPR_HI: f32 = 0.70;` lands
/// in: a band does not have to admit its own name to be a band.
pub fn numeric_const_sites(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    for keyword in ["const ", "static "] {
        let mut from = 0usize;
        while let Some(rel) = code[from..].find(keyword) {
            let at = from + rel;
            from = at + keyword.len();
            let rest = &code[from..];
            let Some(semi) = rest.find(';') else { continue };
            let decl = &rest[..semi];
            let Some(colon) = decl.find(':') else { continue };
            let Some(eq) = decl.find('=') else { continue };
            if eq < colon {
                continue;
            }
            let ty = &decl[colon + 1..eq];
            if !(ty.contains("f32") || ty.contains("f64")) {
                continue;
            }
            if has_tuned_literal(&decl[eq + 1..]) {
                out.push(decl.trim().to_string());
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The scan itself
// ---------------------------------------------------------------------------

/// The review probe that defeated the first version of this scanner, committed
/// as a permanent mutation case.
///
/// `the_scanner_catches_the_renamed_indirect_probe` runs every net over it and
/// requires each one to fire independently, so no single net can rot and leave
/// the others carrying a claim they were never checked for.
pub const PROBE_SOURCE: &str = r#"
const OPENWHISPR_HI: f32 = 0.70;
const OPENWHISPR_LO: f32 = 0.55;

pub fn shipped_bands() -> EnrollmentBands {
    EnrollmentBands::new(
        CosineSimilarity::new(OPENWHISPR_HI),
        CosineSimilarity::new(OPENWHISPR_LO),
    )
    .expect("well ordered")
}
"#;

/// Every reason `body` (the source of `file_name`) carries a tuned band.
///
/// One function, five nets, so a test can drive it over a synthetic file and
/// over the real tree and get the same answer for the same reason.
pub fn scan_source(file_name: &str, body: &str, extra_consts: &[(String, String)]) -> Vec<String> {
    let mut hits = Vec::new();
    let half = shipping_half(body);
    let code = shipping_code(body);
    let mut consts = const_table(&code);
    consts.extend_from_slice(extra_consts);

    // 5 — by name.
    for (n, line) in half.lines().enumerate() {
        let line_code = callsite::code_only(line);
        if is_tuned_band_line(&line_code) {
            hits.push(format!(
                "{file_name}:{}: named tuned band — {}",
                n + 1,
                line.trim()
            ));
        }
    }

    // 3 — any tuned float constant in the band module.
    if BAND_MODULES.contains(&file_name) {
        for decl in numeric_const_sites(&code) {
            hits.push(format!(
                "{file_name}: tuned float constant in the band module — {decl}"
            ));
        }
    }

    // 2 — literal constructor arguments, through one level of indirection.
    for ctor in BAND_CONSTRUCTORS {
        for args in call_arguments(&code, ctor) {
            let expanded = expand_consts(&args, &consts);
            if has_tuned_literal(&expanded) {
                hits.push(format!(
                    "{file_name}: literal band edge — {ctor}{}) [resolved: {}]",
                    args.trim(),
                    expanded.trim()
                ));
            }
        }
    }

    // 1 — provenance: who constructed a band type at all.
    for (ty, producers) in BAND_PRODUCERS {
        for (owner, snippet) in construction_sites(&code, ty) {
            if producers.contains(&owner.as_str()) {
                continue;
            }
            hits.push(format!(
                "{file_name}: `{snippet}` is constructed in `{owner}`, which is not a measured \
                 producer of {ty} (allowed: {producers:?})"
            ));
        }
    }

    // 4 — a band arriving as data.
    for (n, line) in half.lines().enumerate() {
        let line_code = callsite::code_only(line);
        if BAND_TYPES.iter().any(|t| line_code.contains(t))
            && DESERIALIZERS.iter().any(|d| line_code.contains(d))
        {
            hits.push(format!(
                "{file_name}:{}: a band deserialized from data is still a band nobody measured — {}",
                n + 1,
                line.trim()
            ));
        }
    }

    hits
}

/// Every place in `src/**/*.rs` that carries a tuned band.
pub fn tuned_band_sites() -> Vec<String> {
    let sources: Vec<(PathBuf, String)> = callsite::shipping_sources(&[])
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(&p).ok().map(|b| (p, b)))
        .collect();

    // One crate-wide constant table first: a band edge can be declared in one
    // module and used in another, which is one more level of indirection than
    // the per-file table would resolve.
    let mut globals: Vec<(String, String)> = Vec::new();
    for (_, body) in &sources {
        globals.extend(const_table(&shipping_code(body)));
    }

    let mut hits = Vec::new();
    for (path, body) in &sources {
        let name = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("<unnamed>");
        hits.extend(scan_source(name, body, &globals));
    }
    hits
}

// ---------------------------------------------------------------------------
// The measured block's machine-generated provenance
// ---------------------------------------------------------------------------

pub const PROVENANCE_BEGIN: &str = "<!-- BEGIN YV129 MEASUREMENT PROVENANCE -->";
pub const PROVENANCE_END: &str = "<!-- END YV129 MEASUREMENT PROVENANCE -->";

/// The harness-generated record a MEASURED block has to carry.
///
/// The first version of this gate read an adjective: flip `EER: UNMEASURED` to
/// `EER: 0.031 (measured)` and the gate opened. A word is not a measurement, so
/// what is checked now is a record only a run can produce, and it is checked for
/// internal consistency rather than read.
#[derive(Debug, Clone, PartialEq)]
pub struct Provenance {
    pub harness: String,
    pub run_id: String,
    pub corpus_digest: String,
    pub fixture: String,
    pub genuine: usize,
    pub impostor: usize,
    pub eer: f64,
    pub far_at_eer: f64,
    pub frr_at_eer: f64,
    pub eer_threshold: f64,
    pub target_far: f64,
    pub auto_confirm: f64,
    pub new_voice_floor: f64,
    pub far_at_auto_confirm: f64,
    pub frr_at_new_voice_floor: f64,
    /// `(threshold, far, frr)`, ascending by threshold — the printed sweep.
    pub sweep: Vec<(f64, f64, f64)>,
}

fn field<'a>(block: &'a str, key: &str) -> Result<&'a str, String> {
    block
        .lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix(&format!("{key}:")))
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("the provenance record has no `{key}:` line"))
}

fn number(block: &str, key: &str) -> Result<f64, String> {
    field(block, key)?
        .parse::<f64>()
        .map_err(|e| format!("`{key}` is not a number ({e})"))
}

/// Parse the provenance record out of a MEASURED block.
pub fn parse_provenance(block: &str) -> Result<Provenance, String> {
    let start = block
        .find(PROVENANCE_BEGIN)
        .ok_or_else(|| format!("the block carries no `{PROVENANCE_BEGIN}` record"))?;
    let end = block
        .find(PROVENANCE_END)
        .ok_or_else(|| format!("the block carries no `{PROVENANCE_END}` marker"))?;
    if end <= start {
        return Err("the provenance markers are out of order".to_string());
    }
    let body = &block[start + PROVENANCE_BEGIN.len()..end];

    let mut sweep = Vec::new();
    let mut in_sweep = false;
    for line in body.lines() {
        if line.trim().starts_with("sweep:") {
            in_sweep = true;
            continue;
        }
        if !in_sweep {
            continue;
        }
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let mut parts = t.split_whitespace();
        let (Some(th), Some(far), Some(frr)) = (parts.next(), parts.next(), parts.next()) else {
            in_sweep = false;
            continue;
        };
        let far = far.strip_prefix("far=").unwrap_or(far);
        let frr = frr.strip_prefix("frr=").unwrap_or(frr);
        match (th.parse::<f64>(), far.parse::<f64>(), frr.parse::<f64>()) {
            (Ok(th), Ok(far), Ok(frr)) => sweep.push((th, far, frr)),
            _ => in_sweep = false,
        }
    }

    Ok(Provenance {
        harness: field(body, "harness")?.to_string(),
        run_id: field(body, "run_id")?.to_string(),
        corpus_digest: field(body, "corpus_digest")?.to_string(),
        fixture: field(body, "fixture")?.to_string(),
        genuine: number(body, "genuine")? as usize,
        impostor: number(body, "impostor")? as usize,
        eer: number(body, "eer")?,
        far_at_eer: number(body, "far_at_eer")?,
        frr_at_eer: number(body, "frr_at_eer")?,
        eer_threshold: number(body, "eer_threshold")?,
        target_far: number(body, "target_far")?,
        auto_confirm: number(body, "auto_confirm")?,
        new_voice_floor: number(body, "new_voice_floor")?,
        far_at_auto_confirm: number(body, "far_at_auto_confirm")?,
        frr_at_new_voice_floor: number(body, "frr_at_new_voice_floor")?,
        sweep,
    })
}

fn multiple_of(value: f64, step: f64) -> bool {
    if step <= 0.0 {
        return false;
    }
    let k = (value / step).round();
    (value - k * step).abs() <= 1e-4
}

/// Everything wrong with a provenance record, as a list.
///
/// These are consistency checks, not taste: each one relates two numbers a real
/// run produces together. A forged block has to be arithmetically coherent with
/// its own corpus size, its own ROC and the rule
/// `speaker_profiles::bands_from_distribution` actually implements — which is a
/// different act from changing a word.
pub fn verify_provenance(p: &Provenance) -> Vec<String> {
    let mut bad = Vec::new();
    if !p.harness.contains("meeting_eval") {
        bad.push(format!(
            "`harness` must name the meeting_eval arm that produced this, got `{}`",
            p.harness
        ));
    }
    let digest = p.corpus_digest.strip_prefix("sha256:").unwrap_or("");
    if digest.len() != 64 || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
        bad.push(format!(
            "`corpus_digest` must be `sha256:<64 hex>`, got `{}`",
            p.corpus_digest
        ));
    }
    if p.genuine == 0 || p.impostor == 0 {
        bad.push("a distribution with an empty side is not a measurement".to_string());
        return bad;
    }
    let far_step = 1.0 / p.impostor as f64;
    let frr_step = 1.0 / p.genuine as f64;
    if !(0.0..0.5).contains(&p.eer) {
        bad.push(format!("`eer` {} is not below chance", p.eer));
    }
    if ((p.far_at_eer + p.frr_at_eer) / 2.0 - p.eer).abs() > 1e-4 {
        bad.push(format!(
            "`eer` {} is not the mean of far_at_eer {} and frr_at_eer {}",
            p.eer, p.far_at_eer, p.frr_at_eer
        ));
    }
    for (name, value, step) in [
        ("far_at_eer", p.far_at_eer, far_step),
        ("far_at_auto_confirm", p.far_at_auto_confirm, far_step),
        ("frr_at_eer", p.frr_at_eer, frr_step),
        ("frr_at_new_voice_floor", p.frr_at_new_voice_floor, frr_step),
    ] {
        if !multiple_of(value, step) {
            bad.push(format!(
                "`{name}` {value} is not a multiple of this sample's resolution {step:.4} — no run \
                 over {} genuine / {} impostor pairs can produce it",
                p.genuine, p.impostor
            ));
        }
    }
    if (p.new_voice_floor - p.eer_threshold).abs() > 1e-6 {
        bad.push(format!(
            "`new_voice_floor` {} is not the equal-error point {} — the shipped rule places the \
             floor AT the EER operating point, so a block that disagrees was not produced by it",
            p.new_voice_floor, p.eer_threshold
        ));
    }
    if (p.frr_at_new_voice_floor - p.frr_at_eer).abs() > 1e-4 {
        bad.push(
            "the floor is the EER threshold, so its FRR must equal frr_at_eer".to_string(),
        );
    }
    if p.auto_confirm <= p.new_voice_floor {
        bad.push(format!(
            "`auto_confirm` {} does not sit above the floor {}",
            p.auto_confirm, p.new_voice_floor
        ));
    }
    if p.target_far < far_step - 1e-9 {
        bad.push(format!(
            "`target_far` {} is finer than {} impostor pairs can express ({far_step:.4})",
            p.target_far, p.impostor
        ));
    }
    if p.far_at_auto_confirm > p.target_far + 1e-9 {
        bad.push(format!(
            "the auto-confirm edge's achieved FAR {} exceeds the budget {} it was placed to meet",
            p.far_at_auto_confirm, p.target_far
        ));
    }
    if p.sweep.len() < 3 {
        bad.push("`sweep:` must carry the printed ROC, at least three rows".to_string());
        return bad;
    }
    for pair in p.sweep.windows(2) {
        if pair[1].0 <= pair[0].0 {
            bad.push("the sweep must ascend by threshold".to_string());
            break;
        }
    }
    for pair in p.sweep.windows(2) {
        if pair[1].1 > pair[0].1 + 1e-9 || pair[1].2 < pair[0].2 - 1e-9 {
            bad.push(
                "the sweep is not a ROC: FAR must not rise and FRR must not fall as the threshold \
                 rises"
                    .to_string(),
            );
            break;
        }
    }
    for (name, threshold, far, frr) in [
        (
            "the EER point",
            p.eer_threshold,
            p.far_at_eer,
            p.frr_at_eer,
        ),
        (
            "the auto-confirm edge",
            p.auto_confirm,
            p.far_at_auto_confirm,
            f64::NAN,
        ),
    ] {
        match p
            .sweep
            .iter()
            .find(|(t, _, _)| (t - threshold).abs() <= 1e-4)
        {
            None => bad.push(format!(
                "{name} ({threshold}) has no row in the sweep — the operating point that shipped \
                 must appear in the curve it was read off"
            )),
            Some((_, sweep_far, sweep_frr)) => {
                if (sweep_far - far).abs() > 1e-4 {
                    bad.push(format!("{name}: sweep FAR {sweep_far} disagrees with {far}"));
                }
                if !frr.is_nan() && (sweep_frr - frr).abs() > 1e-4 {
                    bad.push(format!("{name}: sweep FRR {sweep_frr} disagrees with {frr}"));
                }
            }
        }
    }
    bad
}
