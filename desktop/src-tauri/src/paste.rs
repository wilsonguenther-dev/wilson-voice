//! Clipboard + frontmost-app paste.
//!
//! CRITICAL (crash 2026-07-17): enigo Key::Unicode on a background thread calls
//! HIToolbox TSMGetInputSourceProperty → dispatch_assert_queue_fail → SIGTRAP.
//! All synthetic keystrokes must run on the **main thread**, and we use raw
//! virtual keycodes (ANSI V + Command) so we never touch the input-source APIs.

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::paste_tx;
use crate::perm_probe;
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

/// Clipboard-only outcome for the paths that never reach an [`InsertBackend`]
/// (a `--smoke` refusal, auto-paste off).
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

// ── yap-p0-insertion-bridge-fallback — the insertion path ───────────────────
//
// Wilson 2026-10-17: "the bridges ... nothing's working". Until this item every
// insertion asked the old Accessibility-only trust check and, on a no, copied
// the take and said so in a toast nobody saw — a silent `pasted=false`. The
// decision now reads THE probe (`perm_probe::probe_all`, the one place Yap asks
// macOS about a grant) once per take and picks one of three paths, and the only
// path that cannot insert by itself puts the text in front of the user at once.

/// The Tauri event the pill's Copy card listens for (desktop/src/pill/copyCardModel.ts, desktop/src/pill/CopyCard.tsx).
pub const INSERT_COPY_CARD_EVENT: &str = "insert-copy-card";

/// The card's one line, verbatim from the item spec.
pub const COPY_CARD_MESSAGE: &str = "Copied — press ⌘V. Repair to auto-insert.";

/// How a take reaches the focused app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InsertPath {
    /// Accessibility is granted: set `kAXSelectedText` on the focused element
    /// (falling back to [`InsertPath::PasteBridge`] when the element refuses).
    Ax,
    /// PostEvent is granted (Accessibility is not): pasteboard + CGEventPost ⌘V,
    /// receipt-checked by `paste_tx` exactly as before.
    PasteBridge,
    /// Neither grant: no keystroke is attempted; the Copy card shows the text.
    CopyCard,
}

/// THE decision, pure. Only a live `Granted` counts — Denied, NotDetermined,
/// StaleIdentity and Unknown all mean "this grant will not work right now".
pub fn choose_insert_path(probe: &perm_probe::ProbeReport) -> InsertPath {
    if probe.accessibility.state == perm_probe::ProbeState::Granted {
        InsertPath::Ax
    } else if probe.post_event.state == perm_probe::ProbeState::Granted {
        InsertPath::PasteBridge
    } else {
        InsertPath::CopyCard
    }
}

/// Why the card is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyCardReason {
    /// Neither Accessibility nor PostEvent is granted.
    NoPermission,
    /// ⌘V went out but nothing read the pasteboard inside `RECEIPT_WINDOW`.
    NoReceipt,
    /// The ⌘V chord itself could not be posted.
    PasteFailed,
}

/// The `insert-copy-card` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyCard {
    pub text: String,
    pub reason: CopyCardReason,
    /// What went wrong, for the log and the card's title attribute.
    pub detail: String,
    /// The transcript is on the clipboard right now (⌘V works without Copy).
    pub copied: bool,
    /// `repair_permission`'s argument for the card's Repair button.
    pub repair_service: &'static str,
}

/// What an AX insert attempt observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AxInsert {
    Inserted,
    /// The element refused (not settable, no readable value, the set errored,
    /// or it claimed success and its value did not change).
    Refused(String),
}

/// What a paste-bridge transaction observed after ⌘V was posted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeReceipt {
    Confirmed {
        after_ms: u128,
    },
    Missing {
        reason: String,
        clipboard_taken: bool,
    },
}

/// Every side effect an insertion can have, so the whole decision is testable
/// with a counting fake (tests/insertion_matrix.rs) and the live app runs the
/// same code through [`LiveInsert`].
pub trait InsertBackend {
    /// Put the text on the clipboard as plain text.
    fn copy(&mut self, text: &str) -> Result<(), String>;
    /// The frontmost app right now (the YV21 wrong-target guard).
    fn frontmost_app(&mut self) -> Option<String>;
    /// `kAXSelectedText` on the focused element.
    fn ax_insert(&mut self, text: &str) -> AxInsert;
    /// Publish + CGEventPost ⌘V + wait for the read receipt. `Err` = the chord
    /// never went out.
    fn paste_bridge(&mut self, text: &str) -> Result<BridgeReceipt, String>;
    /// CGEventPost a bare Delete (YV49 "delete that").
    fn post_delete(&mut self) -> Result<(), String>;
    /// Emit `insert-copy-card`.
    fn show_copy_card(&mut self, card: CopyCard);
    /// A ⌘V with no receipt: the detail line for the card (the live backend
    /// also pokes the permission watcher so the health row agrees).
    fn no_receipt_detail(&mut self, reason: &str) -> String {
        reason.to_string()
    }
}

fn copy_only_with(b: &mut dyn InsertBackend, text: &str, message: &str) -> PasteOutcome {
    match b.copy(text) {
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

/// Copy, then show the card — whether or not the copy worked (the card carries
/// its own Copy button, so the text is never only in a toast).
fn copy_card_with(
    b: &mut dyn InsertBackend,
    text: &str,
    reason: CopyCardReason,
    detail: String,
) -> PasteOutcome {
    let copied = b.copy(text).is_ok();
    b.show_copy_card(CopyCard {
        text: text.to_string(),
        reason,
        detail,
        copied,
        repair_service: perm_probe::Service::Accessibility.key(),
    });
    PasteOutcome {
        copied,
        pasted: false,
        message: if copied {
            COPY_CARD_MESSAGE.into()
        } else {
            "Clipboard failed — use Copy on the card. Repair to auto-insert.".into()
        },
    }
}

/// The whole insertion decision for one take, side effects behind `b`.
///
/// - `want_paste == false` (auto-paste off, a stale/cancelled take, no text
///   target): copy only, never a keystroke and never a card.
/// - [`InsertPath::CopyCard`]: copy + card, zero keystrokes.
/// - Ax / PasteBridge: the YV21 wrong-target guard first (copy only on a
///   switch), then AX → bridge, and a bridge with no receipt ends on the card —
///   never a silent `pasted=false`.
pub fn insert_with(
    b: &mut dyn InsertBackend,
    path: InsertPath,
    text: &str,
    want_paste: bool,
    source_app: Option<&str>,
) -> PasteOutcome {
    if !want_paste {
        return copy_only_with(b, text, "Copied to clipboard (⌘V to paste)");
    }
    if path == InsertPath::CopyCard {
        return copy_card_with(
            b,
            text,
            CopyCardReason::NoPermission,
            "Accessibility and PostEvent are both off for Yap".into(),
        );
    }
    // YV21 (audit M1): `source_app` was frontmost at record start; if the user
    // switched apps during the ASR delay nothing is inserted — the text is
    // copied and never lands in a different app.
    if let Some(dictated) = source_app {
        let current = b.frontmost_app();
        if !is_same_paste_target(Some(dictated), current.as_deref()) {
            return copy_only_with(b, text, "Copied — you switched apps, so I didn't paste");
        }
    }
    if path == InsertPath::Ax {
        match b.ax_insert(text) {
            // `copied` is the "the text was not lost" bit every consumer reads
            // (paste_entry's Ok/Err, the take's last_error) — an AX insert
            // delivered it, and left the user's clipboard alone.
            AxInsert::Inserted => {
                log::info!("insert confirmed: AXSelectedText set on the focused element");
                return PasteOutcome {
                    copied: true,
                    pasted: true,
                    message: "Inserted into frontmost app".into(),
                };
            }
            AxInsert::Refused(why) => {
                log::info!("AX insert refused ({why}) — falling back to the paste bridge");
            }
        }
    }
    match b.paste_bridge(text) {
        Ok(BridgeReceipt::Confirmed { after_ms }) => {
            log::info!("paste confirmed: clipboard read {after_ms}ms after ⌘V");
            outcome_for_verdict(&PasteVerdict::Pasted { after_ms }, Recopy::Kept)
        }
        Ok(BridgeReceipt::Missing {
            reason,
            clipboard_taken,
        }) => {
            log::error!(
                "paste not confirmed: {reason} (window={}ms) — showing the Copy card",
                paste_tx::RECEIPT_WINDOW.as_millis()
            );
            // Never lose text: the promise died with the transaction, so the
            // transcript goes back as plain text — unless the user copied
            // something of their own while we waited (their copy wins; the
            // card's Copy button is how they take the transcript back).
            let recopy = if clipboard_taken {
                Recopy::UserCopyWins
            } else {
                match b.copy(text) {
                    Ok(()) => Recopy::Kept,
                    Err(e) => Recopy::Failed(e),
                }
            };
            let copied = recopy == Recopy::Kept;
            let detail = b.no_receipt_detail(&reason);
            b.show_copy_card(CopyCard {
                text: text.to_string(),
                reason: CopyCardReason::NoReceipt,
                detail,
                copied,
                repair_service: perm_probe::Service::Accessibility.key(),
            });
            outcome_for_verdict(&PasteVerdict::NotPasted { reason }, recopy)
        }
        Err(e) => {
            log::error!("paste failed: {e} — showing the Copy card");
            copy_card_with(b, text, CopyCardReason::PasteFailed, e)
        }
    }
}

/// YV49 "delete that", side effects behind `b`. CopyCard posts nothing (there
/// is no grant to post a Delete with, and nothing to copy); otherwise the same
/// wrong-target guard as insertion, then one Delete keystroke.
pub fn delete_with(
    b: &mut dyn InsertBackend,
    path: InsertPath,
    source_app: Option<&str>,
) -> PasteOutcome {
    if path == InsertPath::CopyCard {
        return PasteOutcome {
            copied: false,
            pasted: false,
            message: "Accessibility is off for Yap — Repair to let me edit your selection.".into(),
        };
    }
    if let Some(started_in) = source_app {
        let current = b.frontmost_app();
        if !is_same_paste_target(Some(started_in), current.as_deref()) {
            return PasteOutcome {
                copied: false,
                pasted: false,
                message: "You switched apps, so I didn't delete anything".into(),
            };
        }
    }
    match b.post_delete() {
        Ok(()) => PasteOutcome {
            copied: false,
            pasted: true,
            message: "Deleted in frontmost app".into(),
        },
        Err(e) => {
            log::error!("delete failed: {e}");
            PasteOutcome {
                copied: false,
                pasted: false,
                message: format!("Couldn't delete ({e})."),
            }
        }
    }
}

/// The live backend: the real clipboard, AX, `paste_tx`, CGEvent and the pill.
struct LiveInsert<'a> {
    app: &'a AppHandle,
    /// From the take's one probe — never a second grant check.
    ax_granted: bool,
}

impl InsertBackend for LiveInsert<'_> {
    fn copy(&mut self, text: &str) -> Result<(), String> {
        copy_text(self.app, text)
    }
    fn frontmost_app(&mut self) -> Option<String> {
        crate::focus::frontmost_app_name()
    }
    fn ax_insert(&mut self, text: &str) -> AxInsert {
        match crate::focus::insert_selected_text(text) {
            Ok(()) => AxInsert::Inserted,
            Err(why) => AxInsert::Refused(why),
        }
    }
    fn paste_bridge(&mut self, text: &str) -> Result<BridgeReceipt, String> {
        // YV39: publish → ⌘V → restore on the READ RECEIPT; YV74: the receipt
        // is also what earns `pasted: true`.
        let handle = paste_tx::publish_and_paste(self.app, text)?;
        Ok(match handle.await_receipt(paste_tx::RECEIPT_WINDOW) {
            Ok(after) => BridgeReceipt::Confirmed {
                after_ms: after.as_millis(),
            },
            Err(n) => BridgeReceipt::Missing {
                reason: n.reason,
                clipboard_taken: n.clipboard_taken,
            },
        })
    }
    fn post_delete(&mut self) -> Result<(), String> {
        post_delete_on_main(self.app)
    }
    fn show_copy_card(&mut self, card: CopyCard) {
        crate::float_pill::hold_copy_card(self.app, card.clone());
        if let Err(e) = self.app.emit(INSERT_COPY_CARD_EVENT, card) {
            log::warn!("emit {INSERT_COPY_CARD_EVENT}: {e}");
        }
    }
    fn no_receipt_detail(&mut self, reason: &str) -> String {
        permissions::poke(permissions::WatchReason::Refusal);
        permissions::paste_failure_detail(self.ax_granted, reason)
    }
}

/// Insert one take into the frontmost app.
///
/// `source_app` is the frontmost app captured at dictation time (record start);
/// when it is `Some` the YV21 wrong-target guard is enforced. Pass `None` for
/// explicit user re-pastes (e.g. `paste_entry`), where the current frontmost
/// app IS the intended target.
pub fn copy_and_maybe_paste(
    app: &AppHandle,
    text: &str,
    want_paste: bool,
    source_app: Option<&str>,
) -> PasteOutcome {
    // Y0-D: a `--smoke` launch never synthesizes a keystroke into whatever window
    // happens to be frontmost on the developer's machine.
    if want_paste && !crate::smoke::paste_allowed() {
        log::warn!("{}", crate::smoke::PASTE_REFUSAL);
        return copy_only(app, text, crate::smoke::PASTE_REFUSAL);
    }
    if !want_paste {
        return copy_only(app, text, "Copied to clipboard (⌘V to paste)");
    }
    let probe = perm_probe::probe_all();
    let path = choose_insert_path(&probe);
    log::info!(
        "insert path={path:?} (ax={:?} post={:?})",
        probe.accessibility.state,
        probe.post_event.state
    );
    let mut live = LiveInsert {
        app,
        ax_granted: probe.accessibility.state == perm_probe::ProbeState::Granted,
    };
    insert_with(&mut live, path, text, true, source_app)
}

/// YV49 command mode: carry out "delete that" by pressing Delete on the
/// frontmost app, NOT by pasting an empty string (a no-op in several apps, and
/// a delete has no business overwriting the user's clipboard).
pub fn delete_selection(app: &AppHandle, source_app: Option<&str>) -> PasteOutcome {
    // Y0-D: same rule as the paste path — a smoke run posts no Delete chord.
    if !crate::smoke::paste_allowed() {
        log::warn!("{}", crate::smoke::DELETE_REFUSAL);
        return PasteOutcome {
            copied: false,
            pasted: false,
            message: crate::smoke::DELETE_REFUSAL.into(),
        };
    }
    let probe = perm_probe::probe_all();
    let mut live = LiveInsert {
        app,
        ax_granted: probe.accessibility.state == perm_probe::ProbeState::Granted,
    };
    delete_with(&mut live, choose_insert_path(&probe), source_app)
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
