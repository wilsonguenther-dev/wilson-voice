//! Y6-C — the paste lands in the app you dictated into, or it does not paste.
//!
//! One end-to-end test of the whole insertion chain (`paste::insert_traced` /
//! `paste::insert_serialized`: wrong-target guard → secure-field refusal → AX →
//! receipt-checked ⌘V bridge → Copy card) against a counting fake backend.
//! `posts` counts every CGEventPost the live backend would make, `clipboard`
//! models the general pasteboard (it starts holding the USER's own copy), and
//! `frontmost` / `secure` are read by the decision when IT asks — so every
//! answer below is the one sampled at PASTE time, never one cached at the hold.
//!
//! What is proven here vs. by hand (headless limits):
//! * Proven by test: the decision for every case below, through the same
//!   `insert_traced` the live `LiveInsert` backend runs.
//! * NOT proven here: that macOS really reports a password field as
//!   `AXSecureTextField` / `IsSecureEventInputEnabled()` (focus.rs /
//!   secure_input.rs live reads), that a real Electron app drops a synthesized
//!   ⌘V, and how the pill renders the held card. Those are live-app checks.
//!
//! THE ORPHANED-TAKE ANSWER (PANEL 2026-09-12, not a builder's choice): a take
//! whose app moved between the hold and the paste is HELD AND OFFERED — it
//! parks on the pill with one Insert button that puts it wherever the user is
//! now (and it is in History). It is never typed into the new app, never
//! dropped, and never clipboard-only behind a toast.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use wilson_voice_lib::paste::{
    self, AxInsert, BridgeReceipt, CopyCard, CopyCardReason, InsertBackend, InsertFallback,
    InsertPath, PasteTarget,
};

const NOTES: &str = "com.apple.Notes";
const SLACK: &str = "com.tinyspeck.slackmacgap";
const FINDER: &str = "com.apple.finder";
const USERS_OWN_COPY: &str = "the user's own clipboard";
const TAKE: &str = "ship the bridge fix today";

struct Fake {
    /// Every plain-text clipboard write Yap makes.
    copies: Vec<String>,
    /// The general pasteboard as the user would see it.
    clipboard: String,
    ax_calls: usize,
    /// CGEventPost count: the paste bridge's ⌘V.
    posts: usize,
    cards: Vec<CopyCard>,
    /// The frontmost app at the moment the decision asks.
    frontmost: Option<String>,
    frontmost_calls: usize,
    electron: bool,
    /// Is the focused element a password field at the moment the decision asks.
    secure: bool,
    secure_calls: usize,
    ax_result: AxInsert,
    bridge_result: Result<BridgeReceipt, String>,
    /// For the interleave test: a shared timeline, a name, a hold time inside
    /// the bridge, and a signal sent the instant the bridge starts.
    log: Option<Arc<Mutex<Vec<String>>>>,
    name: &'static str,
    bridge_hold: Duration,
    on_bridge: Option<mpsc::Sender<()>>,
}

impl Fake {
    fn new(frontmost: &str) -> Self {
        Fake {
            copies: vec![],
            clipboard: USERS_OWN_COPY.into(),
            ax_calls: 0,
            posts: 0,
            cards: vec![],
            frontmost: Some(frontmost.into()),
            frontmost_calls: 0,
            electron: false,
            secure: false,
            secure_calls: 0,
            ax_result: AxInsert::Refused("not settable".into()),
            bridge_result: Ok(BridgeReceipt::Confirmed { after_ms: 14 }),
            log: None,
            name: "",
            bridge_hold: Duration::ZERO,
            on_bridge: None,
        }
    }
    fn keystrokes(&self) -> usize {
        self.posts + self.ax_calls
    }
}

impl InsertBackend for Fake {
    fn copy(&mut self, text: &str) -> Result<(), String> {
        self.copies.push(text.to_string());
        self.clipboard = text.to_string();
        Ok(())
    }
    fn frontmost_app(&mut self) -> Option<String> {
        self.frontmost_calls += 1;
        self.frontmost.clone()
    }
    fn frontmost_has_electron(&mut self) -> bool {
        self.electron
    }
    fn target_is_secure(&mut self) -> bool {
        self.secure_calls += 1;
        self.secure
    }
    fn ax_insert(&mut self, _text: &str) -> AxInsert {
        self.ax_calls += 1;
        self.ax_result.clone()
    }
    fn paste_bridge(&mut self, text: &str) -> Result<BridgeReceipt, String> {
        if let Some(log) = &self.log {
            log.lock().unwrap().push(format!("{} begin", self.name));
        }
        if let Some(tx) = self.on_bridge.take() {
            let _ = tx.send(());
        }
        // The live bridge publishes the text as a pasteboard promise.
        self.clipboard = text.to_string();
        self.posts += 1;
        thread::sleep(self.bridge_hold);
        if let Some(log) = &self.log {
            log.lock().unwrap().push(format!("{} end", self.name));
        }
        self.bridge_result.clone()
    }
    fn post_delete(&mut self) -> Result<(), String> {
        self.posts += 1;
        Ok(())
    }
    fn show_copy_card(&mut self, card: CopyCard) {
        self.cards.push(card);
    }
}

/// The user cmd-tabs to Slack while Yap is transcribing a take held in Notes.
/// Decision: HELD (pill card + clipboard), zero keystrokes into Slack.
#[test]
fn focus_changed_between_hold_and_paste_does_not_paste_to_the_new_app() {
    for path in [InsertPath::Ax, InsertPath::PasteBridge] {
        let mut f = Fake::new(SLACK);
        let (out, trace) =
            paste::insert_traced(&mut f, path, TAKE, true, PasteTarget::Dictated(Some(NOTES)));
        println!(
            "{path:?}: keystrokes={} pasted={} card={:?} fallback={}",
            f.keystrokes(),
            out.pasted,
            f.cards.first().map(|c| c.reason),
            trace.fallback.as_str()
        );
        assert_eq!(
            f.posts, 0,
            "{path:?}: no ⌘V into the app the user switched to"
        );
        assert_eq!(f.ax_calls, 0, "{path:?}: no AX insert into it either");
        assert!(!out.pasted);
        assert_eq!(trace.fallback, InsertFallback::WrongTarget);
        assert_eq!(trace.app.as_deref(), Some(NOTES));
        assert_eq!(f.cards.len(), 1, "{path:?}: the take is held on the pill");
        assert_eq!(f.cards[0].reason, CopyCardReason::TargetMoved);
        assert_eq!(f.cards[0].text, TAKE);
        assert_eq!(out.message, paste::TARGET_MOVED_MESSAGE);
    }
}

/// PANEL 2026-09-12: a long take (Y3) whose app moved while it decoded is held
/// with the WHOLE text, says so, and one Insert (the pill's button →
/// `paste_entry` → an explicit re-paste) puts it wherever the user is now.
#[test]
fn a_long_take_whose_target_moved_is_held_and_insertable() {
    let twelve_minutes: String = "and then we talked about the launch plan ".repeat(400);
    let mut f = Fake::new(SLACK);
    let out = paste::insert_with(
        &mut f,
        InsertPath::PasteBridge,
        &twelve_minutes,
        true,
        PasteTarget::Dictated(Some(NOTES)),
    );
    assert_eq!(
        f.keystrokes(),
        0,
        "never typed into the app it was not held for"
    );
    assert_eq!(f.cards.len(), 1, "held, not dropped");
    let card = f.cards[0].clone();
    assert_eq!(card.reason, CopyCardReason::TargetMoved);
    assert_eq!(card.text, twelve_minutes, "the whole take, not a preview");
    assert!(card.copied, "also on the clipboard, so a manual ⌘V works");
    assert_eq!(
        card.repair_service, None,
        "nothing to repair: a grant works"
    );
    assert!(card.blocked_permissions.is_empty());
    assert!(out.message.contains("held in the pill"));
    assert!(out.message.contains("Insert"));
    // ...and is never silently clipboard-only: the outcome says it was not pasted.
    assert!(!out.pasted);

    // Insert: the user is in Slack now and presses the card's one button.
    let mut now = Fake::new(SLACK);
    let inserted = paste::insert_with(
        &mut now,
        InsertPath::PasteBridge,
        &card.text,
        true,
        PasteTarget::Explicit,
    );
    assert_eq!(now.posts, 1, "Insert puts it where the user is now");
    assert!(inserted.pasted);
    assert!(now.cards.is_empty());

    // An unreadable target identity is held the same way, never typed.
    let mut f = Fake::new(SLACK);
    let out = paste::insert_with(
        &mut f,
        InsertPath::PasteBridge,
        TAKE,
        true,
        PasteTarget::Dictated(None),
    );
    assert_eq!(f.keystrokes(), 0);
    assert_eq!(f.cards[0].reason, CopyCardReason::TargetMoved);
    assert_eq!(out.message, paste::UNKNOWN_TARGET_MESSAGE);
}

/// The app the take was held in QUITS before the paste: either nothing is
/// frontmost, or macOS promotes the next app (Finder). Held either way.
#[test]
fn focused_app_quit_between_hold_and_paste_holds_the_take() {
    for after_quit in [None, Some(FINDER)] {
        let mut f = Fake::new(NOTES);
        f.frontmost = after_quit.map(str::to_string);
        let (out, trace) = paste::insert_traced(
            &mut f,
            InsertPath::PasteBridge,
            TAKE,
            true,
            PasteTarget::Dictated(Some(NOTES)),
        );
        assert_eq!(f.keystrokes(), 0, "{after_quit:?}: nothing typed");
        assert!(!out.pasted);
        assert_eq!(trace.fallback, InsertFallback::WrongTarget);
        assert_eq!(f.cards.len(), 1, "{after_quit:?}: held on the pill");
        assert_eq!(f.cards[0].reason, CopyCardReason::TargetMoved);
        assert_eq!(f.copies, vec![TAKE.to_string()]);
    }
}

/// The target is a password field. Yap refuses, says why, and the dictation is
/// NOT left on the clipboard — the user's own copy is still there. This holds
/// for the copy-only paths too (auto-paste off, a stale take), because the
/// harm being prevented is the clipboard residue itself.
#[test]
fn secure_input_refuses_and_leaves_no_clipboard_residue() {
    for (path, want_paste) in [
        (InsertPath::Ax, true),
        (InsertPath::PasteBridge, true),
        (InsertPath::CopyCard, true),
        (InsertPath::PasteBridge, false),
    ] {
        let mut f = Fake::new(NOTES);
        f.secure = true;
        let (out, trace) = paste::insert_traced(
            &mut f,
            path,
            TAKE,
            want_paste,
            PasteTarget::Dictated(Some(NOTES)),
        );
        println!(
            "{path:?} want_paste={want_paste}: keystrokes={} copies={} clipboard={:?} msg={:?}",
            f.keystrokes(),
            f.copies.len(),
            f.clipboard,
            out.message
        );
        assert_eq!(
            f.keystrokes(),
            0,
            "{path:?}: no paste into a password field"
        );
        assert!(f.copies.is_empty(), "{path:?}: no clipboard write at all");
        assert_eq!(f.clipboard, USERS_OWN_COPY, "{path:?}: no residue");
        assert!(!out.pasted);
        assert!(!out.copied);
        assert_eq!(out.message, paste::SECURE_FIELD_MESSAGE, "it says why");
        assert_eq!(trace.fallback, InsertFallback::SecureField);
        assert_eq!(f.cards.len(), 1, "held on the pill, not lost");
        assert_eq!(f.cards[0].reason, CopyCardReason::SecureField);
        assert!(
            !f.cards[0].copied,
            "the card says it is NOT on the clipboard"
        );
    }

    // The live rule: the focused element's own AX answer wins; macOS's global
    // Secure Input flag decides only when AX is blind.
    assert!(paste::secure_target(Some(true), false));
    assert!(paste::secure_target(Some(true), true));
    assert!(!paste::secure_target(Some(false), true));
    assert!(!paste::secure_target(Some(false), false));
    assert!(paste::secure_target(None, true));
    assert!(!paste::secure_target(None, false));
}

/// Accessibility works, but the target (some Electron / Java apps) ignores a
/// synthesized ⌘V: no receipt. Clipboard fallback WITH an explicit message and
/// the Copy card — never a silent `pasted=false`.
#[test]
fn target_that_refuses_synthesized_cmd_v_falls_back_to_the_clipboard_and_says_so() {
    for path in [InsertPath::Ax, InsertPath::PasteBridge] {
        let mut f = Fake::new("com.example.some-electron-app");
        f.electron = true;
        f.bridge_result = Ok(BridgeReceipt::Missing {
            reason: "no clipboard read within 1500ms of ⌘V".into(),
            clipboard_taken: false,
        });
        let (out, trace) = paste::insert_traced(
            &mut f,
            path,
            TAKE,
            true,
            PasteTarget::Dictated(Some("com.example.some-electron-app")),
        );
        assert_eq!(f.ax_calls, 0, "{path:?}: Electron skips the AX set");
        assert_eq!(f.posts, 1, "{path:?}: one ⌘V, never a retry storm");
        assert!(!out.pasted);
        assert!(out.copied);
        assert_eq!(
            f.clipboard, TAKE,
            "the fallback leaves the text on the clipboard"
        );
        assert!(out.message.contains("didn't land"), "{}", out.message);
        assert!(out.message.contains("clipboard"), "{}", out.message);
        assert_eq!(trace.fallback, InsertFallback::NoReceipt);
        assert_eq!(f.cards.len(), 1);
        assert_eq!(f.cards[0].reason, CopyCardReason::NoReceipt);
    }
}

/// `auto_paste: false` (lib.rs Settings): the text goes to the clipboard and the
/// outcome says so; zero keystrokes of any kind.
#[test]
fn auto_paste_false_synthesizes_no_keystroke() {
    let want_paste = paste::should_paste(false, 3, 3, true);
    assert!(!want_paste);
    for path in [
        InsertPath::Ax,
        InsertPath::PasteBridge,
        InsertPath::CopyCard,
    ] {
        let mut f = Fake::new(NOTES);
        let out = paste::insert_with(
            &mut f,
            path,
            TAKE,
            want_paste,
            PasteTarget::Dictated(Some(NOTES)),
        );
        assert_eq!(f.posts, 0, "{path:?}: no ⌘V");
        assert_eq!(f.ax_calls, 0, "{path:?}: no AX set");
        assert!(
            f.cards.is_empty(),
            "{path:?}: no card for a deliberate setting"
        );
        assert_eq!(f.copies, vec![TAKE.to_string()]);
        assert!(out.copied && !out.pasted);
        assert_eq!(out.message, "Copied to clipboard (⌘V to paste)");
    }
}

/// A long take: the hold was minutes ago. The decision must ask the backend for
/// the frontmost app AND the secure-field state at PASTE time — the fake only
/// answers what is true when it is asked.
#[test]
fn target_is_rechecked_at_paste_time_for_a_long_take() {
    // Hold: Notes is frontmost, a plain text field. Yap captures the identity.
    let mut f = Fake::new(NOTES);
    let held_for = f.frontmost_app();
    assert_eq!(held_for.as_deref(), Some(NOTES));
    let calls_at_hold = f.frontmost_calls;

    // Minutes pass: Slack, then back to Notes — but now a password field.
    f.frontmost = Some(SLACK.into());
    f.frontmost = Some(NOTES.into());
    f.secure = true;
    let (out, trace) = paste::insert_traced(
        &mut f,
        InsertPath::PasteBridge,
        TAKE,
        true,
        PasteTarget::Dictated(held_for.as_deref()),
    );
    assert_eq!(f.secure_calls, 1, "secure state read at paste time");
    assert_eq!(trace.fallback, InsertFallback::SecureField);
    assert_eq!(f.keystrokes(), 0);
    assert!(!out.pasted);

    // Same hold, the user ends in Slack: the paste-time frontmost decides.
    let mut f = Fake::new(NOTES);
    let held_for = f.frontmost_app();
    f.frontmost = Some(SLACK.into());
    let (_, trace) = paste::insert_traced(
        &mut f,
        InsertPath::PasteBridge,
        TAKE,
        true,
        PasteTarget::Dictated(held_for.as_deref()),
    );
    assert!(
        f.frontmost_calls > calls_at_hold,
        "frontmost re-read at paste time"
    );
    assert_eq!(trace.fallback, InsertFallback::WrongTarget);
    assert_eq!(f.posts, 0);

    // Same hold, the user wandered off and came BACK to Notes, plain field:
    // the paste-time answer is "same app", so it pastes.
    let mut f = Fake::new(NOTES);
    let held_for = f.frontmost_app();
    f.frontmost = Some(SLACK.into());
    f.frontmost = Some(NOTES.into());
    let (out, _) = paste::insert_traced(
        &mut f,
        InsertPath::PasteBridge,
        TAKE,
        true,
        PasteTarget::Dictated(held_for.as_deref()),
    );
    assert_eq!(f.frontmost_calls, 2, "one read at hold, one at paste");
    assert_eq!(f.posts, 1);
    assert!(out.pasted);
}

/// Two takes in quick succession: take B finishes while take A is still waiting
/// on its paste receipt. B's whole insertion waits for A's to end — the two
/// publish → ⌘V → receipt transactions never interleave.
#[test]
fn two_takes_cannot_interleave_their_pastes() {
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let (a_in_bridge_tx, a_in_bridge_rx) = mpsc::channel();

    let mut a = Fake::new(NOTES);
    a.log = Some(log.clone());
    a.name = "A";
    a.bridge_hold = Duration::from_millis(150);
    a.on_bridge = Some(a_in_bridge_tx);
    let ta = thread::spawn(move || {
        let (out, _) = paste::insert_serialized(
            &mut a,
            InsertPath::PasteBridge,
            "take A",
            true,
            PasteTarget::Dictated(Some(NOTES)),
        );
        out.pasted
    });

    // B starts only once A is INSIDE its bridge (published, ⌘V out, waiting).
    a_in_bridge_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("take A reached the bridge");
    let mut b = Fake::new(NOTES);
    b.log = Some(log.clone());
    b.name = "B";
    let tb = thread::spawn(move || {
        let (out, _) = paste::insert_serialized(
            &mut b,
            InsertPath::PasteBridge,
            "take B",
            true,
            PasteTarget::Dictated(Some(NOTES)),
        );
        out.pasted
    });

    assert!(ta.join().unwrap());
    assert!(tb.join().unwrap());
    let timeline = log.lock().unwrap().clone();
    println!("timeline: {timeline:?}");
    assert_eq!(
        timeline,
        vec!["A begin", "A end", "B begin", "B end"],
        "B's paste may not start while A's is in flight, and order is arrival order"
    );
}
