//! Y0-D — the state-root override and the `--smoke` launch mode.
//!
//! Until this item the app hardcoded `<Application Support>/WilsonVoice` at two
//! independent sites, so two concurrent launches (two loop lanes, plus Wilson's
//! own installed copy) shared ONE SQLite history, ONE settings store, ONE models
//! dir and ONE set of global hotkeys, and every one of them synthesized ⌘V into
//! whatever window happened to be frontmost.
//!
//! These tests are an INTEGRATION suite on purpose: they drive the crate's real
//! public accessors (`data_dir`, `recovery_dir`, `meetings_dir`,
//! `models::models_dir`) through the real environment variable, which is the
//! only way to prove the override is wired to the paths the app actually uses
//! rather than to a helper nothing calls.
//!
//! They are serialized behind one mutex because the environment and the
//! smoke-mode flag are both PROCESS-global state.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use wilson_voice_lib::{app_paths, models, smoke, DATA_DIR_ENV};

/// Every test in this file touches process-global state (`YAP_DATA_DIR`, the
/// armed smoke flag), so they run one at a time.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// A scratch root for this test process, exported into `YAP_DATA_DIR` exactly
/// once. Every test calls this before it reads any path.
fn scratch_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("yap-y0d-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create scratch root");
        // The LITERAL variable name, never `DATA_DIR_ENV`. Exporting through the
        // constant would make this suite vacuous: rename the constant and the
        // test would obediently export the new name and still pass, proving
        // nothing about the documented `YAP_DATA_DIR` contract.
        std::env::set_var("YAP_DATA_DIR", &root);
        root
    })
}

/// The override moves EVERY stateful path — history DB, settings, models,
/// recordings, recovery and meetings — and leaves the default root untouched.
///
/// This is the test that goes red when `YAP_DATA_DIR` is renamed in
/// `src/lib.rs`: the resolver then reads a variable nobody exported, falls back
/// to the default root, and the first assertion fails. The pure-resolver
/// assertions come FIRST deliberately, so a broken override fails before any
/// accessor can create a directory under the real state root.
#[test]
fn override_relocates_history_settings_models_and_recovery() {
    let _guard = serial();
    let root = scratch_root();

    let resolved = app_paths::resolve_from_env();
    assert_eq!(
        resolved.root,
        root.to_path_buf(),
        "YAP_DATA_DIR did not reach the resolver"
    );
    assert!(resolved.overridden);
    for derived in [
        &resolved.db,
        &resolved.settings,
        &resolved.models,
        &resolved.recordings,
        &resolved.recovery,
        &resolved.meetings,
    ] {
        assert!(
            derived.starts_with(root),
            "{} escaped the override",
            derived.display()
        );
        assert!(
            !derived.starts_with(app_paths::default_root()),
            "{} still points at the real state root",
            derived.display()
        );
    }

    // The real accessors the app calls — not just the helper — must agree.
    assert_eq!(wilson_voice_lib::data_dir(), root.to_path_buf());
    assert!(wilson_voice_lib::recovery_dir().starts_with(root));
    assert!(wilson_voice_lib::meetings_dir().starts_with(root));
    assert!(
        models::models_dir().starts_with(root),
        "models_dir still re-derives its own root: {}",
        models::models_dir().display()
    );

    // And the resolution is frozen: a second read cannot move the root out from
    // under an open SQLite connection.
    assert_eq!(app_paths::paths().root, root.to_path_buf());

    // The documented NAME is part of the contract — it is what ARCHITECTURE.md
    // tells every agent to export. Asserted last so that breaking the override
    // fails on the behaviour above, not on this label.
    assert_eq!(DATA_DIR_ENV, "YAP_DATA_DIR");
}

/// `--smoke` refuses to start whenever the override is absent or points at the
/// real state root, and each refusal is named and exits non-zero. An unflagged
/// launch is never affected.
#[test]
fn smoke_mode_refuses_to_start_against_the_default_root() {
    let _guard = serial();
    let root = scratch_root();
    let argv = vec!["--smoke".to_string()];
    assert_eq!(argv[0], smoke::SMOKE_FLAG);

    assert_eq!(
        smoke::preflight(&argv, None),
        Err(smoke::Refusal::DataDirUnset)
    );
    assert_eq!(
        smoke::preflight(&argv, Some("")),
        Err(smoke::Refusal::DataDirUnset)
    );
    assert_eq!(
        smoke::preflight(&argv, Some("   ")),
        Err(smoke::Refusal::DataDirUnset)
    );

    let default = app_paths::default_root();
    for candidate in [default.clone(), default.join("models")] {
        match smoke::preflight(&argv, candidate.to_str()) {
            Err(smoke::Refusal::DataDirIsDefaultRoot(seen)) => {
                assert_eq!(seen, candidate);
            }
            other => panic!("{} was not refused: {other:?}", candidate.display()),
        }
    }

    for refusal in [
        smoke::Refusal::DataDirUnset,
        smoke::Refusal::DataDirIsDefaultRoot(default),
    ] {
        assert_ne!(refusal.exit_code(), 0, "a refused smoke launch must fail");
        assert!(refusal.message().contains(DATA_DIR_ENV));
        assert!(refusal.message().contains(refusal.name()));
        assert!(refusal.message().contains("Refusing to start"));
    }

    // A scratch root is accepted, and a launch with no flag is untouched.
    assert_eq!(smoke::preflight(&argv, root.to_str()), Ok(true));
    assert_eq!(smoke::preflight(&[], None), Ok(false));
    assert_eq!(
        smoke::preflight(&["--transcribe-file".to_string()], None),
        Ok(false)
    );
}

/// Once armed, a smoke run installs no global hotkey and synthesizes no
/// keystroke, for the rest of the process.
///
/// This asserts the PREDICATES both refusals hang off. The call sites that read
/// them are `ptt_macos::start` (the CGEvent tap), the `run_on_main_thread`
/// closure in `lib.rs` that registers ⌃⌘V / ⌃⌘M / ⌃⌘Z / ⌘⇧V, and
/// `paste::copy_and_maybe_paste` + `paste::delete_selection` — none of which can
/// be constructed without a live `AppHandle` and a granted Accessibility prompt,
/// so their wiring is proved by grep in the PR body, not here.
///
/// Arming is deliberately irreversible, so this test runs last in effect: the
/// two tests above never read the smoke flag.
#[test]
fn smoke_mode_registers_no_global_hotkey_and_never_pastes() {
    let _guard = serial();

    assert!(
        smoke::global_hotkeys_allowed(),
        "a normal launch still registers its hotkeys"
    );
    assert!(smoke::paste_allowed(), "a normal launch still pastes");

    smoke::arm();

    assert!(smoke::is_armed());
    assert!(
        !smoke::global_hotkeys_allowed(),
        "a smoke run must register NO global hotkey"
    );
    assert!(
        !smoke::paste_allowed(),
        "a smoke run must synthesize NO paste"
    );

    // Each refusal is named, so a harness can match on it in the log.
    for message in [
        smoke::HOTKEY_REFUSAL,
        smoke::PASTE_REFUSAL,
        smoke::DELETE_REFUSAL,
    ] {
        assert!(
            message.contains(smoke::SMOKE_FLAG),
            "unnamed refusal: {message}"
        );
        assert!(message.contains("refusing"), "unnamed refusal: {message}");
    }
}
