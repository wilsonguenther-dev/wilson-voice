//! YV100 acceptance — **the tap's IOProc body allocates nothing and blocks on
//! nothing** (plan finding OS-7, restated for 22-B).
//!
//! Same scoped-allocator-hook technique as `tests/meeting_capture_rt_safety.rs`,
//! pointed at the tap's block body instead of the mic callback. It is the same
//! harness on purpose: the claim is that the tap runs the 22-A ring **verbatim**
//! rather than growing a second copy, and the cheapest way to keep that honest
//! is to hold both to the identical bar with the identical instrument.
//!
//! The constraint is STRICTER here than on the mic thread, which is why it gets
//! its own binary rather than a new case in the old one. The aggregate device's
//! main sub-device is the user's real output device, so a missed IOProc deadline
//! is a glitch in the call the user is listening to — not merely a defect in the
//! recording. An allocation on that thread can block on the allocator's lock
//! behind an arbitrary other thread.
//!
//! What is measured is `syscapture::tap_ioproc_guarded` wrapped around
//! `TapClock::stamp` + `meeting::rt_capture_callback`, which is exactly what the
//! registered block runs, in that order: the guard exists because the
//! buffer-list decode inside it can panic across an `extern "C-unwind"`
//! boundary, and a guard that allocated would be its own defect.
//!
//! It also carries the **domain** half of the anchor claim, because that is the
//! same code path. CoreAudio hands the block `mHostTime` — absolute mach ticks
//! since boot — and `CaptureAnchor::host_ns` is nanoseconds rebased to each
//! stream's first callback (`record.rs`, `build_capture_stream`). The tap is
//! only "the same anchor the mic path writes" if it converts, so these tests
//! feed raw ticks through the real `TapClock` and assert the first anchor is
//! `0`. Passing a pre-cooked `host_ns` literal in, as the first cut did, cannot
//! fail that claim and therefore never checked it.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use wilson_voice_lib::meeting::{rt_capture_callback, RtCapture, RING_FRAMES};
use wilson_voice_lib::syscapture::{tap_ioproc_guarded, TapClock};

// ── The scoped allocator hook (same shape as YV91's) ───────────────────────

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

struct CountingAllocator;

// SAFETY: every method forwards to the system allocator unchanged; the counter
// is a side effect and never affects the pointer returned.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        System.realloc(ptr, layout, new_size)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note();
        System.alloc_zeroed(layout)
    }
}

fn note() {
    let armed = ARMED.try_with(|a| a.get()).unwrap_or(false);
    if armed {
        let _ = COUNT.try_with(|c| c.set(c.get() + 1));
    }
}

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator;

fn allocations_during(body: impl FnOnce()) -> usize {
    let before = COUNT.with(|c| c.get());
    ARMED.with(|a| a.set(true));
    body();
    ARMED.with(|a| a.set(false));
    COUNT.with(|c| c.get()) - before
}

#[test]
fn the_allocator_hook_actually_sees_allocations() {
    // A test that can only pass is not a test.
    let seen = allocations_during(|| {
        let mut v: Vec<u64> = Vec::new();
        for i in 0..1024 {
            v.push(i);
        }
        std::hint::black_box(&v);
    });
    assert!(seen > 0, "the counting allocator is not armed");
}

// ── The acceptance criterion ───────────────────────────────────────────────

const CALLBACKS: usize = 2_000;

/// One IOProc invocation, exactly as the registered block runs it: the guard,
/// the `TapClock` conversion of the raw `mHostTime` CoreAudio hands over, and
/// the ring push. The real block additionally decodes an `AudioBufferList` into
/// `frames` INSIDE this guard; that decode is two pointer reads and a
/// `slice::from_raw_parts`, which cannot allocate.
///
/// `host_ticks`, not `host_ns`: the argument is in the units CoreAudio actually
/// delivers, so the conversion under measurement is the shipped one rather than
/// a number the test picked. That is the difference between measuring the
/// callback and measuring a re-composition of it.
fn tap_callback(capture: &RtCapture, clock: &TapClock, frames: &[f32], host_ticks: u64) {
    tap_ioproc_guarded(capture, || {
        let host_ns = clock.stamp(host_ticks);
        rt_capture_callback(capture, frames, |s| s, host_ns);
    });
}

#[test]
fn the_tap_ioproc_body_allocates_nothing() {
    // A mono global tap at 48 kHz handing out 512-frame blocks — the shape
    // CoreAudio actually delivers for this configuration.
    let capture = RtCapture::new(48_000, 1);
    let clock = TapClock::new();
    let block = vec![0.25f32; 512];
    let mut drained: Vec<f32> = Vec::with_capacity(RING_FRAMES);
    let mut anchors = Vec::with_capacity(1024);

    // Warm everything outside the measurement. This also takes the ONE path
    // through `TapClock` that could ever allocate or syscall — the first
    // callback, which claims the epoch — so the loop below is not measuring a
    // cheaper case than production: production pays that first callback too,
    // and it must be free as well. See the dedicated test below, which measures
    // the first callback on its own.
    tap_callback(&capture, &clock, &block, 4_300_000_000_000);
    capture.samples.drain_into(&mut drained);
    capture.anchors.drain_into(&mut anchors);
    drained.clear();
    anchors.clear();

    let mut total = 0usize;
    for i in 0..CALLBACKS {
        total += allocations_during(|| {
            tap_callback(
                &capture,
                &clock,
                &block,
                4_300_000_000_000 + (i as u64 + 1) * 256_000,
            );
        });
        // Draining is the consumer's job and runs disarmed, exactly as it runs
        // off the IO thread in production.
        capture.samples.drain_into(&mut drained);
        capture.anchors.drain_into(&mut anchors);
        drained.clear();
        anchors.clear();
    }

    assert_eq!(
        total, 0,
        "the tap IOProc allocated {total} time(s) over {CALLBACKS} invocations"
    );
    assert!(!capture.callback_panicked());
}

#[test]
fn a_stereo_tap_block_allocates_nothing_either() {
    // `initStereoGlobalTapButExcludeProcesses` is not what this item builds, but
    // the tap's format is READ from `kAudioTapPropertyFormat` rather than
    // assumed, so a 2-channel tap is reachable through configuration alone and
    // must not introduce a scratch buffer.
    let capture = RtCapture::new(48_000, 2);
    let clock = TapClock::new();
    let block = vec![0.1f32; 512 * 2];
    tap_callback(&capture, &clock, &block, 0);
    let mut sink = Vec::with_capacity(RING_FRAMES);
    capture.samples.drain_into(&mut sink);
    sink.clear();

    let allocations = allocations_during(|| {
        tap_callback(&capture, &clock, &block, 256_000);
    });
    assert_eq!(allocations, 0);
}

#[test]
fn the_very_first_callback_allocates_nothing_either() {
    // The first callback is the only one that takes `TapClock`'s
    // `compare_exchange` store path, and it is the callback that runs while the
    // user is already in a call. Measuring only callbacks 2..N would exempt the
    // one that is different. `TapClock::new()` is built OUTSIDE the measurement
    // because that is where it is built in production — in `create_ioproc`,
    // before the block is registered — and it is what pays the one-time
    // `mach_timebase_info` read so the IO thread never does.
    let capture = RtCapture::new(48_000, 1);
    let clock = TapClock::new();
    let block = vec![0.2f32; 512];

    let allocations = allocations_during(|| {
        tap_callback(&capture, &clock, &block, 4_300_000_000_000);
    });
    assert_eq!(
        allocations, 0,
        "the epoch-claiming first callback allocated"
    );
}

#[test]
fn a_full_ring_returns_immediately_instead_of_waiting_for_the_consumer() {
    // "Blocks on nothing", made falsifiable: nobody drains this ring and the
    // IOProc still returns. If a future change ever makes a full ring block,
    // this test hangs — which on the tap's thread is a hung output device.
    let capture = RtCapture::new(48_000, 1);
    let clock = TapClock::new();
    let block = vec![0.5f32; 4_096];
    let mut allocations = 0usize;
    let started = std::time::Instant::now();
    for i in 0..(RING_FRAMES / 4_096 + 4) {
        allocations += allocations_during(|| {
            tap_callback(&capture, &clock, &block, i as u64);
        });
    }
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "the IOProc waited on a full ring"
    );
    assert_eq!(
        allocations, 0,
        "and it did not allocate on the overrun path"
    );
    assert!(
        capture.overruns() > 0,
        "the frames that did not fit are counted, so YV104's watchdog can see them"
    );
    assert!(!capture.callback_panicked());
}

/// A plausible `mHostTime` for a Mac that has been awake a while — mach ticks
/// since boot, which is what CoreAudio hands the block. The number matters:
/// if the tap emitted this datum unconverted, every assertion below about the
/// anchor's domain would fail by roughly an uptime, which is exactly the bug
/// this test exists to catch.
const ABSOLUTE_BOOT_TICKS: u64 = 4_300_000_000_000;

#[test]
fn the_anchor_the_tap_writes_is_the_one_the_mic_path_writes() {
    // The cross-track alignment key (OS-2) only works if both producers stamp
    // the same datum in the SAME DOMAIN, and they do not start in the same
    // domain: CoreAudio gives the tap `mHostTime` (absolute mach ticks since
    // boot), while the mic path stamps
    // `captured_at.duration_since(first_callback)` — nanoseconds, rebased to
    // that stream's first callback, so its first anchor is 0
    // (`record.rs`, `build_capture_stream`; the convention is written down in
    // `meeting.rs`'s splice planner as "record 0's `host_ns` is an arbitrary
    // stream-relative instant").
    //
    // So this test drives the REAL conversion — `TapClock`, the one the
    // registered block calls — with the units CoreAudio really delivers, and
    // asserts the tap lands in the mic path's domain rather than asserting the
    // literal it was handed.
    let capture = RtCapture::new(44_100, 1);
    let clock = TapClock::new();
    let block = vec![0.3f32; 441];
    // 441 frames at 44.1 kHz is 10 ms. In ticks: 10 ms of mach time, which the
    // reference clock below converts for this machine's timebase rather than
    // this test assuming Apple silicon's 125/3 or Intel's 1/1.
    const TEN_MS_TICKS: u64 = 240_000;
    tap_callback(&capture, &clock, &block, ABSOLUTE_BOOT_TICKS);
    tap_callback(&capture, &clock, &block, ABSOLUTE_BOOT_TICKS + TEN_MS_TICKS);

    let mut anchors = Vec::new();
    capture.anchors.drain_into(&mut anchors);
    assert_eq!(anchors.len(), 2);

    // THE domain assertion: the first anchor of a stream is that stream's zero,
    // on this side exactly as on the mic side. Absolute mach time would put
    // this in the 10^12 range and YV106's two-track merge would place Track 1
    // an uptime away from Track 0 — silently, because every single-track delta
    // would still be right.
    assert_eq!(
        anchors[0].host_ns, 0,
        "the tap's first anchor must be its stream's zero, like the mic path's"
    );

    // ...and the second is a true delta in nanoseconds, not ticks. The expected
    // value comes from a second `TapClock` used as a converter (its own epoch is
    // claimed at 0), so the arithmetic under test is the shipped arithmetic on
    // whatever timebase this machine has.
    let reference = TapClock::new();
    assert_eq!(reference.stamp(0), 0);
    let expected_ns = reference.stamp(TEN_MS_TICKS);
    assert!(
        expected_ns > 0,
        "the reference conversion is degenerate; the assertion below would be vacuous"
    );
    assert_eq!(anchors[1].host_ns, expected_ns);
    if expected_ns != TEN_MS_TICKS {
        // Only meaningful where the timebase is not 1:1. On Apple silicon it is
        // 125/3, so ticks emitted as if they were nanoseconds would be wrong by
        // ~41x and this catches it; on an Intel Mac ticks ARE nanoseconds and
        // there is nothing here to distinguish, which is why the check is
        // guarded rather than asserted unconditionally and quietly true.
        assert_ne!(
            anchors[1].host_ns, TEN_MS_TICKS,
            "ticks were emitted as if they were nanoseconds"
        );
    }

    // The rest of the datum, unchanged: cumulative sample index, this
    // callback's frame count, the tap's own rate.
    assert_eq!(anchors[0].sample_index, 0);
    assert_eq!(anchors[0].frames, 441);
    assert_eq!(anchors[0].sample_rate, 44_100);
    assert_eq!(anchors[1].sample_index, 441, "cumulative, not per-callback");
    assert_eq!(anchors[1].lost_frames, 0);
}

#[test]
fn a_rebuilt_tap_rebases_to_zero_the_way_a_rebuilt_mic_stream_does() {
    // YV104's ghost-tap recovery destroys and recreates the tap mid-meeting,
    // which means a new IOProc, a new block and a new `TapClock`. The mic side
    // has the same shape (a rebuilt cpal stream re-derives its epoch), and
    // `plan_silence_splices` already handles the rebase by refusing intervals
    // that run backwards. What it cannot handle is one producer rebasing and
    // the other not.
    let first_stream = TapClock::new();
    assert_eq!(first_stream.stamp(ABSOLUTE_BOOT_TICKS), 0);
    assert!(first_stream.stamp(ABSOLUTE_BOOT_TICKS + 240_000) > 0);

    let after_rebuild = TapClock::new();
    assert_eq!(
        after_rebuild.stamp(ABSOLUTE_BOOT_TICKS + 900_000_000_000),
        0,
        "a rebuilt tap is a new stream and starts its own clock at zero"
    );
}

#[test]
fn a_host_time_that_runs_backwards_clamps_instead_of_wrapping() {
    // The HAL re-timing a device under us is the one way a later callback can
    // carry an EARLIER `mHostTime`. `saturating_sub` makes that 0; a wrapping
    // subtraction would make it ~1.8e19 ns, which `plan_silence_splices` would
    // read as a monotonic step of 585 years and turn into a fabricated stall.
    let clock = TapClock::new();
    assert_eq!(clock.stamp(ABSOLUTE_BOOT_TICKS), 0);
    assert_eq!(clock.stamp(ABSOLUTE_BOOT_TICKS - 1_000_000), 0);
    // And the clock is not disturbed by it: the epoch is still the first value.
    assert!(clock.stamp(ABSOLUTE_BOOT_TICKS + 240_000) > 0);
}
