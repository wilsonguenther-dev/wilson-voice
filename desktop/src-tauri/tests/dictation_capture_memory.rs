//! Y3 acceptance — **a dictation take of any length holds a bounded amount of
//! audio in RAM**, because the capture consumer spills it to the take's own WAV
//! as it goes.
//!
//! The finding, measured against `record.rs` before this landed: `StreamDsp`
//! held `out` (16 kHz mono f32, 64 KB/s) and `raw` (native rate, 192 KB/s at
//! 48 kHz) and both grew on every callback with nothing bounding either, and
//! `finalize_take` then took the whole thing by value. A twenty-minute
//! dictation was ~307 MB of resident audio before a single finishing DSP stage
//! ran. `raw` is gone; `out` is now a bounded working window over a spill.
//!
//! "Resident" is counted the way `tests/meeting_capture_memory.rs` counts a
//! meeting's — the CAPACITY of every retained `f32` buffer the take owns, which
//! for dictation is `out`, the downmix scratch `mono`, and the streaming
//! resampler's `pending` + `scratch`. The two files agree on the definition on
//! purpose: a change that moved audio from one buffer to another would
//! otherwise look like a win in one suite and be invisible in the other.
//!
//! Every fixture here is SYNTHETIC — generated arithmetic — except the byte
//! identity test, which uses the committed `quick-brown-fox-16k.wav`. Same rule
//! as `tests/fixtures/README.md`: real dictation never enters this repo.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::PathBuf;

use wilson_voice_lib::capture_probe::CaptureProbe;
use wilson_voice_lib::MAX_RESIDENT_CAPTURE_BYTES;

/// The shipped default (`lib.rs`'s `denoise: true`), so the byte-identity test
/// exercises the RNNoise pass rather than a configuration nobody runs.
const SHIPPED_DENOISE: bool = true;

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yap-y3-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// A couple of seconds of a two-tone sweep, interleaved for `channels`. Built
/// once and looped, so the fixture itself is not what the measurement sees.
fn synthetic_loop(sample_rate: u32, channels: u16) -> Vec<f32> {
    let frames = sample_rate as usize * 2;
    let mut out = Vec::with_capacity(frames * channels as usize);
    for i in 0..frames {
        let t = i as f32 / sample_rate as f32;
        let s = (t * 220.0 * std::f32::consts::TAU).sin() * 0.3
            + (t * 1_400.0 * std::f32::consts::TAU).sin() * 0.1;
        for _ in 0..channels {
            out.push(s);
        }
    }
    out
}

/// Push `seconds` of `source` through a spilling capture probe in 1024-frame
/// callbacks (near what CoreAudio hands out) and report the peak resident bytes.
fn run_capture(tag: &str, sample_rate: u32, channels: u16, seconds: f64) -> usize {
    run_capture_inner(tag, sample_rate, channels, seconds, true)
}

fn run_capture_inner(
    tag: &str,
    sample_rate: u32,
    channels: u16,
    seconds: f64,
    spill: bool,
) -> usize {
    let dir = tmpdir(tag);
    let recovery = dir.join("recovery");
    std::fs::create_dir_all(&recovery).expect("recovery dir");
    let wav = dir.join("take.wav");
    let spill_paths = if spill {
        Some((wav.as_path(), recovery.as_path()))
    } else {
        None
    };
    let mut probe = CaptureProbe::new(sample_rate, channels, spill_paths)
        .expect("the temp volume has room for a capture spill");

    let source = synthetic_loop(sample_rate, channels);
    let ch = channels.max(1) as usize;
    let callback_frames = 1_024usize;
    let callback_samples = callback_frames * ch;
    let total_frames = (sample_rate as f64 * seconds) as usize;
    let callbacks = total_frames / callback_frames;

    let mut peak = probe.resident_bytes();
    let mut cursor = 0usize;
    for _ in 0..callbacks {
        if cursor + callback_samples > source.len() {
            cursor = 0;
        }
        probe.push(&source[cursor..cursor + callback_samples]);
        cursor += callback_samples;
        let now = probe.resident_bytes();
        if now > peak {
            peak = now;
        }
    }
    // The take is finished and read back here, which is where the one fully
    // resident copy legitimately exists — it is not part of the capture ceiling.
    let out = probe.finish(false).expect("a long take still finalizes");
    assert!(
        out.len() as f64 > sample_rate.min(16_000) as f64 * seconds * 0.9,
        "a bounded take must still contain the whole recording, got {} samples",
        out.len()
    );
    let _ = std::fs::remove_dir_all(&dir);
    peak
}

/// The duration case: twenty minutes at a 16 kHz native rate. The audio's only
/// home is the spill, so the resident bytes must be flat in the LENGTH of the
/// take. Pre-Y3 this buffer alone was 76.8 MB (and `raw` another 76.8 MB).
#[test]
fn resident_bytes_are_bounded_across_a_synthetic_twenty_minute_take() {
    let peak = run_capture("20min", 16_000, 1, 20.0 * 60.0);
    assert!(
        peak < MAX_RESIDENT_CAPTURE_BYTES,
        "twenty minutes of capture held {peak} bytes resident, ceiling is {MAX_RESIDENT_CAPTURE_BYTES}"
    );

    // LIVENESS: the same twenty minutes with NO spill installed is the pre-Y3
    // path, byte for byte. If it did not blow the ceiling, this whole file
    // would be measuring nothing — an assertion that passes on the broken shape
    // proves only that the number is big, not that the fix is reachable.
    let unbounded = run_capture_inner("20min-unbounded", 16_000, 1, 20.0 * 60.0, false);
    assert!(
        unbounded > MAX_RESIDENT_CAPTURE_BYTES,
        "the unbounded path held only {unbounded} bytes — this test cannot detect the defect it \
         claims to bound"
    );
    assert!(
        unbounded / peak >= 8,
        "spilling must be a large win, not a rounding one: {unbounded} vs {peak} bytes"
    );
    eprintln!(
        "Y3: 20 min resident — spilled {peak} B, unbounded {unbounded} B \
         ({}x), ceiling {MAX_RESIDENT_CAPTURE_BYTES} B",
        unbounded / peak
    );
}

/// The rate case: the real 48 kHz device rate, which is the path that also
/// exercises the streaming resampler's pending buffer and the downmix scratch.
/// Those are the buffers that would grow if anybody re-introduced retention,
/// and the 16 kHz duration run above never touches them.
#[test]
fn a_forty_eight_kilohertz_take_does_not_retain_its_resampler_state() {
    let peak = run_capture("48k", 48_000, 2, 120.0);
    assert!(
        peak < MAX_RESIDENT_CAPTURE_BYTES,
        "two minutes at 48 kHz stereo held {peak} bytes resident, ceiling is {MAX_RESIDENT_CAPTURE_BYTES}"
    );
}

/// The regression that would be easy to miss: spilling must not change a single
/// sample. RNNoise (`denoise: true`, the shipped default) still runs over the
/// whole take, and the take it runs over is the one that came back off the disk.
///
/// The comparison is against the SAME `StreamDsp` and the SAME `finalize_take`
/// with no spill installed — which is the pre-Y3 unbounded-RAM path, preserved
/// verbatim as the disk-failure fallback. Equality is on the raw bits, not an
/// epsilon: an i16 spill (the obvious implementation, and what the YV63 journal
/// uses) would quietly requantise every take and pass an epsilon check.
#[test]
fn short_take_output_is_byte_identical_to_the_pre_spill_path() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/quick-brown-fox-16k.wav");
    let mut reader = hound::WavReader::open(&fixture).expect("the committed fixture");
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 16_000, "fixture is 16 kHz");
    let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .samples::<i32>()
            .filter_map(|s| s.ok())
            .map(|s| s as f32 * scale)
            .collect(),
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(|s| s.ok()).collect(),
    };
    assert!(!samples.is_empty(), "fixture has audio");

    let dir = tmpdir("identical");
    let recovery = dir.join("recovery");
    std::fs::create_dir_all(&recovery).expect("recovery dir");
    let wav = dir.join("take.wav");

    let mut spilled = CaptureProbe::new(spec.sample_rate, spec.channels, Some((&wav, &recovery)))
        .expect("spill opens");
    let mut in_ram = CaptureProbe::new(spec.sample_rate, spec.channels, None)
        .expect("the RAM path needs no disk");
    for chunk in samples.chunks(1_024) {
        spilled.push(chunk);
        in_ram.push(chunk);
    }
    let from_disk = spilled.finish(SHIPPED_DENOISE).expect("spilled take");
    let from_ram = in_ram.finish(SHIPPED_DENOISE).expect("in-RAM take");

    assert_eq!(
        from_disk.len(),
        from_ram.len(),
        "the spill must not change the take's length"
    );
    let differing = from_disk
        .iter()
        .zip(&from_ram)
        .filter(|(a, b)| a.to_bits() != b.to_bits())
        .count();
    assert_eq!(
        differing,
        0,
        "{differing} of {} samples differ — the spill round-trip is not lossless",
        from_ram.len()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A take that dies mid-spill is strictly easier to recover than a lost Vec:
/// the WAV header is finalized so the prefix on disk is playable, and the file
/// is moved into `data_dir()/recovery/` next to the YV63 journal's artefacts.
///
/// Its name is `<id>.capture.wav`, which is NOT a journal marker
/// (`<id>.in_progress.json`), so `recover_orphaned_journals` walks past it and
/// the two recovery mechanisms cannot confuse each other.
#[test]
fn spill_survives_a_mid_take_abort_and_lands_in_the_recovery_dir() {
    let dir = tmpdir("abort");
    let recovery = dir.join("recovery");
    std::fs::create_dir_all(&recovery).expect("recovery dir");
    let wav = dir.join("take.wav");

    let mut probe = CaptureProbe::new(16_000, 1, Some((&wav, &recovery))).expect("spill opens");
    let name = probe.spill_file_name().expect("the spill has a name");
    assert!(
        name.ends_with(".capture.wav") && !name.ends_with(".in_progress.json"),
        "the spill must not look like a journal marker, got {name}"
    );

    // Three seconds, so the drain has definitely run at least once and the
    // file on disk is more than a header.
    let source = synthetic_loop(16_000, 1);
    let mut pushed = 0usize;
    while pushed < 16_000 * 3 {
        let end = (pushed % source.len()) + 1_024;
        let start = pushed % source.len();
        if end > source.len() {
            pushed += 1_024;
            continue;
        }
        probe.push(&source[start..end]);
        pushed += 1_024;
    }

    // The abort: the take is dropped without ever being closed.
    probe.abort();

    let landed = recovery.join(&name);
    assert!(
        landed.exists(),
        "an aborted take's spill must land in the recovery dir, looked for {}",
        landed.display()
    );
    assert!(
        !wav.with_extension("capture.wav").exists(),
        "the aborted spill must be moved, not copied and left behind"
    );
    let recovered = hound::WavReader::open(&landed).expect("the abandoned spill is a playable wav");
    let frames = recovered.duration();
    assert!(
        frames > 16_000,
        "the recovered prefix must hold the audio captured before the abort, got {frames} frames"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ── OS-7: the audio callback still allocates nothing ────────────────────────
//
// Y3 added a disk write to the capture path. It belongs on the CONSUMER thread
// (normal priority, already holding the DSP mutex), and the audio callback must
// still do nothing but copy into the ring — `rtring.rs`'s entire premise. This
// is the same scoped-allocator harness `tests/meeting_capture_rt_safety.rs`
// uses, pointed at the same `rt_capture_callback`, because the dictation and
// meeting paths share that one callback: if the spill ever crept into it, both
// suites would go red together.

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
    let _ = ARMED.try_with(|a| {
        if a.get() {
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        }
    });
}

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator;

/// Run `body` with allocation counting armed for exactly its duration.
fn counted(body: impl FnOnce()) -> usize {
    COUNT.with(|c| c.set(0));
    ARMED.with(|a| a.set(true));
    body();
    ARMED.with(|a| a.set(false));
    COUNT.with(|c| c.get())
}

#[test]
fn the_audio_callback_still_never_allocates() {
    use wilson_voice_lib::meeting::{rt_capture_callback, RtCapture};

    let capture = RtCapture::new(48_000, 2);
    let block: Vec<f32> = synthetic_loop(48_000, 2)[..2_048].to_vec();

    // Warm the callback once OUTSIDE the counter: the first call touches lazily
    // initialised thread-locals, which is setup, not steady state.
    rt_capture_callback(&capture, &block, |s| s, 0);

    let allocations = counted(|| {
        for i in 0..64u64 {
            rt_capture_callback(&capture, &block, |s| s, i * 1_000_000);
        }
    });
    assert_eq!(
        allocations, 0,
        "the capture callback allocated {allocations} time(s) — Y3's spill must stay on the consumer thread"
    );
}
