//! yap-1106-paste-target-defect — the paste target is the app frontmost at
//! RELEASE, re-verified at paste time; never the key-down app when the release
//! read is good, never Yap's own float panel; the history row (`source_app`)
//! names the same app the paste went to.
//!
//! Headless: a fake [`InsertBackend`] stands in for macOS and records which app
//! was in front when each ⌘V was posted, so "the text landed in Chrome" is an
//! observed fact of the fake, not an assumption.

use wilson_voice_lib::focus::{
    is_own_app, resolve_paste_target, AppIdentity, CapturedAt, ResolvedTarget, YAP_BUNDLE_PREFIX,
};
use wilson_voice_lib::paste::{
    self, AxInsert, BridgeReceipt, CopyCard, InsertBackend, InsertFallback, InsertPath,
    PasteOutcome, PasteTarget,
};

const CHROME: &str = "com.google.Chrome";
const TERMINAL: &str = "com.apple.Terminal";
const YAP: &str = "com.wilsonguenther.wilson-voice";
const TAKE: &str = "fake transcript for the paste target test";

fn chrome() -> AppIdentity {
    AppIdentity::new(Some(CHROME), Some("Google Chrome"))
}
fn terminal() -> AppIdentity {
    AppIdentity::new(Some(TERMINAL), Some("Terminal"))
}
fn yap_panel() -> AppIdentity {
    AppIdentity::new(Some(YAP), Some("Yap"))
}

/// A Mac with one frontmost app and a text field that receives every ⌘V.
struct FakeMac {
    frontmost: Option<String>,
    copies: Vec<String>,
    /// (app in front when ⌘V was posted, text) — where each paste LANDED.
    landed: Vec<(Option<String>, String)>,
    cards: usize,
}

impl FakeMac {
    fn with_front(bundle: &str) -> Self {
        FakeMac {
            frontmost: Some(bundle.to_string()),
            copies: vec![],
            landed: vec![],
            cards: 0,
        }
    }
}

impl InsertBackend for FakeMac {
    fn copy(&mut self, text: &str) -> Result<(), String> {
        self.copies.push(text.to_string());
        Ok(())
    }
    fn frontmost_app(&mut self) -> Option<String> {
        self.frontmost.clone()
    }
    fn ax_insert(&mut self, _text: &str) -> AxInsert {
        AxInsert::Refused("not used on the paste-bridge path".into())
    }
    fn paste_bridge(&mut self, text: &str) -> Result<BridgeReceipt, String> {
        self.landed.push((self.frontmost.clone(), text.to_string()));
        Ok(BridgeReceipt::Confirmed { after_ms: 9 })
    }
    fn post_delete(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn show_copy_card(&mut self, _card: CopyCard) {
        self.cards += 1;
    }
}

/// One dictated take end to end, headless: resolve the target from the
/// press/release reads, then insert with the app `front_at_paste` in front.
fn dictate(
    press: Option<AppIdentity>,
    release: Option<AppIdentity>,
    front_at_paste: &str,
) -> (ResolvedTarget, PasteOutcome, FakeMac, Option<String>) {
    let target = resolve_paste_target(press.as_ref(), release.as_ref(), Some(YAP));
    let mut mac = FakeMac::with_front(front_at_paste);
    let (outcome, trace) = paste::insert_traced(
        &mut mac,
        InsertPath::PasteBridge,
        TAKE,
        true,
        PasteTarget::Dictated(target.bundle_id.as_deref()),
    );
    let line = paste::paste_target_log_line(
        PasteTarget::Dictated(target.bundle_id.as_deref()),
        target.captured_at,
        true,
        &outcome,
        &trace,
    );
    (target, outcome, mac, line)
}

/// Wilson's 2026-11-06 report: Terminal was in front at key-down, Chrome at
/// release. The text lands in CHROME and the history row says Chrome.
#[test]
fn dictating_into_a_chrome_field_lands_in_chrome_not_the_key_down_app() {
    let (target, outcome, mac, line) = dictate(Some(terminal()), Some(chrome()), CHROME);
    assert_eq!(target.bundle_id.as_deref(), Some(CHROME));
    assert_eq!(target.captured_at, CapturedAt::Release);
    assert_eq!(
        target.name.as_deref(),
        Some("Google Chrome"),
        "the history row (source_app) records the app the take was for"
    );
    assert!(outcome.pasted, "{}", outcome.message);
    assert_eq!(
        mac.landed,
        vec![(Some(CHROME.to_string()), TAKE.to_string())],
        "the text landed in Chrome, exactly once"
    );
    assert_eq!(
        line.as_deref(),
        Some("paste target app=com.google.Chrome captured_at=release result=pasted")
    );
}

/// The mirror case: a Terminal pane in front at release gets the text.
#[test]
fn dictating_into_a_terminal_pane_lands_in_terminal() {
    let (target, outcome, mac, line) = dictate(Some(chrome()), Some(terminal()), TERMINAL);
    assert_eq!(target.bundle_id.as_deref(), Some(TERMINAL));
    assert_eq!(target.name.as_deref(), Some("Terminal"));
    assert!(outcome.pasted, "{}", outcome.message);
    assert_eq!(
        mac.landed,
        vec![(Some(TERMINAL.to_string()), TAKE.to_string())]
    );
    assert_eq!(
        line.as_deref(),
        Some("paste target app=com.apple.Terminal captured_at=release result=pasted")
    );
}

/// The stale-capture case: a target read EARLIER than paste time (here the
/// key-down fallback, Terminal) is re-verified immediately before ⌘V. With
/// Chrome in front by then nothing is typed anywhere — the take is held and on
/// the clipboard — and the log names the failure.
#[test]
fn a_stale_capture_never_pastes_into_the_app_now_in_front() {
    // Release read unreadable → press fallback (Terminal), Chrome in front now.
    let (target, outcome, mac, line) = dictate(Some(terminal()), None, CHROME);
    assert_eq!(target.bundle_id.as_deref(), Some(TERMINAL));
    assert_eq!(target.captured_at, CapturedAt::Press);
    assert!(!outcome.pasted);
    assert!(
        mac.landed.is_empty(),
        "no ⌘V into an app the take was not for"
    );
    assert_eq!(mac.copies, vec![TAKE.to_string()], "the text is never lost");
    assert_eq!(
        line.as_deref(),
        Some("paste target app=com.apple.Terminal captured_at=press result=failed:wrong_target")
    );
}

/// Yap's own float panel / main window in front at release is never the
/// target: the key-down app is used instead (and still re-verified).
#[test]
fn yaps_own_panel_is_never_the_paste_target() {
    let target = resolve_paste_target(Some(&chrome()), Some(&yap_panel()), Some(YAP));
    assert_eq!(target.bundle_id.as_deref(), Some(CHROME));
    assert_eq!(target.captured_at, CapturedAt::Press);
    assert_eq!(target.name.as_deref(), Some("Google Chrome"));

    // Yap at both edges → no target at all, and the history row never says "Yap".
    let none = resolve_paste_target(Some(&yap_panel()), Some(&yap_panel()), Some(YAP));
    assert_eq!(none.bundle_id, None);
    assert_eq!(none.name, None);

    // The test channel and any build id are Yap too.
    assert!(is_own_app(
        Some("com.wilsonguenther.wilson-voice.test"),
        None
    ));
    assert!(is_own_app(Some(YAP_BUNDLE_PREFIX), None));
    assert!(is_own_app(Some("dev.local.yap"), Some("dev.local.yap")));
    assert!(!is_own_app(Some(CHROME), Some(YAP)));
    assert!(!is_own_app(None, Some(YAP)));
}

/// Release wins over press whenever the release read is usable; a blank
/// identity is unusable, never a match.
#[test]
fn release_read_wins_and_blank_identities_are_unusable() {
    let t = resolve_paste_target(Some(&terminal()), Some(&chrome()), None);
    assert_eq!(t.captured_at, CapturedAt::Release);
    assert_eq!(t.bundle_id.as_deref(), Some(CHROME));

    let blank = AppIdentity::new(Some("   "), Some("  "));
    assert_eq!(blank, AppIdentity::default());
    let t = resolve_paste_target(Some(&blank), Some(&blank), None);
    assert_eq!(t.bundle_id, None, "unknown target → held, never typed");
    assert_eq!(CapturedAt::Press.as_str(), "press");
    assert_eq!(CapturedAt::Release.as_str(), "release");
}

/// The result word: `copied` only for a copy-only take by design, `failed:…`
/// for a wanted paste that did not land.
#[test]
fn paste_target_result_words() {
    let pasted = PasteOutcome {
        copied: true,
        pasted: true,
        message: String::new(),
    };
    let copied = PasteOutcome {
        copied: true,
        pasted: false,
        message: String::new(),
    };
    let held = PasteOutcome {
        copied: false,
        pasted: false,
        message: String::new(),
    };
    assert_eq!(
        paste::paste_target_result(true, &pasted, InsertFallback::None),
        "pasted"
    );
    assert_eq!(
        paste::paste_target_result(false, &copied, InsertFallback::None),
        "copied"
    );
    assert_eq!(
        paste::paste_target_result(true, &copied, InsertFallback::NoReceipt),
        "failed:no_receipt"
    );
    assert_eq!(
        paste::paste_target_result(false, &held, InsertFallback::SecureField),
        "failed:secure_field"
    );
    // Not a dictated app → no `paste target` line.
    let trace = paste::InsertTrace {
        path: InsertPath::PasteBridge,
        app: None,
        fallback: InsertFallback::None,
    };
    assert!(paste::paste_target_log_line(
        PasteTarget::Explicit,
        CapturedAt::Release,
        true,
        &pasted,
        &trace
    )
    .is_none());
}
