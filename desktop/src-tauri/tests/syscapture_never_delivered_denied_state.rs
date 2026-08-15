//! YV102 acceptance — **"permission looks denied" vs "recording, silently".**
//!
//! The item's own acceptance line: *a synthetic tap session that never delivers
//! a non-zero sample is reported as "permission looks denied," never as
//! "recording, silently"; a session that delivered at least one non-zero sample
//! and then goes silent is **not** reported as denied (that is YV104's
//! territory, not this item's).*
//!
//! ## Why this file exists at all
//!
//! There is no public API to read system-audio permission (OS-10, quoting
//! AudioCap's README), and §2.1's original heuristic — "silence for N seconds ⇒
//! permission looks denied" — describes a **healthy** tap's symptom just as
//! exactly as a denied one's. [Apple Developer Forums thread 825780] (OS-4): a
//! working `AudioHardwareCreateProcessTap` delivers all-zero buffers while
//! system audio is audible, for minutes at a time — a 51-minute session on a
//! fanless M2 Air lost 60 s, 53 s, 141 s and, in one segment, **16 minutes 3
//! seconds**. IOProc cadence normal, timestamps normal, buffer pointers valid,
//! every sample exactly `0.0`.
//!
//! Badging that meeting "permission revoked" is a lie that also stops Yap
//! retrying. The discriminator that does not lie needs no private API and no new
//! data:
//!
//!     did this tap EVER deliver a non-zero sample?
//!
//! A TCC denial is silent from sample zero, forever. Every other silence — the
//! ghost tap, an all-muted call, an output-device mismatch (YV103), the
//! `exclusive`-flag inversion — has audio on one side of it. That single
//! boolean is a fold over `CaptureAnchor`s and sample blocks YV100 already
//! ships.
//!
//! Every case below is synthetic. No audio hardware, no 14.4, no TCC grant.

use std::time::Duration;

use wilson_voice_lib::rtring::CaptureAnchor;
use wilson_voice_lib::syscapture::{
    permission_verdict, SystemAudioPermission, TapDelivery, DENIAL_GRACE, LOOKS_DENIED_MESSAGE,
    PREWARM_DWELL,
};

const RATE: u32 = 48_000;
const FRAMES: u32 = 512;

fn anchor(callback: u64) -> CaptureAnchor {
    CaptureAnchor {
        host_ns: callback * 10_666_666,
        sample_index: callback * u64::from(FRAMES),
        frames: FRAMES,
        sample_rate: RATE,
        lost_frames: 0,
    }
}

/// One callback's worth of the exact zeros a denied (or ghosted) tap delivers.
fn silence() -> Vec<f32> {
    vec![0.0; FRAMES as usize]
}

/// One callback's worth of ordinary speech-ish audio.
fn audio(callback: u64) -> Vec<f32> {
    (0..FRAMES)
        .map(|i| ((callback as f32 + i as f32) * 0.01).sin() * 0.2)
        .collect()
}

/// Fold `n` callbacks of the given content into a delivery.
fn session(callbacks: u64, block: impl Fn(u64) -> Vec<f32>) -> TapDelivery {
    let mut delivery = TapDelivery::default();
    for i in 0..callbacks {
        delivery.observe(&anchor(i), &block(i));
    }
    delivery
}

/// **The denial case.** All-zero from sample 0, forever.
#[test]
fn a_tap_that_never_delivered_anything_is_reported_as_looks_denied() {
    // Ten minutes of a perfectly-behaved IOProc handing over pure zeros.
    let delivery = session(60_000, |_| silence());
    assert_eq!(delivery.callbacks, 60_000, "the IOProc fired the whole time");
    assert!(delivery.frames > 0, "buffers arrived — they were just empty");
    assert!(!delivery.ever_delivered());

    let verdict = permission_verdict(&delivery, Duration::from_secs(600));
    assert_eq!(verdict, SystemAudioPermission::LooksDenied);
    assert!(verdict.looks_denied());
    assert_eq!(verdict.message(), LOOKS_DENIED_MESSAGE);
}

/// The other half of the same claim, and the one a silent-by-default design
/// gets wrong: this state is never rendered as "recording".
#[test]
fn the_denied_state_is_never_reported_as_recording_silently() {
    let delivery = session(1_000, |_| silence());
    let verdict = permission_verdict(&delivery, DENIAL_GRACE);

    assert_ne!(verdict, SystemAudioPermission::Granted);
    let message = verdict.message().to_lowercase();
    assert!(
        !message.contains("is being captured"),
        "a tap that never delivered a sample must not claim it is capturing: {message}"
    );
    assert!(
        message.contains("system settings"),
        "the denied sentence has to name the only recovery there is: {message}"
    );
    // And it says "has not granted", not "you denied" — Yap cannot read TCC and
    // does not get to claim the user refused.
    assert!(
        LOOKS_DENIED_MESSAGE.contains("has not granted"),
        "the copy must not accuse the user of a denial Yap cannot observe"
    );
}

/// **The ghost-tap case (OS-4).** Good audio, then a 16-minute all-zero hole.
/// This is a HEALTHY session with a known CoreAudio bug in the middle, and
/// calling it a denial is the exact misapplication of matrix row #2 that YV104
/// exists to prevent.
#[test]
fn a_session_that_delivered_then_went_silent_is_not_denied() {
    let mut delivery = session(120, audio);
    assert!(delivery.ever_delivered(), "the good audio was observed");

    // 16 minutes 3 seconds of exactly-zero buffers, the forum's own worst case.
    for i in 0..90_000 {
        delivery.observe(&anchor(120 + i), &silence());
    }

    assert!(
        delivery.ever_delivered(),
        "the bit is sticky — one non-zero sample is a permanent fact"
    );
    let verdict = permission_verdict(&delivery, Duration::from_secs(3_060));
    assert_eq!(
        verdict,
        SystemAudioPermission::Granted,
        "silence AFTER audio is YV104's ghost tap, never this item's denial"
    );
    assert!(!verdict.looks_denied());
}

/// One sample is enough, and it is enough forever. The discriminator is a
/// latch, not a window: a window would re-badge the 16-minute hole as a denial
/// the moment it grew longer than the window.
#[test]
fn a_single_non_zero_sample_is_sticky() {
    let mut delivery = TapDelivery::default();
    delivery.observe(&anchor(0), &silence());
    assert!(!delivery.ever_delivered());

    let mut once = silence();
    once[FRAMES as usize / 2] = -0.000_03; // quiet, and negative — still audio
    delivery.observe(&anchor(1), &once);
    assert!(delivery.ever_delivered());

    for i in 2..10_000 {
        delivery.observe(&anchor(i), &silence());
        assert_eq!(
            permission_verdict(&delivery, Duration::from_secs(3_600)),
            SystemAudioPermission::Granted,
            "callback {i} un-latched a fact that cannot be un-had"
        );
    }
}

/// The grace window. Below it, "nothing yet" is the honest answer — a tap open
/// for 200 ms on a quiet Mac has told you nothing about permission.
#[test]
fn silence_below_the_grace_window_is_unknown_not_denied() {
    let delivery = session(9, |_| silence());

    for short in [
        Duration::ZERO,
        PREWARM_DWELL,
        Duration::from_millis(999),
        DENIAL_GRACE - Duration::from_millis(1),
    ] {
        assert_eq!(
            permission_verdict(&delivery, short),
            SystemAudioPermission::Unknown,
            "{short:?} of silence is not evidence of a refusal"
        );
    }

    // At the boundary, and past it, the verdict flips — once, and stays.
    assert_eq!(
        permission_verdict(&delivery, DENIAL_GRACE),
        SystemAudioPermission::LooksDenied
    );
    assert_eq!(
        permission_verdict(&delivery, DENIAL_GRACE + Duration::from_secs(1)),
        SystemAudioPermission::LooksDenied
    );
}

/// The pre-warm (YV102's own Settings step) can never produce a denial verdict.
/// It runs for 200 ms by design, which is below the grace window by design —
/// so a user who opens Settings on a silent Mac is told "nothing heard yet",
/// not "you have been denied".
#[test]
fn the_two_hundred_millisecond_prewarm_cannot_manufacture_a_denial() {
    assert!(PREWARM_DWELL < DENIAL_GRACE);
    let heard_nothing = session(9, |_| silence());
    assert_eq!(
        permission_verdict(&heard_nothing, PREWARM_DWELL),
        SystemAudioPermission::Unknown
    );

    // If something WAS playing during those 200 ms, that is real positive
    // evidence and the step is allowed to report it.
    let heard_something = session(9, audio);
    assert_eq!(
        permission_verdict(&heard_something, PREWARM_DWELL),
        SystemAudioPermission::Granted
    );
}

/// A tap whose IOProc never fires at all — the other shape a denial takes — is
/// the same verdict as one that fires zeros. Both mean "no audio ever".
#[test]
fn a_tap_whose_ioproc_never_fired_is_the_same_verdict() {
    let never_fired = TapDelivery::default();
    assert_eq!(never_fired.callbacks, 0);
    assert!(!never_fired.ever_delivered());
    assert_eq!(
        permission_verdict(&never_fired, Duration::from_secs(60)),
        SystemAudioPermission::LooksDenied
    );
}

/// Denormals and negative zero are still zero. A tap handing over `-0.0` is
/// handing over silence, and treating it as audio would turn every denial into
/// a `Granted`.
#[test]
fn negative_zero_is_still_silence() {
    let mut delivery = TapDelivery::default();
    let mut block = silence();
    block[0] = -0.0;
    block[1] = 0.0;
    delivery.observe(&anchor(0), &block);
    assert!(!delivery.ever_delivered());
    assert_eq!(
        permission_verdict(&delivery, Duration::from_secs(10)),
        SystemAudioPermission::LooksDenied
    );
}
