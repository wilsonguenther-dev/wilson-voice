//! YV91 acceptance — **the capture-callback body allocates nothing and blocks
//! on nothing** (plan finding OS-7).
//!
//! OS-7's acceptance criterion is written as a harness, not as a review note:
//! *"a test harness with a scoped allocator hook that fails if the IOProc body
//! allocates or blocks"*. This is that harness.
//!
//! A counting global allocator is installed for this test binary. A thread-local
//! arm flag turns counting on for exactly the duration of the callback body and
//! off again, so the test's own setup — which allocates freely — is not
//! measured. Anything the body does that reaches the allocator (a `Vec::push`
//! past capacity, a `to_string`, a `Box`, a channel send that grows a queue,
//! a log line) shows up as a non-zero count and fails the test.
//!
//! "Blocks on nothing" is proven structurally rather than statistically: the
//! body's only synchronisation is the ring's two atomics, and the ring is
//! bounded — a full ring returns immediately with a counted overrun instead of
//! waiting for the consumer. The overrun test below is what makes that
//! falsifiable: if anybody ever "fixes" a full ring by blocking, the test that
//! pushes past capacity stops returning.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use wilson_voice_lib::meeting::{rt_capture_callback, RtCapture, RING_FRAMES};

// ── The scoped allocator hook ───────────────────────────────────────────────

thread_local! {
    /// Counting is armed per-thread and off by default, so the harness — which
    /// allocates all it likes — is not measured, and so two tests running in
    /// parallel on different threads cannot count each other's allocations.
    /// (A process-global counter looks simpler and is silently wrong: cargo
    /// runs the tests in this file concurrently.)
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

/// Count this allocation if the current thread is inside an armed region.
/// `try_with` because a thread tearing down may have destroyed the TLS slot,
/// and an allocation during TLS teardown is not the callback's. Both slots are
/// `const`-initialized `Cell`s, so the hook itself never allocates.
fn note() {
    let armed = ARMED.try_with(|a| a.get()).unwrap_or(false);
    if armed {
        let _ = COUNT.try_with(|c| c.set(c.get() + 1));
    }
}

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator;

/// Run `body` with allocation counting armed; return how many allocations it
/// made on THIS thread.
fn allocations_during(body: impl FnOnce()) -> usize {
    let before = COUNT.with(|c| c.get());
    ARMED.with(|a| a.set(true));
    body();
    ARMED.with(|a| a.set(false));
    COUNT.with(|c| c.get()) - before
}

// ── The harness proves itself first ────────────────────────────────────────

#[test]
fn the_allocator_hook_actually_sees_allocations() {
    // A test that can only pass is not a test. Before trusting a zero, prove
    // the counter is wired up by making it non-zero on purpose.
    let seen = allocations_during(|| {
        let mut v: Vec<u64> = Vec::new();
        for i in 0..1024 {
            v.push(i);
        }
        std::hint::black_box(&v);
    });
    assert!(
        seen > 0,
        "the counting allocator saw nothing — the hook is not armed, so every \
         'zero allocations' result below would be meaningless"
    );
}

// ── The acceptance criterion ───────────────────────────────────────────────

const CALLBACKS: usize = 2_000;

#[test]
fn the_capture_callback_body_allocates_nothing() {
    // A realistic 48 kHz stereo callback: 512 frames, the size CoreAudio hands
    // out by default. Everything the body will ever need is preallocated by
    // `RtCapture::new`, on this thread, before counting starts.
    let capture = RtCapture::new(48_000, 2);
    let block = vec![0.25f32; 512 * 2];
    let mut drained: Vec<f32> = Vec::with_capacity(RING_FRAMES);
    let mut anchors = Vec::with_capacity(1024);

    // Warm the harness once outside the measurement.
    rt_capture_callback(&capture, &block, |s| s, 0);
    capture.samples.drain_into(&mut drained);
    capture.anchors.drain_into(&mut anchors);
    drained.clear();
    anchors.clear();

    let mut total = 0usize;
    for i in 0..CALLBACKS {
        total += allocations_during(|| {
            rt_capture_callback(&capture, &block, |s| s, (i as u64 + 1) * 10_666_666);
        });
        // Draining is the CONSUMER's job and is allowed to allocate — it runs
        // disarmed, exactly as it runs off the audio thread in production.
        capture.samples.drain_into(&mut drained);
        capture.anchors.drain_into(&mut anchors);
        drained.clear();
        anchors.clear();
    }

    assert_eq!(
        total, 0,
        "the capture callback allocated {total} time(s) over {CALLBACKS} invocations — \
         OS-7's whole point is that this number is zero"
    );
    assert!(!capture.callback_panicked());
}

#[test]
fn the_callback_allocates_nothing_for_i16_or_u16_devices_either() {
    // The conversion closure is what replaced the `scratch.extend(...)` Vec, so
    // the non-f32 device formats are exactly where a scratch buffer would sneak
    // back in.
    let capture = RtCapture::new(48_000, 1);
    let i16_block = vec![1234i16; 480];
    let u16_block = vec![32_768u16; 480];
    rt_capture_callback(&capture, &i16_block, |s| s as f32 / i16::MAX as f32, 0);
    let mut sink = Vec::with_capacity(RING_FRAMES);
    capture.samples.drain_into(&mut sink);
    sink.clear();

    let i16_allocs = allocations_during(|| {
        rt_capture_callback(&capture, &i16_block, |s| s as f32 / i16::MAX as f32, 1);
    });
    capture.samples.drain_into(&mut sink);
    sink.clear();
    let u16_allocs = allocations_during(|| {
        rt_capture_callback(
            &capture,
            &u16_block,
            |s| (s as f32 - 32_768.0) / 32_768.0,
            2,
        );
    });
    assert_eq!((i16_allocs, u16_allocs), (0, 0));
}

#[test]
fn a_full_ring_returns_immediately_instead_of_waiting_for_the_consumer() {
    // "Blocks on nothing", made falsifiable: nobody drains this ring, and the
    // callback still returns — over-capacity frames are COUNTED, never waited
    // on. If a future change makes a full ring block, this test hangs.
    let capture = RtCapture::new(48_000, 1);
    let block = vec![0.5f32; 4_096];
    let mut allocations = 0usize;
    let started = std::time::Instant::now();
    for i in 0..(RING_FRAMES / 4_096 + 4) {
        allocations += allocations_during(|| {
            rt_capture_callback(&capture, &block, |s| s, i as u64);
        });
    }
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "the callback waited on a full ring"
    );
    assert_eq!(
        allocations, 0,
        "and it did not allocate on the overrun path"
    );
    assert!(
        capture.overruns() > 0,
        "the frames that did not fit are counted, so the watchdog can see them"
    );
    assert!(!capture.callback_panicked());
}
