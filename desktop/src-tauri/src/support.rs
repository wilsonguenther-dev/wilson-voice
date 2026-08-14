//! YV98 — the support bundle, and the one button that sends it.
//!
//! Everything Yap has ever known about itself has stayed on the user's Mac.
//! This module is the first place that is deliberately not true: a diagnostics
//! zip the user can hand to support. That makes it the most privacy-sensitive
//! code in the app, so three rules shape all of it.
//!
//! * **Yap still makes no outbound connection.** This module opens no socket.
//!   There is no upload, no endpoint, no "off by default" client (the
//!   consent-gated HTTPS upload was explicitly declined — O5). The zip is
//!   written to the user's Desktop and handed to *their* mail client, which
//!   means the compose window is a window they read before anything is sent.
//!   `support_bundle_contents::support_adds_no_network_surface` asserts this
//!   file has no network surface at all, the same way `crash.rs` does — and
//!   `crash_no_network_surface` in that same file asserts THIS item added no
//!   network code to `crash.rs` either.
//! * **Nothing leaves without being redacted first.** `crash_events` rows have
//!   had an allowlist since YV64; `yap.log` never did — PRIVACY.md's claim that
//!   diagnostic logs contain no transcript text was an assertion, not a tested
//!   guarantee (finding #36). [`redact`] is that guarantee, and
//!   `support_bundle_redaction` is its test. It is an ALLOWLIST, not a filter,
//!   and it is an allowlist of MESSAGES rather than of words: after the
//!   structural rules below strip paths, quotes, panic payloads, emails, tokens
//!   and digit runs, the line that is left has to be, in full, a message this
//!   crate compiled in ([`crate::vocab`]), with only checked values in its
//!   holes — or it collapses to a marker and a count. Two earlier cuts of this
//!   rule were falsified in review, both because they were rules about WORDS: a
//!   length threshold that one digit could split, then a word allowlist that
//!   let single words, spoken numbers and doses through the gaps between the
//!   per-token rules. A rule about the whole line has no gaps to fall between.
//! * **The user sees exactly what is inside, first.** [`prepare`] builds the
//!   bundle in memory and hands back a preview of the REAL redacted bytes; the
//!   send step writes those same cached bytes. A preview that re-derives its
//!   contents is a preview of something else.
//!
//! `mailto:` is not used and cannot be: RFC 6068 defines no attachment
//! mechanism, so a `mailto:` "crash report" is a crash report with the report
//! missing. The primary path is `NSSharingService(named: .composeEmail)`,
//! guarded by `canPerformWithItems:` because it works with Mail.app and
//! generally not with Outlook/Airmail/Unibox. When that guard says no — and on
//! plenty of real Macs it will — the fallback is not an apology: the zip is
//! already on the Desktop, so Yap reveals it in Finder and puts the address on
//! the clipboard. Neither path is a dead end.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::Serialize;

/// Where a crash report goes. Also what lands on the clipboard when the mail
/// client cannot be driven.
pub const SUPPORT_EMAIL: &str = "wilson@drivia.consulting";

/// The app's own manifest, read at COMPILE time — so the version/OS block in a
/// bundle describes the binary that produced it, not whatever is on disk.
const TAURI_CONF: &str = include_str!("../tauri.conf.json");
/// The hand-maintained half of the shipped Info.plist (Tauri merges this with
/// the manifest above when it bundles).
const INFO_PLIST: &str = include_str!("../Info.plist");

/// Never pack more than this from one log file. Logs rotate at ~2 MB each, and
/// a bundle is something a person emails.
const MAX_LOG_BYTES: usize = 512 * 1024;
/// How much of an entry the preview shows. Enough to see the shape of the
/// redaction; not so much that the sheet becomes a log viewer.
const PREVIEW_CHARS: usize = 2_400;

// ---------------------------------------------------------------------------
// Redaction
// ---------------------------------------------------------------------------

/// A single token this long with no spaces is an opaque secret-shaped blob — a
/// license key, a token, a hash. Never worth shipping.
const OPAQUE_TOKEN_CHARS: usize = 28;
/// A digit run this long is an account number, a phone number or an id.
const LONG_DIGIT_RUN: usize = 7;

/// What a redacted span is replaced with. Visible on purpose: a user reading
/// the preview should be able to SEE that redaction happened, rather than
/// wonder whether the log was simply empty.
/// The prose marker is written `‹redacted-prose:12›` — the count lives
/// INSIDE the guillemets so the whole thing is one token. A marker containing a
/// space tokenises into two pieces, and the second piece is a token no later
/// rule recognises as a marker at all.
const R_PROSE: &str = "redacted-prose";
const R_PATH: &str = "‹path›";
const R_USER: &str = "‹user›";
const R_EMAIL: &str = "‹email›";
const R_QUOTED: &str = "‹redacted›";
const R_DIGITS: &str = "‹digits›";
const R_TOKEN: &str = "‹token›";

/// Redact one log line.
///
/// Order matters and is deliberate:
/// 1. a panic PAYLOAD is dropped entirely, keeping only the source location —
///    exactly what `crash::parse_panic_line` already does, and for the same
///    reason: a panic payload can embed any runtime value, transcripts
///    included;
/// 2. quoted spans go, because when Yap logs a piece of user content it quotes
///    it;
/// 3. email addresses and URL paths go;
/// 4. absolute filesystem paths go — they carry the account name, the folder
///    names a person chose, and often a document title;
/// 5. the home-directory username goes, catching bare mentions the path rule
///    could not reach;
/// 6. opaque tokens and long digit runs go;
/// 7. the line that is left has to BE a message Yap compiled in — one of its
///    own log messages, or a foreign diagnostic from a fixed list — with only
///    checked values in the holes, or it collapses to a marker and a count.
///
/// The `[timestamp LEVEL module::path]` header `env_logger` writes is kept
/// verbatim — it is a timestamp, a level, and a Rust module path, none of which
/// can carry user content. Kept only when it really is that, though:
/// [`split_header`] validates the shape, so a *line* that merely opens with a
/// bracket — the second line of a multi-line panic payload, say — is body and
/// is redacted like any other.
pub fn redact_line(line: &str, username: &str) -> String {
    let (head, body) = split_header(line);
    let mut body = body.to_string();

    body = drop_panic_payload(&body);
    body = redact_quoted(&body);
    body = redact_emails_and_urls(&body);
    body = redact_paths(&body);
    body = redact_username(&body, username);
    body = redact_opaque(&body);
    body = enforce_vocabulary(&body);

    format!("{head}{body}")
}

/// Redact a whole log file, line by line.
pub fn redact(text: &str, username: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        out.push_str(&redact_line(line, username));
        out.push('\n');
    }
    out
}

/// `[2026-08-09T17:53:02Z ERROR wilson_voice_lib::logging] message` →
/// `("[…logging] ", "message")`. Anything that is not that exact header is all
/// body, and goes through every redaction rule.
///
/// The shape is CHECKED, and the check is the whole point. The first cut
/// trusted any line starting with `[` and kept everything up to the first `]`
/// verbatim — which is a hole, not a nicety, because a log record is not always
/// one line: `logging`'s panic hook writes `PANIC at {loc}: {msg}\nbacktrace:\n
/// {bt}`, and a payload's continuation lines arrive here as whole lines of
/// their own. A sentence that merely *began* with a bracket — `[her HIV result
/// came back positive] and I …` — was therefore emitted unredacted up to the
/// bracket, in the one module whose entire claim is that nothing but Yap's own
/// words leaves the machine. Validating the header closes that: a timestamp, a
/// level and a module path are things Yap writes, and nothing a person says can
/// be mistaken for all three in order.
pub fn split_header(line: &str) -> (&str, &str) {
    if !line.starts_with('[') {
        return ("", line);
    }
    let Some(close) = line.find(']') else {
        return ("", line);
    };
    if !is_env_logger_header(&line[1..close]) {
        return ("", line);
    }
    line.split_at(close + 1)
}

/// `2026-08-09T17:53:02Z ERROR wilson_voice_lib::logging` — an RFC 3339
/// timestamp, a log level and a Rust module path, in that order, and nothing
/// else between the brackets. This is exactly what `env_logger`'s default
/// format writes, which is the only thing `logging::init` configures.
fn is_env_logger_header(inner: &str) -> bool {
    let mut parts = inner.split_whitespace();
    let (Some(timestamp), Some(level), Some(module), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    DateTime::parse_from_rfc3339(timestamp).is_ok()
        && matches!(level, "ERROR" | "WARN" | "INFO" | "DEBUG" | "TRACE")
        && is_module_path(module)
}

/// `wilson_voice_lib`, `wilson_voice_lib::db`, `tauri_plugin_updater` — Rust
/// identifiers joined by `::`, and nothing that is not one.
fn is_module_path(s: &str) -> bool {
    !s.is_empty()
        && s.split("::").all(|seg| {
            let mut chars = seg.chars();
            matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// The panic hook writes `PANIC at <loc>: <payload>`. Keep the location.
///
/// The `: ` separator is kept as well as the location, because what the hook
/// writes is `log::error!("PANIC at {location}: {message}…")` and the line has
/// to still BE that message afterwards — [`enforce_vocabulary`] matches whole
/// compiled-in messages, and a line the redactor itself reshaped into something
/// Yap never writes would be redacted a second time, by its own hand.
fn drop_panic_payload(body: &str) -> String {
    let Some(at) = body.find("PANIC at ") else {
        return body.to_string();
    };
    let rest = &body[at + "PANIC at ".len()..];
    // The location is `file:line:col` (or `unknown`); it ends at the first
    // `: ` separator the hook writes between location and payload. Everything
    // after that separator is the payload, and the payload is the half
    // `crash::parse_panic_line` already refuses to store.
    let loc_end = rest.find(": ").unwrap_or(rest.len());
    format!("{}PANIC at {}: {R_QUOTED}", &body[..at], &rest[..loc_end])
}

/// `"…"`, `` `…` `` and `“…”` spans. Single quotes are deliberately NOT a
/// delimiter: `don't` would open a span that never closes.
fn redact_quoted(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        let close = match c {
            '"' => Some('"'),
            '`' => Some('`'),
            '\u{201C}' => Some('\u{201D}'),
            _ => None,
        };
        let Some(close) = close else {
            out.push(c);
            continue;
        };
        let mut span = String::new();
        let mut closed = false;
        for c2 in chars.by_ref() {
            if c2 == close {
                closed = true;
                break;
            }
            span.push(c2);
        }
        if closed {
            out.push_str(R_QUOTED);
        } else {
            // Unterminated: it was punctuation, not a span. Put it back.
            out.push(c);
            out.push_str(&span);
        }
    }
    out
}

/// Emails vanish. URLs keep scheme + host (a failing model download is a real
/// support case) and lose everything after it, which is where a token would be.
fn redact_emails_and_urls(body: &str) -> String {
    map_tokens(body, |tok| {
        let (core, lead, trail) = trim_punct(tok);
        if already_redacted(core) {
            return None;
        }
        if core.contains("://") {
            let host_end = core[core.find("://").unwrap() + 3..]
                .find('/')
                .map(|i| core.find("://").unwrap() + 3 + i);
            let kept = match host_end {
                Some(i) => format!("{}/{R_QUOTED}", &core[..i]),
                None => core.to_string(),
            };
            return Some(format!("{lead}{kept}{trail}"));
        }
        if core.contains('@')
            && core.split('@').count() == 2
            && core.split('@').nth(1)?.contains('.')
        {
            return Some(format!("{lead}{R_EMAIL}{trail}"));
        }
        None
    })
}

/// Anything with a path separator in it, absolute or not.
///
/// Two carve-outs, both deliberate. Rust source locations (`src/dictation.rs`,
/// `src/transcription.rs:212:9`) are kept: they are repo-relative identifiers
/// that ship in the binary already, and they are the single most useful thing
/// in a crash log. Pure `n/m` ratios are kept because they are numbers.
/// Everything else that contains a `/` — including the tail of a path with a
/// space in it, which token-wise arrives here on its own — goes.
fn redact_paths(body: &str) -> String {
    map_tokens(body, |tok| {
        let (core, lead, trail) = trim_punct(tok);
        if already_redacted(core) || !core.contains('/') {
            return None;
        }
        // A Rust source location, never an account name.
        let absolute = core.starts_with('/') || core.starts_with("~/");
        if !absolute && (core.contains(".rs:") || core.ends_with(".rs")) {
            return None;
        }
        // `1/2`, `3/4` — arithmetic, not a filesystem.
        if core.chars().all(|c| c.is_ascii_digit() || c == '/') {
            return None;
        }
        Some(format!("{lead}{R_PATH}{trail}"))
    })
}

/// A token a previous rule already replaced. Every rule skips these, so the
/// pipeline can never redact its own markers into nonsense.
fn already_redacted(core: &str) -> bool {
    core.contains('\u{2039}')
}

/// Bare mentions of the account name — `Users` is gone by now, but log lines
/// say things like `keychain item for wilsonguenther`.
fn redact_username(body: &str, username: &str) -> String {
    if username.trim().is_empty() {
        return body.to_string();
    }
    let lower_user = username.to_lowercase();
    map_tokens(body, |tok| {
        let (core, lead, trail) = trim_punct(tok);
        if already_redacted(core) {
            return None;
        }
        if core.to_lowercase() == lower_user {
            return Some(format!("{lead}{R_USER}{trail}"));
        }
        // Also catches `wilsonguenther@…`-shaped leftovers and
        // `WilsonVoice-wilsonguenther.db`-shaped file names.
        if core.to_lowercase().contains(&lower_user) {
            return Some(format!("{lead}{R_USER}{trail}"));
        }
        None
    })
}

/// Secrets that are not prose: an opaque blob, or a long number.
fn redact_opaque(body: &str) -> String {
    let per_token = map_tokens(body, |tok| {
        let (core, lead, trail) = trim_punct(tok);
        if already_redacted(core) {
            return None;
        }
        if core.chars().count() >= OPAQUE_TOKEN_CHARS
            && core
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+' | '='))
            && core.chars().any(|c| c.is_ascii_digit())
        {
            return Some(format!("{lead}{R_TOKEN}{trail}"));
        }
        let digits = core.chars().filter(|c| c.is_ascii_digit()).count();
        if digits >= LONG_DIGIT_RUN && core.chars().all(|c| c.is_ascii_digit() || c == '-') {
            return Some(format!("{lead}{R_DIGITS}{trail}"));
        }
        None
    });
    collapse_digit_runs(&per_token)
}

/// A phone number, a card number or an account number does not arrive as one
/// token when somebody SAID it: ASR writes `512 738 7951` and
/// `4111 1111 1111 1111`, which is three or four tokens of three or four digits
/// each, and a per-token threshold of seven that never fires. Both of those
/// shipped out of a support bundle verbatim. The digits are counted across a
/// run of adjacent bare numbers instead, which is the unit a person speaks them
/// in.
fn collapse_digit_runs(body: &str) -> String {
    let indent: String = body.chars().take_while(|c| *c == ' ').collect();
    let toks: Vec<&str> = body.split_whitespace().collect();
    let mut out: Vec<String> = Vec::with_capacity(toks.len());
    let mut i = 0usize;
    while i < toks.len() {
        let mut j = i;
        let mut digits = 0usize;
        while j < toks.len() && is_bare_number(toks[j]) {
            digits += toks[j].chars().filter(|c| c.is_ascii_digit()).count();
            j += 1;
        }
        if j == i {
            out.push(toks[i].to_string());
            i += 1;
        } else if digits >= LONG_DIGIT_RUN {
            out.push(R_DIGITS.to_string());
            i = j;
        } else {
            out.extend(toks[i..j].iter().map(|t| (*t).to_string()));
            i = j;
        }
    }
    format!("{indent}{}", out.join(" "))
}

/// A token that is a number and nothing else — `7951`, `(512)`, `1,024`,
/// `0.8.0`. A source location (`212:9`) is not one: it carries a colon, and it
/// is the half of a crash log worth keeping.
fn is_bare_number(tok: &str) -> bool {
    let (core, _, _) = trim_punct(tok);
    !core.is_empty()
        && core.chars().any(|c| c.is_ascii_digit())
        && core
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '-' | '.' | ','))
}

/// The line has to BE a message Yap compiled in. This is the rule that catches
/// a transcript, and the third attempt at it.
///
/// Version one counted words: nine word-like tokens in a row were assumed to be
/// prose. One digit in a dictated sentence split the run and shipped the whole
/// utterance.
///
/// Version two inverted that into an allowlist of Yap's own words, matched as
/// contiguous phrases. It was falsified three ways at once, and all three were
/// the same mistake — *a rule about words says nothing about a line*. Single
/// words are their own phrases, so `her HIV+ result` walked out between two
/// non-word tokens with both of its words individually accounted for. Tokens
/// that are not words were governed by nothing, so `4111 1111 1111 1111`,
/// `20mg`, `A1C`, `9.4`, `COVID-19` and `P.O.` shipped. And the corpus itself
/// held ~6,000 words of `#[cfg(test)]` English written to look like dictation.
///
/// The rule now is about the whole line: the body survives only if it is, in
/// full, one message this crate compiled in, with every interpolated value
/// checked (see [`crate::vocab`]). There is no seam between rules for a token
/// class to fall through, because there are no per-token survival rules left to
/// fall between — either the line is a message Yap wrote, or it is not.
///
/// A line that is not gets a marker and a count. It keeps only the tokens that
/// stand on their own with nothing to explain them: a marker an earlier rule
/// left, a Rust path, a source location. Numbers are not on that list, on
/// purpose.
fn enforce_vocabulary(body: &str) -> String {
    let indent: String = body.chars().take_while(|c| *c == ' ').collect();
    let norm = crate::vocab::normalize(body);
    if norm.is_empty() {
        return body.to_string();
    }
    // `std::backtrace`'s two shapes — the most useful lines in a crash report,
    // and neither of them reachable from speech.
    if crate::vocab::is_backtrace_frame(&norm) {
        return body.to_string();
    }
    if crate::vocab::templates().explains(&norm) {
        return body.to_string();
    }

    let mut out: Vec<String> = Vec::new();
    let mut dropped = 0usize;
    for tok in norm.split_whitespace() {
        if crate::vocab::token_is_structural(tok) {
            flush_dropped(&mut dropped, &mut out);
            out.push(tok.to_string());
        } else {
            dropped += 1;
        }
    }
    flush_dropped(&mut dropped, &mut out);
    format!("{indent}{}", out.join(" "))
}

/// The tokens on this line that nothing accounts for.
///
/// Empty when the line is, in full, a message this crate compiled in (or one of
/// `std::backtrace`'s two shapes), and empty when every token left standing is
/// structural on its own. Public because the guarantee is asserted from OUTSIDE
/// this module, over every token of the packed bundle rather than over the word
/// runs in it — the previous coverage test iterated word runs, which is exactly
/// why it could not see the class of leak that shipped (`4111 1111 1111 1111`,
/// `20mg`, `A1C`, `P.O.`).
pub fn unexplained_tokens(body: &str) -> Vec<String> {
    let norm = crate::vocab::normalize(body);
    // A long number is never something the compiler put in the binary — it can
    // only ever have arrived through a hole — so it is checked BEFORE the
    // message match rather than inside it. Without this the coverage test would
    // be blind to exactly the class that shipped: `{}`-leading messages exist,
    // a run of bare numbers is value-shaped, and so a spoken card number would
    // be "explained" by a message it was merely interpolated into.
    let toks: Vec<&str> = norm.split_whitespace().collect();
    let mut i = 0usize;
    while i < toks.len() {
        let mut j = i;
        let mut digits = 0usize;
        while j < toks.len() && is_bare_number(toks[j]) {
            digits += toks[j].chars().filter(|c| c.is_ascii_digit()).count();
            j += 1;
        }
        if j > i {
            if digits >= LONG_DIGIT_RUN {
                return toks[i..j].iter().map(|t| (*t).to_string()).collect();
            }
            i = j;
        } else {
            i += 1;
        }
    }
    if norm.is_empty()
        || crate::vocab::is_backtrace_frame(&norm)
        || crate::vocab::templates().explains(&norm)
    {
        return Vec::new();
    }
    norm.split_whitespace()
        .filter(|t| !crate::vocab::token_is_structural(t))
        .map(str::to_string)
        .collect()
}

fn flush_dropped(dropped: &mut usize, out: &mut Vec<String>) {
    if *dropped > 0 {
        // One token, no spaces inside it: a marker that tokenises into two
        // pieces is a marker that half of every later rule cannot recognise.
        out.push(format!("\u{2039}{R_PROSE}:{dropped}\u{203A}"));
        *dropped = 0;
    }
}

/// Apply `f` to each whitespace-separated token, keeping the token when `f`
/// returns `None`. Whitespace is normalised to single spaces, which is fine for
/// a log line and keeps every rule above a one-liner.
fn map_tokens(body: &str, f: impl Fn(&str) -> Option<String>) -> String {
    let indent: String = body.chars().take_while(|c| *c == ' ').collect();
    let mapped: Vec<String> = body
        .split_whitespace()
        .map(|t| f(t).unwrap_or_else(|| t.to_string()))
        .collect();
    format!("{indent}{}", mapped.join(" "))
}

/// Split `(foo.rs:12)` into `("foo.rs:12", "(", ")")` so a rule can match the
/// core without losing the punctuation a log line wrapped it in.
fn trim_punct(tok: &str) -> (&str, &str, &str) {
    let lead_len = tok
        .char_indices()
        .take_while(|(_, c)| matches!(c, '(' | '[' | '{' | '<' | '"' | '\''))
        .map(|(i, c)| i + c.len_utf8())
        .last()
        .unwrap_or(0);
    let rest = &tok[lead_len..];
    let trail_start = rest
        .char_indices()
        .rev()
        .take_while(|(_, c)| {
            matches!(
                c,
                ')' | ']' | '}' | '>' | '"' | '\'' | ',' | '.' | ';' | ':'
            )
        })
        .map(|(i, _)| i)
        .last()
        .unwrap_or(rest.len());
    (&rest[..trail_start], &tok[..lead_len], &rest[trail_start..])
}

// ---------------------------------------------------------------------------
// The bundle
// ---------------------------------------------------------------------------

/// One file inside the zip. Text only, by construction — a bundle that could
/// carry a wav or a database could carry a recording.
#[derive(Debug, Clone)]
pub struct BundleEntry {
    pub name: String,
    pub text: String,
}

/// Everything the builder needs, so the builder itself touches no globals and
/// is testable with zero app state.
#[derive(Debug, Clone, Default)]
pub struct BundleInputs {
    /// `crash::summary_text(&rows)`.
    pub crash_summary: String,
    /// `(file name, raw contents)` — `yap.log` first, then its rotations.
    pub logs: Vec<(String, String)>,
    /// Rendered by [`environment_block`].
    pub environment: String,
    /// Rendered by [`permissions_block`].
    pub permissions: String,
    /// Rendered by [`models_block`].
    pub models: String,
    /// The account name to scrub. Derived from the home directory, never typed.
    pub username: String,
    pub generated_at: DateTime<Utc>,
}

/// Build the bundle's entries. Pure: same inputs, same bytes, no I/O.
///
/// Every log file goes through [`redact`] here and nowhere else, so there is
/// exactly one door out of this module and one place its test has to guard.
pub fn build_entries(inputs: &BundleInputs) -> Vec<BundleEntry> {
    let mut entries = vec![BundleEntry {
        name: "README.txt".into(),
        text: readme_text(inputs.generated_at),
    }];

    entries.push(BundleEntry {
        name: "environment.txt".into(),
        text: inputs.environment.clone(),
    });
    entries.push(BundleEntry {
        name: "crash-summary.txt".into(),
        text: inputs.crash_summary.clone(),
    });
    entries.push(BundleEntry {
        name: "permissions.txt".into(),
        text: inputs.permissions.clone(),
    });
    entries.push(BundleEntry {
        name: "models.txt".into(),
        text: inputs.models.clone(),
    });

    for (name, raw) in &inputs.logs {
        let tail = tail_bytes(raw, MAX_LOG_BYTES);
        entries.push(BundleEntry {
            name: format!("logs/{name}"),
            text: redact(tail, &inputs.username),
        });
    }
    entries
}

fn readme_text(at: DateTime<Utc>) -> String {
    format!(
        "Yap — diagnostics bundle\n\
         generated: {}\n\
         \n\
         What this is: the state of one Yap install, packed so it can be emailed\n\
         to {SUPPORT_EMAIL}.\n\
         \n\
         What is in it: this file, the app/OS version block, the local crash\n\
         summary, permission states, the model catalog state, and yap.log plus\n\
         its rotations.\n\
         \n\
         What is NOT in it: any recording, any database, any transcript. Every\n\
         log line was run through a redaction pass before it was packed —\n\
         filesystem paths, your account name, email addresses, URL paths, long\n\
         opaque tokens and long numbers are replaced with a marker, and then the\n\
         line itself has to be, in full, a message Yap ships inside itself (one\n\
         of its own log messages, or a system error string from a fixed list),\n\
         with only values — numbers, file names, markers — in the gaps. Anything\n\
         else becomes a marker and a count. A sentence you spoke is never one of\n\
         Yap's own messages, and a number you spoke is not a value Yap put\n\
         there. You can read every byte of the zip; Yap showed it to you before\n\
         writing it.\n\
         \n\
         Nothing here was uploaded. Yap has no network path for this: the file\n\
         was written to your Desktop and handed to your mail client, and you are\n\
         the one who presses send.\n",
        at.to_rfc3339()
    )
}

/// Keep the END of a log — the tail is where the failure is.
///
/// `text.len() - max` is a BYTE offset into a `&str`, and a byte offset lands
/// inside a character as often as Yap's own log lines contain one: `→` and `—`
/// are three bytes each and appear in the per-dictation hot paths. Slicing
/// there panics — inside the one feature a user reaches when the app is already
/// failing them. So the offset is walked forward to the next character boundary
/// before anything is sliced.
fn tail_bytes(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut start = text.len() - max;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    match text[start..].find('\n') {
        Some(i) => &text[start + i + 1..],
        None => &text[start..],
    }
}

/// `Yap-Diagnostics-0.8.0-20260812-141530.zip`.
pub fn bundle_file_name(version: &str, at: DateTime<Utc>) -> String {
    format!(
        "Yap-Diagnostics-{version}-{:04}{:02}{:02}-{:02}{:02}{:02}.zip",
        at.year(),
        at.month(),
        at.day(),
        at.hour(),
        at.minute(),
        at.second()
    )
}

// ---------------------------------------------------------------------------
// The version/OS block, permission states, catalog state
// ---------------------------------------------------------------------------

/// The value of `<key>K</key><string>V</string>` in a plist, without pulling a
/// plist parser in for four keys.
fn plist_string(xml: &str, key: &str) -> Option<String> {
    let needle = format!("<key>{key}</key>");
    let after = &xml[xml.find(&needle)? + needle.len()..];
    let open = after.find("<string>")? + "<string>".len();
    let close = after[open..].find("</string>")?;
    Some(after[open..open + close].trim().to_string())
}

fn conf_str(conf: &serde_json::Value, path: &[&str]) -> String {
    let mut cur = conf;
    for k in path {
        cur = &cur[*k];
    }
    cur.as_str().unwrap_or("unknown").to_string()
}

/// The Info.plist version/OS block: what this binary claims to be, and what it
/// is running on. Both halves matter — "0.8.0 on macOS 12" is a different bug
/// report from "0.8.0 on macOS 26".
pub fn environment_block(os_version: &str, arch: &str) -> String {
    let conf: serde_json::Value = serde_json::from_str(TAURI_CONF).unwrap_or_default();
    let display = plist_string(INFO_PLIST, "CFBundleDisplayName")
        .or_else(|| plist_string(INFO_PLIST, "CFBundleName"))
        .unwrap_or_else(|| "Yap".into());
    format!(
        "[Info.plist]\n\
         CFBundleDisplayName: {display}\n\
         CFBundleIdentifier: {}\n\
         CFBundleShortVersionString: {}\n\
         LSMinimumSystemVersion: {}\n\
         \n\
         [OS]\n\
         os: {os_version}\n\
         arch: {arch}\n\
         build: {}\n",
        conf_str(&conf, &["identifier"]),
        conf_str(&conf, &["version"]),
        conf_str(&conf, &["bundle", "macOS", "minimumSystemVersion"]),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
    )
}

/// `sw_vers`, best effort. A process spawn, not a socket.
pub fn os_version() -> String {
    let read = |arg: &str| -> Option<String> {
        let out = std::process::Command::new("/usr/bin/sw_vers")
            .arg(arg)
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!s.is_empty()).then_some(s)
    };
    match (read("-productVersion"), read("-buildVersion")) {
        (Some(v), Some(b)) => format!("macOS {v} ({b})"),
        (Some(v), None) => format!("macOS {v}"),
        _ => "unknown".into(),
    }
}

/// Permission states, as booleans and one sentence — never the probe's own
/// free text about a device name.
pub fn permissions_block(report: &crate::permissions::PermissionReport) -> String {
    format!(
        "accessibility: {}\nmicrophone: {}\nasr_model_ready: {}\nasr_detail: {}\nall_critical_ok: {}\n",
        report.accessibility, report.microphone, report.asr_ok, report.asr_detail, report.all_critical_ok
    )
}

/// Which models are on disk. "Yap cannot transcribe" is a catalog question
/// nine times out of ten.
pub fn models_block(selected_native: &str) -> String {
    let mut out = format!("selected_asr_model: {selected_native}\n\n[asr catalog]\n");
    for m in &crate::models::catalog().models {
        out.push_str(&format!(
            "{}: downloaded={}\n",
            m.id,
            crate::models::is_downloaded(m)
        ));
    }
    out.push_str("\n[polish catalog]\n");
    for m in crate::models::polish_models() {
        out.push_str(&format!(
            "{}: downloaded={}\n",
            m.id,
            crate::models::is_polish_downloaded(m)
        ));
    }
    out
}

/// Read `yap.log` and its rotations, newest first.
pub fn read_logs(logs_dir: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for name in log_file_names() {
        let path = logs_dir.join(&name);
        if let Ok(bytes) = std::fs::read(&path) {
            out.push((name, String::from_utf8_lossy(&bytes).into_owned()));
        }
    }
    out
}

/// `yap.log`, `yap.log.1` … — the same set `logging`'s rotation writes.
pub fn log_file_names() -> Vec<String> {
    let mut names = vec!["yap.log".to_string()];
    for i in 1..=crate::logging::MAX_BACKUPS {
        names.push(format!("yap.log.{i}"));
    }
    names
}

/// The account name, taken from the home directory. Never asked for, never
/// typed — the thing we are scrubbing is exactly the thing macOS put in the
/// path.
pub fn username_from_home(home: &Path) -> String {
    home.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Preview + prepared bundle
// ---------------------------------------------------------------------------

/// One row in the preview sheet: what the file is called, how big it is, and
/// the first [`PREVIEW_CHARS`] of the ACTUAL redacted bytes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewEntry {
    pub name: String,
    pub bytes: usize,
    pub lines: usize,
    pub excerpt: String,
    pub truncated: bool,
}

/// The preview payload, and the handle the send step uses.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundlePreview {
    pub file_name: String,
    pub recipient: String,
    /// `canPerformWithItems:` said yes — i.e. a compose window is achievable.
    /// `false` is not an error; it is the fallback path, announced up front so
    /// the sheet can tell the truth about what the button will do.
    pub mail_available: bool,
    pub total_bytes: usize,
    pub entries: Vec<PreviewEntry>,
}

/// The built-but-not-yet-written bundle. Held in app state between the preview
/// and the send so the two are the same bytes.
#[derive(Debug, Clone)]
pub struct PreparedBundle {
    pub file_name: String,
    pub entries: Vec<BundleEntry>,
    pub generated_at: DateTime<Utc>,
}

impl PreparedBundle {
    pub fn preview(&self, mail_available: bool) -> BundlePreview {
        BundlePreview {
            file_name: self.file_name.clone(),
            recipient: SUPPORT_EMAIL.to_string(),
            mail_available,
            total_bytes: self.entries.iter().map(|e| e.text.len()).sum(),
            entries: self
                .entries
                .iter()
                .map(|e| {
                    let truncated = e.text.chars().count() > PREVIEW_CHARS;
                    PreviewEntry {
                        name: e.name.clone(),
                        bytes: e.text.len(),
                        lines: e.text.lines().count(),
                        excerpt: e.text.chars().take(PREVIEW_CHARS).collect(),
                        truncated,
                    }
                })
                .collect(),
        }
    }
}

/// Build the bundle from inputs. The only constructor.
pub fn prepare(version: &str, inputs: BundleInputs) -> PreparedBundle {
    PreparedBundle {
        file_name: bundle_file_name(version, inputs.generated_at),
        entries: build_entries(&inputs),
        generated_at: inputs.generated_at,
    }
}

/// `~/Desktop`, falling back to the home directory itself — a Mac without a
/// Desktop folder should still get its zip somewhere findable.
pub fn desktop_dir(home: &Path) -> PathBuf {
    let desktop = home.join("Desktop");
    if desktop.is_dir() {
        desktop
    } else {
        home.to_path_buf()
    }
}

// ---------------------------------------------------------------------------
// zip (stored, no compression)
// ---------------------------------------------------------------------------

/// Write the entries as a zip archive.
///
/// Hand-rolled and STORE-only, in the same spirit as `logging`'s hand-rolled
/// rotation: the alternative is a compression stack in the dependency graph of
/// a signed, notarised app so that five text files can be 3 KB smaller. The
/// format written here is the plain PKZIP 2.0 subset — local header, data,
/// central directory, end record — which every unzip on earth reads, and
/// `support_bundle_contents` proves it by unzipping the real thing.
pub fn write_zip(path: &Path, entries: &[BundleEntry], at: DateTime<Utc>) -> std::io::Result<u64> {
    use std::io::Write;

    let (dos_time, dos_date) = dos_timestamp(at);
    let mut out: Vec<u8> = Vec::new();
    let mut directory: Vec<u8> = Vec::new();

    for entry in entries {
        let name = entry.name.as_bytes();
        let data = entry.text.as_bytes();
        let crc = crc32(data);
        let offset = out.len() as u32;

        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes()); // local file header
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0x0800u16.to_le_bytes()); // flags: UTF-8 names
        out.extend_from_slice(&0u16.to_le_bytes()); // method: stored
        out.extend_from_slice(&dos_time.to_le_bytes());
        out.extend_from_slice(&dos_date.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra len
        out.extend_from_slice(name);
        out.extend_from_slice(data);

        directory.extend_from_slice(&0x0201_4b50u32.to_le_bytes()); // central header
        directory.extend_from_slice(&20u16.to_le_bytes()); // version made by
        directory.extend_from_slice(&20u16.to_le_bytes()); // version needed
        directory.extend_from_slice(&0x0800u16.to_le_bytes());
        directory.extend_from_slice(&0u16.to_le_bytes());
        directory.extend_from_slice(&dos_time.to_le_bytes());
        directory.extend_from_slice(&dos_date.to_le_bytes());
        directory.extend_from_slice(&crc.to_le_bytes());
        directory.extend_from_slice(&(data.len() as u32).to_le_bytes());
        directory.extend_from_slice(&(data.len() as u32).to_le_bytes());
        directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
        directory.extend_from_slice(&0u16.to_le_bytes()); // extra
        directory.extend_from_slice(&0u16.to_le_bytes()); // comment
        directory.extend_from_slice(&0u16.to_le_bytes()); // disk start
        directory.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        directory.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        directory.extend_from_slice(&offset.to_le_bytes());
        directory.extend_from_slice(name);
    }

    let dir_offset = out.len() as u32;
    let dir_size = directory.len() as u32;
    out.extend_from_slice(&directory);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes()); // end of central directory
    out.extend_from_slice(&0u16.to_le_bytes()); // this disk
    out.extend_from_slice(&0u16.to_le_bytes()); // disk with directory
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&dir_size.to_le_bytes());
    out.extend_from_slice(&dir_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment len

    let mut file = std::fs::File::create(path)?;
    file.write_all(&out)?;
    file.sync_all()?;
    Ok(out.len() as u64)
}

/// MS-DOS date/time, which is what a zip header carries.
fn dos_timestamp(at: DateTime<Utc>) -> (u16, u16) {
    let year = at.year().clamp(1980, 2107) as u16;
    let date = ((year - 1980) << 9) | ((at.month() as u16) << 5) | at.day() as u16;
    let time = ((at.hour() as u16) << 11) | ((at.minute() as u16) << 5) | (at.second() as u16 / 2);
    (time, date)
}

fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *slot = c;
        }
        t
    });
    let mut crc = 0xFFFF_FFFFu32;
    for b in data {
        crc = table[((crc ^ *b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

// ---------------------------------------------------------------------------
// Compose / reveal — the two ways out, neither a dead end
// ---------------------------------------------------------------------------

/// What actually happened when the user pressed the button.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendOutcome {
    /// `"compose"` — a pre-filled mail window is open, with the zip attached
    /// and nothing sent yet. `"reveal"` — the zip is selected in Finder and the
    /// address is on the clipboard.
    pub method: String,
    pub path: String,
    pub recipient: String,
    /// The one sentence the UI shows. Written here so both paths say something
    /// true and specific rather than "done".
    pub message: String,
}

pub fn subject_line(version: &str, headline: &str) -> String {
    if headline.is_empty() {
        format!("Yap crash report — v{version}")
    } else {
        format!("Yap crash report — v{version} — {headline}")
    }
}

pub fn body_text(version: &str, os: &str, headline: &str) -> String {
    let what = if headline.is_empty() {
        "No crash was recorded on this Mac — sending diagnostics anyway.".to_string()
    } else {
        format!("Most recent crash: {headline}")
    };
    format!(
        "Yap {version} on {os}\n{what}\n\n\
         The attached zip is a diagnostics bundle. It contains logs, the crash\n\
         summary, permission states and the model catalog state — no recordings,\n\
         no transcripts, no database. Yap redacted the logs before packing them:\n\
         paths, account names, tokens and long numbers are gone, and every word\n\
         left is one the app itself ships. It showed me the contents first.\n\n\
         What I was doing when it happened:\n\n"
    )
}

/// The macOS compose path. `check_only` answers "would this work?" without
/// opening anything, which is what the preview sheet needs.
///
/// MUST be called on the main thread — `NSSharingService` is AppKit.
#[cfg(target_os = "macos")]
pub fn compose_email(zip: Option<&Path>, subject: &str, body: &str, check_only: bool) -> bool {
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSSharingService, NSSharingServiceNameComposeEmail};
    use objc2_foundation::{NSArray, NSString, NSURL};

    // SAFETY: a read-only framework static.
    let name = unsafe { NSSharingServiceNameComposeEmail };
    let Some(service) = NSSharingService::sharingServiceNamed(name) else {
        // No compose service at all — the guard the plan calls load-bearing.
        return false;
    };

    let recipients = NSArray::from_retained_slice(&[NSString::from_str(SUPPORT_EMAIL)]);
    service.setRecipients(Some(&recipients));
    service.setSubject(Some(&NSString::from_str(subject)));

    let body_string = NSString::from_str(body);
    let url = zip.map(|p| NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy())));

    let body_item: &AnyObject = &body_string;
    let items = match &url {
        Some(u) => {
            let url_item: &AnyObject = u;
            NSArray::from_slice(&[body_item, url_item])
        }
        None => NSArray::from_slice(&[body_item]),
    };

    // SAFETY: both items are NSPasteboardWriting conformers (NSString, NSURL),
    // which is exactly what the generic `items` parameter documents.
    if !unsafe { service.canPerformWithItems(Some(&items)) } {
        return false;
    }
    if check_only {
        return true;
    }
    // Opens a compose window. It CANNOT send in the background, which is the
    // property that makes this consistent with everything Yap claims: the user
    // reads the mail before it leaves.
    unsafe { service.performWithItems(&items) };
    true
}

#[cfg(not(target_os = "macos"))]
pub fn compose_email(_zip: Option<&Path>, _subject: &str, _body: &str, _check_only: bool) -> bool {
    false
}

/// Select the file in Finder. Not `open <dir>` — the user needs THIS file.
pub fn reveal_in_finder(path: &Path) {
    if let Err(e) = std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(path)
        .spawn()
    {
        log::warn!("support bundle: reveal failed ({e})");
    }
}

/// Put the support address on the clipboard, so the fallback is one paste away.
///
/// MUST be called on the main thread.
#[cfg(target_os = "macos")]
pub fn copy_address_to_clipboard() {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::{NSArray, NSString};

    let pb = NSPasteboard::generalPasteboard();
    // SAFETY: NSPasteboardTypeString is a read-only framework static.
    let string_type = unsafe { NSPasteboardTypeString };
    let types = NSArray::from_slice(&[string_type]);
    // SAFETY: no owner, so there is no promise to fulfil later — the bytes are
    // written immediately below. (The lazy-promise dance in `paste_tx` exists
    // to sequence a paste; an address on the clipboard needs none of it.)
    unsafe { pb.declareTypes_owner(&types, None) };
    let ok = pb.setString_forType(&NSString::from_str(SUPPORT_EMAIL), string_type);
    if !ok {
        log::warn!("support bundle: address not copied to clipboard");
    }
}

#[cfg(not(target_os = "macos"))]
pub fn copy_address_to_clipboard() {}

/// The fallback, all of it: reveal + clipboard + the sentence.
pub fn fallback_outcome(path: &Path) -> SendOutcome {
    reveal_in_finder(path);
    copy_address_to_clipboard();
    SendOutcome {
        method: "reveal".into(),
        path: path.to_string_lossy().into_owned(),
        recipient: SUPPORT_EMAIL.into(),
        message: format!(
            "Yap could not open your mail app. The file is on your Desktop \
             (selected in Finder) and {SUPPORT_EMAIL} is on your clipboard — \
             attach the file to an email to that address."
        ),
    }
}

pub fn compose_outcome(path: &Path) -> SendOutcome {
    SendOutcome {
        method: "compose".into(),
        path: path.to_string_lossy().into_owned(),
        recipient: SUPPORT_EMAIL.into(),
        message: format!(
            "A message to {SUPPORT_EMAIL} is open with the file attached. \
             Nothing has been sent — read it, add what you were doing, and press send."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-12T14:15:30Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn header_is_kept_and_body_is_scrubbed() {
        let line = "[2026-08-09T17:53:02Z INFO wilson_voice_lib::logging] file logging → /Users/wilsonguenther/Library/Application Support/WilsonVoice/logs/yap.log";
        let out = redact_line(line, "wilsonguenther");
        assert!(out.starts_with("[2026-08-09T17:53:02Z INFO wilson_voice_lib::logging]"));
        assert!(!out.contains("wilsonguenther"));
        assert!(out.contains(R_PATH));
    }

    #[test]
    fn panic_payload_never_survives_but_the_location_does() {
        let line = "[2026-08-09T17:53:02Z ERROR wilson_voice_lib::logging] PANIC at src/dictation.rs:120:5: transcript was the quick brown fox";
        let out = redact_line(line, "wilsonguenther");
        assert!(out.contains("src/dictation.rs:120:5"));
        assert!(!out.contains("quick brown fox"));
    }

    #[test]
    fn short_operational_lines_are_left_readable() {
        for line in [
            "[2026-08-09T17:53:02Z INFO wilson_voice_lib::polish] polish sidecar starting",
            "[2026-08-09T17:53:02Z WARN wilson_voice_lib::db] wal_checkpoint failed: database is locked",
        ] {
            let out = redact_line(line, "wilsonguenther");
            assert!(!out.contains(R_PROSE), "over-redacted: {out}");
        }
    }

    #[test]
    fn bundle_file_name_carries_version_and_stamp() {
        assert_eq!(
            bundle_file_name("0.8.0", at()),
            "Yap-Diagnostics-0.8.0-20260812-141530.zip"
        );
    }

    #[test]
    fn environment_block_is_the_info_plist_version_and_os() {
        let block = environment_block("macOS 26.5.2 (25F84)", "aarch64");
        assert!(block.contains("CFBundleIdentifier: com.wilsonguenther.wilson-voice"));
        assert!(block.contains("CFBundleShortVersionString: "));
        assert!(block.contains("LSMinimumSystemVersion: "));
        assert!(block.contains("os: macOS 26.5.2 (25F84)"));
    }

    #[test]
    fn crc32_matches_the_known_vector() {
        // The canonical CRC-32/ISO-HDLC check value.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn tail_keeps_the_end_and_starts_on_a_line_boundary() {
        let text = "one\ntwo\nthree\nfour\n";
        let tail = tail_bytes(text, 10);
        assert!(tail.ends_with("four\n"));
        assert!(!tail.starts_with("hree"));
    }
}

#[cfg(test)]
mod edge_tests {
    use super::*;

    /// Redaction must never panic and never lose the line entirely, whatever a
    /// failing app wrote into the log.
    #[test]
    fn redaction_is_total_on_hostile_input() {
        for line in [
            "",
            " ",
            "[",
            "[]",
            "[unclosed",
            "\"",
            "\"unterminated quote",
            "PANIC at ",
            "PANIC at unknown",
            "[x] PANIC at unknown: boom",
            "////",
            "~/",
            "‹already redacted›",
            "…………",
            "🙂 🙂 🙂",
        ] {
            let out = redact_line(line, "wilsonguenther");
            assert!(out.len() < 4000, "runaway output for {line:?}");
        }
        // A username that is empty must not turn every token into ‹user›. (The
        // words themselves do not survive — `[x] hello world` is not a header
        // and not a message Yap compiled in — but they must be redacted as
        // prose, by the rule that owns them, and not misfiled as the account
        // name.)
        let out = redact_line("[x] hello world", "");
        assert!(!out.contains(R_USER), "{out}");
        assert!(out.contains(R_PROSE), "{out}");
    }

    /// A log whose header is missing (a raw backtrace line, say) is still
    /// redacted rather than passed through.
    #[test]
    fn headerless_lines_are_redacted_too() {
        let out = redact_line(
            "   at /Users/wilsonguenther/Library/Application Support/WilsonVoice/x.dylib",
            "wilsonguenther",
        );
        assert!(!out.contains("wilsonguenther"), "{out}");
        assert!(out.contains(R_PATH), "{out}");
    }

    /// The crash-report button's own panic, reproduced. `MAX_LOG_BYTES` is a
    /// BYTE count and `text.len() - max` is a byte offset; `→` and `—` are
    /// three bytes each and Yap logs them on the per-dictation hot path (PTT
    /// hold armed/released, reliable-paste read confirmed). Logs rotate at
    /// ~2 MB, so any real install sails past 512 KB and lands the offset inside
    /// a character — `byte index 298 is not a char boundary` — inside the one
    /// feature a user reaches when the app is already failing them.
    #[test]
    fn a_log_of_multibyte_characters_does_not_panic_the_tail() {
        let text = "\u{2192}".repeat(MAX_LOG_BYTES / 3 + 100);
        assert!(text.len() > MAX_LOG_BYTES);
        // The offset the reviewer's repro landed on: 298 bytes in, one byte
        // inside a character.
        assert!(!text.is_char_boundary(text.len() - MAX_LOG_BYTES));

        let tail = tail_bytes(&text, MAX_LOG_BYTES);
        assert!(tail.len() <= MAX_LOG_BYTES, "tail is longer than the cap");
        assert!(
            tail.chars().all(|c| c == '\u{2192}'),
            "the tail lost the shape of the log"
        );

        // And through the real door — `build_entries` is where it fired.
        let stamp = DateTime::parse_from_rfc3339("2026-08-12T14:15:30Z")
            .unwrap()
            .with_timezone(&Utc);
        let entries = build_entries(&BundleInputs {
            logs: vec![("yap.log".into(), text)],
            username: "wilsonguenther".into(),
            generated_at: stamp,
            ..Default::default()
        });
        assert!(entries.iter().any(|e| e.name == "logs/yap.log"));
    }

    /// The same offset, one character short of the cap, so the walk forward has
    /// to happen at every misalignment rather than only the one above.
    #[test]
    fn the_tail_starts_on_a_boundary_at_every_offset() {
        for extra in 0..6usize {
            let text = "\u{2014}".repeat(40 + extra);
            let tail = tail_bytes(&text, 30);
            assert!(tail.chars().all(|c| c == '\u{2014}'), "{tail:?}");
        }
    }

    /// The zip's own guarantee: two entries with the same bytes still produce
    /// distinct members, and an empty entry does not corrupt the directory.
    #[test]
    fn zip_handles_duplicate_content_and_empty_members() {
        let dir = std::env::temp_dir().join(format!("yv98-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.zip");
        let entries = vec![
            BundleEntry {
                name: "a.txt".into(),
                text: "same".into(),
            },
            BundleEntry {
                name: "b.txt".into(),
                text: "same".into(),
            },
            BundleEntry {
                name: "logs/empty.txt".into(),
                text: String::new(),
            },
        ];
        let stamp = DateTime::parse_from_rfc3339("2026-08-12T14:15:30Z")
            .unwrap()
            .with_timezone(&Utc);
        let bytes = write_zip(&path, &entries, stamp).expect("write");
        assert!(bytes > 0);
        let out = std::process::Command::new("/usr/bin/unzip")
            .arg("-t")
            .arg(&path)
            .output()
            .expect("unzip");
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("No errors detected"),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
