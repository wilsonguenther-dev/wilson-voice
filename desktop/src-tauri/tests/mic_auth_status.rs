//! PERM-A — the microphone authorization contract, provable on a CI runner with
//! no microphone, no grant and no window server.
//!
//! Nothing in this file touches TCC. Anything that would is `#[ignore]`d with
//! the reason in its name, because `requestAccessForMediaType:` on an un-bundled
//! binary (which is what `cargo test` produces) is a process kill, not a failure.

use wilson_voice_lib::mic_auth::MicAuth;
use wilson_voice_lib::permissions;

/// The four values are AVFoundation's, not ours.
///
/// A silent renumber — someone reordering the variants for tidiness — turns
/// `Denied` into `Authorized` at the FFI boundary and produces a permanently
/// wrong permission screen that still compiles and still passes every other
/// test in this repo.
#[test]
fn mic_auth_discriminants_match_avfoundation() {
    assert_eq!(MicAuth::NotDetermined as isize, 0);
    assert_eq!(MicAuth::Restricted as isize, 1);
    assert_eq!(MicAuth::Denied as isize, 2);
    assert_eq!(MicAuth::Authorized as isize, 3);

    // The decode has to be a match over those same numbers, in both directions.
    for (raw, expected) in [
        (0isize, MicAuth::NotDetermined),
        (1, MicAuth::Restricted),
        (2, MicAuth::Denied),
        (3, MicAuth::Authorized),
    ] {
        assert_eq!(MicAuth::from_raw(raw), expected, "raw {raw}");
        assert_eq!(expected as isize, raw, "round trip {raw}");
    }

    // An unknown status must never become a grant.
    assert_eq!(MicAuth::from_raw(99), MicAuth::Restricted);
    assert_eq!(MicAuth::from_raw(-1), MicAuth::Restricted);

    // `microphone_ready()` is readiness, not authorization, and may only be
    // true when TCC actually says Authorized. Runs anywhere: on a runner with
    // no grant and no device both sides are false and the implication holds.
    if wilson_voice_lib::mic_auth::microphone_ready() {
        assert_eq!(
            wilson_voice_lib::mic_auth::authorization_status(),
            MicAuth::Authorized,
            "microphone_ready() must never outrun the TCC status"
        );
        assert!(wilson_voice_lib::mic_auth::input_device_present());
    }

    assert_eq!(MicAuth::NotDetermined.as_str(), "not_determined");
    assert_eq!(MicAuth::Restricted.as_str(), "restricted");
    assert_eq!(MicAuth::Denied.as_str(), "denied");
    assert_eq!(MicAuth::Authorized.as_str(), "authorized");
}

/// Split Rust source into `(header, body)` pairs, one per `fn`, where `header`
/// is the doc comments + attributes + signature that precede the opening brace.
fn functions(src: &str) -> Vec<(String, String)> {
    let bytes: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        // A function starts at "fn " that begins a token.
        if bytes[i] == 'f'
            && bytes.get(i + 1) == Some(&'n')
            && bytes.get(i + 2).is_some_and(|c| c.is_whitespace())
            && (i == 0 || !bytes[i - 1].is_alphanumeric() && bytes[i - 1] != '_')
        {
            // Header: back up over the signature and the doc block above it.
            let mut start = src[..i].rfind("\n\n").map(|p| p + 2).unwrap_or(0);
            // Never swallow a preceding function body.
            if let Some(prev) = src[..i].rfind('}') {
                if prev + 1 > start {
                    start = prev + 1;
                }
            }
            // Find the opening brace of the body (skipping `where` clauses).
            let mut j = i;
            let mut depth_angle = 0i32;
            let mut depth_paren = 0i32;
            while j < bytes.len() {
                match bytes[j] {
                    '(' => depth_paren += 1,
                    ')' => depth_paren -= 1,
                    '<' => depth_angle += 1,
                    '>' => depth_angle -= 1,
                    ';' if depth_paren == 0 => break, // a declaration, no body
                    '{' if depth_paren == 0 && depth_angle <= 0 => break,
                    _ => {}
                }
                j += 1;
            }
            if j >= bytes.len() || bytes[j] != '{' {
                i += 2;
                continue;
            }
            let header: String = src[start..j].to_string();
            // Body: brace matching from j.
            let mut depth = 0i32;
            let mut k = j;
            while k < bytes.len() {
                match bytes[k] {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                k += 1;
            }
            let body: String = src[j..(k + 1).min(src.len())].to_string();
            out.push((header, body));
            i = j + 1;
            continue;
        }
        i += 1;
    }
    out
}

const AUTHORITY_WORDS: [&str; 5] = [
    "permission",
    "authorization",
    "authorize",
    "granted",
    "authoriz",
];

/// Every function whose NAME or DOC claims to answer a permission question, and
/// whose BODY nonetheless asks cpal about the hardware.
fn offenders(src: &str) -> Vec<String> {
    functions(src)
        .into_iter()
        .filter(|(header, body)| {
            let h = header.to_lowercase();
            let claims_authority = AUTHORITY_WORDS.iter().any(|w| h.contains(w));
            claims_authority && body.contains("default_input_config")
        })
        .map(|(header, _)| {
            header
                .lines()
                .filter(|l| l.trim_start().starts_with("fn ") || l.contains(" fn "))
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string()
        })
        .collect()
}

/// The device probe is not an authorization answer, anywhere in the permission
/// modules.
///
/// Scope: `src/mic_auth.rs` + `src/permissions.rs` — the two files the app reads
/// a grant out of. Pattern: `default_input_config`, inside a function whose name
/// or doc mentions permission/authorization/granted.
///
/// The first half of this test proves the scanner can SEE the shape we fear, so
/// the zero it reports for the real files is a measurement and not a vacuous
/// pass. (A grep proving absence with the wrong pattern or the wrong scope
/// proves nothing; this one is checked against a positive control.)
#[test]
fn authorization_status_is_the_only_microphone_authority() {
    // Positive control — this is EXACTLY the code PERM-A deletes.
    let control = r#"
/// True if default input device + config are available (mic present / often authorized).
pub fn microphone_ready() -> bool {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    match host.default_input_device() {
        None => false,
        Some(d) => d.default_input_config().is_ok(),
    }
}
"#;
    let found = offenders(control);
    assert_eq!(
        found.len(),
        1,
        "scanner failed to flag the known-bad shape; it cannot prove absence. found={found:?}"
    );

    // A function that legitimately probes hardware and says so must NOT be
    // flagged, or the test would just ban cpal.
    let benign = r#"
/// HARDWARE, not permission: is a default input device attached at all?
pub fn input_device_present() -> bool {
    use cpal::traits::HostTrait;
    cpal::default_host().default_input_device().is_some()
}
"#;
    assert!(
        offenders(benign).is_empty(),
        "scanner over-matched a hardware-only probe"
    );

    // The real scope.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut scanned = 0usize;
    for rel in ["src/mic_auth.rs", "src/permissions.rs"] {
        let path = root.join(rel);
        let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{rel}: {e}"));
        assert!(
            !functions(&src).is_empty(),
            "{rel}: parsed zero functions — the scanner is broken, not the file"
        );
        scanned += 1;
        let found = offenders(&src);
        assert!(
            found.is_empty(),
            "{rel}: these functions answer a permission question with a device probe: {found:?}"
        );
    }
    assert_eq!(scanned, 2);
}

/// Only `Authorized` may set `all_critical_ok`.
///
/// This is the bug in one line: the old report set `microphone: true` from a
/// hardware probe, so a DENIED mic produced `all_critical_ok = true` and the app
/// told the user everything was fine before recording silence.
#[test]
fn report_requires_authorized_for_all_critical_ok() {
    for status in [
        MicAuth::NotDetermined,
        MicAuth::Restricted,
        MicAuth::Denied,
        MicAuth::Authorized,
    ] {
        let r = permissions::build_report(true, status, true, "model ready".into());
        assert_eq!(r.microphone_status, status.as_str());
        if status == MicAuth::Authorized {
            // On a runner with no input device, readiness (and therefore
            // all_critical_ok) legitimately stays false — authorization is
            // necessary, not sufficient. Assert the implication, both ways.
            assert_eq!(
                r.all_critical_ok, r.microphone,
                "authorized: all_critical_ok must track device readiness"
            );
        } else {
            assert!(
                !r.all_critical_ok,
                "{} must never be critical-ok",
                status.as_str()
            );
            assert!(
                !r.microphone,
                "{} must never report the microphone as ready",
                status.as_str()
            );
        }
    }

    // ASR still gates it independently.
    let no_asr = permissions::build_report(true, MicAuth::Authorized, false, "no model".into());
    assert!(!no_asr.all_critical_ok);

    // Each status gets its own next step, and none of them is the old advice.
    let mut seen = std::collections::HashSet::new();
    for status in [
        MicAuth::NotDetermined,
        MicAuth::Restricted,
        MicAuth::Denied,
        MicAuth::Authorized,
    ] {
        let detail = permissions::microphone_detail(status);
        assert!(
            !detail.contains("click Dictate once"),
            "the pre-PERM-A advice survived for {}",
            status.as_str()
        );
        assert!(
            seen.insert(detail),
            "duplicate prose for {}",
            status.as_str()
        );
    }
    assert_eq!(seen.len(), 4);
}

/// TCC-touching, therefore never run unattended: `requestAccessForMediaType:`
/// without a bundled Info.plist is a process kill, and `cargo test` builds a
/// bare Mach-O with no bundle.
#[test]
#[ignore = "touches_tcc_needs_a_bundled_app_run_manually"]
fn ignored_touches_tcc_request_access_round_trip() {
    let before = wilson_voice_lib::mic_auth::authorization_status();
    println!("status before: {}", before.as_str());
    assert!(
        wilson_voice_lib::mic_auth::usage_description_present(),
        "run this from a bundled .app — NSMicrophoneUsageDescription is missing"
    );
}

/// A RUNTIME probe of the FFI itself, not of the code around it.
///
/// "It compiles" would be satisfied by a `msg_send!` to a selector that does not
/// exist — objc2 verifies return encodings only inside `define_class` and the
/// opt-in `AnyClass::verify_sel`, so a wrong selector or an unlinked framework
/// shows up at RUN time. This calls the real class method and asserts the value
/// is one of AVFoundation's four. It prompts nothing (reading never prompts) and
/// needs no bundle, no grant and no microphone.
#[test]
fn authorization_status_returns_a_real_avfoundation_value() {
    let status = wilson_voice_lib::mic_auth::authorization_status();
    println!(
        "AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio -> {status:?} ({})",
        status.as_str()
    );
    assert!(
        matches!(
            status,
            MicAuth::NotDetermined | MicAuth::Restricted | MicAuth::Denied | MicAuth::Authorized
        ),
        "unexpected status {status:?}"
    );
    // Reading twice must not change anything — a read that prompts, or a read
    // that mutates, would show up here.
    assert_eq!(status, wilson_voice_lib::mic_auth::authorization_status());
}
