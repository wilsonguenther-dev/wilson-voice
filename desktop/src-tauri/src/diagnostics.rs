//! yap-telemetry-diagnostics — the ONE in-app diagnostics surface.
//!
//! A fixed-size ring of structured events, fed two ways and recorded exactly
//! once per occurrence:
//!
//! - every `log` record at INFO and above, by the tee in `logging.rs`
//!   ([`record_from_log`]) — level, target as module, message, no fields;
//! - the [`diag_error!`] / [`diag_warn!`] / [`diag_info!`] macros, which record
//!   the event WITH its `key = value` fields and then emit the same line through
//!   `log` so `yap.log` keeps it. The tee skips that line (a thread-local flag
//!   set around the `log` call), so it lands in the ring once, not twice.
//!
//! Beside the ring: [`record_duration`] / [`bump`] stats (last value, count,
//! max) and a few named states (last insert path/outcome, meeting stage) the
//! health line reads. Nothing here leaves the machine: the export is two local
//! files in ~/Downloads, redacted by `support.rs`'s redaction.

use std::cell::Cell;
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use chrono::{DateTime, TimeZone, Utc};
use serde::Serialize;

/// How many events the ring keeps; the oldest is evicted first.
pub const DIAG_RING_CAPACITY: usize = 500;
/// How many of the newest events a snapshot (and an export) carries.
pub const SNAPSHOT_EVENTS: usize = 200;
/// Field keys that could carry what a person said or typed. Dropped from every
/// export, whatever their value.
pub const SENSITIVE_FIELD_KEYS: [&str; 4] = ["text", "transcript", "content", "snippet"];

/// One thing that happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagEvent {
    pub ts_ms: i64,
    pub level: String,
    pub module: String,
    pub msg: String,
    pub fields: BTreeMap<String, String>,
}

/// Fixed-capacity FIFO of [`DiagEvent`]s.
#[derive(Debug)]
pub struct DiagRing {
    cap: usize,
    events: VecDeque<DiagEvent>,
}

impl DiagRing {
    pub fn new(cap: usize) -> Self {
        let cap = cap.max(1);
        Self {
            cap,
            events: VecDeque::with_capacity(cap),
        }
    }

    pub fn push(&mut self, event: DiagEvent) {
        while self.events.len() >= self.cap {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// The newest `n` events, NEWEST FIRST.
    pub fn newest(&self, n: usize) -> Vec<DiagEvent> {
        self.events.iter().rev().take(n).cloned().collect()
    }
}

/// A duration or counter: the last value, how many times, the largest seen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stat {
    pub last: u64,
    pub count: u64,
    pub max: u64,
}

struct Diag {
    ring: DiagRing,
    stats: BTreeMap<String, Stat>,
    states: BTreeMap<String, String>,
}

fn global() -> MutexGuard<'static, Diag> {
    static DIAG: OnceLock<Mutex<Diag>> = OnceLock::new();
    DIAG.get_or_init(|| {
        Mutex::new(Diag {
            ring: DiagRing::new(DIAG_RING_CAPACITY),
            stats: BTreeMap::new(),
            states: BTreeMap::new(),
        })
    })
    .lock()
    .unwrap_or_else(|p| p.into_inner())
}

thread_local! {
    /// Set while a diag macro hands its line to `log`, so the tee does not
    /// record the same occurrence a second time.
    static IN_DIAG_MACRO: Cell<bool> = const { Cell::new(false) };
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn push_event(event: DiagEvent) {
    global().ring.push(event);
}

/// The tee's entry: one `log` record → one event (INFO and above), unless a
/// diag macro is emitting it (it already recorded the event with its fields).
pub fn record_from_log(record: &log::Record<'_>) {
    if record.level() > log::Level::Info {
        return;
    }
    if IN_DIAG_MACRO.with(Cell::get) {
        return;
    }
    push_event(DiagEvent {
        ts_ms: now_ms(),
        level: record.level().to_string(),
        module: record.target().to_string(),
        msg: record.args().to_string(),
        fields: BTreeMap::new(),
    });
}

/// `<module> <msg> k=v k=v` — the line a diag macro writes to `yap.log`.
pub fn format_line(module: &str, msg: &str, fields: &BTreeMap<String, String>) -> String {
    let mut line = format!("{module} {msg}");
    for (k, v) in fields {
        line.push(' ');
        line.push_str(k);
        line.push('=');
        line.push_str(v);
    }
    line
}

/// The `yap.log` line of a diag event: `msg` with every `{key}` hole filled
/// from the fields, and any field the message does not name appended as
/// ` key=value`. A converted call site keeps its old message as the template
/// (`"model download attempt failed ({url}): {err}"`), so its `yap.log` line
/// is byte-identical to before AND still a message this crate compiled in —
/// which is what lets `support.rs`'s vocabulary rule keep it in a bundle.
pub fn render_line(msg: &str, fields: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(msg.len() + 16);
    let mut used: Vec<&str> = Vec::new();
    let mut rest = msg;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let key = &after[..close];
                if let Some((k, v)) = fields.get_key_value(key) {
                    out.push_str(v);
                    used.push(k.as_str());
                } else {
                    out.push('{');
                    out.push_str(key);
                    out.push('}');
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push_str(&rest[open..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    for (k, v) in fields {
        if !used.contains(&k.as_str()) {
            out.push(' ');
            out.push_str(k);
            out.push('=');
            out.push_str(v);
        }
    }
    out
}

/// What the diag macros expand to. `target` is the caller's `module_path!()`
/// so `yap.log` keeps naming the file that logged. The ring keeps the module,
/// the rendered message and every field; `yap.log` gets [`render_line`].
pub fn emit(
    level: log::Level,
    target: &str,
    module: &str,
    msg: &str,
    fields: Vec<(&'static str, String)>,
) {
    let fields: BTreeMap<String, String> = fields
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    let line = render_line(msg, &fields);
    // The event's msg: holes filled, nothing appended — its fields carry the rest.
    let mut named = fields.clone();
    named.retain(|k, _| msg.contains(&format!("{{{k}}}")));
    let rendered = render_line(msg, &named);
    push_event(DiagEvent {
        ts_ms: now_ms(),
        level: level.to_string(),
        module: module.to_string(),
        msg: rendered,
        fields,
    });
    IN_DIAG_MACRO.with(|f| f.set(true));
    log::log!(target: target, level, "{line}");
    IN_DIAG_MACRO.with(|f| f.set(false));
}

/// `diag_error!("paste", "paste failed", err = e)` — records the event with its
/// fields and writes `paste paste failed err=…` to the log at ERROR.
macro_rules! diag_error {
    ($module:expr, $msg:expr $(, $key:ident = $val:expr)* $(,)?) => {
        $crate::diagnostics::emit(
            ::log::Level::Error,
            ::std::module_path!(),
            $module,
            &::std::string::ToString::to_string(&$msg),
            ::std::vec![$((::std::stringify!($key), ::std::format!("{}", $val))),*],
        )
    };
}

/// [`diag_error!`] at WARN.
macro_rules! diag_warn {
    ($module:expr, $msg:expr $(, $key:ident = $val:expr)* $(,)?) => {
        $crate::diagnostics::emit(
            ::log::Level::Warn,
            ::std::module_path!(),
            $module,
            &::std::string::ToString::to_string(&$msg),
            ::std::vec![$((::std::stringify!($key), ::std::format!("{}", $val))),*],
        )
    };
}

/// [`diag_error!`] at INFO.
macro_rules! diag_info {
    ($module:expr, $msg:expr $(, $key:ident = $val:expr)* $(,)?) => {
        $crate::diagnostics::emit(
            ::log::Level::Info,
            ::std::module_path!(),
            $module,
            &::std::string::ToString::to_string(&$msg),
            ::std::vec![$((::std::stringify!($key), ::std::format!("{}", $val))),*],
        )
    };
}

fn record_value(key: String, value: u64) {
    let mut g = global();
    let s = g.stats.entry(key).or_default();
    s.last = value;
    s.count += 1;
    s.max = s.max.max(value);
}

/// A duration in milliseconds, kept as `<module>.<name>_ms` and written as one
/// key=value INFO line (the `latency.rs` style).
pub fn record_duration(module: &str, name: &str, ms: u64) {
    record_value(format!("{module}.{name}_ms"), ms);
    log::info!("{module} duration {name}_ms={ms}");
}

/// A non-time quantity (bytes, items) kept as `<module>.<name>`.
pub fn record_amount(module: &str, name: &str, value: u64) {
    record_value(format!("{module}.{name}"), value);
    log::info!("{module} amount {name}={value}");
}

/// One more of `counter`, kept as `<module>.<counter>` (count is the tally).
pub fn bump(module: &str, counter: &str) {
    record_value(format!("{module}.{counter}"), 1);
}

/// A named state the health line reads (`insert.path`, `meeting.stage` …).
pub fn set_state(key: &str, value: impl Into<String>) {
    global().states.insert(key.to_string(), value.into());
}

pub fn state(key: &str) -> Option<String> {
    global().states.get(key).cloned()
}

/// Meeting pipeline stage boundary: the stage name becomes `meeting.stage`.
pub fn meeting_stage(stage: &str) {
    set_state("meeting.stage", stage);
}

// ── health + snapshot ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelHealth {
    pub selected: String,
    pub loaded: bool,
    pub loaded_model: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyHealth {
    pub registered: bool,
    pub chord: String,
    pub fn_tap: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingHealth {
    /// idle | recording | processing | failed
    pub state: String,
    pub last_stage: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InsertHealth {
    pub last_path: String,
    pub last_outcome: String,
}

/// The single health line's inputs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    /// microphone / accessibility / input_monitoring / audio_capture /
    /// post_event → authorized | denied | unknown.
    pub permissions: BTreeMap<String, String>,
    pub model: ModelHealth,
    pub hotkey: HotkeyHealth,
    pub meeting: MeetingHealth,
    pub insert: InsertHealth,
}

/// Meeting state as the pipeline last left it (the caller overrides with
/// `recording` when a capture is live).
pub fn meeting_health() -> MeetingHealth {
    MeetingHealth {
        state: state("meeting.state").unwrap_or_else(|| "idle".into()),
        last_stage: state("meeting.stage").unwrap_or_else(|| "-".into()),
    }
}

pub fn insert_health() -> InsertHealth {
    InsertHealth {
        last_path: state("insert.path").unwrap_or_else(|| "-".into()),
        last_outcome: state("insert.outcome").unwrap_or_else(|| "-".into()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagSnapshot {
    pub generated_at_ms: i64,
    /// Newest first.
    pub events: Vec<DiagEvent>,
    pub counters: BTreeMap<String, Stat>,
    pub health: Health,
}

/// The live view: the newest [`SNAPSHOT_EVENTS`] events, every stat, the
/// health the caller assembled. Transcript-shaped fields never leave here.
pub fn snapshot(health: Health) -> DiagSnapshot {
    let (mut events, counters) = {
        let g = global();
        (g.ring.newest(SNAPSHOT_EVENTS), g.stats.clone())
    };
    for e in &mut events {
        e.fields.retain(|k, _| !is_sensitive_key(k));
    }
    DiagSnapshot {
        generated_at_ms: now_ms(),
        events,
        counters,
        health,
    }
}

pub fn is_sensitive_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    SENSITIVE_FIELD_KEYS.contains(&k.as_str())
}

/// One line: `mic=authorized ax=… · model=… loaded=yes · hotkey=… …`.
pub fn health_line(h: &Health) -> String {
    let perm = |k: &str| h.permissions.get(k).cloned().unwrap_or_else(|| "?".into());
    let yn = |b: bool| if b { "yes" } else { "no" };
    format!(
        "mic={} ax={} input={} post_event={} · model={} loaded={} · hotkey={} registered={} · meeting={} stage={} · insert={} outcome={}",
        perm("microphone"),
        perm("accessibility"),
        perm("input_monitoring"),
        perm("post_event"),
        h.model.selected,
        yn(h.model.loaded),
        h.hotkey.chord,
        yn(h.hotkey.registered),
        h.meeting.state,
        h.meeting.last_stage,
        h.insert.last_path,
        h.insert.last_outcome,
    )
}

// ── export ──────────────────────────────────────────────────────────────────

/// A path's last component — what a log line may say about a file.
pub fn basename<P: AsRef<Path>>(p: P) -> String {
    p.as_ref()
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// An absolute (or `~/`) path → its basename. Anything else is returned as is.
pub fn path_to_basename(s: &str) -> String {
    let t = s.trim();
    if t.starts_with('/') || t.starts_with("~/") {
        return Path::new(t)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
    s.to_string()
}

/// Every whitespace token that is an absolute path → its basename, then the
/// support bundle's redaction (username scrub and the rest).
pub fn redact_value(s: &str, username: &str) -> String {
    let whole = path_to_basename(s);
    let tokens: Vec<String> = whole
        .split(' ')
        .map(|tok| {
            let core = tok.trim_matches(|c: char| matches!(c, '(' | ')' | ',' | ';' | '"' | '\''));
            if core.starts_with('/') || core.starts_with("~/") {
                tok.replace(core, &path_to_basename(core))
            } else {
                tok.to_string()
            }
        })
        .collect();
    crate::support::redact(&tokens.join(" "), username)
        .trim_end_matches('\n')
        .to_string()
}

fn scrub_username(s: &str, username: &str) -> String {
    if username.trim().is_empty() {
        return s.to_string();
    }
    let lower = s.to_lowercase();
    let user = username.to_lowercase();
    if !lower.contains(&user) {
        return s.to_string();
    }
    crate::support::redact(s, username)
        .trim_end_matches('\n')
        .to_string()
}

/// The snapshot as it may leave the app: sensitive fields dropped, paths cut to
/// their basename, every event string run through `support::redact`.
pub fn redact_snapshot(snap: &DiagSnapshot, username: &str) -> DiagSnapshot {
    let events = snap
        .events
        .iter()
        .map(|e| DiagEvent {
            ts_ms: e.ts_ms,
            level: e.level.clone(),
            module: redact_value(&e.module, username),
            msg: redact_value(&e.msg, username),
            fields: e
                .fields
                .iter()
                .filter(|(k, _)| !is_sensitive_key(k))
                .map(|(k, v)| (k.clone(), redact_value(v, username)))
                .collect(),
        })
        .collect();
    let mut health = snap.health.clone();
    health.model.selected = scrub_username(&path_to_basename(&health.model.selected), username);
    health.model.loaded_model = health
        .model
        .loaded_model
        .as_deref()
        .map(|m| scrub_username(&path_to_basename(m), username));
    health.hotkey.chord = scrub_username(&health.hotkey.chord, username);
    health.insert.last_path = scrub_username(&health.insert.last_path, username);
    DiagSnapshot {
        generated_at_ms: snap.generated_at_ms,
        events,
        counters: snap.counters.clone(),
        health,
    }
}

fn ts_label(ms: i64) -> String {
    Utc.timestamp_millis_opt(ms)
        .single()
        .map(|t| t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .unwrap_or_else(|| ms.to_string())
}

/// The human-readable export: health line, counters, events newest first.
pub fn render_text(snap: &DiagSnapshot) -> String {
    let mut out = format!(
        "Yap diagnostics — generated {}\n\nHEALTH\n{}\n\nPERMISSIONS\n",
        ts_label(snap.generated_at_ms),
        health_line(&snap.health)
    );
    for (k, v) in &snap.health.permissions {
        out.push_str(&format!("  {k}: {v}\n"));
    }
    out.push_str("\nCOUNTERS\n");
    if snap.counters.is_empty() {
        out.push_str("  (none yet)\n");
    }
    for (k, s) in &snap.counters {
        out.push_str(&format!(
            "  {k}  last={} count={} max={}\n",
            s.last, s.count, s.max
        ));
    }
    out.push_str(&format!("\nEVENTS (newest first, {})\n", snap.events.len()));
    for e in &snap.events {
        out.push_str(&format!(
            "  {} {:<5} {}\n",
            ts_label(e.ts_ms),
            e.level,
            format_line(&e.module, &e.msg, &e.fields)
        ));
    }
    out
}

/// `yap-diagnostics-20261027T191740Z` — both files share it.
pub fn export_stem(at: DateTime<Utc>) -> String {
    format!("yap-diagnostics-{}", at.format("%Y%m%dT%H%M%SZ"))
}

/// The two export paths, written as a pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPaths {
    pub json: String,
    pub txt: String,
}

/// Write the REDACTED snapshot to `<dir>/yap-diagnostics-<stamp>.json` and
/// `.txt`. Returns both paths.
pub fn write_export(
    dir: &Path,
    snap: &DiagSnapshot,
    username: &str,
    at: DateTime<Utc>,
) -> Result<(PathBuf, PathBuf), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not create the export folder: {e}"))?;
    let redacted = redact_snapshot(snap, username);
    let stem = export_stem(at);
    let json_path = dir.join(format!("{stem}.json"));
    let txt_path = dir.join(format!("{stem}.txt"));
    let json = serde_json::to_string_pretty(&redacted)
        .map_err(|e| format!("Could not encode diagnostics: {e}"))?;
    std::fs::write(&json_path, json)
        .map_err(|e| format!("Could not write the diagnostics file: {e}"))?;
    std::fs::write(&txt_path, render_text(&redacted))
        .map_err(|e| format!("Could not write the diagnostics file: {e}"))?;
    Ok((json_path, txt_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(msg: &str) -> DiagEvent {
        DiagEvent {
            ts_ms: 1,
            level: "INFO".into(),
            module: "test".into(),
            msg: msg.into(),
            fields: BTreeMap::new(),
        }
    }

    #[test]
    fn ring_buffer_evicts_oldest_at_capacity() {
        let mut ring = DiagRing::new(3);
        for i in 0..5 {
            ring.push(ev(&format!("e{i}")));
        }
        assert_eq!(ring.len(), 3);
        let msgs: Vec<String> = ring.newest(10).into_iter().map(|e| e.msg).collect();
        assert_eq!(msgs, vec!["e4", "e3", "e2"]);
        // The global ring is built at the contract's capacity.
        assert_eq!(DIAG_RING_CAPACITY, 500);
        let mut big = DiagRing::new(DIAG_RING_CAPACITY);
        for i in 0..(DIAG_RING_CAPACITY + 7) {
            big.push(ev(&format!("b{i}")));
        }
        assert_eq!(big.len(), DIAG_RING_CAPACITY);
        assert_eq!(big.newest(DIAG_RING_CAPACITY).last().unwrap().msg, "b7");
    }

    #[test]
    fn ring_buffer_keeps_order() {
        let mut ring = DiagRing::new(10);
        for i in 0..4 {
            ring.push(ev(&format!("o{i}")));
        }
        let msgs: Vec<String> = ring.newest(10).into_iter().map(|e| e.msg).collect();
        assert_eq!(msgs, vec!["o3", "o2", "o1", "o0"]);
        let two: Vec<String> = ring.newest(2).into_iter().map(|e| e.msg).collect();
        assert_eq!(two, vec!["o3", "o2"]);
    }

    fn count_global(pred: impl Fn(&DiagEvent) -> bool) -> usize {
        global().ring.events.iter().filter(|e| pred(e)).count()
    }

    #[test]
    fn macro_records_once_not_twice() {
        crate::logging::install_for_tests();
        // The tee is live: a plain log line is recorded exactly once.
        let plain = format!("diag-plain-{}", std::process::id());
        log::warn!("{plain}");
        assert_eq!(
            count_global(|e| e.msg == plain),
            1,
            "the tee did not record a plain log line"
        );

        let marker = format!("diag-macro-{}", std::process::id());
        diag_warn!("diagtest", marker.clone(), attempt = 2, reason = "timeout");
        let hits = count_global(|e| e.msg == marker || e.msg.contains(&marker));
        assert_eq!(hits, 1, "one occurrence must be one event");
        let g = global();
        let e = g
            .ring
            .events
            .iter()
            .rev()
            .find(|e| e.msg == marker)
            .unwrap();
        assert_eq!(e.module, "diagtest");
        assert_eq!(e.level, "WARN");
        assert_eq!(e.fields.get("attempt").map(String::as_str), Some("2"));
        assert_eq!(e.fields.get("reason").map(String::as_str), Some("timeout"));
    }

    fn snap_with(fields: &[(&str, &str)], msg: &str) -> DiagSnapshot {
        DiagSnapshot {
            generated_at_ms: 0,
            events: vec![DiagEvent {
                ts_ms: 0,
                level: "WARN".into(),
                module: "paste".into(),
                msg: msg.into(),
                fields: fields
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            }],
            counters: BTreeMap::new(),
            health: Health::default(),
        }
    }

    fn export_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("yap-diag-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn export_redacts_username_and_transcript_fields() {
        let snap = snap_with(
            &[
                ("text", "my secret dictation"),
                ("transcript", "another secret"),
                ("content", "c"),
                ("snippet", "s"),
                ("owner", "zorbuser"),
                ("attempt", "2"),
            ],
            "paste failed",
        );
        let dir = export_dir("redact");
        let (json, txt) = write_export(&dir, &snap, "zorbuser", Utc::now()).unwrap();
        for p in [&json, &txt] {
            let body = std::fs::read_to_string(p).unwrap();
            assert!(
                !body.contains("zorbuser"),
                "username leaked into {}",
                p.display()
            );
            assert!(
                !body.contains("secret"),
                "transcript field leaked into {}",
                p.display()
            );
            assert!(
                !body.contains("\"text\""),
                "text key survived in {}",
                p.display()
            );
            assert!(
                !body.contains("snippet="),
                "snippet key survived in {}",
                p.display()
            );
        }
        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).unwrap();
        let fields = &parsed["events"][0]["fields"];
        assert!(fields.get("text").is_none() && fields.get("content").is_none());
        assert!(fields.get("attempt").is_some() && fields.get("owner").is_some());
        assert!(json
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("yap-diagnostics-"));
        assert!(txt.extension().unwrap() == "txt");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_reduces_paths_to_basename() {
        let snap = snap_with(
            &[(
                "path",
                "/Users/zorbuser/Library/Application Support/com.wilsonguenther.wilsonvoice/yap.log",
            )],
            "paste failed",
        );
        assert_eq!(
            path_to_basename("/Users/zorbuser/Library/Application Support/x/yap.log"),
            "yap.log"
        );
        assert_eq!(path_to_basename("not a path"), "not a path");
        let dir = export_dir("paths");
        let (json, txt) = write_export(&dir, &snap, "zorbuser", Utc::now()).unwrap();
        for p in [&json, &txt] {
            let body = std::fs::read_to_string(p).unwrap();
            assert!(
                !body.contains("/Users/"),
                "absolute path survived in {}",
                p.display()
            );
            assert!(
                !body.contains("Application Support"),
                "path segment survived in {}",
                p.display()
            );
            assert!(!body.contains("zorbuser"));
        }
        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).unwrap();
        assert_eq!(parsed["events"][0]["fields"]["path"], "yap.log");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn render_line_fills_named_holes_and_appends_the_rest() {
        let f: BTreeMap<String, String> =
            [("url", "https://h/x"), ("err", "timed out"), ("n", "3")]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
        assert_eq!(
            render_line("model download attempt failed ({url}): {err}", &f),
            "model download attempt failed (https://h/x): timed out n=3"
        );
        assert_eq!(
            render_line("no holes {missing}", &BTreeMap::new()),
            "no holes {missing}"
        );
        assert_eq!(
            render_line("open { brace", &BTreeMap::new()),
            "open { brace"
        );
    }

    #[test]
    fn stats_keep_last_count_max() {
        record_duration("diagtest", "op", 30);
        record_duration("diagtest", "op", 10);
        let s = global().stats.get("diagtest.op_ms").copied().unwrap();
        assert_eq!((s.last, s.max), (10, 30));
        assert!(s.count >= 2);
        let h = Health::default();
        assert!(health_line(&h).contains("loaded=no"));
    }
}
