//! macOS TCC / trust helpers for Wilson Voice.
//!
//! Identity that must appear in System Settings is the **app bundle**:
//!   com.wilsonguenther.wilson-voice  ("Wilson Voice")
//! Yap execs no helper interpreter, so that bundle is the ONLY row users ever
//! need to enable.

use crate::mic_auth::MicAuth;
use serde::Serialize;
use std::process::Command;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

/// PERM-E — one grant, tri-state.
///
/// A bool cannot carry the third answer, and the third answer is the whole
/// item: system audio capture has **no readable status at all** (see
/// [`audio_capture_state`]), and `IOHIDCheckAccess` has its own
/// `kIOHIDAccessTypeUnknown`. `Unknown` is a first-class, NON-NAGGING state —
/// it is never rendered as a problem, never polled at, and never inferred into
/// a grant. Reporting an unreadable grant as denied is the same lie PERM-A
/// exists to delete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantState {
    Authorized,
    Denied,
    Unknown,
}

impl GrantState {
    /// The wire form the frontend branches on.
    pub fn as_str(self) -> &'static str {
        match self {
            GrantState::Authorized => "authorized",
            GrantState::Denied => "denied",
            GrantState::Unknown => "unknown",
        }
    }

    /// May this state put a line in front of the user? ONLY a denial may.
    ///
    /// `Unknown` deliberately returns false: a permanent banner for a grant
    /// nobody can read is how a good app becomes nagware.
    pub fn nags(self) -> bool {
        matches!(self, GrantState::Denied)
    }
}

/// Stable keys for the four grants. The frontend matches on these, so they are
/// constants rather than literals sprinkled through two languages.
pub const GRANT_MICROPHONE: &str = "microphone";
pub const GRANT_ACCESSIBILITY: &str = "accessibility";
pub const GRANT_INPUT_MONITORING: &str = "input_monitoring";
pub const GRANT_AUDIO_CAPTURE: &str = "audio_capture";

/// One row of the single permission surface.
///
/// PANEL (4): these four rows ARE the onboarding checklist's rows. One
/// component, one source — `PermissionHealthRow.tsx` renders them and the
/// onboarding step renders the same array, so the two can never disagree.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantRow {
    /// One of the `GRANT_*` constants.
    pub key: &'static str,
    /// Sentence-case name, as it reads in System Settings.
    pub label: &'static str,
    pub status: GrantState,
    /// The argument `open_privacy_pane` matches on. The frontend passes a PANE
    /// NAME and never a URL — the deep link is verified on the Rust side.
    pub pane: &'static str,
    /// One sentence: what happens next, not what went wrong.
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReport {
    /// AXIsProcessTrusted — required for enigo Cmd+V paste
    pub accessibility: bool,
    /// TCC says Authorized **and** an input device is attached. Kept for the
    /// existing call sites; `microphone_status` is what the UI branches on.
    pub microphone: bool,
    /// The raw AVFoundation authorization status:
    /// `"not_determined" | "denied" | "restricted" | "authorized"`.
    ///
    /// A bool cannot tell "never asked" from "the user said no", and those two
    /// need opposite UI: one shows a prompt button, the other a Settings link.
    pub microphone_status: String,
    /// PERM-E — `IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)`, the grant the
    /// modifier-only fn / fn⌃ hold in `ptt_macos.rs` actually runs on.
    pub input_monitoring: GrantState,
    /// PERM-E — system audio capture. UNKNOWN BY CONSTRUCTION; see
    /// [`audio_capture_state`].
    pub audio_capture: GrantState,
    /// The four grants, in the order a human should read them. THE surface.
    pub grants: Vec<GrantRow>,
    /// A speech model is on disk, so the embedded engine can transcribe
    pub asr_ok: bool,
    pub asr_detail: String,
    /// Human summary for UI
    pub summary: String,
    pub all_critical_ok: bool,
    /// yap-perm-truth — `NSBundle.mainBundle.bundlePath` of the process that
    /// asked. The banner names it, because "Accessibility is off" without
    /// "for WHICH Yap" is how a user ends up trusting a stale toggle instead.
    pub checked_bundle_path: String,
    /// Leaf certificate summary from `SecCodeCopySigningInformation` on self.
    pub signing_authority: String,
    /// Each OS call that answered no, e.g. "Accessibility false (AXIsProcessTrusted)".
    pub failing_conditions: Vec<crate::perm_truth::FailingCondition>,
    /// Grants last seen working under a different signature than this one.
    pub stale_grants: Vec<crate::perm_truth::StaleGrant>,
    /// Other bundles on disk with our bundle id, any of which may hold the grant.
    pub duplicate_bundles: Vec<String>,
    /// yap-p0-perm-repair — the live per-service probe, with StaleIdentity.
    pub probe: crate::perm_probe::ProbeReport,
    /// fix-ax-probe-refresh — the name every permission line uses ("Yap Test"
    /// on the test channel): the running bundle's name, as System Settings lists it.
    pub product_name: String,
}

// yap-p0-perm-repair: every grant read goes through `perm_probe` — the ONE
// module holding grant FFI (AX, CGPreflight*, IOHID). `permissions.rs` keeps
// none of its own.

/// The microphone answer, read from macOS rather than inferred from hardware.
///
/// PERM-A: this used to ask `cpal` whether the default input device would hand
/// over a stream config and report that success as "mic ready", which is blind
/// to a revoked grant — and the prose it printed ("If Yap is missing from System Settings → Microphone, click Dictate
/// once to trigger the prompt") was advice built on that wrong model. macOS only
/// shows the prompt from `NotDetermined`; clicking Dictate after a denial does
/// nothing at all. Each status now gets the one next step that actually works.
pub fn microphone_probe() -> (bool, String) {
    let status = crate::mic_auth::authorization_status();
    (
        status == MicAuth::Authorized,
        microphone_detail(status, &crate::perm_truth::product_name()),
    )
}

/// Prose per status. Pure, so `tests/mic_auth_status.rs` can read it without a
/// microphone, a grant, or a window server. `product` is the name System
/// Settings lists the grant under (`perm_truth::product_name`, "Yap Test" on the
/// test channel) — the row the user has to find.
pub fn microphone_detail(status: MicAuth, product: &str) -> String {
    match status {
        MicAuth::Authorized => {
            if crate::mic_auth::input_device_present() {
                format!("Microphone authorized for {product}.")
            } else {
                "Microphone authorized, but no input device is attached — plug one in or pick one in System Settings → Sound.".into()
            }
        }
        MicAuth::NotDetermined => {
            "macOS has not been asked for the microphone yet — click Request Microphone to show the system prompt.".into()
        }
        MicAuth::Denied => {
            format!("Microphone is DENIED for {product}. macOS will not ask again — turn {product} on in System Settings → Privacy & Security → Microphone.")
        }
        MicAuth::Restricted => {
            format!("Microphone is restricted by a device policy (Screen Time or MDM). {product} cannot request it; an administrator has to allow it.")
        }
    }
}

/// Can this install transcribe? YV34: the embedded GGUF engine is the ONLY
/// path, so a downloaded model (`native_ready`) is the whole answer. Purely a
/// value check — it spawns nothing, which is what makes the 1.2 s onboarding
/// poll safe (the old probe exec'd an interpreter on every tick).
pub fn asr_probe(native_ready: bool) -> (bool, String) {
    if native_ready {
        (
            true,
            "Speech model downloaded — embedded engine ready".into(),
        )
    } else {
        (
            false,
            "No speech model on disk yet — click Get a speech model.".into(),
        )
    }
}

pub fn report(prompt_accessibility: bool, native_ready: bool) -> PermissionReport {
    if prompt_accessibility {
        let _ = crate::perm_probe::request_accessibility_prompt();
    }
    // yap-p0-perm-repair: ONE probe per report. The banner, onboarding and the
    // hotkey degraded line all read this same value.
    let probe = crate::perm_probe::probe_all();
    let (asr_ok, asr_detail) = asr_probe(native_ready);
    // yap-test-channel-product-name: every line is BUILT with the running
    // product's name (the TCC row System Settings lists), never written as
    // "Yap" and renamed afterwards.
    let product = crate::perm_truth::product_name();
    log_product_name_once(&product);
    let mut report = report_from_probe(
        probe,
        tap_is_delivering(),
        audio_capture_state(),
        asr_ok,
        asr_detail,
        &product,
    );
    // yap-perm-truth: WHICH binary was asked, and what exactly it was told.
    let me = crate::perm_truth::running_identity();
    report.checked_bundle_path = me.bundle_path.clone();
    report.signing_authority = me.signature.authority.clone();
    report.failing_conditions =
        crate::perm_truth::failing_conditions(report.accessibility, report.input_monitoring);
    report.stale_grants = crate::perm_truth::observe(
        report.accessibility,
        report.probe.listen_event_hid == GrantState::Authorized,
        report.input_monitoring,
    );
    report.duplicate_bundles = crate::perm_truth::duplicate_bundles().to_vec();
    report
}

/// yap-test-channel-product-name — one INFO line per process naming the
/// product every permission line is built with. `report()` runs on a 5 s poll
/// while the banner is up, so this logs the first time only. The product is the
/// bundle's file stem, never a full path.
fn log_product_name_once(product: &str) {
    static LOGGED: std::sync::Once = std::sync::Once::new();
    LOGGED.call_once(|| {
        log::info!(
            "permissions product_name product={product:?} source=bundle_stem test_channel={}",
            product != "Yap"
        );
    });
}

/// [`name_product`] for THIS process: the one way a permission sentence built
/// outside [`report`] (a notification, a record error, a paste card, the fn
/// tap's blocked reason) names the running product. Same source of truth as
/// `PermissionReport.product_name` (`perm_truth::product_name`). A no-op in
/// the shipped "Yap" build.
pub fn named(text: &str) -> String {
    name_product(text, &crate::perm_truth::product_name())
}

/// Replace every whole-word "Yap" in `text` with `product`, idempotently: an
/// occurrence that already reads as `product` (main's #249 names Input
/// Monitoring and stale-grant lines at the source, "Yap Test") is left as-is,
/// so "Yap Test" never becomes "Yap Test Test". Pure; a no-op for "Yap".
pub fn name_product(text: &str, product: &str) -> String {
    let product = product.trim();
    if product.is_empty() || product == "Yap" {
        return text.to_string();
    }
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut out = String::with_capacity(text.len() + 16);
    let mut i = 0;
    while let Some(off) = text[i..].find("Yap") {
        let at = i + off;
        out.push_str(&text[i..at]);
        let rest = &text[at..];
        let before = text[..at].chars().next_back();
        let after = rest["Yap".len()..].chars().next();
        if rest.starts_with(product) {
            // Already named: copy the product through untouched.
            out.push_str(product);
            i = at + product.len();
        } else if word(before) || word(after) {
            // Part of a longer word ("Yapper"): not the product name.
            out.push_str("Yap");
            i = at + "Yap".len();
        } else {
            out.push_str(product);
            i = at + "Yap".len();
        }
    }
    out.push_str(&text[i..]);
    out
}

/// The report from one [`crate::perm_probe::ProbeReport`], pure: the grant rows
/// onboarding renders and the `probe` field the banner's Repair renders are the
/// same reading, so the two can never disagree.
pub fn report_from_probe(
    probe: crate::perm_probe::ProbeReport,
    tap_delivering: bool,
    audio_capture: GrantState,
    asr_ok: bool,
    asr_detail: String,
    product: &str,
) -> PermissionReport {
    // yap-dev-perm-probe-attribution: `usable()` — a host-attributed answer still
    // drives what the process can DO; the UI reads the state to say whose it is.
    let accessibility = probe.accessibility.usable();
    // The grant row reads the SAME combined ListenEvent answer the banner reads
    // (CGPreflightListenEventAccess OR IOHIDCheckAccess), not IOHID alone.
    let listen_hid = if probe.listen_event.raw == crate::perm_probe::RawAnswer::Granted {
        GrantState::Authorized
    } else {
        probe.listen_event_hid
    };
    let input_monitoring = input_monitoring_state(listen_hid, accessibility, tap_delivering);
    let mut probe = probe;
    reconcile_listen_event(&mut probe.listen_event, input_monitoring, product);
    let mut report = build_report_with_grants(
        accessibility,
        probe.microphone_auth,
        input_monitoring,
        audio_capture,
        asr_ok,
        asr_detail,
        product,
    );
    report.probe = probe;
    report
}

/// Bring the banner's ListenEvent row into line with the Input Monitoring
/// grant row, so the two surfaces can never disagree.
///
/// The grant row applies the pinned product rule (**a working tap is never
/// reported blocked**; a HID denial under a live Accessibility grant is
/// Unknown). The fn tap is listen-only and runs on Accessibility alone, so a
/// raw ListenEvent "no" under either of those conditions is not a broken path
/// and must not get a Repair button. Only a grant row of Denied keeps the
/// probe's repairable state (Denied / StaleIdentity). The raw answers
/// (`raw`, `listen_event_cg`, `listen_event_hid`) are left untouched.
pub fn reconcile_listen_event(
    listen: &mut crate::perm_probe::ServiceProbe,
    input_monitoring: GrantState,
    product: &str,
) {
    use crate::perm_probe::ProbeState;
    // yap-dev-perm-probe-attribution: a host-attributed row stays host-attributed;
    // turning it Granted here is exactly the green row this item removes.
    if listen.state == ProbeState::DevHostAttributed {
        return;
    }
    let repairable = matches!(listen.state, ProbeState::Denied | ProbeState::StaleIdentity);
    let (state, why) = match input_monitoring {
        GrantState::Authorized if listen.state != ProbeState::Granted => (
            ProbeState::Granted,
            format!("the fn tap is delivering, so Input Monitoring is not blocking {product}"),
        ),
        GrantState::Unknown if repairable => (
            ProbeState::Unknown,
            "Accessibility is on, which is enough for the listen-only fn tap".to_string(),
        ),
        _ => return,
    };
    listen.state = state;
    listen.note = Some(match listen.note.take() {
        Some(n) => format!("{n}; {why}"),
        None => why,
    });
}

/// The report, as a pure function of its four original inputs.
///
/// Kept as the 4-argument shape the existing call sites and `mic_auth_status.rs`
/// use. Both PERM-E grants default to [`GrantState::Unknown`], which is the
/// honest answer when nobody asked the OS: Unknown never nags, so a caller that
/// does not know cannot manufacture a warning. A pure report describes no
/// process, so it is worded for the shipped product, "Yap".
pub fn build_report(
    accessibility: bool,
    status: MicAuth,
    asr_ok: bool,
    asr_detail: String,
) -> PermissionReport {
    build_report_with_grants(
        accessibility,
        status,
        GrantState::Unknown,
        GrantState::Unknown,
        asr_ok,
        asr_detail,
        "Yap",
    )
}

/// The whole report, pure, so every branch is testable on a machine with no
/// microphone, no grant and no window server — the exact machine on which the
/// old bool-only report was wrong. `product` names the TCC row in every line
/// (`perm_truth::product_name`: "Yap Test" for `/Applications/Yap Test.app`).
pub fn build_report_with_grants(
    accessibility: bool,
    status: MicAuth,
    input_monitoring: GrantState,
    audio_capture: GrantState,
    asr_ok: bool,
    asr_detail: String,
    product: &str,
) -> PermissionReport {
    // Authorization is TCC's answer; readiness additionally needs hardware.
    let microphone = status == MicAuth::Authorized && crate::mic_auth::input_device_present();
    let mic_detail = microphone_detail(status, product);

    let grants = vec![
        GrantRow {
            key: GRANT_MICROPHONE,
            label: "Microphone",
            status: mic_grant_state(status),
            pane: "Microphone",
            detail: mic_detail.clone(),
        },
        GrantRow {
            key: GRANT_ACCESSIBILITY,
            label: "Accessibility",
            status: if accessibility {
                GrantState::Authorized
            } else {
                GrantState::Denied
            },
            pane: "Accessibility",
            detail: accessibility_detail(accessibility, product),
        },
        GrantRow {
            key: GRANT_INPUT_MONITORING,
            label: "Input Monitoring",
            status: input_monitoring,
            pane: "InputMonitoring",
            detail: input_monitoring_row_detail(input_monitoring, product),
        },
        GrantRow {
            key: GRANT_AUDIO_CAPTURE,
            label: "System Audio Recording",
            status: audio_capture,
            pane: SYSTEM_AUDIO_PANE,
            detail: audio_capture_detail(audio_capture, product),
        },
    ];

    let mut parts: Vec<String> = Vec::new();
    if !accessibility {
        parts.push(format!(
            "Accessibility OFF — enable for FN hold + auto-paste (Privacy → Accessibility → {product})"
        ));
    }
    if status != MicAuth::Authorized {
        parts.push(mic_detail.clone());
    }
    // Only a DENIAL speaks. An Unknown grant is silent by construction.
    if input_monitoring.nags() {
        parts.push(input_monitoring_detail(input_monitoring, product));
    }
    if audio_capture.nags() {
        parts.push(audio_capture_detail(audio_capture, product));
    }
    if !asr_ok {
        parts.push("Speech model needed".into());
    }

    // Mic + ASR are critical for dictation. Accessibility only for paste
    // (clipboard always works). A microphone that is not AUTHORIZED can never
    // be critical-ok, whatever the hardware says.
    //
    // PERM-E deleted the vestigial ffmpeg row from this computation: it was
    // hardcoded `true` with the comment "no longer required — cpal in-process",
    // so `all_critical_ok` was partly a function of a fiction.
    let all_critical_ok = status == MicAuth::Authorized && microphone && asr_ok;
    let summary = if all_critical_ok && parts.is_empty() {
        format!("All critical permissions look good for {product}.")
    } else if parts.is_empty() {
        mic_detail.clone()
    } else {
        parts.join(" · ")
    };

    PermissionReport {
        accessibility,
        microphone,
        microphone_status: status.as_str().to_string(),
        input_monitoring,
        audio_capture,
        grants,
        asr_ok,
        asr_detail,
        summary,
        all_critical_ok,
        // Filled by `report()` from the live process; a pure report has no
        // process to describe, so it carries none.
        checked_bundle_path: String::new(),
        signing_authority: String::new(),
        failing_conditions: Vec::new(),
        stale_grants: Vec::new(),
        duplicate_bundles: Vec::new(),
        probe: unprobed(accessibility, status, input_monitoring),
        product_name: product.to_string(),
    }
}

/// The probe a PURE report carries: the given answers, no identity history.
fn unprobed(
    accessibility: bool,
    status: MicAuth,
    input_monitoring: GrantState,
) -> crate::perm_probe::ProbeReport {
    let none = crate::perm_truth::SigningSignature::default();
    crate::perm_probe::build_probe_report(
        crate::perm_probe::RawProbes {
            accessibility,
            listen_event_cg: input_monitoring == GrantState::Authorized,
            listen_event_hid: input_monitoring,
            post_event: accessibility,
            microphone: status,
        },
        &crate::perm_truth::TrustLedger::default(),
        None,
        &none,
    )
}

/// The microphone's four TCC answers, collapsed onto the tri-state.
///
/// `NotDetermined` is UNKNOWN, not denied: nobody has been asked, and the
/// health row must not accuse macOS of blocking something it never refused.
pub fn mic_grant_state(status: MicAuth) -> GrantState {
    match status {
        MicAuth::Authorized => GrantState::Authorized,
        MicAuth::Denied | MicAuth::Restricted => GrantState::Denied,
        MicAuth::NotDetermined => GrantState::Unknown,
    }
}

/// The sentence a missing Accessibility grant gets, and the only place it is
/// written. A function so the paste path reuses the exact words, naming the
/// same TCC row (`product`).
pub fn accessibility_off_detail(product: &str) -> String {
    format!(
        "Accessibility is off for {product}, so the transcript is copied but never pasted. \
         Turn {product} on in System Settings → Privacy & Security → Accessibility."
    )
}

/// The Accessibility grant row's line, either way.
pub fn accessibility_detail(on: bool, product: &str) -> String {
    if on {
        format!("Accessibility is on — {product} can paste what it hears.")
    } else {
        accessibility_off_detail(product)
    }
}

// ── Input Monitoring ───────────────────────────────────────────────────────

/// Is the push-to-talk tap actually delivering events right now?
///
/// This is OBSERVED behaviour, not a permission read, and it outranks
/// `IOHIDCheckAccess`: a listen-only `CGEventTap` runs perfectly well under an
/// Accessibility grant alone, so a tap that is working must never be reported
/// blocked (PANEL 2).
///
/// yap-perm-truth: the evidence is DELIVERY — a key or flags edge reached
/// `tap_callback` since the last re-arm — never `re_arms == 0`. A re-arm count
/// is not delivery: on 0.9.0 the tap was re-armed 7 times and delivered nothing,
/// and a tap that was never re-armed may equally never have delivered a thing.
fn tap_is_delivering() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::ptt_macos::tap_health().delivering()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// What Yap reports for Input Monitoring, given what IOKit said, whether
/// Accessibility is trusted, and whether the tap is observably working.
///
/// Pure and total, because the rule it encodes is a product decision and not an
/// API detail: **a working tap is never reported blocked**, and a HID denial
/// under a live Accessibility grant is UNKNOWN (we cannot prove either way)
/// rather than a red line telling the user to fix something that is not broken.
/// The raw IOHIDCheckAccess(ListenEvent) answer — what the hotkey probe reads
/// (yap-p0-hotkeys-no-permission). Informational: it never gates the tap.
pub fn input_monitoring_hid() -> GrantState {
    crate::perm_probe::listen_event_hid()
}

pub fn input_monitoring_state(
    hid: GrantState,
    accessibility: bool,
    tap_delivering: bool,
) -> GrantState {
    match hid {
        GrantState::Authorized => GrantState::Authorized,
        _ if tap_delivering => GrantState::Authorized,
        GrantState::Denied if accessibility => GrantState::Unknown,
        other => other,
    }
}

/// focus-2026-10-19 — the Input Monitoring row for THIS bundle: the test
/// channel has its own grant, so the row names the running product (#247's
/// `perm_truth::product_name`), and a TCC row that says "allowed" while the live
/// check says otherwise is called out as a stale grant with the remove + re-add
/// fix — it never reads as a plain "off".
pub fn input_monitoring_row_detail(state: GrantState, product: &str) -> String {
    let row =
        crate::hotkey_table::tcc_listen_event_row(&crate::perm_truth::running_identity().bundle_id);
    let granted = state == GrantState::Authorized;
    let stale = row == crate::hotkey_table::TccRow::Allowed && !granted;
    let detail = if stale || product != "Yap" {
        crate::hotkey_table::input_monitoring_detail_for(granted, stale, product)
    } else {
        input_monitoring_detail(state, product)
    };
    // yap-hotkeys-advisories-r1 #4/#5: no Full Disk Access, or a failed TCC
    // query, cannot rule out a stale grant — say so, with the fix.
    match crate::hotkey_table::input_monitoring_unverified_hint(granted, &row, product) {
        Some(hint) => format!("{detail} {hint}"),
        None => detail,
    }
}

pub fn input_monitoring_detail(state: GrantState, product: &str) -> String {
    match state {
        GrantState::Authorized => {
            format!("Input Monitoring is on — the fn hold reaches {product}.")
        }
        GrantState::Denied => format!(
            "Input Monitoring is off for {product}, so holding fn does nothing. \
             Turn {product} on in System Settings → Privacy & Security → Input Monitoring."
        ),
        GrantState::Unknown => format!(
            "Input Monitoring has not been answered for {product}. The fn hold still works while \
             Accessibility is on."
        ),
    }
}

// ── System audio capture: UNKNOWN BY CONSTRUCTION ──────────────────────────
//
// `syscapture.rs` quotes AudioCap's own README on why: *"There's no public API
// to request audio recording permission or to check if the app has that
// permission."* No `requestAccess`, no `authorizationStatus`, and TCC does not
// re-ask after a denial. So this grant is NEVER PROBED — creating a tap to find
// out would itself pop the system alert, which is precisely the thing YV102
// moved into an explicit, explained Settings step.
//
// The only legitimate input is an OBSERVATION that already happened: the YV102
// pre-warm, or a live meeting tap. Until one of those runs, the answer is
// Unknown, and Unknown says nothing to the user.

/// What a tap that already ran turned out to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCaptureObservation {
    /// No tap has ever run in this install's memory.
    NeverRan,
    /// Audio was actually delivered — the grant is real.
    Delivered,
    /// A tap ran past `syscapture::DENIAL_GRACE` and delivered nothing.
    SilentPastGrace,
}

const OBS_NEVER_RAN: u8 = 0;
const OBS_DELIVERED: u8 = 1;
const OBS_SILENT_PAST_GRACE: u8 = 2;

static AUDIO_CAPTURE_OBSERVED: AtomicU8 = AtomicU8::new(OBS_NEVER_RAN);

/// Record what a tap that ALREADY RAN observed. The only writer of this state.
pub fn note_audio_capture_observation(observation: AudioCaptureObservation) {
    let v = match observation {
        AudioCaptureObservation::NeverRan => OBS_NEVER_RAN,
        AudioCaptureObservation::Delivered => OBS_DELIVERED,
        AudioCaptureObservation::SilentPastGrace => OBS_SILENT_PAST_GRACE,
    };
    AUDIO_CAPTURE_OBSERVED.store(v, Ordering::Relaxed);
}

/// The observation → grant mapping, pure. Note there is no arm that turns
/// "never ran" into anything but Unknown.
pub fn audio_capture_from_observation(observation: AudioCaptureObservation) -> GrantState {
    match observation {
        AudioCaptureObservation::Delivered => GrantState::Authorized,
        AudioCaptureObservation::SilentPastGrace => GrantState::Denied,
        AudioCaptureObservation::NeverRan => GrantState::Unknown,
    }
}

/// The current system-audio answer. Reads the stored observation and NOTHING
/// else — it opens no tap, spawns nothing, and cannot prompt.
pub fn audio_capture_state() -> GrantState {
    audio_capture_from_observation(match AUDIO_CAPTURE_OBSERVED.load(Ordering::Relaxed) {
        OBS_DELIVERED => AudioCaptureObservation::Delivered,
        OBS_SILENT_PAST_GRACE => AudioCaptureObservation::SilentPastGrace,
        _ => AudioCaptureObservation::NeverRan,
    })
}

pub fn audio_capture_detail(state: GrantState, product: &str) -> String {
    match state {
        GrantState::Authorized => {
            format!("{product} is receiving system audio — macOS shows a purple dot while it records.")
        }
        GrantState::Denied => format!(
            "No system audio reached {product} in the last 2 meetings while other apps \
             were playing sound. macOS will not ask again — check System Audio Recording for {product} in \
             System Settings, then start a new meeting."
        ),
        GrantState::Unknown => {
            "System audio recording has not been tried yet. Run the meeting setup step when you \
             want it; macOS asks once, and only then."
                .to_string()
        }
    }
}

// ── The watcher ────────────────────────────────────────────────────────────

/// The event the frontend listens to. Emitted ONLY on a transition.
pub const PERMISSION_CHANGED_EVENT: &str = "permission_changed";

/// The floor on any re-read interval in this module. YV81 deleted Yap's busy
/// timers on purpose; a permission that changes needs to be noticed in seconds
/// of wall clock, not milliseconds of CPU.
pub const WATCH_MIN_INTERVAL: Duration = Duration::from_secs(5);

/// The interval used ONLY while the banner is shown — i.e. something is denied
/// and therefore fixable by the user walking to System Settings. macOS
/// publishes no notification for a TCC grant, so this poll is the only reading
/// there is while Settings is in front; yap-perm-truth cut it from 45 s to 5 s
/// because a fix that takes 45 s to show up reads as "Yap is wrong". The cost is
/// three cheap reads (AX, IOHID, AVFoundation) every 5 s, and only while a
/// banner is up. Parked otherwise, exactly as before.
pub const WATCH_INTERVAL: Duration = Duration::from_secs(5);

/// The four grants, as one comparable value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantSnapshot {
    pub microphone: GrantState,
    pub accessibility: GrantState,
    pub input_monitoring: GrantState,
    pub audio_capture: GrantState,
    /// fix-ax-probe-refresh — a bit per service the banner shows a Repair row
    /// for ([`repair_row_bits`]). Without it a row the four grants do not carry
    /// (PostEvent) never caused a transition, so the banner never re-read.
    pub repair_rows: u8,
}

/// The services with a repairable (Denied / StaleIdentity) probe, as bits in
/// `Service::ALL` order. Pure.
pub fn repair_row_bits(probe: &crate::perm_probe::ProbeReport) -> u8 {
    use crate::perm_probe::{ProbeState, Service};
    Service::ALL
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            matches!(
                probe.get(**s).state,
                ProbeState::Denied | ProbeState::StaleIdentity
            )
        })
        .fold(0u8, |bits, (i, _)| bits | (1 << i))
}

impl GrantSnapshot {
    pub fn from_report(report: &PermissionReport) -> Self {
        Self {
            microphone: mic_grant_state(match report.microphone_status.as_str() {
                "authorized" => MicAuth::Authorized,
                "denied" => MicAuth::Denied,
                "restricted" => MicAuth::Restricted,
                _ => MicAuth::NotDetermined,
            }),
            accessibility: if report.accessibility {
                GrantState::Authorized
            } else {
                GrantState::Denied
            },
            input_monitoring: report.input_monitoring,
            audio_capture: report.audio_capture,
            repair_rows: repair_row_bits(&report.probe),
        }
    }

    pub fn all_authorized(self) -> bool {
        self.microphone == GrantState::Authorized
            && self.accessibility == GrantState::Authorized
            && self.input_monitoring == GrantState::Authorized
            && self.audio_capture == GrantState::Authorized
    }

    /// Is anything DENIED? Unknown is not, and never will be.
    pub fn has_denial(self) -> bool {
        self.microphone.nags()
            || self.accessibility.nags()
            || self.input_monitoring.nags()
            || self.audio_capture.nags()
            // A shown Repair row is a denial the user is fixing: keep reading.
            || self.repair_rows != 0
    }
}

/// How long until the next unprompted re-read — or `None` for "park entirely".
///
/// The watcher parks whenever nothing is denied. That covers the all-Authorized
/// case the item names, and it also covers the ordinary steady state where
/// system audio is Unknown: an Unknown grant CANNOT be resolved by re-reading
/// (there is no API to read), so keeping a timer alive for it would be a busy
/// loop that can never learn anything. Revocation while parked is caught by the
/// event triggers — focus, wake, a hotkey refusal — not by a clock.
pub fn next_wait(snapshot: GrantSnapshot) -> Option<Duration> {
    if snapshot.has_denial() {
        Some(WATCH_INTERVAL)
    } else {
        None
    }
}

/// Emit-on-transition, and nothing else.
///
/// The FIRST observation establishes the baseline and is deliberately silent:
/// the window fetches the report when it mounts, so an emit there would be a
/// duplicate on every launch. After that, one emit per change, zero for any
/// number of identical re-reads.
#[derive(Debug, Default)]
pub struct TransitionGate {
    last: Option<GrantSnapshot>,
}

impl TransitionGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// `Some(snapshot)` exactly when this read DIFFERS from the previous one.
    pub fn observe(&mut self, next: GrantSnapshot) -> Option<GrantSnapshot> {
        match self.last.replace(next) {
            Some(prev) if prev == next => None,
            Some(_) => Some(next),
            None => None,
        }
    }
}

/// Why the watcher woke up. Logged, so a support bundle can tell a focus-driven
/// re-read from a wake-driven one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchReason {
    Launch,
    Focus,
    /// Wake from sleep. **Yap registers no sleep/wake observer here.** The one
    /// observer lives in `power.rs` (Y1-B, `power::SleepWakeObserver`); its
    /// `"permissions"` consumer calls [`on_sleep_wake`], which pokes with this
    /// reason. A second observer is never added.
    Wake,
    /// A hotkey or a paste was refused for want of a grant.
    Refusal,
    /// yap-p0-perm-repair — a Repair's re-probe saw the grant flip.
    Repair,
    /// `NSApplicationDidBecomeActiveNotification` — Yap became the active app
    /// (Cmd-Tab back from System Settings, a Dock click, the menu bar). Window
    /// focus alone misses the case where the app activates without its main
    /// window taking key.
    Activate,
}

/// Register the ONE `NSApplicationDidBecomeActiveNotification` observer and
/// call `on_activate` each time it fires. Main thread only (AppKit); the
/// observer lives for the life of the process.
#[cfg(target_os = "macos")]
pub fn observe_app_activation<F: Fn() + 'static>(on_activate: F) {
    use block2::RcBlock;
    use objc2_app_kit::NSApplicationDidBecomeActiveNotification;
    use objc2_foundation::{NSNotification, NSNotificationCenter};
    use std::ptr::NonNull;

    let block = RcBlock::new(move |_note: NonNull<NSNotification>| on_activate());
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: a framework constant name, no sender filter, delivered on the
    // posting (main) thread; the centre copies the block.
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSApplicationDidBecomeActiveNotification),
            None,
            None,
            &block,
        )
    };
    // Never removed: one observer for the life of the process.
    std::mem::forget(token);
}

#[cfg(not(target_os = "macos"))]
pub fn observe_app_activation<F: Fn() + 'static>(_on_activate: F) {}

struct WatchInner {
    /// Bumped by every poke; the condvar predicate.
    ticket: Mutex<u64>,
    cv: Condvar,
}

/// A running watcher. Dropping it does not stop the thread — the thread lives
/// for the life of the process, parked, costing nothing.
pub struct PermissionWatch {
    inner: Arc<WatchInner>,
}

impl PermissionWatch {
    /// Wake the watcher for an immediate re-read.
    pub fn poke(&self, reason: WatchReason) {
        let mut t = self.inner.ticket.lock().expect("permission watch ticket");
        *t = t.wrapping_add(1);
        drop(t);
        log::debug!("permission watch poked: {reason:?}");
        self.inner.cv.notify_all();
    }
}

static WATCH: OnceLock<PermissionWatch> = OnceLock::new();

/// Poke the process-wide watcher, if one is running. A no-op before
/// [`start_watch`] — call sites (focus, wake, a refusal) must never care.
pub fn poke(reason: WatchReason) {
    if let Some(w) = WATCH.get() {
        w.poke(reason);
    }
}

/// Y1-B — PERM-E's consumer of the one sleep/wake publisher in `power.rs`.
/// A wake is the single most common moment a TCC grant or an audio device
/// changed under a laptop user, so it re-reads; going to sleep changes nothing
/// worth reading. Returns the reason it poked with, if any.
pub fn on_sleep_wake(event: crate::meeting_matrix::SleepEvent) -> Option<WatchReason> {
    match event {
        crate::meeting_matrix::SleepEvent::DidWake => {
            poke(WatchReason::Wake);
            Some(WatchReason::Wake)
        }
        crate::meeting_matrix::SleepEvent::WillSleep => None,
    }
}

/// Start the one watcher thread.
///
/// `read` produces a snapshot (the app passes a closure that calls [`report`]),
/// `emit` publishes [`PERMISSION_CHANGED_EVENT`]. Both are injected so the whole
/// loop is testable with no Tauri host and no grants.
pub fn spawn_watch<R, E>(read: R, emit: E) -> PermissionWatch
where
    R: Fn() -> GrantSnapshot + Send + 'static,
    E: Fn(GrantSnapshot) + Send + 'static,
{
    let inner = Arc::new(WatchInner {
        ticket: Mutex::new(0),
        cv: Condvar::new(),
    });
    let thread_inner = Arc::clone(&inner);
    std::thread::Builder::new()
        .name("permission-watch".into())
        .spawn(move || {
            let mut gate = TransitionGate::new();
            loop {
                let snapshot = read();
                if let Some(changed) = gate.observe(snapshot) {
                    emit(changed);
                }
                let mut ticket = thread_inner.ticket.lock().expect("permission watch ticket");
                let seen = *ticket;
                match next_wait(snapshot) {
                    // Parked. Costs exactly one blocked thread and zero wakeups
                    // until somebody pokes.
                    None => {
                        while *ticket == seen {
                            ticket = thread_inner
                                .cv
                                .wait(ticket)
                                .expect("permission watch condvar");
                        }
                    }
                    Some(wait) => {
                        let _ = thread_inner
                            .cv
                            .wait_timeout(ticket, wait)
                            .expect("permission watch condvar");
                    }
                }
            }
        })
        .expect("spawn permission watch");
    PermissionWatch { inner }
}

/// Install the process-wide watcher. Idempotent: a second call is ignored, so
/// there can only ever be one watcher thread.
pub fn start_watch<R, E>(read: R, emit: E)
where
    R: Fn() -> GrantSnapshot + Send + 'static,
    E: Fn(GrantSnapshot) + Send + 'static,
{
    let _ = WATCH.set(spawn_watch(read, emit));
    poke(WatchReason::Launch);
}

/// The message a paste that failed for want of Accessibility must carry.
///
/// PERM-C's `blocked` pill phase covers the microphone: nothing was heard.
/// Accessibility has the opposite shape — the take SUCCEEDED and the paste is
/// what died — so routing it through the generic paste error reads to the user
/// as "Yap did not understand me", which is a lie about a transcript sitting on
/// their clipboard. `accessibility_trusted = false` names the real cause.
pub fn paste_failure_detail(accessibility_trusted: bool, fallback: &str) -> String {
    if accessibility_trusted {
        fallback.to_string()
    } else {
        accessibility_off_detail(&crate::perm_truth::product_name())
    }
}

/// YV102 — the pane a denied system-audio tap has to be fixed in, and the ONLY
/// recovery there is: TCC never re-asks after a denial.
///
/// **The anchor was verified on the target OS, not guessed**, because OS-10 is
/// explicit that a wrong one opens the top of System Settings, which is a worse
/// dead end than no link at all. The plan's candidate string —
/// `…?Privacy_SystemAudio` — **does not exist**. Enumerating the anchors the
/// Settings pane extension actually recognises:
///
/// ```text
/// $ strings -a /System/Library/ExtensionKit/Extensions/SecurityPrivacyExtension.appex\
///     /Contents/MacOS/SecurityPrivacyExtension | grep -oE 'Privacy_[A-Za-z0-9]+' | sort -u
/// … Privacy_Accessibility  Privacy_AudioCapture  Privacy_Camera  Privacy_Microphone
/// Privacy_ScreenCapture … (no Privacy_SystemAudio)
/// ```
///
/// `Privacy_AudioCapture` is the real one. Opening it and reading the window
/// title back out of the window server (macOS 26.5.2, build 25F84) gives
/// `Screen & System Audio Recording` — the pane that carries the **System Audio
/// Recording Only** list Yap needs to appear in. Transcript and screenshot:
/// `docs/pr-screenshots/YV102/`.
///
/// The fallback below is not decoration: on macOS 14.4–14.x the pane is titled
/// "System Audio Recording Only" and is reached by the same anchor, but if a
/// future OS retires it, landing on Privacy & Security is still one scroll from
/// the answer, whereas an unrecognised anchor lands on General.
pub const SYSTEM_AUDIO_PANE: &str = "SystemAudio";

/// Open the correct System Settings privacy pane (macOS 13+ / 15 URLs).
pub fn open_privacy_pane(pane: &str) -> Result<(), String> {
    let urls = open_privacy_pane_urls(pane);
    let mut last_err = String::new();
    for url in urls {
        match Command::new("open").arg(url).spawn() {
            Ok(_) => return Ok(()),
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(last_err)
}

/// The URL candidates for a pane name, pure, so a test can assert that every
/// row of the permission surface points at a deep link that exists — the exact
/// check OS-10 asks for, since an unrecognised anchor opens System Settings at
/// General and is a worse dead end than no link at all.
pub fn open_privacy_pane_urls(pane: &str) -> &'static [&'static str] {
    // Prefer modern Settings URLs; fall back to legacy preference panes.
    match pane {
        "Microphone" => &[
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension",
        ],
        "Accessibility" => {
            &["x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"]
        }
        "InputMonitoring" | "ListenEvent" => {
            &["x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"]
        }
        // YV102 — see SYSTEM_AUDIO_PANE. Verified anchor first, Privacy &
        // Security second; never a bare System Settings launch, which is the
        // dead end OS-10 warns about.
        "SystemAudio" | "AudioCapture" => &[
            "x-apple.systempreferences:com.apple.preference.security?Privacy_AudioCapture",
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension",
        ],
        _ => &["x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"],
    }
}

/// Prompt user for Accessibility if not trusted (system dialog).
pub fn request_accessibility_prompt() -> bool {
    crate::perm_probe::request_accessibility_prompt()
}

pub fn is_accessibility_trusted() -> bool {
    crate::perm_probe::accessibility_trusted()
}

#[cfg(test)]
mod product_name_tests {
    //! yap-test-channel-product-name — every permission sentence built OUTSIDE
    //! `report()` goes through `named()` (= `name_product` with this process's
    //! product). These are those sentences: in a "Yap Test" build each must name
    //! "Yap Test" and leave no bare "Yap" that points at the wrong TCC row.
    use super::name_product;

    fn bare_yap_left(s: &str) -> bool {
        let without = s.replace("Yap Test", "");
        let b = without.as_bytes();
        without.match_indices("Yap").any(|(i, _)| {
            let before = i.checked_sub(1).map(|j| b[j]);
            let after = b.get(i + 3).copied();
            let word = |c: Option<u8>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_');
            !word(before) && !word(after)
        })
    }

    #[test]
    fn permission_sentences_outside_the_report_name_the_test_product() {
        let sentences: &[(&str, &str)] = &[
            ("lib MIC_REQUIRED_MESSAGE", crate::MIC_REQUIRED_MESSAGE),
            ("lib SILENT_CAPTURE_PERMISSION_MESSAGE", crate::SILENT_CAPTURE_PERMISSION_MESSAGE),
            ("record NO_MIC_ERR", crate::record::NO_MIC_ERR),
            ("record NO_SAMPLES_ERR", crate::record::NO_SAMPLES_ERR),
            ("record NO_AUDIO_ERR", crate::record::NO_AUDIO_ERR),
            ("ptt_macos TAP_BLOCKED_REASON", crate::ptt_macos::TAP_BLOCKED_REASON),
            ("record too-short", "Audio too short (120 ms) — hold longer, or allow Microphone for Yap."),
            ("record timeout", "Microphone open timed out after 3000ms — check Yap's Microphone permission under System Settings → Privacy."),
            ("record config", "Mic config failed (x). Enable Microphone for Yap (not Python) in System Settings."),
            ("record start", "mic start failed (x). Allow Microphone for Yap."),
            ("paste copy card", "PostEvent and Accessibility are both off for Yap, so it cannot type into this app"),
            ("paste edit selection", "Accessibility is off for Yap — Repair to let me edit your selection."),
            ("hotkeys armed-no-delivery", "macOS is not delivering key events to Yap (Input Monitoring is off)"),
            ("meeting badge looks-denied", crate::syscapture::LOOKS_DENIED_MESSAGE),
            ("meeting badge setup-failed", crate::syscapture::SETUP_FAILED_MESSAGE),
        ];
        for (name, raw) in sentences {
            assert!(
                bare_yap_left(raw),
                "{name}: fixture must start with a bare Yap: {raw:?}"
            );
            let named = name_product(raw, "Yap Test");
            assert!(
                named.contains("Yap Test"),
                "{name}: {named:?} does not name Yap Test"
            );
            assert!(
                !bare_yap_left(&named),
                "{name}: {named:?} still names a bare Yap"
            );
            // Shipped build: untouched.
            assert_eq!(
                name_product(raw, "Yap"),
                *raw,
                "{name}: changed in the Yap build"
            );
        }
    }

    /// yap-test-channel-product-name — every permission line the report BUILDS
    /// takes its product from `perm_truth` (the TCC row System Settings lists):
    /// for `/Applications/Yap Test.app`, "Yap Test" appears and a bare "Yap "
    /// never does, across every grant state and the summary.
    #[test]
    fn permission_copy_uses_product_name() {
        use super::{
            accessibility_detail, accessibility_off_detail, audio_capture_detail,
            build_report_with_grants, input_monitoring_detail, microphone_detail, GrantState,
            MicAuth,
        };
        let product =
            crate::perm_truth::product_name_from_bundle_path("/Applications/Yap Test.app");
        assert_eq!(product, "Yap Test");
        let grants = [
            GrantState::Authorized,
            GrantState::Denied,
            GrantState::Unknown,
        ];
        let mics = [
            MicAuth::Authorized,
            MicAuth::NotDetermined,
            MicAuth::Denied,
            MicAuth::Restricted,
        ];
        let mut lines: Vec<String> = Vec::new();
        for m in mics {
            lines.push(microphone_detail(m, &product));
        }
        for on in [true, false] {
            lines.push(accessibility_detail(on, &product));
        }
        for g in grants {
            lines.push(input_monitoring_detail(g, &product));
            lines.push(audio_capture_detail(g, &product));
        }
        lines.push(crate::perm_truth::stale_grant_message(
            "Accessibility",
            "0.9.2",
            "/Applications/Yap Test.app",
        ));
        for ax in [true, false] {
            for m in mics {
                for im in grants {
                    for ac in grants {
                        let r = build_report_with_grants(
                            ax,
                            m,
                            im,
                            ac,
                            false,
                            "no model".into(),
                            &product,
                        );
                        assert_eq!(r.product_name, "Yap Test");
                        lines.push(r.summary.clone());
                        lines.extend(r.grants.iter().map(|g| g.detail.clone()));
                    }
                }
            }
        }
        assert!(lines.iter().any(|l| l.contains("Yap Test")));
        for l in &lines {
            assert!(
                !l.replace("Yap Test", "").contains("Yap "),
                "a bare \"Yap \" names the wrong TCC row: {l:?}"
            );
            assert!(!bare_yap_left(l), "bare Yap left in {l:?}");
            assert!(!l.contains("Yap Test Test"), "doubled product in {l:?}");
        }
        // Every DENIED line names the row to fix.
        for l in [
            microphone_detail(MicAuth::Denied, &product),
            accessibility_off_detail(&product),
            input_monitoring_detail(GrantState::Denied, &product),
            audio_capture_detail(GrantState::Denied, &product),
        ] {
            assert!(l.contains("Yap Test"), "{l:?}");
        }
        // The shipped bundle keeps its words.
        let shipped = crate::perm_truth::product_name_from_bundle_path("/Applications/Yap.app");
        assert_eq!(
            accessibility_off_detail(&shipped),
            "Accessibility is off for Yap, so the transcript is copied but never pasted. \
             Turn Yap on in System Settings → Privacy & Security → Accessibility."
        );
    }

    /// Fix round 2 — a wiring guard, not a fixture: every non-comment line of
    /// meeting_control.rs that hands LOOKS_DENIED_MESSAGE to the meeting row
    /// (tap_notes -> meetings.error -> Meetings.tsx) wraps it in `named()`.
    /// The badge_meeting branch once pushed it bare.
    #[test]
    fn meeting_denial_note_is_named_at_every_call_site() {
        let src = include_str!("meeting_control.rs");
        let sites: Vec<&str> = src
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("//") && l.contains("LOOKS_DENIED_MESSAGE"))
            .collect();
        assert!(
            !sites.is_empty(),
            "meeting_control.rs no longer uses LOOKS_DENIED_MESSAGE"
        );
        for l in sites {
            assert!(
                l.contains("permissions::named("),
                "bare LOOKS_DENIED_MESSAGE: {l}"
            );
        }
    }
}

#[cfg(test)]
mod copy_table_tests {
    //! yap24-OS3 — the permission copy table. Every string macOS shows in a TCC
    //! prompt, and every line that tells someone which pane to open, is pinned
    //! here so a stale engine name or a wrong pane cannot ship again.

    const INFO_PLIST: &str = include_str!("../Info.plist");
    const INFO_DEV_PLIST: &str = include_str!("../Info.dev.plist");

    /// (usage key, the exact copy the first TCC prompt shows).
    const PURPOSE_STRINGS: &[(&str, &str)] = &[
        (
            "NSMicrophoneUsageDescription",
            "Yap uses the microphone when you start dictation or record a meeting, and transcribes on this Mac.",
        ),
        (
            "NSAudioCaptureUsageDescription",
            "Yap records the audio from your meeting — the other people on the call as well as you — so it can write the notes. Recording stays on your Mac.",
        ),
        (
            "NSAppleEventsUsageDescription",
            "Yap pastes transcribed text into the app you're typing in.",
        ),
    ];

    /// Engine and model names a purpose string must never carry: the engine is
    /// a setting, the prompt is a first impression that outlives it.
    const ENGINE_NAMES: &[&str] = &[
        "Whisper",
        "Parakeet",
        "Moonshine",
        "Canary",
        "Qwen",
        "Llama",
    ];

    fn usage_string<'a>(plist: &'a str, key: &str) -> Option<&'a str> {
        let (_, rest) = plist.split_once(&format!("<key>{key}</key>"))?;
        let (_, rest) = rest.split_once("<string>")?;
        let (value, _) = rest.split_once("</string>")?;
        Some(value.trim())
    }

    #[test]
    fn permissions_purpose_strings_match_the_copy_table() {
        for (name, plist) in [
            ("Info.plist", INFO_PLIST),
            ("Info.dev.plist", INFO_DEV_PLIST),
        ] {
            for (key, want) in PURPOSE_STRINGS {
                assert_eq!(
                    usage_string(plist, key),
                    Some(*want),
                    "{name} {key} drifted from the permission copy table"
                );
            }
        }
    }

    #[test]
    fn permissions_purpose_strings_name_no_engine() {
        for (name, plist) in [
            ("Info.plist", INFO_PLIST),
            ("Info.dev.plist", INFO_DEV_PLIST),
        ] {
            for (key, _) in PURPOSE_STRINGS {
                let value = usage_string(plist, key).unwrap_or_default();
                for engine in ENGINE_NAMES {
                    assert!(
                        !value.contains(engine),
                        "{name} {key} names the {engine} engine: {value:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn permissions_mic_purpose_covers_dictation_and_meetings() {
        let mic = usage_string(INFO_PLIST, "NSMicrophoneUsageDescription").unwrap_or_default();
        assert!(
            mic.contains("dictation"),
            "mic copy must name dictation: {mic:?}"
        );
        // Takes also start hands-free, from the tray, the pill, the Home
        // button and onboarding calibration, none of which hold a key, so the
        // prompt must not claim the mic is live "only" while a key is held.
        assert!(
            !mic.contains("only") && !mic.contains("hold"),
            "mic copy must not claim key-held exclusivity: {mic:?}"
        );
        assert!(
            mic.contains("meeting"),
            "mic copy must name meetings, which also use the mic: {mic:?}"
        );
        assert!(
            mic.contains("on this Mac"),
            "mic copy must say it stays local: {mic:?}"
        );
    }

    /// The listen-only tap is gated by Input Monitoring; its failure copy and
    /// deep link must both send people to that pane, never to Accessibility.
    #[cfg(target_os = "macos")]
    #[test]
    fn permissions_tap_failure_copy_names_input_monitoring() {
        for line in [
            crate::ptt_macos::TAP_CREATE_FAILED,
            crate::ptt_macos::TAP_BLOCKED_REASON,
        ] {
            assert!(line.contains("Input Monitoring"), "{line:?}");
            assert!(!line.contains("Accessibility"), "{line:?}");
        }
        assert_eq!(
            super::open_privacy_pane_urls("InputMonitoring")[0],
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
        );
    }
}
