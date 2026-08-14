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
//! meeting_recording())` and nothing else decides the cadence.

use wilson_voice_lib::float_pill::{
    hover_tick_ms, meeting_recording, set_meeting_recording, HOVER_IDLE_TICK_MS, HOVER_TICK_MS,
};

/// The line from the backlog, verbatim.
#[test]
fn hover_polling_stays_at_the_idle_tick_while_a_meeting_records() {
    assert_eq!(
        hover_tick_ms(true, false, true),
        HOVER_IDLE_TICK_MS,
        "a visible, untouched pill during a meeting must poll at 1 Hz"
    );
}

/// …and steps up to 75 ms only after the first mouse-enter, which is the other
/// half of the fix: the pill stays grabbable for anybody actually reaching for
/// it.
#[test]
fn the_first_mouse_enter_steps_the_tick_back_up() {
    assert_eq!(hover_tick_ms(true, true, true), HOVER_TICK_MS);
    // And back down when the cursor leaves — otherwise one hover at minute two
    // would cost 13 Hz for the remaining 178 minutes.
    assert_eq!(hover_tick_ms(true, false, true), HOVER_IDLE_TICK_MS);
}

/// Dictation is deliberately unchanged. A take lasts seconds and the pill is its
/// primary control; slowing that down would be a regression sold as a fix.
#[test]
fn dictation_keeps_the_fast_tick() {
    assert_eq!(
        hover_tick_ms(true, false, false),
        HOVER_TICK_MS,
        "a visible pill outside a meeting must stay at the responsive tick"
    );
    assert_eq!(hover_tick_ms(true, true, false), HOVER_TICK_MS);
}

/// YV81's original rule survives intact: a hidden pill costs the long tick, in
/// every combination.
#[test]
fn a_hidden_pill_never_polls_fast() {
    for hovered in [false, true] {
        for recording in [false, true] {
            assert_eq!(
                hover_tick_ms(false, hovered, recording),
                HOVER_IDLE_TICK_MS,
                "hidden pill polled fast (hovered={hovered} recording={recording})"
            );
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
    let after = MEETING_MS / hover_tick_ms(true, false, true);
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
    assert!(
        !meeting_recording(),
        "no meeting is running in a fresh test"
    );
    set_meeting_recording(true);
    assert!(meeting_recording());
    assert_eq!(
        hover_tick_ms(true, false, meeting_recording()),
        HOVER_IDLE_TICK_MS
    );
    set_meeting_recording(false);
    assert!(!meeting_recording());
    assert_eq!(
        hover_tick_ms(true, false, meeting_recording()),
        HOVER_TICK_MS
    );
}
