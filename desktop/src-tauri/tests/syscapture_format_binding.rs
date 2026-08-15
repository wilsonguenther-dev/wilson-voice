//! YV100 acceptance — **the tap's own format is what stamps its anchors**, with
//! zero audio hardware.
//!
//! ## The defect this suite exists for
//!
//! `rt_capture_callback` divides the interleaved block it is handed by
//! `RtCapture::channels` to get a frame count, and stamps every `CaptureAnchor`
//! with `RtCapture::sample_rate`. The tap's IOProc trims its `AudioBufferList`
//! by `TapFormat::channels`. Those are two different numbers unless something
//! makes them the same one, and the first cut of this module had nothing that
//! did: `start_system_tap` took a ready-made `Arc<RtCapture>` as an *argument*,
//! so the ring was fixed before `AudioHardwareCreateProcessTap` had run — and
//! `kAudioTapPropertyFormat`, the only source of truth for the tap's real rate
//! and channel count, cannot be read until after it has.
//!
//! There was therefore **no caller ordering that got it right**. That is what
//! makes this a design defect rather than a bug: a 44.1 kHz stereo tap stamped
//! through a ring built from the microphone's 48 kHz mono reports
//! `anchor.frames = 882` where the truth is `441`, and `sample_rate = 48000`
//! where the truth is `44100`, and **every CoreAudio call still returns
//! `noErr`**. Nothing in this module notices, because nothing in this module
//! divides one by the other — YV107's cross-track merge does, which is where a
//! 2× sample axis and an 8.8 % rate error would finally surface, as Track 1
//! drifting against Track 0 for reasons no measurement here would explain.
//!
//! ## The fix, and why these tests are shaped this way
//!
//! The ring is now an **output** of opening the tap, not an input to it:
//! `open_tap` reads the format and immediately calls `TapPlatform::bind_capture`,
//! which builds the ring from that format and checks it. So the tests below are
//! about *ordering* and *refusal*, which are pure, rather than about CoreAudio,
//! which is not.

use std::sync::Arc;

use wilson_voice_lib::meeting::{rt_capture_callback, RtCapture};
use wilson_voice_lib::syscapture::{
    capture_for_format, capture_matches_format, open_tap, virtual_meeting_config, TapError,
    TapFormat, TapStage,
};

#[path = "support/tap.rs"]
mod tap;
use tap::{Call, FakePlatform};

const SELF_OBJECT: Option<u32> = Some(4242);

fn open(platform: &mut FakePlatform) -> Result<wilson_voice_lib::syscapture::OpenTap, TapError> {
    open_tap(
        platform,
        SELF_OBJECT,
        "uid.under.test",
        "Yap meeting capture",
    )
}

/// The ring reaching the IOProc is built from the tap's OWN format — at 44.1 kHz
/// stereo, not at whatever the caller had in hand.
#[test]
fn the_ring_the_anchors_are_stamped_from_is_built_from_the_taps_own_format() {
    let mut platform = FakePlatform {
        format: TapFormat {
            sample_rate: 44_100,
            channels: 2,
        },
        ..FakePlatform::default()
    };
    let open = open(&mut platform).expect("setup succeeds");

    assert_eq!(open.format.sample_rate, 44_100);
    assert_eq!(open.format.channels, 2);

    let bound = platform
        .bound_capture
        .as_ref()
        .expect("bind_capture ran during open_tap");
    assert_eq!(bound.sample_rate(), 44_100);
    assert_eq!(bound.channels(), 2);
    assert!(capture_matches_format(bound, open.format).is_ok());
}

/// The binding happens in the ONE window where it can: after the format is
/// readable, before anything stamps through it.
#[test]
fn the_binding_happens_after_the_format_is_read_and_before_the_ioproc_exists() {
    let mut platform = FakePlatform::default();
    open(&mut platform).expect("setup succeeds");

    let position = |call: &Call| {
        platform
            .calls
            .iter()
            .position(|c| c == call)
            .unwrap_or_else(|| panic!("{call:?} never happened: {:?}", platform.calls))
    };
    // Reading the format is the earliest moment the truth exists...
    assert!(position(&Call::BindCapture) > position(&Call::TapFormat));
    // ...and creating the IOProc is the first moment anything can stamp through
    // the ring, so the binding must be strictly between the two. A `bind` that
    // drifted to after `CreateIoProc` is the original defect wearing a new name.
    assert!(position(&Call::BindCapture) < position(&Call::CreateIoProc));
    assert!(position(&Call::BindCapture) < position(&Call::Start));
}

/// A ring that disagrees with the format is refused — and the refusal names both
/// sides, because "the tap is 44.1/2 and the ring is 48/1" is the whole content
/// of the diagnosis.
#[test]
fn a_ring_that_disagrees_with_the_tap_is_refused_rather_than_stamped_through() {
    let format = TapFormat {
        sample_rate: 44_100,
        channels: 2,
    };
    // The exact pairing that was reproduced: the mic's ring against a 44.1 kHz
    // stereo tap.
    let mismatched = Arc::new(RtCapture::new(48_000, 1));
    assert_eq!(
        capture_matches_format(&mismatched, format),
        Err(TapError::CaptureFormatMismatch {
            tap_sample_rate: 44_100,
            tap_channels: 2,
            capture_sample_rate: 48_000,
            capture_channels: 1,
        })
    );

    let rendered = format!(
        "{}",
        capture_matches_format(&mismatched, format).unwrap_err()
    );
    assert!(rendered.contains("44100"), "{rendered}");
    assert!(rendered.contains("48000"), "{rendered}");

    // Either half alone is enough to refuse: a rate that matches with the wrong
    // channel count is the 2× sample-axis case, and a channel count that matches
    // with the wrong rate is the drift case.
    assert!(capture_matches_format(&Arc::new(RtCapture::new(44_100, 1)), format).is_err());
    assert!(capture_matches_format(&Arc::new(RtCapture::new(48_000, 2)), format).is_err());
    assert!(capture_matches_format(&capture_for_format(format), format).is_ok());
}

/// And the refusal is a real exit path: the tap that already exists is destroyed
/// before the error comes back, and no IOProc is ever created.
#[test]
fn a_refused_binding_tears_the_tap_down_and_never_creates_an_ioproc() {
    let format = TapFormat {
        sample_rate: 44_100,
        channels: 2,
    };
    let mut platform = FakePlatform {
        format,
        // Only a fake can reach this: the real platform builds its ring from the
        // format and so cannot disagree with itself. That is the point of the
        // fix — but a guard nothing can trip is a guard nothing proves.
        capture_override: Some(Arc::new(RtCapture::new(48_000, 1))),
        ..FakePlatform::default()
    };

    let err = open(&mut platform).expect_err("the mismatched ring is refused");
    assert_eq!(
        err,
        TapError::CaptureFormatMismatch {
            tap_sample_rate: 44_100,
            tap_channels: 2,
            capture_sample_rate: 48_000,
            capture_channels: 1,
        }
    );

    // The tap exists by this point and `coreaudiod` owns it, so it must be
    // destroyed — and the aggregate device must never have been created, since
    // the sequence stops before `default_output_uid`.
    assert_eq!(
        platform.teardown_calls(),
        vec![Call::DestroyTap],
        "a refused binding tears down exactly the tap: {:?}",
        platform.calls
    );
    assert!(!platform.calls.contains(&Call::CreateAggregate));
    assert!(!platform.calls.contains(&Call::CreateIoProc));
    assert!(!platform.calls.contains(&Call::Start));
    assert!(platform.bound_capture.is_none());
}

/// The stage the error is attributed to is a real stage with a readable name,
/// not a borrowed neighbour's.
#[test]
fn the_binding_is_its_own_named_stage_between_the_two_it_sits_between() {
    assert_eq!(
        TapStage::BindCapture.call(),
        "bind kAudioTapPropertyFormat to the capture ring"
    );
    // Attributing this failure to `ReadTapFormat` would blame a call that
    // succeeded, and attributing it to `CreateIoProc` would blame one that never
    // ran.
    assert_ne!(TapStage::BindCapture, TapStage::ReadTapFormat);
    assert_ne!(TapStage::BindCapture, TapStage::CreateIoProc);
}

/// THE FAILURE THE OLD SHAPE PRODUCED, MEASURED.
///
/// This is the reproduction, run through the *real* `rt_capture_callback` rather
/// than described: one 10 ms block of 44.1 kHz stereo audio, stamped once
/// through the correctly-bound ring and once through the ring the old signature
/// would have accepted. The numbers in the module docs above are this test's
/// output, and it fails the day the binding stops happening.
#[test]
fn the_wrong_ring_reports_double_the_frames_and_the_wrong_rate_with_no_error() {
    let format = TapFormat {
        sample_rate: 44_100,
        channels: 2,
    };
    // 10 ms at 44.1 kHz stereo: 441 frames, 882 interleaved samples.
    let block = vec![0.25f32; 882];

    let correct = capture_for_format(format);
    rt_capture_callback(&correct, &block, |s| s, 0);
    let mut anchors = Vec::new();
    correct.anchors.drain_into(&mut anchors);
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].frames, 441, "441 frames of stereo audio");
    assert_eq!(anchors[0].sample_rate, 44_100);

    let wrong = Arc::new(RtCapture::new(48_000, 1));
    rt_capture_callback(&wrong, &block, |s| s, 0);
    let mut wrong_anchors = Vec::new();
    wrong.anchors.drain_into(&mut wrong_anchors);
    assert_eq!(wrong_anchors.len(), 1);
    assert_eq!(
        wrong_anchors[0].frames, 882,
        "the sample axis is exactly 2x off — and nothing errored"
    );
    assert_eq!(
        wrong_anchors[0].sample_rate, 48_000,
        "the declared rate is 8.8% off — and nothing errored"
    );
    assert!(
        !wrong.callback_panicked(),
        "no failure is reported anywhere"
    );

    // Which is why the check is not optional decoration: it is the only thing
    // between that silence and YV107's merge.
    assert!(capture_matches_format(&wrong, format).is_err());
    assert!(capture_matches_format(&correct, format).is_ok());
}

/// And the session config the tap feeds is built from the same discovered
/// format, so the journal's declared rate cannot disagree with the anchors'.
#[test]
fn the_session_config_declares_the_same_rate_the_anchors_are_stamped_with() {
    let format = TapFormat {
        sample_rate: 44_100,
        channels: 2,
    };
    let dir = std::env::temp_dir().join("yv100-format-binding");
    let config = virtual_meeting_config(&dir, format);
    let capture = capture_for_format(format);

    assert_eq!(config.native_rate, capture.sample_rate());
    assert_eq!(config.channels, capture.channels());
    assert_eq!(config.tracks, 2);
}
