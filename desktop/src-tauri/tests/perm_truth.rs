//! yap-perm-truth — the permission banner tells the truth about THIS binary.
//!
//! Pure decisions only: no TCC read, no TCC write, no window server. The six
//! cases the item names: a stale-signature message, silence when signatures
//! match, a duplicate bundle found, none found, the failing-conditions wording,
//! and delivery evidence after a re-arm.

use std::fs;
use std::path::{Path, PathBuf};
use wilson_voice_lib::perm_truth::{
    self, failing_conditions, find_duplicate_bundles, plist_xml_string, record_trust, stale_grant,
    stale_grants, SigningSignature, TrustLedger, ACCESSIBILITY_CONDITION,
    INPUT_MONITORING_CONDITION,
};
use wilson_voice_lib::permissions::{self, GrantState};
use wilson_voice_lib::ptt_macos::{TapDisable, TapHealth};

const YAP: &str = "/Applications/Yap.app";

fn sig(authority: &str, cdhash: &str, version: &str) -> SigningSignature {
    SigningSignature {
        authority: authority.into(),
        cdhash: cdhash.into(),
        version: version.into(),
    }
}

fn dev_085() -> SigningSignature {
    sig(
        "Apple Development: Wilson Guenther (ABCDE12345)",
        "aa11",
        "0.8.5",
    )
}

fn devid_090() -> SigningSignature {
    sig(
        "Developer ID Application: Wilson Guenther (ABCDE12345)",
        "bb22",
        "0.9.0",
    )
}

#[test]
fn stale_signature_produces_the_remove_and_re_add_message() {
    let stale = stale_grant(
        permissions::GRANT_ACCESSIBILITY,
        "Accessibility",
        false,
        Some(&dev_085()),
        &devid_090(),
        YAP,
    )
    .expect("a grant last true under another signature is stale");
    assert_eq!(stale.previous_version, "0.8.5");
    assert_eq!(
        stale.message,
        "macOS still lists Yap as allowed in Accessibility, but that permission was given to \
         Yap 0.8.5, signed differently. In System Settings → Privacy & Security → \
         Accessibility, select Yap, click the minus button, then click plus and choose \
         /Applications/Yap.app."
    );

    // Same decision for Input Monitoring, through the two-grant helper.
    let ledger = TrustLedger {
        accessibility: Some(dev_085()),
        input_monitoring: Some(dev_085()),
    };
    let both = stale_grants(&ledger, false, GrantState::Denied, &devid_090(), YAP);
    assert_eq!(both.len(), 2);
    assert_eq!(both[1].grant, permissions::GRANT_INPUT_MONITORING);
    assert!(both[1].message.contains("allowed in Input Monitoring"));
    // An Unknown Input Monitoring grant is silent, as it is everywhere else.
    assert_eq!(
        stale_grants(&ledger, false, GrantState::Unknown, &devid_090(), YAP).len(),
        1
    );
}

#[test]
fn no_stale_message_when_signatures_match_or_the_grant_works() {
    // Same signature, still false: the user never granted THIS build — no
    // stale story to tell, the plain failing condition is the whole truth.
    assert!(stale_grant(
        "accessibility",
        "Accessibility",
        false,
        Some(&devid_090()),
        &devid_090(),
        YAP
    )
    .is_none());
    // Different signature but the grant reads true right now: nothing is stale.
    assert!(stale_grant(
        "accessibility",
        "Accessibility",
        true,
        Some(&dev_085()),
        &devid_090(),
        YAP
    )
    .is_none());
    // Never seen true at all: we cannot claim it went stale.
    assert!(stale_grant(
        "accessibility",
        "Accessibility",
        false,
        None,
        &devid_090(),
        YAP
    )
    .is_none());
    // Any ONE of authority / cdhash / version differing is a different signature.
    let rebuilt = sig(&devid_090().authority, "cc33", "0.9.0");
    assert!(stale_grant(
        "accessibility",
        "Accessibility",
        false,
        Some(&rebuilt),
        &devid_090(),
        YAP
    )
    .is_some());
}

#[test]
fn a_true_read_records_the_running_signature_and_a_false_read_never_erases_it() {
    let mut ledger = TrustLedger::default();
    assert!(record_trust(&mut ledger, true, false, &dev_085()));
    assert_eq!(ledger.accessibility, Some(dev_085()));
    assert_eq!(ledger.input_monitoring, None);
    // Re-reading the same truth is not a change (no disk write on a 5 s poll).
    assert!(!record_trust(&mut ledger, true, false, &dev_085()));
    // A false read under a new signature keeps the evidence of the last good one.
    assert!(!record_trust(&mut ledger, false, false, &devid_090()));
    assert_eq!(ledger.accessibility, Some(dev_085()));
    // Ledger survives a JSON round trip (it is persisted in the data dir).
    let json = serde_json::to_string(&ledger).unwrap();
    assert!(json.contains("\"accessibility\""));
    let back: TrustLedger = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ledger);
}

fn temp_root(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "yap-perm-truth-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn make_app(path: &Path, bundle_id: &str) {
    let contents = path.join("Contents");
    fs::create_dir_all(&contents).unwrap();
    fs::write(
        contents.join("Info.plist"),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n\
             \t<key>CFBundleName</key>\n\t<string>Yap</string>\n\
             \t<key>CFBundleIdentifier</key>\n\t<string>{bundle_id}</string>\n</dict>\n</plist>\n"
        ),
    )
    .unwrap();
}

#[test]
fn a_duplicate_yap_is_found_at_the_top_level_and_one_folder_down() {
    let apps = temp_root("dup");
    let home_apps = temp_root("dup-home");
    let me = apps.join("Yap.app");
    make_app(&me, perm_truth::BUNDLE_ID);
    make_app(&apps.join("Yap 0.8.5.app"), perm_truth::BUNDLE_ID);
    make_app(
        &apps.join("Old builds").join("Yap.app"),
        perm_truth::BUNDLE_ID,
    );
    make_app(&home_apps.join("Yap.app"), perm_truth::BUNDLE_ID);
    // Two levels down is out of scope by design.
    make_app(
        &apps.join("a").join("b").join("Yap.app"),
        perm_truth::BUNDLE_ID,
    );
    // A different app is never a duplicate.
    make_app(&apps.join("Notes.app"), "com.example.notes");

    let found = find_duplicate_bundles(
        &[apps.clone(), home_apps.clone()],
        perm_truth::BUNDLE_ID,
        &me,
    );
    let mut want = vec![
        apps.join("Old builds").join("Yap.app"),
        apps.join("Yap 0.8.5.app"),
        home_apps.join("Yap.app"),
    ];
    want.sort();
    assert_eq!(
        found, want,
        "the running bundle itself is never its own duplicate"
    );

    let _ = fs::remove_dir_all(&apps);
    let _ = fs::remove_dir_all(&home_apps);
}

#[test]
fn no_duplicate_when_only_this_yap_exists() {
    let apps = temp_root("nodup");
    let me = apps.join("Yap.app");
    make_app(&me, perm_truth::BUNDLE_ID);
    make_app(&apps.join("Other.app"), "com.example.other");
    fs::create_dir_all(apps.join("Utilities")).unwrap();
    let missing = apps.join("does-not-exist");
    assert!(
        find_duplicate_bundles(&[apps.clone(), missing], perm_truth::BUNDLE_ID, &me).is_empty()
    );
    assert_eq!(
        plist_xml_string(
            &fs::read_to_string(me.join("Contents/Info.plist")).unwrap(),
            "CFBundleIdentifier"
        )
        .as_deref(),
        Some(perm_truth::BUNDLE_ID)
    );
    let _ = fs::remove_dir_all(&apps);
}

#[test]
fn failing_conditions_name_the_os_call_and_its_answer() {
    let both = failing_conditions(false, GrantState::Denied);
    let texts: Vec<&str> = both.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(
        texts,
        vec![
            "Accessibility false (AXIsProcessTrusted)",
            "Input Monitoring false (IOHIDCheckAccess listen = denied)"
        ]
    );
    assert_eq!(texts[0], ACCESSIBILITY_CONDITION);
    assert_eq!(texts[1], INPUT_MONITORING_CONDITION);
    assert_eq!(
        (both[0].api, both[0].observed),
        ("AXIsProcessTrusted", "false")
    );
    assert_eq!(
        (both[1].api, both[1].observed),
        ("IOHIDCheckAccess listen", "denied")
    );
    // Unknown is not a failing condition; a granted Accessibility is not either.
    assert!(failing_conditions(true, GrantState::Unknown).is_empty());
    assert!(failing_conditions(true, GrantState::Authorized).is_empty());
    // The report carries them under the camelCase names the banner reads.
    let mut report = permissions::build_report_with_grants(
        false,
        wilson_voice_lib::mic_auth::MicAuth::Authorized,
        GrantState::Denied,
        GrantState::Unknown,
        true,
        String::new(),
        "Yap",
    );
    report.checked_bundle_path = YAP.into();
    report.failing_conditions = both;
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["checkedBundlePath"], YAP);
    assert_eq!(
        json["failingConditions"][0]["text"],
        ACCESSIBILITY_CONDITION
    );
    assert!(json["staleGrants"].is_array());
    assert!(json["duplicateBundles"].is_array());
}

#[test]
fn delivery_evidence_is_an_edge_since_the_last_re_arm_never_a_zero_re_arm_count() {
    // A fresh tap that has delivered nothing is NOT delivering, even though
    // re_arms == 0 — the exact premise the old rule got wrong.
    let mut h = TapHealth::default();
    assert_eq!(h.re_arms, 0);
    assert!(!h.delivering());
    // An edge arrives: now it is.
    h.note_edge();
    assert!(h.delivering());
    // macOS disables and we re-arm: the old evidence is void.
    h.record(TapDisable::Timeout);
    assert!(!h.delivering(), "a re-arm resets delivery evidence");
    // 0.9.0: re-armed 7 times, delivered nothing → never delivering.
    for _ in 0..6 {
        h.record(TapDisable::UserInput);
    }
    assert_eq!(h.re_arms, 7);
    assert!(!h.delivering());
    // An edge after the re-arm is real evidence again, re_arms notwithstanding.
    h.note_edge();
    assert!(h.delivering());

    // And the rule it feeds: no delivery + HID denied + no Accessibility is a denial.
    assert_eq!(
        permissions::input_monitoring_state(GrantState::Denied, false, false),
        GrantState::Denied
    );
    assert_eq!(
        permissions::input_monitoring_state(GrantState::Denied, false, h.delivering()),
        GrantState::Authorized
    );
}

#[test]
fn the_banner_poll_is_five_seconds_and_activation_is_a_watch_reason() {
    assert_eq!(
        permissions::WATCH_INTERVAL,
        std::time::Duration::from_secs(5)
    );
    let r = permissions::WatchReason::Activate;
    assert_eq!(format!("{r:?}"), "Activate");
}

#[test]
fn probe_the_running_process_identity_and_the_real_application_folders() {
    // Command-level probe of the two FFI reads (NSBundle + SecCode) on THIS
    // test binary, and of the duplicate scan against the real /Applications
    // and ~/Applications. Read-only; prints what it saw for the PR body.
    let me = perm_truth::running_identity();
    println!("checkedBundlePath={}", me.bundle_path);
    println!("bundleId={}", me.bundle_id);
    println!("signingAuthority={}", me.signature.authority);
    println!("cdhash={}", me.signature.cdhash);
    println!("version={}", me.signature.version);
    assert!(!me.bundle_path.is_empty());
    assert!(!me.signature.authority.is_empty());
    let own = PathBuf::from(&me.bundle_path);
    let dups = find_duplicate_bundles(
        &perm_truth::default_scan_roots(),
        perm_truth::BUNDLE_ID,
        &own,
    );
    println!("sameIdBundles={dups:?}");
}
