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
    (status == MicAuth::Authorized, microphone_detail(status))
}

/// Prose per status. Pure, so `tests/mic_auth_status.rs` can read it without a
/// microphone, a grant, or a window server.
pub fn microphone_detail(status: MicAuth) -> String {
    match status {
        MicAuth::Authorized => {
            if crate::mic_auth::input_device_present() {
                "Microphone authorized for Yap.".into()
            } else {
                "Microphone authorized, but no input device is attached — plug one in or pick one in System Settings → Sound.".into()
            }
        }
        MicAuth::NotDetermined => {
            "macOS has not been asked for the microphone yet — click Request Microphone to show the system prompt.".into()
        }
        MicAuth::Denied => {
            "Microphone is DENIED for Yap. macOS will not ask again — turn Yap on in System Settings → Privacy & Security → Microphone.".into()
        }
        MicAuth::Restricted => {
            "Microphone is restricted by a device policy (Screen Time or MDM). Yap cannot request it; an administrator has to allow it.".into()
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
    let mut report = report_from_probe(
        probe,
        tap_is_delivering(),
        audio_capture_state(),
        asr_ok,
        asr_detail,
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

/// The report from one [`crate::perm_probe::ProbeReport`], pure: the grant rows
/// onboarding renders and the `probe` field the banner's Repair renders are the
/// same reading, so the two can never disagree.
pub fn report_from_probe(
    probe: crate::perm_probe::ProbeReport,
    tap_delivering: bool,
    audio_capture: GrantState,
    asr_ok: bool,
    asr_detail: String,
) -> PermissionReport {
    let accessibility = probe.accessibility.state == crate::perm_probe::ProbeState::Granted;
    let input_monitoring =
        input_monitoring_state(probe.listen_event_hid, accessibility, tap_delivering);
    let mut report = build_report_with_grants(
        accessibility,
        probe.microphone_auth,
        input_monitoring,
        audio_capture,
        asr_ok,
        asr_detail,
    );
    report.probe = probe;
    report
}

/// The report, as a pure function of its four original inputs.
///
/// Kept as the 4-argument shape the existing call sites and `mic_auth_status.rs`
/// use. Both PERM-E grants default to [`GrantState::Unknown`], which is the
/// honest answer when nobody asked the OS: Unknown never nags, so a caller that
/// does not know cannot manufacture a warning.
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
    )
}

/// The whole report, pure, so every branch is testable on a machine with no
/// microphone, no grant and no window server — the exact machine on which the
/// old bool-only report was wrong.
pub fn build_report_with_grants(
    accessibility: bool,
    status: MicAuth,
    input_monitoring: GrantState,
    audio_capture: GrantState,
    asr_ok: bool,
    asr_detail: String,
) -> PermissionReport {
    // Authorization is TCC's answer; readiness additionally needs hardware.
    let microphone = status == MicAuth::Authorized && crate::mic_auth::input_device_present();
    let mic_detail = microphone_detail(status);

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
            detail: if accessibility {
                "Accessibility is on — Yap can paste what it hears.".into()
            } else {
                ACCESSIBILITY_OFF_DETAIL.to_string()
            },
        },
        GrantRow {
            key: GRANT_INPUT_MONITORING,
            label: "Input Monitoring",
            status: input_monitoring,
            pane: "InputMonitoring",
            detail: input_monitoring_detail(input_monitoring),
        },
        GrantRow {
            key: GRANT_AUDIO_CAPTURE,
            label: "System Audio Recording",
            status: audio_capture,
            pane: SYSTEM_AUDIO_PANE,
            detail: audio_capture_detail(audio_capture),
        },
    ];

    let mut parts: Vec<String> = Vec::new();
    if !accessibility {
        parts.push(
            "Accessibility OFF — enable for FN hold + auto-paste (Privacy → Accessibility → Yap)"
                .into(),
        );
    }
    if status != MicAuth::Authorized {
        parts.push(mic_detail.clone());
    }
    // Only a DENIAL speaks. An Unknown grant is silent by construction.
    if input_monitoring.nags() {
        parts.push(input_monitoring_detail(input_monitoring));
    }
    if audio_capture.nags() {
        parts.push(audio_capture_detail(audio_capture));
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
        "All critical permissions look good for Yap.".to_string()
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
/// written. Named so the paste path can reuse the exact words.
pub const ACCESSIBILITY_OFF_DETAIL: &str =
    "Accessibility is off for Yap, so the transcript is copied but never pasted. \
     Turn Yap on in System Settings → Privacy & Security → Accessibility.";

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

pub fn input_monitoring_detail(state: GrantState) -> String {
    match state {
        GrantState::Authorized => "Input Monitoring is on — the fn hold reaches Yap.".to_string(),
        GrantState::Denied => "Input Monitoring is off for Yap, so holding fn does nothing. \
             Turn Yap on in System Settings → Privacy & Security → Input Monitoring."
            .to_string(),
        GrantState::Unknown => {
            "Input Monitoring has not been answered for Yap. The fn hold still works while \
             Accessibility is on."
                .to_string()
        }
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

pub fn audio_capture_detail(state: GrantState) -> String {
    match state {
        GrantState::Authorized => {
            "Yap is receiving system audio — macOS shows a purple dot while it records.".to_string()
        }
        GrantState::Denied => "No system audio reached Yap in the last 2 meetings while other apps \
             were playing sound. macOS will not ask again — check System Audio Recording for Yap in \
             System Settings, then start a new meeting."
            .to_string(),
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
    /// Wake from sleep. **Yap registers no sleep/wake observer here.** Y1-B owns
    /// the `NSWorkspaceWillSleepNotification` / `NSWorkspaceDidWakeNotification`
    /// call site (`meeting_matrix.rs` records it as the absent one today); this
    /// is the entry point it calls, and a second observer is never added.
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
        ACCESSIBILITY_OFF_DETAIL.to_string()
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
