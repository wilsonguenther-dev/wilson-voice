//! YV95 / OS-12 fix (2) — the pill's hover watch must not invert the YV81
//! energy design once the pill is visible for three hours instead of three
//! seconds.
//!
//! The backlog's stated acceptance command: hover polling stays at
//! `HOVER_IDLE_TICK_MS` while the pill is visible-but-unhovered during an active
//! recording, not `HOVER_TICK_MS`.
//!
//! This drives the policy function the watch thread actually sleeps on. The
//! alternative — timing the thread from outside — would measure the scheduler,
//! take seconds per case, and be flaky on a loaded CI box, while proving less:
//! `start_hover_watch` calls `hover_tick_ms(is_shown(), is_hovered(),
//! meeting_recording(), dictating())` and nothing else decides the cadence.
//!
//! ## What round 3 added, and why
//!
//! The first version of this policy took three inputs, and the review found two
//! defects that no test in this file could have caught, because neither
//! behaviour had an input:
//!
//! 1. **It starved dictation.** `hover_tick_ms(shown, hovered, meeting)` has no
//!    way to say "a take is live", so a dictation taken DURING a meeting polled
//!    at 1 Hz — and the pill is that take's primary control.
//! 2. **It swallowed the first click on the pill.** The panel is click-through
//!    until this watch notices the cursor, so a slower notice is not latency: it
//!    is a click delivered to the app underneath.
//!
//! Both are regression-guarded below: `a_dictation_inside_a_meeting_keeps_the_fast_tick`
//! and the `sleep_slices` / `should_wake_early` group, which pin the mechanism
//! that lets a 1 Hz main-thread cadence coexist with a pill that is reachable
//! within one slice.

use wilson_voice_lib::float_pill::{
    capsule_screen_rect, dictating, hover_tick_ms, meeting_recording, set_dictating,
    set_meeting_recording, should_wake_early, sleep_slices, HOVER_IDLE_TICK_MS, HOVER_TICK_MS,
    PROXIMITY_SLICE_MS,
};

/// The line from the backlog, verbatim.
#[test]
fn hover_polling_stays_at_the_idle_tick_while_a_meeting_records() {
    assert_eq!(
        hover_tick_ms(true, false, true, false),
        HOVER_IDLE_TICK_MS,
        "a visible, untouched pill during a meeting must poll at 1 Hz"
    );
}

/// …and steps up to 75 ms only after the first mouse-enter, which is the other
/// half of the fix: the pill stays grabbable for anybody actually reaching for
/// it.
#[test]
fn the_first_mouse_enter_steps_the_tick_back_up() {
    assert_eq!(hover_tick_ms(true, true, true, false), HOVER_TICK_MS);
    // And back down when the cursor leaves — otherwise one hover at minute two
    // would cost 13 Hz for the remaining 178 minutes.
    assert_eq!(hover_tick_ms(true, false, true, false), HOVER_IDLE_TICK_MS);
}

/// Dictation is deliberately unchanged. A take lasts seconds and the pill is its
/// primary control; slowing that down would be a regression sold as a fix.
#[test]
fn dictation_keeps_the_fast_tick() {
    assert_eq!(
        hover_tick_ms(true, false, false, true),
        HOVER_TICK_MS,
        "a visible pill outside a meeting must stay at the responsive tick"
    );
    assert_eq!(hover_tick_ms(true, true, false, true), HOVER_TICK_MS);
}

/// **Regression guard — BLOCKING finding 1 of the final review.**
///
/// Dictating during a meeting is a designed case, not an edge one: YV91 fans the
/// same captured block out to both consumers, and `live.ts`'s `framePlan` puts
/// the live-take branch ABOVE the meeting park for exactly this reason. The Rust
/// policy did not have the matching ordering, so the pill — the take's primary
/// control — dropped to 1 Hz for the length of every sentence spoken during a
/// meeting.
///
/// Falsify by moving the `dictating` branch below the `meeting_recording` one in
/// `float_pill::hover_tick_ms`: this test then reports `1000`.
#[test]
fn a_dictation_inside_a_meeting_keeps_the_fast_tick() {
    assert_eq!(
        hover_tick_ms(true, false, true, true),
        HOVER_TICK_MS,
        "a take taken during a meeting must not inherit the meeting's 1 Hz cadence — \
         the pill is that take's primary control"
    );
    // Hovered or not: the take is what decides, and it decides FIRST.
    assert_eq!(hover_tick_ms(true, true, true, true), HOVER_TICK_MS);
}

/// The dictation flag is a real bit `lib.rs` writes, not an argument only tests
/// pass. Same standard `the_recording_flag_is_the_one_the_watch_thread_reads`
/// holds the meeting flag to.
#[test]
fn the_dictation_flag_is_the_one_the_watch_thread_reads() {
    set_meeting_recording(true);
    set_dictating(true);
    assert!(dictating());
    assert_eq!(
        hover_tick_ms(true, false, meeting_recording(), dictating()),
        HOVER_TICK_MS
    );
    set_dictating(false);
    assert!(!dictating());
    assert_eq!(
        hover_tick_ms(true, false, meeting_recording(), dictating()),
        HOVER_IDLE_TICK_MS
    );
    set_meeting_recording(false);
}

/// YV81's original rule survives intact: a hidden pill costs the long tick, in
/// every combination — including with a take live, because a pill nobody can see
/// is not anybody's control.
#[test]
fn a_hidden_pill_never_polls_fast() {
    for hovered in [false, true] {
        for recording in [false, true] {
            for dictating in [false, true] {
                assert_eq!(
                    hover_tick_ms(false, hovered, recording, dictating),
                    HOVER_IDLE_TICK_MS,
                    "hidden pill polled fast (hovered={hovered} recording={recording} \
                     dictating={dictating})"
                );
            }
        }
    }
}

/// The 3-hour budget OS-12 actually objected to, as arithmetic. 180 minutes at
/// 13 Hz is ~144,000 cursor tests and the same number of main-thread hops; the
/// same 180 minutes under this policy is 10,800.
#[test]
fn a_three_hour_meeting_costs_an_order_of_magnitude_fewer_polls() {
    const MEETING_MS: u64 = 3 * 60 * 60 * 1000;
    let before = MEETING_MS / HOVER_TICK_MS;
    let after = MEETING_MS / hover_tick_ms(true, false, true, false);
    assert_eq!(before, 144_000);
    assert_eq!(after, 10_800);
    assert!(
        after * 10 <= before,
        "the fix must be an order of magnitude, not a rounding"
    );
}

/// The flag the watch thread reads is the flag the meeting controller's status
/// sink sets. Asserted through the real setter so the two cannot be different
/// bits.
#[test]
fn the_recording_flag_is_the_one_the_watch_thread_reads() {
    set_dictating(false);
    set_meeting_recording(true);
    assert!(meeting_recording());
    assert_eq!(
        hover_tick_ms(true, false, meeting_recording(), dictating()),
        HOVER_IDLE_TICK_MS
    );
    set_meeting_recording(false);
    assert!(!meeting_recording());
    assert_eq!(
        hover_tick_ms(true, false, meeting_recording(), dictating()),
        HOVER_TICK_MS
    );
}

// ───────── the mechanism that makes a slow tick safe (BLOCKING finding 2) ────
//
// The review's second blocking finding on this file's subject: the panel is
// click-through until the hover watch's MAIN-THREAD test says otherwise, so
// dropping that test to 1 Hz means the first click on the pill during a meeting
// can be delivered to the app underneath — which, during a meeting, is the
// meeting. "Noticed within a second" is not a second of latency; it is a lost
// click aimed at Yap that landed in Zoom.
//
// The fix keeps the main-thread cadence exactly where OS-12 wants it and stops
// making it the only thing that can wake the watch. The tests below pin both
// halves of that claim.

/// The waiting is sliced, and the slices still add up to the tick. If they did
/// not, the OS-12 defence would be false — the whole argument for fix (2) is
/// that the MAIN-THREAD test rate is unchanged.
#[test]
fn slicing_the_wait_does_not_change_the_tick() {
    for tick in [
        HOVER_TICK_MS,
        HOVER_IDLE_TICK_MS,
        1,
        30,
        31,
        59,
        60,
        61,
        2000,
    ] {
        let (sleeps, last) = sleep_slices(tick, PROXIMITY_SLICE_MS);
        assert!(sleeps >= 1, "tick={tick} produced no sleep at all");
        let total = (sleeps - 1) * PROXIMITY_SLICE_MS + last;
        assert_eq!(
            total, tick,
            "tick={tick} sliced into {sleeps} sleeps summing to {total}"
        );
    }
}

/// …and the 1 Hz meeting tick is checked often enough that a cursor arriving on
/// the capsule cannot sit unnoticed for anything like a human's click latency.
#[test]
fn a_meeting_tick_is_rechecked_far_faster_than_a_hand_can_click() {
    let (sleeps, _) = sleep_slices(HOVER_IDLE_TICK_MS, PROXIMITY_SLICE_MS);
    assert!(
        sleeps >= 30,
        "a 1 Hz tick must be re-checked tens of times, not once ({sleeps})"
    );
    // The worst case is one whole slice: the cursor lands on the capsule just
    // after a check. It must stay well under the ~150 ms floor on a deliberate
    // human click, or the panel is still click-through when the button goes
    // down and the click goes to the app underneath.
    let worst_case_notice_ms = PROXIMITY_SLICE_MS;
    assert!(
        worst_case_notice_ms <= 50,
        "worst-case notice is {worst_case_notice_ms} ms"
    );
}

/// The early wake fires on the RISING edge only. Without that, a cursor resting
/// on the pill would break every slice and turn the 1 Hz meeting tick into a
/// 33 Hz one — worse than the bug this item exists to fix.
#[test]
fn the_early_wake_is_an_edge_not_a_level() {
    assert!(should_wake_early(false, true), "the cursor just arrived");
    assert!(
        !should_wake_early(true, true),
        "a cursor already on the capsule must not re-wake every slice"
    );
    assert!(!should_wake_early(true, false), "the cursor left");
    assert!(!should_wake_early(false, false), "nothing happened");
}

/// The capsule's screen rect, in AppKit's own coordinate system.
///
/// The proximity check compares `+[NSEvent mouseLocation]` (points, bottom-left
/// origin) against this, so the y flip is the whole of the conversion and it is
/// the one place it can be wrong. A 2× display 1000 pt tall, the float window
/// parked with its top-left 200 pt from the top of the screen, and a 200×40
/// capsule sitting 50 pt down inside it:
///
/// * the capsule's top edge is 250 pt from the top of the screen,
/// * its bottom edge is 290 pt from the top → 710 pt from the bottom,
/// * so AppKit's y for the rect's ORIGIN (its bottom-left) is 710.
#[test]
fn the_capsule_rect_converts_into_appkit_screen_points() {
    let rect = capsule_screen_rect(
        (600.0, 400.0), // physical px, top-left origin (2× display)
        2.0,
        1000.0,
        (30.0, 50.0, 200.0, 40.0),
    )
    .expect("a reported capsule has a screen rect");
    assert_eq!(rect, (330.0, 710.0, 200.0, 40.0));
}

/// Nothing reported yet is not a rect at the origin — that would make the whole
/// bottom-left corner of the screen "the pill".
#[test]
fn an_unreported_capsule_has_no_screen_rect() {
    assert!(capsule_screen_rect((0.0, 0.0), 2.0, 1000.0, (0.0, 0.0, 0.0, 0.0)).is_none());
    assert!(capsule_screen_rect((0.0, 0.0), 2.0, 1000.0, (10.0, 10.0, 200.0, 0.0)).is_none());
    // A scale factor of zero is a monitor query that failed, not a 1× display.
    assert!(capsule_screen_rect((0.0, 0.0), 0.0, 1000.0, (10.0, 10.0, 200.0, 40.0)).is_none());
}
