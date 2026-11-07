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

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// How long after the *last* observed read the transcript stays on the
/// clipboard before we restore. Covers apps that read several times per paste.
const QUIET_PERIOD: Duration = Duration::from_millis(200);

/// YV74 — how long the caller waits for the target app to actually READ the
/// pasteboard before the paste is reported as a failure. The restore waiter
/// above is allowed a much longer leash (`RESTORE_TIMEOUT`) because holding the
/// clipboard costs the user nothing; the *claim* that the text landed cannot
/// wait that long, because the history row and the toast are written from it.
pub const RECEIPT_WINDOW: Duration = Duration::from_millis(1500);

/// How often [`PasteHandle::await_receipt`] re-checks the transaction.
const RECEIPT_POLL: Duration = Duration::from_millis(10);

/// Upper bound on how long the transcript may occupy the clipboard before we
/// settle regardless of receipts. Long enough that a realistically loaded
/// target always gets to read first.
const RESTORE_TIMEOUT: Duration = Duration::from_secs(8);

/// When the chord could not be injected at all, no legitimate receipt can
/// arrive, so settle quickly instead of waiting out the full timeout.
const FAILED_INJECTION_TIMEOUT: Duration = Duration::from_millis(500);

/// YV73 — how many read receipts a transaction retains. A chatty target
/// (Chromium probes the pasteboard, then reads) can request the promised text
/// hundreds of times inside one `RESTORE_TIMEOUT`, and every request used to
/// push an `Instant` onto a Vec nothing ever bounded. Only the NEWEST
/// post-injection receipt is ever read (`last_receipt_after_injection`), so
/// dropping the oldest is loss-free.
const MAX_RECEIPTS: usize = 64;

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
    /// Times at which a consumer requested the promised text, capped at
    /// [`MAX_RECEIPTS`] (oldest dropped).
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
        if self.receipts.len() >= MAX_RECEIPTS {
            self.receipts.remove(0);
        }
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

/// YV74 — why a transaction produced no read receipt inside the window.
#[derive(Debug, PartialEq, Eq)]
pub struct NoReceipt {
    /// Cause, logged at ERROR and surfaced in the toast.
    pub reason: String,
    /// Someone else took the pasteboard while we waited (the user copied
    /// something of their own). Their copy wins — the caller must NOT put the
    /// transcript back over it, exactly as `settle` refuses to restore.
    pub clipboard_taken: bool,
}

/// YV74 — a live paste transaction the caller can wait on.
///
/// The receipt machinery already existed (YV39) but only the background
/// restore waiter ever looked at it: `publish_and_paste` returned `Ok` the
/// instant the ⌘V chord was *posted*, so the caller recorded "Pasted into
/// frontmost app" before anything had read the pasteboard — and the production
/// log proved it, with successes recorded for pastes that never landed. This
/// handle hands the same receipt to the caller so the claim can be earned.
pub struct PasteHandle {
    state: Arc<Mutex<TxState>>,
}

impl PasteHandle {
    pub fn new(state: Arc<Mutex<TxState>>) -> Self {
        Self { state }
    }

    /// Block until a post-⌘V read receipt is observed, or `window` elapses.
    ///
    /// `Ok(elapsed)` is the ONLY evidence that the transcript reached the
    /// target app. Polling (rather than a condvar) keeps the AppKit side free
    /// of Rust synchronization it would have to signal from
    /// `provideDataForType:` on the main run loop.
    pub fn await_receipt(&self, window: Duration) -> Result<Duration, NoReceipt> {
        let started = Instant::now();
        loop {
            match self.state.lock() {
                Ok(st) => {
                    if st.any_receipt_after_injection() {
                        return Ok(started.elapsed());
                    }
                    if st.ownership_lost {
                        return Err(NoReceipt {
                            reason: "the clipboard changed before the paste was read".into(),
                            clipboard_taken: true,
                        });
                    }
                }
                Err(_) => {
                    return Err(NoReceipt {
                        reason: "paste transaction state was poisoned".into(),
                        clipboard_taken: false,
                    })
                }
            }
            if started.elapsed() >= window {
                return Err(NoReceipt {
                    reason: format!(
                        "no app read the clipboard within {}ms of ⌘V",
                        window.as_millis()
                    ),
                    clipboard_taken: false,
                });
            }
            thread::sleep(RECEIPT_POLL);
        }
    }
}

/// Test-only transaction stub. A real receipt arrives from an AppKit callback
/// on a live pasteboard, which `cargo test` has no way to drive, so the honesty
/// rule in `paste` is exercised against a hand-built transaction: `receipted`
/// is a target that read the promise after ⌘V, otherwise the chord landed
/// nowhere.
#[cfg(test)]
pub fn stub_handle(receipted: bool) -> PasteHandle {
    let mut state = TxState::new();
    state.injected_at = Some(Instant::now());
    if receipted {
        state.record_receipt(Instant::now());
    }
    PasteHandle::new(Arc::new(Mutex::new(state)))
}

/// Publish the transcript as a lazy pasteboard promise, inject ⌘V, and hand the
/// guarded restore to a background waiter. Returns once the chord has been
/// posted, with a [`PasteHandle`] the caller waits on for the read receipt —
/// the restore itself still completes asynchronously.
///
/// `Err` means the chord never went out, so the transcript was NOT pasted; the
/// caller re-copies it as plain text and reports the failure.
#[cfg(target_os = "macos")]
pub fn publish_and_paste(app: &tauri::AppHandle, text: &str) -> Result<PasteHandle, String> {
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

    let state = rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(|_| "paste timed out waiting for main thread".to_string())??;
    Ok(PasteHandle::new(state))
}

/// Y6-B — copy `text` to the clipboard AFTER settling any paste transaction
/// still holding the pasteboard, on the main thread (where every settle runs),
/// so the copy is the last write and a pending restore can never overwrite it.
/// Blocks until the main thread has done it; never call it from main.
#[cfg(target_os = "macos")]
pub fn copy_sequenced(app: &tauri::AppHandle, text: &str) -> Result<(), String> {
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel();
    let app_main = app.clone();
    let text = text.to_string();
    app.run_on_main_thread(move || {
        let _ = tx.send(mac::settle_then_copy(&app_main, text));
    })
    .map_err(|e| format!("schedule main-thread copy: {e}"))?;
    rx.recv_timeout(Duration::from_secs(3))
        .map_err(|_| "copy timed out waiting for main thread".to_string())?
}

#[cfg(not(target_os = "macos"))]
pub fn copy_sequenced(app: &tauri::AppHandle, text: &str) -> Result<(), String> {
    crate::paste::copy_text(app, text)
}

/// WF-15 — the plain-text pasteboard type every copy declares.
pub const PLAIN_TEXT_TYPE: &str = "public.utf8-plain-text";

// ─── yap-1107-clipboard-write-never-fails-silently ─────────────────────────
//
// Every clipboard write Yap makes goes through [`write_verified_with`]: write,
// read back (changeCount moved + the string compares equal), and retry with
// backoff before anything is reported. Before this, the write path had three
// `let _ = clipboard.write_text(..)` (the settle restore, the left-behind
// transcript, the clear) whose failures vanished, and every other write trusted
// the plugin's `Ok` without looking at the pasteboard afterwards — a second
// writer landing between our write and the target's read (a clipboard manager,
// Universal Clipboard, a script running pbcopy) went unseen.

/// Waits before each retry. The first try runs at once, so a write gets at
/// most `1 + WRITE_BACKOFF_MS.len()` attempts (≈260 ms of waiting in total)
/// before the error is surfaced.
pub const WRITE_BACKOFF_MS: [u64; 3] = [20, 60, 180];

/// One pasteboard the verified writer drives: the real general pasteboard on
/// macOS, the clipboard plugin elsewhere, a fake in the tests.
pub trait PasteboardIo {
    /// Replace the pasteboard's contents with `text` as plain text.
    fn write(&mut self, text: &str) -> Result<(), String>;
    /// The plain-text contents right now (`None` = no string on it).
    fn read(&mut self) -> Option<String>;
    /// The pasteboard's change counter, where the platform has one.
    fn change_count(&mut self) -> Option<i64>;
}

/// Why one attempt did not verify. Logged by name; never carries the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteCause {
    /// The pasteboard refused the write outright (plugin / AppKit said no).
    Refused,
    /// The write returned OK but the changeCount never moved: nothing landed.
    Unchanged,
    /// Read-back found another string: a second writer won the race.
    Clobbered,
    /// Read-back found no string at all (promise owner gone, cleared).
    Empty,
}

impl WriteCause {
    pub fn as_str(self) -> &'static str {
        match self {
            WriteCause::Refused => "refused",
            WriteCause::Unchanged => "unchanged",
            WriteCause::Clobbered => "clobbered",
            WriteCause::Empty => "empty",
        }
    }
}

/// A write that was SEEN on the pasteboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteReceipt {
    /// 1 = first try.
    pub attempts: u32,
    /// The changeCount the verified contents carry (macOS), for a caller that
    /// wants to know later whether the pasteboard still holds this write.
    pub change_count: Option<i64>,
}

/// A write that never verified after every retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteFailure {
    pub attempts: u32,
    pub cause: WriteCause,
    /// The platform's own words for a `Refused` write (log only).
    pub detail: String,
}

impl std::fmt::Display for WriteFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "clipboard write: {} after {} tries",
            self.cause.as_str(),
            self.attempts
        )?;
        if !self.detail.is_empty() {
            write!(f, " ({})", self.detail)?;
        }
        Ok(())
    }
}

/// One attempt: write, then prove it by reading back.
fn attempt_write(
    io: &mut dyn PasteboardIo,
    text: &str,
) -> Result<Option<i64>, (WriteCause, String)> {
    let before = io.change_count();
    io.write(text).map_err(|e| (WriteCause::Refused, e))?;
    let after = io.change_count();
    if let (Some(b), Some(a)) = (before, after) {
        if a == b {
            return Err((WriteCause::Unchanged, String::new()));
        }
    }
    match io.read() {
        Some(got) if got == text => Ok(after),
        Some(_) => Err((WriteCause::Clobbered, String::new())),
        None => Err((WriteCause::Empty, String::new())),
    }
}

/// Write `text`, verify it on the pasteboard, retry with [`WRITE_BACKOFF_MS`].
/// `sleep` is injected so the tests run without waiting. `site` names the
/// caller in the log line ("copy", "restore", "leave_transcript", …).
pub fn write_verified_with(
    io: &mut dyn PasteboardIo,
    text: &str,
    site: &str,
    sleep: &mut dyn FnMut(Duration),
) -> Result<WriteReceipt, WriteFailure> {
    let started = Instant::now();
    let mut attempts: u32 = 0;
    let mut last = (WriteCause::Refused, String::new());
    for wait in std::iter::once(None).chain(WRITE_BACKOFF_MS.iter().map(|ms| Some(*ms))) {
        if let Some(ms) = wait {
            log::warn!(
                "paste_tx clipboard_write retry site={site} attempt={next} cause={cause} wait_ms={ms}",
                next = attempts + 1,
                cause = last.0.as_str(),
            );
            sleep(Duration::from_millis(ms));
        }
        attempts += 1;
        match attempt_write(io, text) {
            Ok(change_count) => {
                if attempts > 1 {
                    log::info!(
                        "paste_tx clipboard_write recovered site={site} attempts={attempts} ms={}",
                        started.elapsed().as_millis()
                    );
                }
                return Ok(WriteReceipt {
                    attempts,
                    change_count,
                });
            }
            Err(cause) => last = cause,
        }
    }
    let failure = WriteFailure {
        attempts,
        cause: last.0,
        detail: last.1,
    };
    log::error!(
        "paste_tx clipboard_write failed site={site} attempts={attempts} cause={} ms={}",
        failure.cause.as_str(),
        started.elapsed().as_millis()
    );
    Err(failure)
}

/// [`write_verified_with`] with a real sleep.
pub fn write_verified(
    io: &mut dyn PasteboardIo,
    text: &str,
    site: &str,
) -> Result<WriteReceipt, WriteFailure> {
    write_verified_with(io, text, site, &mut |d: Duration| std::thread::sleep(d))
}

/// The general pasteboard as plain text — the one every Yap copy lands on.
#[cfg(target_os = "macos")]
pub(crate) fn general_pasteboard() -> mac::GeneralPasteboard {
    mac::GeneralPasteboard
}

/// WF-15 — the nspasteboard.org marker that tells well-behaved clipboard
/// managers (Maccy, Paste, Raycast, …) "this write is transient, do not file
/// it in your history". The Scratchpad's Copy declares it: a whole note is
/// often private and long, and it should not live on in a history list.
pub const TRANSIENT_MARKER: &str = "org.nspasteboard.TransientType";

/// WF-15 — the exact pasteboard types a transient copy declares, in order:
/// the text first (so a reader that takes the first type gets the note), then
/// the marker. Pure, so the contract is tested off-macOS.
pub fn transient_copy_types() -> [&'static str; 2] {
    [PLAIN_TEXT_TYPE, TRANSIENT_MARKER]
}

/// WF-15 — a sequenced copy (see [`copy_sequenced`]) that also carries the
/// [`TRANSIENT_MARKER`]. Blocks until the main thread has done it; never call
/// it from main.
#[cfg(target_os = "macos")]
pub fn copy_transient_sequenced(app: &tauri::AppHandle, text: &str) -> Result<(), String> {
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel();
    let app_main = app.clone();
    let text = text.to_string();
    app.run_on_main_thread(move || {
        let _ = tx.send(mac::settle_then_copy_transient(&app_main, text));
    })
    .map_err(|e| format!("schedule main-thread copy: {e}"))?;
    rx.recv_timeout(Duration::from_secs(3))
        .map_err(|_| "copy timed out waiting for main thread".to_string())?
}

/// Off macOS there is no pasteboard marker convention: a plain copy.
#[cfg(not(target_os = "macos"))]
pub fn copy_transient_sequenced(app: &tauri::AppHandle, text: &str) -> Result<(), String> {
    copy_sequenced(app, text)
}

#[cfg(not(target_os = "macos"))]
pub fn publish_and_paste(_app: &tauri::AppHandle, _text: &str) -> Result<PasteHandle, String> {
    Err("paste only implemented on macOS".into())
}

#[cfg(target_os = "macos")]
mod mac {
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{define_class, msg_send, AnyThread, DefinedClass};
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::{NSArray, NSInteger, NSObject, NSString};
    use tauri::AppHandle;
    use tauri_plugin_clipboard_manager::ClipboardExt;

    use super::{evaluate, PasteboardIo, TxState, WaitDecision};

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
            match &p.saved_text {
                Some(text) => {
                    match super::write_verified(&mut GeneralPasteboard, text, "restore") {
                        Ok(_) => {
                            log::info!("reliable-paste: read confirmed — prior clipboard restored")
                        }
                        // The transcript stays on the clipboard (the take already
                        // landed); the user's prior copy is what we failed to put
                        // back, so say so in the log rather than claim a restore.
                        Err(e) => diag_warn!(
                            "paste_tx",
                            "reliable-paste: read confirmed — prior clipboard NOT restored: {err}",
                            err = e
                        ),
                    }
                }
                None => {
                    if let Err(e) = app.clipboard().clear() {
                        diag_warn!(
                            "paste_tx",
                            "reliable-paste: read confirmed — clipboard clear failed: {err}",
                            err = e
                        );
                    } else {
                        log::info!(
                            "reliable-paste: read confirmed — prior (empty) clipboard restored"
                        );
                    }
                }
            }
        } else {
            // Nobody read it, so the ⌘V never landed. Never lose text: replace
            // the dead promise with the transcript as plain text so the user
            // can still ⌘V it themselves.
            match super::write_verified(&mut GeneralPasteboard, &p.transcript, "leave_transcript") {
                Ok(_) => diag_warn!(
                    "paste_tx",
                    "reliable-paste: no clipboard read within timeout — left the transcript on the clipboard"
                ),
                Err(e) => diag_error!(
                    "paste_tx",
                    "reliable-paste: no clipboard read within timeout and the transcript could not be left on the clipboard: {err}",
                    err = e
                ),
            }
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

    /// Y6-B — main-thread half of `copy_sequenced`: settle whatever paste is
    /// still open (its restore, if any, happens NOW), then write the copy.
    pub fn settle_then_copy(app: &AppHandle, text: String) -> Result<(), String> {
        flush_pending(app);
        super::write_verified(&mut GeneralPasteboard, &text, "copy_sequenced")
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// WF-15 — main-thread half of `copy_transient_sequenced`: settle any open
    /// paste, then put the text on the general pasteboard as real data (no
    /// owner, no promise) together with the transient marker.
    pub fn settle_then_copy_transient(app: &AppHandle, text: String) -> Result<(), String> {
        // A pending paste restore must not land after (and clobber) this copy.
        flush_pending(app);
        super::write_verified(&mut TransientPasteboard, &text, "copy_transient")
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// The general pasteboard as plain text (no owner, no promise). Every
    /// method is a plain AppKit call; `NSPasteboard` may be driven off the main
    /// thread for data that is already on it (no promise involved).
    pub struct GeneralPasteboard;

    impl PasteboardIo for GeneralPasteboard {
        fn write(&mut self, text: &str) -> Result<(), String> {
            let pasteboard = NSPasteboard::generalPasteboard();
            pasteboard.clearContents();
            let body = NSString::from_str(text);
            // SAFETY: NSPasteboardTypeString is a read-only framework static.
            let wrote = pasteboard.setString_forType(&body, unsafe { NSPasteboardTypeString });
            if wrote {
                Ok(())
            } else {
                Err("setString:forType: returned NO".into())
            }
        }
        fn read(&mut self) -> Option<String> {
            // SAFETY: as above.
            NSPasteboard::generalPasteboard()
                .stringForType(unsafe { NSPasteboardTypeString })
                .map(|s| s.to_string())
        }
        fn change_count(&mut self) -> Option<i64> {
            Some(NSPasteboard::generalPasteboard().changeCount() as i64)
        }
    }

    /// WF-15 — the general pasteboard with the transient marker declared
    /// beside the text, so clipboard managers skip the note.
    pub struct TransientPasteboard;

    impl PasteboardIo for TransientPasteboard {
        fn write(&mut self, text: &str) -> Result<(), String> {
            let pasteboard = NSPasteboard::generalPasteboard();
            let names: Vec<Retained<NSString>> = super::transient_copy_types()
                .iter()
                .map(|t| NSString::from_str(t))
                .collect();
            let types = NSArray::from_retained_slice(&names);
            let no_owner: Option<&AnyObject> = None;
            let change_count: NSInteger =
                unsafe { msg_send![&*pasteboard, declareTypes: &*types, owner: no_owner] };
            if change_count <= 0 {
                return Err("declareTypes:owner: failed".into());
            }
            let body = NSString::from_str(text);
            let wrote: bool =
                unsafe { msg_send![&*pasteboard, setString: &*body, forType: &*names[0]] };
            if !wrote {
                return Err("the pasteboard refused the note text".into());
            }
            // The marker's presence is the signal; its payload is empty.
            let empty = NSString::from_str("");
            for marker in &names[1..] {
                let marked: bool =
                    unsafe { msg_send![&*pasteboard, setString: &*empty, forType: &**marker] };
                if !marked {
                    log::warn!("paste_tx clipboard_write marker_refused site=copy_transient");
                }
            }
            Ok(())
        }
        fn read(&mut self) -> Option<String> {
            GeneralPasteboard.read()
        }
        fn change_count(&mut self) -> Option<i64> {
            GeneralPasteboard.change_count()
        }
    }

    /// Main-thread half: publish the promise, post ⌘V, arm the waiter. The
    /// transaction state travels back to the caller (YV74) so it can wait for
    /// the read receipt before claiming the paste landed.
    pub fn publish_and_chord(app: &AppHandle, text: String) -> Result<Arc<Mutex<TxState>>, String> {
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
            diag_error!(
                "paste_tx",
                "reliable-paste: ⌘V injection failed: {err}",
                err = e
            );
        }

        let pending = Arc::new(Mutex::new(Pending {
            state: state.clone(),
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

        chord.map(|()| state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_copy_declares_text_first_then_the_transient_marker() {
        let types = transient_copy_types();
        assert_eq!(
            types[0], "public.utf8-plain-text",
            "the note text is the first type"
        );
        assert_eq!(types[1], "org.nspasteboard.TransientType");
        assert!(
            types.contains(&TRANSIENT_MARKER),
            "clipboard managers skip only what carries the marker"
        );
    }

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

    /// YV73 — a target that hammers the pasteboard promise must not grow the
    /// transaction without bound, and capping it must not cost the decision the
    /// newest receipt is what drives.
    #[test]
    fn receipts_are_capped_keeping_the_newest() {
        let injected = Instant::now();
        let mut s = published_ago(Duration::from_millis(10));
        s.injected_at = Some(injected);
        for i in 1..=(MAX_RECEIPTS * 4) {
            s.record_receipt(injected + Duration::from_millis(i as u64));
        }
        assert_eq!(s.receipts.len(), MAX_RECEIPTS, "receipts are bounded");
        assert_eq!(
            s.last_receipt_after_injection(),
            Some(injected + Duration::from_millis((MAX_RECEIPTS * 4) as u64)),
            "the newest receipt still decides the quiet period"
        );
    }

    // ─── yap-1107-clipboard-write-never-fails-silently ─────────────────────

    /// A scripted pasteboard: each write consumes one scripted behaviour.
    struct FakeBoard {
        contents: Option<String>,
        count: i64,
        script: Vec<FakeWrite>,
        writes: u32,
    }

    #[derive(Clone, Copy)]
    enum FakeWrite {
        Lands,
        Refused,
        /// Returns OK but nothing changes (changeCount stays put).
        Ignored,
        /// Lands, then a second writer replaces it before the read-back.
        Clobbered,
    }

    impl FakeBoard {
        fn new(script: Vec<FakeWrite>) -> Self {
            Self {
                contents: Some("prior".into()),
                count: 7,
                script,
                writes: 0,
            }
        }
    }

    impl PasteboardIo for FakeBoard {
        fn write(&mut self, text: &str) -> Result<(), String> {
            let step = self
                .script
                .get(self.writes as usize)
                .copied()
                .unwrap_or(FakeWrite::Lands);
            self.writes += 1;
            match step {
                FakeWrite::Lands => {
                    self.count += 1;
                    self.contents = Some(text.to_string());
                    Ok(())
                }
                FakeWrite::Refused => Err("busy".into()),
                FakeWrite::Ignored => Ok(()),
                FakeWrite::Clobbered => {
                    self.count += 2;
                    self.contents = Some("someone else".into());
                    Ok(())
                }
            }
        }
        fn read(&mut self) -> Option<String> {
            self.contents.clone()
        }
        fn change_count(&mut self) -> Option<i64> {
            Some(self.count)
        }
    }

    #[test]
    fn verified_write_lands_on_the_first_try_without_sleeping() {
        let mut board = FakeBoard::new(vec![FakeWrite::Lands]);
        let mut slept = Vec::new();
        let r = write_verified_with(&mut board, "hello", "test", &mut |d: Duration| {
            slept.push(d)
        })
        .expect("a landing write verifies");
        assert_eq!(r.attempts, 1);
        assert_eq!(r.change_count, Some(8));
        assert!(slept.is_empty());
    }

    /// Acceptance (2): a write another process clobbers is retried with the
    /// backoff and verified on a later try — never reported as copied early.
    #[test]
    fn verified_write_retries_a_clobbered_write_with_backoff() {
        let mut board = FakeBoard::new(vec![
            FakeWrite::Clobbered,
            FakeWrite::Refused,
            FakeWrite::Lands,
        ]);
        let mut slept = Vec::new();
        let r = write_verified_with(&mut board, "hello", "test", &mut |d: Duration| {
            slept.push(d)
        })
        .expect("third try lands");
        assert_eq!(r.attempts, 3);
        assert_eq!(
            slept,
            vec![Duration::from_millis(20), Duration::from_millis(60)]
        );
        assert_eq!(board.contents.as_deref(), Some("hello"));
    }

    /// Acceptance (2)+(4): the error surfaces only after every retry, with the
    /// cause of the last attempt — never a silent `Ok`.
    #[test]
    fn verified_write_reports_failure_only_after_every_retry() {
        let all = 1 + WRITE_BACKOFF_MS.len();
        for (step, cause) in [
            (FakeWrite::Clobbered, WriteCause::Clobbered),
            (FakeWrite::Refused, WriteCause::Refused),
            (FakeWrite::Ignored, WriteCause::Unchanged),
        ] {
            let mut board = FakeBoard::new(vec![step; all]);
            let mut slept = Vec::new();
            let e = write_verified_with(&mut board, "hello", "test", &mut |d: Duration| {
                slept.push(d)
            })
            .expect_err("never verified");
            assert_eq!(e.attempts as usize, all);
            assert_eq!(e.cause, cause);
            assert_eq!(board.writes as usize, all, "every retry really wrote");
            assert_eq!(
                slept,
                WRITE_BACKOFF_MS
                    .iter()
                    .map(|ms| Duration::from_millis(*ms))
                    .collect::<Vec<_>>()
            );
            assert!(e.to_string().starts_with("clipboard write: "));
        }
    }

    /// An empty read-back (promise owner gone, pasteboard cleared) is a
    /// failure, not a success.
    #[test]
    fn verified_write_treats_an_empty_read_back_as_a_failure() {
        struct Vanishing;
        impl PasteboardIo for Vanishing {
            fn write(&mut self, _text: &str) -> Result<(), String> {
                Ok(())
            }
            fn read(&mut self) -> Option<String> {
                None
            }
            fn change_count(&mut self) -> Option<i64> {
                None
            }
        }
        let e = write_verified_with(&mut Vanishing, "hello", "test", &mut |_| {})
            .expect_err("nothing on the pasteboard");
        assert_eq!(e.cause, WriteCause::Empty);
    }

    /// Acceptance (3) — 200 Yap writes through the REAL general pasteboard
    /// while a second process hammers it with pbcopy. Every write must end
    /// either verified (and, if nobody wrote since, still on the pasteboard)
    /// or reported as failed: zero silent failures. Touches the user's real
    /// clipboard, so it is ignored by default; the integrator runs it:
    /// `cargo test --lib paste_tx::tests::clipboard_contention_200_writes -- --ignored --nocapture`
    /// and the results land in docs/loop/evidence/clipboard-write/.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn clipboard_contention_200_writes() {
        use std::io::Write as _;
        use std::process::{Command, Stdio};

        let mut board = general_pasteboard();
        let saved = board.read();
        let mut hammer = Command::new("/bin/sh")
            .arg("-c")
            .arg(
                "i=0; while :; do i=$((i+1)); printf 'contender-%s' \"$i\" | /usr/bin/pbcopy; done",
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the pbcopy contender");
        std::thread::sleep(Duration::from_millis(100));

        let (mut verified, mut failed, mut silent, mut retried, mut overwritten_later) =
            (0u32, 0u32, 0u32, 0u32, 0u32);
        let mut causes: std::collections::BTreeMap<&'static str, u32> = Default::default();
        let started = Instant::now();
        for i in 0..200 {
            let text = format!("yap-contention-{i}");
            match write_verified(&mut board, &text, "contention_test") {
                Ok(r) => {
                    verified += 1;
                    if r.attempts > 1 {
                        retried += 1;
                    }
                    // Independent check: if nothing wrote since the verified
                    // changeCount, the pasteboard must still hold OUR text.
                    let now = board.change_count();
                    let got = board.read();
                    if now == r.change_count {
                        if got.as_deref() != Some(text.as_str()) {
                            silent += 1;
                        }
                    } else {
                        overwritten_later += 1;
                    }
                }
                Err(e) => {
                    failed += 1;
                    *causes.entry(e.cause.as_str()).or_default() += 1;
                }
            }
        }
        let elapsed_ms = started.elapsed().as_millis();
        // Our own child, by handle — never a pattern kill.
        let _ = hammer.kill();
        let _ = hammer.wait();
        if let Some(prior) = saved {
            let _ = write_verified(&mut board, &prior, "contention_test_restore");
        }

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/loop/evidence/clipboard-write");
        std::fs::create_dir_all(&dir).expect("evidence dir");
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let body = serde_json::json!({
            "test": "clipboard_contention_200_writes",
            "unix_time": stamp,
            "writes": 200,
            "verified": verified,
            "failed_reported": failed,
            "silent_failures": silent,
            "verified_after_retry": retried,
            "overwritten_by_contender_after_verify": overwritten_later,
            "failure_causes": causes,
            "backoff_ms": WRITE_BACKOFF_MS,
            "elapsed_ms": elapsed_ms as u64,
        });
        let path = dir.join(format!("contention-{stamp}.json"));
        let mut f = std::fs::File::create(&path).expect("evidence file");
        writeln!(f, "{}", serde_json::to_string_pretty(&body).unwrap()).unwrap();
        println!("{body}");

        assert_eq!(verified + failed, 200, "every write is accounted for");
        assert_eq!(silent, 0, "zero silent failures");
    }
}
