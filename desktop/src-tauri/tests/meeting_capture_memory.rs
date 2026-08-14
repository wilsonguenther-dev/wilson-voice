//! YV91 acceptance — **a synthetic 3-hour capture stays under 400 MB peak RSS**
//! (plan finding #1).
//!
//! The finding, verified against `record.rs:1346-1401` before the panel merged
//! it: `StreamDsp` holds `out` (16 kHz) and `raw` (native rate) and both grow
//! every callback with no cap, and `finalize_take` then clones the buffer three
//! more times. A three-hour meeting on one track is roughly 2.8 GB resident
//! before post-processing starts, on a machine the plan itself names as an 8 GB
//! M1 Air. No memory budget appears anywhere in the plan — only a disk number.
//!
//! So this is the budget, as a test. A short synthetic WAV is looped through
//! the REAL capture path — `rt_capture_callback` → drain → `fan_out_block` →
//! meeting DSP → journal spill — for three hours of audio, while RSS is
//! sampled. The gate is the plan's own number: **peak RSS < 400 MB**.
//!
//! Two runs, because two different things can leak:
//!
//! * `three_hours_of_capture_stays_under_the_memory_budget` — the full 3 h at a
//!   16 kHz native rate. This is the duration case: the audio's only home is
//!   the spill, so RSS must be flat in the length of the meeting.
//! * `a_forty_eight_kilohertz_source_does_not_retain_its_resampler_state` — a
//!   shorter run at the real 48 kHz device rate, which is the path that also
//!   exercises the streaming resampler's pending buffer and the downmix
//!   scratch. Those are the buffers that would grow if anybody re-introduced
//!   retention; the duration run above would not catch a per-sample leak at a
//!   rate it never uses.
//!
//! The fixture is SYNTHETIC — generated arithmetic, not anybody's audio. Same
//! rule as `tests/fixtures/README.md`: real dictation never enters this repo.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use wilson_voice_lib::meeting::{
    fan_out_block, rt_capture_callback, session_turnstile, ExternalStream, MeetingSession,
    RtCapture, SessionConfig,
};
use wilson_voice_lib::rtring::CaptureAnchor;

/// The plan's number (finding #1's acceptance line), in megabytes.
const PEAK_RSS_BUDGET_MB: u64 = 400;
const HOURS: u64 = 3;

/// Resident set size of this process, in MB. `ps` is the portable answer on
/// macOS and it is sampled a few hundred times over a multi-second test, so its
/// cost is irrelevant.
fn rss_mb() -> u64 {
    let pid = std::process::id();
    let out = std::process::Command::new("/bin/ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u64>()
        .map(|kb| kb / 1024)
        .expect("ps prints RSS in KB")
}

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yap-yv91-mem-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// One synthetic "WAV": a couple of seconds of a two-tone sweep, looped. Built
/// once, so the fixture itself is not what the RSS measurement sees growing.
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

/// Only one meeting may be registered at a time (the capture consumer reaches
/// the active meeting through a process-global slot), so the two runs in this
/// file take turns rather than racing.
fn turn() -> std::sync::MutexGuard<'static, ()> {
    // The library's turnstile, not a private one: `MeetingSession::start`
    // refuses a second concurrent session, so every suite that starts a
    // meeting has to queue behind the same lock the others take.
    session_turnstile()
}

/// Push `hours` of `source` through the real capture path and report peak RSS.
fn run_capture(tag: &str, sample_rate: u32, channels: u16, hours: f64) -> (u64, f64) {
    let _turn = turn();
    let dir = tmpdir(tag);
    let session = MeetingSession::start(SessionConfig {
        // The blocks come from this test, not from a microphone — so the
        // session holds no stream of its own (YV91's `ExternalStream`).
        stream: Arc::new(ExternalStream),
        // The watchdog is not what this test is about; keep it out of the way.
        watchdog_interval: Duration::from_secs(3_600),
        ..SessionConfig::new(&dir, sample_rate, channels)
    })
    .expect("the temp volume has room for a synthetic meeting");

    let capture = RtCapture::new(sample_rate, channels);
    let source = synthetic_loop(sample_rate, channels);
    let ch = channels as usize;
    // 1024-frame callbacks, near what CoreAudio hands out.
    let callback_frames = 1_024usize;
    let callback_samples = callback_frames * ch;
    let total_frames = (sample_rate as f64 * 3_600.0 * hours) as usize;
    let callbacks = total_frames / callback_frames;

    let mut drained: Vec<f32> = Vec::with_capacity(callback_samples * 4);
    let mut anchors: Vec<CaptureAnchor> = Vec::with_capacity(64);
    let mut peak = rss_mb();
    let mut cursor = 0usize;
    let started = Instant::now();

    for i in 0..callbacks {
        // The callback: interleaved native frames straight into the ring.
        if cursor + callback_samples > source.len() {
            cursor = 0;
        }
        rt_capture_callback(
            &capture,
            &source[cursor..cursor + callback_samples],
            |s| s,
            (i as u64) * (callback_frames as u64) * 1_000_000_000 / sample_rate as u64,
        );
        cursor += callback_samples;

        // The consumer: drain, fan out, forget.
        drained.clear();
        anchors.clear();
        capture.anchors.drain_into(&mut anchors);
        capture.samples.drain_into(&mut drained);
        fan_out_block(&drained, &anchors, None, None);

        if i % 256 == 0 {
            let rss = rss_mb();
            if rss > peak {
                peak = rss;
            }
        }
    }
    let elapsed = started.elapsed();
    let rss = rss_mb();
    if rss > peak {
        peak = rss;
    }
    let seconds = session.capture().seconds();
    let finalized = session.stop();
    println!(
        "[yv91] {tag}: {hours:.2}h at {sample_rate} Hz x{channels} in {:.1}s wall, \
         captured {seconds:.0}s of audio, peak RSS {peak} MB, ring overruns {}",
        elapsed.as_secs_f64(),
        capture.overruns()
    );
    if let Some(finalized) = finalized.as_ref() {
        println!(
            "[yv91] {tag}: finalized {} track(s), {:.0}s, state {:?}, spliced {} sample(s)",
            finalized.tracks.len(),
            finalized.seconds,
            finalized.state,
            finalized.spliced_silence_samples
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
    (peak, seconds)
}

#[test]
fn three_hours_of_capture_stays_under_the_memory_budget() {
    let (peak_rss_mb, captured_seconds) = run_capture("3h", 16_000, 1, HOURS as f64);

    // The capture really did run for three hours of audio — otherwise a leak
    // could pass simply by not being exercised.
    let expected = (HOURS * 3_600) as f64;
    assert!(
        captured_seconds > expected * 0.99,
        "only {captured_seconds:.0}s of audio went through the path, expected ~{expected:.0}s"
    );
    assert!(
        peak_rss_mb < PEAK_RSS_BUDGET_MB,
        "peak RSS was {peak_rss_mb} MB over a {HOURS}h capture — finding #1's budget is \
         {PEAK_RSS_BUDGET_MB} MB, and the target machine is an 8 GB M1 Air"
    );
}

#[test]
fn a_forty_eight_kilohertz_source_does_not_retain_its_resampler_state() {
    // 20 minutes at the real device rate: enough that a per-sample retention in
    // the downmix scratch or the resampler's `pending` would be plainly visible
    // (20 min of stereo f32 at 48 kHz is ~460 MB if retained), short enough to
    // stay honest about CI time.
    let (peak_rss_mb, captured_seconds) = run_capture("48k", 48_000, 2, 20.0 / 60.0);
    assert!(
        captured_seconds > 1_180.0,
        "{captured_seconds:.0}s captured"
    );
    assert!(
        peak_rss_mb < PEAK_RSS_BUDGET_MB,
        "peak RSS was {peak_rss_mb} MB at 48 kHz stereo — the 3:1 resampler is retaining state"
    );
}
