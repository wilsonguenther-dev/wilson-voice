//! YV100 acceptance — **a panic inside the IOProc block does not abort the
//! process, and it sets the flag the watchdog reads** (plan finding OS-6,
//! correction #2).
//!
//! The exact signature in `objc2-core-audio` 0.3.2 is
//! `pub unsafe extern "C-unwind" fn AudioHardwareCreateProcessTap(...)`, and the
//! block registered with `AudioDeviceCreateIOProcIDWithBlock` runs behind the
//! same kind of boundary. `C-unwind` means a Rust panic reaching it is not an
//! error value — it unwinds into CoreAudio's real-time thread, which is
//! undefined behaviour and, in practice, a HAL-level kill of the whole app.
//! Losing the meeting is bad; losing the app mid-call because a slice index was
//! off by one is worse.
//!
//! OS-6's own words: *"OS-7's ring-buffer body is exactly where an off-by-one
//! becomes a panic."* So the guard wraps the ENTIRE block body — including the
//! `AudioBufferList` decode, which is the part `rt_capture_callback` does not
//! cover — and reports through the one atomic flag YV104's watchdog already
//! reads for the mic path.

use wilson_voice_lib::meeting::{rt_capture_callback, RtCapture};
use wilson_voice_lib::syscapture::tap_ioproc_guarded;

/// Silence the panic printout for the duration of a deliberate panic, then put
/// the previous hook back. Without this the suite's output is a wall of
/// backtraces for tests that are passing.
fn quietly<R>(body: impl FnOnce() -> R) -> R {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let out = body();
    std::panic::set_hook(previous);
    out
}

#[test]
fn a_panic_in_the_block_body_is_caught_and_flagged_rather_than_unwound_into_coreaudio() {
    let capture = RtCapture::new(48_000, 1);
    assert!(!capture.callback_panicked(), "clean to start with");

    let returned = quietly(|| {
        tap_ioproc_guarded(&capture, || {
            // Stand-in for the buffer-list decode: the one step of the block
            // that `rt_capture_callback` does not already wrap.
            panic!("mDataByteSize did not divide by the channel count");
        })
    });

    assert!(
        returned.is_none(),
        "the guard reports the failure to its caller"
    );
    assert!(
        capture.callback_panicked(),
        "the atomic flag YV104's watchdog reads must be set — a swallowed panic \
         with no flag is a tap that is silently dead for the rest of the meeting"
    );
    // And the process is still here to assert it.
}

#[test]
fn a_panic_inside_the_ring_push_sets_the_same_one_flag() {
    // Two guards nest here in production: this one, and `rt_capture_callback`'s
    // own. The claim is that they raise the SAME flag, so the watchdog has one
    // question to ask rather than two.
    let capture = RtCapture::new(48_000, 1);
    let block = vec![0.5f32; 480];
    quietly(|| {
        tap_ioproc_guarded(&capture, || {
            rt_capture_callback(&capture, &block, |_| panic!("conversion blew up"), 42);
        });
    });
    assert!(capture.callback_panicked());
}

#[test]
fn a_clean_callback_leaves_the_flag_alone_and_returns_its_value() {
    let capture = RtCapture::new(48_000, 1);
    let block = vec![0.5f32; 480];
    let out = tap_ioproc_guarded(&capture, || {
        rt_capture_callback(&capture, &block, |s| s, 7);
        "ran"
    });
    assert_eq!(out, Some("ran"));
    assert!(
        !capture.callback_panicked(),
        "the flag is a report of a real failure, not a counter of invocations"
    );
    let mut drained = Vec::new();
    capture.samples.drain_into(&mut drained);
    assert_eq!(drained.len(), 480, "and the frames still landed");
}

#[test]
fn the_flag_survives_the_callbacks_that_come_after_the_panic() {
    // A tap that panicked once and then recovered is still a tap that dropped
    // audio, and YV104 decides what to do about it — not the block. So the flag
    // latches rather than tracking the most recent callback.
    let capture = RtCapture::new(48_000, 1);
    let block = vec![0.25f32; 480];
    quietly(|| {
        tap_ioproc_guarded(&capture, || panic!("once"));
    });
    for i in 0..50 {
        tap_ioproc_guarded(&capture, || {
            rt_capture_callback(&capture, &block, |s| s, i);
        });
    }
    assert!(capture.callback_panicked(), "the flag latches");
}

#[test]
fn the_guard_does_not_swallow_a_panic_raised_outside_the_block() {
    // The guard's blast radius is one callback. A test-harness bug, or a panic
    // on the consumer thread, must still fail loudly — a `catch_unwind` that
    // creeps outward is how a real defect becomes a shrug.
    let result = quietly(|| {
        std::panic::catch_unwind(|| {
            let capture = RtCapture::new(48_000, 1);
            let block = vec![0.1f32; 480];
            tap_ioproc_guarded(&capture, || {
                rt_capture_callback(&capture, &block, |s| s, 0);
            });
            panic!("this one is NOT inside the guard");
        })
    });
    assert!(result.is_err());
}
