//! YV39 — receipt-sequenced "reliable paste".
//!
//! The old path was `write clipboard → ⌘V → sleep 400ms → restore`. The chord
//! is only *enqueued* at that point: the target application reads the
//! pasteboard whenever its event loop gets to it, so any fixed delay loses the
//! race on a busy/slow target and the user gets their OLD clipboard pasted
//! back. (Handy hit this as their #502; Yap had the same bug shape.)
//!
//! This module publishes the transcript as a *lazy promise* instead and waits
//! for macOS to tell us a consumer actually read it — a "receipt" — before
//! restoring: `declareTypes:owner:` puts a promise (no data) on the general
//! pasteboard, and AppKit calls `pasteboard:provideDataForType:` on our owner
//! object when anything asks for the text. That callback IS the read receipt.
//!
//! Two rules make the receipt trustworthy:
//!
//! 1. Only receipts observed *after* the ⌘V chord was injected count. An
//!    earlier read is an eager third party (clipboard manager, antivirus)
//!    reacting to the pasteboard change itself.
//! 2. We only restore while we still own the pasteboard — `changeCount`
//!    unchanged and no ownership-lost callback. If the user copied something
//!    else in the meantime, their copy wins and we touch nothing.
//!
//! The restore is additionally gated on a short quiet period after the *last*
//! receipt, because some apps read several times per paste (Chromium probes,
//! then reads), and capped by a bounded timeout so a chord that lands nowhere
//! cannot strand the transaction. Yap's failure mode is always "the transcript
//! stays on the clipboard" (never lose text), never "stale content pasted".
//!
//! Ported from Handy's `src-tauri/src/paste_tx/` (the `TxState`/`evaluate`
//! decision core is kept verbatim — it is pure and unit-tested); the Yap
//! specifics are: no auto-submit, text-only restore, and the no-receipt
//! outcome keeps the transcript instead of restoring.

use std::time::{Duration, Instant};

/// How long after the *last* observed read the transcript stays on the
/// clipboard before we restore. Covers apps that read several times per paste.
const QUIET_PERIOD: Duration = Duration::from_millis(200);

/// Upper bound on how long the transcript may occupy the clipboard before we
/// settle regardless of receipts. Long enough that a realistically loaded
/// target always gets to read first.
const RESTORE_TIMEOUT: Duration = Duration::from_secs(8);

/// When the chord could not be injected at all, no legitimate receipt can
/// arrive, so settle quickly instead of waiting out the full timeout.
const FAILED_INJECTION_TIMEOUT: Duration = Duration::from_millis(500);

/// Shared, cross-thread record of one paste transaction.
#[derive(Debug)]
pub struct TxState {
    /// When the transcript was published to the pasteboard.
    published_at: Instant,
    /// When the ⌘V chord was injected. Only receipts *after* this count as
    /// evidence the target read the transcript.
    injected_at: Option<Instant>,
    /// The chord could not be sent; short-circuit the wait.
    injection_failed: bool,
    /// Times at which a consumer requested the promised text.
    receipts: Vec<Instant>,
    /// Someone else took pasteboard ownership (the user copied elsewhere, …).
    ownership_lost: bool,
    /// First post-injection receipt has been logged.
    logged_receipt: bool,
}

impl TxState {
    pub fn new() -> Self {
        Self {
            published_at: Instant::now(),
            injected_at: None,
            injection_failed: false,
            receipts: Vec::new(),
            ownership_lost: false,
            logged_receipt: false,
        }
    }

    /// Records a read receipt, logging the first one that counts as evidence.
    pub fn record_receipt(&mut self, at: Instant) {
        self.receipts.push(at);
        if !self.logged_receipt {
            if let Some(injected) = self.injected_at {
                if at >= injected {
                    self.logged_receipt = true;
                    log::info!(
                        "reliable-paste: clipboard read {}ms after ⌘V",
                        at.duration_since(injected).as_millis()
                    );
                }
            }
        }
    }

    fn last_receipt_after_injection(&self) -> Option<Instant> {
        let injected = self.injected_at?;
        self.receipts.iter().copied().rev().find(|t| *t >= injected)
    }

    pub fn any_receipt_after_injection(&self) -> bool {
        self.last_receipt_after_injection().is_some()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum WaitDecision {
    KeepWaiting,
    /// Stop waiting; settle the transaction (guarded restore).
    Finish,
}

/// Pure decision: given the current transaction state, keep waiting for the
/// target to read, or finish now.
pub fn evaluate(state: &TxState, now: Instant) -> WaitDecision {
    if state.ownership_lost {
        return WaitDecision::Finish;
    }
    if let Some(last) = state.last_receipt_after_injection() {
        if now.duration_since(last) >= QUIET_PERIOD {
            return WaitDecision::Finish;
        }
    }
    let deadline = if state.injection_failed {
        FAILED_INJECTION_TIMEOUT
    } else {
        RESTORE_TIMEOUT
    };
    if now.duration_since(state.published_at) >= deadline {
        return WaitDecision::Finish;
    }
    WaitDecision::KeepWaiting
}

/// Publish the transcript as a lazy pasteboard promise, inject ⌘V, and hand the
/// guarded restore to a background waiter. Returns once the chord has been
/// posted — the receipt handshake and restore complete asynchronously, so this
/// adds nothing to the release→paste latency.
///
/// `Err` means the transcript was NOT pasted; the caller re-copies it as plain
/// text and reports the failure.
#[cfg(target_os = "macos")]
pub fn publish_and_paste(app: &tauri::AppHandle, text: &str) -> Result<(), String> {
    use std::sync::mpsc;

    // Publishing and the chord both belong on the main thread: AppKit services
    // promised pasteboard data on the main run loop, and Yap's CGEvent ⌘V is
    // main-thread-only (see the SIGTRAP note in paste.rs).
    let (tx, rx) = mpsc::channel();
    let app_main = app.clone();
    let text = text.to_string();
    app.run_on_main_thread(move || {
        let _ = tx.send(mac::publish_and_chord(&app_main, text));
    })
    .map_err(|e| format!("schedule main-thread paste: {e}"))?;

    rx.recv_timeout(Duration::from_secs(3))
        .map_err(|_| "paste timed out waiting for main thread".to_string())?
}

#[cfg(not(target_os = "macos"))]
pub fn publish_and_paste(_app: &tauri::AppHandle, _text: &str) -> Result<(), String> {
    Err("paste only implemented on macOS".into())
}

#[cfg(target_os = "macos")]
mod mac {
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use objc2::rc::Retained;
    use objc2::{define_class, msg_send, AnyThread, DefinedClass};
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::{NSArray, NSInteger, NSObject, NSString};
    use tauri::AppHandle;
    use tauri_plugin_clipboard_manager::ClipboardExt;

    use super::{evaluate, TxState, WaitDecision};

    /// How often the waiter re-evaluates the transaction.
    const POLL: Duration = Duration::from_millis(15);

    /// Concealment marker types declared alongside the text so well-behaved
    /// clipboard managers (Maccy, Paste, …) skip the transcript — fewer eager
    /// readers means a receipt is more likely to be the actual paste target.
    /// These are conventions; nothing enforces them.
    const CONCEALMENT_TYPES: [&str; 3] = [
        "org.nspasteboard.TransientType",
        "org.nspasteboard.ConcealedType",
        "org.nspasteboard.AutoGeneratedType",
    ];

    pub struct ProviderIvars {
        state: Arc<Mutex<TxState>>,
        text: String,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements and the ivars are
        // plain Rust values guarded by a Mutex.
        #[unsafe(super(NSObject))]
        #[name = "YapPasteProvider"]
        #[ivars = ProviderIvars]
        pub struct PasteProvider;

        impl PasteProvider {
            // NSPasteboardOwner informal protocol: the pasteboard is asking for
            // the promised data — our receipt that a consumer read it.
            #[unsafe(method(pasteboard:provideDataForType:))]
            fn pasteboard_provide_data_for_type(
                &self,
                pasteboard: &NSPasteboard,
                data_type: &NSString,
            ) {
                let ivars = self.ivars();
                // SAFETY: NSPasteboardTypeString is a read-only framework static.
                let is_text = unsafe { data_type.isEqualToString(NSPasteboardTypeString) };
                // Only a text read counts: a request for a concealment marker is
                // a clipboard manager inspecting markers, not the paste target
                // reading the transcript.
                if is_text {
                    if let Ok(mut state) = ivars.state.lock() {
                        state.record_receipt(Instant::now());
                    }
                }
                let payload = if is_text {
                    NSString::from_str(&ivars.text)
                } else {
                    NSString::from_str("")
                };
                pasteboard.setString_forType(&payload, data_type);
            }

            #[unsafe(method(pasteboardChangedOwner:))]
            fn pasteboard_changed_owner(&self, _pasteboard: &NSPasteboard) {
                if let Ok(mut state) = self.ivars().state.lock() {
                    state.ownership_lost = true;
                }
            }
        }
    );

    impl PasteProvider {
        fn new(state: Arc<Mutex<TxState>>, text: String) -> Retained<Self> {
            let this = Self::alloc().set_ivars(ProviderIvars { state, text });
            unsafe { msg_send![super(this), init] }
        }
    }

    struct Pending {
        state: Arc<Mutex<TxState>>,
        /// The user's clipboard before we published (text only — image restore
        /// stays a tracked follow-up, as it was under the old sleep path).
        saved_text: Option<String>,
        /// Pasteboard changeCount right after `declareTypes:owner:`. Restoring
        /// is only safe while it still matches.
        change_count: NSInteger,
        provider: Option<Retained<PasteProvider>>,
        transcript: String,
        settled: bool,
    }

    /// The transaction currently holding the pasteboard, if any. A new paste
    /// settles it first (see `flush_pending`) so the snapshot below captures
    /// the user's real clipboard rather than the previous transcript.
    static PENDING: Mutex<Option<Arc<Mutex<Pending>>>> = Mutex::new(None);

    /// Settle a transaction exactly once. Must run on the main thread.
    fn settle(pending: &Arc<Mutex<Pending>>, app: &AppHandle) {
        let mut p = match pending.lock() {
            Ok(p) => p,
            Err(_) => return,
        };
        if p.settled {
            return;
        }
        p.settled = true;

        let (receipt_seen, ownership_lost) = match p.state.lock() {
            Ok(st) => (st.any_receipt_after_injection(), st.ownership_lost),
            Err(_) => (false, true),
        };

        let still_ours =
            !ownership_lost && NSPasteboard::generalPasteboard().changeCount() == p.change_count;
        if !still_ours {
            log::info!("reliable-paste: clipboard changed externally — leaving it untouched");
        } else if receipt_seen {
            // The target demonstrably read the transcript: hand the user their
            // own clipboard back. This is the receipt, not a guess at a sleep.
            let clipboard = app.clipboard();
            match &p.saved_text {
                Some(text) => {
                    let _ = clipboard.write_text(text.clone());
                }
                None => {
                    let _ = clipboard.clear();
                }
            }
            log::info!("reliable-paste: read confirmed — prior clipboard restored");
        } else {
            // Nobody read it, so the ⌘V never landed. Never lose text: replace
            // the dead promise with the transcript as plain text so the user
            // can still ⌘V it themselves.
            let _ = app.clipboard().write_text(p.transcript.clone());
            log::warn!(
                "reliable-paste: no clipboard read within timeout — left the transcript on the clipboard"
            );
        }

        // Release the owner; the outstanding promise dies with the pasteboard
        // contents we just replaced (or with the external change).
        p.provider = None;
    }

    /// Settle any still-open transaction now, so the caller's snapshot sees the
    /// user's original clipboard instead of the previous transcript promise.
    fn flush_pending(app: &AppHandle) {
        let previous = match PENDING.lock() {
            Ok(mut slot) => slot.take(),
            Err(_) => None,
        };
        if let Some(previous) = previous {
            settle(&previous, app);
        }
    }

    fn spawn_waiter(pending: Arc<Mutex<Pending>>, app: AppHandle) {
        thread::spawn(move || {
            loop {
                thread::sleep(POLL);
                let finished = {
                    let p = match pending.lock() {
                        Ok(p) => p,
                        Err(_) => return,
                    };
                    // A newer paste already settled this transaction.
                    if p.settled {
                        return;
                    }
                    let done = match p.state.lock() {
                        Ok(st) => evaluate(&st, Instant::now()) == WaitDecision::Finish,
                        Err(_) => return,
                    };
                    done
                };
                if finished {
                    break;
                }
            }

            let pending_for_finish = pending.clone();
            let app_for_finish = app.clone();
            let _ = app.run_on_main_thread(move || {
                settle(&pending_for_finish, &app_for_finish);
                if let Ok(mut slot) = PENDING.lock() {
                    let is_us = slot
                        .as_ref()
                        .map(|current| Arc::ptr_eq(current, &pending))
                        .unwrap_or(false);
                    if is_us {
                        *slot = None;
                    }
                }
            });
        });
    }

    /// Main-thread half: publish the promise, post ⌘V, arm the waiter.
    pub fn publish_and_chord(app: &AppHandle, text: String) -> Result<(), String> {
        flush_pending(app);

        let saved_text = app.clipboard().read_text().ok().filter(|t| !t.is_empty());

        let state = Arc::new(Mutex::new(TxState::new()));
        let provider = PasteProvider::new(state.clone(), text.clone());
        let pasteboard = NSPasteboard::generalPasteboard();

        let mut types: Vec<Retained<NSString>> = Vec::with_capacity(1 + CONCEALMENT_TYPES.len());
        types.push(NSString::from_str("public.utf8-plain-text"));
        for t in CONCEALMENT_TYPES {
            types.push(NSString::from_str(t));
        }
        let types = NSArray::from_retained_slice(&types);

        // declareTypes:owner: clears the pasteboard and puts our promise on it;
        // the return value is the new changeCount.
        let change_count: NSInteger =
            unsafe { msg_send![&*pasteboard, declareTypes: &*types, owner: &*provider] };
        if change_count <= 0 {
            return Err("declareTypes:owner: failed".into());
        }

        // Mark the injection *before* posting: a fast target can legitimately
        // read while the chord is still in flight.
        if let Ok(mut st) = state.lock() {
            st.injected_at = Some(Instant::now());
        }
        let chord = crate::paste::cg::post_cmd_v();
        if let Err(e) = &chord {
            // Keep the transaction alive: the waiter settles it after the short
            // failed-injection timeout and leaves the transcript behind.
            if let Ok(mut st) = state.lock() {
                st.injection_failed = true;
            }
            log::error!("reliable-paste: ⌘V injection failed: {e}");
        }

        let pending = Arc::new(Mutex::new(Pending {
            state,
            saved_text,
            change_count,
            provider: Some(provider),
            transcript: text,
            settled: false,
        }));
        if let Ok(mut slot) = PENDING.lock() {
            *slot = Some(pending.clone());
        }
        spawn_waiter(pending, app.clone());

        chord
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn published_ago(ago: Duration) -> TxState {
        let mut s = TxState::new();
        s.published_at = Instant::now() - ago;
        s
    }

    #[test]
    fn keeps_waiting_without_receipt_within_timeout() {
        let s = published_ago(Duration::from_millis(100));
        assert_eq!(evaluate(&s, Instant::now()), WaitDecision::KeepWaiting);
    }

    #[test]
    fn finishes_after_quiet_period_once_read() {
        let mut s = published_ago(Duration::from_millis(300));
        s.injected_at = Some(Instant::now() - Duration::from_millis(250));
        s.receipts.push(Instant::now() - QUIET_PERIOD);
        assert!(s.any_receipt_after_injection());
        assert_eq!(evaluate(&s, Instant::now()), WaitDecision::Finish);
    }

    #[test]
    fn waits_through_quiet_period_after_recent_read() {
        let mut s = published_ago(Duration::from_millis(300));
        s.injected_at = Some(Instant::now() - Duration::from_millis(100));
        s.receipts.push(Instant::now() - Duration::from_millis(50));
        assert_eq!(evaluate(&s, Instant::now()), WaitDecision::KeepWaiting);
    }

    /// A read *before* ⌘V is a clipboard manager reacting to the change, not
    /// the paste target — it must never end the wait.
    #[test]
    fn pre_injection_receipt_does_not_count() {
        let mut s = published_ago(Duration::from_millis(300));
        s.receipts.push(Instant::now() - Duration::from_millis(200));
        s.injected_at = Some(Instant::now() - Duration::from_millis(100));
        assert!(!s.any_receipt_after_injection());
        assert_eq!(evaluate(&s, Instant::now()), WaitDecision::KeepWaiting);
    }

    #[test]
    fn finishes_on_timeout_without_receipt() {
        let s = published_ago(RESTORE_TIMEOUT);
        assert_eq!(evaluate(&s, Instant::now()), WaitDecision::Finish);
    }

    #[test]
    fn failed_injection_uses_short_timeout() {
        let mut s = published_ago(FAILED_INJECTION_TIMEOUT);
        s.injection_failed = true;
        assert_eq!(evaluate(&s, Instant::now()), WaitDecision::Finish);
        // …but the full timeout has NOT elapsed for a healthy chord.
        let healthy = published_ago(FAILED_INJECTION_TIMEOUT);
        assert_eq!(
            evaluate(&healthy, Instant::now()),
            WaitDecision::KeepWaiting
        );
    }

    #[test]
    fn ownership_loss_finishes_immediately() {
        let mut s = published_ago(Duration::from_millis(10));
        s.ownership_lost = true;
        assert_eq!(evaluate(&s, Instant::now()), WaitDecision::Finish);
    }

    /// The very first receipt after ⌘V is what unblocks the restore; the logged
    /// flag must latch so a chatty target (Chromium probes then reads) does not
    /// re-log on every read.
    #[test]
    fn receipt_logging_latches_once() {
        let mut s = published_ago(Duration::from_millis(10));
        s.injected_at = Some(Instant::now());
        s.record_receipt(Instant::now());
        assert!(s.logged_receipt);
        s.record_receipt(Instant::now());
        assert_eq!(s.receipts.len(), 2);
    }
}
