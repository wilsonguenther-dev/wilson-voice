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
//! - [`duplicate_copies_notice`]: yap-fx-08 — the one-time "Two copies of Yap
//!   are installed" question, naming the running copy and the Microphone holder.
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
/// yap-test-channel-product-name: the row it names is the bundle's own
/// ([`product_name_from_bundle_path`] of the path it tells the user to choose).
pub fn stale_grant_message(label: &str, previous_version: &str, bundle_path: &str) -> String {
    let product = product_name_from_bundle_path(bundle_path);
    let who = if previous_version.is_empty() {
        format!("an earlier {product}")
    } else {
        format!("{product} {previous_version}")
    };
    format!(
        "macOS still lists {product} as allowed in {label}, but that permission was given to {who}, \
         signed differently. In System Settings → Privacy & Security → {label}, select {product}, \
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
/// TC-1: follows the build channel (`.test` suffix on "Yap Test").
pub const BUNDLE_ID: &str = crate::channel::current().identifier;

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
/// The product name the UI says about permissions: the running bundle's file
/// stem ("Yap Test" for `/Applications/Yap Test.app`), because that is the
/// name System Settings lists the grant under. Pure. Anything that is not an
/// `.app` path (an unbundled dev binary) is "Yap".
pub fn product_name_from_bundle_path(bundle_path: &str) -> String {
    let leaf = bundle_path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    match leaf.strip_suffix(".app") {
        Some(stem) if !stem.trim().is_empty() => stem.to_string(),
        _ => "Yap".to_string(),
    }
}

/// [`product_name_from_bundle_path`] for this process.
pub fn product_name() -> String {
    product_name_from_bundle_path(&running_identity().bundle_path)
}

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

/// Other bundles claiming our id. Scanned ONCE per launch (see [`prime`]):
/// the `/Applications` + `~/Applications` folder scan, joined with every copy
/// LaunchServices has registered under our id (yap-fx-08 — that is how the
/// Desktop copy is found without listing the Desktop, which would raise the
/// macOS "access files in your Desktop folder" prompt). A copy the user moved
/// to the Trash from the duplicate notice is dropped via [`forget_duplicates`].
pub fn duplicate_bundles() -> Vec<String> {
    let mut guard = match DUPS.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    guard
        .get_or_insert_with(|| {
            let me = running_identity();
            let own = Path::new(&me.bundle_path);
            let scanned = find_duplicate_bundles(&default_scan_roots(), &me.bundle_id, own);
            merge_registered_copies(scanned, registered_copies(&me.bundle_id), own)
                .into_iter()
                .map(|p| p.display().to_string())
                .collect()
        })
        .clone()
}

static DUPS: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// Drop copies that no longer exist (moved to the Trash from the duplicate
/// notice) from the per-launch cache, so the banner stops naming them.
pub fn forget_duplicates(gone: &[String]) {
    let mut guard = match DUPS.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if let Some(list) = guard.as_mut() {
        list.retain(|p| !gone.contains(p));
    }
}

/// Is a LaunchServices-registered path a copy a PERSON installed? Pure. Build
/// products (`target/`, `DerivedData/`), Trash contents, mounted disk images
/// (`/Volumes/…`), Gatekeeper's translocation mounts and anything nested
/// inside another bundle are registered too, but they are not copies to warn
/// about — on a dev Mac every cargo build would otherwise count.
pub fn is_user_copy_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    if !s.ends_with(".app") || s.starts_with("/Volumes/") {
        return false;
    }
    let comps: Vec<String> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    let parents = &comps[..comps.len().saturating_sub(1)];
    !parents.iter().any(|c| {
        matches!(
            c.as_str(),
            ".Trash" | ".Trashes" | "target" | "DerivedData" | "AppTranslocation"
        ) || c.ends_with(".app")
    })
}

/// The folder scan joined with the LaunchServices list, pure apart from an
/// existence check on the registered paths (LaunchServices keeps entries for
/// deleted bundles). Never the running bundle, never a non-user path, sorted
/// and deduplicated so the banner and the notice read the same every launch.
pub fn merge_registered_copies(
    scanned: Vec<PathBuf>,
    registered: Vec<PathBuf>,
    own_path: &Path,
) -> Vec<PathBuf> {
    let mut out = scanned;
    for p in registered {
        if !is_user_copy_path(&p) || same_place(&p, own_path) || !p.is_dir() {
            continue;
        }
        if out.iter().any(|q| same_place(q, &p)) {
            continue;
        }
        out.push(p);
    }
    out.sort();
    out.dedup();
    out
}

/// Every bundle LaunchServices knows under `bundle_id`
/// (`NSWorkspace URLsForApplicationsWithBundleIdentifier:`, macOS 12+). Lists
/// no folder, so it raises no Files & Folders prompt.
#[cfg(target_os = "macos")]
fn registered_copies(bundle_id: &str) -> Vec<PathBuf> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;
    let urls = NSWorkspace::sharedWorkspace()
        .URLsForApplicationsWithBundleIdentifier(&NSString::from_str(bundle_id));
    urls.iter().filter_map(|u| u.to_file_path()).collect()
}

#[cfg(not(target_os = "macos"))]
fn registered_copies(_bundle_id: &str) -> Vec<PathBuf> {
    Vec::new()
}

// ── yap-fx-08 · the one-time duplicate-copies notice ───────────────────────

/// What the notice asks and what "Move to Trash" would do. Built by
/// [`duplicate_copies_notice`], pure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateCopiesNotice {
    /// The question, e.g. "Two copies of Yap are installed — keep
    /// /Applications/Yap.app and move the other to the Trash?"
    pub title: String,
    /// Which copy is running and which one holds the Microphone permission.
    pub detail: String,
    /// The copy that stays.
    pub keep: String,
    /// The copies "Move to Trash" moves, in order.
    pub to_trash: Vec<String>,
    /// `NSBundle.mainBundle.bundlePath` of this process.
    pub running: String,
    /// Every copy, sorted and newline-joined: the notice shows once per set.
    pub fingerprint: String,
}

/// Where the shipped app lives: `/Applications/<product>.app`.
pub fn canonical_bundle_path(product: &str) -> String {
    format!("/Applications/{product}.app")
}

fn count_word(n: usize) -> String {
    match n {
        2 => "Two".into(),
        3 => "Three".into(),
        4 => "Four".into(),
        5 => "Five".into(),
        n => n.to_string(),
    }
}

/// The microphone sentence. Yap never reads TCC, so it states what macOS told
/// THIS process and, when that is not "allowed", which other copy the grant
/// may sit with — a TCC grant follows the bundle id AND the signature.
pub fn microphone_holder_sentence(
    mic: crate::mic_auth::MicAuth,
    running: &str,
    others: &[String],
) -> String {
    use crate::mic_auth::MicAuth;
    let others = others.join(", ");
    match mic {
        MicAuth::Authorized => {
            format!("The Microphone permission is held by the running copy ({running}).")
        }
        MicAuth::Denied => format!(
            "The running copy is denied the Microphone; if you allowed it before, that permission is held by the other copy ({others})."
        ),
        MicAuth::NotDetermined => format!(
            "The running copy has not been given the Microphone yet; if you allowed it before, that permission is held by the other copy ({others})."
        ),
        MicAuth::Restricted => {
            "The Microphone is restricted by a device policy for every copy.".to_string()
        }
    }
}

/// The notice DECISION, pure. `None` when there is nothing to warn about: no
/// other copy, or a running binary that is not an `.app` bundle (a `cargo run`
/// dev build). The copy to keep is `/Applications/<product>.app` when it is one
/// of the copies, otherwise the running one. The running copy can be among the
/// copies to trash (run from the Desktop while /Applications holds the real
/// one): macOS lets a running bundle move to the Trash, and it keeps working
/// until it quits — the detail says so.
pub fn duplicate_copies_notice(
    running: &str,
    duplicates: &[String],
    product: &str,
    mic: crate::mic_auth::MicAuth,
) -> Option<DuplicateCopiesNotice> {
    let running = running.trim_end_matches('/');
    if !running.ends_with(".app") {
        return None;
    }
    let others: Vec<String> = duplicates
        .iter()
        .map(|d| d.trim_end_matches('/').to_string())
        .filter(|d| d != running)
        .collect();
    if others.is_empty() {
        return None;
    }
    let mut all: Vec<String> = others.clone();
    all.push(running.to_string());
    all.sort();
    all.dedup();
    let canonical = canonical_bundle_path(product);
    let keep = if all.contains(&canonical) {
        canonical
    } else {
        running.to_string()
    };
    let to_trash: Vec<String> = all.iter().filter(|p| **p != keep).cloned().collect();
    let other_word = if to_trash.len() == 1 {
        "the other"
    } else {
        "the others"
    };
    let title = format!(
        "{} copies of {product} are installed — keep {keep} and move {other_word} to the Trash?",
        count_word(all.len())
    );
    let mut detail = format!("Running now: {running}.");
    if keep != running {
        detail.push_str(&format!(
            " This running copy moves to the Trash too; it keeps working until you quit it, then open {keep}."
        ));
    }
    detail.push(' ');
    detail.push_str(&microphone_holder_sentence(mic, running, &others));
    Some(DuplicateCopiesNotice {
        title,
        detail,
        keep,
        to_trash,
        running: running.to_string(),
        fingerprint: all.join("\n"),
    })
}

/// `settings_kv` key holding the fingerprint of the copy set last warned about.
pub const DUPLICATE_NOTICE_KEY: &str = "perm_truth.duplicate_notice_fingerprint";

/// One-time rule, pure: show unless this exact set of copies was already shown.
/// A NEW set (a third copy appears later) is a new situation and is shown once.
pub fn should_show_duplicate_notice(stored: Option<&str>, notice: &DuplicateCopiesNotice) -> bool {
    stored != Some(notice.fingerprint.as_str())
}

/// The sentence shown when a copy could not be moved: one line, one action.
pub fn trash_failed_sentence(path: &str) -> String {
    format!("Yap could not move {path} to the Trash — drag it to the Trash in Finder.")
}

/// Launch-time, off the main thread: wait for the window to settle, decide,
/// and — once per set of copies, ever — ask on an app-modal alert. The
/// fingerprint is written BEFORE the alert shows, so a crash or a force-quit
/// on top of it never turns "one-time" into "every launch".
pub fn spawn_duplicate_copies_notice(
    app: tauri::AppHandle,
    db: std::sync::Arc<crate::db::Database>,
) {
    let _ = std::thread::Builder::new()
        .name("perm-truth-duplicates".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(4));
            let me = running_identity();
            let dups = duplicate_bundles();
            let product = crate::channel::current().product_name;
            let mic = crate::mic_auth::authorization_status();
            let Some(notice) = duplicate_copies_notice(&me.bundle_path, &dups, product, mic) else {
                return;
            };
            let stored = db.setting_get(DUPLICATE_NOTICE_KEY);
            if !should_show_duplicate_notice(stored.as_deref(), &notice) {
                log::info!(
                    "perm_truth duplicate_notice skipped reason=already_shown copies={}",
                    notice.to_trash.len() + 1
                );
                return;
            }
            if let Err(e) = db.setting_set(DUPLICATE_NOTICE_KEY, &notice.fingerprint) {
                log::warn!("perm_truth duplicate_notice persist_failed err={e}");
            }
            log::info!(
                "perm_truth duplicate_notice shown copies={} running_is_keep={} mic={}",
                notice.to_trash.len() + 1,
                notice.keep == notice.running,
                mic.as_str()
            );
            let Some(trash) = ask_on_main(&app, &notice.title, &notice.detail, true) else {
                log::warn!("perm_truth duplicate_notice failed reason=main_thread");
                return;
            };
            log::info!(
                "perm_truth duplicate_notice choice={}",
                if trash { "trash" } else { "keep" }
            );
            if !trash {
                return;
            }
            let started = std::time::Instant::now();
            let mut moved: Vec<String> = Vec::new();
            let mut failed: Vec<String> = Vec::new();
            for path in &notice.to_trash {
                match move_to_trash(path) {
                    Ok(()) => moved.push(path.clone()),
                    Err(e) => {
                        log::warn!("perm_truth duplicate_trash failed err={e}");
                        failed.push(path.clone());
                    }
                }
            }
            log::info!(
                "perm_truth duplicate_trash moved={} failed={} ms={}",
                moved.len(),
                failed.len(),
                started.elapsed().as_millis()
            );
            forget_duplicates(&moved);
            if !moved.is_empty() {
                crate::permissions::poke(crate::permissions::WatchReason::Focus);
            }
            if let Some(first) = failed.first() {
                let _ = ask_on_main(&app, &trash_failed_sentence(first), "", false);
            }
        });
}

/// Run an app-modal NSAlert on the main thread and wait for the answer.
/// `question` = two buttons ("Move to Trash" / "Not Now", `Some(true)` for the
/// first); otherwise one "OK" button. `None` when the main thread is gone.
#[cfg(target_os = "macos")]
fn ask_on_main(app: &tauri::AppHandle, title: &str, detail: &str, question: bool) -> Option<bool> {
    let (tx, rx) = std::sync::mpsc::channel();
    let title = title.to_string();
    let detail = detail.to_string();
    app.run_on_main_thread(move || {
        use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn};
        use objc2_foundation::NSString;
        // Dropping `tx` unsent reads as "no answer" (None) on the other side.
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            return;
        };
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(&title));
        alert.setInformativeText(&NSString::from_str(&detail));
        if question {
            let _ = alert.addButtonWithTitle(&NSString::from_str("Move to Trash"));
            let _ = alert.addButtonWithTitle(&NSString::from_str("Not Now"));
        } else {
            let _ = alert.addButtonWithTitle(&NSString::from_str("OK"));
        }
        let answer = alert.runModal();
        let _ = tx.send(question && answer == NSAlertFirstButtonReturn);
    })
    .ok()?;
    rx.recv().ok()
}

#[cfg(not(target_os = "macos"))]
fn ask_on_main(
    _app: &tauri::AppHandle,
    _title: &str,
    _detail: &str,
    _question: bool,
) -> Option<bool> {
    None
}

/// `-[NSFileManager trashItemAtURL:resultingItemURL:error:]` — the Finder's
/// own "Move to Trash" (recoverable with Put Back), never a delete.
#[cfg(target_os = "macos")]
fn move_to_trash(path: &str) -> Result<(), String> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, None)
        // The error CODE only: its description names the bundle, and log
        // lines never carry paths.
        .map_err(|e| format!("nserror_code={}", e.code()))
}

#[cfg(not(target_os = "macos"))]
fn move_to_trash(_path: &str) -> Result<(), String> {
    Err("move to Trash is macOS-only".into())
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

static LEDGER: Mutex<Option<TrustLedger>> = Mutex::new(None);

fn load_ledger() -> TrustLedger {
    std::fs::read(ledger_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// The ledger as it stands now (loaded once per process). `perm_probe` reads
/// it to classify a denied grant as StaleIdentity.
pub fn ledger_snapshot() -> TrustLedger {
    let mut guard = match LEDGER.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    guard.get_or_insert_with(load_ledger).clone()
}

/// `NSBundle.mainBundle.bundleIdentifier` at runtime — `None` for an unbundled
/// binary. Never defaulted to [`BUNDLE_ID`]: a repair must only ever target the
/// bundle that is actually running.
pub fn main_bundle_id() -> Option<String> {
    sec::main_bundle().1
}

/// Fold one fresh grant reading into the persisted ledger and return the stale
/// grants it reveals. The ledger is loaded once and written only on change, so
/// a 5 s poll never touches the disk while nothing moves.
pub fn observe(
    accessibility: bool,
    input_monitoring_authorized: bool,
    input_monitoring: GrantState,
) -> Vec<StaleGrant> {
    let me = running_identity();
    let mut guard = match LEDGER.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let ledger = guard.get_or_insert_with(load_ledger);
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
                diag_warn!("perm_truth", "perm-truth: could not persist {file}: {err}", file = LEDGER_FILE, err = e);
            }
        }
    }
    stale
}
