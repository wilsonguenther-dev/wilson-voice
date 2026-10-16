//! yap-perm-truth — the permission banner tells the truth about THIS binary.
//!
//! Wilson's 0.9.0 report: System Settings showed Yap's Accessibility toggle ON,
//! Yap's banner said Accessibility was off, and he trusted the toggle. The banner
//! was right — 0.9.0 changed signing identity (Apple Development → Developer ID)
//! and every TCC grant is pinned to the code signature it was given to — but it
//! never said WHICH binary it had asked about, WHAT the OS answered, or WHY a
//! visible toggle can be dead. This module carries those three facts:
//!
//! - [`RunningIdentity`]: the bundle path, bundle id and signature of the
//!   running process (`NSBundle.mainBundle`, `SecCodeCopySigningInformation`).
//! - [`failing_conditions`]: the exact OS calls that said no.
//! - [`stale_grant`]: a grant last seen working under a DIFFERENT signature.
//! - [`find_duplicate_bundles`]: another Yap.app that may be the one holding it.
//!
//! Every decision is a pure function (unit-tested in `tests/perm_truth.rs`).
//! Nothing here reads or writes TCC. The identity and the duplicate scan are
//! computed ONCE per process — a running binary's signature cannot change under
//! it — while the grants themselves are still read fresh on every report.

use crate::permissions::GrantState;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Who this permission was given to. Equality is the whole point: TCC pins a
/// grant to the code requirement, so a different authority OR cdhash OR
/// version is, for the banner's purposes, a different app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SigningSignature {
    /// Leaf certificate summary, e.g. "Developer ID Application: … (TEAMID)".
    /// `"ad-hoc"` for a signature without certificates, `"unsigned"` when the
    /// process has no readable signature at all.
    pub authority: String,
    /// Hex of `kSecCodeInfoUnique` (the code directory hash).
    pub cdhash: String,
    /// `CFBundleShortVersionString`.
    pub version: String,
}

/// The last signature each grant was observed TRUE under. Persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrustLedger {
    #[serde(default)]
    pub accessibility: Option<SigningSignature>,
    #[serde(default)]
    pub input_monitoring: Option<SigningSignature>,
}

/// One OS call that answered "no", as the banner names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailingCondition {
    /// One of `permissions::GRANT_*`.
    pub grant: &'static str,
    /// As it reads in System Settings.
    pub label: &'static str,
    /// The API that was asked, e.g. `AXIsProcessTrusted`.
    pub api: &'static str,
    /// What it answered, e.g. `false`.
    pub observed: &'static str,
    /// The canonical one-liner: "Accessibility false (AXIsProcessTrusted)".
    pub text: String,
}

/// A grant that worked for a differently signed Yap and does not work for this one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StaleGrant {
    pub grant: &'static str,
    pub label: &'static str,
    pub previous_version: String,
    /// The full sentence the banner shows. Written here and only here.
    pub message: String,
}

pub const ACCESSIBILITY_CONDITION: &str = "Accessibility false (AXIsProcessTrusted)";
pub const INPUT_MONITORING_CONDITION: &str =
    "Input Monitoring false (IOHIDCheckAccess listen = denied)";

/// Which OS answers are "no", in reading order. Only a DENIAL counts: an
/// Unknown Input Monitoring grant is silent, exactly as it is everywhere else.
pub fn failing_conditions(
    accessibility: bool,
    input_monitoring: GrantState,
) -> Vec<FailingCondition> {
    let mut out = Vec::new();
    if !accessibility {
        out.push(FailingCondition {
            grant: crate::permissions::GRANT_ACCESSIBILITY,
            label: "Accessibility",
            api: "AXIsProcessTrusted",
            observed: "false",
            text: ACCESSIBILITY_CONDITION.to_string(),
        });
    }
    if input_monitoring == GrantState::Denied {
        out.push(FailingCondition {
            grant: crate::permissions::GRANT_INPUT_MONITORING,
            label: "Input Monitoring",
            api: "IOHIDCheckAccess listen",
            observed: "denied",
            text: INPUT_MONITORING_CONDITION.to_string(),
        });
    }
    out
}

/// The stale-grant sentence. `label` is the System Settings list it lives in.
pub fn stale_grant_message(label: &str, previous_version: &str, bundle_path: &str) -> String {
    let who = if previous_version.is_empty() {
        "an earlier Yap".to_string()
    } else {
        format!("Yap {previous_version}")
    };
    format!(
        "macOS still lists Yap as allowed in {label}, but that permission was given to {who}, \
         signed differently. In System Settings → Privacy & Security → {label}, select Yap, \
         click the minus button, then click plus and choose {bundle_path}."
    )
}

/// The stale-grant DECISION, pure. Speaks only when the grant reads false NOW
/// and the last signature it read true under is not this one. No stored
/// signature → silent (we never saw it work, so we cannot say it went stale).
pub fn stale_grant(
    grant: &'static str,
    label: &'static str,
    granted_now: bool,
    stored: Option<&SigningSignature>,
    running: &SigningSignature,
    bundle_path: &str,
) -> Option<StaleGrant> {
    if granted_now {
        return None;
    }
    let old = stored?;
    if old == running {
        return None;
    }
    Some(StaleGrant {
        grant,
        label,
        previous_version: old.version.clone(),
        message: stale_grant_message(label, &old.version, bundle_path),
    })
}

/// Record what was just observed into the ledger, pure. Returns true when the
/// ledger changed (and so is worth persisting). A grant is only ever RECORDED
/// on a true read; a false read never erases the evidence of the last good one.
pub fn record_trust(
    ledger: &mut TrustLedger,
    accessibility: bool,
    input_monitoring_authorized: bool,
    running: &SigningSignature,
) -> bool {
    let mut changed = false;
    if accessibility && ledger.accessibility.as_ref() != Some(running) {
        ledger.accessibility = Some(running.clone());
        changed = true;
    }
    if input_monitoring_authorized && ledger.input_monitoring.as_ref() != Some(running) {
        ledger.input_monitoring = Some(running.clone());
        changed = true;
    }
    changed
}

/// Both stale-grant decisions for one report.
pub fn stale_grants(
    ledger: &TrustLedger,
    accessibility: bool,
    input_monitoring: GrantState,
    running: &SigningSignature,
    bundle_path: &str,
) -> Vec<StaleGrant> {
    let mut out = Vec::new();
    if let Some(s) = stale_grant(
        crate::permissions::GRANT_ACCESSIBILITY,
        "Accessibility",
        accessibility,
        ledger.accessibility.as_ref(),
        running,
        bundle_path,
    ) {
        out.push(s);
    }
    // Only a DENIED Input Monitoring grant can be stale; Unknown is silent.
    if input_monitoring == GrantState::Denied {
        if let Some(s) = stale_grant(
            crate::permissions::GRANT_INPUT_MONITORING,
            "Input Monitoring",
            false,
            ledger.input_monitoring.as_ref(),
            running,
            bundle_path,
        ) {
            out.push(s);
        }
    }
    out
}

// ── Duplicate bundles ──────────────────────────────────────────────────────

/// The string value of `key` in an XML property list, or None. Enough of a
/// parser for `CFBundleIdentifier`: the key element, whitespace, a string element.
pub fn plist_xml_string(xml: &str, key: &str) -> Option<String> {
    let needle = format!("<key>{key}</key>");
    let rest = &xml[xml.find(&needle)? + needle.len()..];
    let rest = rest.trim_start();
    let rest = rest.strip_prefix("<string>")?;
    let end = rest.find("</string>")?;
    Some(rest[..end].trim().to_string())
}

/// `CFBundleIdentifier` of a bundle on disk. XML plists are parsed in-process;
/// a binary plist (common for Apple's own apps) is handed to `plutil`, which
/// only ever happens for a bundle that is not Yap's XML-plist build.
pub fn read_bundle_id(app: &Path) -> Option<String> {
    let plist = app.join("Contents").join("Info.plist");
    let bytes = std::fs::read(&plist).ok()?;
    if bytes.starts_with(b"bplist") {
        let out = std::process::Command::new("/usr/bin/plutil")
            .args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
            .arg(&plist)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        return (!id.is_empty()).then_some(id);
    }
    plist_xml_string(&String::from_utf8_lossy(&bytes), "CFBundleIdentifier")
}

fn is_app_dir(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "app") && p.is_dir()
}

fn same_place(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Every bundle under `roots` whose `CFBundleIdentifier` is `bundle_id` and
/// whose path is not `own_path`. Scans `root/*.app` and `root/<folder>/*.app`
/// (one folder level — where people park "Yap old" copies), never deeper, and
/// never inside a bundle. Sorted, so the banner reads the same every launch.
pub fn find_duplicate_bundles(roots: &[PathBuf], bundle_id: &str, own_path: &Path) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if is_app_dir(&p) {
                candidates.push(p);
            } else if p.is_dir() {
                if let Ok(inner) = std::fs::read_dir(&p) {
                    candidates.extend(inner.flatten().map(|e| e.path()).filter(|q| is_app_dir(q)));
                }
            }
        }
    }
    let mut out: Vec<PathBuf> = candidates
        .into_iter()
        .filter(|p| !same_place(p, own_path))
        .filter(|p| read_bundle_id(p).as_deref() == Some(bundle_id))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// `/Applications` and `~/Applications`.
pub fn default_scan_roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Applications"));
    }
    roots
}

// ── The running process ────────────────────────────────────────────────────

/// The bundle id Yap ships under. NEVER renamed (TCC + data dir are keyed to it).
pub const BUNDLE_ID: &str = "com.wilsonguenther.wilson-voice";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningIdentity {
    /// `NSBundle.mainBundle.bundlePath` — the path macOS checked.
    pub bundle_path: String,
    pub bundle_id: String,
    pub signature: SigningSignature,
}

#[cfg(target_os = "macos")]
mod sec {
    use core_foundation::array::{
        CFArrayGetCount, CFArrayGetTypeID, CFArrayGetValueAtIndex, CFArrayRef,
    };
    use core_foundation::base::{CFGetTypeID, CFRelease, CFTypeRef, TCFType};
    use core_foundation::data::{CFDataGetBytePtr, CFDataGetLength, CFDataGetTypeID, CFDataRef};
    use core_foundation::dictionary::{CFDictionaryGetValue, CFDictionaryRef};
    use core_foundation::string::{CFString, CFStringRef};
    use std::ffi::c_void;

    #[link(name = "Security", kind = "framework")]
    extern "C" {
        fn SecCodeCopySelf(flags: u32, code: *mut *const c_void) -> i32;
        fn SecCodeCopySigningInformation(
            code: *const c_void,
            flags: u32,
            info: *mut CFDictionaryRef,
        ) -> i32;
        fn SecCertificateCopySubjectSummary(cert: *const c_void) -> CFStringRef;
        static kSecCodeInfoCertificates: CFStringRef;
        static kSecCodeInfoUnique: CFStringRef;
    }
    /// `kSecCSSigningInformation`.
    const K_SEC_CS_SIGNING_INFORMATION: u32 = 1 << 1;

    /// (leaf certificate summary | "ad-hoc" | "unsigned", cdhash hex).
    pub fn self_signing() -> (String, String) {
        // SAFETY: every CF object below is either borrowed from `info` (Get rule)
        // or released exactly once (Create/Copy rule).
        unsafe {
            let mut code: *const c_void = std::ptr::null();
            if SecCodeCopySelf(0, &mut code) != 0 || code.is_null() {
                return ("unsigned".into(), String::new());
            }
            let mut info: CFDictionaryRef = std::ptr::null();
            let status =
                SecCodeCopySigningInformation(code, K_SEC_CS_SIGNING_INFORMATION, &mut info);
            CFRelease(code as CFTypeRef);
            if status != 0 || info.is_null() {
                return ("unsigned".into(), String::new());
            }
            let mut authority = "ad-hoc".to_string();
            let certs = CFDictionaryGetValue(info, kSecCodeInfoCertificates as *const c_void);
            if !certs.is_null()
                && CFGetTypeID(certs) == CFArrayGetTypeID()
                && CFArrayGetCount(certs as CFArrayRef) > 0
            {
                let leaf = CFArrayGetValueAtIndex(certs as CFArrayRef, 0);
                let summary = SecCertificateCopySubjectSummary(leaf);
                if !summary.is_null() {
                    authority = CFString::wrap_under_create_rule(summary).to_string();
                }
            }
            let mut cdhash = String::new();
            let unique = CFDictionaryGetValue(info, kSecCodeInfoUnique as *const c_void);
            if !unique.is_null() && CFGetTypeID(unique) == CFDataGetTypeID() {
                let len = CFDataGetLength(unique as CFDataRef);
                let ptr = CFDataGetBytePtr(unique as CFDataRef);
                if !ptr.is_null() && len > 0 {
                    for b in std::slice::from_raw_parts(ptr, len as usize) {
                        cdhash.push_str(&format!("{b:02x}"));
                    }
                }
            }
            CFRelease(info as CFTypeRef);
            (authority, cdhash)
        }
    }

    /// (bundlePath, bundleIdentifier, CFBundleShortVersionString).
    pub fn main_bundle() -> (String, Option<String>, Option<String>) {
        use objc2_foundation::{NSBundle, NSString};
        let bundle = NSBundle::mainBundle();
        let path = bundle.bundlePath().to_string();
        let id = bundle.bundleIdentifier().map(|s| s.to_string());
        let version = bundle
            .objectForInfoDictionaryKey(&NSString::from_str("CFBundleShortVersionString"))
            .and_then(|o| o.downcast::<NSString>().ok())
            .map(|s| s.to_string());
        (path, id, version)
    }
}

#[cfg(not(target_os = "macos"))]
mod sec {
    pub fn self_signing() -> (String, String) {
        ("unsigned".into(), String::new())
    }
    pub fn main_bundle() -> (String, Option<String>, Option<String>) {
        let path = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        (path, None, None)
    }
}

/// The running process's identity. Computed once; it cannot change under us.
pub fn running_identity() -> &'static RunningIdentity {
    static ID: OnceLock<RunningIdentity> = OnceLock::new();
    ID.get_or_init(|| {
        let (bundle_path, bundle_id, version) = sec::main_bundle();
        let (authority, cdhash) = sec::self_signing();
        RunningIdentity {
            bundle_path,
            // A `cargo run` dev binary is not inside a bundle and has no id;
            // it still answers to the shipped id for the duplicate scan.
            bundle_id: bundle_id.unwrap_or_else(|| BUNDLE_ID.to_string()),
            signature: SigningSignature {
                authority,
                cdhash,
                version: version.unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string()),
            },
        }
    })
}

/// Other bundles claiming our id. Scanned ONCE per launch (see [`prime`]).
pub fn duplicate_bundles() -> &'static [String] {
    static DUPS: OnceLock<Vec<String>> = OnceLock::new();
    DUPS.get_or_init(|| {
        let me = running_identity();
        find_duplicate_bundles(
            &default_scan_roots(),
            &me.bundle_id,
            Path::new(&me.bundle_path),
        )
        .into_iter()
        .map(|p| p.display().to_string())
        .collect()
    })
}

/// Launch-time warm-up, off the main thread: the signature read and the
/// directory scan happen once, before the first report needs them.
pub fn prime() {
    let _ = std::thread::Builder::new()
        .name("perm-truth-prime".into())
        .spawn(|| {
            let me = running_identity();
            let dups = duplicate_bundles();
            log::info!(
                "perm-truth: running {} ({}, {} {}), {} duplicate bundle(s): {:?}",
                me.bundle_path,
                me.bundle_id,
                me.signature.authority,
                me.signature.version,
                dups.len(),
                dups
            );
        });
}

const LEDGER_FILE: &str = "permission-signature.json";

fn ledger_path() -> PathBuf {
    crate::data_dir().join(LEDGER_FILE)
}

/// Fold one fresh grant reading into the persisted ledger and return the stale
/// grants it reveals. The ledger is loaded once and written only on change, so
/// a 5 s poll never touches the disk while nothing moves.
pub fn observe(
    accessibility: bool,
    input_monitoring_authorized: bool,
    input_monitoring: GrantState,
) -> Vec<StaleGrant> {
    static LEDGER: Mutex<Option<TrustLedger>> = Mutex::new(None);
    let me = running_identity();
    let mut guard = match LEDGER.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let ledger = guard.get_or_insert_with(|| {
        std::fs::read(ledger_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    });
    // Stale is decided against the ledger as it stood BEFORE this reading, then
    // the reading is recorded (a true read makes the running signature current).
    let stale = stale_grants(
        ledger,
        accessibility,
        input_monitoring,
        &me.signature,
        &me.bundle_path,
    );
    if record_trust(
        ledger,
        accessibility,
        input_monitoring_authorized,
        &me.signature,
    ) {
        if let Ok(json) = serde_json::to_vec_pretty(&*ledger) {
            if let Err(e) = std::fs::write(ledger_path(), json) {
                log::warn!("perm-truth: could not persist {LEDGER_FILE}: {e}");
            }
        }
    }
    stale
}
