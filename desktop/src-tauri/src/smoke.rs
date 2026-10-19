//! Y0-D — `--smoke`: the launch mode an automated agent is allowed to use.
//!
//! A normal Yap launch does three things that make it unsafe to start from a
//! script on Wilson's machine: it opens the SQLite history and `settings.json`
//! under the shared state root, it installs a system-wide keyboard tap plus four
//! global shortcut registrations, and it synthesizes ⌘V into whatever window
//! happens to be frontmost. Two lanes doing that at once — with Wilson typing in
//! a third window — is a corrupted history and a paste into his editor.
//!
//! `--smoke` is the launch that cannot do any of it:
//!
//! 1. it registers NO global hotkey (neither the CGEvent PTT tap nor the
//!    `global_shortcut` accelerators),
//! 2. it synthesizes NO paste and no delete keystroke (transcripts still land on
//!    the clipboard, so nothing is lost — that is a refusal, not a degradation),
//! 3. it REFUSES TO START at all unless [`crate::DATA_DIR_ENV`] points somewhere
//!    outside the default state root, and
//! 4. each start refusal exits NON-ZERO with a named message, instead of quietly
//!    falling back to the real data dir.
//!
//! Rule (3) is the one that matters: a smoke run that silently used the default
//! root would be reading and writing Wilson's real dictation history, which is
//! the entire failure this mode exists to prevent.
//!
//! yap-p0-release-smoke — `--release-smoke` is the OTHER automated launch, and
//! the opposite of `--smoke`: it exists to prove, on a freshly signed build, that
//! the things `--smoke` refuses actually work. It registers the hotkeys, probes
//! every TCC service through `perm_probe::probe_all`, makes exactly ONE
//! insertion — and only into TextEdit, checked against the frontmost app first —
//! then quits. Every step is logged as one line built by the pure formatters
//! below (`release-smoke step=...`), and `scripts/qa/release-smoke.sh --parse`
//! is the single reader of those lines. It shares rule (3): no launch against
//! the real state root.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// The flag that requests smoke mode.
pub const SMOKE_FLAG: &str = "--smoke";

/// The flag that requests a release smoke (see the module docs).
pub const RELEASE_SMOKE_FLAG: &str = "--release-smoke";

/// What the release smoke inserts; `release-smoke.sh` compares the TextEdit
/// document against exactly this string.
pub const RELEASE_SMOKE_TEXT: &str = "yap release smoke ok";

/// The only app a release smoke may insert into.
pub const RELEASE_SMOKE_TARGET: &str = "com.apple.TextEdit";

/// Exit status for a refused `--smoke` launch. `EX_CONFIG` from `sysexits.h`:
/// the request was well-formed, the environment it was made in was not.
pub const EXIT_REFUSED: i32 = 78;

/// Logged (at WARN) when smoke mode declines to install a global hotkey.
pub const HOTKEY_REFUSAL: &str =
    "--smoke: refusing to register any global hotkey (no PTT tap, no ⌃⌘V/⌃⌘M/⌃⌘Z/⌘⇧V)";

/// Logged (at WARN) when smoke mode declines to synthesize ⌘V.
pub const PASTE_REFUSAL: &str =
    "--smoke: refusing to synthesize a paste — transcript left on the clipboard";

/// Logged (at WARN) when smoke mode declines to synthesize a Delete keystroke.
pub const DELETE_REFUSAL: &str =
    "--smoke: refusing to synthesize a keystroke into the frontmost app";

/// Why a `--smoke` launch was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// `YAP_DATA_DIR` was unset, empty or whitespace.
    DataDirUnset,
    /// `YAP_DATA_DIR` resolved to the default state root (or inside it), which
    /// is exactly the history a smoke run must not touch.
    DataDirIsDefaultRoot(PathBuf),
}

impl Refusal {
    /// Stable, greppable name for this refusal — what a harness matches on.
    pub fn name(&self) -> &'static str {
        match self {
            Refusal::DataDirUnset => "SMOKE_REFUSED_DATA_DIR_UNSET",
            Refusal::DataDirIsDefaultRoot(_) => "SMOKE_REFUSED_DATA_DIR_IS_DEFAULT_ROOT",
        }
    }

    /// The message printed on stderr before the process exits.
    pub fn message(&self) -> String {
        match self {
            Refusal::DataDirUnset => format!(
                "{}: {SMOKE_FLAG} / {RELEASE_SMOKE_FLAG} requires {} to name a scratch state root. \
                 It is unset, so this launch would read and write the real dictation history. Refusing to start.",
                self.name(),
                crate::DATA_DIR_ENV
            ),
            Refusal::DataDirIsDefaultRoot(root) => format!(
                "{}: {SMOKE_FLAG} / {RELEASE_SMOKE_FLAG} was given {}={} , which is the default state root. \
                 A smoke run must not touch the real dictation history. Refusing to start.",
                self.name(),
                crate::DATA_DIR_ENV,
                root.display()
            ),
        }
    }

    /// Never zero — a refused smoke launch must fail its caller.
    pub fn exit_code(&self) -> i32 {
        EXIT_REFUSED
    }
}

/// Decide whether this launch is a smoke run of EITHER kind. Shared by
/// [`preflight`] and [`release_preflight`]: both refuse the default state root.
fn preflight_flag(argv: &[String], env_root: Option<&str>, flag: &str) -> Result<bool, Refusal> {
    if !argv.iter().any(|a| a == flag) {
        return Ok(false);
    }
    let paths = crate::app_paths::resolve(env_root);
    if !paths.overridden {
        return Err(Refusal::DataDirUnset);
    }
    if crate::app_paths::is_default_root(&paths.root) {
        return Err(Refusal::DataDirIsDefaultRoot(paths.root));
    }
    Ok(true)
}

/// Decide, from argv and the override value, whether this launch is a smoke run.
///
/// `Ok(false)` — a normal launch, nothing changes. `Ok(true)` — a smoke launch
/// that may proceed. `Err(_)` — a smoke launch that must exit non-zero.
///
/// Pure: no environment read, no process exit, no logging, so it is drivable
/// from a test.
pub fn preflight(argv: &[String], env_root: Option<&str>) -> Result<bool, Refusal> {
    preflight_flag(argv, env_root, SMOKE_FLAG)
}

/// [`preflight`] for `--release-smoke`. Same refusals: a release smoke runs
/// against a scratch state root (release-smoke.sh copies the host's two
/// signature ledgers into it, read-only, so a stale identity is still visible).
pub fn release_preflight(argv: &[String], env_root: Option<&str>) -> Result<bool, Refusal> {
    preflight_flag(argv, env_root, RELEASE_SMOKE_FLAG)
}

static ARMED: AtomicBool = AtomicBool::new(false);

/// Arm smoke mode for the rest of the process. Called once, from `run()`, only
/// after [`preflight`] returned `Ok(true)`. There is no disarm: a process that
/// started as a smoke run stays one.
pub fn arm() {
    ARMED.store(true, Ordering::SeqCst);
}

/// Is this process a smoke run?
pub fn is_armed() -> bool {
    ARMED.load(Ordering::SeqCst)
}

/// May this process install a global hotkey? False for the whole life of a
/// smoke run.
pub fn global_hotkeys_allowed() -> bool {
    !is_armed()
}

/// May this process synthesize a keystroke (⌘V, Delete) into another app?
/// Never in a `--smoke` run; in a `--release-smoke` run only while the single
/// smoke insertion holds its permit ([`take_insert_permit`]).
pub fn paste_allowed() -> bool {
    !is_armed() && (!release_armed() || INSERT_PERMIT.load(Ordering::SeqCst))
}

// ── --release-smoke ────────────────────────────────────────────────────────

static RELEASE_ARMED: AtomicBool = AtomicBool::new(false);
static INSERT_PERMIT: AtomicBool = AtomicBool::new(false);
static INSERT_SPENT: AtomicBool = AtomicBool::new(false);

/// Arm release-smoke mode for the rest of the process (after
/// [`release_preflight`] returned `Ok(true)`). No disarm.
pub fn arm_release() {
    RELEASE_ARMED.store(true, Ordering::SeqCst);
}

/// Is this process a `--release-smoke` run?
pub fn release_armed() -> bool {
    RELEASE_ARMED.load(Ordering::SeqCst)
}

/// The ONE insertion a release smoke may make. `true` exactly once per process;
/// [`paste_allowed`] is true only between this and [`release_insert_permit`].
pub fn take_insert_permit() -> bool {
    if INSERT_SPENT.swap(true, Ordering::SeqCst) {
        return false;
    }
    INSERT_PERMIT.store(true, Ordering::SeqCst);
    true
}

/// Drop the insertion permit; nothing can take it again.
pub fn release_insert_permit() {
    INSERT_PERMIT.store(false, Ordering::SeqCst);
}

/// `perm_probe::Service` as the release-smoke line spells it.
pub fn service_token(service: crate::perm_probe::Service) -> &'static str {
    use crate::perm_probe::Service;
    match service {
        Service::Accessibility => "accessibility",
        Service::ListenEvent => "listenevent",
        Service::PostEvent => "postevent",
        Service::Microphone => "microphone",
    }
}

/// `release-smoke step=probe service=<s> state=<ProbeState> ledger=<granted|none>`
pub fn probe_line(
    service: crate::perm_probe::Service,
    state: crate::perm_probe::ProbeState,
    ledger_granted: bool,
) -> String {
    format!(
        "release-smoke step=probe service={} state={state:?} ledger={}",
        service_token(service),
        if ledger_granted { "granted" } else { "none" }
    )
}

/// The fn-tap half of the hotkeys line (stable, one token, no reason text).
pub fn fn_tap_token(s: &crate::hotkeys::FnTapStatus) -> &'static str {
    use crate::hotkeys::FnTapStatus;
    match s {
        FnTapStatus::NotAttempted => "not_attempted",
        FnTapStatus::Delivering => "delivering",
        FnTapStatus::ArmedNoDelivery => "armed_no_delivery",
        FnTapStatus::Blocked(_) => "blocked",
        FnTapStatus::SecureInput(_) => "secure_input",
    }
}

/// `release-smoke step=hotkeys carbon=<registered|failed> fn_tap=<status>`.
/// Anything but a live Carbon registration is `failed` — a register call that
/// never ran is not a pass.
pub fn hotkeys_line(
    carbon: &crate::hotkeys::CarbonStatus,
    fn_tap: &crate::hotkeys::FnTapStatus,
) -> String {
    let c = match carbon {
        crate::hotkeys::CarbonStatus::Registered => "registered",
        _ => "failed",
    };
    format!(
        "release-smoke step=hotkeys carbon={c} fn_tap={}",
        fn_tap_token(fn_tap)
    )
}

/// `release-smoke step=insert path=<Ax|PasteBridge|CopyCard> ok=<bool>`
pub fn insert_line(path: crate::paste::InsertPath, ok: bool) -> String {
    format!("release-smoke step=insert path={path:?} ok={ok}")
}

/// `release-smoke step=quit ok=true`
pub fn quit_line() -> String {
    "release-smoke step=quit ok=true".to_string()
}

/// Logged (WARN) when the insert step refuses because TextEdit is not frontmost.
pub fn not_textedit_line(frontmost: Option<&str>) -> String {
    format!(
        "release-smoke: refusing to insert — frontmost is {}, not {RELEASE_SMOKE_TARGET}",
        frontmost.unwrap_or("nothing")
    )
}

/// The OS seam for the insert step, so a test can prove the inserter is never
/// reached unless TextEdit is frontmost.
pub trait InsertHost {
    fn frontmost_bundle_id(&self) -> Option<String>;
    /// The path the live probe would pick right now.
    fn insert_path(&self) -> crate::paste::InsertPath;
    /// Insert [`RELEASE_SMOKE_TEXT`]; true when it landed (or, for the Copy
    /// card, when the text reached the clipboard and the card is up).
    fn insert(&mut self, text: &str) -> bool;
}

/// The insert step: frontmost FIRST, then at most one insertion. Returns the
/// line to log.
pub fn run_insert_step(host: &mut dyn InsertHost) -> String {
    let path = host.insert_path();
    let front = host.frontmost_bundle_id();
    if front.as_deref() != Some(RELEASE_SMOKE_TARGET) {
        log::warn!("{}", not_textedit_line(front.as_deref()));
        return insert_line(path, false);
    }
    if !take_insert_permit() {
        log::warn!("release-smoke: the one insertion was already spent");
        return insert_line(path, false);
    }
    let ok = host.insert(RELEASE_SMOKE_TEXT);
    release_insert_permit();
    insert_line(path, ok)
}

struct LiveInsertHost {
    app: tauri::AppHandle,
}

impl InsertHost for LiveInsertHost {
    fn frontmost_bundle_id(&self) -> Option<String> {
        crate::focus::frontmost_bundle_id()
    }
    fn insert_path(&self) -> crate::paste::InsertPath {
        crate::paste::choose_insert_path(&crate::perm_probe::probe_all())
    }
    fn insert(&mut self, text: &str) -> bool {
        let path = self.insert_path();
        let out =
            crate::paste::insert_dictated_take(&self.app, text, true, Some(RELEASE_SMOKE_TARGET));
        if path == crate::paste::InsertPath::CopyCard {
            out.copied
        } else {
            out.pasted
        }
    }
}

fn emit(line: &str) {
    log::info!("{line}");
    log::logger().flush();
}

/// The whole `--release-smoke` run, off the main thread. Waits (≤ 5 s) for the
/// launch-time hotkey registration to report, then probe → hotkeys → insert →
/// quit, one line each, and exits the app.
pub fn spawn_release_smoke(app: tauri::AppHandle) {
    let _ = std::thread::Builder::new()
        .name("release-smoke".into())
        .spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while crate::hotkeys::status().carbon == crate::hotkeys::CarbonStatus::NotAttempted
                && std::time::Instant::now() < deadline
            {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            let report = crate::perm_probe::probe_all();
            let ledger = crate::perm_truth::ledger_snapshot();
            for service in crate::perm_probe::Service::ALL {
                let granted = crate::perm_probe::ledger_entry(&ledger, service).is_some();
                emit(&probe_line(service, report.get(service).state, granted));
            }
            let hk = crate::hotkeys::status();
            emit(&hotkeys_line(&hk.carbon, &hk.fn_tap));
            let mut host = LiveInsertHost { app: app.clone() };
            emit(&run_insert_step(&mut host));
            emit(&quit_line());
            app.exit(0);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smoke_argv() -> Vec<String> {
        vec!["--some-other-flag".into(), SMOKE_FLAG.into()]
    }

    #[test]
    fn no_flag_is_a_normal_launch_even_with_no_override() {
        assert_eq!(preflight(&[], None), Ok(false));
        assert_eq!(
            preflight(&["--transcribe-file".to_string()], None),
            Ok(false)
        );
    }

    #[test]
    fn a_scratch_root_is_accepted() {
        assert_eq!(
            preflight(&smoke_argv(), Some("/tmp/yap-smoke-unit")),
            Ok(true)
        );
    }

    #[test]
    fn refusals_are_named_and_non_zero() {
        for refusal in [
            Refusal::DataDirUnset,
            Refusal::DataDirIsDefaultRoot(crate::app_paths::default_root()),
        ] {
            assert_ne!(refusal.exit_code(), 0);
            assert!(refusal.message().contains(crate::DATA_DIR_ENV));
            assert!(refusal.message().contains(refusal.name()));
        }
    }

    #[test]
    fn release_smoke_flag_is_its_own_flag_and_shares_the_refusals() {
        let argv = vec![RELEASE_SMOKE_FLAG.to_string()];
        assert_eq!(
            preflight(&argv, None),
            Ok(false),
            "--release-smoke is not --smoke"
        );
        assert_eq!(release_preflight(&argv, None), Err(Refusal::DataDirUnset));
        assert_eq!(
            release_preflight(&argv, Some("/tmp/yap-release-smoke-unit")),
            Ok(true)
        );
        assert_eq!(
            release_preflight(&[SMOKE_FLAG.to_string()], None),
            Ok(false)
        );
    }

    #[test]
    fn release_smoke_lines_have_the_contract_shape() {
        use crate::hotkeys::{CarbonStatus, FnTapStatus};
        use crate::paste::InsertPath;
        use crate::perm_probe::{ProbeState, Service};
        assert_eq!(
            probe_line(Service::ListenEvent, ProbeState::StaleIdentity, true),
            "release-smoke step=probe service=listenevent state=StaleIdentity ledger=granted"
        );
        assert_eq!(
            probe_line(Service::Microphone, ProbeState::NotDetermined, false),
            "release-smoke step=probe service=microphone state=NotDetermined ledger=none"
        );
        assert_eq!(
            hotkeys_line(&CarbonStatus::Registered, &FnTapStatus::Delivering),
            "release-smoke step=hotkeys carbon=registered fn_tap=delivering"
        );
        assert_eq!(
            hotkeys_line(
                &CarbonStatus::NotAttempted,
                &FnTapStatus::Blocked("x".into())
            ),
            "release-smoke step=hotkeys carbon=failed fn_tap=blocked"
        );
        assert_eq!(
            insert_line(InsertPath::PasteBridge, false),
            "release-smoke step=insert path=PasteBridge ok=false"
        );
        assert_eq!(quit_line(), "release-smoke step=quit ok=true");
    }

    struct FakeHost {
        front: Option<&'static str>,
        inserts: usize,
    }
    impl InsertHost for FakeHost {
        fn frontmost_bundle_id(&self) -> Option<String> {
            self.front.map(str::to_string)
        }
        fn insert_path(&self) -> crate::paste::InsertPath {
            crate::paste::InsertPath::Ax
        }
        fn insert(&mut self, text: &str) -> bool {
            assert_eq!(text, RELEASE_SMOKE_TEXT);
            self.inserts += 1;
            true
        }
    }

    /// One test, because the insertion permit is process-wide and one-shot.
    #[test]
    fn release_smoke_never_inserts_unless_textedit_is_frontmost_and_only_once() {
        for front in [
            None,
            Some("com.apple.Terminal"),
            Some("com.apple.TextEditX"),
        ] {
            let mut h = FakeHost { front, inserts: 0 };
            assert_eq!(
                run_insert_step(&mut h),
                "release-smoke step=insert path=Ax ok=false"
            );
            assert_eq!(h.inserts, 0, "inserted with frontmost={front:?}");
        }
        let mut h = FakeHost {
            front: Some(RELEASE_SMOKE_TARGET),
            inserts: 0,
        };
        assert_eq!(
            run_insert_step(&mut h),
            "release-smoke step=insert path=Ax ok=true"
        );
        assert_eq!(h.inserts, 1);
        assert!(
            !INSERT_PERMIT.load(Ordering::SeqCst),
            "permit dropped after the insert"
        );
        assert_eq!(
            run_insert_step(&mut h),
            "release-smoke step=insert path=Ax ok=false",
            "a second insertion is refused"
        );
        assert_eq!(h.inserts, 1);
    }
}
