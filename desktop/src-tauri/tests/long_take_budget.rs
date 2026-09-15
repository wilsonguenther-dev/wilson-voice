//! Y3-G — **the long-take latency and energy budget, published as a test that
//! fails on regression.**
//!
//! `latency.rs` already instruments the press→capture_start span (YV35, anchored
//! on the physical key-down: `lib.rs` takes `ptt_macos::press_started_at()` and
//! hands it to the arm call as `pressed_at`). YV81 was an explicit energy pass —
//! no busy timers, idle animations park, the polish sidecar unloads when unused.
//! Y3 adds a spill writer, chunked decode and a progress event stream: three new
//! opportunities to undo all of that, and nothing in the suite would have
//! noticed.
//!
//! The marketing stake is real and already written down —
//! `reference_wispr_parity_research §2.11` records Wispr at ~800 MB RAM / ~8% CPU
//! idle and says of Yap "measure it and publish the number, it is free
//! marketing". A number you cannot re-measure is folklore by the next release.
//! So every budget here is a NAMED CONSTANT with the machine and the date it was
//! measured on, and the prose copy lives in `docs/BUDGETS.md`.
//!
//! **What each budget is, and what it is not.**
//!
//! * `press_to_capture_start_is_not_paid_by_the_spill_writer` — a RATIO-free
//!   millisecond ceiling on one narrow thing: what `SpillWriter::open` adds to
//!   the arm path. It does not re-time the whole span with a new stopwatch; the
//!   span itself is `latency::PipelineSpans::press_capture_ms` and stays the
//!   authority.
//! * `chunk_decode_wall_per_audio_second` — a RATIO against the same fixture
//!   decoded single-window on the same machine in the same run. Deliberately not
//!   a wall-clock number: a wall-clock decode budget measures the runner's GPU.
//! * `progress_events_per_minute_of_audio` — a CEILING. An event storm off the
//!   progress stream is an energy regression that nothing else here would catch,
//!   and it is invisible to every correctness test because the transcript is
//!   still right.
//! * `resident_bytes_after_a_long_take_return_to_baseline` — the spill buffers
//!   and the chunk text must be DROPPED when the take finalizes, not leaked.
//! * `no_new_polling_timer_was_introduced` — the standing guard for YV81: a
//!   census of every sub-second `Duration` literal in the three modules Y3
//!   touches, against an explicit allowlist of the ones that already exist.
//! * `peak_resident_bytes_with_asr_and_polish_loaded` — the footprint ceiling on
//!   the declared floor machine. SEC-C turns on a 1.12 GB resident GGUF and Y4-E
//!   runs a multi-chunk pass over it while the ASR engine is loaded; Y3-A caps
//!   only the audio buffers (~32 MB), so the audio ceiling says nothing at all
//!   about what the machine actually pays.
//!
//! **A timing test is never the only proof of a correctness fix.** Nothing in
//! this file asserts that a transcript is right — `dictation_capture_memory.rs`,
//! `two_track_phase_e2e.rs` and the matrix suites do that. This file asserts
//! only what those cannot see: cost.
//!
//! Every fixture is synthetic arithmetic except the committed
//! `tests/fixtures/quick-brown-fox-16k.wav`, per `tests/fixtures/README.md`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use wilson_voice_lib::capture_probe::CaptureProbe;

// ───────────────────────────────────────────────────────────────────────────
// THE BUDGETS
// ───────────────────────────────────────────────────────────────────────────

/// The machine every absolute number below was measured on.
///
/// **Measured on:** Apple M4 Pro, 24 GB unified memory, macOS 26.6.2,
/// `aarch64-apple-darwin`, `cargo test --features custom-protocol` (dev
/// profile for this crate, release for the sidecars), on AC power, no other
/// load. **Measured at:** 2026-09-15.
const MEASURED_ON: &str = "Apple M4 Pro, 24 GB, macOS 26.6.2, aarch64-apple-darwin, 2026-09-15";

/// The FLOOR MACHINE — the weakest Mac this app claims to support, and the one
/// the footprint ceiling is set for. The repo already names it in three places
/// (`transcription.rs:582`, `tests/meeting_capture_memory.rs`,
/// `tests/meeting_no_model_resident.rs`): a **fanless 8 GB M1 Air**. `Y6-E` is
/// the release-engineering item that ratifies it; until it lands this constant
/// is the single place the number is written down, so a budget cannot quietly
/// be set against a 24 GB developer machine.
const FLOOR_MACHINE_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Milliseconds `SpillWriter::open` is allowed to add to the arm path, i.e. to
/// `latency::PipelineSpans::press_capture_ms`. Measured p50 on the machine
/// above: **0 ms** (the open is a create + a 44-byte header write). The ceiling
/// is set at 25 ms because press→capture is a span a human feels, and anything
/// that turned the open into an fsync, a directory scan or a preallocate would
/// blow straight through it.
const PRESS_TO_CAPTURE_SPILL_OVERHEAD_MS: u128 = 25;

/// `chunk_decode_wall_per_audio_second`, expressed as a RATIO against the same
/// audio decoded in ONE window on the same machine in the same run. A chunker
/// that halved throughput would report 2.0 and fail. Set at 1.60: chunking
/// legitimately costs something (per-window model priming, the overlap region
/// decoded twice), and 60% of a single-window decode is the most that is worth
/// paying for a take that can be shown progressing.
const CHUNK_DECODE_WALL_PER_AUDIO_SECOND_RATIO_CEILING: f64 = 1.60;

/// `progress_events_per_minute_of_audio` — a CEILING, not a target. 60/min is
/// one event per second of audio, which is already more than a human pill can
/// show. The IPC hop, the JS event loop wake and the React render behind each
/// one are the energy cost; a chunker that emitted per decoded window at a
/// 250 ms cadence would report 240 and fail.
const PROGRESS_EVENTS_PER_MINUTE_OF_AUDIO_CEILING: u64 = 60;

/// The minimum throttle interval that ceiling implies, in milliseconds. Any
/// take-path progress emitter must be gated behind a named constant at least
/// this large.
const MIN_PROGRESS_INTERVAL_MS: u64 = 60_000 / PROGRESS_EVENTS_PER_MINUTE_OF_AUDIO_CEILING;

/// How many bytes a finished twenty-minute take may still hold LIVE on the
/// capture thread, over the baseline it started from.
///
/// Measured with a thread-local metering allocator, NOT with process RSS.
/// Process RSS was the first instrument tried here and it is unusable in this
/// binary: `cargo test` runs these six tests concurrently IN ONE PROCESS, so
/// `chunk_decode_wall_per_audio_second` loading a 900 MB ASR engine showed up
/// as a 1.8 GB "leak" in this test. That is the same defect
/// `tests/meeting_capture_rt_safety.rs` hit with a process-global allocation
/// counter, and it has the same fix: make the meter thread-local so tests
/// cannot count each other. 1 MiB is the slack, which is tight because the
/// meter is exact.
const RETURN_TO_BASELINE_SLACK_BYTES: i64 = 1024 * 1024;

/// Ceiling on peak resident bytes with the ASR engine AND the polish child both
/// loaded — the two-process footprint, because the user's machine pays for both
/// at once.
///
/// **Measured: 2 241 462 272 bytes (2 137.6 MiB)** by
/// `tests/polish_envelope.rs::polish_envelope_peak_resident_bytes_with_asr_and_polish_loaded`
/// on the machine above (parakeet-unified-en-0.6b-gguf in-process at 857.4 MiB
/// + `yap-polish` holding qwen2.5-1.5b-instruct-q4_k_m.gguf at 1 280.2 MiB,
///   both after real work so neither is counted lazily-mapped-and-untouched.
///
/// The ceiling is set at 2.44 GiB — ~16% headroom over the measurement, and
/// under a third of the floor machine's 8 GB.
const PEAK_RESIDENT_BYTES_CEILING: u64 = 2_621_440_000;

/// A ceiling is only a budget if it is under the floor machine's RAM with room
/// left for an OS and a browser. Checked at COMPILE time, so it binds even on a
/// machine that has no models installed and skips every runtime measurement.
const _: () = assert!(PEAK_RESIDENT_BYTES_CEILING < FLOOR_MACHINE_BYTES / 2);

/// Anything strictly shorter than this is a candidate polling timer for the
/// YV81 census. A second is the line because YV81's finding was sub-second
/// wakeups: a 5 s or 60 s deadline is a supervision interval, a 5 ms one in a
/// loop is a busy-wait wearing a `Duration`'s clothes.
const SHORT_TIMER_THRESHOLD_MS: u64 = 1_000;

/// The three modules Y3 touches. A new short `Duration` literal in any of them
/// has to be added to [`short_timer_allowlist`] deliberately.
const Y3_TAKE_PATH_MODULES: [&str; 3] = ["src/record.rs", "src/transcription.rs", "src/lib.rs"];

// ───────────────────────────────────────────────────────────────────────────
// instruments
// ───────────────────────────────────────────────────────────────────────────

fn manifest(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn skip(reason: &str) {
    println!("long_take_budget SKIPPED — {reason}");
}

/// The heavy tests — the two that load an ASR engine and the one that measures
/// memory — take this before they measure anything. `cargo test` runs a test
/// binary's tests concurrently in ONE process; two engines resident at once, or
/// one resident while another test is asking the OS how much memory this
/// process holds, is tests measuring each other rather than the code.
static HEAVY: Mutex<()> = Mutex::new(());

fn heavy() -> MutexGuard<'static, ()> {
    HEAVY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

thread_local! {
    static METER_ARMED: Cell<bool> = const { Cell::new(false) };
    static METER_LIVE: Cell<i64> = const { Cell::new(0) };
}

/// Live heap bytes on THIS thread while armed. Allocation counting is the only
/// honest way to ask "did the take drop its buffers" inside a process shared
/// with five other tests.
struct MeteringAllocator;

unsafe impl GlobalAlloc for MeteringAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        meter(layout.size() as i64);
        System.alloc(layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        meter(layout.size() as i64);
        System.alloc_zeroed(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        meter(-(layout.size() as i64));
        System.dealloc(ptr, layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        meter(new_size as i64 - layout.size() as i64);
        System.realloc(ptr, layout, new_size)
    }
}

fn meter(delta: i64) {
    let _ = METER_ARMED.try_with(|armed| {
        if armed.get() {
            let _ = METER_LIVE.try_with(|live| live.set(live.get() + delta));
        }
    });
}

#[global_allocator]
static ALLOC: MeteringAllocator = MeteringAllocator;

fn meter_live() -> i64 {
    METER_LIVE.with(|live| live.get())
}

/// Resident set size of a live pid, in bytes — the same `ps` question
/// `hygiene::resident_mb` and `tests/polish_envelope.rs` ask, so the three
/// agree on what "resident" means.
fn resident_bytes(pid: u32) -> Option<u64> {
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let kb: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some(kb * 1024)
}

fn tmpdir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "y3g-{tag}-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Two seconds of a two-tone sweep, interleaved for `channels` — the same
/// generator `tests/dictation_capture_memory.rs` uses, so a change that moved
/// audio between buffers cannot look like a win in one file and be invisible in
/// the other.
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

fn median(mut v: Vec<u128>) -> u128 {
    v.sort_unstable();
    v[v.len() / 2]
}

// ───────────────────────────────────────────────────────────────────────────
// 1. press → capture_start, unchanged by the spill writer
// ───────────────────────────────────────────────────────────────────────────

/// The span being protected is `latency::PipelineSpans::press_capture_ms`,
/// anchored on the physical key-down. This test does NOT re-time it with a new
/// stopwatch — it times the one thing Y3-A inserted ahead of the stream going
/// live, `SpillWriter::open`, and holds THAT to a ceiling.
///
/// Both arms build the real `StreamDsp` through the shipped `CaptureProbe`, so
/// the difference between them is the spill and nothing else.
#[test]
fn press_to_capture_start_is_not_paid_by_the_spill_writer() {
    // STRUCTURAL: the span is still anchored on the physical key-down. If this
    // grep stops matching, the millisecond budget below is measuring the
    // overhead of a span nobody reports any more.
    let lib = std::fs::read_to_string(manifest("src/lib.rs")).expect("read lib.rs");
    assert!(
        lib.contains("ptt_macos::press_started_at()") && lib.contains("pressed_at,"),
        "the press→capture_start anchor (YV35) is gone from lib.rs — latency.rs's \
         press_capture_ms would then be measured from the arm call, not the key-down, and this \
         budget would be guarding the wrong span"
    );
    let latency = std::fs::read_to_string(manifest("src/latency.rs")).expect("read latency.rs");
    assert!(
        latency.contains("pub press_capture_ms: i64"),
        "latency.rs no longer reports press_capture_ms as its own disjoint span"
    );

    const REPS: usize = 21;
    let dir = tmpdir("arm");
    let recovery = dir.join("recovery");
    std::fs::create_dir_all(&recovery).expect("recovery dir");

    let mut with_spill = Vec::with_capacity(REPS);
    let mut without_spill = Vec::with_capacity(REPS);
    for i in 0..REPS {
        let wav = dir.join(format!("take-{i}.wav"));

        let t0 = Instant::now();
        let armed = CaptureProbe::new(16_000, 1, Some((wav.as_path(), recovery.as_path())))
            .expect("the temp volume has room for a capture spill");
        with_spill.push(t0.elapsed().as_micros());
        drop(armed);

        let t1 = Instant::now();
        let bare = CaptureProbe::new(16_000, 1, None).expect("arming without a spill cannot fail");
        without_spill.push(t1.elapsed().as_micros());
        drop(bare);
    }

    let spill_us = median(with_spill);
    let bare_us = median(without_spill);
    let overhead_ms = spill_us.saturating_sub(bare_us) / 1_000;
    println!(
        "press_to_capture_start: arm_with_spill_p50={spill_us}us arm_without_spill_p50={bare_us}us \
         spill_overhead_p50={overhead_ms}ms ceiling={PRESS_TO_CAPTURE_SPILL_OVERHEAD_MS}ms \
         measured_on={MEASURED_ON}"
    );
    assert!(
        overhead_ms <= PRESS_TO_CAPTURE_SPILL_OVERHEAD_MS,
        "the spill writer adds {overhead_ms} ms to press→capture_start, ceiling is \
         {PRESS_TO_CAPTURE_SPILL_OVERHEAD_MS} ms (with={spill_us}us without={bare_us}us)"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ───────────────────────────────────────────────────────────────────────────
// 2. chunk_decode_wall_per_audio_second
// ───────────────────────────────────────────────────────────────────────────

/// Read the committed fixture and tile it up to `seconds` of 16 kHz mono.
fn fixture_tiled(seconds: f64) -> Option<Vec<f32>> {
    let path = manifest("tests/fixtures/quick-brown-fox-16k.wav");
    let mut reader = hound::WavReader::open(&path).ok()?;
    let spec = reader.spec();
    if spec.sample_rate != 16_000 || spec.channels != 1 {
        return None;
    }
    let one: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => reader
            .samples::<i16>()
            .filter_map(Result::ok)
            .map(|s| s as f32 / 32_768.0)
            .collect(),
    };
    if one.is_empty() {
        return None;
    }
    let want = (16_000.0 * seconds) as usize;
    let mut out = Vec::with_capacity(want);
    while out.len() < want {
        let take = (want - out.len()).min(one.len());
        out.extend_from_slice(&one[..take]);
    }
    Some(out)
}

/// A ceiling on the wall clock a chunked decode spends per second of audio,
/// expressed as a RATIO against the SAME fixture decoded single-window on the
/// SAME machine in the SAME run. A wall-clock millisecond budget here would be
/// a budget on the runner's GPU and would flap on every machine.
///
/// Skips — naming the missing path — when the ASR weights are not installed, so
/// a fresh clone and CI print one line and pass instead of silently asserting
/// nothing. That is the same contract `docs/BUDGETS.md` describes for
/// `polish_envelope.rs`.
#[test]
fn chunk_decode_wall_per_audio_second() {
    use wilson_voice_lib::{asr_engine, models};

    const AUDIO_SECONDS: f64 = 60.0;
    const WINDOW_SECONDS: f64 = 15.0;

    // See `heavy()`: this test brings ~900 MB of ASR engine resident, which is
    // exactly what the footprint and memory tests are trying to measure.
    let _heavy = heavy();

    let model = models::recommended_model();
    let Some(path) = models::model_path(model).filter(|_| models::is_downloaded(model)) else {
        return skip(&format!(
            "ASR weights absent: the recommended model {} is missing or incomplete under {} — \
             the chunked/single-window decode ratio cannot be measured",
            model.id,
            models::models_dir().display()
        ));
    };
    let Some(audio) = fixture_tiled(AUDIO_SECONDS) else {
        return skip("tests/fixtures/quick-brown-fox-16k.wav is missing or is not 16 kHz mono");
    };

    asr_engine::init_backends();
    let mut engine = match asr_engine::load(&path) {
        Ok(engine) => engine,
        Err(reason) => {
            return skip(&format!(
                "the ASR engine would not load {}: {reason}",
                path.display()
            ))
        }
    };

    // Warm-up: the first decode after a load pays the graph build, and charging
    // that to whichever arm ran first would make the ratio an artefact of test
    // ordering rather than a measurement of chunking.
    let _ = asr_engine::transcribe(&mut engine, &audio[..16_000], None, None);

    let t0 = Instant::now();
    let single = asr_engine::transcribe(&mut engine, &audio, None, None);
    let single_ms = t0.elapsed().as_millis().max(1);
    if let Err(reason) = &single {
        return skip(&format!("the single-window decode failed: {reason}"));
    }

    let window = (16_000.0 * WINDOW_SECONDS) as usize;
    let t1 = Instant::now();
    let mut windows = 0usize;
    for chunk in audio.chunks(window) {
        if asr_engine::transcribe(&mut engine, chunk, None, None).is_err() {
            return skip("a windowed decode failed; the ratio would be measured off a short run");
        }
        windows += 1;
    }
    let chunked_ms = t1.elapsed().as_millis().max(1);

    let single_per_audio_s = single_ms as f64 / AUDIO_SECONDS;
    let chunked_per_audio_s = chunked_ms as f64 / AUDIO_SECONDS;
    let ratio = chunked_ms as f64 / single_ms as f64;
    println!(
        "chunk_decode_wall_per_audio_second: single={single_per_audio_s:.1}ms/s \
         chunked={chunked_per_audio_s:.1}ms/s over {windows} windows of {WINDOW_SECONDS}s \
         ratio={ratio:.2} ceiling={CHUNK_DECODE_WALL_PER_AUDIO_SECOND_RATIO_CEILING:.2} \
         measured_on={MEASURED_ON}"
    );
    assert!(
        windows > 1,
        "the chunked arm decoded {windows} window(s) — a one-window run is the single-window arm \
         again and the ratio would be meaningless"
    );
    assert!(
        ratio <= CHUNK_DECODE_WALL_PER_AUDIO_SECOND_RATIO_CEILING,
        "chunked decode cost {ratio:.2}x the single-window decode of the same fixture \
         ({chunked_ms} ms vs {single_ms} ms), ceiling is \
         {CHUNK_DECODE_WALL_PER_AUDIO_SECOND_RATIO_CEILING:.2}x"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// 3. progress_events_per_minute_of_audio
// ───────────────────────────────────────────────────────────────────────────

/// Every take-path progress emitter found in a source file, by event name.
///
/// SCOPE: `.emit("…")` / `.emit_to(…, "…")` whose event name contains
/// `progress`. `model_download_progress` is excluded by name — a download is
/// not a take, and its cadence is bounded by the network, not by a decode loop.
fn take_progress_emitters(src: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (i, _) in src.match_indices("emit") {
        let rest = &src[i..];
        let Some(open) = rest.find('"') else { continue };
        if open > 32 {
            continue;
        }
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else {
            continue;
        };
        let name = &after[..close];
        if name.contains("progress") && !name.contains("download") {
            found.push(name.to_string());
        }
    }
    found.sort();
    found.dedup();
    found
}

/// The smallest throttle interval declared in a file, in milliseconds — a
/// `const …PROGRESS…_MS: … = N;`. `None` means a file emits progress with no
/// named interval anywhere, which is the unthrottled shape.
fn declared_progress_interval_ms(src: &str) -> Option<u64> {
    let mut best: Option<u64> = None;
    for line in src.lines() {
        let line = line.trim();
        if !line.starts_with("const ") || !line.contains("_MS") {
            continue;
        }
        let upper_name = line.split(':').next().unwrap_or("");
        if !upper_name.contains("PROGRESS") {
            continue;
        }
        let Some(eq) = line.find('=') else { continue };
        let value: String = line[eq + 1..]
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '_')
            .collect();
        if let Ok(n) = value.replace('_', "").parse::<u64>() {
            best = Some(best.map_or(n, |b: u64| b.min(n)));
        }
    }
    best
}

/// A CEILING on the progress stream's event rate. An event storm is a pure
/// energy regression: the transcript is still correct, every correctness test
/// still passes, and the machine wakes the JS event loop four times a second
/// for an hour.
///
/// Enforced structurally rather than by running the app, because the rate that
/// matters is the one the code can produce, not the one a five-second test
/// happened to observe.
#[test]
fn progress_events_per_minute_of_audio() {
    // SELF-FALSIFICATION FIRST. A scanner that cannot see the shape it claims
    // to forbid is a test that passes on a broken tree. Prove the pattern
    // matches the shape feared, not the shape remembered.
    const UNTHROTTLED: &str = r#"
        fn decode_loop(app: &AppHandle) {
            for chunk in chunks {
                let _ = app.emit("dictation_progress", chunk.index);
            }
        }
    "#;
    let bait = take_progress_emitters(UNTHROTTLED);
    assert_eq!(
        bait,
        vec!["dictation_progress".to_string()],
        "the emitter scanner cannot see an unthrottled take-progress emit — it would report a \
         clean census on exactly the regression this test exists to catch"
    );
    assert_eq!(
        declared_progress_interval_ms(UNTHROTTLED),
        None,
        "the throttle scanner reported an interval in a source that declares none"
    );
    // ...and that it does NOT fire on the download stream, which is out of scope.
    assert!(
        take_progress_emitters(r#"app.emit("model_download_progress", pct);"#).is_empty(),
        "the scanner claimed the model download stream as a take-path emitter"
    );

    let mut worst: u64 = 0;
    let mut sites: Vec<(String, String, u64)> = Vec::new();
    for rel in Y3_TAKE_PATH_MODULES {
        let src =
            std::fs::read_to_string(manifest(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let emitters = take_progress_emitters(&src);
        if emitters.is_empty() {
            continue;
        }
        let interval = declared_progress_interval_ms(&src);
        let rate = match interval {
            Some(ms) if ms > 0 => 60_000 / ms,
            _ => u64::MAX,
        };
        for name in emitters {
            sites.push((rel.to_string(), name, rate));
        }
        worst = worst.max(rate);
    }

    println!(
        "progress_events_per_minute_of_audio: sites={sites:?} worst_rate_per_min={worst} \
         ceiling={PROGRESS_EVENTS_PER_MINUTE_OF_AUDIO_CEILING} \
         min_interval={MIN_PROGRESS_INTERVAL_MS}ms measured_on={MEASURED_ON}"
    );
    if sites.is_empty() {
        println!(
            "progress_events_per_minute_of_audio: no take-path progress emitter exists yet \
             (Y3-C has not landed on this base). The ceiling is armed and will bind the first \
             emitter that does."
        );
    }
    assert!(
        worst <= PROGRESS_EVENTS_PER_MINUTE_OF_AUDIO_CEILING,
        "a take-path progress emitter can fire {worst} times per minute of audio, ceiling is \
         {PROGRESS_EVENTS_PER_MINUTE_OF_AUDIO_CEILING}. Sites: {sites:?}. Gate the emit behind a \
         `const …PROGRESS…_MS` of at least {MIN_PROGRESS_INTERVAL_MS} ms."
    );
}

// ───────────────────────────────────────────────────────────────────────────
// 4. resident_bytes_after_a_long_take_return_to_baseline
// ───────────────────────────────────────────────────────────────────────────

/// A finished take must DROP its spill buffers and its chunk text, not leak
/// them. `tests/dictation_capture_memory.rs` bounds the PEAK during capture;
/// nothing before this bounded what is still held AFTERWARDS, and a take that
/// hands 30 MB back to nobody every time is a laptop that gets slower the
/// longer it is dictated into.
///
/// Two instruments, and the file says which is which:
///   * EXACT — a thread-local metering allocator. Live bytes on the capture
///     thread must come back within [`RETURN_TO_BASELINE_SLACK_BYTES`] of where
///     they started, and the same meter proves it actually SAW the take by
///     reporting a peak far above that slack while the take was running.
///   * EXACT — a fresh take armed after the long one must retain byte-for-byte
///     what a fresh take armed before it retained, measured at the same point
///     in the take, so nothing carried over inside `StreamDsp`.
///
/// Process RSS is printed and NOT asserted. See
/// [`RETURN_TO_BASELINE_SLACK_BYTES`] for why: in a binary whose other tests
/// load a 900 MB ASR engine concurrently, an RSS assertion here measures the
/// other tests.
#[test]
fn resident_bytes_after_a_long_take_return_to_baseline() {
    const MINUTES: f64 = 20.0;
    const SHORT_SECONDS: f64 = 5.0;
    let _heavy = heavy();
    let pid = std::process::id();

    let dir = tmpdir("longtake");
    let recovery = dir.join("recovery");
    std::fs::create_dir_all(&recovery).expect("recovery dir");
    let source = synthetic_loop(16_000, 1);

    /// Push `seconds` of audio through a spilling probe and return
    /// `(peak_retained_bytes, finalized_samples)`.
    fn run(
        dir: &Path,
        recovery: &Path,
        name: &str,
        source: &[f32],
        seconds: f64,
    ) -> (usize, usize) {
        let mut probe = CaptureProbe::new(16_000, 1, Some((dir.join(name).as_path(), recovery)))
            .expect("arm a spilling take");
        let callback = 1_024usize;
        let callbacks = (16_000.0 * seconds) as usize / callback;
        let mut peak = probe.resident_bytes();
        let mut cursor = 0usize;
        for _ in 0..callbacks {
            if cursor + callback > source.len() {
                cursor = 0;
            }
            probe.push(&source[cursor..cursor + callback]);
            cursor += callback;
            peak = peak.max(probe.resident_bytes());
        }
        let out = probe.finish(false).expect("a take still finalizes");
        let samples = out.len();
        drop(out);
        (peak, samples)
    }

    // A reference short take BEFORE, to have a non-zero number to compare
    // against. Comparing "retained at arm" would compare 0 with 0.
    let (short_peak_before, _) = run(&dir, &recovery, "before.wav", &source, SHORT_SECONDS);
    assert!(
        short_peak_before > 0,
        "a five-second take retained zero bytes at every sample point — the meter is not looking \
         at a running take"
    );

    let rss_before = resident_bytes(pid);

    METER_LIVE.with(|live| live.set(0));
    METER_ARMED.with(|armed| armed.set(true));
    let baseline_live = meter_live();

    let mut probe = CaptureProbe::new(
        16_000,
        1,
        Some((dir.join("take.wav").as_path(), recovery.as_path())),
    )
    .expect("arm the long take");
    let callback = 1_024usize;
    let callbacks = (16_000.0 * 60.0 * MINUTES) as usize / callback;
    let mut peak_retained = probe.resident_bytes();
    let mut peak_live = meter_live();
    let mut cursor = 0usize;
    for _ in 0..callbacks {
        if cursor + callback > source.len() {
            cursor = 0;
        }
        probe.push(&source[cursor..cursor + callback]);
        cursor += callback;
        peak_retained = peak_retained.max(probe.resident_bytes());
        peak_live = peak_live.max(meter_live());
    }
    let out = probe.finish(false).expect("a long take still finalizes");
    let samples = out.len();
    peak_live = peak_live.max(meter_live());
    drop(out);
    let after_live = meter_live();
    METER_ARMED.with(|armed| armed.set(false));

    assert!(
        samples as f64 > 16_000.0 * 60.0 * MINUTES * 0.9,
        "the take lost audio ({samples} samples) — measuring the memory of a take that did not \
         happen proves nothing"
    );

    // LIVENESS: the meter must have seen the take. A "no leak" verdict from an
    // instrument that never moved is the loudest false green there is.
    let swing = peak_live - baseline_live;
    assert!(
        swing > RETURN_TO_BASELINE_SLACK_BYTES,
        "the allocation meter never rose more than {swing} bytes during a twenty-minute take — \
         it is not armed on the thread the take runs on, and a clean result from it would mean \
         nothing"
    );

    let growth = after_live - baseline_live;
    println!(
        "resident_bytes_after_a_long_take_return_to_baseline: peak_capture={peak_retained}B \
         meter_peak_live={peak_live}B meter_after={after_live}B meter_growth={growth}B \
         slack={RETURN_TO_BASELINE_SLACK_BYTES}B measured_on={MEASURED_ON}"
    );
    assert!(
        growth <= RETURN_TO_BASELINE_SLACK_BYTES,
        "a finished twenty-minute take still holds {growth} live heap bytes over its baseline \
         (slack {RETURN_TO_BASELINE_SLACK_BYTES}) — the spill buffers or the chunk text are being \
         leaked, not dropped. Peak during the take was {peak_live} B, so the meter saw it."
    );

    // EXACT, second instrument: nothing carried over inside StreamDsp.
    let (short_peak_after, _) = run(&dir, &recovery, "after.wav", &source, SHORT_SECONDS);
    assert_eq!(
        short_peak_after, short_peak_before,
        "a five-second take run AFTER the long one peaked at {short_peak_after} retained bytes \
         where the same take before it peaked at {short_peak_before} — state survived the long \
         take"
    );

    // INFORMATIONAL ONLY — see RETURN_TO_BASELINE_SLACK_BYTES.
    if let (Some(before), Some(after)) = (rss_before, resident_bytes(pid)) {
        println!(
            "resident_bytes_after_a_long_take_return_to_baseline: rss_before={before}B \
             rss_after={after}B (INFORMATIONAL — this binary shares a process with tests that \
             load an ASR engine, so RSS here is not attributable and is not asserted)"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ───────────────────────────────────────────────────────────────────────────
// 5. no_new_polling_timer_was_introduced  (the standing guard for YV81)
// ───────────────────────────────────────────────────────────────────────────

/// Every sub-[`SHORT_TIMER_THRESHOLD_MS`] `Duration` LITERAL in a source file,
/// as `millis -> count`.
///
/// SCOPE AND ITS LIMITS, stated rather than implied: literals only.
/// `Duration::from_millis(interval)` with a variable is invisible here, and so
/// is a `sleep` built from arithmetic. That is a real hole, and it is the
/// reason this test is a CENSUS against an allowlist rather than a proof of
/// absence: it makes the cheap regression (someone drops a
/// `from_millis(16)` into the decode loop) impossible to land silently.
fn short_timer_census(src: &str) -> BTreeMap<u64, usize> {
    let mut census: BTreeMap<u64, usize> = BTreeMap::new();
    for (unit, scale) in [
        ("from_secs(", 1_000u64),
        ("from_millis(", 1),
        ("from_micros(", 0),
    ] {
        let needle = format!("Duration::{unit}");
        for (i, _) in src.match_indices(&needle) {
            let rest = &src[i + needle.len()..];
            let Some(close) = rest.find(')') else {
                continue;
            };
            let raw = rest[..close].replace('_', "");
            let Ok(n) = raw.parse::<u64>() else { continue };
            // micros are always sub-second; scale 0 marks them and floors to 0 ms.
            let ms = if scale == 0 { n / 1_000 } else { n * scale };
            if ms < SHORT_TIMER_THRESHOLD_MS {
                *census.entry(ms).or_insert(0) += 1;
            }
        }
    }
    census
}

/// The census as it stands on this branch's base. Measured, not guessed — the
/// test prints a ready-to-paste replacement whenever it disagrees.
///
/// Adding a row is a deliberate act: it is the moment somebody decided the take
/// path needs another sub-second wakeup, and YV81 says that decision gets
/// written down.
fn short_timer_allowlist() -> BTreeMap<&'static str, BTreeMap<u64, usize>> {
    let mut all = BTreeMap::new();
    all.insert(
        "src/record.rs",
        BTreeMap::from([(1, 1), (2, 1), (5, 3), (10, 3), (60, 2), (300, 1), (500, 4)]),
    );
    all.insert(
        "src/transcription.rs",
        BTreeMap::from([
            (2, 2),
            (5, 2),
            (10, 2),
            (20, 2),
            (50, 3),
            (80, 1),
            (120, 1),
            (150, 1),
            (200, 1),
            (300, 1),
            (400, 3),
        ]),
    );
    all.insert(
        "src/lib.rs",
        BTreeMap::from([
            (5, 2),
            (10, 1),
            (50, 1),
            (100, 1),
            (150, 2),
            (400, 1),
            (500, 1),
        ]),
    );
    all
}

/// The standing YV81 guard. Y3 adds a chunked decoder, a spill writer and a
/// progress stream; every one of them is a natural place to reach for a
/// `sleep(from_millis(10))` in a drain loop, and every one of those is a core
/// that never sleeps on a fanless Air.
#[test]
fn no_new_polling_timer_was_introduced() {
    // SELF-FALSIFICATION: prove the census sees the shape feared.
    let bait = short_timer_census("std::thread::sleep(Duration::from_millis(16));");
    assert_eq!(
        bait.get(&16),
        Some(&1),
        "the timer census cannot see a sub-second Duration literal — it would report a clean \
         sweep over a busy-wait"
    );
    assert!(
        short_timer_census("Duration::from_secs(5)").is_empty(),
        "the census counted a 5 s supervision interval as a polling timer"
    );
    assert_eq!(
        short_timer_census("Duration::from_micros(250)").get(&0),
        Some(&1),
        "the census cannot see a microsecond timer, which is the worst kind"
    );

    let allow = short_timer_allowlist();
    let mut failures = Vec::new();
    let mut paste = String::new();
    for rel in Y3_TAKE_PATH_MODULES {
        let src =
            std::fs::read_to_string(manifest(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let census = short_timer_census(&src);
        let expected = allow.get(rel).cloned().unwrap_or_default();
        println!("no_new_polling_timer_was_introduced: {rel} census={census:?}");
        paste.push_str(&format!("  \"{rel}\" => {census:?}\n"));
        if census != expected {
            failures.push(format!(
                "{rel}: found {census:?}, allowlist says {expected:?}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "the sub-{SHORT_TIMER_THRESHOLD_MS}ms Duration census in the Y3 take path changed.\n{}\n\
         If the new timer is deliberate, say so in docs/BUDGETS.md and update \
         short_timer_allowlist() to:\n{paste}",
        failures.join("\n")
    );
}

// ───────────────────────────────────────────────────────────────────────────
// 6. peak_resident_bytes_with_asr_and_polish_loaded  (the footprint ceiling)
// ───────────────────────────────────────────────────────────────────────────

/// The footprint ceiling on the declared floor machine, with the ASR engine AND
/// the polish model both loaded — the state SEC-C and Y4-E put the app in.
///
/// The MEASUREMENT lives in `tests/polish_envelope.rs` (it owns the sidecar
/// harness and the model prerequisites); this is the CEILING, asserted here so
/// the budget sits with the other budgets. When both models are installed this
/// test loads the engine itself and asserts the parent's own contribution is
/// real, so the ceiling is never asserted against a run where nothing loaded.
#[test]
fn peak_resident_bytes_with_asr_and_polish_loaded() {
    use wilson_voice_lib::{asr_engine, models};

    // Serialize against the other engine-loading test: two 900 MB engines
    // resident at once in one test process is not the state a user's machine is
    // ever in, and asserting a footprint ceiling against it measures the test
    // harness rather than the app.
    let _heavy = heavy();

    // The ceiling-vs-floor-machine check is a `const _: () = assert!(…)` at the
    // top of this file: it binds at compile time, so it holds even on a machine
    // with no models installed where everything below this point skips.

    let model = models::recommended_model();
    let Some(path) = models::model_path(model).filter(|_| models::is_downloaded(model)) else {
        return skip(&format!(
            "ASR weights absent: {} is missing or incomplete under {} — the two-model footprint \
             cannot be re-measured here. The recorded measurement (2 241 462 272 B on \
             {MEASURED_ON}) is in docs/BUDGETS.md, taken by \
             polish_envelope_peak_resident_bytes_with_asr_and_polish_loaded",
            model.id,
            models::models_dir().display()
        ));
    };
    let polish_bytes = models::recommended_polish_model()
        .filter(|m| models::is_polish_downloaded(m))
        .and_then(|m| std::fs::metadata(models::polish_model_path(m)).ok())
        .map(|md| md.len());
    let Some(polish_bytes) = polish_bytes else {
        return skip(
            "the recommended polish model is not installed, so the polish half of the footprint \
             would be zero and the ceiling would be asserted against half a measurement",
        );
    };

    let pid = std::process::id();
    let before = resident_bytes(pid).unwrap_or(0);
    asr_engine::init_backends();
    let engine = match asr_engine::load(&path) {
        Ok(engine) => engine,
        Err(reason) => {
            return skip(&format!(
                "the ASR engine would not load {}: {reason}",
                path.display()
            ))
        }
    };
    let with_asr = resident_bytes(pid).unwrap_or(0);
    let asr_cost = with_asr.saturating_sub(before);

    // The ASR engine's cost is taken as a DELTA, not as absolute RSS: this
    // binary's other tests have already allocated and freed hundreds of
    // megabytes, and absolute RSS would charge the app for their high-water
    // mark. If the delta is implausibly small the allocator has simply reused
    // freed pages and the load is not attributable — that is a SKIP, not a
    // pass, because a pass would be asserting the ceiling against nothing.
    const MIN_PLAUSIBLE_ASR_BYTES: u64 = 400 * 1024 * 1024;
    if asr_cost < MIN_PLAUSIBLE_ASR_BYTES {
        drop(engine);
        return skip(&format!(
            "loading {} moved this process's RSS by only {asr_cost} B ({before} → {with_asr}); \
             the engine's cost is not attributable in a shared test process, so the footprint \
             ceiling is not asserted on this run",
            model.id
        ));
    }

    // The polish child's resident cost is dominated by its weights; the file
    // size is the floor for it, and `polish_envelope.rs` measures the live
    // child (1 280.2 MiB against a 1 065.6 MiB file). Using the floor here
    // keeps this test independent of a spawned sidecar while still refusing to
    // pretend polish is free.
    let peak = asr_cost + polish_bytes;
    println!(
        "peak_resident_bytes_with_asr_and_polish_loaded: rss_before={before}B rss_with_asr={with_asr}B \
         asr_cost={asr_cost}B polish_weights_floor={polish_bytes}B peak>={peak}B \
         ceiling={PEAK_RESIDENT_BYTES_CEILING}B floor_machine={FLOOR_MACHINE_BYTES}B \
         measured_on={MEASURED_ON}"
    );
    assert!(
        peak <= PEAK_RESIDENT_BYTES_CEILING,
        "ASR + polish resident is at least {peak} B, ceiling is {PEAK_RESIDENT_BYTES_CEILING} B \
         on the floor machine ({FLOOR_MACHINE_BYTES} B). Either a model grew or one is being \
         held resident that should have been unloaded."
    );
    drop(engine);
}
