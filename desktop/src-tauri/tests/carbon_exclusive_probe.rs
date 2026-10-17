//! yap-p0-hotkeys-no-permission, fix round 2 (independent reviewer BLOCKING 1):
//! global-hotkey 0.8.0 registers with `RegisterEventHotKey(.., options 0, ..)`,
//! which returns noErr for a chord another process already holds, so a plain
//! register `Ok` is no proof Yap owns the chord. The dictate registrar now runs
//! `carbon_probe::probe_chord` (kEventHotKeyExclusive) first. These tests use a
//! REAL second process holding a chord exactly the way global-hotkey does.
#![cfg(target_os = "macos")]

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};
use wilson_voice_lib::carbon_probe::{self, EVENT_HOT_KEY_EXISTS_ERR};

const LIB: &str = include_str!("../src/lib.rs");

/// The Carbon Event Manager is not thread-safe (two tests registering at once
/// trap with SIGTRAP); the app only calls it on the main thread. Tests that
/// touch Carbon take this lock.
static CARBON: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// An obscure chord nothing ships with: ⌃⌥⇧⌘F19.
fn test_chord() -> Shortcut {
    Shortcut::new(
        Some(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT | Modifiers::SUPER),
        Code::F19,
    )
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EventHotKeyID {
    signature: u32,
    id: u32,
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn GetApplicationEventTarget() -> *mut std::ffi::c_void;
    fn RegisterEventHotKey(
        code: u32,
        mods: u32,
        id: EventHotKeyID,
        target: *mut std::ffi::c_void,
        options: u32,
        out: *mut *mut std::ffi::c_void,
    ) -> i32;
}

/// The "other app": when spawned with YAP_PROBE_HOLD=1 it registers the test
/// chord NON-exclusively (options 0, global-hotkey's 'htrs' signature — the
/// same call Yap's own plugin makes) and holds it until killed. Run normally
/// it does nothing.
#[test]
fn holder_child_process() {
    if std::env::var("YAP_PROBE_HOLD").as_deref() != Ok("1") {
        return;
    }
    let c = test_chord();
    let mut r = std::ptr::null_mut();
    // SAFETY: plain Carbon call with a valid out-pointer.
    let st = unsafe {
        RegisterEventHotKey(
            carbon_probe::carbon_keycode(c.key).unwrap(),
            carbon_probe::carbon_mods(c.mods),
            EventHotKeyID {
                signature: u32::from_be_bytes(*b"htrs"),
                id: 7,
            },
            GetApplicationEventTarget(),
            std::env::var("YAP_PROBE_HOLD_OPTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            &mut r,
        )
    };
    println!("HOLDER-READY status={st}");
    std::thread::sleep(std::time::Duration::from_secs(30));
}

/// Measured on this Mac (see `measure_holder_matrix`): while another process
/// holds the chord EXCLUSIVELY, the plugin's non-exclusive register still says
/// noErr — the fail-open the reviewer found — and only the exclusive probe
/// says -9878. Same for macOS's own symbolic hot keys (⌘Space → -9878).
#[test]
fn chord_held_by_another_process_is_refused_by_the_probe() {
    let _g = CARBON.lock().unwrap_or_else(|p| p.into_inner());
    let c = test_chord();
    let kc = carbon_probe::carbon_keycode(c.key).unwrap();
    let mods = carbon_probe::carbon_mods(c.mods);
    assert_eq!(
        carbon_probe::probe_chord(&c),
        Ok(()),
        "test chord must start free"
    );

    let (mut child, holder_status) = spawn_holder(carbon_probe::K_EVENT_HOT_KEY_EXCLUSIVE);
    if holder_status != "0" {
        let _ = child.kill();
        let _ = child.wait();
        // No window-server session: Carbon cannot register at all here, so
        // there is nothing to measure (an environment, not a pass).
        println!("carbon_probe: SKIP holder could not register (OSStatus {holder_status})");
        return;
    }
    // The premise: the NON-exclusive register the plugin makes says noErr.
    let shared = carbon_probe::register_then_release(kc, mods, 0);
    // The fix: the exclusive probe refuses it.
    let probed = carbon_probe::probe_chord(&c);
    let _ = child.kill();
    let _ = child.wait();
    let after = carbon_probe::probe_chord(&c);
    println!(
        "carbon_probe: holder=exclusive holder_register={holder_status} shared_register_while_held={shared} exclusive_probe_while_held={probed:?} exclusive_probe_after_holder_exit={after:?}"
    );
    assert_eq!(
        shared, 0,
        "non-exclusive register accepts a held chord (the fail-open premise)"
    );
    let e = probed.expect_err("a chord another process holds exclusively must be refused");
    assert!(e.contains(&EVENT_HOT_KEY_EXISTS_ERR.to_string()), "{e}");
    assert_eq!(
        after,
        Ok(()),
        "the chord is free again once the holder exits"
    );
}

/// The residual, pinned so nobody overclaims it: a process that holds the chord
/// NON-exclusively (options 0, as global-hotkey / MASShortcut / soffes HotKey
/// do) is invisible to every Carbon register mode — the exclusive probe says
/// free. There is no public API that lists another app's hot keys.
#[test]
fn non_exclusive_co_holder_is_not_detectable() {
    let _g = CARBON.lock().unwrap_or_else(|p| p.into_inner());
    let c = test_chord();
    let (mut child, holder_status) = spawn_holder(0);
    let probed = carbon_probe::probe_chord(&c);
    let _ = child.kill();
    let _ = child.wait();
    println!("carbon_probe: holder=shared holder_register={holder_status} exclusive_probe_while_held={probed:?}");
    if holder_status == "0" {
        assert_eq!(probed, Ok(()));
    }
}

#[test]
fn probe_result_maps_exists_err_and_others() {
    assert_eq!(carbon_probe::probe_result(0), Ok(()));
    assert!(carbon_probe::probe_result(-9878)
        .unwrap_err()
        .contains("another app or macOS"));
    assert!(carbon_probe::probe_result(-50)
        .unwrap_err()
        .contains("OSStatus -50"));
}

#[test]
fn keycode_and_mask_match_global_hotkey() {
    // ⌃⌥⌘Space: kVK_Space 0x31; controlKey|optionKey|cmdKey.
    assert_eq!(carbon_probe::carbon_keycode(Code::Space), Some(0x31));
    assert_eq!(
        carbon_probe::carbon_mods(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER),
        4096 | 2048 | 256
    );
    // Media keys are not Carbon in global-hotkey: nothing to probe.
    assert_eq!(carbon_probe::carbon_keycode(Code::MediaPlayPause), None);
}

#[test]
fn dictate_registrar_probes_before_the_plugin_register() {
    let body = LIB
        .split("impl hotkeys::CarbonRegistrar for TauriCarbonRegistrar")
        .nth(1)
        .unwrap();
    let probe = body
        .find("carbon_probe::probe_chord(&chord)")
        .expect("registrar probes");
    let plugin = body.find("global_shortcut().register(").unwrap();
    assert!(
        probe < plugin,
        "the exclusive probe must run before the plugin register"
    );
}

/// Machine-dependent evidence (run with --ignored): Spotlight's ⌘Space.
#[test]
#[ignore]
fn spotlight_cmd_space_is_refused_on_this_mac() {
    let _g = CARBON.lock().unwrap_or_else(|p| p.into_inner());
    let c = Shortcut::new(Some(Modifiers::SUPER), Code::Space);
    let r = carbon_probe::probe_chord(&c);
    let d = carbon_probe::probe_chord(&wilson_voice_lib::shortcuts::DICTATE_TOGGLE.shortcut());
    println!("carbon_probe: cmd+Space={r:?} default_dictate={d:?}");
    assert!(r.is_err());
}

/// Spawn the holder with `opts` and wait until it holds the test chord.
fn spawn_holder(opts: u32) -> (std::process::Child, String) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "holder_child_process",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("YAP_PROBE_HOLD", "1")
        .env("YAP_PROBE_HOLD_OPTS", opts.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut ready = None;
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let line = line.unwrap();
        if let Some(i) = line.find("HOLDER-READY status=") {
            ready = Some(line[i + "HOLDER-READY status=".len()..].trim().to_string());
            break;
        }
    }
    (child, ready.expect("holder process reported"))
}

/// Evidence matrix (run with --ignored): what each register says while another
/// process holds the chord non-exclusively (0) or exclusively (1).
#[test]
#[ignore]
fn measure_holder_matrix() {
    let _g = CARBON.lock().unwrap_or_else(|p| p.into_inner());
    let c = test_chord();
    let kc = carbon_probe::carbon_keycode(c.key).unwrap();
    let mods = carbon_probe::carbon_mods(c.mods);
    for opts in [0u32, 1] {
        let (mut child, st) = spawn_holder(opts);
        let shared = carbon_probe::register_then_release(kc, mods, 0);
        let excl = carbon_probe::register_then_release(kc, mods, 1);
        let _ = child.kill();
        let _ = child.wait();
        println!("carbon_matrix: holder_options={opts} holder_status={st} shared_register={shared} exclusive_register={excl}");
    }
    let space = Shortcut::new(Some(Modifiers::SUPER), Code::Space);
    let (k, m) = (
        carbon_probe::carbon_keycode(space.key).unwrap(),
        carbon_probe::carbon_mods(space.mods),
    );
    println!(
        "carbon_matrix: system cmd+Space shared_register={} exclusive_register={}",
        carbon_probe::register_then_release(k, m, 0),
        carbon_probe::register_then_release(k, m, 1)
    );
}
