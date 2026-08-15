//! YV100 — the **manual** smoke, `#[ignore]`d because it cannot be a gate.
//!
//! Everything else about this item is provable with no audio hardware; this is
//! the one criterion that is not. It creates a REAL process tap, which means a
//! Mac on macOS 14.4+ and — the part that makes it unautomatable —
//! **a TCC grant**. There is no public API to request audio-recording
//! permission and none to query it: the prompt is a side effect of
//! `AudioHardwareCreateProcessTap`/`AudioDeviceStart`, and TCC is keyed to the
//! **code-signing identity**, so an unsigned `cargo test` binary is not the same
//! subject as the signed Yap.app the user granted. Run from `cargo test`, this
//! either prompts for a binary the user has never heard of or fails outright,
//! and neither outcome is evidence about the shipping app. That is exactly why
//! YV102 exists (pre-warm the grant from an explicit onboarding step) and why
//! YV101's acceptance is written against a signed, notarized build.
//!
//! So it is ignored by default and run by hand:
//!
//! ```text
//! cargo test --test syscapture_manual_smoke -- --ignored --nocapture
//! ```
//!
//! **What to check, and why both halves matter.** Play something in another app
//! (Spotify, a YouTube tab) and confirm the captured RMS is well above zero.
//! Then stop everything and confirm it drops to silence. The second half is not
//! a formality: it is the only check that distinguishes exclude-**self** from
//! exclude-**nothing**. An empty exclusion list produces a tap that works
//! beautifully and includes Yap's own output, and the plan's §2.1 gotcha is that
//! this inversion is silent — a tap that records everything looks healthier than
//! one that records correctly.

#![cfg(target_os = "macos")]

#[test]
#[ignore = "needs macOS 14.4 + a real TCC audio-capture grant; see the module docs"]
fn a_real_tap_delivers_non_silent_audio_while_another_app_plays() {
    // No ring is passed in, and there is no correct value that could have been:
    // the tap's rate and channel count are only readable after the tap exists.
    // `start_system_tap` opens it, reads `kAudioTapPropertyFormat`, and builds
    // the ring from that — so the format printed below and the ring drained
    // below are the same fact by construction, not by a matching pair of
    // constants that happened to be typed correctly.
    let tap = match wilson_voice_lib::syscapture::imp::start_system_tap() {
        Ok(tap) => tap,
        Err(err) => panic!(
            "the tap could not start: {err}\n\
             On macOS 12/13 this is expected (YV101's gate). On 14.4+ with audio \
             capture denied, this is the denial — grant it in System Settings › \
             Privacy & Security › Audio Recording and run again."
        ),
    };
    let capture = std::sync::Arc::clone(tap.capture());
    let format = tap.format();
    println!(
        "tap format: {} Hz, {} ch",
        format.sample_rate, format.channels
    );
    assert_eq!(capture.sample_rate(), format.sample_rate);
    assert_eq!(capture.channels(), format.channels);

    // Ten seconds, drained on this (normal-priority) thread exactly the way the
    // meeting consumer will — never from the IOProc.
    let mut samples: Vec<f32> = Vec::new();
    let mut anchors = Vec::new();
    let started = std::time::Instant::now();
    while started.elapsed() < std::time::Duration::from_secs(10) {
        capture.samples.drain_into(&mut samples);
        capture.anchors.drain_into(&mut anchors);
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    tap.stop();

    assert!(
        !capture.callback_panicked(),
        "an IOProc callback unwound — see the catch-unwind test"
    );
    assert_eq!(capture.overruns(), 0, "the consumer kept up");
    assert!(
        !samples.is_empty(),
        "the tap delivered no frames at all in ten seconds"
    );

    // The frame counts the anchors report are in the tap's OWN channel count,
    // which is the number the IOProc trimmed by. On a stereo tap read through a
    // mono ring this total is double the truth, with nothing else to show for
    // it — the defect the format binding exists to make impossible.
    let anchor_frames: u64 = anchors
        .iter()
        .map(|a: &wilson_voice_lib::rtring::CaptureAnchor| a.frames as u64)
        .sum();
    assert_eq!(
        anchor_frames * format.channels as u64,
        samples.len() as u64,
        "anchor frame counts must agree with the interleaved sample total at the \
         tap's own channel count"
    );
    assert!(
        anchors
            .iter()
            .all(|a: &wilson_voice_lib::rtring::CaptureAnchor| a.sample_rate == format.sample_rate),
        "every anchor is stamped with the tap's real rate"
    );

    let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    println!(
        "captured {} frames over {} callbacks — rms {rms:.6}, peak {peak:.6}",
        samples.len(),
        anchors.len()
    );
    println!(
        "AUDIBLE-CHECK: with another app playing, rms should be >> 0.001. \
         With everything silent, it should be ~0 — and if Yap itself is making \
         noise and it shows up here, the exclusion inverted."
    );
}
