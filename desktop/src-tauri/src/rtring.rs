//! YV91 — the real-time-safe hand-off out of an audio callback.
//!
//! The panel's OS-7 finding is that `record.rs`'s capture callback did three
//! things a real-time thread must never do: `scratch.extend(...)` (may
//! reallocate), `dsp.lock()` (a blocking `std::sync::Mutex` with no priority
//! inheritance) and, inside that lock, growth of two unbounded `Vec`s plus a
//! fresh `Vec<i16>` allocation per callback. A five-second dictation never
//! exposes that. Three continuous hours on a meeting — and, in 22-B, the same
//! body running as a CoreAudio tap IOProc where a missed deadline glitches the
//! call the user is actually listening to — does.
//!
//! So the callback body becomes: copy the interleaved frames into a
//! preallocated lock-free SPSC ring, write one `(host_time, sample_index,
//! frame_count)` record into a second ring, return. Every bit of DSP moves to a
//! normal-priority consumer thread draining the rings. This module is that ring,
//! and it is deliberately built ONCE here for the mic path so 22-B reuses it
//! verbatim rather than growing a second copy.
//!
//! ## The contract this type keeps
//!
//! * **Single producer, single consumer.** The producer is the audio callback;
//!   the consumer is one normal-priority thread. Two producers, or two
//!   consumers, is a bug this type does not defend against (and does not need
//!   to: there is exactly one input stream).
//! * **The producer never allocates, never locks, never blocks and never
//!   panics on a full ring.** A full ring means the consumer fell behind, and
//!   the frames that did not fit are counted as [`SpscRing::overruns`] — a
//!   number the watchdog can read — rather than blocking the audio thread or
//!   growing memory without limit. This is the same "the queue is bounded, the
//!   loss is COUNTED, never silent" posture YV63's journal already takes.
//! * **The ring is preallocated at construction and never grows.** Capacity is
//!   the entire memory budget of the capture path.
//!
//! Zero-allocation on the producer side is not a claim, it is a test:
//! `tests/meeting_capture_rt_safety.rs` installs a counting global allocator and
//! fails if a single allocation happens inside the callback body.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, Ordering};

/// One capture-callback timing record: the host clock at the moment these
/// frames were captured, the cumulative per-channel frame index they start at,
/// and how many frames they are.
///
/// This single datum is four things at once, which is exactly why it is built
/// here and not in four places: it is OS-2's cross-track alignment key (22-B),
/// merged finding #12's journal index record (so a dropped spill chunk is
/// detectable as a GAP instead of silently shifting every timestamp after it),
/// finding #3's resume anchor for `processed_through_seconds`, and the gap
/// detector itself.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct CaptureAnchor {
    /// Host clock in nanoseconds, as reported by the callback's own capture
    /// timestamp (cpal's `InputCallbackInfo::timestamp().capture`, which is
    /// `mach_absolute_time`-derived on macOS). NOT `Instant::now()` — the point
    /// is the time the ADC captured the frames, not the time we noticed.
    pub host_ns: u64,
    /// Cumulative per-channel frame index of the FIRST frame in this callback.
    pub sample_index: u64,
    /// Per-channel frames in this callback.
    pub frames: u32,
    /// Native capture rate these frames are at. Carried per-anchor on purpose:
    /// YV92 handles a mid-meeting format change, and a rate that changes must
    /// be visible in the record rather than assumed constant for the session.
    pub sample_rate: u32,
    /// CUMULATIVE per-channel frames this ring has refused, as of this
    /// callback — the producer's running total, not this callback's share.
    ///
    /// Cumulative on purpose, twice over: an anchor the anchor ring itself
    /// dropped loses no information (the next one carries the total), and the
    /// consumer never has to reconstruct a sum from a sequence it cannot prove
    /// is complete. This is the number that turns a ring overrun into a GAP the
    /// journal's index records can see — without it, lost audio and audio that
    /// never existed are the same observation.
    pub lost_frames: u64,
}

/// A bounded, preallocated, lock-free single-producer/single-consumer ring.
///
/// `T: Copy + Default` because the backing store is allocated once, up front,
/// and never reallocated — there is no drop glue to run on overwrite and no
/// path that can allocate after construction.
pub struct SpscRing<T: Copy + Default> {
    buf: UnsafeCell<Box<[T]>>,
    cap: usize,
    /// Total items ever written. Monotonic; the producer alone advances it.
    write: AtomicU64,
    /// Total items ever read. Monotonic; the consumer alone advances it.
    read: AtomicU64,
    /// Items the producer could not fit because the consumer fell behind.
    overruns: AtomicU64,
}

// SAFETY: the producer only ever writes the `[write, write+n)` window and the
// consumer only ever reads the `[read, write)` window; the two windows are
// disjoint by construction and the Acquire/Release pairing on `write`/`read`
// publishes the payload before the index that exposes it. `T: Copy` means no
// drop glue can run concurrently with either side.
unsafe impl<T: Copy + Default + Send> Sync for SpscRing<T> {}
unsafe impl<T: Copy + Default + Send> Send for SpscRing<T> {}

impl<T: Copy + Default> SpscRing<T> {
    /// Allocate a ring holding `capacity` items. All of the memory this ring
    /// will ever use is committed here, on the caller's (non-real-time) thread.
    pub fn with_capacity(capacity: usize) -> Self {
        let cap = capacity.max(1);
        Self {
            buf: UnsafeCell::new(vec![T::default(); cap].into_boxed_slice()),
            cap,
            write: AtomicU64::new(0),
            read: AtomicU64::new(0),
            overruns: AtomicU64::new(0),
        }
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// Items currently waiting for the consumer.
    pub fn len(&self) -> usize {
        let w = self.write.load(Ordering::Acquire);
        let r = self.read.load(Ordering::Acquire);
        w.saturating_sub(r) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Items the producer had to drop because the ring was full. A non-zero
    /// value is a real defect (the consumer is too slow or the ring is too
    /// small) and the meeting watchdog surfaces it rather than letting it pass
    /// as "audio that never existed".
    pub fn overruns(&self) -> u64 {
        self.overruns.load(Ordering::Relaxed)
    }

    /// Total items the producer has accepted, ever.
    pub fn written(&self) -> u64 {
        self.write.load(Ordering::Acquire)
    }

    /// Total items the consumer has taken, ever.
    pub fn consumed(&self) -> u64 {
        self.read.load(Ordering::Acquire)
    }

    /// PRODUCER SIDE. Copy `src` into the ring, converting each element with
    /// `f` on the way in.
    ///
    /// No allocation, no lock, no syscall, no unbounded work: a fixed number of
    /// stores plus two atomic operations. Returns how many items were accepted;
    /// a short return means the ring was full and the remainder was counted as
    /// an overrun. NEVER blocks and NEVER panics.
    pub fn push_mapped<S: Copy>(&self, src: &[S], f: impl Fn(S) -> T) -> usize {
        if src.is_empty() {
            return 0;
        }
        let w = self.write.load(Ordering::Relaxed);
        let r = self.read.load(Ordering::Acquire);
        let used = w.saturating_sub(r) as usize;
        let free = self.cap.saturating_sub(used);
        let n = src.len().min(free);
        if n < src.len() {
            self.overruns
                .fetch_add((src.len() - n) as u64, Ordering::Relaxed);
        }
        if n > 0 {
            // SAFETY: single producer; the window written here is disjoint from
            // anything the consumer may read until the Release store below.
            let buf = unsafe { &mut *self.buf.get() };
            let start = (w % self.cap as u64) as usize;
            for (i, &s) in src.iter().take(n).enumerate() {
                let mut idx = start + i;
                if idx >= self.cap {
                    idx -= self.cap;
                }
                buf[idx] = f(s);
            }
            self.write.store(w + n as u64, Ordering::Release);
        }
        n
    }

    /// PRODUCER SIDE. Push one item; `false` if the ring was full (counted).
    pub fn push_one(&self, value: T) -> bool {
        self.push_mapped(std::slice::from_ref(&value), |v| v) == 1
    }

    /// CONSUMER SIDE. Append everything currently available to `dst` and return
    /// how many items were moved. `dst` is the consumer's own reusable buffer;
    /// give it capacity once and steady-state drains allocate nothing either.
    pub fn drain_into(&self, dst: &mut Vec<T>) -> usize {
        let r = self.read.load(Ordering::Relaxed);
        let w = self.write.load(Ordering::Acquire);
        let n = w.saturating_sub(r) as usize;
        if n == 0 {
            return 0;
        }
        // SAFETY: single consumer; this window was published by the producer's
        // Release store and the producer will not reuse it until our Release
        // store below.
        let buf = unsafe { &*self.buf.get() };
        let start = (r % self.cap as u64) as usize;
        for i in 0..n {
            let mut idx = start + i;
            if idx >= self.cap {
                idx -= self.cap;
            }
            dst.push(buf[idx]);
        }
        self.read.store(r + n as u64, Ordering::Release);
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_in_order_across_the_wrap() {
        let ring: SpscRing<f32> = SpscRing::with_capacity(8);
        let mut out = Vec::new();
        // Three pushes of 5 over a capacity of 8 forces two wraps.
        for round in 0..3u32 {
            let src: Vec<f32> = (0..5).map(|i| (round * 5 + i) as f32).collect();
            assert_eq!(ring.push_mapped(&src, |s| s), 5, "round {round} fit");
            ring.drain_into(&mut out);
        }
        let expect: Vec<f32> = (0..15).map(|i| i as f32).collect();
        assert_eq!(out, expect, "every item comes out once, in order");
        assert_eq!(ring.overruns(), 0);
    }

    #[test]
    fn a_full_ring_counts_the_overrun_and_never_blocks() {
        let ring: SpscRing<f32> = SpscRing::with_capacity(4);
        let src = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0];
        // Six into four: four land, two are counted, the call returns.
        assert_eq!(ring.push_mapped(&src, |s| s), 4);
        assert_eq!(ring.overruns(), 2, "the loss is counted, never silent");
        let mut out = Vec::new();
        ring.drain_into(&mut out);
        assert_eq!(out, vec![1.0, 2.0, 3.0, 4.0], "no partial/torn item");
    }

    #[test]
    fn conversion_happens_on_the_way_in_so_no_scratch_buffer_is_needed() {
        // This is why `push_mapped` takes a closure instead of a `&[f32]`: the
        // callback's samples are i16/u16/f32 and converting them into a scratch
        // Vec first is exactly the allocation OS-7 wants gone.
        let ring: SpscRing<f32> = SpscRing::with_capacity(16);
        let src: [i16; 4] = [0, i16::MAX, i16::MIN, -1];
        ring.push_mapped(&src, |s| s as f32 / i16::MAX as f32);
        let mut out = Vec::new();
        ring.drain_into(&mut out);
        assert_eq!(out.len(), 4);
        assert!((out[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn anchors_ride_the_same_ring_type() {
        let ring: SpscRing<CaptureAnchor> = SpscRing::with_capacity(4);
        assert!(ring.push_one(CaptureAnchor {
            host_ns: 42,
            sample_index: 0,
            frames: 512,
            sample_rate: 48_000,
            lost_frames: 0,
        }));
        let mut out = Vec::new();
        ring.drain_into(&mut out);
        assert_eq!(out[0].host_ns, 42);
        assert_eq!(out[0].frames, 512);
    }

    #[test]
    fn producer_and_consumer_agree_on_the_totals_under_real_threads() {
        use std::sync::Arc;
        let ring: Arc<SpscRing<f32>> = Arc::new(SpscRing::with_capacity(1024));
        let producer = Arc::clone(&ring);
        let handle = std::thread::spawn(move || {
            let block = [0.5f32; 64];
            let mut sent = 0usize;
            while sent < 100_000 {
                sent += producer.push_mapped(&block, |s| s);
                std::thread::yield_now();
            }
        });
        let mut got = Vec::with_capacity(1024);
        let mut total = 0usize;
        while total < 100_000 {
            total += ring.drain_into(&mut got);
            got.clear();
            std::thread::yield_now();
        }
        handle.join().expect("producer finished");
        assert_eq!(ring.consumed(), ring.written());
    }
}
