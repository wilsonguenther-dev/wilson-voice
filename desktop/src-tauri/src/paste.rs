//! Clipboard + frontmost-app paste.
//!
//! CRITICAL (crash 2026-07-17): enigo Key::Unicode on a background thread calls
//! HIToolbox TSMGetInputSourceProperty → dispatch_assert_queue_fail → SIGTRAP.
//! All synthetic keystrokes must run on the **main thread**, and we use raw
//! virtual keycodes (ANSI V + Command) so we never touch the input-source APIs.

use std::sync::mpsc;
use std::time::Duration;
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

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

/// Always copy. Paste only when Accessibility is granted, **on the main thread**.
///
/// After a successful auto-paste the user's *prior* clipboard is restored (like
/// Wispr) so dictation never silently clobbers what they had copied. On paste
/// failure the transcript is left on the clipboard so they can ⌘V manually.
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
    // Snapshot the existing clipboard before we overwrite it. (Text only for now;
    // image/file restore is a tracked follow-up.)
    let prior = app.clipboard().read_text().ok();

    if let Err(e) = copy_text(app, text) {
        return PasteOutcome {
            copied: false,
            pasted: false,
            message: format!("Clipboard failed: {e}"),
        };
    }
    if !want_paste {
        return PasteOutcome {
            copied: true,
            pasted: false,
            message: "Copied to clipboard (⌘V to paste)".into(),
        };
    }
    if !permissions::is_accessibility_trusted() {
        return PasteOutcome {
            copied: true,
            pasted: false,
            message: "Copied. Enable Accessibility for Yap to auto-paste.".into(),
        };
    }

    // YV21 (audit M1): guard against a wrong-target paste. `source_app` was the
    // frontmost app at record start; sample the CURRENT frontmost app right
    // before synthesizing ⌘V. If the user switched apps during the ASR delay,
    // fall back to clipboard-only — the text is already copied, so nothing is
    // lost and it never lands in a different app.
    if let Some(dictated) = source_app {
        let current = crate::focus::frontmost_app_name();
        if !is_same_paste_target(Some(dictated), current.as_deref()) {
            return PasteOutcome {
                copied: true,
                pasted: false,
                message: "Copied — you switched apps, so I didn't paste".into(),
            };
        }
    }

    match dispatch_paste_on_main(app) {
        Ok(()) => {
            // Restore the prior clipboard once the target app has consumed the
            // paste. Off-thread + delayed so we neither block nor race the ⌘V.
            if let Some(prev) = prior {
                let app2 = app.clone();
                std::thread::spawn(move || {
                    // Wait past the target app's async ⌘V read before restoring.
                    // 400ms is a safer heuristic than 200ms for slow/busy targets;
                    // NSPasteboard.changeCount polling is a tracked follow-up.
                    std::thread::sleep(Duration::from_millis(400));
                    let _ = app2.clipboard().write_text(prev);
                });
            }
            PasteOutcome {
                copied: true,
                pasted: true,
                message: "Pasted into frontmost app".into(),
            }
        }
        Err(e) => PasteOutcome {
            copied: true,
            pasted: false,
            message: format!("Copied, but paste failed ({e})."),
        },
    }
}

/// Schedule paste on the main thread and wait briefly for completion.
pub fn dispatch_paste_on_main(app: &AppHandle) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let r = paste_cmd_v_main_thread();
        let _ = tx.send(r);
    })
    .map_err(|e| format!("schedule main-thread paste: {e}"))?;

    rx.recv_timeout(Duration::from_secs(3))
        .map_err(|_| "paste timed out waiting for main thread".to_string())?
}

/// macOS virtual keycodes (ANSI layout — independent of current input source).
#[cfg(target_os = "macos")]
mod cg {
    use std::ffi::c_void;

    pub type CGEventRef = *mut c_void;
    pub type CGEventSourceRef = *mut c_void;
    pub type CGKeyCode = u16;
    pub type CGEventFlags = u64;

    pub const CG_EVENT_FLAG_MASK_COMMAND: CGEventFlags = 0x100000;
    pub const CG_HID_EVENT_TAP: u32 = 0; // kCGHIDEventTap
    pub const K_VK_ANSI_V: CGKeyCode = 0x09;
    pub const K_VK_COMMAND: CGKeyCode = 0x37;

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
    pub fn post_cmd_v() -> Result<(), String> {
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
}

#[cfg(target_os = "macos")]
fn paste_cmd_v_main_thread() -> Result<(), String> {
    if !permissions::is_accessibility_trusted() {
        return Err("Accessibility not granted to Yap".into());
    }
    // Tiny settle so focus returns to the previous app after our UI/notification
    std::thread::sleep(Duration::from_millis(30));
    cg::post_cmd_v()
}

#[cfg(not(target_os = "macos"))]
fn paste_cmd_v_main_thread() -> Result<(), String> {
    Err("paste only implemented on macOS".into())
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
