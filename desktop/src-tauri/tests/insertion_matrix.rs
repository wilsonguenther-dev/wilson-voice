//! yap-p0-insertion-bridge-fallback — the insertion matrix.
//!
//! Every row drives the REAL decision (`paste::choose_insert_path` over a
//! `perm_probe::ProbeReport` built by the real `build_probe_report`) and the
//! REAL insertion flow (`paste::insert_with` / `paste::delete_with`) against a
//! counting fake backend. `posts` counts every CGEventPost the live backend
//! would make (the ⌘V of the paste bridge and the Delete of "delete that"), so
//! "posts nothing" is a counted zero, not an absence of evidence.

use wilson_voice_lib::mic_auth::MicAuth;
use wilson_voice_lib::paste::{
    self, AxInsert, BridgeReceipt, CopyCard, CopyCardReason, InsertBackend, InsertPath,
    PasteTarget, COPY_CARD_MESSAGE, COPY_CARD_MESSAGE_NO_REPAIR, INSERT_COPY_CARD_EVENT,
};
use wilson_voice_lib::perm_probe::{self, ProbeReport, ProbeState, RawProbes};
use wilson_voice_lib::perm_truth::{SigningSignature, TrustLedger};
use wilson_voice_lib::permissions::GrantState;

fn me() -> SigningSignature {
    SigningSignature {
        authority: "Developer ID Application: Wilson Guenther (VHYV8C2JNU)".into(),
        cdhash: "abc".into(),
        version: "0.9.1".into(),
    }
}

/// A probe whose live answers are `ax` / `post` (everything else granted).
fn probe(ax: bool, post: bool) -> ProbeReport {
    perm_probe::build_probe_report(
        RawProbes {
            accessibility: ax,
            listen_event_cg: true,
            listen_event_hid: GrantState::Authorized,
            post_event: post,
            microphone: MicAuth::Authorized,
        },
        &TrustLedger::default(),
        None,
        &me(),
    )
}

fn word(s: ProbeState) -> String {
    format!("{s:?}").to_lowercase()
}

struct Fake {
    copies: Vec<String>,
    ax_calls: usize,
    /// CGEventPost count: paste-bridge ⌘V + Delete keystrokes.
    posts: usize,
    cards: Vec<CopyCard>,
    frontmost: Option<String>,
    ax_result: AxInsert,
    bridge_result: Result<BridgeReceipt, String>,
}

impl Fake {
    fn new() -> Self {
        Fake {
            copies: vec![],
            ax_calls: 0,
            posts: 0,
            cards: vec![],
            frontmost: Some("TextEdit".into()),
            ax_result: AxInsert::Inserted,
            bridge_result: Ok(BridgeReceipt::Confirmed { after_ms: 12 }),
        }
    }
}

impl InsertBackend for Fake {
    fn copy(&mut self, text: &str) -> Result<(), String> {
        self.copies.push(text.to_string());
        Ok(())
    }
    fn frontmost_app(&mut self) -> Option<String> {
        self.frontmost.clone()
    }
    fn ax_insert(&mut self, _text: &str) -> AxInsert {
        self.ax_calls += 1;
        self.ax_result.clone()
    }
    fn paste_bridge(&mut self, _text: &str) -> Result<BridgeReceipt, String> {
        self.posts += 1;
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

const TAKE: &str = "ship the bridge fix today";

#[test]
fn ax_trusted_uses_ax() {
    // Accessibility granted wins whatever PostEvent says.
    assert_eq!(
        paste::choose_insert_path(&probe(true, true)),
        InsertPath::Ax
    );
    assert_eq!(
        paste::choose_insert_path(&probe(true, false)),
        InsertPath::Ax
    );

    let mut f = Fake::new();
    let out = paste::insert_with(
        &mut f,
        InsertPath::Ax,
        TAKE,
        true,
        PasteTarget::Dictated(Some("TextEdit")),
    );
    assert!(out.pasted, "an AX insert that landed is a paste");
    assert_eq!(f.ax_calls, 1);
    assert_eq!(f.posts, 0, "a landed AX insert posts no ⌘V");
    assert!(f.copies.is_empty(), "AX leaves the user's clipboard alone");
    assert!(f.cards.is_empty());

    // The element refuses → the paste bridge, receipt-checked.
    let mut f = Fake::new();
    f.ax_result = AxInsert::Refused("AXSelectedText is not settable".into());
    let out = paste::insert_with(
        &mut f,
        InsertPath::Ax,
        TAKE,
        true,
        PasteTarget::Dictated(Some("TextEdit")),
    );
    assert_eq!((f.ax_calls, f.posts), (1, 1));
    assert!(out.pasted, "the bridge's receipt earned it");
    assert_eq!(out.message, "Pasted into frontmost app");
}

#[test]
fn ax_dead_post_event_granted_uses_paste_bridge() {
    let p = probe(false, true);
    assert_eq!(p.accessibility.state, ProbeState::Denied);
    assert_eq!(p.post_event.state, ProbeState::Granted);
    assert_eq!(paste::choose_insert_path(&p), InsertPath::PasteBridge);

    // A STALE Accessibility grant (0.9.0 signing change) is just as dead.
    let last = SigningSignature {
        authority: "Apple Development: Wilson Guenther (OLDTEAM123)".into(),
        ..me()
    };
    let stale = perm_probe::build_probe_report(
        RawProbes {
            accessibility: false,
            listen_event_cg: true,
            listen_event_hid: GrantState::Authorized,
            post_event: true,
            microphone: MicAuth::Authorized,
        },
        &TrustLedger::default(),
        Some(&last),
        &me(),
    );
    assert_eq!(stale.accessibility.state, ProbeState::StaleIdentity);
    assert_eq!(paste::choose_insert_path(&stale), InsertPath::PasteBridge);

    let mut f = Fake::new();
    let out = paste::insert_with(
        &mut f,
        InsertPath::PasteBridge,
        TAKE,
        true,
        PasteTarget::Explicit,
    );
    assert_eq!(f.ax_calls, 0, "no AX attempt without Accessibility");
    assert_eq!(f.posts, 1, "exactly one ⌘V");
    assert!(out.pasted);
    assert!(f.cards.is_empty());
}

#[test]
fn none_granted_shows_copy_card_and_posts_nothing() {
    let p = probe(false, false);
    let path = paste::choose_insert_path(&p);
    assert_eq!(path, InsertPath::CopyCard);

    let mut f = Fake::new();
    let out = paste::insert_with(
        &mut f,
        path,
        TAKE,
        true,
        PasteTarget::Dictated(Some("TextEdit")),
    );
    // Delete-that under the same grants posts nothing either.
    let del = paste::delete_with(&mut f, path, Some("TextEdit"));
    println!(
        "insertion_matrix: ax={} post={} path={:?} posts={}",
        word(p.accessibility.state),
        word(p.post_event.state),
        path,
        f.posts
    );
    assert_eq!(f.posts, 0, "zero CGEventPost with neither grant");
    assert_eq!(f.ax_calls, 0);
    assert!(!out.pasted && !del.pasted);
    assert_eq!(f.copies, vec![TAKE.to_string()], "copied exactly once");
    assert_eq!(f.cards.len(), 1, "the card is shown, once");
    assert_eq!(f.cards[0].text, TAKE);
    assert_eq!(f.cards[0].reason, CopyCardReason::NoPermission);
    assert_eq!(f.cards[0].repair_service, Some("accessibility"));
    assert!(f.cards[0].copied);
    assert_eq!(out.message, COPY_CARD_MESSAGE);
    assert_eq!(INSERT_COPY_CARD_EVENT, "insert-copy-card");
}

#[test]
fn paste_bridge_no_receipt_falls_to_copy_card() {
    let mut f = Fake::new();
    f.bridge_result = Ok(BridgeReceipt::Missing {
        reason: "no app read the clipboard within 1500ms of ⌘V".into(),
        clipboard_taken: false,
    });
    let out = paste::insert_with(
        &mut f,
        InsertPath::PasteBridge,
        TAKE,
        true,
        PasteTarget::Explicit,
    );
    assert!(!out.pasted, "no receipt is never a paste");
    assert!(out.copied, "the transcript went back on the clipboard");
    assert_eq!(f.posts, 1);
    assert_eq!(f.copies, vec![TAKE.to_string()]);
    assert_eq!(f.cards.len(), 1, "never a silent pasted=false");
    assert_eq!(f.cards[0].reason, CopyCardReason::NoReceipt);
    assert!(f.cards[0].detail.contains("1500ms"));

    // The user copied something of their own meanwhile: their clipboard wins,
    // and the card (with its own Copy button) still carries the take.
    let mut f = Fake::new();
    f.bridge_result = Ok(BridgeReceipt::Missing {
        reason: "the clipboard changed before the paste was read".into(),
        clipboard_taken: true,
    });
    let out = paste::insert_with(
        &mut f,
        InsertPath::PasteBridge,
        TAKE,
        true,
        PasteTarget::Explicit,
    );
    assert!(!out.pasted && !out.copied);
    assert!(f.copies.is_empty(), "never copied over the user's own copy");
    assert_eq!(f.cards.len(), 1);
    assert!(!f.cards[0].copied);

    // The chord could not even be posted → card too.
    let mut f = Fake::new();
    f.bridge_result = Err("paste timed out waiting for main thread".into());
    let out = paste::insert_with(
        &mut f,
        InsertPath::PasteBridge,
        TAKE,
        true,
        PasteTarget::Explicit,
    );
    assert!(!out.pasted);
    assert_eq!(f.cards.len(), 1);
    assert_eq!(f.cards[0].reason, CopyCardReason::PasteFailed);
}

#[test]
fn wrong_target_copies_only() {
    for path in [InsertPath::Ax, InsertPath::PasteBridge] {
        let mut f = Fake::new();
        f.frontmost = Some("Messages".into());
        let out = paste::insert_with(
            &mut f,
            path,
            TAKE,
            true,
            PasteTarget::Dictated(Some("Notes")),
        );
        assert_eq!(
            f.posts, 0,
            "{path:?}: no ⌘V into the app the user switched to"
        );
        assert_eq!(f.ax_calls, 0, "{path:?}: no AX insert there either");
        assert!(f.cards.is_empty());
        assert_eq!(f.copies, vec![TAKE.to_string()]);
        assert!(!out.pasted);
        assert!(out.message.contains("switched apps"));

        let del = paste::delete_with(&mut f, path, Some("Notes"));
        assert_eq!(f.posts, 0, "{path:?}: no Delete into the wrong app");
        assert!(!del.pasted);
    }
    // Unknown frontmost identity is not a confirmed match either.
    let mut f = Fake::new();
    f.frontmost = None;
    paste::insert_with(
        &mut f,
        InsertPath::PasteBridge,
        TAKE,
        true,
        PasteTarget::Dictated(Some("Notes")),
    );
    assert_eq!(f.posts, 0);
}

#[test]
fn stale_take_copies_only() {
    // The take's generation moved on (cancelled during transcription).
    let want = paste::should_paste(true, 7, 8, true);
    assert!(!want);
    for path in [
        InsertPath::Ax,
        InsertPath::PasteBridge,
        InsertPath::CopyCard,
    ] {
        let mut f = Fake::new();
        let out = paste::insert_with(
            &mut f,
            path,
            TAKE,
            want,
            PasteTarget::Dictated(Some("TextEdit")),
        );
        assert_eq!(f.posts, 0, "{path:?}");
        assert_eq!(f.ax_calls, 0, "{path:?}");
        assert!(
            f.cards.is_empty(),
            "{path:?}: a cancelled take raises no card"
        );
        assert_eq!(f.copies, vec![TAKE.to_string()]);
        assert!(!out.pasted);
    }
}

// ── fix round 1 (PR #229 review) ─────────────────────────────────────────────

/// BLOCKING (both reviewers): AX denied + PostEvent granted, a dictation take
/// whose target identity could not be read must post NOTHING — before this
/// fix the guard was skipped whenever the identity was `None`.
#[test]
fn bridge_never_types_into_an_unconfirmed_target() {
    let mut f = Fake::new();
    f.frontmost = Some("com.apple.MobileSMS".into());
    let out = paste::insert_with(
        &mut f,
        InsertPath::PasteBridge,
        TAKE,
        true,
        PasteTarget::Dictated(None),
    );
    println!(
        "unconfirmed target: posts={} pasted={}",
        f.posts, out.pasted
    );
    assert_eq!(f.posts, 0, "no ⌘V when the dictated app is unknown");
    assert!(!out.pasted);
    assert_eq!(f.copies, vec![TAKE.to_string()], "the text is still copied");

    // Switched apps on the bridge path (bundle ids, which need no AX).
    let mut f = Fake::new();
    f.frontmost = Some("com.apple.MobileSMS".into());
    let out = paste::insert_with(
        &mut f,
        InsertPath::PasteBridge,
        TAKE,
        true,
        PasteTarget::Dictated(Some("com.apple.Notes")),
    );
    println!("switched target: posts={} pasted={}", f.posts, out.pasted);
    assert_eq!(f.posts, 0);
    assert!(!out.pasted);

    // Same app: the bridge still works.
    let mut f = Fake::new();
    f.frontmost = Some("com.apple.Notes".into());
    let out = paste::insert_with(
        &mut f,
        InsertPath::PasteBridge,
        TAKE,
        true,
        PasteTarget::Dictated(Some("com.apple.Notes")),
    );
    assert_eq!(f.posts, 1);
    assert!(out.pasted);
}

/// BLOCKING (both reviewers): a card raised while a grant WORKS must not offer
/// Repair (whose plan resets Accessibility AND PostEvent).
#[test]
fn copy_card_repair_only_when_no_grant_works() {
    assert_eq!(
        paste::repair_service_for(CopyCardReason::NoPermission),
        Some("accessibility")
    );
    for r in [
        CopyCardReason::NoReceipt,
        CopyCardReason::PasteFailed,
        CopyCardReason::AxUnverified,
    ] {
        assert_eq!(paste::repair_service_for(r), None, "{r:?}");
    }
    // NoReceipt on the AX path (Accessibility granted, element refused, ⌘V unread).
    let mut f = Fake::new();
    f.ax_result = AxInsert::Refused("not settable".into());
    f.bridge_result = Ok(BridgeReceipt::Missing {
        reason: "no app read the clipboard".into(),
        clipboard_taken: false,
    });
    paste::insert_with(
        &mut f,
        InsertPath::Ax,
        TAKE,
        true,
        PasteTarget::Dictated(Some("TextEdit")),
    );
    println!(
        "no-receipt card: reason={:?} repair_service={:?}",
        f.cards[0].reason, f.cards[0].repair_service
    );
    assert_eq!(f.cards[0].reason, CopyCardReason::NoReceipt);
    assert_eq!(f.cards[0].repair_service, None);
    // PasteFailed on the bridge path (PostEvent granted).
    let mut f = Fake::new();
    f.bridge_result = Err("timed out".into());
    let out = paste::insert_with(
        &mut f,
        InsertPath::PasteBridge,
        TAKE,
        true,
        PasteTarget::Explicit,
    );
    assert_eq!(f.cards[0].repair_service, None);
    assert_eq!(out.message, COPY_CARD_MESSAGE_NO_REPAIR);
}

/// BLOCKING (independent #2): an AX set macOS accepted but whose value was not
/// seen to change is never followed by a ⌘V (double insert).
#[test]
fn ax_unverified_never_posts_cmd_v() {
    let mut f = Fake::new();
    f.ax_result = AxInsert::Unverified("value never read back changed".into());
    let out = paste::insert_with(
        &mut f,
        InsertPath::Ax,
        TAKE,
        true,
        PasteTarget::Dictated(Some("TextEdit")),
    );
    println!(
        "ax unverified: ax_calls={} posts={} cards={}",
        f.ax_calls,
        f.posts,
        f.cards.len()
    );
    assert_eq!(f.ax_calls, 1);
    assert_eq!(f.posts, 0, "no ⌘V on top of a set that may have landed");
    assert!(!out.pasted);
    assert_eq!(f.cards.len(), 1);
    assert_eq!(f.cards[0].reason, CopyCardReason::AxUnverified);
    assert_eq!(f.cards[0].repair_service, None);
    assert_eq!(f.copies, vec![TAKE.to_string()]);
}
