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

/// YV74 — what a paste attempt is ALLOWED to claim.
///
/// The audit found "Pasted into frontmost app" written the moment the ⌘V chord
/// was posted, with the production log showing the same success recorded for
/// pastes that demonstrably never landed. The receipt (YV39) existed but only
/// the background clipboard-restore looked at it. This enum is the single place
/// the claim is decided, and `pasted: true` is produced by exactly one arm of
/// [`outcome_for_verdict`] — the one holding a receipt.
#[derive(Debug, PartialEq, Eq)]
pub enum PasteVerdict {
    /// A consumer read the pasteboard promise AFTER ⌘V, within the window.
    Pasted { after_ms: u128 },
    /// No receipt: the chord never went out, nothing read it in time, or the
    /// user took the clipboard back mid-flight.
    NotPasted { reason: String },
}

/// Where the transcript ended up once the verdict was known.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Recopy {
    /// It is on the clipboard — either the publish that fed ⌘V, or the plain
    /// text we put back after a failed paste. A manual ⌘V works.
    Kept,
    /// Deliberately not put back: the user copied something of their own while
    /// we waited, and their copy wins (the same rule `paste_tx::settle`
    /// follows). The toast's "Copy again" is how they get the transcript back.
    UserCopyWins,
    /// The clipboard write itself failed.
    Failed(String),
}

/// Turn an observed verdict into the outcome that gets recorded (history row,
/// latency line, toast). `pasted: true` lives on the receipt arm and nowhere
/// else — that is the whole point of YV74.
pub(crate) fn outcome_for_verdict(verdict: &PasteVerdict, recopy: Recopy) -> PasteOutcome {
    match verdict {
        PasteVerdict::Pasted { .. } => PasteOutcome {
            copied: true,
            pasted: true,
            message: "Pasted into frontmost app".into(),
        },
        PasteVerdict::NotPasted { reason } => match recopy {
            // Never lose text: the transcript is on the clipboard, so a manual
            // ⌘V still works and the toast says so.
            Recopy::Kept => PasteOutcome {
                copied: true,
                pasted: false,
                message: format!("Paste didn't land ({reason}) — text is on your clipboard"),
            },
            Recopy::UserCopyWins => PasteOutcome {
                copied: false,
                pasted: false,
                message: format!("Paste didn't land ({reason}) — your own copy was left alone"),
            },
            Recopy::Failed(e) => PasteOutcome {
                copied: false,
                pasted: false,
                message: format!("Paste didn't land ({reason}) and the clipboard failed: {e}"),
            },
        },
    }
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

/// YV39/YV74 — may this take synthesize ⌘V at all?
///
/// A take whose paste generation moved on was cancelled (or interrupted) while
/// it was still transcribing: the user has since told Yap to stop, so its text
/// is copied and NEVER pasted, whatever auto-paste says. It lives here next to
/// the honesty rule because "we must not paste" and "we must not claim we
/// pasted" are the two halves of the same guarantee.
pub fn should_paste(
    auto_paste: bool,
    take_generation: u64,
    current_generation: u64,
    focus_ok: bool,
) -> bool {
    auto_paste && take_generation == current_generation && focus_ok
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
/// YV74: that receipt now also gates what we CLAIM. Until this item, success
/// was recorded as soon as the chord was posted — the caller never looked at
/// the receipt, so a paste that landed nowhere still wrote "Pasted into
/// frontmost app" into the history row, the toast and the latency line. We now
/// block for up to `paste_tx::RECEIPT_WINDOW` (1.5s) waiting for a real
/// post-⌘V read; only then is `pasted: true` produced. Without one the reason
/// is logged at ERROR and the transcript goes back on the clipboard as plain
/// text so nothing is ever lost.
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
    let handle = match paste_tx::publish_and_paste(app, text) {
        Ok(handle) => handle,
        Err(e) => {
            // The audit (yap-logs finding 3) found a pasted=false transcript
            // whose cause was unrecoverable from the log: the paste path only
            // recorded the boolean. Log the actual reason + the app we aimed at.
            log::error!(
                "paste failed: {e} (frontmost={})",
                crate::focus::frontmost_app_name().unwrap_or_else(|| "unknown".into())
            );
            // Leave the transcript on the clipboard so ⌘V still works by hand.
            return copy_only(app, text, &format!("Copied, but paste failed ({e})."));
        }
    };

    // YV74 — the honest bit. Nothing below may report success without this.
    match handle.await_receipt(paste_tx::RECEIPT_WINDOW) {
        Ok(after) => {
            log::info!(
                "paste confirmed: clipboard read {}ms after ⌘V",
                after.as_millis()
            );
            outcome_for_verdict(
                &PasteVerdict::Pasted {
                    after_ms: after.as_millis(),
                },
                Recopy::Kept,
            )
        }
        Err(no_receipt) => {
            log::error!(
                "paste not confirmed: {} (frontmost={}, window={}ms)",
                no_receipt.reason,
                crate::focus::frontmost_app_name().unwrap_or_else(|| "unknown".into()),
                paste_tx::RECEIPT_WINDOW.as_millis()
            );
            // Never lose text. The promise on the pasteboard dies with the
            // transaction, so put the transcript back as plain text — unless
            // the user copied something themselves while we waited, in which
            // case their clipboard wins and the toast's "Copy again" is how
            // they take the transcript back.
            let recopy = if no_receipt.clipboard_taken {
                Recopy::UserCopyWins
            } else {
                match copy_text(app, text) {
                    Ok(()) => Recopy::Kept,
                    Err(e) => Recopy::Failed(e),
                }
            };
            outcome_for_verdict(
                &PasteVerdict::NotPasted {
                    reason: no_receipt.reason,
                },
                recopy,
            )
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
    use std::time::Duration;

    use super::{is_same_paste_target, outcome_for_verdict, should_paste, PasteVerdict, Recopy};
    use crate::paste_tx;

    /// YV74 — the claim is earned, not assumed. A transaction that HAS a
    /// post-⌘V read receipt is the only input that produces `pasted: true`;
    /// the identical call on a transaction without one does not.
    #[test]
    fn paste_success_only_after_receipt() {
        let receipted = paste_tx::stub_handle(true)
            .await_receipt(Duration::from_millis(200))
            .expect("a stubbed post-injection receipt is observable");
        let ok = outcome_for_verdict(
            &PasteVerdict::Pasted {
                after_ms: receipted.as_millis(),
            },
            Recopy::Kept,
        );
        assert!(ok.pasted, "a receipt is what makes the paste claim true");
        assert!(ok.copied);
        assert_eq!(ok.message, "Pasted into frontmost app");

        // Same code path, no receipt → the success message is unreachable.
        let silent = paste_tx::stub_handle(false).await_receipt(Duration::from_millis(20));
        let reason = silent.expect_err("no receipt was recorded").reason;
        let bad = outcome_for_verdict(&PasteVerdict::NotPasted { reason }, Recopy::Kept);
        assert!(!bad.pasted, "no receipt must never be reported as a paste");
        assert_ne!(bad.message, "Pasted into frontmost app");
    }

    /// The production log the audit read showed paste failures recorded as
    /// successes with no diagnostic. A silent transaction must record
    /// `pasted: false`, name the reason, and keep the transcript reachable.
    #[test]
    fn paste_no_receipt_records_false_and_keeps_clipboard() {
        let window = Duration::from_millis(30);
        let failure = paste_tx::stub_handle(false)
            .await_receipt(window)
            .expect_err("nothing read the promise");
        assert!(
            failure.reason.contains("no app read the clipboard"),
            "the reason has to survive into the log/toast: {}",
            failure.reason
        );
        assert!(
            !failure.clipboard_taken,
            "a plain timeout leaves us owning the pasteboard, so the transcript goes back on it"
        );

        let outcome = outcome_for_verdict(
            &PasteVerdict::NotPasted {
                reason: failure.reason.clone(),
            },
            Recopy::Kept,
        );
        assert!(!outcome.pasted);
        assert!(outcome.copied, "never lose text: the transcript is kept");
        assert!(outcome.message.contains("on your clipboard"));
        assert!(outcome.message.contains(&failure.reason));

        // The one case where the transcript is NOT put back is the user copying
        // something of their own — their clipboard wins, and "Copy again" in the
        // toast is how they take the transcript back.
        let user_copied = outcome_for_verdict(
            &PasteVerdict::NotPasted {
                reason: "the clipboard changed before the paste was read".into(),
            },
            Recopy::UserCopyWins,
        );
        assert!(!user_copied.pasted);
        assert!(!user_copied.copied);
    }

    /// A take cancelled while it was still transcribing is copy-only: the ⌘V is
    /// never synthesized, so no receipt can exist and no success is claimable.
    #[test]
    fn stale_generation_never_pastes() {
        assert!(should_paste(true, 7, 7, true), "the live take pastes");
        assert!(
            !should_paste(true, 7, 8, true),
            "the user cancelled during transcription — copy only"
        );
        assert!(
            !should_paste(false, 7, 7, true),
            "auto-paste off is copy-only"
        );
        assert!(
            !should_paste(true, 7, 7, false),
            "no text target focused — copy only"
        );
    }

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
