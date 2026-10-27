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
    self, AxCaretSnapshot, AxInsert, BridgeReceipt, CopyCard, CopyCardReason, InsertBackend,
    InsertFallback, InsertPath, InsertTrace, PasteTarget, COPY_CARD_MESSAGE,
    COPY_CARD_MESSAGE_NO_REPAIR, INSERT_COPY_CARD_EVENT,
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
    /// The frontmost bundle ships Electron Framework.framework.
    electron: bool,
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
            electron: false,
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
    fn frontmost_has_electron(&mut self) -> bool {
        self.electron
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

// ── yap-insert-bridge-2026-10-26 — paste bridge first ───────────────────────
// #229 encoded "Accessibility granted → AX first" here (ax_trusted_uses_ax) and
// that is the regression: Electron/Chromium accept the AX set and never read it
// back changed, so every take ended on the Copy card. Replaced, not kept.

/// PostEvent granted → the receipt-checked paste bridge, whatever Accessibility says.
#[test]
fn post_event_granted_uses_paste_bridge_regardless_of_ax() {
    assert_eq!(
        paste::choose_insert_path(&probe(true, true)),
        InsertPath::PasteBridge,
        "ax=Granted post=Granted is the 0.9.3-dev regression case"
    );
    assert_eq!(
        paste::choose_insert_path(&probe(false, true)),
        InsertPath::PasteBridge
    );

    let mut f = Fake::new();
    f.frontmost = Some("com.anthropic.claudefordesktop".into());
    let (out, trace) = paste::insert_traced(
        &mut f,
        paste::choose_insert_path(&probe(true, true)),
        TAKE,
        true,
        PasteTarget::Dictated(Some("com.anthropic.claudefordesktop")),
    );
    println!(
        "claude desktop, ax+post granted: {} ax_calls={} posts={} pasted={}",
        paste::insert_log_line(&trace),
        f.ax_calls,
        f.posts,
        out.pasted
    );
    assert_eq!(f.ax_calls, 0, "no AX attempt when ⌘V can be posted");
    assert_eq!(f.posts, 1, "exactly one ⌘V");
    assert!(out.pasted, "the bridge's receipt earned it");
    assert!(f.cards.is_empty(), "no Copy card on a confirmed paste");
    assert_eq!(trace.path, InsertPath::PasteBridge);
    assert_eq!(trace.fallback, InsertFallback::None);
}

/// AX is the path only when PostEvent is NOT granted but Accessibility is.
#[test]
fn ax_only_when_post_event_denied() {
    assert_eq!(
        paste::choose_insert_path(&probe(true, false)),
        InsertPath::Ax
    );
    let mut f = Fake::new();
    f.frontmost = Some("com.apple.TextEdit".into());
    let (out, trace) = paste::insert_traced(
        &mut f,
        InsertPath::Ax,
        TAKE,
        true,
        PasteTarget::Dictated(Some("com.apple.TextEdit")),
    );
    assert!(out.pasted, "an AX insert that landed is a paste");
    assert_eq!(f.ax_calls, 1);
    assert_eq!(f.posts, 0, "a landed AX insert posts no ⌘V");
    assert!(f.copies.is_empty(), "AX leaves the user's clipboard alone");
    assert!(f.cards.is_empty());
    assert_eq!(trace.path, InsertPath::Ax);
    assert_eq!(trace.fallback, InsertFallback::PostEventDenied);
}

/// Neither grant → the Copy card, which NAMES both blocked permissions and
/// offers Repair; zero keystrokes, zero AX.
#[test]
fn both_denied_shows_copy_card() {
    let path = paste::choose_insert_path(&probe(false, false));
    assert_eq!(path, InsertPath::CopyCard);
    let mut f = Fake::new();
    let (out, trace) = paste::insert_traced(
        &mut f,
        path,
        TAKE,
        true,
        PasteTarget::Dictated(Some("TextEdit")),
    );
    println!(
        "both denied: {} blocked={:?} repair={:?}",
        paste::insert_log_line(&trace),
        f.cards[0].blocked_permissions,
        f.cards[0].repair_service
    );
    assert_eq!((f.posts, f.ax_calls), (0, 0));
    assert!(!out.pasted);
    assert_eq!(
        f.cards.len(),
        1,
        "pasted=false always comes with a card here"
    );
    assert_eq!(f.cards[0].reason, CopyCardReason::NoPermission);
    assert_eq!(
        f.cards[0].blocked_permissions,
        vec!["PostEvent", "Accessibility"]
    );
    assert_eq!(f.cards[0].repair_service, Some("accessibility"));
    assert_eq!(trace.path, InsertPath::CopyCard);
    assert_eq!(trace.fallback, InsertFallback::NoPermission);
    // A card raised while a grant works names nothing blocked.
    assert!(paste::blocked_permissions_for(CopyCardReason::NoReceipt).is_empty());
}

/// Denylisted bundle ids and any Electron bundle never get an AX attempt, even
/// on the Ax path (PostEvent denied): they go to the bridge.
#[test]
fn denylisted_bundle_never_calls_ax_insert() {
    for id in paste::AX_ASYNC_DENYLIST {
        let mut f = Fake::new();
        f.frontmost = Some(id.to_string());
        let (out, trace) = paste::insert_traced(
            &mut f,
            InsertPath::Ax,
            TAKE,
            true,
            PasteTarget::Dictated(Some(id)),
        );
        assert_eq!(f.ax_calls, 0, "{id}: never an AX insert");
        assert_eq!(f.posts, 1, "{id}: the bridge, once");
        assert!(out.pasted);
        assert_eq!(trace.path, InsertPath::PasteBridge);
        assert_eq!(trace.fallback, InsertFallback::ElectronDenylist);
    }
    // Not on the list, but the bundle ships Electron Framework.framework.
    let mut f = Fake::new();
    f.frontmost = Some("com.example.some-electron-app".into());
    f.electron = true;
    let (_, trace) = paste::insert_traced(
        &mut f,
        InsertPath::Ax,
        TAKE,
        true,
        PasteTarget::Dictated(Some("com.example.some-electron-app")),
    );
    println!(
        "electron bundle: {} ax_calls={}",
        paste::insert_log_line(&trace),
        f.ax_calls
    );
    assert_eq!((f.ax_calls, f.posts), (0, 1));
    assert_eq!(trace.fallback, InsertFallback::ElectronDenylist);
    assert!(
        paste::is_ax_async_app(Some("com.google.chrome"), false),
        "case-insensitive"
    );
    assert!(!paste::is_ax_async_app(Some("com.apple.TextEdit"), false));
    assert!(paste::is_ax_async_app(None, true));
    assert_eq!(paste::ELECTRON_FRAMEWORK, "Electron Framework.framework");
    // The bundle check itself, against a real directory shape.
    let dir = std::env::temp_dir().join(format!("ib26-electron-{}", std::process::id()));
    let fw = dir.join("Fake.app/Contents/Frameworks/Electron Framework.framework");
    std::fs::create_dir_all(&fw).unwrap();
    assert!(paste::bundle_has_electron_framework(
        dir.join("Fake.app").to_str().unwrap()
    ));
    assert!(!paste::bundle_has_electron_framework(
        dir.join("Other.app").to_str().unwrap()
    ));
    std::fs::remove_dir_all(&dir).unwrap();
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

/// Y6-C (PANEL 2026-09-12) changed this row from "copied only" to HELD AND
/// OFFERED: still zero keystrokes into the app the user switched to, and the
/// take now also waits on the pill (one Insert button) instead of living only
/// behind a toast. tests/paste_target_e2e.rs owns the full case list.
#[test]
fn wrong_target_is_held_never_typed() {
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
        assert_eq!(f.cards.len(), 1, "{path:?}: the take is held on the pill");
        assert_eq!(f.cards[0].reason, CopyCardReason::TargetMoved);
        assert_eq!(f.copies, vec![TAKE.to_string()]);
        assert!(!out.pasted);
        assert_eq!(out.message, paste::TARGET_MOVED_MESSAGE);

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

/// An AX set macOS accepted whose value never read back changed, and whose
/// caret (AXNumberOfCharacters / AXSelectedTextRange) did not move in the
/// ~400 ms re-read: nothing landed, so it is Refused → the bridge runs EXACTLY
/// once and no card is shown. (Replaces ax_unverified_never_posts_cmd_v, which
/// stranded every Electron take on the card.)
#[test]
fn ax_unverified_unchanged_becomes_refused_bridge_once() {
    let before = AxCaretSnapshot {
        chars: Some(12),
        range: Some((12, 0)),
    };
    let settled = paste::settle_unverified("value never read back changed", &before, &[before; 8]);
    assert!(
        matches!(&settled, AxInsert::Refused(why) if why.starts_with(paste::AX_UNVERIFIED_UNCHANGED)),
        "{settled:?}"
    );
    let mut f = Fake::new();
    f.ax_result = settled;
    let (out, trace) = paste::insert_traced(
        &mut f,
        InsertPath::Ax,
        TAKE,
        true,
        PasteTarget::Dictated(Some("TextEdit")),
    );
    println!(
        "ax unverified+unchanged: {} ax_calls={} posts={} cards={}",
        paste::insert_log_line(&trace),
        f.ax_calls,
        f.posts,
        f.cards.len()
    );
    assert_eq!(f.ax_calls, 1);
    assert_eq!(f.posts, 1, "the bridge runs exactly once");
    assert!(f.cards.is_empty(), "no Copy card");
    assert!(out.pasted);
    assert_eq!(trace.path, InsertPath::PasteBridge);
    assert_eq!(trace.fallback, InsertFallback::AxUnverifiedUnchanged);
}

/// PR #251 fix round 1 (independent BLOCKING 1): an unverified AX set whose
/// caret attributes are UNREADABLE cannot be called "unchanged" — an
/// unreadable attribute proves nothing, and macOS accepted the set, so the
/// text may already be there. It must stay Unverified: the Copy card, ZERO ⌘V
/// (a ⌘V on top of a set that may have landed inserts the take twice). Covers
/// chars=None/range=None on both sides, a caret readable before but unreadable
/// in every re-read, unreadable before but readable after, no re-read at all,
/// and attributes readable on only one side each.
#[test]
fn ax_unverified_unreadable_caret_is_card_zero_cmd_v() {
    let none = AxCaretSnapshot::default();
    let readable = AxCaretSnapshot {
        chars: Some(12),
        range: Some((12, 0)),
    };
    let chars_only = AxCaretSnapshot {
        chars: Some(12),
        range: None,
    };
    let range_only = AxCaretSnapshot {
        chars: None,
        range: Some((12, 0)),
    };
    let cases: [(&str, AxCaretSnapshot, Vec<AxCaretSnapshot>); 5] = [
        ("chars=None range=None both sides", none, vec![none; 8]),
        (
            "readable before, unreadable re-reads",
            readable,
            vec![none; 8],
        ),
        (
            "unreadable before, readable re-reads",
            none,
            vec![readable; 8],
        ),
        ("no re-read at all", readable, vec![]),
        (
            "chars only before, range only after",
            chars_only,
            vec![range_only; 8],
        ),
    ];
    for (label, before, rereads) in cases {
        let settled = paste::settle_unverified("value never read back changed", &before, &rereads);
        assert!(
            matches!(&settled, AxInsert::Unverified(_)),
            "{label}: unreadable caret must stay Unverified, got {settled:?}"
        );
        let mut f = Fake::new();
        f.ax_result = settled;
        let (out, trace) = paste::insert_traced(
            &mut f,
            InsertPath::Ax,
            TAKE,
            true,
            PasteTarget::Dictated(Some("TextEdit")),
        );
        println!(
            "{label}: {} ax_calls={} posts={} cards={} pasted={}",
            paste::insert_log_line(&trace),
            f.ax_calls,
            f.posts,
            f.cards.len(),
            out.pasted
        );
        assert_eq!(f.ax_calls, 1, "{label}");
        assert_eq!(
            f.posts, 0,
            "{label}: no ⌘V on top of a set that may have landed"
        );
        assert_eq!(
            f.cards.len(),
            1,
            "{label}: the Copy card, not a silent drop"
        );
        assert_eq!(f.cards[0].reason, CopyCardReason::AxUnverified, "{label}");
        assert!(!out.pasted, "{label}");
        assert_eq!(trace.fallback, InsertFallback::AxUnverified, "{label}");
    }
    // A caret readable on BOTH sides that did not move is still the proven
    // "unchanged" refusal (bridge once) — the fix narrows, it does not remove.
    assert!(matches!(
        paste::settle_unverified("x", &chars_only, &[none, chars_only]),
        AxInsert::Refused(_)
    ));
}

/// The same unverified set, but the caret DID move in the re-read: the text
/// landed — Inserted, zero ⌘V (no double insert), no card.
#[test]
fn ax_unverified_changed_stays_inserted_zero_cmd_v() {
    let before = AxCaretSnapshot {
        chars: Some(12),
        range: Some((12, 0)),
    };
    let moved_chars = AxCaretSnapshot {
        chars: Some(37),
        range: Some((12, 0)),
    };
    let moved_range = AxCaretSnapshot {
        chars: None,
        range: Some((37, 0)),
    };
    for after in [moved_chars, moved_range] {
        let settled =
            paste::settle_unverified("value never read back changed", &before, &[before, after]);
        assert_eq!(settled, AxInsert::Inserted, "{after:?}");
        let mut f = Fake::new();
        f.ax_result = settled;
        let (out, trace) = paste::insert_traced(
            &mut f,
            InsertPath::Ax,
            TAKE,
            true,
            PasteTarget::Dictated(Some("TextEdit")),
        );
        assert_eq!(f.posts, 0, "a landed insert is never followed by ⌘V");
        assert!(out.pasted);
        assert!(f.cards.is_empty());
        assert!(f.copies.is_empty());
        assert_eq!(trace.path, InsertPath::Ax);
    }
}

/// A ⌘V nothing read ends on the Copy card — never a silent `pasted=false` —
/// with the transcript back on the clipboard and the trace saying why.
#[test]
fn bridge_no_receipt_ends_on_card_never_silent() {
    let mut f = Fake::new();
    f.frontmost = Some("com.anthropic.claudefordesktop".into());
    f.bridge_result = Ok(BridgeReceipt::Missing {
        reason: "no app read the clipboard within 1500ms of ⌘V".into(),
        clipboard_taken: false,
    });
    let (out, trace) = paste::insert_traced(
        &mut f,
        InsertPath::PasteBridge,
        TAKE,
        true,
        PasteTarget::Dictated(Some("com.anthropic.claudefordesktop")),
    );
    println!(
        "no receipt: {} pasted={} cards={}",
        paste::insert_log_line(&trace),
        out.pasted,
        f.cards.len()
    );
    assert!(!out.pasted);
    assert_eq!(f.posts, 1);
    assert_eq!(f.cards.len(), 1, "pasted=false comes with a card");
    assert_eq!(f.cards[0].reason, CopyCardReason::NoReceipt);
    assert_eq!(f.copies, vec![TAKE.to_string()]);
    assert_eq!(trace.path, InsertPath::CopyCard);
    assert_eq!(trace.fallback, InsertFallback::NoReceipt);
}

/// Snapshot of the one-per-take log line.
#[test]
fn insert_log_line_format() {
    let line = |path, app: Option<&str>, fallback| {
        paste::insert_log_line(&InsertTrace {
            path,
            app: app.map(str::to_string),
            fallback,
        })
    };
    assert_eq!(
        line(
            InsertPath::PasteBridge,
            Some("com.anthropic.claudefordesktop"),
            InsertFallback::None
        ),
        "insert path=PasteBridge app=com.anthropic.claudefordesktop fallback=none"
    );
    assert_eq!(
        line(
            InsertPath::PasteBridge,
            Some("com.example.e"),
            InsertFallback::ElectronDenylist
        ),
        "insert path=PasteBridge app=com.example.e fallback=electron_denylist"
    );
    assert_eq!(
        line(
            InsertPath::PasteBridge,
            Some("com.apple.TextEdit"),
            InsertFallback::AxUnverifiedUnchanged
        ),
        "insert path=PasteBridge app=com.apple.TextEdit fallback=ax_unverified_unchanged"
    );
    assert_eq!(
        line(
            InsertPath::Ax,
            Some("com.apple.TextEdit"),
            InsertFallback::PostEventDenied
        ),
        "insert path=Ax app=com.apple.TextEdit fallback=post_event_denied"
    );
    assert_eq!(
        line(InsertPath::CopyCard, None, InsertFallback::NoReceipt),
        "insert path=CopyCard app=unknown fallback=no_receipt"
    );
    // Through the real flow: the trace's app is the dictated bundle id.
    let mut f = Fake::new();
    f.frontmost = Some("com.apple.Terminal".into());
    let (_, trace) = paste::insert_traced(
        &mut f,
        InsertPath::PasteBridge,
        TAKE,
        true,
        PasteTarget::Dictated(Some("com.apple.Terminal")),
    );
    assert_eq!(
        paste::insert_log_line(&trace),
        "insert path=PasteBridge app=com.apple.Terminal fallback=none"
    );
}

/// Fix round 2 (independent BLOCKING 1): a `kAXSelectedText` set that returned
/// kAXErrorCannotComplete (-25204, the 1 s messaging timeout) may still be
/// applied by a busy target, so it ends on the Copy card with ZERO ⌘V — never
/// Refused → paste bridge (a double insert). A proven refusal still falls back.
#[test]
fn ax_set_timeout_never_posts_cmd_v() {
    let mut f = Fake::new();
    f.ax_result = paste::ax_insert_after_set_error(-25204);
    let out = paste::insert_with(
        &mut f,
        InsertPath::Ax,
        TAKE,
        true,
        PasteTarget::Dictated(Some("TextEdit")),
    );
    println!(
        "ax set -25204: ax_calls={} posts={} cards={} pasted={}",
        f.ax_calls,
        f.posts,
        f.cards.len(),
        out.pasted
    );
    assert_eq!(f.ax_calls, 1);
    assert_eq!(f.posts, 0, "no ⌘V on top of a set that timed out");
    assert!(!out.pasted);
    assert_eq!(f.cards.len(), 1);
    assert_eq!(f.cards[0].reason, CopyCardReason::AxUnverified);
    assert_eq!(f.copies, vec![TAKE.to_string()]);

    // Positive control: kAXErrorAttributeUnsupported (-25205) proves nothing
    // was written, so the bridge fallback (one ⌘V) is still allowed.
    let mut g = Fake::new();
    g.ax_result = paste::ax_insert_after_set_error(-25205);
    assert!(matches!(g.ax_result, AxInsert::Refused(_)));
    let out = paste::insert_with(
        &mut g,
        InsertPath::Ax,
        TAKE,
        true,
        PasteTarget::Dictated(Some("TextEdit")),
    );
    println!("ax set -25205: posts={} pasted={}", g.posts, out.pasted);
    assert_eq!(g.posts, 1, "a proven refusal falls back to exactly one ⌘V");
}
