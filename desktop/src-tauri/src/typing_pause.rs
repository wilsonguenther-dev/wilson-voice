//! yap-1106-fb-04 — Paste waits for a typing pause ("Pastes when you pause").
//!
//! A dictation take can finish while the user is already typing in the target
//! app again. A ⌘V landing mid-word splices the transcript into whatever they
//! were typing. So: if a key went down in the last [`RECENT_TYPING_MS`] when
//! the transcript is ready, the paste is HELD — the pill says
//! [`PASTE_WAIT_LINE`] — until [`PAUSE_MS`] pass with no key-down, or
//! [`MAX_HOLD_MS`] at most. An app switch during the hold ends it at once and
//! the take takes the existing wrong-target branch in `paste::insert_traced`
//! (copied, held on the pill), exactly as before this module.
//!
//! PRIVACY: the only thing observed is WHEN a key went down. The fn tap
//! (`ptt_macos::tap_callback`) calls [`note_key_down`] with no argument: no
//! keycode, no character, no app. The hold logs a key COUNT, never keys.
//!
//! Every clock read goes through [`KeyClock`], so the tests drive a fake key
//! clock (typing that continues, a pause, the 5 s cap, an app switch) with no
//! real sleeps.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

/// A key-down this recent when the transcript is ready holds the paste.
pub const RECENT_TYPING_MS: u64 = 1_500;
/// The pause that releases a held paste: this long with no key-down.
pub const PAUSE_MS: u64 = 600;
/// A held paste never waits longer than this.
pub const MAX_HOLD_MS: u64 = 5_000;
/// How often a hold re-checks the clock and the frontmost app.
pub const POLL_MS: u64 = 100;
/// The pill line while a paste is held. Mirrored by `PASTE_WAIT_LINE` in
/// `desktop/src/pill/live.ts` (held equal by `paste_wait_line_matches_the_pill`).
pub const PASTE_WAIT_LINE: &str = "Pastes when you pause";

static EPOCH: OnceLock<Instant> = OnceLock::new();
/// Milliseconds since [`EPOCH`] of the newest key-down, plus one (0 = none yet).
static LAST_KEY_DOWN: AtomicU64 = AtomicU64::new(0);
/// Key-downs seen since launch — a count, never which keys.
static KEY_DOWNS: AtomicU64 = AtomicU64::new(0);

fn mono_ms() -> u64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Called from the fn tap on a non-modifier key-down. Records the time only.
pub fn note_key_down() {
    LAST_KEY_DOWN.store(mono_ms() + 1, Ordering::Relaxed);
    KEY_DOWNS.fetch_add(1, Ordering::Relaxed);
}

/// Everything the hold reads about time and typing.
pub trait KeyClock {
    /// Monotonic milliseconds.
    fn now_ms(&self) -> u64;
    /// When the newest key-down happened (same timebase), if any.
    fn last_key_down_ms(&self) -> Option<u64>;
    /// Key-downs so far (a count only).
    fn key_downs(&self) -> u64;
    /// Wait this long.
    fn sleep_ms(&mut self, ms: u64);
}

/// The real clock: the tap's timestamps and a real thread sleep.
pub struct LiveKeyClock;

impl KeyClock for LiveKeyClock {
    fn now_ms(&self) -> u64 {
        mono_ms()
    }
    fn last_key_down_ms(&self) -> Option<u64> {
        match LAST_KEY_DOWN.load(Ordering::Relaxed) {
            0 => None,
            t => Some(t - 1),
        }
    }
    fn key_downs(&self) -> u64 {
        KEY_DOWNS.load(Ordering::Relaxed)
    }
    fn sleep_ms(&mut self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
}

/// How a take's paste got past the typing check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseWait {
    /// No key-down in the last [`RECENT_TYPING_MS`]: pasted at once, no hold.
    NotTyping,
    /// Held, then [`PAUSE_MS`] passed with no key-down.
    Paused { waited_ms: u64, keys_during: u64 },
    /// Held, and the user kept typing until [`MAX_HOLD_MS`].
    Capped { waited_ms: u64, keys_during: u64 },
    /// Held, and the target app left the front: the existing copy branch runs.
    AppSwitched { waited_ms: u64, keys_during: u64 },
    /// fix-l3b3-1 — held, and the take stopped being wanted mid-hold (the
    /// user cancelled it, or a newer take made it stale). The caller decides
    /// which from [`TakeWanted`]: a cancel pastes and copies NOTHING (Y3-D), a
    /// stale take is copy-only.
    Cancelled { waited_ms: u64, keys_during: u64 },
}

/// fix-l3b3-1 — is the take still wanted at this instant? Read by the hold
/// every poll step, so a cancel pressed while the pill says
/// [`PASTE_WAIT_LINE`] is honoured within one [`POLL_MS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakeWanted {
    /// Neither cancelled nor superseded: the hold carries on.
    Wanted,
    /// Y3-D — the user cancelled the take: nothing is pasted, nothing copied.
    Cancelled,
    /// The paste generation moved on (a newer take): copy-only, never pasted.
    Stale,
}

impl PauseWait {
    pub fn as_str(&self) -> &'static str {
        match self {
            PauseWait::NotTyping => "not_typing",
            PauseWait::Paused { .. } => "paused",
            PauseWait::Capped { .. } => "capped",
            PauseWait::AppSwitched { .. } => "app_switched",
            PauseWait::Cancelled { .. } => "cancelled",
        }
    }

    /// Whether the paste was held at all.
    pub fn held(&self) -> bool {
        !matches!(self, PauseWait::NotTyping)
    }

    pub fn waited_ms(&self) -> u64 {
        match *self {
            PauseWait::NotTyping => 0,
            PauseWait::Paused { waited_ms, .. }
            | PauseWait::Capped { waited_ms, .. }
            | PauseWait::AppSwitched { waited_ms, .. }
            | PauseWait::Cancelled { waited_ms, .. } => waited_ms,
        }
    }

    pub fn keys_during(&self) -> u64 {
        match *self {
            PauseWait::NotTyping => 0,
            PauseWait::Paused { keys_during, .. }
            | PauseWait::Capped { keys_during, .. }
            | PauseWait::AppSwitched { keys_during, .. }
            | PauseWait::Cancelled { keys_during, .. } => keys_during,
        }
    }

    /// The `paste typing_pause` log line (no text, no keys, no app).
    pub fn log_line(&self) -> String {
        format!(
            "paste typing_pause outcome={} waited_ms={} keys_during={}",
            self.as_str(),
            self.waited_ms(),
            self.keys_during()
        )
    }
}

/// Did a key go down within [`RECENT_TYPING_MS`] of `now`?
pub fn typed_recently(now_ms: u64, last_key_down_ms: Option<u64>) -> bool {
    matches!(last_key_down_ms, Some(t) if now_ms.saturating_sub(t) < RECENT_TYPING_MS)
}

/// Hold the paste while the user is typing.
///
/// - No key-down in the last [`RECENT_TYPING_MS`] → [`PauseWait::NotTyping`],
///   `on_hold` is never called and nothing waits.
/// - Otherwise `on_hold` runs once (the pill shows [`PASTE_WAIT_LINE`]) and the
///   hold lasts until [`PAUSE_MS`] with no key-down — counted from the later of
///   the newest key-down and the start of the hold, so a held paste always
///   waits at least one full pause — or [`MAX_HOLD_MS`] at most.
/// - `still_target` is checked before every wait step; `false` (the user
///   switched apps) ends the hold at once with [`PauseWait::AppSwitched`].
/// - fix-l3b3-1: `still_wanted` is checked first, every step; `false` (the take
///   was cancelled, or a newer take made it stale) ends the hold at once with
///   [`PauseWait::Cancelled`] — so a cancel is honoured within one [`POLL_MS`].
pub fn wait_for_pause(
    clock: &mut dyn KeyClock,
    still_wanted: &mut dyn FnMut() -> bool,
    still_target: &mut dyn FnMut() -> bool,
    on_hold: &mut dyn FnMut(),
) -> PauseWait {
    let start = clock.now_ms();
    if !typed_recently(start, clock.last_key_down_ms()) {
        return PauseWait::NotTyping;
    }
    let keys_at_start = clock.key_downs();
    on_hold();
    loop {
        let now = clock.now_ms();
        let waited_ms = now.saturating_sub(start);
        let keys_during = clock.key_downs().saturating_sub(keys_at_start);
        if !still_wanted() {
            return PauseWait::Cancelled {
                waited_ms,
                keys_during,
            };
        }
        if !still_target() {
            return PauseWait::AppSwitched {
                waited_ms,
                keys_during,
            };
        }
        let quiet_since = clock.last_key_down_ms().unwrap_or(start).max(start);
        let quiet_ms = now.saturating_sub(quiet_since);
        if quiet_ms >= PAUSE_MS {
            return PauseWait::Paused {
                waited_ms,
                keys_during,
            };
        }
        if waited_ms >= MAX_HOLD_MS {
            return PauseWait::Capped {
                waited_ms,
                keys_during,
            };
        }
        let step = (PAUSE_MS - quiet_ms)
            .min(MAX_HOLD_MS - waited_ms)
            .clamp(1, POLL_MS);
        clock.sleep_ms(step);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::Cell;
    use std::rc::Rc;

    /// The fake key clock: `now` moves only when the hold sleeps, and the
    /// scripted key-downs "happen" when `now` passes them. `now` is shared so
    /// the fake frontmost-app check can read the same time.
    struct FakeKeyClock {
        now: Rc<Cell<u64>>,
        keys: Vec<u64>,
        slept: u64,
    }

    impl FakeKeyClock {
        fn new(now: u64, keys: Vec<u64>) -> Self {
            FakeKeyClock {
                now: Rc::new(Cell::new(now)),
                keys,
                slept: 0,
            }
        }
    }

    impl KeyClock for FakeKeyClock {
        fn now_ms(&self) -> u64 {
            self.now.get()
        }
        fn last_key_down_ms(&self) -> Option<u64> {
            let now = self.now.get();
            self.keys.iter().copied().filter(|&t| t <= now).max()
        }
        fn key_downs(&self) -> u64 {
            let now = self.now.get();
            self.keys.iter().filter(|&&t| t <= now).count() as u64
        }
        fn sleep_ms(&mut self, ms: u64) {
            self.now.set(self.now.get() + ms);
            self.slept += ms;
        }
    }

    /// Run one hold; `switch_at` = when the target app leaves the front.
    fn run(clock: &mut FakeKeyClock, switch_at: Option<u64>) -> (PauseWait, u32) {
        run_with_cancel(clock, switch_at, None)
    }

    /// One hold where the take stops being wanted (`cancel_at`) as well.
    fn run_with_cancel(
        clock: &mut FakeKeyClock,
        switch_at: Option<u64>,
        cancel_at: Option<u64>,
    ) -> (PauseWait, u32) {
        let mut holds = 0u32;
        let now = Rc::clone(&clock.now);
        let mut still = move || switch_at.is_none_or(|s| now.get() < s);
        let now_w = Rc::clone(&clock.now);
        let mut wanted = move || cancel_at.is_none_or(|c| now_w.get() < c);
        let mut on_hold = || holds += 1;
        let wait = wait_for_pause(clock, &mut wanted, &mut still, &mut on_hold);
        (wait, holds)
    }

    /// fix-l3b3-1 — Escape pressed 300 ms into a hold while the user is still
    /// typing: the hold ends within one poll step as Cancelled, the pill's
    /// "Pastes when you pause" was shown once, and it never reaches the pause.
    #[test]
    fn a_cancel_mid_hold_ends_it_within_one_poll_step() {
        let keys: Vec<u64> = (0..20).map(|i| 9_900 + i * 200).collect();
        let mut c = FakeKeyClock::new(10_000, keys);
        let (w, holds) = run_with_cancel(&mut c, None, Some(10_300));
        assert_eq!(holds, 1, "on_hold ran once");
        match w {
            PauseWait::Cancelled { waited_ms, .. } => {
                assert!(waited_ms >= 300, "not before the cancel: {waited_ms}");
                assert!(
                    waited_ms <= 300 + POLL_MS,
                    "honoured within one POLL_MS: {waited_ms}"
                );
            }
            other => panic!("expected a cancel, got {other:?}"),
        }
        assert_eq!(w.as_str(), "cancelled");
        assert!(w.held());
        assert!(c.slept <= 300 + POLL_MS);
    }

    /// A cancel wins over an app switch at the same step: nothing is copied.
    #[test]
    fn a_cancel_wins_over_an_app_switch() {
        let keys: Vec<u64> = (0..20).map(|i| 9_900 + i * 200).collect();
        let mut c = FakeKeyClock::new(10_000, keys);
        let (w, _) = run_with_cancel(&mut c, Some(10_000), Some(10_000));
        assert_eq!(w.as_str(), "cancelled");
        assert_eq!(w.waited_ms(), 0);
    }

    /// No recent typing: the take was never held, so the cancel predicate is
    /// irrelevant (the caller's own Y3-D checkpoint already ran).
    #[test]
    fn no_hold_never_reads_the_cancel() {
        let mut c = FakeKeyClock::new(10_000, vec![1_000]);
        let (w, holds) = run_with_cancel(&mut c, None, Some(0));
        assert_eq!(w, PauseWait::NotTyping);
        assert_eq!(holds, 0);
    }

    /// The cancelled log line carries counts only.
    #[test]
    fn the_cancelled_log_line_carries_counts_only() {
        let line = PauseWait::Cancelled {
            waited_ms: 340,
            keys_during: 2,
        }
        .log_line();
        assert_eq!(
            line,
            "paste typing_pause outcome=cancelled waited_ms=340 keys_during=2"
        );
    }

    #[test]
    fn no_recent_typing_pastes_at_once() {
        // Last key 2 s before the transcript was ready, and never typed at all.
        let mut c = FakeKeyClock::new(10_000, vec![8_000]);
        let (w, holds) = run(&mut c, None);
        assert_eq!(w, PauseWait::NotTyping);
        assert_eq!(holds, 0, "no hold, so the pill never says the wait line");
        assert_eq!(c.slept, 0);
        let mut never = FakeKeyClock::new(10_000, vec![]);
        assert_eq!(run(&mut never, None).0, PauseWait::NotTyping);
    }

    #[test]
    fn a_pause_releases_the_paste_after_600_ms_quiet() {
        // Typed 200 ms before ready, then stopped.
        let mut c = FakeKeyClock::new(10_000, vec![9_800]);
        let (w, holds) = run(&mut c, None);
        assert_eq!(holds, 1, "held exactly once");
        assert_eq!(
            w,
            PauseWait::Paused {
                waited_ms: PAUSE_MS,
                keys_during: 0
            }
        );
        assert!(w.held());
    }

    #[test]
    fn typing_that_continues_holds_then_pauses_after_the_last_key() {
        // Keys at +200, +500, +900 into the hold, then quiet: pastes 600 ms
        // after the last key (at +1500), never before.
        let mut c = FakeKeyClock::new(10_000, vec![9_500, 10_200, 10_500, 10_900]);
        let (w, holds) = run(&mut c, None);
        assert_eq!(holds, 1);
        assert_eq!(
            w,
            PauseWait::Paused {
                waited_ms: 1_500,
                keys_during: 3
            }
        );
    }

    #[test]
    fn typing_that_never_pauses_hits_the_5_s_cap() {
        // A key every 300 ms for 8 s: never a 600 ms gap.
        let keys: Vec<u64> = (0..27).map(|i| 9_900 + i * 300).collect();
        let mut c = FakeKeyClock::new(10_000, keys);
        let (w, holds) = run(&mut c, None);
        assert_eq!(holds, 1);
        match w {
            PauseWait::Capped {
                waited_ms,
                keys_during,
            } => {
                assert_eq!(waited_ms, MAX_HOLD_MS, "never waits past the cap");
                assert!(keys_during > 10, "keys were counted: {keys_during}");
            }
            other => panic!("expected the cap, got {other:?}"),
        }
    }

    #[test]
    fn an_app_switch_ends_the_hold_for_the_copy_branch() {
        // Still typing, and the target app leaves the front 1 s into the hold.
        let keys: Vec<u64> = (0..20).map(|i| 9_900 + i * 300).collect();
        let mut c = FakeKeyClock::new(10_000, keys);
        let (w, holds) = run(&mut c, Some(11_000));
        assert_eq!(holds, 1);
        assert_eq!(w.as_str(), "app_switched");
        assert_eq!(w.waited_ms(), 1_000);
        assert!(w.waited_ms() < MAX_HOLD_MS);
    }

    #[test]
    fn the_log_line_carries_counts_never_keys() {
        let line = PauseWait::Capped {
            waited_ms: 5_000,
            keys_during: 17,
        }
        .log_line();
        assert_eq!(
            line,
            "paste typing_pause outcome=capped waited_ms=5000 keys_during=17"
        );
    }

    #[test]
    fn live_clock_records_time_and_a_count_only() {
        let before = LiveKeyClock.key_downs();
        note_key_down();
        let c = LiveKeyClock;
        assert!(c.key_downs() > before);
        let last = c.last_key_down_ms().expect("a key-down was noted");
        assert!(typed_recently(c.now_ms(), Some(last)));
    }

    #[test]
    fn paste_wait_line_matches_the_pill() {
        let live = include_str!("../../src/pill/live.ts");
        assert!(
            live.contains(&format!(
                "export const PASTE_WAIT_LINE = \"{}\";",
                PASTE_WAIT_LINE
            )),
            "live.ts PASTE_WAIT_LINE drifted from typing_pause.rs"
        );
    }
}
