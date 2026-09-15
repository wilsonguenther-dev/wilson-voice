//! PERM-E — the watcher emits `permission_changed` ONCE PER TRANSITION and
//! never on a repeat read of an unchanged report.
//!
//! Nothing here touches TCC, IOKit or a window server. The whole watcher was
//! built around two injected closures (`read` / `emit`) and one pure gate
//! precisely so this contract is provable on a machine with no grants at all —
//! the machine on which a "it works on my Mac" permission surface is wrong.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use wilson_voice_lib::permissions::{
    self, AudioCaptureObservation, GrantSnapshot, GrantState, TransitionGate, WatchReason,
    PERMISSION_CHANGED_EVENT, WATCH_INTERVAL, WATCH_MIN_INTERVAL,
};

fn snap(
    microphone: GrantState,
    accessibility: GrantState,
    input_monitoring: GrantState,
    audio_capture: GrantState,
) -> GrantSnapshot {
    GrantSnapshot {
        microphone,
        accessibility,
        input_monitoring,
        audio_capture,
    }
}

fn all_ok() -> GrantSnapshot {
    snap(
        GrantState::Authorized,
        GrantState::Authorized,
        GrantState::Authorized,
        GrantState::Authorized,
    )
}

#[test]
fn the_event_name_is_the_one_the_frontend_listens_to() {
    assert_eq!(PERMISSION_CHANGED_EVENT, "permission_changed");
}

#[test]
fn a_repeat_read_of_an_unchanged_report_emits_nothing_at_all() {
    let mut gate = TransitionGate::new();
    let s = all_ok();
    // The first read is the baseline, not a change.
    assert!(gate.observe(s).is_none(), "baseline must not emit");
    for i in 0..50 {
        assert!(
            gate.observe(s).is_none(),
            "repeat read #{i} emitted permission_changed on an unchanged report"
        );
    }
}

#[test]
fn every_transition_emits_exactly_once() {
    let mut gate = TransitionGate::new();
    let ok = all_ok();
    let revoked = snap(
        GrantState::Authorized,
        GrantState::Denied,
        GrantState::Authorized,
        GrantState::Authorized,
    );

    gate.observe(ok);
    let mut emits = 0usize;
    // grant → revoked → revoked → revoked → grant → grant: TWO transitions,
    // six reads.
    for s in [revoked, revoked, revoked, ok, ok] {
        if gate.observe(s).is_some() {
            emits += 1;
        }
    }
    assert_eq!(emits, 2, "one emit per transition, and no more");
}

#[test]
fn a_transition_in_any_one_of_the_four_grants_is_a_transition() {
    let ok = all_ok();
    for changed in [
        snap(
            GrantState::Denied,
            GrantState::Authorized,
            GrantState::Authorized,
            GrantState::Authorized,
        ),
        snap(
            GrantState::Authorized,
            GrantState::Denied,
            GrantState::Authorized,
            GrantState::Authorized,
        ),
        snap(
            GrantState::Authorized,
            GrantState::Authorized,
            GrantState::Denied,
            GrantState::Authorized,
        ),
        snap(
            GrantState::Authorized,
            GrantState::Authorized,
            GrantState::Authorized,
            GrantState::Denied,
        ),
    ] {
        let mut gate = TransitionGate::new();
        gate.observe(ok);
        assert!(
            gate.observe(changed).is_some(),
            "a changed grant must emit: {changed:?}"
        );
        assert!(
            gate.observe(changed).is_none(),
            "and then go quiet again: {changed:?}"
        );
    }
}

#[test]
fn the_watcher_parks_entirely_when_nothing_is_denied() {
    assert_eq!(
        permissions::next_wait(all_ok()),
        None,
        "all four Authorized must park — YV81 deleted Yap's busy timers on purpose"
    );
    // Unknown is not a denial, so it does not keep a timer alive either. It
    // CANNOT be: system audio has no readable status, so a poll for it could
    // never learn anything.
    assert_eq!(
        permissions::next_wait(snap(
            GrantState::Authorized,
            GrantState::Authorized,
            GrantState::Unknown,
            GrantState::Unknown,
        )),
        None,
        "an unknown grant must never start a timer"
    );
}

#[test]
fn no_interval_in_the_watcher_is_under_thirty_seconds() {
    assert_eq!(WATCH_MIN_INTERVAL, Duration::from_secs(30));
    let denied = snap(
        GrantState::Denied,
        GrantState::Authorized,
        GrantState::Authorized,
        GrantState::Authorized,
    );
    let wait = permissions::next_wait(denied).expect("a denial is watchable");
    assert_eq!(wait, WATCH_INTERVAL);
    assert!(
        wait >= WATCH_MIN_INTERVAL,
        "{wait:?} is under the 30s floor this module promises"
    );
}

#[test]
fn the_live_watcher_thread_emits_on_a_transition_and_stays_quiet_otherwise() {
    // The real spawned thread, driven by a poke rather than by a clock — which
    // is also the proof that a poke re-reads a PARKED watcher.
    let reading = Arc::new(Mutex::new(all_ok()));
    let reads = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = mpsc::channel();

    let r = Arc::clone(&reading);
    let counter = Arc::clone(&reads);
    let watch = permissions::spawn_watch(
        move || {
            counter.fetch_add(1, Ordering::SeqCst);
            *r.lock().unwrap()
        },
        move |snapshot| {
            let _ = tx.send(snapshot);
        },
    );

    // Baseline read, then two pokes with NOTHING changed: no emit may arrive.
    watch.poke(WatchReason::Focus);
    watch.poke(WatchReason::Wake);
    assert!(
        rx.recv_timeout(Duration::from_millis(400)).is_err(),
        "an unchanged report emitted permission_changed"
    );

    // Now revoke Accessibility and poke as a refusal would.
    *reading.lock().unwrap() = snap(
        GrantState::Authorized,
        GrantState::Denied,
        GrantState::Authorized,
        GrantState::Authorized,
    );
    watch.poke(WatchReason::Refusal);
    let got = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("a revoked grant must emit permission_changed");
    assert_eq!(got.accessibility, GrantState::Denied);

    // And it does not repeat itself for the same state.
    watch.poke(WatchReason::Focus);
    assert!(
        rx.recv_timeout(Duration::from_millis(400)).is_err(),
        "the same denial emitted twice"
    );
    assert!(
        reads.load(Ordering::SeqCst) >= 2,
        "the watcher never re-read after a poke"
    );
}

#[test]
fn an_observed_system_audio_outcome_is_a_transition_the_watcher_can_see() {
    // The ONLY way this grant ever moves off Unknown. Ordering matters for the
    // process-global store, so both halves live in one test.
    permissions::note_audio_capture_observation(AudioCaptureObservation::NeverRan);
    assert_eq!(permissions::audio_capture_state(), GrantState::Unknown);
    permissions::note_audio_capture_observation(AudioCaptureObservation::Delivered);
    assert_eq!(permissions::audio_capture_state(), GrantState::Authorized);
    permissions::note_audio_capture_observation(AudioCaptureObservation::SilentPastGrace);
    assert_eq!(permissions::audio_capture_state(), GrantState::Denied);
    permissions::note_audio_capture_observation(AudioCaptureObservation::NeverRan);
}
