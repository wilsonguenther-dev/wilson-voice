//! YV110 — **the tap, inside a real meeting.**
//!
//! Everything 22-B built was merged and unreachable: `start_system_tap` had no
//! caller, `CaptureEnv::tap_liveness` answered `None` for every environment in
//! the tree, and matrix rows 1 and 2 were published as `PolicyOnly` naming
//! exactly that. This file drives the wiring that closes it, end to end, on a
//! machine with no microphone, no macOS 14.4 and no TCC grant.
//!
//! ## Why that is possible, and what it therefore does not prove
//!
//! `meeting_control::start_capture_session` is the whole of the wiring —
//! two-track config, the tap's own format announced to track 1, the pump
//! thread, the teardown on the way out, the setup row rewritten from what the
//! meeting actually heard — and it takes the tap as a parameter. The half above
//! it that needs a Mac is exactly two calls (`imp::start_system_tap`, and the
//! thread that owns it), so the fake here stands in for CoreAudio and for
//! nothing else: the `TapPlatform` seam YV100 built is the same one
//! `syscapture_teardown_order.rs` has used since that item merged.
//!
//! What no test in CI can prove is that a real Zoom call's audio arrives. That
//! is a manual step on a signed build, written down in `docs/MEETING-DEMO.md`,
//! and it is deliberately NOT claimed here.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use wilson_voice_lib::meeting::{
    self, rt_capture_callback, CaptureStream, ExternalStream, MIC_TRACK, SYSTEM_TRACK,
};
use wilson_voice_lib::meeting_control::start_capture_session;
use wilson_voice_lib::meetings::SetupVerdict;
use wilson_voice_lib::os_version_gate::{system_audio_gate, OsVersion, SystemAudioGate};
use wilson_voice_lib::rtring::CaptureAnchor;
use wilson_voice_lib::syscapture::{
    self, full_rebuild_sequence, open_tap, teardown, track_b_plan, MeetingTap, TapResources,
    TapStep, TrackBPlan, TEARDOWN_ORDER,
};

mod support;
#[path = "support/tap.rs"]
mod tap;

const RATE: u32 = 48_000;
/// One 10 ms block at 48 kHz mono — the cadence an IOProc really delivers at.
const BLOCK_FRAMES: usize = 480;

/// A tap the meeting can drive with no CoreAudio: YV100's real setup state
/// machine over YV100's fake platform, with the teardown wired to the same
/// `teardown()` the ghost watchdog's rebuild sequence begins with.
struct FakeTap {
    tap: Arc<MeetingTap>,
    platform: Arc<Mutex<tap::FakePlatform>>,
}

fn fake_tap() -> FakeTap {
    let mut platform = tap::FakePlatform::default();
    let open = open_tap(&mut platform, Some(7), "yv110-uid", "Yap meeting capture")
        .expect("the fake platform opens");
    let ring = platform
        .bound_capture
        .clone()
        .expect("open_tap binds the ring from the tap's own format");
    let platform = Arc::new(Mutex::new(platform));
    let teardown_platform = Arc::clone(&platform);
    let mut resources: TapResources = open.resources;
    let tap = MeetingTap::new(ring, open.format, move || {
        let mut platform = teardown_platform.lock().expect("fake platform");
        teardown(&mut *platform, &mut resources);
    });
    FakeTap { tap, platform }
}

fn block(audible: bool) -> Vec<f32> {
    if audible {
        (0..BLOCK_FRAMES)
            .map(|i| (i as f32 / BLOCK_FRAMES as f32) - 0.5)
            .collect()
    } else {
        vec![0.0; BLOCK_FRAMES]
    }
}

/// Feed the MIC track the way `record.rs`'s worker does: one drained block into
/// the fan-out, which serves the recording meeting first and unconditionally.
fn feed_mic(blocks: usize) {
    for i in 0..blocks {
        let samples = block(true);
        let anchors = [CaptureAnchor {
            host_ns: i as u64 * 10_000_000,
            sample_index: i as u64 * BLOCK_FRAMES as u64,
            frames: BLOCK_FRAMES as u32,
            sample_rate: RATE,
            lost_frames: 0,
        }];
        meeting::fan_out_block(&samples, &anchors, None, None);
    }
}

/// Feed the TAP the way its IOProc does: straight into the RT ring the tap's
/// own format built. Nothing here touches the meeting — the pump thread is what
/// carries it across, which is the wiring under test.
fn feed_tap(ring: &Arc<meeting::RtCapture>, blocks: usize, audible: bool) {
    for i in 0..blocks {
        rt_capture_callback(ring, &block(audible), |s| s, i as u64 * 10_000_000);
    }
}

/// Long enough for the 20 ms pump to have drained several times over, short
/// enough that the suite stays fast.
fn let_the_pump_run() {
    std::thread::sleep(syscapture::TAP_PUMP_INTERVAL * 8);
}

/// The stored row, as `db.system_audio_setup()` would read it back.
///
/// `NotRun` is the ABSENCE of a row and not a row saying "not_run" — encoding
/// that string and parsing it back gives `Ran`, deliberately (`SetupVerdict::parse`
/// degrades anything it does not recognise to "it ran", never to a denial). So
/// the helper models the storage rather than the enum.
fn setup_row(verdict: SetupVerdict) -> wilson_voice_lib::meetings::SystemAudioSetup {
    let row = (verdict != SetupVerdict::NotRun).then(|| {
        wilson_voice_lib::meetings::SystemAudioSetup::encode(verdict, "2026-08-15T00:00:00Z")
    });
    wilson_voice_lib::meetings::SystemAudioSetup::from_row(row)
}

fn external() -> Arc<dyn CaptureStream> {
    Arc::new(ExternalStream)
}

/// **The headline.** macOS is new enough and the setup step has run, so a
/// meeting started from any entry point comes up with TWO tracks and the tap's
/// audio really lands on track 1 — no extra user step anywhere in the path.
#[test]
fn meeting_on_supported_granted_attaches_track_b() {
    let _turnstile = meeting::session_turnstile();
    let dir = support::temp_dir("yv110-attach");
    let db = Arc::new(support::open_db(&dir));
    db.record_system_audio_setup(SetupVerdict::Ran)
        .expect("seed the setup row");

    // The gate and the grant, read exactly the way `SessionEngine` reads them.
    let plan = track_b_plan(SystemAudioGate::Available, &db.system_audio_setup());
    assert_eq!(plan, TrackBPlan::Attach, "gate + grant must attach Track B");
    assert_eq!(plan.tracks(), 2);
    assert_eq!(
        plan.badge(),
        None,
        "a two-track meeting has nothing to badge"
    );

    let fake = fake_tap();
    let ring = Arc::clone(fake.tap.ring());
    let capture = start_capture_session(
        &dir,
        RATE,
        1,
        plan,
        Some(Arc::clone(&fake.tap)),
        Some(Arc::clone(&db)),
        external(),
    )
    .expect("the two-track session starts");

    feed_mic(40);
    feed_tap(&ring, 40, true);
    let_the_pump_run();

    let outcome = capture.stop().expect("the meeting finalizes");
    assert!(
        outcome.wav_path.is_some(),
        "track 0 (the mic) must always land: {outcome:?}"
    );
    assert!(
        outcome.sys_wav_path.is_some(),
        "TRACK B DID NOT LAND. The tap's ring filled and the meeting finalized one wav, which is \
         a mic-only meeting wearing a two-track config: {outcome:?}"
    );

    // …and what the meeting HEARD is written back over what the 200 ms pre-warm
    // could only guess at, so the next meeting starts from evidence.
    assert_eq!(
        db.system_audio_setup().verdict,
        SetupVerdict::Granted,
        "a tap that delivered non-zero audio is the only positive proof of the grant there is, \
         and the setup row must carry it forward"
    );
}

/// Row 1, from the other end: no grant, so no tap is opened at all — and the
/// meeting still records, mic-only, carrying the sentence that says why.
#[test]
fn meeting_without_grant_stays_mic_only_with_badge() {
    let _turnstile = meeting::session_turnstile();
    let dir = support::temp_dir("yv110-no-grant");

    // Never run, and explicitly denied, are different sentences and both are
    // mic-only. Neither may be silent about it.
    assert_eq!(
        track_b_plan(SystemAudioGate::Available, &setup_row(SetupVerdict::NotRun)),
        TrackBPlan::MicOnly {
            badge: syscapture::SETUP_REQUIRED_MESSAGE
        }
    );
    let plan = track_b_plan(
        SystemAudioGate::Available,
        &setup_row(SetupVerdict::LooksDenied),
    );
    assert_eq!(
        plan,
        TrackBPlan::MicOnly {
            badge: syscapture::LOOKS_DENIED_MESSAGE
        },
        "TCC never re-asks, so a known denial must not cost a CoreAudio aggregate device and a \
         second wav of digital silence"
    );
    assert_eq!(plan.tracks(), 1);

    let capture = start_capture_session(&dir, RATE, 1, plan, None, None, external())
        .expect("a mic-only meeting starts on any machine");
    let badge = capture
        .system_audio_probe()
        .and_then(|probe| probe())
        .expect("a mic-only meeting must SAY it is mic-only");
    assert_eq!(badge, syscapture::LOOKS_DENIED_MESSAGE);

    feed_mic(20);
    let outcome = capture.stop().expect("the meeting finalizes");
    assert!(
        outcome.wav_path.is_some(),
        "row 1: the meeting is never aborted over system audio"
    );
    assert!(
        outcome.sys_wav_path.is_none(),
        "there is no second track to claim"
    );
    let note = outcome.note.unwrap_or_default();
    assert!(
        note.contains("System Settings"),
        "the badge has to survive onto the meeting itself, with the one recovery a denied user \
         has: {note}"
    );
}

/// Row 12's floor, enforced where it matters: on macOS 13 there is no
/// process-tap API, and mic-only meeting recording keeps working anyway.
#[test]
fn meeting_pre_14_4_stays_mic_only() {
    let _turnstile = meeting::session_turnstile();
    let dir = support::temp_dir("yv110-pre-144");

    // Even with the strongest possible grant on the row: the OS gate is first,
    // because no setting on this machine can conjure the API.
    let plan = track_b_plan(
        system_audio_gate(OsVersion::new(13, 6, 0)),
        &setup_row(SetupVerdict::Granted),
    );
    assert_eq!(
        plan,
        TrackBPlan::MicOnly {
            badge: wilson_voice_lib::os_version_gate::SYSTEM_AUDIO_REQUIREMENT
        }
    );
    // …and an unreadable OS version fails closed the same way.
    assert!(!track_b_plan(
        system_audio_gate(OsVersion::UNKNOWN),
        &setup_row(SetupVerdict::Granted)
    )
    .attaches());

    let capture = start_capture_session(&dir, RATE, 1, plan, None, None, external())
        .expect("22-A recording runs all the way down to the macOS 12 floor");
    assert_eq!(
        capture.system_audio_probe().and_then(|probe| probe()),
        Some(wilson_voice_lib::os_version_gate::SYSTEM_AUDIO_REQUIREMENT.to_string())
    );
    feed_mic(20);
    let outcome = capture.stop().expect("the meeting finalizes");
    assert!(outcome.wav_path.is_some());
    assert!(outcome.sys_wav_path.is_none());
}

/// Stopping the meeting tears the tap down through the SAME four calls, in the
/// same order, that YV104's 7-step rebuild begins with — not a second teardown
/// written next to it.
#[test]
fn finish_tears_down_tap_via_watchdog_path() {
    let _turnstile = meeting::session_turnstile();
    let dir = support::temp_dir("yv110-teardown");
    let fake = fake_tap();
    let ring = Arc::clone(fake.tap.ring());

    let capture = start_capture_session(
        &dir,
        RATE,
        1,
        TrackBPlan::Attach,
        Some(Arc::clone(&fake.tap)),
        None,
        external(),
    )
    .expect("the two-track session starts");
    assert!(
        fake.platform
            .lock()
            .expect("fake platform")
            .teardown_calls()
            .is_empty(),
        "nothing may be torn down while the meeting is running"
    );

    feed_mic(10);
    feed_tap(&ring, 10, true);
    let outcome = capture.stop().expect("the meeting finalizes");

    let calls = fake
        .platform
        .lock()
        .expect("fake platform")
        .teardown_calls();
    assert_eq!(
        calls,
        vec![
            tap::Call::Stop,
            tap::Call::DestroyIoProc,
            tap::Call::DestroyAggregate,
            tap::Call::DestroyTap,
        ],
        "the four calls of TEARDOWN_ORDER, in order — destroying the tap while the aggregate \
         still references it is the ordering bug that list exists to make impossible"
    );
    // And the order those four calls came in IS the head of the one declared
    // rebuild sequence, rather than a second copy that happens to agree today.
    assert_eq!(
        TEARDOWN_ORDER.to_vec(),
        full_rebuild_sequence()[..4].to_vec()
    );
    assert!(TEARDOWN_ORDER
        .iter()
        .all(|step: &TapStep| step.is_teardown()));

    // The last of the ring reached the journal BEFORE the session closed it:
    // teardown, then drain, then finalize.
    assert!(
        outcome.sys_wav_path.is_some(),
        "the audio in the ring at stop must not be dropped on the floor: {outcome:?}"
    );

    // A second stop is a no-op, not a double-free of a CoreAudio object.
    fake.tap.stop();
    assert_eq!(
        fake.platform
            .lock()
            .expect("fake platform")
            .teardown_calls()
            .len(),
        4,
        "teardown must be idempotent — `MeetingTap::stop` and its `Drop` backstop both run"
    );
}

/// The other half of the wiring, and the one matrix row 2 turns on: a meeting
/// with a tap now supplies `CaptureEnv::tap_liveness`, so the ghost watchdog's
/// whole ladder is reachable from a real session instead of only from its own
/// tests.
#[test]
fn an_attached_tap_supplies_the_watchdog_its_liveness() {
    // Held even though this test starts no session: `MeetingTap::pump` fans its
    // block out to whatever meeting is registered, and the tests in this file
    // run concurrently. Without the turnstile this one would be feeding another
    // test's track 1 while it ran.
    let _turnstile = meeting::session_turnstile();
    let fake = fake_tap();
    let ring = Arc::clone(fake.tap.ring());
    let env = syscapture::TappedEnv::new(std::path::PathBuf::from("."), Arc::clone(&fake.tap));

    // Nothing delivered yet: a STARTING tap, which is not a silent one.
    let (live, _, host_ns) = meeting::CaptureEnv::tap_liveness(&env).expect("a tap is attached");
    assert!(!live.ever_nonzero);
    assert_eq!(live.frames_delivered, 0);
    assert_eq!(host_ns, 0);

    feed_tap(&ring, 5, true);
    fake.tap.pump(Duration::from_secs(1));
    let (live, _, host_ns) = meeting::CaptureEnv::tap_liveness(&env).expect("a tap is attached");
    assert!(
        live.ever_nonzero,
        "the discriminator every YV102/YV104 verdict rests on has to be fed by the drain"
    );
    assert_eq!(live.frames_delivered, 5 * BLOCK_FRAMES as u64);
    assert!(host_ns > 0, "the most recent anchor's host time");
    assert_eq!(
        fake.tap.permission(syscapture::DENIAL_GRACE * 2),
        syscapture::SystemAudioPermission::Granted
    );

    // A tap that has never delivered, run past the grace window, is the only
    // shape this app is allowed to call a denial — and it still is not one
    // until the watchdog has spent its budget (that is row 1's own test).
    let quiet = fake_tap();
    let quiet_ring = Arc::clone(quiet.tap.ring());
    feed_tap(&quiet_ring, 5, false);
    quiet.tap.pump(Duration::from_secs(1));
    assert_eq!(
        quiet.tap.permission(syscapture::DENIAL_GRACE * 2),
        syscapture::SystemAudioPermission::LooksDenied
    );
    let (live, _, _) = quiet.tap.liveness();
    assert!(!live.ever_nonzero);
    assert!(
        live.frames_delivered > 0,
        "callbacks fired; every sample in them was exactly zero — the one thing a denied tap and \
         OS-4's ghost have in common"
    );
}

/// A tap that fails to open is a BADGE, never a refused meeting.
#[test]
fn a_tap_that_cannot_open_still_records_the_mic() {
    let _turnstile = meeting::session_turnstile();
    let dir = support::temp_dir("yv110-tap-fails");

    // What `SessionEngine::start` does with a `TapError`: the plan degrades to
    // mic-only carrying the failure sentence, and the meeting goes ahead.
    let mut platform = tap::FakePlatform::failing_with_status("create_aggregate");
    let error = open_tap(&mut platform, Some(7), "uid", "name").expect_err("aggregate fails");
    assert!(
        !platform.teardown_calls().is_empty(),
        "a half-built tap is torn down by `open_tap` itself: {error}"
    );

    let plan = TrackBPlan::MicOnly {
        badge: syscapture::SETUP_FAILED_MESSAGE,
    };
    let capture = start_capture_session(&dir, RATE, 1, plan, None, None, external())
        .expect("the meeting starts anyway");
    feed_mic(10);
    let outcome = capture.stop().expect("the meeting finalizes");
    assert!(outcome.wav_path.is_some());
    assert_eq!(
        outcome.note.as_deref(),
        Some(syscapture::SETUP_FAILED_MESSAGE),
        "a CoreAudio failure is a bug report, not a denial — the two must not read the same"
    );
}

/// Track 1 is stamped with the TAP's format, not the mic's.
///
/// `MeetingCapture::with_tracks` starts every track on the mic's rate because
/// that is the only one known at `start`; the tap's own
/// (`kAudioTapPropertyFormat`) is not knowable until the tap exists. If the
/// wiring forgets to announce it, nothing fails — track 1 simply resamples from
/// a rate it was never captured at, silently, at the wrong speed.
#[test]
fn track_b_is_retuned_to_the_taps_own_format() {
    let _turnstile = meeting::session_turnstile();
    let dir = support::temp_dir("yv110-retune");
    // A tap running at 44.1 kHz stereo against a 48 kHz mono mic — the exact
    // disagreement `capture_matches_format`'s doc comment reproduces.
    let mut platform = tap::FakePlatform {
        format: syscapture::TapFormat {
            sample_rate: 44_100,
            channels: 2,
        },
        ..tap::FakePlatform::default()
    };
    let open = open_tap(&mut platform, Some(7), "uid", "name").expect("opens");
    let ring = platform.bound_capture.clone().expect("bound");
    assert_eq!(ring.sample_rate(), 44_100);
    assert_eq!(ring.channels(), 2);
    let platform = Arc::new(Mutex::new(platform));
    let teardown_platform = Arc::clone(&platform);
    let mut resources: TapResources = open.resources;
    let meeting_tap = MeetingTap::new(ring, open.format, move || {
        teardown(
            &mut *teardown_platform.lock().expect("fake platform"),
            &mut resources,
        );
    });

    let capture = start_capture_session(
        &dir,
        48_000,
        1,
        TrackBPlan::Attach,
        Some(Arc::clone(&meeting_tap)),
        None,
        external(),
    )
    .expect("starts");
    let active = meeting::active_capture().expect("a meeting is recording");
    assert_eq!(
        active.track_native_rate(SYSTEM_TRACK),
        44_100,
        "track 1 must carry the tap's rate the moment the session exists — before its first block"
    );
    assert_eq!(
        active.track_native_rate(MIC_TRACK),
        48_000,
        "and announcing the tap's rate must not move the mic's"
    );
    drop(active);
    let _ = capture.stop();
}
