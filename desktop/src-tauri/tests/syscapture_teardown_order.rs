//! YV100 acceptance — **the teardown order, on every exit path, including a
//! panic mid-setup**, with zero audio hardware.
//!
//! `AudioDeviceStop` → `AudioDeviceDestroyIOProcID` →
//! `AudioHardwareDestroyAggregateDevice` → `AudioHardwareDestroyProcessTap`.
//!
//! Two reasons this order is load-bearing rather than tidy.
//!
//! **It is what YV104's rebuild is made of.** Apple's own guidance for the
//! ghost-tap bug is that restarting the IOProc alone, or recreating only the
//! aggregate device, is not reliable — *both* the process tap and the aggregate
//! device must be destroyed and recreated. The 7-step rebuild is this sequence
//! followed by the setup sequence, so it is written once and asserted here.
//!
//! **A half-built tap outlives the app.** A process tap and an aggregate device
//! are owned by `coreaudiod`, not by our address space. Leak one and the user
//! keeps a private aggregate holding their output device until they log out. So
//! the interesting cases are not the happy path — they are the five ways setup
//! can stop early, and the one where it unwinds.

use wilson_voice_lib::syscapture::{
    full_rebuild_sequence, open_tap, teardown, TapError, TapResources, TapStage, TapStep,
    TEARDOWN_ORDER,
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

/// The teardown calls, mapped onto the canonical step names.
fn steps(platform: &FakePlatform) -> Vec<TapStep> {
    platform
        .teardown_calls()
        .into_iter()
        .map(|c| match c {
            Call::Stop => TapStep::AudioDeviceStop,
            Call::DestroyIoProc => TapStep::AudioDeviceDestroyIOProcID,
            Call::DestroyAggregate => TapStep::AudioHardwareDestroyAggregateDevice,
            Call::DestroyTap => TapStep::AudioHardwareDestroyProcessTap,
            other => panic!("{other:?} is not a teardown call"),
        })
        .collect()
}

/// Is `steps` a subsequence of the canonical order? Subsequence, not prefix:
/// a failure before the IOProc exists skips `Stop` and `DestroyIOProcID` and
/// still has to destroy the aggregate before the tap.
fn is_canonical_subsequence(steps: &[TapStep]) -> bool {
    let mut canonical = TEARDOWN_ORDER.iter();
    steps
        .iter()
        .all(|step| canonical.any(|expected| expected == step))
}

#[test]
fn the_happy_path_calls_setup_in_the_documented_order() {
    let mut platform = FakePlatform::default();
    let open = open(&mut platform).expect("the fake platform succeeds at every step");
    assert_eq!(
        platform.calls,
        vec![
            Call::CreateTap {
                excluded: vec![4242]
            },
            Call::TapUid,
            Call::TapFormat,
            // Between reading the format and using it: the ring is built from
            // it here, and nowhere else.
            Call::BindCapture,
            Call::DefaultOutputUid,
            Call::CreateAggregate,
            Call::CreateIoProc,
            Call::Start,
        ],
    );
    // Nothing is torn down on the way up.
    assert!(platform.teardown_calls().is_empty());
    assert_eq!(open.resources.tap, Some(9001));
    assert_eq!(open.resources.aggregate, Some(9002));
    assert!(open.resources.running);
    assert_eq!(open.format.sample_rate, 48_000);
}

#[test]
fn a_running_tap_tears_down_all_four_steps_in_order() {
    let mut platform = FakePlatform::default();
    let open = open(&mut platform).expect("setup succeeds");
    let mut resources = open.resources;
    let reported = teardown(&mut platform, &mut resources);
    assert_eq!(reported, TEARDOWN_ORDER.to_vec());
    assert_eq!(steps(&platform), TEARDOWN_ORDER.to_vec());
    assert!(
        resources.is_empty(),
        "every handle is cleared, so a second teardown is a no-op rather than a double-free"
    );
    // And it IS a no-op the second time.
    assert!(teardown(&mut platform, &mut resources).is_empty());
}

#[test]
fn every_status_failure_tears_down_exactly_what_existed_at_that_moment() {
    // step → the teardown steps that must fire, and only those.
    let cases: Vec<(&'static str, TapStage, Vec<TapStep>)> = vec![
        ("create_tap", TapStage::CreateTap, vec![]),
        (
            "tap_uid",
            TapStage::ReadTapUid,
            vec![TapStep::AudioHardwareDestroyProcessTap],
        ),
        (
            "tap_format",
            TapStage::ReadTapFormat,
            vec![TapStep::AudioHardwareDestroyProcessTap],
        ),
        (
            "default_output_uid",
            TapStage::ResolveDefaultOutput,
            vec![TapStep::AudioHardwareDestroyProcessTap],
        ),
        (
            "create_aggregate",
            TapStage::CreateAggregate,
            vec![TapStep::AudioHardwareDestroyProcessTap],
        ),
        (
            "create_ioproc",
            TapStage::CreateIoProc,
            vec![
                TapStep::AudioHardwareDestroyAggregateDevice,
                TapStep::AudioHardwareDestroyProcessTap,
            ],
        ),
        (
            // The one that matters most: the IOProc exists but was never
            // started, so `AudioDeviceStop` must NOT fire while the other three
            // must.
            "start",
            TapStage::Start,
            vec![
                TapStep::AudioDeviceDestroyIOProcID,
                TapStep::AudioHardwareDestroyAggregateDevice,
                TapStep::AudioHardwareDestroyProcessTap,
            ],
        ),
    ];

    for (step, stage, expected) in cases {
        let mut platform = FakePlatform::failing_with_status(step);
        let err = open(&mut platform).expect_err("this step was rigged to fail");
        assert_eq!(
            err,
            TapError::Os { stage, status: -4 },
            "the error names the call that failed, for {step}"
        );
        let taken = steps(&platform);
        assert_eq!(taken, expected, "teardown after {step} failed");
        assert!(
            is_canonical_subsequence(&taken),
            "teardown after {step} was not in canonical order: {taken:?}"
        );
    }
}

#[test]
fn a_panic_mid_setup_still_tears_down_in_order_and_does_not_abort() {
    // A panic is not a `Result`, and the FFI boundary this runs behind is
    // `extern "C-unwind"` — an unwind past it is undefined behaviour. So the
    // setup catches its own unwind, tears down what it had built, and reports.
    // Without this, a panic between "aggregate created" and "IOProc started"
    // leaks a private aggregate device holding the user's speakers.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    for (step, stage, expected) in [
        (
            "create_aggregate",
            TapStage::CreateAggregate,
            vec![TapStep::AudioHardwareDestroyProcessTap],
        ),
        (
            "create_ioproc",
            TapStage::CreateIoProc,
            vec![
                TapStep::AudioHardwareDestroyAggregateDevice,
                TapStep::AudioHardwareDestroyProcessTap,
            ],
        ),
        (
            "start",
            TapStage::Start,
            vec![
                TapStep::AudioDeviceDestroyIOProcID,
                TapStep::AudioHardwareDestroyAggregateDevice,
                TapStep::AudioHardwareDestroyProcessTap,
            ],
        ),
    ] {
        let mut platform = FakePlatform::panicking_at(step);
        let err = open(&mut platform).expect_err("this step was rigged to panic");
        assert_eq!(err, TapError::PanicDuringSetup { stage });
        let taken = steps(&platform);
        assert_eq!(taken, expected, "teardown after a panic in {step}");
        assert!(is_canonical_subsequence(&taken));
    }

    std::panic::set_hook(previous);
    // The process is still here, which is the other half of the claim.
}

#[test]
fn a_tap_is_never_built_without_an_exclusion_and_no_call_is_made_when_it_cannot_be() {
    // The refusal happens BEFORE `AudioHardwareCreateProcessTap`, so there is
    // nothing to tear down — and, crucially, no global tap of everything
    // including Yap's own output was ever created.
    let mut platform = FakePlatform::default();
    let err = open_tap(&mut platform, None, "uid", "name")
        .expect_err("no self process object means no tap");
    assert_eq!(err, TapError::SelfProcessObjectUnavailable);
    assert!(
        platform.calls.is_empty(),
        "not one CoreAudio call may be made without an exclusion list"
    );
}

#[test]
fn the_exclusion_list_that_reaches_coreaudio_is_exactly_us() {
    let mut platform = FakePlatform::default();
    open_tap(&mut platform, Some(77), "uid", "name").expect("setup succeeds");
    assert_eq!(
        platform.calls.first(),
        Some(&Call::CreateTap { excluded: vec![77] }),
        "global-exclude-self means ONE exclusion — never an empty list, never a target"
    );
}

#[test]
fn the_composition_dictionary_reaches_the_platform_intact() {
    // The pure builder is tested next door; this proves the state machine hands
    // that exact dictionary to `AudioHardwareCreateAggregateDevice` rather than
    // rebuilding a second one on the way.
    let mut platform = FakePlatform::default();
    open(&mut platform).expect("setup succeeds");
    let description = platform
        .description
        .as_ref()
        .expect("create_aggregate saw a description");
    assert_eq!(
        description,
        &wilson_voice_lib::syscapture::aggregate_description(
            &wilson_voice_lib::syscapture::AggregateSpec {
                aggregate_uid: "uid.under.test".to_string(),
                aggregate_name: "Yap meeting capture".to_string(),
                output_uid: "BuiltInSpeakerDevice".to_string(),
                tap_uid: "11111111-2222-3333-4444-555555555555".to_string(),
            }
        )
    );
}

#[test]
fn an_empty_resource_set_tears_nothing_down() {
    let mut platform = FakePlatform::default();
    let mut nothing = TapResources::default();
    assert!(teardown(&mut platform, &mut nothing).is_empty());
    assert!(platform.calls.is_empty());
}

#[test]
fn a_virtual_meeting_runs_two_tracks_on_the_external_stream() {
    // The 22-A seams this item reuses rather than forks: `tracks: 2` is the
    // generalization `MeetingJournal::start(dir, tracks)` was built for, and
    // `ExternalStream` is the no-cpal-stream-to-hold source whose own doc
    // comment names 22-B's tap as its second user.
    let dir = std::env::temp_dir().join("yv100-virtual-meeting");
    let config = wilson_voice_lib::syscapture::virtual_meeting_config(
        &dir,
        wilson_voice_lib::syscapture::TapFormat {
            sample_rate: 44_100,
            channels: 1,
        },
    );
    assert_eq!(config.tracks, 2);
    assert_eq!(config.native_rate, 44_100);
    assert_eq!(config.channels, 1);
    // `hold` on a tap-backed session must be a no-op that succeeds: there is no
    // cpal stream, and a session that refuses to start because it cannot hold
    // one would be a meeting that never records.
    assert!(config.stream.hold().is_ok());
    config.stream.release();
}

/// THE REGRESSION GUARD FOR THE REBASE THAT PRODUCED THIS FILE.
///
/// This item and YV104 both describe the same seven CoreAudio calls, and YV104
/// merged to `main` first while this one was in review. `full_rebuild_sequence`
/// exists precisely so there is one declaration of that order, and its own doc
/// comment says so: *"YV100 binds its FFI to this list instead of writing a
/// second copy that can drift from it."*
///
/// The rebase that brought this branch onto that `main` was **textually clean**
/// and still wrong: this file's `teardown` had grown a private four-variant
/// `TeardownStep` enum with its own literal order, so `git` saw two unrelated
/// additions and merged both, and the codebase would have shipped exactly the
/// second copy the comment warns about — silently, with every test in this file
/// green, because they all asserted against the copy.
///
/// So the two are now the same list by construction (`TEARDOWN_ORDER` slices
/// `full_rebuild_sequence()`), and this test is the standing proof of it: it
/// fails if either order is edited without the other, if the teardown half
/// stops being a prefix of the rebuild, or if a fifth teardown step is invented
/// somewhere that the rebuild sequence does not know about.
#[test]
fn the_teardown_order_is_the_first_four_steps_of_the_one_declared_rebuild() {
    let rebuild = full_rebuild_sequence();

    // Not "equal to a list I typed here" — equal to the *prefix of the other
    // declaration*, which is the property that can actually drift.
    assert_eq!(
        TEARDOWN_ORDER.as_slice(),
        &rebuild[..4],
        "TEARDOWN_ORDER must BE the first four steps of full_rebuild_sequence(), \
         not a second copy of them"
    );

    // And the split is the semantic one, not a lucky index: every step in the
    // teardown half reports itself as a teardown step, and none of the three
    // create steps does. A reorder that kept the count but moved a create step
    // into the first four passes the slice check above and fails here.
    assert!(
        TEARDOWN_ORDER.iter().all(|step| step.is_teardown()),
        "every step of TEARDOWN_ORDER must be a teardown step: {TEARDOWN_ORDER:?}"
    );
    assert!(
        rebuild[4..].iter().all(|step| !step.is_teardown()),
        "the rebuild half must contain no teardown step: {:?}",
        &rebuild[4..]
    );
    assert_eq!(
        rebuild.iter().filter(|step| step.is_teardown()).count(),
        TEARDOWN_ORDER.len(),
        "the rebuild sequence must contain exactly the teardown steps this \
         module tears down — no more, no fewer"
    );

    // The names are CoreAudio's own, checked once here so a rename cannot
    // quietly turn the shared list into a list of something else.
    assert_eq!(
        TEARDOWN_ORDER.map(TapStep::as_str),
        [
            "AudioDeviceStop",
            "AudioDeviceDestroyIOProcID",
            "AudioHardwareDestroyAggregateDevice",
            "AudioHardwareDestroyProcessTap",
        ]
    );
}
