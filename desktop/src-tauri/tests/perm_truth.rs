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

// ── yap-fx-08 · the one-time duplicate-copies notice ───────────────────────

use wilson_voice_lib::mic_auth::MicAuth;
use wilson_voice_lib::perm_truth::{
    canonical_bundle_path, duplicate_copies_notice, is_user_copy_path, merge_registered_copies,
    microphone_holder_sentence, should_show_duplicate_notice, trash_failed_sentence,
    DUPLICATE_NOTICE_KEY,
};

const DESKTOP_YAP: &str = "/Users/w/Desktop/Yap.app";

#[test]
fn two_copies_notice_asks_wilsons_question_word_for_word() {
    let n = duplicate_copies_notice(YAP, &[DESKTOP_YAP.to_string()], "Yap", MicAuth::Authorized)
        .expect("a second copy with our bundle id is warned about");
    assert_eq!(
        n.title,
        "Two copies of Yap are installed — keep /Applications/Yap.app and move the other to the Trash?"
    );
    assert_eq!(n.keep, YAP);
    assert_eq!(n.running, YAP);
    assert_eq!(n.to_trash, vec![DESKTOP_YAP.to_string()]);
    assert_eq!(canonical_bundle_path("Yap"), YAP);
}

#[test]
fn notice_names_the_running_copy_and_the_microphone_holder() {
    let n = duplicate_copies_notice(YAP, &[DESKTOP_YAP.to_string()], "Yap", MicAuth::Authorized)
        .unwrap();
    assert!(
        n.detail.contains("Running now: /Applications/Yap.app."),
        "{}",
        n.detail
    );
    assert!(
        n.detail.contains(
            "The Microphone permission is held by the running copy (/Applications/Yap.app)."
        ),
        "{}",
        n.detail
    );

    // Run from the Desktop, denied the mic: the grant sits with the other copy.
    let d =
        duplicate_copies_notice(DESKTOP_YAP, &[YAP.to_string()], "Yap", MicAuth::Denied).unwrap();
    assert!(
        d.detail.contains("Running now: /Users/w/Desktop/Yap.app."),
        "{}",
        d.detail
    );
    assert!(
        d.detail
            .contains("held by the other copy (/Applications/Yap.app)"),
        "{}",
        d.detail
    );
    let nd = microphone_holder_sentence(MicAuth::NotDetermined, DESKTOP_YAP, &[YAP.to_string()]);
    assert!(
        nd.contains("held by the other copy (/Applications/Yap.app)"),
        "{nd}"
    );
    assert!(microphone_holder_sentence(MicAuth::Restricted, YAP, &[]).contains("device policy"));
}

#[test]
fn running_from_the_desktop_keeps_applications_and_trashes_the_running_copy() {
    let n = duplicate_copies_notice(DESKTOP_YAP, &[YAP.to_string()], "Yap", MicAuth::Authorized)
        .unwrap();
    assert_eq!(n.keep, YAP);
    assert_eq!(n.to_trash, vec![DESKTOP_YAP.to_string()]);
    assert!(
        n.detail
            .contains("This running copy moves to the Trash too")
            && n.detail.contains("then open /Applications/Yap.app"),
        "{}",
        n.detail
    );
    assert!(n
        .title
        .starts_with("Two copies of Yap are installed — keep /Applications/Yap.app"));
}

#[test]
fn without_an_applications_copy_the_running_one_is_kept() {
    let mine = "/Users/w/Applications/Yap.app";
    let n = duplicate_copies_notice(mine, &[DESKTOP_YAP.to_string()], "Yap", MicAuth::Authorized)
        .unwrap();
    assert_eq!(n.keep, mine);
    assert_eq!(n.to_trash, vec![DESKTOP_YAP.to_string()]);
    assert!(!n.detail.contains("moves to the Trash too"));
}

#[test]
fn three_copies_say_the_others() {
    let n = duplicate_copies_notice(
        YAP,
        &[
            DESKTOP_YAP.to_string(),
            "/Users/w/Downloads/Yap.app".to_string(),
        ],
        "Yap",
        MicAuth::Authorized,
    )
    .unwrap();
    assert_eq!(
        n.title,
        "Three copies of Yap are installed — keep /Applications/Yap.app and move the others to the Trash?"
    );
    assert_eq!(n.to_trash.len(), 2);
    assert!(!n.to_trash.contains(&YAP.to_string()));
}

#[test]
fn no_notice_without_a_second_copy_or_without_a_bundle() {
    assert!(duplicate_copies_notice(YAP, &[], "Yap", MicAuth::Authorized).is_none());
    assert!(duplicate_copies_notice(YAP, &[YAP.to_string()], "Yap", MicAuth::Authorized).is_none());
    // A `cargo run` dev binary is not a bundle: nothing to keep or trash.
    assert!(duplicate_copies_notice(
        "/Users/w/code/wilson-voice/desktop/src-tauri/target/debug/wilson-voice",
        &[YAP.to_string()],
        "Yap",
        MicAuth::Authorized
    )
    .is_none());
}

#[test]
fn notice_shows_once_per_set_of_copies() {
    let n = duplicate_copies_notice(YAP, &[DESKTOP_YAP.to_string()], "Yap", MicAuth::Authorized)
        .unwrap();
    assert!(should_show_duplicate_notice(None, &n), "never shown → show");
    assert!(
        !should_show_duplicate_notice(Some(n.fingerprint.as_str()), &n),
        "already shown for this set → silent on every later launch"
    );
    // The same two copies seen from the other side are the same set.
    let flipped =
        duplicate_copies_notice(DESKTOP_YAP, &[YAP.to_string()], "Yap", MicAuth::Denied).unwrap();
    assert_eq!(flipped.fingerprint, n.fingerprint);
    // A new copy is a new situation.
    let three = duplicate_copies_notice(
        YAP,
        &[
            DESKTOP_YAP.to_string(),
            "/Users/w/Downloads/Yap.app".to_string(),
        ],
        "Yap",
        MicAuth::Authorized,
    )
    .unwrap();
    assert!(should_show_duplicate_notice(
        Some(n.fingerprint.as_str()),
        &three
    ));
    assert_eq!(
        DUPLICATE_NOTICE_KEY,
        "perm_truth.duplicate_notice_fingerprint"
    );
}

#[test]
fn trash_failure_is_one_sentence_with_one_action() {
    assert_eq!(
        trash_failed_sentence(DESKTOP_YAP),
        "Yap could not move /Users/w/Desktop/Yap.app to the Trash — drag it to the Trash in Finder."
    );
}

#[test]
fn registered_copies_skip_build_trash_and_disk_image_paths() {
    assert!(is_user_copy_path(Path::new(YAP)));
    assert!(is_user_copy_path(Path::new(DESKTOP_YAP)));
    assert!(is_user_copy_path(Path::new(
        "/Users/w/Applications/Old builds/Yap.app"
    )));
    for p in [
        "/Users/w/code/wilson-voice/desktop/src-tauri/target/release/bundle/macos/Yap.app",
        "/Users/w/Library/Developer/Xcode/DerivedData/x/Build/Products/Debug/Yap.app",
        "/Users/w/.Trash/Yap.app",
        "/Volumes/Yap 0.9.3/Yap.app",
        "/private/var/folders/ab/T/AppTranslocation/1234/d/Yap.app",
        "/Applications/Yap.app/Contents/Helpers/Yap.app",
        "/Users/w/Desktop/Yap",
    ] {
        assert!(!is_user_copy_path(Path::new(p)), "{p} is not a user copy");
    }
}

#[test]
fn merge_adds_the_desktop_copy_launchservices_knows() {
    let apps = temp_root("merge-apps");
    let desk = temp_root("merge-desktop");
    let me = apps.join("Yap.app");
    make_app(&me, perm_truth::BUNDLE_ID);
    let desktop_copy = desk.join("Yap.app");
    make_app(&desktop_copy, perm_truth::BUNDLE_ID);
    let scanned = find_duplicate_bundles(std::slice::from_ref(&apps), perm_truth::BUNDLE_ID, &me);
    assert!(
        scanned.is_empty(),
        "the folder scan never lists the Desktop"
    );

    let registered = vec![
        desktop_copy.clone(),
        me.clone(),                          // the running copy itself
        desk.join("Deleted.app"),            // LaunchServices remembers deleted bundles
        desk.join("target").join("Yap.app"), // a build product
        desktop_copy.clone(),                // listed twice
    ];
    let merged = merge_registered_copies(scanned, registered, &me);
    assert_eq!(merged, vec![desktop_copy.clone()]);

    let _ = fs::remove_dir_all(&apps);
    let _ = fs::remove_dir_all(&desk);
}
