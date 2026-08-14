//! YV91 acceptance — **error matrix row #4: the disk fills during a long
//! recording.**
//!
//! The row's required behaviour, verbatim: *"Pre-flight at start: require
//! `free ≥ duration_estimate × 64 KB/s + 2 GB`; refuse with a clear number if
//! not. Watchdog every 60 s: below 1 GB ⇒ stop cleanly, finalize both journals,
//! mark `state='partial'`. Never write until the FS errors."*
//!
//! Every clause is a test here. The watchdog's interval is injected (60 s in
//! production, milliseconds here) and so is the free-space probe, because a row
//! about a full disk that can only be tested by filling a disk is a row that
//! never gets tested.
//!
//! Finding #39 is folded in: the plan's formula needs a `duration_estimate`
//! that a manually-started meeting does not have, so with `None` the preflight
//! budgets against the 3 h hard cap.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use wilson_voice_lib::meeting::{
    fan_out_block, preflight, required_free_bytes, session_turnstile, watchdog_tick,
    BatterySnapshot, CaptureEnv, ExternalStream, MeetingSession, MeetingState, PreflightInputs,
    SessionConfig, StopReason, ThermalState, WatchdogAction, WatchdogInputs, DISK_BYTES_PER_SECOND,
    DISK_FLOOR_BYTES, DISK_HEADROOM_BYTES, MEETING_HARD_CAP, TARGET_RATE,
};
use wilson_voice_lib::rtring::CaptureAnchor;

/// A machine whose free space we control, sample by sample.
struct FakeDisk {
    free: AtomicU64,
}

impl CaptureEnv for FakeDisk {
    fn free_bytes(&self) -> u64 {
        self.free.load(Ordering::Relaxed)
    }

    fn battery(&self) -> BatterySnapshot {
        BatterySnapshot::default()
    }

    fn thermal(&self) -> ThermalState {
        ThermalState::Nominal
    }
}

fn turn() -> std::sync::MutexGuard<'static, ()> {
    // The library's turnstile, not a private one: `MeetingSession::start`
    // refuses a second concurrent session, so every suite that starts a
    // meeting has to queue behind the same lock the others take.
    session_turnstile()
}

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yap-yv91-row4-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

#[test]
fn the_preflight_formula_is_the_one_the_row_specifies() {
    // free ≥ duration × 64 KB/s + 2 GB.
    let two_hours = Duration::from_secs(2 * 60 * 60);
    assert_eq!(
        required_free_bytes(two_hours),
        two_hours.as_secs() * DISK_BYTES_PER_SECOND + DISK_HEADROOM_BYTES
    );
    // Finding #39: with no estimate, the budget is the hard cap.
    assert_eq!(
        required_free_bytes(MEETING_HARD_CAP),
        MEETING_HARD_CAP.as_secs() * DISK_BYTES_PER_SECOND + DISK_HEADROOM_BYTES
    );
}

#[test]
fn a_start_that_would_not_fit_is_refused_before_a_single_byte_is_written() {
    // One meeting at a time, process-wide: `MeetingSession::start` refuses a
    // second concurrent session, so the suites take the same turnstile.
    let _turn = session_turnstile();
    let dir = tmpdir("refused");
    let free = 900_000_000u64; // under the 2 GB headroom on its own
    let started = MeetingSession::start(SessionConfig {
        // The blocks come from this test, not from a microphone — so the
        // session holds no stream of its own (YV91's `ExternalStream`).
        stream: Arc::new(ExternalStream),
        env: Arc::new(FakeDisk {
            free: AtomicU64::new(free),
        }),
        watchdog_interval: Duration::from_millis(10),
        ..SessionConfig::new(&dir, 16_000, 1)
    });
    let message = match started {
        Ok(_) => panic!("a 900 MB volume must not be allowed to start a 3h recording"),
        Err(err) => err.to_string(),
    };
    assert!(message.contains(&free.to_string()), "{message}");

    // "Never write until the FS errors" starts here: a refused start leaves
    // nothing behind at all.
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .map(|e| e.filter_map(|e| e.ok()).collect())
        .unwrap_or_default();
    assert!(
        leftovers.is_empty(),
        "a refused start wrote {} file(s)",
        leftovers.len()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_start_that_fits_is_allowed() {
    let ok = preflight(&PreflightInputs {
        free_bytes: required_free_bytes(MEETING_HARD_CAP),
        duration_estimate: None,
        battery: BatterySnapshot::default(),
        thermal: ThermalState::Nominal,
    })
    .expect("exactly enough is enough");
    assert_eq!(ok.budgeted, MEETING_HARD_CAP);
    assert!(ok.warnings.is_empty());
}

#[test]
fn the_watchdog_rule_is_one_gigabyte_and_it_is_pure() {
    assert_eq!(
        watchdog_tick(&WatchdogInputs {
            elapsed: Duration::from_secs(120),
            free_bytes: DISK_FLOOR_BYTES,
            device_failed: false,
            since_last_block: Duration::from_millis(20),
            thermal: ThermalState::Nominal,
            cap_warned: false,
            tap: None,
        }),
        WatchdogAction::Continue,
        "exactly at the floor is still fine"
    );
    assert_eq!(
        watchdog_tick(&WatchdogInputs {
            elapsed: Duration::from_secs(120),
            free_bytes: DISK_FLOOR_BYTES - 1,
            device_failed: false,
            since_last_block: Duration::from_millis(20),
            thermal: ThermalState::Nominal,
            cap_warned: false,
            tap: None,
        }),
        WatchdogAction::Stop(StopReason::LowDisk {
            free_bytes: DISK_FLOOR_BYTES - 1
        })
    );
}

#[test]
fn the_watchdog_stops_a_running_meeting_cleanly_and_marks_it_partial() {
    let _turn = turn();
    let dir = tmpdir("watchdog");
    // Starts with plenty of room…
    let disk = Arc::new(FakeDisk {
        free: AtomicU64::new(50 * 1024 * 1024 * 1024),
    });
    let session = MeetingSession::start(SessionConfig {
        // The blocks come from this test, not from a microphone — so the
        // session holds no stream of its own (YV91's `ExternalStream`).
        stream: Arc::new(ExternalStream),
        env: disk.clone(),
        watchdog_interval: Duration::from_millis(10),
        ..SessionConfig::new(&dir, TARGET_RATE, 1)
    })
    .expect("the start is allowed");

    // …records a second of audio…
    for i in 0..10u64 {
        let block = vec![0.4f32; TARGET_RATE as usize / 10];
        let anchors = [CaptureAnchor {
            host_ns: i * 100_000_000,
            sample_index: i * (TARGET_RATE as u64 / 10),
            frames: TARGET_RATE / 10,
            sample_rate: TARGET_RATE,
            lost_frames: 0,
        }];
        fan_out_block(&block, &anchors, None, None);
    }

    // …and then the disk fills under it.
    disk.free.store(DISK_FLOOR_BYTES - 1, Ordering::Relaxed);
    let reason = session
        .wait_for_watchdog_stop(Duration::from_secs(5))
        .expect("the watchdog noticed within its interval");
    assert_eq!(
        reason,
        StopReason::LowDisk {
            free_bytes: DISK_FLOOR_BYTES - 1
        }
    );
    assert!(
        reason
            .to_string()
            .contains(&(DISK_FLOOR_BYTES - 1).to_string()),
        "the stop reason carries the number too: {reason}"
    );

    // "Stop cleanly, finalize the journal, mark state='partial'."
    let finalized = session.stop().expect("the journal finalized");
    assert_eq!(
        finalized.state,
        MeetingState::Partial,
        "a watchdog stop is partial — the audio is real, the meeting is not over"
    );
    assert!(
        finalized.tracks[0].exists(),
        "the audio it did capture is playable"
    );
    assert!(finalized.seconds > 0.9, "{}s recovered", finalized.seconds);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_meeting_the_watchdog_never_stops_is_complete_not_partial() {
    let _turn = turn();
    let dir = tmpdir("healthy");
    let session = MeetingSession::start(SessionConfig {
        // The blocks come from this test, not from a microphone — so the
        // session holds no stream of its own (YV91's `ExternalStream`).
        stream: Arc::new(ExternalStream),
        env: Arc::new(FakeDisk {
            free: AtomicU64::new(50 * 1024 * 1024 * 1024),
        }),
        watchdog_interval: Duration::from_millis(10),
        ..SessionConfig::new(&dir, TARGET_RATE, 1)
    })
    .expect("the start is allowed");
    let block = vec![0.4f32; TARGET_RATE as usize / 10];
    for _ in 0..10 {
        fan_out_block(&block, &[], None, None);
    }
    std::thread::sleep(Duration::from_millis(60)); // several watchdog ticks
    assert!(session.watchdog_stop().is_none());
    let finalized = session.stop().expect("finalized");
    assert_eq!(finalized.state, MeetingState::Complete);
    let _ = std::fs::remove_dir_all(&dir);
}
