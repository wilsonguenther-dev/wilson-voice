//! YV101 — the macOS 14.4 runtime gate for system-audio capture, and the two
//! structural guards that keep it from being decorative (plan finding OS-11).
//!
//! OS-11 has three consequences and this file covers all three:
//!
//! * **the gate** — a pure, table-driven check that a process tap is refused
//!   below macOS 14.4 and allowed at or above it;
//! * **the launch check** — the CI step and the script that assert the shipped
//!   release binary does not *hard*-require a 14.4-era CoreAudio symbol (a hard
//!   import is a dyld launch failure for the entire app on macOS 12/13, not a
//!   disabled feature). This file asserts the check is WIRED; the check itself
//!   runs against a real Mach-O in CI, which no unit test can do;
//! * **the Info.plist string and the untouched deployment floor** — the two
//!   `grep` acceptance criteria, made permanent so a later edit reddens here
//!   rather than being noticed at release time.

use wilson_voice_lib::os_version_gate::{
    system_audio_gate, OsVersion, SystemAudioGate, SYSTEM_AUDIO_REQUIREMENT,
};

const INFO_PLIST: &str = include_str!("../Info.plist");
const TAURI_CONF: &str = include_str!("../tauri.conf.json");
const BUILD_RS: &str = include_str!("../build.rs");
const CI_WORKFLOW: &str = include_str!("../../../.github/workflows/ci.yml");
const CHECK_SCRIPT: &str = include_str!("../../../scripts/assert-weak-linked-14_4-symbols.sh");

/// The step name the workflow has to carry, verbatim from the backlog.
const CI_STEP_NAME: &str = "ci: assert weak-linked 14.4 CoreAudio symbols";
const CHECK_SCRIPT_PATH: &str = "scripts/assert-weak-linked-14_4-symbols.sh";

/// The exact table the acceptance criterion names, plus the versions that break
/// a lexicographic comparison and the floor Yap actually ships to.
const TABLE: &[(&str, bool)] = &[
    ("12.0", false), // the deployment floor — mic-only lives here
    ("13.0", false),
    ("14.3", false),
    ("14.4", true), // the boundary, on the allowed side
    ("14.5", true),
    ("15.0", true),
    ("14.10", true), // > 14.4 numerically, < "14.4" as a string
    ("26.0", true),  // current macOS; < "14.4" as a string
];

#[test]
fn the_gate_refuses_below_14_4_and_allows_at_or_above() {
    for (text, expected_available) in TABLE {
        let os = OsVersion::parse(text).unwrap_or_else(|| panic!("'{text}' should parse"));
        let verdict = system_audio_gate(os);
        assert_eq!(
            verdict.is_available(),
            *expected_available,
            "macOS {text} → {verdict:?}"
        );
        if !*expected_available {
            assert_eq!(
                verdict,
                SystemAudioGate::RequiresMacOS14_4 { found: os },
                "the refusal has to carry the version it actually saw"
            );
        }
    }
}

/// 14.4.0 exactly is available and 14.3.9 is not — the boundary is `>=`, and it
/// is the one comparison in this module that a `>` would silently invert for
/// every user on the release the plan actually names.
#[test]
fn the_boundary_is_inclusive() {
    assert!(system_audio_gate(OsVersion::SYSTEM_AUDIO_MIN).is_available());
    assert_eq!(OsVersion::SYSTEM_AUDIO_MIN, OsVersion::new(14, 4, 0));
    assert!(system_audio_gate(OsVersion::new(14, 4, 1)).is_available());
    assert!(!system_audio_gate(OsVersion::new(14, 3, 9)).is_available());
}

/// An OS version that could not be read is not a reason to try a 14.4 API.
#[test]
fn an_unknown_os_version_fails_closed() {
    assert!(!system_audio_gate(OsVersion::UNKNOWN).is_available());
    assert!(!OsVersion::UNKNOWN.is_known());
}

/// The runtime read OS-11 names (`ProcessInfo.operatingSystemVersion`). It is
/// the whole mechanism — if this ever returned `UNKNOWN` on a real Mac, the
/// gate would refuse system audio on every machine, silently.
#[test]
#[cfg(target_os = "macos")]
fn the_runtime_read_reports_a_real_macos_version() {
    let os = OsVersion::current();
    assert!(os.is_known(), "ProcessInfo returned nothing usable: {os:?}");
    assert!(
        os >= OsVersion::new(12, 0, 0),
        "this app's floor is macOS 12; ProcessInfo said {os}"
    );
    println!("ProcessInfo.operatingSystemVersion = {os} ({os:?})");
    println!("system audio gate here = {:?}", system_audio_gate(os));
}

/// Acceptance criterion 1: the usage string ships, and it is a sentence a user
/// reads in a system alert — not a token.
#[test]
fn info_plist_declares_the_system_audio_usage_description() {
    assert!(
        INFO_PLIST.contains("<key>NSAudioCaptureUsageDescription</key>"),
        "Info.plist must declare NSAudioCaptureUsageDescription"
    );
    let description = INFO_PLIST
        .split("<key>NSAudioCaptureUsageDescription</key>")
        .nth(1)
        .and_then(|rest| rest.split("<string>").nth(1))
        .and_then(|rest| rest.split("</string>").next())
        .expect("NSAudioCaptureUsageDescription needs a <string> value");
    println!("NSAudioCaptureUsageDescription = {description}");
    assert!(
        description.len() > 40,
        "too terse for a permission alert: {description}"
    );
    assert!(
        description.contains("Yap"),
        "the alert should name the app: {description}"
    );
    assert!(description.ends_with('.'), "not a sentence: {description}");
    // The mic string is the tone to match, and both must survive together —
    // adding one must never displace the other.
    assert!(INFO_PLIST.contains("<key>NSMicrophoneUsageDescription</key>"));
    assert!(INFO_PLIST.contains("<key>NSAppleEventsUsageDescription</key>"));
}

/// Acceptance criterion 2: this item is a RUNTIME gate, so the deployment floor
/// does not move. Raising it would "fix" the launch failure by dropping every
/// macOS 12/13 user instead, which is the outcome §2.1 refuses.
#[test]
fn the_deployment_floor_is_still_macos_12() {
    assert!(
        TAURI_CONF.contains("\"minimumSystemVersion\": \"12.0\""),
        "minimumSystemVersion must stay 12.0 — YV101 is a runtime gate, not a floor bump"
    );
    let config: serde_json::Value =
        serde_json::from_str(TAURI_CONF).expect("tauri.conf.json is valid JSON");
    assert_eq!(
        config["bundle"]["macOS"]["minimumSystemVersion"].as_str(),
        Some("12.0")
    );
}

/// The launch check exists, is executable, is wired into CI under the name the
/// backlog specifies, and names every symbol it claims to guard.
///
/// This test cannot run `nm` against a release Mach-O — there is no release
/// binary during `cargo test`. It asserts the *wiring*, so the check cannot be
/// quietly unhooked; CI runs the check itself against the real binary.
#[test]
fn the_weak_link_launch_check_is_wired_into_ci() {
    assert!(
        CI_WORKFLOW.contains(CI_STEP_NAME),
        "the CI workflow must carry a step named `{CI_STEP_NAME}`"
    );
    assert!(
        CI_WORKFLOW.contains(CHECK_SCRIPT_PATH),
        "the CI step must actually invoke {CHECK_SCRIPT_PATH}"
    );
    for symbol in [
        "_AudioHardwareCreateProcessTap",
        "_AudioHardwareDestroyProcessTap",
        "_AudioHardwareCreateAggregateDevice",
        "_AudioHardwareDestroyAggregateDevice",
    ] {
        assert!(
            CHECK_SCRIPT.contains(symbol),
            "the check does not guard {symbol}"
        );
    }
    // The controls are the reason a PASS means anything: without them the
    // script passes against a stripped binary, a wrong path, or an `nm` whose
    // output format moved.
    assert!(
        CHECK_SCRIPT.contains("control 1") && CHECK_SCRIPT.contains("control 2"),
        "the check must prove it is not passing vacuously"
    );
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(CHECK_SCRIPT_PATH);
    let script = std::fs::canonicalize(&script)
        .unwrap_or_else(|e| panic!("{} is missing: {e}", script.display()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&script)
            .expect("stat the check")
            .permissions()
            .mode();
        assert!(
            mode & 0o111 != 0,
            "{} is not executable (mode {mode:o}) — the CI step would fail on it",
            script.display()
        );
    }
}

/// The other half of the launch check: the linker flag it verifies. Deleting
/// the flag and deleting the check are the same regression, so both are
/// asserted, and the comment that explains WHY is required to survive with it.
#[test]
fn coreaudio_is_weak_linked_at_build_time() {
    assert!(
        BUILD_RS.contains("-Wl,-weak_framework,CoreAudio"),
        "build.rs must weak-link CoreAudio or macOS 12/13 launches fail"
    );
    assert!(
        BUILD_RS.contains("CARGO_CFG_TARGET_OS"),
        "the flag must be macOS-scoped"
    );
}

/// The user-visible sentence, printed so a reviewer can read the copy without
/// running the app. The affordance that renders it lands with the Settings step
/// (YV102); the sentence and the verdict behind it are decided here.
#[test]
fn the_refusal_sentence_is_plain_and_says_the_version() {
    assert!(
        SYSTEM_AUDIO_REQUIREMENT.contains("14.4"),
        "{SYSTEM_AUDIO_REQUIREMENT}"
    );
    assert!(
        SYSTEM_AUDIO_REQUIREMENT.contains("System audio"),
        "{SYSTEM_AUDIO_REQUIREMENT}"
    );
    assert!(
        !SYSTEM_AUDIO_REQUIREMENT.contains('_'),
        "enum jargon leaked into user copy: {SYSTEM_AUDIO_REQUIREMENT}"
    );
    println!("system-audio gate copy (YV101): {SYSTEM_AUDIO_REQUIREMENT}");
}

/// Version parsing is the input to every comparison above, and both of these
/// orderings are backwards under the string compare a careless implementation
/// would reach for.
#[test]
fn version_ordering_is_numeric() {
    let v = |t: &str| OsVersion::parse(t).unwrap();
    assert!(v("14.10") > v("14.4"));
    assert!(v("26.0") > v("14.4"));
    assert!(v("9.0") < v("14.4"));
    assert!(v("14.4.1") > v("14.4"));
    assert_eq!(v("14"), OsVersion::new(14, 0, 0));
    for junk in ["", "macOS 14.4", "14.4.1.2", "fourteen"] {
        assert!(OsVersion::parse(junk).is_none(), "{junk:?} parsed");
    }
}
