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
/// in this repository. The drift half of the gate binds where the source exists;
/// the substance half binds everywhere.
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

// ---------------------------------------------------------------------------
// "has a tuned band appeared in the shipping crate?"
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
/// threshold, and `IMPOSTOR_HEADROOM` is documented as an epsilon.
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
/// Exported so both tests can prove the predicate non-vacuous against synthetic
/// lines in both directions — a scanner nobody checked reports "clean" for a
/// reason it never states.
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

/// A file's shipping half: everything before the first `#[cfg(test)]`.
///
/// Unit tests inside `src/` are not shipping code, and they legitimately
/// construct bands from literals — that is how the band logic is exercised at
/// all. Scanning them would make this gate impossible to satisfy for the wrong
/// reason.
pub fn shipping_half(body: &str) -> String {
    match body.find("#[cfg(test)]") {
        Some(i) => body[..i].to_string(),
        None => body.to_string(),
    }
}

/// The file's shipping half with comments and string literals removed, on one
/// line, so a paren-balanced scan crosses line breaks.
pub fn shipping_code(body: &str) -> String {
    shipping_half(body)
        .lines()
        .map(|l| callsite::code_only(l))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Constructors whose arguments would be a tuned band if they were literals.
pub const BAND_CONSTRUCTORS: [&str; 2] = ["EnrollmentBands::new(", "ChipFloor::new("];

/// Every place in `src/**/*.rs` that carries a tuned band.
pub fn tuned_band_sites() -> Vec<String> {
    let mut hits = Vec::new();
    for path in callsite::shipping_sources(&[]) {
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (n, line) in shipping_half(&body).lines().enumerate() {
            let code = callsite::code_only(line);
            if is_tuned_band_line(&code) {
                hits.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
            }
        }
        let code = shipping_code(&body);
        for ctor in BAND_CONSTRUCTORS {
            for args in call_arguments(&code, ctor) {
                if has_tuned_literal(&args) {
                    hits.push(format!("{}: {ctor}{args})", path.display()));
                }
            }
        }
    }
    hits
}
