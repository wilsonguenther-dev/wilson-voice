//! yap-p0-perm-repair — ONE truthful grant probe per service, a detected
//! "stale identity" state, and a one-click Repair.
//!
//! Wilson 2026-10-17: "permissioning ... nothing's working". 0.9.0 changed
//! signing identity (Apple Development → Developer ID). Every TCC grant is
//! pinned to the code requirement it was given to, so System Settings kept
//! showing Yap ON while every live API answered no. `perm_truth` could only call
//! that stale when its ledger had seen the grant work under the old signature —
//! and the ledger shipped (#211) AFTER the change, so for the one user who hit
//! it the ledger was empty and the stale state was invisible.
//!
//! This module is the only place Yap asks macOS whether it holds a grant:
//!
//! | service        | live API                                                  |
//! |----------------|-----------------------------------------------------------|
//! | Accessibility  | `AXIsProcessTrusted`                                      |
//! | ListenEvent    | `CGPreflightListenEventAccess` AND `IOHIDCheckAccess`     |
//! | PostEvent      | `CGPreflightPostEventAccess`                              |
//! | Microphone     | `+[AVCaptureDevice authorizationStatusForMediaType:]`     |
//!
//! Nothing here reads TCC.db or the System Settings list — the list is exactly
//! what lied. `permissions::report()`, the banner, onboarding and the hotkey
//! degraded line all read [`probe_all`] (or the per-service probes below);
//! `permissions.rs` keeps no grant FFI of its own.
//!
//! Every decision is pure and unit-tested in `tests/perm_repair.rs`.

use crate::mic_auth::MicAuth;
use crate::perm_truth::{SigningSignature, TrustLedger};
use crate::permissions::GrantState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

// ── Services ───────────────────────────────────────────────────────────────

/// The four TCC services Yap's dictation path depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Service {
    Accessibility,
    ListenEvent,
    PostEvent,
    Microphone,
}

impl Service {
    pub const ALL: [Service; 4] = [
        Service::Accessibility,
        Service::ListenEvent,
        Service::PostEvent,
        Service::Microphone,
    ];

    /// The `tccutil` service name (`kTCCService<name>` in tccd, verified on
    /// macOS 26.6.2 — see docs/loop/evidence/yap-p0-perm-repair.md).
    pub fn tcc_name(self) -> &'static str {
        match self {
            Service::Accessibility => "Accessibility",
            Service::ListenEvent => "ListenEvent",
            Service::PostEvent => "PostEvent",
            Service::Microphone => "Microphone",
        }
    }

    /// The wire key (`repair_permission`'s argument, the frontend's key).
    pub fn key(self) -> &'static str {
        match self {
            Service::Accessibility => "accessibility",
            Service::ListenEvent => "listen_event",
            Service::PostEvent => "post_event",
            Service::Microphone => "microphone",
        }
    }

    /// As System Settings names the list the grant lives in.
    pub fn label(self) -> &'static str {
        match self {
            // fix-ax-probe-refresh: PostEvent is NOT Accessibility. Labelling it
            // "Accessibility" is how a PostEvent row read "Accessibility is off"
            // while Accessibility was granted, and its Repair reset PostEvent.
            Service::Accessibility => "Accessibility",
            Service::PostEvent => "Event posting (⌘V)",
            Service::ListenEvent => "Input Monitoring",
            Service::Microphone => "Microphone",
        }
    }

    /// The System Settings list the grant appears in (user-facing words).
    pub fn settings_list(self) -> &'static str {
        match self {
            Service::Accessibility | Service::PostEvent => "Accessibility",
            Service::ListenEvent => "Input Monitoring",
            Service::Microphone => "Microphone",
        }
    }

    /// The `open_privacy_pane` argument. PostEvent clients are listed under
    /// Accessibility in System Settings.
    pub fn pane_name(self) -> &'static str {
        match self {
            Service::Accessibility | Service::PostEvent => "Accessibility",
            Service::ListenEvent => "ListenEvent",
            Service::Microphone => "Microphone",
        }
    }

    /// The live API that answers for this service.
    pub fn api(self) -> &'static str {
        match self {
            Service::Accessibility => "AXIsProcessTrusted",
            Service::ListenEvent => "CGPreflightListenEventAccess + IOHIDCheckAccess",
            Service::PostEvent => "CGPreflightPostEventAccess",
            Service::Microphone => "AVCaptureDevice authorizationStatus",
        }
    }

    /// Accepts the wire key or the tccutil name, case-insensitively.
    pub fn parse(s: &str) -> Option<Service> {
        let s = s.trim().to_ascii_lowercase();
        match s.as_str() {
            "accessibility" => Some(Service::Accessibility),
            "listen_event" | "listenevent" | "input_monitoring" | "inputmonitoring" => {
                Some(Service::ListenEvent)
            }
            "post_event" | "postevent" => Some(Service::PostEvent),
            "microphone" => Some(Service::Microphone),
            _ => None,
        }
    }
}

// ── States ─────────────────────────────────────────────────────────────────

/// What a live API answered, before identity is considered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RawAnswer {
    Granted,
    Denied,
    NotDetermined,
    Unknown,
}

/// The state the UI branches on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeState {
    Granted,
    Denied,
    NotDetermined,
    /// The live API says no AND this grant belongs to a differently signed Yap:
    /// the ledger saw it work under another signature, or the last launch ran
    /// under another authority / team. System Settings shows Yap ON; the switch
    /// is pinned to the old signature and does nothing for this one.
    StaleIdentity,
    Unknown,
    /// yap-dev-perm-probe-attribution — this process is NOT running as its own
    /// signed app bundle (an unbundled, ad-hoc signed dev binary from
    /// `npm run desktop:dev`). TCC attributes such a process to its RESPONSIBLE
    /// process — the terminal that launched it — so every live API answers
    /// with the terminal's grants, not Yap's. The answer is kept in `raw` (the
    /// process really can or cannot do the thing), but no row may present it
    /// as Yap's grant and no Repair may run: `tccutil reset` would target an
    /// identity that is not the one macOS consulted.
    DevHostAttributed,
}

impl ServiceProbe {
    /// Can this process actually use the service right now? `Granted`, or a
    /// host-attributed answer whose raw call said yes (a dev build really can
    /// post events under its terminal's grant). For DECISIONS (the insert
    /// path, hotkey wiring) — never for a row that says whose grant it is.
    pub fn usable(&self) -> bool {
        match self.state {
            ProbeState::Granted => true,
            ProbeState::DevHostAttributed => self.raw == RawAnswer::Granted,
            _ => false,
        }
    }
}

// ── Who macOS is answering about ───────────────────────────────────────────

/// The words every surface uses for a host-attributed probe.
pub const DEV_HOST_LINE: &str = "dev build: permissions are the host terminal's";

/// Is `exe_path` the main executable of an `.app` bundle
/// (`<Name>.app/Contents/MacOS/<exe>`)? Pure, path-shape only.
pub fn exe_inside_app_bundle(exe_path: &str) -> bool {
    let mut parts = std::path::Path::new(exe_path).components().rev();
    let _exe = parts.next();
    let macos = parts.next().and_then(|c| c.as_os_str().to_str());
    let contents = parts.next().and_then(|c| c.as_os_str().to_str());
    let app = parts.next().and_then(|c| c.as_os_str().to_str());
    macos == Some("MacOS")
        && contents == Some("Contents")
        && app.is_some_and(|a| a.len() > ".app".len() && a.ends_with(".app"))
}

/// yap-dev-perm-probe-attribution — does macOS attribute this process's TCC
/// grants to Yap ITSELF? Pure.
///
/// True only when all three hold:
/// - `exe_path` is the main executable inside an `.app` bundle;
/// - `NSBundle.mainBundle` has a bundle id (a Tauri dev binary may carry an
///   embedded Info.plist and so an id, which is why the id alone proves
///   nothing — the path is checked too);
/// - the code signature carries a certificate with a team id (Developer ID or
///   Apple Development). An ad-hoc or unsigned binary has no designated
///   requirement TCC can pin a grant to, and launched from a shell it is
///   attributed to the responsible process: the terminal.
///
/// `/Applications/Yap.app/Contents/MacOS/wilson-voice` under Developer ID →
/// true; `desktop/target/debug/wilson-voice` ad hoc → false.
pub fn running_as_bundle(
    exe_path: &str,
    bundle_id_of_main_bundle: Option<&str>,
    code_signature: &SigningSignature,
) -> bool {
    exe_inside_app_bundle(exe_path)
        && bundle_id_of_main_bundle.is_some_and(|id| !id.trim().is_empty())
        && team_of(&code_signature.authority).is_some()
}

/// [`running_as_bundle`] for this process. Computed once: it cannot change
/// under a running process.
pub fn this_process_runs_as_bundle() -> bool {
    static AS_BUNDLE: OnceLock<bool> = OnceLock::new();
    *AS_BUNDLE.get_or_init(|| {
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let id = crate::perm_truth::main_bundle_id();
        let me = crate::perm_truth::running_identity();
        let as_bundle = running_as_bundle(&exe, id.as_deref(), &me.signature);
        if !as_bundle {
            log::warn!(
                "perm-probe: {DEV_HOST_LINE} (exe={exe}, bundle id={id:?}, authority={:?})",
                me.signature.authority
            );
        }
        as_bundle
    })
}

/// Mark every service [`ProbeState::DevHostAttributed`]. Pure. The raw answers
/// are left as the live APIs gave them.
pub fn attribute_to_host(report: &mut ProbeReport) {
    for p in [
        &mut report.accessibility,
        &mut report.listen_event,
        &mut report.post_event,
        &mut report.microphone,
    ] {
        p.state = ProbeState::DevHostAttributed;
    }
    report.dev_host_attributed = true;
}

/// The team id in a Developer ID / Apple Development leaf summary:
/// `"Developer ID Application: Wilson Guenther (VHYV8C2JNU)"` → `VHYV8C2JNU`.
pub fn team_of(authority: &str) -> Option<&str> {
    let a = authority.trim_end();
    let inner = a.strip_suffix(')')?;
    let open = inner.rfind('(')?;
    let team = &inner[open + 1..];
    (!team.is_empty()).then_some(team)
}

/// Did the last launch run under a different authority or team?
pub fn identity_changed(last_run: &SigningSignature, running: &SigningSignature) -> bool {
    last_run.authority != running.authority
        || team_of(&last_run.authority) != team_of(&running.authority)
}

/// Is a grant the ledger recorded under `ledger` pinned to a different identity
/// from `running`? TCC pins a certificate-signed grant to the designated
/// requirement (identifier + team), which survives a version bump and a new
/// cdhash — so only an authority / team change counts, exactly as
/// [`identity_changed`] decides. An ad-hoc or unsigned build has no team and
/// its requirement IS the cdhash, so for those a cdhash change counts too.
pub fn grant_identity_changed(ledger: &SigningSignature, running: &SigningSignature) -> bool {
    if identity_changed(ledger, running) {
        return true;
    }
    team_of(&running.authority).is_none() && ledger.cdhash != running.cdhash
}

/// THE classification, pure.
///
/// - Granted is Granted, whatever any record says: the live API is the truth.
/// - Denied becomes StaleIdentity when the ledger saw this grant work under a
///   different authority / team ([`grant_identity_changed`]; a same-team
///   update is NOT a different identity), OR the last launch ran under a different authority /
///   team (the 0.9.0 case: no ledger, because the ledger shipped after it).
/// - NotDetermined and Unknown pass through: nobody refused anything.
pub fn classify(
    raw: RawAnswer,
    ledger: Option<&SigningSignature>,
    last_run: Option<&SigningSignature>,
    running: &SigningSignature,
) -> ProbeState {
    match raw {
        RawAnswer::Granted => ProbeState::Granted,
        RawAnswer::NotDetermined => ProbeState::NotDetermined,
        RawAnswer::Unknown => ProbeState::Unknown,
        RawAnswer::Denied => {
            let ledger_other = ledger.is_some_and(|l| grant_identity_changed(l, running));
            let last_run_other = last_run.is_some_and(|l| identity_changed(l, running));
            if ledger_other || last_run_other {
                ProbeState::StaleIdentity
            } else {
                ProbeState::Denied
            }
        }
    }
}

/// The ledger entry for a service. The `perm_truth` ledger records the two
/// grants it has always recorded; PostEvent and Microphone have no entry and
/// are caught by the last-run signature alone.
pub fn ledger_entry(ledger: &TrustLedger, service: Service) -> Option<&SigningSignature> {
    match service {
        Service::Accessibility => ledger.accessibility.as_ref(),
        Service::ListenEvent => ledger.input_monitoring.as_ref(),
        Service::PostEvent | Service::Microphone => None,
    }
}

// ── The report ─────────────────────────────────────────────────────────────

/// What the live calls answered, as plain values (so the whole report is a
/// pure function of them — see [`build_probe_report`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawProbes {
    pub accessibility: bool,
    pub listen_event_cg: bool,
    pub listen_event_hid: GrantState,
    pub post_event: bool,
    pub microphone: MicAuth,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceProbe {
    pub service: Service,
    pub state: ProbeState,
    pub raw: RawAnswer,
    pub label: &'static str,
    pub api: &'static str,
    /// Set when the APIs behind one service disagree (ListenEvent only).
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReport {
    pub accessibility: ServiceProbe,
    pub listen_event: ServiceProbe,
    pub post_event: ServiceProbe,
    pub microphone: ServiceProbe,
    /// `CGPreflightListenEventAccess()`.
    pub listen_event_cg: bool,
    /// `IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)`.
    pub listen_event_hid: GrantState,
    /// The two ListenEvent answers disagree.
    pub listen_event_disagree: bool,
    /// The last launch ran under a different authority / team.
    pub signature_changed: bool,
    /// yap-dev-perm-probe-attribution — every state is DevHostAttributed: this
    /// process is not its own signed bundle, so the answers are its host's.
    pub dev_host_attributed: bool,
    /// The raw AVFoundation answer (Restricted is not representable above).
    #[serde(skip)]
    pub microphone_auth: MicAuth,
}

impl ProbeReport {
    pub fn get(&self, service: Service) -> &ServiceProbe {
        match service {
            Service::Accessibility => &self.accessibility,
            Service::ListenEvent => &self.listen_event,
            Service::PostEvent => &self.post_event,
            Service::Microphone => &self.microphone,
        }
    }
}

fn bool_answer(b: bool) -> RawAnswer {
    if b {
        RawAnswer::Granted
    } else {
        RawAnswer::Denied
    }
}

/// ListenEvent's two answers, combined. Either one saying Granted is Granted
/// (a working listen grant is never reported broken); both are kept and a
/// disagreement is reported in `note`.
pub fn listen_event_answer(cg: bool, hid: GrantState) -> (RawAnswer, Option<String>) {
    let raw = match (cg, hid) {
        (true, _) | (_, GrantState::Authorized) => RawAnswer::Granted,
        (false, GrantState::Denied) => RawAnswer::Denied,
        (false, GrantState::Unknown) => RawAnswer::NotDetermined,
    };
    let disagree = match hid {
        GrantState::Authorized => !cg,
        GrantState::Denied => cg,
        GrantState::Unknown => cg,
    };
    let note = disagree.then(|| {
        format!(
            "CGPreflightListenEventAccess = {cg}, IOHIDCheckAccess = {} — the two disagree",
            hid.as_str()
        )
    });
    (raw, note)
}

pub fn microphone_answer(status: MicAuth) -> RawAnswer {
    match status {
        MicAuth::Authorized => RawAnswer::Granted,
        MicAuth::Denied | MicAuth::Restricted => RawAnswer::Denied,
        MicAuth::NotDetermined => RawAnswer::NotDetermined,
    }
}

/// The whole report, pure.
pub fn build_probe_report(
    raw: RawProbes,
    ledger: &TrustLedger,
    last_run: Option<&SigningSignature>,
    running: &SigningSignature,
) -> ProbeReport {
    let (listen_raw, listen_note) = listen_event_answer(raw.listen_event_cg, raw.listen_event_hid);
    let one = |service: Service, answer: RawAnswer, note: Option<String>| ServiceProbe {
        service,
        state: classify(answer, ledger_entry(ledger, service), last_run, running),
        raw: answer,
        label: service.label(),
        api: service.api(),
        note,
    };
    ProbeReport {
        accessibility: one(Service::Accessibility, bool_answer(raw.accessibility), None),
        listen_event: one(Service::ListenEvent, listen_raw, listen_note.clone()),
        post_event: one(Service::PostEvent, bool_answer(raw.post_event), None),
        microphone: one(Service::Microphone, microphone_answer(raw.microphone), None),
        listen_event_cg: raw.listen_event_cg,
        listen_event_hid: raw.listen_event_hid,
        listen_event_disagree: listen_note.is_some(),
        signature_changed: last_run.is_some_and(|l| identity_changed(l, running)),
        dev_host_attributed: false,
        microphone_auth: raw.microphone,
    }
}

// ── Live calls (the ONLY grant FFI in Yap) ─────────────────────────────────

#[cfg(target_os = "macos")]
mod ffi {
    use super::GrantState;
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::string::CFString;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightListenEventAccess() -> bool;
        fn CGPreflightPostEventAccess() -> bool;
        fn CGRequestPostEventAccess() -> bool;
    }

    /// `kIOHIDRequestTypeListenEvent`.
    const K_IOHID_REQUEST_TYPE_LISTEN_EVENT: u32 = 0;
    /// `IOHIDAccessType`: Granted, Denied, Unknown.
    const K_IOHID_ACCESS_TYPE_GRANTED: u32 = 0;
    const K_IOHID_ACCESS_TYPE_DENIED: u32 = 1;

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOHIDCheckAccess(request: u32) -> u32;
        fn IOHIDRequestAccess(request: u32) -> bool;
    }

    pub fn ax_trusted() -> bool {
        // SAFETY: no arguments, returns a C bool.
        unsafe { AXIsProcessTrusted() }
    }

    /// `AXIsProcessTrustedWithOptions({kAXTrustedCheckOptionPrompt: true})` —
    /// may draw the system dialog, which also (re)adds Yap to the list.
    pub fn ax_trusted_prompt() -> bool {
        let key = CFString::new("AXTrustedCheckOptionPrompt");
        let val = CFBoolean::true_value();
        let dict = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), val.as_CFType())]);
        // SAFETY: a valid CFDictionary that outlives the call.
        unsafe { AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef()) }
    }

    pub fn listen_event_cg() -> bool {
        // SAFETY: no arguments, returns a C bool; never prompts.
        unsafe { CGPreflightListenEventAccess() }
    }

    pub fn post_event_cg() -> bool {
        // SAFETY: no arguments, returns a C bool; never prompts.
        unsafe { CGPreflightPostEventAccess() }
    }

    pub fn listen_event_hid() -> GrantState {
        // SAFETY: plain enum in, plain enum out; never prompts.
        match unsafe { IOHIDCheckAccess(K_IOHID_REQUEST_TYPE_LISTEN_EVENT) } {
            K_IOHID_ACCESS_TYPE_GRANTED => GrantState::Authorized,
            K_IOHID_ACCESS_TYPE_DENIED => GrantState::Denied,
            _ => GrantState::Unknown,
        }
    }

    /// May draw the PostEvent dialog (which adds the app to Accessibility).
    pub fn post_event_request() -> bool {
        // SAFETY: no arguments, C bool out.
        unsafe { CGRequestPostEventAccess() }
    }

    /// May draw the Input Monitoring dialog (which adds Yap to the list).
    pub fn listen_event_request() -> bool {
        // SAFETY: plain enum in, C bool out.
        unsafe { IOHIDRequestAccess(K_IOHID_REQUEST_TYPE_LISTEN_EVENT) }
    }
}

#[cfg(not(target_os = "macos"))]
mod ffi {
    use super::GrantState;
    pub fn ax_trusted() -> bool {
        true
    }
    pub fn ax_trusted_prompt() -> bool {
        true
    }
    pub fn listen_event_cg() -> bool {
        false
    }
    pub fn post_event_cg() -> bool {
        false
    }
    pub fn listen_event_hid() -> GrantState {
        GrantState::Unknown
    }
    pub fn listen_event_request() -> bool {
        false
    }
    pub fn post_event_request() -> bool {
        false
    }
}

/// Accessibility: `AXIsProcessTrusted`. Never prompts.
pub fn accessibility_trusted() -> bool {
    ffi::ax_trusted()
}

/// Accessibility with the system prompt.
pub fn request_accessibility_prompt() -> bool {
    ffi::ax_trusted_prompt()
}

/// ListenEvent, IOKit's half: `IOHIDCheckAccess(ListenEvent)`.
pub fn listen_event_hid() -> GrantState {
    ffi::listen_event_hid()
}

/// ListenEvent, CoreGraphics' half: `CGPreflightListenEventAccess`.
pub fn listen_event_cg() -> bool {
    ffi::listen_event_cg()
}

/// PostEvent: `CGPreflightPostEventAccess`.
pub fn post_event_trusted() -> bool {
    ffi::post_event_cg()
}

/// Every live call, once.
pub fn raw_probes() -> RawProbes {
    RawProbes {
        accessibility: accessibility_trusted(),
        listen_event_cg: listen_event_cg(),
        listen_event_hid: listen_event_hid(),
        post_event: post_event_trusted(),
        microphone: crate::mic_auth::authorization_status(),
    }
}

/// THE probe. Live API calls, the `perm_truth` ledger, the last-run signature.
///
/// yap-dev-perm-probe-attribution: when this process is not running as its own
/// signed bundle ([`this_process_runs_as_bundle`]), every service is marked
/// [`ProbeState::DevHostAttributed`] — the answers are the launching
/// terminal's grants, not Yap's. The installed build is unchanged.
pub fn probe_all() -> ProbeReport {
    let me = crate::perm_truth::running_identity();
    let mut report = build_probe_report(
        raw_probes(),
        &crate::perm_truth::ledger_snapshot(),
        previous_run_signature(),
        &me.signature,
    );
    if !this_process_runs_as_bundle() {
        attribute_to_host(&mut report);
    }
    report
}

/// One service's state, freshly probed (the re-probe loop's reader).
pub fn probe_service(service: Service) -> ProbeState {
    probe_all().get(service).state
}

// ── Last-run signature ─────────────────────────────────────────────────────

const LAST_RUN_FILE: &str = "last-run-signature.json";

fn last_run_path() -> PathBuf {
    crate::data_dir().join(LAST_RUN_FILE)
}

/// The signature the PREVIOUS launch ran under. Read once per process, and the
/// running signature written in its place in the same step — at EVERY launch,
/// so the next identity change is caught even when no grant ever read true.
pub fn previous_run_signature() -> Option<&'static SigningSignature> {
    static PREV: OnceLock<Option<SigningSignature>> = OnceLock::new();
    PREV.get_or_init(|| {
        let path = last_run_path();
        let prev: Option<SigningSignature> = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        let me = crate::perm_truth::running_identity();
        match serde_json::to_vec_pretty(&me.signature) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    log::warn!("perm-probe: could not persist {LAST_RUN_FILE}: {e}");
                }
            }
            Err(e) => log::warn!("perm-probe: could not encode signature: {e}"),
        }
        if let Some(p) = &prev {
            if identity_changed(p, &me.signature) {
                log::warn!(
                    "perm-probe: signing identity changed since last launch: {:?} -> {:?}",
                    p.authority,
                    me.signature.authority
                );
            }
        }
        prev
    })
    .as_ref()
}

/// Launch-time: persist the last-run signature (off the main thread).
pub fn prime() {
    let _ = std::thread::Builder::new()
        .name("perm-probe-prime".into())
        .spawn(|| {
            let _ = previous_run_signature();
        });
}

// ── Repair ─────────────────────────────────────────────────────────────────

pub const TCCUTIL: &str = "/usr/bin/tccutil";

/// The exact argv the app runs for one service. Pure. Always Yap's OWN bundle
/// id (from `NSBundle.mainBundle` at runtime), never `All`.
pub fn repair_argv(service: Service, bundle_id: &str) -> Vec<String> {
    vec![
        TCCUTIL.to_string(),
        "reset".to_string(),
        service.tcc_name().to_string(),
        bundle_id.to_string(),
    ]
}

/// Every argv one Repair runs. Accessibility ALSO resets PostEvent: the ⌘V
/// insertion bridge posts events and depends on it.
pub fn repair_plan(service: Service, bundle_id: &str) -> Vec<Vec<String>> {
    match service {
        Service::Accessibility => vec![
            repair_argv(Service::Accessibility, bundle_id),
            repair_argv(Service::PostEvent, bundle_id),
        ],
        other => vec![repair_argv(other, bundle_id)],
    }
}

/// The `kTCCService*` names one Repair of `service` resets, in plan order.
/// The row's OWN service is always first: a row never resets only somebody
/// else's grant (the 0.9.2 Accessibility-titled row reset only PostEvent).
pub fn reset_services(service: Service) -> Vec<String> {
    repair_plan(service, "_")
        .iter()
        .map(|argv| format!("kTCCService{}", argv[2]))
        .collect()
}

/// What a Repair says it did: "Reset kTCCServiceAccessibility and
/// kTCCServicePostEvent for Yap Test."
pub fn repair_summary(service: Service, product: &str) -> String {
    format!("Reset {} for {product}.", reset_services(service).join(" and "))
}

/// The deep link the Repair opens, through the `open_privacy_pane` allow-list.
pub fn repair_pane_url(service: Service) -> &'static str {
    crate::permissions::open_privacy_pane_urls(service.pane_name())[0]
}

/// Input Monitoring usually only takes effect after a quit and reopen.
pub fn needs_relaunch(service: Service) -> bool {
    service == Service::ListenEvent
}

/// The manual steps shown when tccutil fails.
pub fn manual_steps(service: Service, bundle_path: &str) -> String {
    manual_steps_for(service, bundle_path, "Yap")
}

/// [`manual_steps`], naming the running product ("Yap Test" on the test channel).
pub fn manual_steps_for(service: Service, bundle_path: &str, product: &str) -> String {
    let pane = service.settings_list();
    format!(
        "Open System Settings → Privacy & Security → {pane}, select {product}, click the minus button, \
         then click plus, choose {bundle_path} and switch it on."
    )
}

/// What one Repair click did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairOutcome {
    pub service: Service,
    /// Each argv run, space-joined.
    pub commands: Vec<String>,
    pub ok: bool,
    /// tccutil's stderr when it failed.
    pub stderr: String,
    /// Shown only when `ok` is false.
    pub manual_steps: String,
    pub pane_url: &'static str,
    pub needs_relaunch: bool,
    /// The `kTCCService*` names this Repair reset, in order.
    pub reset: Vec<String>,
    /// The product the UI names ("Yap" / "Yap Test").
    pub product: String,
    /// [`repair_summary`]: which services were reset, for whom.
    pub summary: String,
}

/// Run one Repair: tccutil reset for our own bundle id, the prompt, the pane.
pub fn repair(service: Service) -> RepairOutcome {
    let me = crate::perm_truth::running_identity();
    let product = crate::perm_truth::product_name();
    let mut outcome = RepairOutcome {
        service,
        commands: Vec::new(),
        ok: false,
        stderr: String::new(),
        manual_steps: manual_steps_for(service, &me.bundle_path, &product),
        pane_url: repair_pane_url(service),
        needs_relaunch: needs_relaunch(service),
        reset: reset_services(service),
        summary: repair_summary(service, &product),
        product: product.clone(),
    };
    // yap-dev-perm-probe-attribution: a host-attributed process's grants are
    // its terminal's. A tccutil reset here would clear an identity macOS never
    // consulted for this process, so Repair refuses outright.
    if !this_process_runs_as_bundle() {
        outcome.stderr = format!("{DEV_HOST_LINE} — Repair would reset the wrong identity.");
        return outcome;
    }
    // NEVER fall back to the shipped id: an unbundled dev binary resetting the
    // installed app's grants would be repairing somebody else.
    let Some(bundle_id) = crate::perm_truth::main_bundle_id() else {
        outcome.stderr =
            format!("{product} is not running from an app bundle, so it has no bundle id to reset.");
        return outcome;
    };
    for argv in repair_plan(service, &bundle_id) {
        outcome.commands.push(argv.join(" "));
        match std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .output()
        {
            Ok(out) if out.status.success() => {}
            Ok(out) => {
                outcome.stderr = format!(
                    "{} exited {}: {}",
                    argv.join(" "),
                    out.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&out.stderr).trim()
                );
                log::warn!("perm-repair: {}", outcome.stderr);
                return outcome;
            }
            Err(e) => {
                outcome.stderr = format!("{}: {e}", argv.join(" "));
                log::warn!("perm-repair: {}", outcome.stderr);
                return outcome;
            }
        }
    }
    log::info!(
        "perm-repair: {} row ({:?}) reset {}: {:?}",
        service.label(),
        service,
        outcome.reset.join(" + "),
        outcome.commands
    );
    match service {
        Service::Accessibility => {
            let _ = request_accessibility_prompt();
        }
        // The reset PostEvent entry only comes back through ITS OWN request;
        // the AX prompt never re-adds it.
        Service::PostEvent => {
            let _ = ffi::post_event_request();
        }
        Service::ListenEvent => {
            let _ = ffi::listen_event_request();
        }
        // The microphone prompt is drawn by `request_microphone`, which the
        // frontend calls on a successful Microphone repair.
        Service::Microphone => {}
    }
    if let Err(e) = crate::permissions::open_privacy_pane(service.pane_name()) {
        log::warn!("perm-repair: could not open {}: {e}", outcome.pane_url);
    }
    outcome.ok = true;
    outcome
}

// ── Re-probe ───────────────────────────────────────────────────────────────

pub const REPROBE_INTERVAL: Duration = Duration::from_secs(1);
pub const REPROBE_TIMEOUT: Duration = Duration::from_secs(120);
/// The event the re-probe publishes on every phase change.
pub const REPAIR_EVENT: &str = "permission_repair";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairPhase {
    /// "Waiting for you to switch Yap on…"
    Waiting,
    /// The probe flipped to Granted.
    Fixed,
    /// The probe flipped for ListenEvent: "Granted — quit and reopen Yap to finish".
    Relaunch,
    /// 120 s and nothing flipped.
    TimedOut,
}

/// The phase a Granted probe means for a service.
pub fn granted_phase(service: Service) -> RepairPhase {
    if needs_relaunch(service) {
        RepairPhase::Relaunch
    } else {
        RepairPhase::Fixed
    }
}

/// The English line for a phase (logs; the frontend renders its own copy of
/// the same words from `permissionRepair.ts`).
pub fn phase_message(service: Service, phase: RepairPhase) -> String {
    phase_message_for(service, phase, "Yap")
}

/// [`phase_message`], naming the running product ("Yap Test" on the test channel).
pub fn phase_message_for(service: Service, phase: RepairPhase, product: &str) -> String {
    match phase {
        RepairPhase::Waiting => format!("Waiting for you to switch {product} on…"),
        RepairPhase::Fixed => "Fixed".into(),
        RepairPhase::Relaunch => format!("Granted — quit and reopen {product} to finish"),
        RepairPhase::TimedOut => format!(
            "{product} is still not allowed in {}. Switch {product} on there, then click Repair again.",
            service.settings_list()
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairEvent {
    pub service: Service,
    pub phase: RepairPhase,
    pub message: String,
}

impl RepairEvent {
    pub fn new(service: Service, phase: RepairPhase) -> Self {
        Self {
            service,
            phase,
            message: phase_message_for(service, phase, &crate::perm_truth::product_name()),
        }
    }
}

/// Re-probe every [`REPROBE_INTERVAL`] for [`REPROBE_TIMEOUT`]. Emits Waiting,
/// then exactly one terminal phase: Fixed / Relaunch the moment the probe reads
/// Granted, or TimedOut. Probe, sleep and emit are injected so the loop is
/// testable with no grants and no clock.
pub fn reprobe<P, S, E>(service: Service, mut probe: P, mut sleep: S, mut emit: E) -> RepairPhase
where
    P: FnMut() -> ProbeState,
    S: FnMut(Duration),
    E: FnMut(RepairPhase),
{
    emit(RepairPhase::Waiting);
    let ticks = REPROBE_TIMEOUT.as_millis() / REPROBE_INTERVAL.as_millis();
    for _ in 0..ticks {
        sleep(REPROBE_INTERVAL);
        if probe() == ProbeState::Granted {
            let phase = granted_phase(service);
            emit(phase);
            return phase;
        }
    }
    emit(RepairPhase::TimedOut);
    RepairPhase::TimedOut
}

#[cfg(test)]
mod running_as_bundle_tests {
    use super::*;

    fn sig(authority: &str) -> SigningSignature {
        SigningSignature {
            authority: authority.into(),
            cdhash: "ab12".into(),
            version: "0.9.3".into(),
        }
    }
    const DEV_ID: &str = "Developer ID Application: Wilson Guenther (VHYV8C2JNU)";
    const BUNDLE_ID: &str = "com.wilsonguenther.wilson-voice";

    #[test]
    fn running_as_bundle_installed_developer_id_is_true() {
        assert!(running_as_bundle(
            "/Applications/Yap.app/Contents/MacOS/wilson-voice",
            Some(BUNDLE_ID),
            &sig(DEV_ID),
        ));
    }

    #[test]
    fn running_as_bundle_test_channel_apple_development_is_true() {
        assert!(running_as_bundle(
            "/Applications/Yap Test.app/Contents/MacOS/wilson-voice",
            Some("com.wilsonguenther.wilson-voice.test"),
            &sig("Apple Development: Wilson Guenther (ABCDE12345)"),
        ));
    }

    #[test]
    fn running_as_bundle_dev_binary_ad_hoc_is_false() {
        assert!(!running_as_bundle(
            "/Users/w/code/wilson-voice/desktop/target/debug/wilson-voice",
            None,
            &sig("ad-hoc"),
        ));
        // A Tauri dev binary can carry an embedded Info.plist, so NSBundle may
        // report the shipped id: still not a bundle.
        assert!(!running_as_bundle(
            "desktop/target/debug/wilson-voice",
            Some(BUNDLE_ID),
            &sig("ad-hoc"),
        ));
    }

    #[test]
    fn running_as_bundle_needs_all_three() {
        let app = "/Applications/Yap.app/Contents/MacOS/wilson-voice";
        // ad-hoc / unsigned bundle: no team, no designated requirement
        assert!(!running_as_bundle(app, Some(BUNDLE_ID), &sig("ad-hoc")));
        assert!(!running_as_bundle(app, Some(BUNDLE_ID), &sig("unsigned")));
        // no bundle id
        assert!(!running_as_bundle(app, None, &sig(DEV_ID)));
        assert!(!running_as_bundle(app, Some("  "), &sig(DEV_ID)));
        // Developer ID signed, but not inside Contents/MacOS of an .app
        for path in [
            "/usr/local/bin/wilson-voice",
            "/x/Yap.app/wilson-voice",
            "/x/.app/Contents/MacOS/wilson-voice",
            "/x/Yap/Contents/MacOS/wilson-voice",
        ] {
            assert!(
                !running_as_bundle(path, Some(BUNDLE_ID), &sig(DEV_ID)),
                "{path}"
            );
        }
    }

    fn report(ax: bool, post: bool) -> ProbeReport {
        build_probe_report(
            RawProbes {
                accessibility: ax,
                listen_event_cg: true,
                listen_event_hid: GrantState::Authorized,
                post_event: post,
                microphone: MicAuth::Authorized,
            },
            &TrustLedger::default(),
            None,
            &sig("ad-hoc"),
        )
    }

    #[test]
    fn running_as_bundle_false_marks_every_service_dev_host_attributed() {
        let mut r = report(true, false);
        assert!(!r.dev_host_attributed);
        attribute_to_host(&mut r);
        assert!(r.dev_host_attributed);
        for s in Service::ALL {
            assert_eq!(r.get(s).state, ProbeState::DevHostAttributed, "{s:?}");
        }
        // The raw answers survive: the process really can (or cannot) act.
        assert_eq!(r.accessibility.raw, RawAnswer::Granted);
        assert_eq!(r.post_event.raw, RawAnswer::Denied);
        assert!(r.accessibility.usable());
        assert!(!r.post_event.usable());
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["accessibility"]["state"], "dev_host_attributed");
        assert_eq!(json["devHostAttributed"], true);
    }

    #[test]
    fn running_as_bundle_repair_refuses_in_a_host_attributed_process() {
        // The unit-test binary is itself an unbundled, ad hoc signed binary:
        // exactly the dev-build case. Repair must refuse before any tccutil.
        assert!(!this_process_runs_as_bundle());
        let out = repair(Service::Accessibility);
        assert!(!out.ok);
        assert!(out.commands.is_empty(), "{:?}", out.commands);
        assert!(out.stderr.starts_with(DEV_HOST_LINE), "{}", out.stderr);
    }

    #[test]
    fn running_as_bundle_reconcile_keeps_host_attribution() {
        let mut r = report(true, true);
        attribute_to_host(&mut r);
        crate::permissions::reconcile_listen_event(&mut r.listen_event, GrantState::Authorized, "Yap");
        assert_eq!(r.listen_event.state, ProbeState::DevHostAttributed);
    }

    #[test]
    fn running_as_bundle_true_keeps_states_and_usable_is_granted_only() {
        let r = report(true, false);
        assert_eq!(r.accessibility.state, ProbeState::Granted);
        assert!(r.accessibility.usable());
        assert_eq!(r.post_event.state, ProbeState::Denied);
        assert!(!r.post_event.usable());
    }
}
