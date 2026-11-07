//! yap-1106-friend-visits — the pill's quiet signal.
//!
//! A friend never visits the pill during Do Not Disturb (a macOS Focus) or a
//! screen share. The pill (`pill/FriendVisit.tsx`) listens for
//! [`PILL_QUIET_EVENT`] and asks [`current`] once on mount through the
//! `pill_quiet_state` command; the rule itself lives in
//! `pill/friendVisits.ts` (`quietReason`).
//!
//! What is read, and what is not:
//! - Focus / Do Not Disturb: macOS keeps the manually switched-on Focus in
//!   `~/Library/DoNotDisturb/DB/Assertions.json` (a non-empty
//!   `storeAssertionRecords` list means a Focus is on). A Focus that only runs
//!   on a schedule is not in that file and is not seen. The file can be
//!   unreadable (privacy protection); that reads as "not quiet" and is logged
//!   once per change of readability, never the file's contents.
//! - Screen share: macOS has no public "is the screen being shared" signal
//!   without Screen Recording permission, which Yap never asks for. Shares
//!   almost always happen inside a call, and a live call already keeps every
//!   friend away (`callLive` in friendVisits.ts), so `screen_sharing` stays
//!   false here until a real signal exists.
//!
//! The watch polls every [`POLL_SECS`] seconds (the 30 s floor) and emits only
//! when the state changes.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// The event the pill listens for (`PILL_QUIET_EVENT` in friendVisits.ts).
pub const PILL_QUIET_EVENT: &str = "pill_quiet";
/// Poll interval for the Focus file.
pub const POLL_SECS: u64 = 30;

/// The payload `parseQuietState` (friendVisits.ts) reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuietState {
    pub screen_sharing: bool,
    pub do_not_disturb: bool,
}

static WATCH_ON: AtomicBool = AtomicBool::new(false);
static LAST: Mutex<QuietState> = Mutex::new(QuietState {
    screen_sharing: false,
    do_not_disturb: false,
});

/// Is a Focus switched on, from the text of `Assertions.json`? `None` when the
/// text is not the shape macOS writes (read as "not quiet" by the caller).
pub fn focus_on_from_assertions(json: &str) -> Option<bool> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let data = v.get("data")?.as_array()?;
    Some(data.iter().any(|d| {
        d.get("storeAssertionRecords")
            .and_then(|r| r.as_array())
            .is_some_and(|r| !r.is_empty())
    }))
}

/// Where macOS keeps the switched-on Focus.
fn assertions_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join("Library/DoNotDisturb/DB/Assertions.json"))
}

/// Read the Focus state from `path`. `Err` carries a reason word for the log.
pub fn read_focus(path: &Path) -> Result<bool, &'static str> {
    let text = std::fs::read_to_string(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "missing",
        std::io::ErrorKind::PermissionDenied => "denied",
        _ => "unreadable",
    })?;
    focus_on_from_assertions(&text).ok_or("unparsed")
}

/// One sample of the quiet state, plus the reason the Focus read failed.
fn sample() -> (QuietState, Option<&'static str>) {
    let focus = match assertions_path() {
        Some(p) => read_focus(&p),
        None => Err("no_home"),
    };
    let state = QuietState {
        screen_sharing: false,
        do_not_disturb: focus.unwrap_or(false),
    };
    (state, focus.err())
}

/// The last quiet state the watch saw (all false before the first sample).
pub fn current() -> QuietState {
    LAST.lock().map(|g| *g).unwrap_or_default()
}

/// Start the 30 s watch (idempotent). Emits [`PILL_QUIET_EVENT`] on a change.
pub fn start_watch(app: AppHandle) {
    if WATCH_ON.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = thread::Builder::new()
        .name("wv-pill-quiet".into())
        .spawn(move || {
            let mut last_reason: Option<&'static str> = None;
            loop {
                let (state, reason) = sample();
                if reason != last_reason {
                    match reason {
                        Some(r) => {
                            log::info!("pill friend_visit quiet focus_read=failed reason={r}")
                        }
                        None => log::info!("pill friend_visit quiet focus_read=ok"),
                    }
                    last_reason = reason;
                }
                let changed = match LAST.lock() {
                    Ok(mut g) => {
                        let changed = *g != state;
                        *g = state;
                        changed
                    }
                    Err(_) => false,
                };
                if changed {
                    log::info!(
                        "pill friend_visit quiet screen_sharing={} dnd={}",
                        state.screen_sharing,
                        state.do_not_disturb
                    );
                    if let Err(e) = app.emit(PILL_QUIET_EVENT, state) {
                        log::warn!("pill friend_visit quiet emit_failed reason={e}");
                    }
                }
                thread::sleep(Duration::from_secs(POLL_SECS));
            }
        });
    if let Err(e) = spawned {
        WATCH_ON.store(false, Ordering::SeqCst);
        log::warn!("pill friend_visit quiet watch_spawn_failed reason={e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pill_quiet_focus_on_when_an_assertion_record_exists() {
        let on = r#"{"data":[{"storeAssertionRecords":[{"assertionDetails":{"assertionDetailsModeIdentifier":"com.apple.donotdisturb.mode.default"}}]}],"header":{"timestamp":1}}"#;
        assert_eq!(focus_on_from_assertions(on), Some(true));
    }

    #[test]
    fn pill_quiet_focus_off_when_no_records() {
        assert_eq!(focus_on_from_assertions(r#"{"data":[{}]}"#), Some(false));
        assert_eq!(
            focus_on_from_assertions(r#"{"data":[{"storeAssertionRecords":[]}]}"#),
            Some(false)
        );
        assert_eq!(focus_on_from_assertions(r#"{"data":[]}"#), Some(false));
    }

    #[test]
    fn pill_quiet_unknown_shape_is_none() {
        assert_eq!(focus_on_from_assertions("not json"), None);
        assert_eq!(focus_on_from_assertions(r#"{"header":{}}"#), None);
    }

    #[test]
    fn pill_quiet_missing_file_reads_as_missing() {
        let p = std::env::temp_dir().join("yap-pill-quiet-does-not-exist-7f3a.json");
        assert_eq!(read_focus(&p), Err("missing"));
    }

    #[test]
    fn pill_quiet_payload_matches_the_pill_parser() {
        let s = QuietState {
            screen_sharing: false,
            do_not_disturb: true,
        };
        let v = serde_json::to_value(s).unwrap();
        assert_eq!(v["doNotDisturb"], serde_json::Value::Bool(true));
        assert_eq!(v["screenSharing"], serde_json::Value::Bool(false));
        assert_eq!(PILL_QUIET_EVENT, "pill_quiet");
    }
}
