//! yap-wf-01-click-the-pill-to — WF-01: click the pill to dictate.
//!
//! A click on the resting pill starts a hands-free take; a click while it is
//! listening stops it and the text pastes into the app that had focus BEFORE
//! the click. The float panel is non-activating, so in the normal case the
//! user's app never loses frontmost and nothing has to be done. This module is
//! the guard for the case where it did (Yap's main window was raised during the
//! take, or the panel took key): at every press on the pill the frontmost app
//! that is NOT Yap is latched, and the stop click hands focus back to it before
//! the stop edge reads the paste target.
//!
//! The decision is pure (`PreClickLatch`, `paste_target`) so the "paste target
//! is the pre-click app" rule is a unit test; only `frontmost` and `refocus`
//! touch AppKit.

/// How long a latched press stays authoritative. The click that consumes it
/// lands one pointer-up (< 300 ms, see `CLICK_MAX_MS` in pill/drag.ts) plus an
/// IPC hop after the press, so anything older is not this click's press.
pub const LATCH_FRESH_MS: u64 = 2_000;

/// How long the stop click waits for the refocused app to become frontmost
/// before it reads the paste target anyway.
pub const REFOCUS_WAIT_MS: u64 = 250;

/// The app that was frontmost, as AppKit names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontApp {
    pub pid: i32,
    pub bundle_id: Option<String>,
}

impl FrontApp {
    /// True when this is Yap itself (its own bundle id, or its own process).
    pub fn is_self(&self, own_bundle: &str, own_pid: i32) -> bool {
        self.pid == own_pid || self.bundle_id.as_deref() == Some(own_bundle)
    }
}

/// The last non-Yap app that was frontmost when the pill was pressed.
#[derive(Debug, Default, Clone)]
pub struct PreClickLatch {
    app: Option<FrontApp>,
    at_ms: u64,
}

impl PreClickLatch {
    pub const fn new() -> Self {
        Self {
            app: None,
            at_ms: 0,
        }
    }

    /// Record a press. A press while Yap itself is frontmost keeps the earlier
    /// latch (the app the user was in before Yap came forward), but refreshes
    /// its time so the click that follows can still use it.
    pub fn press(&mut self, front: Option<FrontApp>, own_bundle: &str, own_pid: i32, now_ms: u64) {
        match front {
            Some(f) if !f.is_self(own_bundle, own_pid) => {
                self.app = Some(f);
                self.at_ms = now_ms;
            }
            _ => {
                if self.app.is_some() {
                    self.at_ms = now_ms;
                }
            }
        }
    }

    /// The pre-click app for a click at `now_ms`, if the press is fresh.
    pub fn pre_click(&self, now_ms: u64) -> Option<&FrontApp> {
        self.app
            .as_ref()
            .filter(|_| now_ms.saturating_sub(self.at_ms) <= LATCH_FRESH_MS)
    }
}

/// THE latch: written at every press on the pill (`pill_drag_start`), read
/// by the click that follows it (`pill_click_toggle`).
pub static PRESS: parking_lot::Mutex<PreClickLatch> = parking_lot::Mutex::new(PreClickLatch::new());

/// What the stop click must do before the stop edge reads the paste target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteTarget {
    /// The frontmost app already is the pre-click app (the normal case: the
    /// panel is non-activating), or there is no pre-click app to return to.
    Keep,
    /// Hand focus back to this app first: it had focus before the click.
    Refocus(FrontApp),
}

/// The paste target is the PRE-CLICK app. If something else is frontmost now
/// (Yap itself, or nothing), focus goes back to the pre-click app first.
pub fn paste_target(pre_click: Option<&FrontApp>, front_now: Option<&FrontApp>) -> PasteTarget {
    match (pre_click, front_now) {
        (None, _) => PasteTarget::Keep,
        (Some(pre), Some(now)) if now.pid == pre.pid => PasteTarget::Keep,
        (Some(pre), _) => PasteTarget::Refocus(pre.clone()),
    }
}

/// The current frontmost app (NSWorkspace; needs no Accessibility grant).
pub fn frontmost() -> Option<FrontApp> {
    #[cfg(target_os = "macos")]
    {
        let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
        let app = workspace.frontmostApplication()?;
        let pid = app.processIdentifier();
        let bundle_id = app
            .bundleIdentifier()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());
        Some(FrontApp { pid, bundle_id })
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Ask AppKit to bring `target` forward. `false` when the app is gone or
/// refused the request.
pub fn refocus(target: &FrontApp) -> bool {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
        let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(target.pid)
        else {
            return false;
        };
        app.activateWithOptions(NSApplicationActivationOptions::empty())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = target;
        false
    }
}

/// Wait (bounded by `REFOCUS_WAIT_MS`) until `target` is frontmost. Returns
/// whether it got there. Never called on the main thread: activation lands
/// through the main run loop.
pub fn wait_until_front(target: &FrontApp) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(REFOCUS_WAIT_MS);
    loop {
        if frontmost().is_some_and(|f| f.pid == target.pid) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

// ── yap-1107-pill-click-always-registers: no click is lost while busy ───────

/// How long a click that arrived while the previous take was still finishing
/// (stop in flight, or the take decoding) waits to become the next start. A
/// finish normally clears in well under this; past it the user has moved on,
/// and a take opening the microphone by surprise is worse than a missed one,
/// so the queued click expires with its own log line (never silently).
pub const QUEUED_CLICK_TTL_MS: u64 = 10_000;
/// How often the waiter looks at "is the previous take done yet?".
pub const QUEUED_CLICK_POLL_MS: u64 = 20;

/// What a click did to the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueAction {
    /// The click is now the queued next start.
    Queued,
    /// A start was already queued: this click is its toggle partner, so the
    /// pair cancels (click to start, click again to stop = nothing). Keeps
    /// start/stop alternating exactly as the user clicked.
    Cancelled,
}

/// What the waiter should do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueDue {
    /// Nothing queued (cancelled, or another path consumed it).
    Nothing,
    /// Still busy; look again later.
    Wait,
    /// The previous take is done: start now. `waited_ms` is for the log.
    Start { waited_ms: u64 },
    /// The queued click outlived [`QUEUED_CLICK_TTL_MS`]; it is dropped with a log.
    Expired { waited_ms: u64 },
}

/// At most ONE queued click: the pill is a toggle, so two queued clicks are a
/// start and its stop, which cancel.
#[derive(Debug, Clone, Default)]
pub struct ClickQueue {
    at_ms: Option<u64>,
}

impl ClickQueue {
    pub const fn new() -> Self {
        Self { at_ms: None }
    }

    /// Is a click waiting to become the next start?
    pub fn pending(&self) -> bool {
        self.at_ms.is_some()
    }

    /// A click while busy: queue it, or cancel the one already queued.
    pub fn click(&mut self, now_ms: u64) -> QueueAction {
        if self.at_ms.take().is_some() {
            QueueAction::Cancelled
        } else {
            self.at_ms = Some(now_ms);
            QueueAction::Queued
        }
    }

    /// The waiter's question. `busy` = the previous take is still stopping or
    /// decoding. Consumes the queued click on Start and Expired.
    pub fn due(&mut self, now_ms: u64, busy: bool) -> QueueDue {
        let Some(at) = self.at_ms else {
            return QueueDue::Nothing;
        };
        let waited_ms = now_ms.saturating_sub(at);
        if waited_ms > QUEUED_CLICK_TTL_MS {
            self.at_ms = None;
            return QueueDue::Expired { waited_ms };
        }
        if busy {
            return QueueDue::Wait;
        }
        self.at_ms = None;
        QueueDue::Start { waited_ms }
    }
}

/// THE queue: written by `pill_click_toggle` when the previous take is still
/// finishing, drained by the one waiter thread it spawns.
pub static QUEUE: parking_lot::Mutex<ClickQueue> = parking_lot::Mutex::new(ClickQueue::new());

#[cfg(test)]
mod tests {
    use super::*;

    const YAP: &str = "com.wilsonguenther.yap.test";
    const YAP_PID: i32 = 100;

    fn app(pid: i32, id: &str) -> FrontApp {
        FrontApp {
            pid,
            bundle_id: Some(id.to_string()),
        }
    }

    #[test]
    fn paste_target_is_the_pre_click_app() {
        let mut latch = PreClickLatch::default();
        let notes = app(200, "com.apple.Notes");
        latch.press(Some(notes.clone()), YAP, YAP_PID, 1_000);
        // Yap came forward during the take; the stop click returns focus to Notes.
        let yap = app(YAP_PID, YAP);
        assert_eq!(
            paste_target(latch.pre_click(1_150), Some(&yap)),
            PasteTarget::Refocus(notes.clone())
        );
        // Nothing frontmost at all: still Notes.
        assert_eq!(
            paste_target(latch.pre_click(1_150), None),
            PasteTarget::Refocus(notes.clone())
        );
        // The normal case: the non-activating panel left Notes frontmost.
        assert_eq!(
            paste_target(latch.pre_click(1_150), Some(&notes)),
            PasteTarget::Keep
        );
    }

    #[test]
    fn a_press_while_yap_is_frontmost_keeps_the_pre_click_app() {
        let mut latch = PreClickLatch::default();
        let mail = app(300, "com.apple.mail");
        latch.press(Some(mail.clone()), YAP, YAP_PID, 1_000);
        // The stop press lands while Yap's own window is frontmost — by bundle
        // id, and by pid with no bundle id at all.
        latch.press(Some(app(YAP_PID, YAP)), YAP, YAP_PID, 9_000);
        latch.press(
            Some(FrontApp {
                pid: YAP_PID,
                bundle_id: None,
            }),
            YAP,
            YAP_PID,
            9_010,
        );
        assert_eq!(latch.pre_click(9_100), Some(&mail));
        assert_eq!(
            paste_target(latch.pre_click(9_100), Some(&app(YAP_PID, YAP))),
            PasteTarget::Refocus(mail)
        );
    }

    #[test]
    fn switching_apps_mid_take_retargets_to_the_app_before_the_stop_click() {
        let mut latch = PreClickLatch::default();
        latch.press(Some(app(200, "com.apple.Notes")), YAP, YAP_PID, 1_000);
        let mail = app(300, "com.apple.mail");
        latch.press(Some(mail.clone()), YAP, YAP_PID, 5_000);
        assert_eq!(latch.pre_click(5_100), Some(&mail));
    }

    #[test]
    fn a_stale_press_is_not_this_clicks_press() {
        let mut latch = PreClickLatch::default();
        latch.press(Some(app(200, "com.apple.Notes")), YAP, YAP_PID, 1_000);
        assert!(latch.pre_click(1_000 + LATCH_FRESH_MS + 1).is_none());
        assert_eq!(
            paste_target(
                latch.pre_click(1_000 + LATCH_FRESH_MS + 1),
                Some(&app(YAP_PID, YAP))
            ),
            PasteTarget::Keep
        );
    }

    #[test]
    fn no_press_ever_latched_keeps_whatever_is_front() {
        let latch = PreClickLatch::default();
        assert_eq!(
            paste_target(latch.pre_click(10), Some(&app(YAP_PID, YAP))),
            PasteTarget::Keep
        );
        // A press with only Yap frontmost latches nothing.
        let mut latch = PreClickLatch::default();
        latch.press(Some(app(YAP_PID, YAP)), YAP, YAP_PID, 5);
        assert!(latch.pre_click(6).is_none());
    }
}

#[cfg(test)]
mod queue_tests {
    use super::*;

    #[test]
    fn a_click_while_busy_becomes_the_next_start() {
        let mut q = ClickQueue::new();
        assert_eq!(q.click(1_000), QueueAction::Queued);
        assert!(q.pending());
        assert_eq!(q.due(1_020, true), QueueDue::Wait);
        assert_eq!(q.due(1_900, false), QueueDue::Start { waited_ms: 900 });
        assert!(!q.pending());
        assert_eq!(q.due(1_920, false), QueueDue::Nothing);
    }

    #[test]
    fn a_second_click_while_busy_cancels_the_first() {
        let mut q = ClickQueue::new();
        assert_eq!(q.click(0), QueueAction::Queued);
        assert_eq!(q.click(150), QueueAction::Cancelled);
        assert!(!q.pending());
        assert_eq!(q.due(500, false), QueueDue::Nothing);
        // and a third is a fresh start again
        assert_eq!(q.click(300), QueueAction::Queued);
        assert_eq!(q.due(400, false), QueueDue::Start { waited_ms: 100 });
    }

    #[test]
    fn clicks_at_0_50_150_300_ms_into_a_finish_are_never_dropped() {
        // acceptance 3: one click at each offset into a busy window; each is
        // honoured as the next start once the finish clears.
        for offset in [0u64, 50, 150, 300] {
            let mut q = ClickQueue::new();
            let busy_from = 10_000;
            assert_eq!(
                q.click(busy_from + offset),
                QueueAction::Queued,
                "offset {offset}"
            );
            assert_eq!(q.due(busy_from + offset + 10, true), QueueDue::Wait);
            assert_eq!(
                q.due(busy_from + 800, false),
                QueueDue::Start {
                    waited_ms: 800 - offset
                },
                "offset {offset}"
            );
        }
    }

    #[test]
    fn a_queued_click_expires_loudly_after_the_ttl() {
        let mut q = ClickQueue::new();
        q.click(0);
        assert_eq!(q.due(QUEUED_CLICK_TTL_MS, true), QueueDue::Wait);
        assert_eq!(
            q.due(QUEUED_CLICK_TTL_MS + 1, true),
            QueueDue::Expired {
                waited_ms: QUEUED_CLICK_TTL_MS + 1
            }
        );
        assert!(!q.pending());
    }
}

/// yap-1107-pill-click-always-registers acceptance 4 (the logic half): fifty
/// seeded clicks through the SAME two pieces `lib.rs::pill_toggle` composes —
/// the one take machine and [`ClickQueue`] — against a simulated recorder
/// (warm-up, the stop's refocus wait, decoding). Every click must count as the
/// next intent in order: start, stop, start, ... none dropped. The on-screen
/// half (points inside the drawn pill, pressed latency) is `pill-matrix.mjs
/// --pill-click`; set `YAP_PILL_CLICK_MODEL_CSV=<file>` to save the timings.
#[cfg(test)]
mod fifty_click_tests {
    use super::*;
    use crate::take_control::{Decision, IgnoreReason, Intent, Surface, TakeMachine, TakePhase};

    /// Recorder warm-up (click -> armed).
    const ARM_MS: u64 = 180;
    /// The stop's refocus wait (machine stays `Stopping`).
    const STOP_MS: u64 = 120;
    /// Decoding after the stop (`state.busy`).
    const DECODE_MS: u64 = 600;
    /// Gaps between clicks, cycled. Each lands in a different window of the
    /// previous take: idle, warm-up, stopping, decoding (queued then its
    /// cancelling partner), decoding again (queued, then started by the
    /// waiter), then a plain stop of the waiter's take.
    const GAPS: [u64; 6] = [900, 50, 180, 100, 300, 1200];

    struct Sim {
        machine: TakeMachine,
        queue: ClickQueue,
        live: bool,
        arm_at: Option<u64>,
        stop_done_at: Option<u64>,
        busy_until: u64,
        takes_recorded: u32,
        takes_stopped: u32,
        queued_fired: u32,
    }

    /// One click's honest outcome, as the user meant it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Outcome {
        Start,
        Stop,
        Dropped,
    }

    impl Sim {
        fn new() -> Self {
            Self {
                machine: TakeMachine::new(),
                queue: ClickQueue::new(),
                live: false,
                arm_at: None,
                stop_done_at: None,
                busy_until: 0,
                takes_recorded: 0,
                takes_stopped: 0,
                queued_fired: 0,
            }
        }

        fn busy(&self, now: u64) -> bool {
            now < self.busy_until
        }

        /// `lib.rs::pill_toggle`, minus the I/O.
        fn toggle(&mut self, now: u64) -> Outcome {
            self.machine.observe(self.live);
            let decision = self.machine.intent(Surface::Pill, Intent::Toggle, now);
            match decision {
                Decision::Start => {
                    if self.busy(now) || self.queue.pending() {
                        // busy or already queued: the start closure did not open the mic: armed(false)
                        let _ = self.machine.armed(false);
                    } else {
                        self.arm_at = Some(now + ARM_MS);
                        return Outcome::Start;
                    }
                }
                Decision::Stop => {
                    self.stop_done_at = Some(now + STOP_MS);
                    return Outcome::Stop;
                }
                Decision::StopWhenArmed => return Outcome::Stop,
                Decision::Ignore(IgnoreReason::Stopping) => {}
                Decision::Ignore(_) => return Outcome::Dropped,
            }
            match self.queue.click(now) {
                QueueAction::Queued => Outcome::Start,
                QueueAction::Cancelled => Outcome::Stop,
            }
        }

        /// Advance the recorder and the queued-click waiter to `now`.
        fn tick(&mut self, now: u64) {
            if self.arm_at.is_some_and(|t| t <= now) {
                self.arm_at = None;
                self.live = true;
                self.takes_recorded += 1;
                if self.machine.armed(true) == Decision::Stop {
                    self.stop_done_at = Some(now + STOP_MS);
                }
            }
            if self.stop_done_at.is_some_and(|t| t <= now) {
                self.stop_done_at = None;
                self.live = false;
                self.machine.stopped();
                self.takes_stopped += 1;
                self.busy_until = now + DECODE_MS;
            }
            if now % QUEUED_CLICK_POLL_MS == 0 && self.queue.pending() {
                let stopping = matches!(self.machine.phase(), TakePhase::Stopping);
                let busy = self.busy(now) || stopping;
                match self.queue.due(now, busy) {
                    QueueDue::Start { .. } => {
                        self.queued_fired += 1;
                        assert_eq!(
                            self.toggle(now),
                            Outcome::Start,
                            "the waiter's start at {now}"
                        );
                    }
                    QueueDue::Expired { waited_ms } => {
                        panic!("a queued click expired after {waited_ms} ms at {now}")
                    }
                    QueueDue::Wait | QueueDue::Nothing => {}
                }
            }
        }
    }

    fn phase_name(p: TakePhase) -> &'static str {
        p.as_str()
    }

    #[test]
    fn fifty_seeded_clicks_alternate_start_stop_through_the_take_machine() {
        let mut rng: u64 = 1107;
        let mut next_jitter = || {
            // xorshift64: the run is the same every time.
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng % 15
        };
        let mut sim = Sim::new();
        let mut csv = String::from("i,click_ms,intent,outcome,phase_before,phase_after,queued\n");
        let mut now = 0u64;
        let mut t = 0u64;
        let mut outcomes = Vec::new();
        for i in 0..50usize {
            let gap = if i == 0 {
                100
            } else {
                GAPS[i % GAPS.len()] + next_jitter()
            };
            let at = t + gap;
            while now < at {
                now += 1;
                sim.tick(now);
            }
            t = at;
            let before = phase_name(sim.machine.phase());
            let outcome = sim.toggle(now);
            let after = phase_name(sim.machine.phase());
            let intent = if i % 2 == 0 {
                Outcome::Start
            } else {
                Outcome::Stop
            };
            csv.push_str(&format!(
                "{i},{now},{intent:?},{outcome:?},{before},{after},{}\n",
                sim.queue.pending()
            ));
            assert_ne!(
                outcome,
                Outcome::Dropped,
                "click {i} at {now} ms was dropped"
            );
            assert_eq!(
                outcome, intent,
                "click {i} at {now} ms: start/stop out of order"
            );
            outcomes.push(outcome);
        }
        // drain: the last click (a stop during warm-up) fires once armed
        for _ in 0..5_000 {
            now += 1;
            sim.tick(now);
        }
        assert_eq!(outcomes.len(), 50);
        assert!(!sim.queue.pending());
        assert!(!sim.live, "every take ends stopped");
        assert_eq!(sim.takes_recorded, sim.takes_stopped);
        // 8 full cycles start 2 takes each (the click + the waiter's queued
        // start); clicks 48/49 start and stop one more.
        assert_eq!(sim.queued_fired, 8);
        assert_eq!(sim.takes_recorded, 17);
        if let Ok(path) = std::env::var("YAP_PILL_CLICK_MODEL_CSV") {
            std::fs::write(&path, csv).expect("write the model CSV");
        }
    }
}
