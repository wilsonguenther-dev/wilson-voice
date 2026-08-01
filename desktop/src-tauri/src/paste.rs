//! Clipboard + frontmost-app paste.
//!
//! CRITICAL (crash 2026-07-17): enigo Key::Unicode on a background thread calls
//! HIToolbox TSMGetInputSourceProperty → dispatch_assert_queue_fail → SIGTRAP.
//! All synthetic keystrokes must run on the **main thread**, and we use raw
//! virtual keycodes (ANSI V + Command) so we never touch the input-source APIs.

use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::paste_tx;
use crate::permissions;

pub fn copy_text(app: &AppHandle, text: &str) -> Result<(), String> {
    app.clipboard()
        .write_text(text.to_string())
        .map_err(|e| format!("clipboard write: {e}"))
}

#[derive(Debug)]
pub struct PasteOutcome {
    pub copied: bool,
    pub pasted: bool,
    pub message: String,
}

/// YV21 (audit M1): decide whether it is safe to synthesize ⌘V.
///
/// The transcript is auto-pasted 1–3s after key-release, inside the ASR thread.
/// If the user switched apps during that delay, ⌘V would land in the WRONG app
/// (a privacy leak). `source_app` is the frontmost app captured at record start;
/// `current_app` is the frontmost app sampled immediately before ⌘V. We only
/// paste when we can positively confirm the SAME app is still frontmost. Any
/// uncertainty (either identity unknown/empty, or a genuinely different app) is
/// treated as a mismatch → clipboard-only, so we never paste into another app.
pub fn is_same_paste_target(source_app: Option<&str>, current_app: Option<&str>) -> bool {
    match (source_app, current_app) {
        (Some(a), Some(b)) => {
            let a = a.trim();
            let b = b.trim();
            !a.is_empty() && a.eq_ignore_ascii_case(b)
        }
        _ => false,
    }
}

/// Clipboard-only outcome: write the transcript as plain text and say why we
/// did not paste. Keeps the "never lose text" guarantee on every non-paste
/// path (auto-paste off, Accessibility missing, wrong target, paste failure).
fn copy_only(app: &AppHandle, text: &str, message: &str) -> PasteOutcome {
    match copy_text(app, text) {
        Ok(()) => PasteOutcome {
            copied: true,
            pasted: false,
            message: message.into(),
        },
        Err(e) => PasteOutcome {
            copied: false,
            pasted: false,
            message: format!("Clipboard failed: {e}"),
        },
    }
}

/// Always copy. Paste only when Accessibility is granted, **on the main thread**.
///
/// YV39: the paste itself is receipt-sequenced (`paste_tx`) — the transcript is
/// published as a lazy pasteboard promise, ⌘V is injected, and the user's prior
/// clipboard is restored only once the target app has demonstrably READ the
/// pasteboard (with a bounded timeout), instead of after a fixed sleep that
/// could restore before a busy target ever read it. On paste failure the
/// transcript is left on the clipboard so they can ⌘V manually, and the reason
/// is logged at ERROR (the audit found a `pasted=false` with no diagnostic).
///
/// `source_app` is the frontmost app captured at dictation time (record start).
/// When it is `Some`, the wrong-target guard (YV21) is enforced: we sample the
/// current frontmost app immediately before ⌘V and only paste if it still
/// matches. Pass `None` for explicit user re-pastes (e.g. `paste_entry`), where
/// the current frontmost app IS the intended target and no guard applies.
pub fn copy_and_maybe_paste(
    app: &AppHandle,
    text: &str,
    want_paste: bool,
    source_app: Option<&str>,
) -> PasteOutcome {
    if !want_paste {
        return copy_only(app, text, "Copied to clipboard (⌘V to paste)");
    }
    if !permissions::is_accessibility_trusted() {
        return copy_only(
            app,
            text,
            "Copied. Enable Accessibility for Yap to auto-paste.",
        );
    }

    // YV21 (audit M1): guard against a wrong-target paste. `source_app` was the
    // frontmost app at record start; sample the CURRENT frontmost app right
    // before synthesizing ⌘V. If the user switched apps during the ASR delay,
    // fall back to clipboard-only — the text is already copied, so nothing is
    // lost and it never lands in a different app.
    if let Some(dictated) = source_app {
        let current = crate::focus::frontmost_app_name();
        if !is_same_paste_target(Some(dictated), current.as_deref()) {
            return copy_only(app, text, "Copied — you switched apps, so I didn't paste");
        }
    }

    // YV39: publish → ⌘V → restore on the READ RECEIPT (see paste_tx). The
    // prior clipboard is snapshotted inside, on the main thread, right after
    // any still-open transaction is settled — so it is the user's clipboard,
    // never a previous transcript promise.
    match paste_tx::publish_and_paste(app, text) {
        Ok(()) => PasteOutcome {
            copied: true,
            pasted: true,
            message: "Pasted into frontmost app".into(),
        },
        Err(e) => {
            // The audit (yap-logs finding 3) found a pasted=false transcript
            // whose cause was unrecoverable from the log: the paste path only
            // recorded the boolean. Log the actual reason + the app we aimed at.
            log::error!(
                "paste failed: {e} (frontmost={})",
                crate::focus::frontmost_app_name().unwrap_or_else(|| "unknown".into())
            );
            // Leave the transcript on the clipboard so ⌘V still works by hand.
            copy_only(app, text, &format!("Copied, but paste failed ({e})."))
        }
    }
}

/// YV49 command mode: carry out "delete that" by pressing Delete on the
/// frontmost app, NOT by pasting an empty string.
///
/// Two reasons this is a keystroke instead of a paste: an empty pasteboard is a
/// no-op in several apps (so the selection would survive a "delete that"), and
/// a delete has no business overwriting whatever the user has on their
/// clipboard. It reuses the same two guards as the paste path — Accessibility
/// must be granted, and the app that was frontmost when the command started
/// must still be frontmost — because a stray Delete in the WRONG app destroys
/// text just as effectively as a wrong paste.
pub fn delete_selection(app: &AppHandle, source_app: Option<&str>) -> PasteOutcome {
    if !permissions::is_accessibility_trusted() {
        return PasteOutcome {
            copied: false,
            pasted: false,
            message: "Enable Accessibility for Yap to edit your selection.".into(),
        };
    }
    if let Some(started_in) = source_app {
        let current = crate::focus::frontmost_app_name();
        if !is_same_paste_target(Some(started_in), current.as_deref()) {
            return PasteOutcome {
                copied: false,
                pasted: false,
                message: "You switched apps, so I didn't delete anything".into(),
            };
        }
    }
    match post_delete_on_main(app) {
        Ok(()) => PasteOutcome {
            copied: false,
            pasted: true,
            message: "Deleted in frontmost app".into(),
        },
        Err(e) => {
            log::error!(
                "delete failed: {e} (frontmost={})",
                crate::focus::frontmost_app_name().unwrap_or_else(|| "unknown".into())
            );
            PasteOutcome {
                copied: false,
                pasted: false,
                message: format!("Couldn't delete ({e})."),
            }
        }
    }
}

/// Synthetic keystrokes are main-thread-only (see the SIGTRAP note at the top),
/// so the chord is scheduled there and its result handed back over a channel —
/// the same shape `paste_tx::publish_and_paste` uses for ⌘V.
#[cfg(target_os = "macos")]
fn post_delete_on_main(app: &AppHandle) -> Result<(), String> {
    use std::sync::mpsc;
    use std::time::Duration;

    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(cg::post_delete());
    })
    .map_err(|e| format!("schedule main-thread delete: {e}"))?;

    rx.recv_timeout(Duration::from_secs(3))
        .map_err(|_| "delete timed out waiting for main thread".to_string())?
}

#[cfg(not(target_os = "macos"))]
fn post_delete_on_main(_app: &AppHandle) -> Result<(), String> {
    Err("delete only implemented on macOS".into())
}

/// macOS virtual keycodes (ANSI layout — independent of current input source).
///
/// `pub(crate)` since YV39: `paste_tx` posts the chord on the main thread as
/// part of the receipt-sequenced transaction (publish → ⌘V → receipt →
/// restore), which replaced the old `dispatch_paste_on_main` + fixed sleeps.
#[cfg(target_os = "macos")]
pub(crate) mod cg {
    use std::ffi::c_void;

    pub type CGEventRef = *mut c_void;
    pub type CGEventSourceRef = *mut c_void;
    pub type CGKeyCode = u16;
    pub type CGEventFlags = u64;

    pub const CG_EVENT_FLAG_MASK_COMMAND: CGEventFlags = 0x100000;
    pub const CG_HID_EVENT_TAP: u32 = 0; // kCGHIDEventTap
    pub const K_VK_ANSI_V: CGKeyCode = 0x09;
    pub const K_VK_COMMAND: CGKeyCode = 0x37;
    /// kVK_Delete — the Delete/Backspace key. With a selection active every
    /// macOS text control deletes the selection (YV49 "delete that").
    pub const K_VK_DELETE: CGKeyCode = 0x33;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceCreate(state_id: i32) -> CGEventSourceRef;
        fn CGEventCreateKeyboardEvent(
            source: CGEventSourceRef,
            virtual_key: CGKeyCode,
            key_down: bool,
        ) -> CGEventRef;
        fn CGEventSetFlags(event: CGEventRef, flags: CGEventFlags);
        fn CGEventPost(tap: u32, event: CGEventRef);
        fn CFRelease(cf: *mut c_void);
    }

    const K_CG_EVENT_SOURCE_STATE_HID: i32 = 1;

    /// Post ⌘V using only virtual keycodes — never HIToolbox input-source APIs.
    pub(crate) fn post_cmd_v() -> Result<(), String> {
        unsafe {
            let source = CGEventSourceCreate(K_CG_EVENT_SOURCE_STATE_HID);
            if source.is_null() {
                return Err("CGEventSourceCreate failed".into());
            }

            // Cmd down
            let cmd_down = CGEventCreateKeyboardEvent(source, K_VK_COMMAND, true);
            if cmd_down.is_null() {
                CFRelease(source);
                return Err("cmd down event null".into());
            }
            CGEventSetFlags(cmd_down, CG_EVENT_FLAG_MASK_COMMAND);
            CGEventPost(CG_HID_EVENT_TAP, cmd_down);
            CFRelease(cmd_down);

            // V down with Cmd flag
            let v_down = CGEventCreateKeyboardEvent(source, K_VK_ANSI_V, true);
            if v_down.is_null() {
                CFRelease(source);
                return Err("v down event null".into());
            }
            CGEventSetFlags(v_down, CG_EVENT_FLAG_MASK_COMMAND);
            CGEventPost(CG_HID_EVENT_TAP, v_down);
            CFRelease(v_down);

            // V up
            let v_up = CGEventCreateKeyboardEvent(source, K_VK_ANSI_V, false);
            if !v_up.is_null() {
                CGEventSetFlags(v_up, CG_EVENT_FLAG_MASK_COMMAND);
                CGEventPost(CG_HID_EVENT_TAP, v_up);
                CFRelease(v_up);
            }

            // Cmd up
            let cmd_up = CGEventCreateKeyboardEvent(source, K_VK_COMMAND, false);
            if !cmd_up.is_null() {
                CGEventPost(CG_HID_EVENT_TAP, cmd_up);
                CFRelease(cmd_up);
            }

            CFRelease(source);
        }
        Ok(())
    }

    /// Post a bare Delete keypress (YV49 command mode). No modifier flags — the
    /// selection itself is what makes this a "delete the selection".
    pub(crate) fn post_delete() -> Result<(), String> {
        unsafe {
            let source = CGEventSourceCreate(K_CG_EVENT_SOURCE_STATE_HID);
            if source.is_null() {
                return Err("CGEventSourceCreate failed".into());
            }

            let down = CGEventCreateKeyboardEvent(source, K_VK_DELETE, true);
            if down.is_null() {
                CFRelease(source);
                return Err("delete down event null".into());
            }
            CGEventPost(CG_HID_EVENT_TAP, down);
            CFRelease(down);

            let up = CGEventCreateKeyboardEvent(source, K_VK_DELETE, false);
            if !up.is_null() {
                CGEventPost(CG_HID_EVENT_TAP, up);
                CFRelease(up);
            }

            CFRelease(source);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::is_same_paste_target;

    #[test]
    fn same_paste_target_decision() {
        // Same app still frontmost → paste is allowed (case/whitespace tolerant).
        assert!(is_same_paste_target(Some("Notes"), Some("Notes")));
        assert!(is_same_paste_target(Some("Notes"), Some("notes")));
        assert!(is_same_paste_target(Some(" Slack "), Some("slack")));

        // User switched apps during the ASR delay → mismatch → clipboard-only.
        assert!(!is_same_paste_target(Some("Notes"), Some("Messages")));

        // Unknown identity on either side is NOT a confirmed match → do not paste.
        assert!(!is_same_paste_target(Some("Notes"), None));
        assert!(!is_same_paste_target(None, Some("Notes")));
        assert!(!is_same_paste_target(None, None));
        assert!(!is_same_paste_target(Some(""), Some("")));
        assert!(!is_same_paste_target(Some("   "), Some("   ")));
    }
}
