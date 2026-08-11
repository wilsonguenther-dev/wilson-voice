//! YV92 — the input format-change state machine, driven by a synthetic
//! `kAudioDevicePropertyStreamFormat` event sequence (plan finding OS-9).
//!
//! No audio hardware, no AirPods, no corpus: the same shape as
//! `sysaudio::restore_plan`'s tests, for the same reason. The interesting part
//! of "AirPods went in at minute ten" is not the FFI — it is the decision the
//! capture path makes when it hears about it, and that decision has to be
//! provable on a CI runner with no microphone.
//!
//! One thing this file CANNOT prove, and deliberately does not claim to: that
//! production holds a single watch across a whole take. Every test here keeps
//! one `InputFormatWatch` alive by construction, so the segment counter looks
//! right whatever the capture path does with it — which is exactly how the
//! shipped path once managed to stamp `segment_index: 1` on all three markers of
//! an AirPods sequence while these tests stayed green. That ownership is proved
//! against the production objects instead, in `record::tests`
//! (`every_swap_of_one_take_opens_the_next_segment` and
//! `a_reopen_is_not_a_segment_and_a_new_take_restarts_the_numbering`), which
//! drive a real `LiveStream` through the real `observe_input` the watchdog calls
//! and read the indices back off the real marker sidecar.
//!
//! Each test below is one of the three things the acceptance criteria demand of
//! an event, plus the failure modes that make them meaningful: a burst of
//! selectors is not three segments, a rate the OS could not read is not a
//! reason to install a garbage ratio, and the ratio after the event must come
//! from the NEW nominal rate rather than the one the stream happened to open
//! at.

use wilson_voice_lib::input_format::{
    selectors, take_event, FormatChangeAction, FormatEventSource, InputFormat, InputFormatWatch,
    InputObservation, DEFAULT_TARGET_RATE,
};

/// A MacBook's built-in mic: 48 kHz, mono after cpal's default config.
fn built_in() -> InputFormat {
    InputFormat::new(48_000, 1)
}

/// AirPods in normal operation report 24 kHz (Apple Forums 770232).
fn airpods() -> InputFormat {
    InputFormat::new(24_000, 1)
}

/// Engaging the AirPods MICROPHONE renegotiates the link to HFP/SCO, which
/// drags the rate down again — the second half of the OS-9 story.
fn airpods_hfp() -> InputFormat {
    InputFormat::new(16_000, 1)
}

fn watching() -> InputFormatWatch {
    InputFormatWatch::new("MacBook Pro Microphone", built_in(), DEFAULT_TARGET_RATE)
}

fn event(
    device: &str,
    format: InputFormat,
    host_time: u64,
    output_sample_index: u64,
    selector: u32,
) -> InputObservation {
    InputObservation {
        device_name: device.to_string(),
        format,
        host_time,
        output_sample_index,
        source: FormatEventSource::from_selector(selector),
    }
}

#[test]
fn a_stream_format_event_opens_a_segment_writes_a_marker_and_retunes_the_ratio() {
    let mut watch = watching();
    assert_eq!(watch.segment_index(), 0, "a capture starts on segment 0");
    assert_eq!(watch.ratio().from_hz, 48_000);

    // Minute ten of a lecture: AirPods go in. The HAL announces it as a stream
    // format change on the live input device.
    let action = watch.observe(event(
        "AirPods Pro",
        airpods(),
        7_310_522_118_400,
        9_600_000, // 600 s of 16 kHz output written before the change
        selectors::STREAM_FORMAT,
    ));

    // (1) a new spill segment opens
    let FormatChangeAction::Reconfigure { marker, ratio } = action else {
        panic!("a real format change must be a Reconfigure, got {action:?}");
    };
    assert_eq!(marker.segment_index, 1, "the change opens segment 1");
    assert_eq!(watch.segment_index(), 1);

    // (2) a `device_change` marker with the correct host time
    assert_eq!(marker.host_time, 7_310_522_118_400);
    assert_eq!(marker.output_sample_index, 9_600_000);
    assert_eq!(marker.from_device, "MacBook Pro Microphone");
    assert_eq!(marker.to_device, "AirPods Pro");
    assert_eq!(marker.from, built_in());
    assert_eq!(marker.to, airpods());
    assert_eq!(marker.source, FormatEventSource::StreamFormat);
    let json = marker.to_json();
    assert_eq!(json["kind"], "device_change");
    assert_eq!(json["host_time"], 7_310_522_118_400u64);
    assert_eq!(json["output_sample_index"], 9_600_000u64);
    assert_eq!(json["segment_index"], 1);
    assert_eq!(json["from_sample_rate"], 48_000);
    assert_eq!(json["to_sample_rate"], 24_000);
    assert_eq!(json["source"], "stream_format");

    // (3) the resample ratio after the event matches the NEW nominal rate
    assert_eq!(ratio.from_hz, 24_000, "the ratio follows the device");
    assert_eq!(ratio.to_hz, DEFAULT_TARGET_RATE);
    assert!((ratio.value() - 1.5).abs() < 1e-9);
    assert_eq!(
        watch.ratio(),
        ratio,
        "the watch and the action must agree on the live ratio"
    );
}

#[test]
fn a_ratio_never_survives_a_format_change() {
    // The failure OS-9 describes: at minute ten the ratio is still 3.0 and the
    // remaining eighty minutes come out at half speed. Walk the whole AirPods
    // sequence — 48 k → 24 k (inserted) → 16 k (mic engaged, HFP) → 48 k (taken
    // out) — and assert the ratio tracks every step.
    let mut watch = watching();
    for (n, (device, format, expected)) in [
        ("AirPods Pro", airpods(), 1.5),
        ("AirPods Pro", airpods_hfp(), 1.0),
        ("MacBook Pro Microphone", built_in(), 3.0),
    ]
    .into_iter()
    .enumerate()
    {
        let action = watch.observe(event(
            device,
            format,
            1_000 + n as u64,
            (n as u64 + 1) * 16_000,
            selectors::NOMINAL_SAMPLE_RATE,
        ));
        assert!(action.is_reconfigure(), "step {n} must reconfigure");
        assert_eq!(
            watch.ratio().from_hz,
            format.sample_rate_hz,
            "step {n}: the ratio must be rebuilt from the new nominal rate"
        );
        assert!(
            (watch.ratio().value() - expected).abs() < 1e-9,
            "step {n}: expected ratio {expected}, got {}",
            watch.ratio().value()
        );
        assert_eq!(watch.segment_index(), n as u32 + 1);
    }
}

#[test]
fn a_burst_of_selectors_for_one_renegotiation_is_a_single_segment() {
    // One renegotiation makes the HAL fire several properties (default input
    // device, stream format, nominal sample rate). If each one opened a segment
    // a single AirPods insertion would shred the journal into three.
    let mut watch = watching();
    let first = watch.observe(event(
        "AirPods Pro",
        airpods(),
        42,
        16_000,
        selectors::DEFAULT_INPUT_DEVICE,
    ));
    assert!(first.is_reconfigure());
    for selector in [
        selectors::STREAM_FORMAT,
        selectors::NOMINAL_SAMPLE_RATE,
        selectors::DEFAULT_INPUT_DEVICE,
    ] {
        assert_eq!(
            watch.observe(event("AirPods Pro", airpods(), 43, 16_100, selector)),
            FormatChangeAction::Unchanged,
            "a repeat of a change already applied is not another segment"
        );
    }
    assert_eq!(watch.segment_index(), 1, "one renegotiation, one segment");
}

#[test]
fn an_unreadable_format_is_ignored_rather_than_installed() {
    // CoreAudio reporting a zero rate means "I don't know yet", usually while a
    // Bluetooth link is being renegotiated. Installing a ratio from it would be
    // strictly worse than keeping the one that is working.
    let mut watch = watching();
    for bad in [InputFormat::new(0, 1), InputFormat::new(24_000, 0)] {
        let action = watch.observe(event(
            "AirPods Pro",
            bad,
            7,
            0,
            selectors::NOMINAL_SAMPLE_RATE,
        ));
        assert!(
            matches!(action, FormatChangeAction::Ignored(_)),
            "an unusable format must be ignored, got {action:?}"
        );
    }
    assert_eq!(watch.ratio().from_hz, 48_000, "the live ratio is untouched");
    assert_eq!(watch.segment_index(), 0, "and no segment was opened");

    // …and the very next usable reading is still acted on.
    assert!(watch
        .observe(event(
            "AirPods Pro",
            airpods(),
            8,
            0,
            selectors::STREAM_FORMAT
        ))
        .is_reconfigure());
}

#[test]
fn a_same_named_device_at_a_new_rate_is_still_a_format_change() {
    // The exact hole the name-keyed device cache left: AirPods keep their name
    // across a rate renegotiation, so "did the device change?" is the wrong
    // question — "did the FORMAT change?" is the right one.
    let mut watch = InputFormatWatch::new("AirPods Pro", airpods(), DEFAULT_TARGET_RATE);
    let action = watch.observe(event(
        "AirPods Pro",
        airpods_hfp(),
        99,
        320_000,
        selectors::STREAM_FORMAT,
    ));
    let marker = action.marker().expect("same name, new rate → reconfigure");
    assert_eq!(marker.from_device, marker.to_device);
    assert_eq!(marker.from.sample_rate_hz, 24_000);
    assert_eq!(marker.to.sample_rate_hz, 16_000);
    assert_eq!(watch.ratio().from_hz, 16_000);
}

/// The FFI half, on real hardware. `#[ignore]`d because it asserts something a
/// CI runner cannot honour — a GitHub macOS runner has no input device, so
/// `arm_listeners` legitimately returns false there and the app legitimately
/// falls back to the 60 s watchdog re-read. Run it on a Mac with a microphone:
///
/// ```text
/// cargo test --test input_format_change_handler -- --ignored --nocapture
/// ```
#[test]
#[ignore = "needs a real input device: run by hand on a Mac with a microphone"]
fn input_format_listeners_install_on_this_mac() {
    let armed = wilson_voice_lib::input_format::arm_listeners();
    eprintln!("AudioObjectAddPropertyListener on the default input device: armed={armed}");
    assert!(
        armed,
        "this machine has an input device, so the CoreAudio listeners must install"
    );
    // Idempotent: arming twice must not double-register or fail.
    assert!(wilson_voice_lib::input_format::arm_listeners());
    wilson_voice_lib::input_format::disarm_listeners();
    // …and disarming twice is a no-op rather than a crash.
    wilson_voice_lib::input_format::disarm_listeners();
    eprintln!("listeners removed cleanly");
}

#[test]
fn the_listener_edge_is_taken_once_and_names_its_selector() {
    // The HAL callback's whole job: record which selector fired. The capture
    // watchdog takes that edge (clearing it) and hands the source to the state
    // machine, so a burst of callbacks costs exactly one wake-up.
    assert_eq!(take_event(), None, "no event pending on a quiet system");
    wilson_voice_lib::input_format::record_event(selectors::STREAM_FORMAT);
    wilson_voice_lib::input_format::record_event(selectors::NOMINAL_SAMPLE_RATE);
    assert_eq!(
        take_event(),
        Some(FormatEventSource::NominalSampleRate),
        "the most recent selector is what the watchdog sees"
    );
    assert_eq!(take_event(), None, "the edge is consumed by the read");
}
