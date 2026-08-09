//! YV64 — local crash telemetry.
//!
//! Yap has crashed on real machines and nothing in the app ever knew: the
//! evidence sat in `~/Library/Logs/DiagnosticReports/` (macOS' own `.ips`
//! reports) and in the panic lines [`crate::logging`]'s hook already writes to
//! `yap.log`, and neither was ever read back. This module is the reader. At
//! startup it turns both sources into `crash_events` rows the Diagnostics
//! screen can show, so "Yap died last session" is something the app can say.
//!
//! Three rules shape everything below.
//!
//! * **Nothing leaves the machine.** There is no client, no endpoint, no
//!   upload, no opt-in that would add one. Rows are read from disk, written to
//!   the local SQLite DB, and shown in the local UI — that is the whole loop.
//!   `no_network_no_transcript_in_crash_rows` asserts this file has no network
//!   surface at all.
//! * **No dictated text can enter a row.** A row is assembled ONLY from an
//!   allowlist of structured fields ([`DETAIL_FIELDS`]) — process, version, OS,
//!   exception, termination, faulting thread, report filename. Free-text
//!   regions of a report (thread names, image paths, `asi` strings) and the
//!   free-text half of a panic message are never copied, because a panic
//!   payload can embed arbitrary runtime values including a transcript. The
//!   full text stays in `yap.log`, which the user can read and export
//!   deliberately.
//! * **Never panic while reading a crash.** Every input here is a file written
//!   by something that was already failing. Parsing is total: unreadable,
//!   truncated, empty, or wholly non-JSON reports produce `None` or a
//!   degraded-but-honest row, never an unwrap.

use crate::db::{CrashEvent, Database};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// A Rust panic, read back from the panic hook's own log line.
pub const KIND_PANIC: &str = "panic";
/// A native crash — the process died on a signal and macOS wrote a report.
pub const KIND_NATIVE: &str = "native";
/// The system watchdog ended (or sampled) a wedged process: a hang report, or
/// the classic `0x8badf00d` watchdog exception code.
pub const KIND_WATCHDOG: &str = "watchdog";

/// The ONLY facts a crash row may carry. Anything not on this list — thread
/// names, loaded image paths, `asi` strings, the free-text half of a panic
/// message — is never read out of a report, so no dictated text can reach the
/// DB by construction (see the module docs).
pub const DETAIL_FIELDS: &[&str] = &[
    "process",
    "version",
    "os",
    "exception",
    "termination",
    "faulting thread",
    "report",
    "site",
    "log",
];

/// Hard cap on a stored `details` blob. Reports are ~80 KB; a row is a summary.
const MAX_DETAILS: usize = 800;
/// Hard cap on one allowlisted field, so a hostile/garbage report cannot make a
/// row that is one enormous string.
const MAX_FIELD: usize = 120;
/// Hard cap on a stored `signature`.
const MAX_SIGNATURE: usize = 120;
/// Never read more than this from one report — the header + the structured top
/// of the body are in the first few KB, and this bounds a corrupt/huge file.
const MAX_REPORT_BYTES: usize = 256 * 1024;
/// Never read more than this from one log file when hunting panic lines.
const MAX_LOG_BYTES: usize = 4 * 1024 * 1024;
/// How many rotated logs the panic scan walks (mirrors `logging::MAX_BACKUPS`).
const LOG_FILES: &[&str] = &[
    "yap.log",
    "yap.log.1",
    "yap.log.2",
    "yap.log.3",
    "yap.log.4",
];

/// The process names that are OURS: the binary, the bundle, and the YV61 polish
/// sidecar (which crashes as its own process but is still Yap dying).
const OUR_PROCESSES: &[&str] = &["wilson-voice", "yap", "yap-polish"];

/// The panic hook's marker (`logging::install_panic_hook`). Everything before
/// the location is ours; everything after the location is the payload, which we
/// deliberately do not store.
const PANIC_MARKER: &str = "PANIC at ";

/// macOS report types that mean "the watchdog ended a wedged process" rather
/// than "the process took a fatal signal": 142 is an app hang, 238 a spin.
const HANG_BUG_TYPES: &[&str] = &["142", "238"];

/// The two directories macOS writes user crash reports to — the live one and
/// the `Retired/` overflow it ages reports into. The SAME report can be in
/// both mid-rotation, which is why the scan dedupes by filename.
pub fn diagnostic_report_dirs(home: &Path) -> Vec<PathBuf> {
    let base = home.join("Library").join("Logs").join("DiagnosticReports");
    vec![base.join("Retired"), base]
}

/// Read every crash artifact on disk into `crash_events` and report how many
/// rows were NEW (i.e. how many crashes the user has not been told about yet).
///
/// Best-effort throughout: a directory that cannot be listed, a report that
/// will not parse, or a DB write that fails leaves the user exactly where they
/// were before this feature existed.
pub fn ingest(db: &Database, home: &Path, logs_dir: &Path) -> usize {
    let mut events = scan_diagnostic_reports(&diagnostic_report_dirs(home));
    events.extend(scan_panic_log(logs_dir));
    let mut fresh = 0;
    for event in events {
        match db.record_crash_event(&event) {
            // `false` is the normal case on every launch after the first: the
            // report is already a row, so it is not news.
            Ok(true) => fresh += 1,
            Ok(false) => {}
            Err(e) => log::warn!("crash telemetry: row not written ({e})"),
        }
    }
    fresh
}

/// Walk the report directories, newest-surviving-wins, and build one event per
/// report that belongs to Yap. Reports are deduped by FILENAME across the
/// directories: macOS moves a report from the live folder into `Retired/`, and
/// for a moment it exists in both.
pub fn scan_diagnostic_reports(dirs: &[PathBuf]) -> Vec<CrashEvent> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.to_ascii_lowercase().ends_with(".ips") || !looks_like_ours(name) {
                continue;
            }
            if !seen.insert(name.to_string()) {
                continue;
            }
            if let Some(event) = event_from_report(&path, name) {
                out.push(event);
            }
        }
    }
    out
}

/// Cheap filename prefilter so a folder full of OTHER apps' reports is never
/// opened. macOS names the file after the crashing process, so this is the same
/// test as [`is_our_process`] applied to the report's stem; the header is still
/// the authority once the file is open.
fn looks_like_ours(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    OUR_PROCESSES
        .iter()
        .any(|p| lower.starts_with(&format!("{p}-")))
}

/// Is this process name one of ours?
pub fn is_our_process(name: &str) -> bool {
    let trimmed = name.trim().to_ascii_lowercase();
    OUR_PROCESSES.iter().any(|p| trimmed == *p)
}

/// Build the row for one report file, or `None` when the file is unreadable or
/// provably belongs to another process.
fn event_from_report(path: &Path, file_name: &str) -> Option<CrashEvent> {
    let text = read_bounded(path, MAX_REPORT_BYTES)?;
    let fallback = file_modified_at(path);
    parse_report(&text, file_name, fallback)
}

/// The pure half of [`event_from_report`]: report text → row. Split out so the
/// parser is testable on fixtures and on garbage without touching a filesystem.
///
/// `fallback_at` is used when the header carries no usable timestamp (a
/// truncated report still deserves a row, dated by the file that holds it).
pub fn parse_report(text: &str, file_name: &str, fallback_at: DateTime<Utc>) -> Option<CrashEvent> {
    let header = parse_ips_header(text);
    // A header that names a DIFFERENT process is the one hard rejection: the
    // filename prefilter can be fooled (an app really can be called "Yap-2"),
    // the header cannot.
    if let Some(h) = header.as_ref() {
        let named = h.process();
        if !named.is_empty() && !is_our_process(named) {
            return None;
        }
    }
    let body = parse_ips_body(text);
    let bug_type = header
        .as_ref()
        .map(|h| h.bug_type.clone())
        .unwrap_or_default();
    let exception = field(&body, &["exception", "type"]);
    let signal = field(&body, &["exception", "signal"]);
    let codes = field(&body, &["exception", "codes"]);
    let termination = field(&body, &["termination", "indicator"]);
    let namespace = field(&body, &["termination", "namespace"]);
    let thread = field(&body, &["faultingThread"]);

    let kind = classify(&bug_type, &codes);
    let signature = signature_for(&exception, &signal, &termination, kind);
    let mut process = header
        .as_ref()
        .map(|h| h.process().to_string())
        .unwrap_or_default();
    if process.is_empty() {
        process = field(&body, &["procName"]);
    }

    let details = details_text(&[
        ("process", process),
        (
            "version",
            header
                .as_ref()
                .map(|h| h.app_version.clone())
                .unwrap_or_default(),
        ),
        (
            "os",
            header
                .as_ref()
                .map(|h| h.os_version.clone())
                .unwrap_or_default(),
        ),
        ("exception", join_pair(&exception, &signal)),
        ("termination", join_pair(&namespace, &termination)),
        ("faulting thread", thread),
        ("report", file_name.to_string()),
    ]);

    Some(CrashEvent {
        id: uuid::Uuid::new_v4().to_string(),
        occurred_at: header
            .as_ref()
            .and_then(|h| h.occurred_at)
            .unwrap_or(fallback_at),
        kind: kind.to_string(),
        signature,
        source_file: file_name.to_string(),
        details,
        acknowledged: false,
    })
}

/// The first line of an `.ips` is a one-line JSON header (app name, version,
/// OS, timestamp); the REST of the file is a second, much larger JSON document.
/// Anything here can be truncated or absent on a report written by a process
/// that died mid-write, so every field is optional and every parse is `.ok()`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IpsHeader {
    pub app_name: String,
    pub name: String,
    pub app_version: String,
    pub os_version: String,
    pub bug_type: String,
    pub occurred_at: Option<DateTime<Utc>>,
}

impl IpsHeader {
    /// The process this report is about — `app_name` when present, else `name`.
    pub fn process(&self) -> &str {
        if self.app_name.is_empty() {
            &self.name
        } else {
            &self.app_name
        }
    }
}

/// Parse the header line. Returns `None` for anything that is not a JSON
/// object — never panics, whatever the bytes are.
pub fn parse_ips_header(text: &str) -> Option<IpsHeader> {
    let line = text.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    let value: Value = serde_json::from_str(line).ok()?;
    if !value.is_object() {
        return None;
    }
    Some(IpsHeader {
        app_name: scalar(&value, "app_name"),
        name: scalar(&value, "name"),
        app_version: scalar(&value, "app_version"),
        os_version: scalar(&value, "os_version"),
        bug_type: scalar(&value, "bug_type"),
        occurred_at: parse_report_time(&scalar(&value, "timestamp")),
    })
}

/// Parse the body (everything after the header line) as JSON. `None` when it is
/// absent or truncated — a header-only row is still worth having.
fn parse_ips_body(text: &str) -> Option<Value> {
    let (_, rest) = text.split_once('\n')?;
    serde_json::from_str(rest).ok()
}

/// macOS header timestamps look like `2026-08-07 19:12:31.00 -0500`.
fn parse_report_time(raw: &str) -> Option<DateTime<Utc>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f %z")
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// A scalar JSON field as a string. Numbers and bools stringify; objects,
/// arrays and nulls read as empty, so a report that types a field differently
/// than expected degrades instead of failing.
fn scalar(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(s)) => clamp(s, MAX_FIELD),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// A scalar at a nested path, or "" — the allowlist's only accessor.
fn field(body: &Option<Value>, path: &[&str]) -> String {
    let Some(mut cursor) = body.as_ref() else {
        return String::new();
    };
    let Some((last, parents)) = path.split_last() else {
        return String::new();
    };
    for key in parents {
        match cursor.get(key) {
            Some(next) => cursor = next,
            None => return String::new(),
        }
    }
    scalar(cursor, last)
}

/// `EXC_CRASH (SIGABRT)` from its two halves, tolerating either being absent.
fn join_pair(head: &str, tail: &str) -> String {
    match (head.is_empty(), tail.is_empty()) {
        (true, true) => String::new(),
        (true, false) => tail.to_string(),
        (false, true) => head.to_string(),
        (false, false) => format!("{head} ({tail})"),
    }
}

/// Native crash vs. watchdog kill. A hang/spin report type, or the classic
/// `0x8badf00d` ("ate bad food") watchdog exception code, means the process was
/// wedged and the system ended it — a different bug from a fatal signal, so it
/// gets a different kind.
fn classify(bug_type: &str, exception_codes: &str) -> &'static str {
    if HANG_BUG_TYPES.contains(&bug_type.trim())
        || exception_codes.to_ascii_lowercase().contains("8badf00d")
    {
        KIND_WATCHDOG
    } else {
        KIND_NATIVE
    }
}

/// The short line the Stability list shows. Structured fields only.
fn signature_for(exception: &str, signal: &str, termination: &str, kind: &str) -> String {
    let from_exception = join_pair(exception, signal);
    let raw = if !from_exception.is_empty() {
        from_exception
    } else if !termination.is_empty() {
        termination.to_string()
    } else if kind == KIND_WATCHDOG {
        "hang (no exception recorded)".to_string()
    } else {
        "crash report (no exception recorded)".to_string()
    };
    clamp(&raw, MAX_SIGNATURE)
}

/// Render the allowlisted pairs as the stored `details` blob, skipping empties
/// and clamping the whole thing.
///
/// [`DETAIL_FIELDS`] is ENFORCED here, not merely documented: a pair whose key
/// is not on the list is dropped, so adding a field to a row is a deliberate
/// two-place edit rather than something that can slip in beside a caller.
fn details_text(pairs: &[(&str, String)]) -> String {
    let body = pairs
        .iter()
        .filter(|(k, v)| DETAIL_FIELDS.contains(k) && !v.trim().is_empty())
        .map(|(k, v)| format!("{k}: {}", clamp(v, MAX_FIELD)))
        .collect::<Vec<_>>()
        .join("\n");
    clamp(&body, MAX_DETAILS)
}

/// Truncate on a CHARACTER boundary (a report can hold any UTF-8) and mark it.
fn clamp(s: &str, max: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(max.saturating_sub(1)).collect();
    format!("{head}…")
}

/// Read at most `max` bytes of a file as lossy UTF-8. `None` when it cannot be
/// opened — a report we cannot read is simply not a row.
fn read_bounded(path: &Path, max: usize) -> Option<String> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    file.take(max as u64).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// The file's mtime as the last-resort timestamp for a report whose header
/// carries none. Falls back to "now" if even that is unavailable.
fn file_modified_at(path: &Path) -> DateTime<Utc> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(|_| Utc::now())
}

/// Correlate the panic-hook lines [`crate::logging`] writes into `panic` rows.
///
/// The hook's line is `PANIC at <file:line:col>: <payload>\nbacktrace:…`. Only
/// the timestamp and the LOCATION are stored: the payload is a formatted Rust
/// value and could hold anything the panicking code was holding, which on this
/// app's hot path can be a transcript. The payload stays in `yap.log`.
///
/// A line with no parseable timestamp is skipped rather than guessed at — the
/// timestamp is half of the row's identity, and a guessed one would re-insert
/// the same panic on every launch.
pub fn scan_panic_log(logs_dir: &Path) -> Vec<CrashEvent> {
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut out = Vec::new();
    for name in LOG_FILES {
        let path = logs_dir.join(name);
        let Some(text) = read_bounded(&path, MAX_LOG_BYTES) else {
            continue;
        };
        for line in text.lines() {
            let Some(event) = parse_panic_line(line, name) else {
                continue;
            };
            // Rotation copies a line from yap.log into yap.log.1, so the same
            // panic is visible under two filenames — identity is the instant
            // plus the site, never the file it happens to sit in today.
            if seen.insert((event.occurred_at.to_rfc3339(), event.source_file.clone())) {
                out.push(event);
            }
        }
    }
    out
}

/// One log line → a `panic` row, or `None` when the line is not a panic line
/// (the overwhelming majority) or carries no usable timestamp.
pub fn parse_panic_line(line: &str, log_file: &str) -> Option<CrashEvent> {
    let marker = line.find(PANIC_MARKER)?;
    let occurred_at = parse_log_time(line)?;
    let after = &line[marker + PANIC_MARKER.len()..];
    // The hook writes `<location>: <payload>`, where location is `file:line:col`
    // or the literal "unknown". Only a location-SHAPED head is taken — the two
    // trailing segments must be numbers — so a hook line with no location can
    // never let the payload through as one (see the fn docs).
    let parts: Vec<&str> = after.split(':').collect();
    let location = if parts.len() >= 3
        && parts[1].trim().parse::<u32>().is_ok()
        && parts[2].trim().parse::<u32>().is_ok()
    {
        format!(
            "{}:{}:{}",
            parts[0].trim(),
            parts[1].trim(),
            parts[2].trim()
        )
    } else {
        parts[0].to_string()
    };
    let site = clamp(&location, MAX_FIELD);
    if site.is_empty() {
        return None;
    }
    Some(CrashEvent {
        id: uuid::Uuid::new_v4().to_string(),
        occurred_at,
        kind: KIND_PANIC.to_string(),
        signature: clamp(&format!("panic at {site}"), MAX_SIGNATURE),
        source_file: site.clone(),
        details: details_text(&[("site", site), ("log", log_file.to_string())]),
        acknowledged: false,
    })
}

/// `env_logger`'s default line opens `[2026-08-09T17:53:02Z LEVEL module]`.
fn parse_log_time(line: &str) -> Option<DateTime<Utc>> {
    let rest = line.strip_prefix('[')?;
    let stamp = rest.split_whitespace().next()?;
    DateTime::parse_from_rfc3339(stamp)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// The crash summary that rides along with "Export diagnostics" — the same
/// facts the Stability list shows, as plain text next to `yap.log`. Written to
/// the logs folder the export reveals; it never goes anywhere else.
pub fn summary_text(events: &[CrashEvent]) -> String {
    let mut out = String::from("Yap — crash summary (local only, never uploaded)\n");
    out.push_str(&format!("generated: {}\n\n", Utc::now().to_rfc3339()));
    if events.is_empty() {
        out.push_str("No crashes recorded.\n");
        return out;
    }
    out.push_str(&format!(
        "{} recorded crash(es), newest first:\n",
        events.len()
    ));
    for e in events {
        out.push_str(&format!(
            "\n[{}] {} — {}\n{}\n",
            e.occurred_at.to_rfc3339(),
            e.kind,
            e.signature,
            e.details
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sanitized report committed alongside the parser, so the fixture is the
    /// real shape macOS writes (see the file's own header comment).
    const FIXTURE: &str = include_str!("../tests/fixtures/crash/wilson-voice-crash.ips");
    const FIXTURE_NAME: &str = "wilson-voice-2026-08-07-191231.ips";

    fn epoch() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2000-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }


    #[test]
    fn ips_header_parse_on_fixture() {
        let header = parse_ips_header(FIXTURE).expect("fixture header parses");
        assert_eq!(header.process(), "wilson-voice");
        assert_eq!(header.app_version, "0.6.0");
        assert_eq!(header.os_version, "macOS 26.5.2 (25F84)");
        assert_eq!(header.bug_type, "309");
        assert_eq!(
            header.occurred_at.map(|t| t.to_rfc3339()),
            // 19:12:31 -0500 is 00:12:31Z the next day.
            Some("2026-08-08T00:12:31+00:00".to_string())
        );

        let event = parse_report(FIXTURE, FIXTURE_NAME, epoch()).expect("fixture becomes a row");
        assert_eq!(event.kind, KIND_NATIVE);
        assert_eq!(event.signature, "EXC_CRASH (SIGABRT)");
        assert_eq!(event.source_file, FIXTURE_NAME);
        assert_eq!(event.occurred_at, header.occurred_at.unwrap());
        assert!(event.details.contains("process: wilson-voice"));
        assert!(event
            .details
            .contains("termination: SIGNAL (Abort trap: 6)"));
        assert!(event.details.contains(FIXTURE_NAME));
        assert!(!event.acknowledged);
        // Every line of details is an allowlisted field, nothing else.
        for line in event.details.lines() {
            let key = line.split(':').next().unwrap_or_default();
            assert!(
                DETAIL_FIELDS.contains(&key),
                "unexpected detail field: {key}"
            );
        }
    }

    #[test]
    fn ips_parse_never_panics_on_garbage() {
        let garbage = [
            "",
            "\n",
            "\n\n\n",
            "{",
            "}{",
            "null",
            "[]",
            "[1,2,3]",
            "\"just a string\"",
            "not json at all",
            "{\"app_name\": 12345}",
            "{\"app_name\": {\"nested\": true}}",
            "{\"app_name\":\"wilson-voice\"}\nnot json either",
            "{\"app_name\":\"wilson-voice\",\"timestamp\":\"nope\"}\n{}",
            "{\"app_name\":\"wilson-voice\",\"bug_type\":309}\n{\"exception\":[]}",
            "{\"app_name\":\"wilson-voice\"}\n{\"exception\":{\"type\":null}}",
            "\u{feff}{\"app_name\":\"wilson-voice\"}",
            "{\"app_name\":\"wilson-voice\"}\n{\"termination\":\"scalar not object\"}",
            // A truncated report: real header, body cut mid-write.
            &FIXTURE[..FIXTURE.len() / 2],
        ];
        for input in garbage {
            // The contract is total: a value or a None, never an unwind.
            let _ = parse_ips_header(input);
            let event = parse_report(input, "garbage.ips", epoch());
            if let Some(e) = event {
                assert!(!e.signature.is_empty(), "a row always names something");
                assert!(e.details.chars().count() <= MAX_DETAILS);
                assert!(e.signature.chars().count() <= MAX_SIGNATURE);
            }
        }
        // Panic-line parsing is total too.
        for line in [
            "",
            "[",
            "[not-a-time] PANIC at ",
            "PANIC at ",
            "[2026-08-09T17:53:02Z ERROR x] no marker",
        ] {
            let _ = parse_panic_line(line, "yap.log");
        }
    }

    #[test]
    fn crash_scan_dedupes_by_filename() {
        let base = std::env::temp_dir().join(format!("wv-crash-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let reports = base.join("Library").join("Logs").join("DiagnosticReports");
        let retired = reports.join("Retired");
        std::fs::create_dir_all(&retired).unwrap();

        // The same report macOS is mid-way through retiring: present in BOTH.
        std::fs::write(reports.join(FIXTURE_NAME), FIXTURE).unwrap();
        std::fs::write(retired.join(FIXTURE_NAME), FIXTURE).unwrap();
        // A second, genuinely different report of ours.
        std::fs::write(reports.join("yap-polish-2026-08-08-101010.ips"), FIXTURE).unwrap();
        // Another app's crash, which must never be read.
        std::fs::write(
            reports.join("Safari-2026-08-08-101010.ips"),
            "{\"app_name\":\"Safari\",\"timestamp\":\"2026-08-08 10:10:10.00 -0500\"}\n{}",
        )
        .unwrap();
        // A report NAMED like ours whose header says otherwise (the filename
        // prefilter can be fooled; the header is the authority).
        std::fs::write(
            reports.join("yap-imposter-2026-08-08-101010.ips"),
            "{\"app_name\":\"yap-imposter\",\"timestamp\":\"2026-08-08 10:10:10.00 -0500\"}\n{}",
        )
        .unwrap();

        let found = scan_diagnostic_reports(&diagnostic_report_dirs(&base));
        let names: Vec<&str> = found.iter().map(|e| e.source_file.as_str()).collect();
        assert_eq!(found.len(), 2, "one row per FILENAME, both dirs: {names:?}");
        assert!(names.contains(&FIXTURE_NAME));
        assert!(names.contains(&"yap-polish-2026-08-08-101010.ips"));

        // And the DB agrees: ingesting twice (i.e. two launches) is idempotent.
        let db = Database::open(base.join("wilson_voice.db")).unwrap();
        let logs = base.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        assert_eq!(ingest(&db, &base, &logs), 2, "first launch sees both");
        assert_eq!(
            ingest(&db, &base, &logs),
            0,
            "second launch sees nothing new"
        );
        assert_eq!(db.list_crash_events(50).unwrap().len(), 2);

        drop(db);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn no_network_no_transcript_in_crash_rows() {
        // (a) No network surface in this module, at all. The needles are
        // assembled with concat! so this assertion cannot match itself.
        const SRC: &str = include_str!("crash.rs");
        for needle in [
            concat!("req", "west"),
            concat!("Tcp", "Stream"),
            concat!("Udp", "Socket"),
            concat!("ur", "eq"),
            concat!("http", "://"),
            concat!("https", "://"),
            concat!("to_socket", "_addrs"),
        ] {
            assert!(
                !SRC.contains(needle),
                "crash telemetry must have no network surface, found {needle}"
            );
        }

        // (b) A report whose free-text regions are full of dictated words. None
        // of those regions is on the allowlist, so none can reach the row.
        let sentinel = "the quick brown fox jumped over the lazy dog";
        let report = format!(
            "{{\"app_name\":\"wilson-voice\",\"timestamp\":\"2026-08-07 19:12:31.00 -0500\",\
              \"app_version\":\"0.6.0\",\"bug_type\":\"309\",\"os_version\":\"macOS 26.5.2\"}}\n\
             {{\"procName\":\"wilson-voice\",\
               \"procPath\":\"/Users/{sentinel}/Yap.app\",\
               \"coalitionName\":\"{sentinel}\",\
               \"asi\":{{\"libsystem_c.dylib\":[\"{sentinel}\"]}},\
               \"threads\":[{{\"name\":\"{sentinel}\"}}],\
               \"exception\":{{\"type\":\"EXC_CRASH\",\"signal\":\"SIGABRT\"}},\
               \"termination\":{{\"namespace\":\"SIGNAL\",\"indicator\":\"Abort trap: 6\"}}}}"
        );
        let event = parse_report(&report, "wilson-voice-2026-08-07-191231.ips", epoch())
            .expect("report becomes a row");
        assert!(
            !event.details.contains(sentinel),
            "details: {}",
            event.details
        );
        assert!(!event.signature.contains(sentinel));
        assert!(!event.source_file.contains(sentinel));

        // (c) A panic whose PAYLOAD is a transcript — the location is stored,
        // the payload is not (it stays in yap.log). Both hook shapes are
        // covered: a located panic, and one whose location is "unknown".
        for (line, site) in [
            (
                format!(
                    "[2026-08-09T17:53:02Z ERROR wilson_voice_lib::logging] \
                     PANIC at src/dictation.rs:120:5: transcript was {sentinel}"
                ),
                "src/dictation.rs:120:5",
            ),
            (
                format!(
                    "[2026-08-09T17:53:02Z ERROR wilson_voice_lib::logging] \
                     PANIC at unknown: transcript was {sentinel}"
                ),
                "unknown",
            ),
        ] {
            let row = parse_panic_line(&line, "yap.log").expect("panic line becomes a row");
            assert_eq!(row.kind, KIND_PANIC);
            assert_eq!(row.source_file, site);
            assert!(!row.details.contains(sentinel), "details: {}", row.details);
            assert!(!row.signature.contains(sentinel));
            // (d) And the summary, which is what leaves the DB for the export
            // file, is built from those same rows only.
            assert!(!summary_text(&[event.clone(), row]).contains(sentinel));
        }
    }

    #[test]
    fn panic_scan_survives_log_rotation_without_duplicating() {
        let dir = std::env::temp_dir().join(format!("wv-crash-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let line = "[2026-08-09T17:53:02Z ERROR wilson_voice_lib::logging] \
                    PANIC at src/record.rs:42:9: capture thread died";
        // The SAME panic, visible in the live log and in the rotated copy.
        std::fs::write(dir.join("yap.log"), format!("{line}\nbacktrace:\n  0: x\n")).unwrap();
        std::fs::write(
            dir.join("yap.log.1"),
            format!("[2026-08-09T17:00:00Z INFO x] hi\n{line}\n"),
        )
        .unwrap();

        let events = scan_panic_log(&dir);
        assert_eq!(events.len(), 1, "one panic, not one per log file");
        assert_eq!(events[0].kind, KIND_PANIC);
        assert_eq!(events[0].source_file, "src/record.rs:42:9");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
