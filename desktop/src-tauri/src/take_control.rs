//! yap-1107-start-stop-any-surface — THE take state machine.
//!
//! Wilson, 2026-11-07: a take started from one surface stops from any other,
//! the way Wispr Flow does it. Start with the hotkey, stop by clicking the
//! pill; start from the menu bar, stop with the hotkey; start from the Dictate
//! button, stop from the pill. No surface owns the take.
//!
//! Before this module every surface decided for itself: `pill_click_toggle`,
//! the tray's `ToggleDictation`, `manual_toggle` (the in-app Dictate button)
//! each read `state.recording` and called `start_recording` / `stop_take`, and
//! the hotkey arbiter kept its own "I started it" `owner` flag. Two defects
//! came out of that: a stop that landed while `start_recording` was still
//! opening the microphone (arming) read `recording == false` and STARTED a
//! second take, and the chord's hold-release could only ever stop a take whose
//! owner field it had written itself.
//!
//! Now there is ONE state ([`TakePhase`]) and ONE decision
//! ([`TakeMachine::intent`]). Surfaces only send an [`Intent`] tagged with
//! their [`Surface`] and render the state they are told. The hold-to-talk
//! semantics (release stops) live on the TAKE ([`Hold`]), not on a surface: a
//! click or menu stop during a hold also stops, and the release that follows
//! is then a no-op.
//!
//! The machine is pure (no AppKit, no audio), so the 4x4 start-surface x
//! stop-surface matrix is a unit test against a fake recorder ([`drive`]).

use serde::Serialize;

/// Where an intent came from. Every way a human can start or stop a take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    /// A click on the floating pill (`pill_click_toggle`).
    Pill,
    /// The global hotkey: the Carbon dictate chord or the fn tap.
    Hotkey,
    /// The menu bar's Start / Stop dictation item.
    MenuBar,
    /// The in-app Dictate button (Home / sidebar, `manual_toggle`).
    DictateButton,
}

impl Surface {
    pub const ALL: [Surface; 4] = [
        Surface::Pill,
        Surface::Hotkey,
        Surface::MenuBar,
        Surface::DictateButton,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Surface::Pill => "pill",
            Surface::Hotkey => "hotkey",
            Surface::MenuBar => "menu_bar",
            Surface::DictateButton => "dictate_button",
        }
    }
}

/// Which held key a hold-to-talk take is riding on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldKey {
    /// The Carbon dictate chord: a press while a take is live stops it
    /// (toggle off); a press held at least [`CARBON_HOLD_MS`] then released
    /// stops it (hold-to-talk); shorter, the take keeps running.
    Chord,
    /// The fn / fn⌃ tap. Its own machine (`ptt_macos`) decides tap vs hold and
    /// sends a plain [`Intent::Stop`] when it wants the take to end, so a fn
    /// press while a take is live is never a second start and never a stop.
    Fn,
}

/// A chord press held at least this long, then released, is hold-to-talk.
/// Equal to `hotkeys::CARBON_HOLD_MS` (asserted in the tests below).
pub const CARBON_HOLD_MS: u64 = 280;

/// The held key of a hold-to-talk take, and when it went down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Hold {
    pub key: HoldKey,
    pub at_ms: u64,
}

/// THE one state enum of a dictation take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum TakePhase {
    /// No take.
    Idle,
    /// A start was decided and the recorder is opening the microphone
    /// (warm-up). `stop_by` is set by a stop that lands in this window (the
    /// surface that asked); it is honoured the moment the recorder reports
    /// armed.
    Arming {
        started_by: Surface,
        stop_by: Option<Surface>,
    },
    /// Capture is live.
    Recording {
        /// Who started it — reported in logs and to the pill, never consulted
        /// to refuse a stop.
        started_by: Option<Surface>,
    },
    /// A stop was decided; the recorder is finalizing. Any further stop is a
    /// no-op, and no start is accepted until the recorder reports stopped.
    Stopping,
}

impl TakePhase {
    pub fn as_str(self) -> &'static str {
        match self {
            TakePhase::Idle => "idle",
            TakePhase::Arming { .. } => "arming",
            TakePhase::Recording { .. } => "recording",
            TakePhase::Stopping => "stopping",
        }
    }

    pub fn is_live(self) -> bool {
        matches!(self, TakePhase::Arming { .. } | TakePhase::Recording { .. })
    }
}

/// What a surface asks for. Surfaces never decide; they send one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Intent {
    /// Start a take if none is live (hands-free chord, upcoming-meeting row).
    Start,
    /// Stop the live take, whoever started it.
    Stop,
    /// Start when idle, stop when live (pill click, menu item, Dictate button).
    Toggle,
    /// A held key went down: starts a hold-to-talk take when idle; while a
    /// take is live the chord stops it and fn is ignored.
    HoldPress(HoldKey),
    /// A held key came up: stops a take this hold is riding on once the hold
    /// was long enough; otherwise the take keeps running hands-free.
    HoldRelease(HoldKey),
}

/// Why an intent did nothing (logged as `reason=`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IgnoreReason {
    /// A stop with no take (a double stop lands here).
    NotLive,
    /// A start while a take is already live or arming: never a second take.
    AlreadyLive,
    /// A stop or start while the recorder is still finalizing the last stop.
    Stopping,
    /// A short tap release: the take keeps running.
    TapNotHold,
    /// A release whose press this take is not riding on.
    NotHeld,
    /// fn pressed while a take is live (its release, if any, stops it).
    FnWhileLive,
}

impl IgnoreReason {
    pub fn as_str(self) -> &'static str {
        match self {
            IgnoreReason::NotLive => "not_live",
            IgnoreReason::AlreadyLive => "already_live",
            IgnoreReason::Stopping => "stopping",
            IgnoreReason::TapNotHold => "tap_not_hold",
            IgnoreReason::NotHeld => "not_held",
            IgnoreReason::FnWhileLive => "fn_while_live",
        }
    }
}

/// What the dispatcher must do with one intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Decision {
    /// Open the recorder, then report [`TakeMachine::armed`].
    Start,
    /// Stop the live take now, then report [`TakeMachine::stopped`].
    Stop,
    /// The take is still arming: the stop is remembered and fires when the
    /// recorder reports armed (the `armed` call returns [`Decision::Stop`]).
    StopWhenArmed,
    Ignore(IgnoreReason),
}

/// THE machine. One per app (`AppState::take_machine`).
#[derive(Debug, Clone)]
pub struct TakeMachine {
    phase: TakePhase,
    hold: Option<Hold>,
    takes_started: u64,
}

impl Default for TakeMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl TakeMachine {
    pub const fn new() -> Self {
        Self {
            phase: TakePhase::Idle,
            hold: None,
            takes_started: 0,
        }
    }

    pub fn phase(&self) -> TakePhase {
        self.phase
    }

    /// The held key the live take is riding on, if it is a hold-to-talk take.
    pub fn hold(&self) -> Option<Hold> {
        self.hold
    }

    /// How many starts this machine has decided (a counter for the log).
    pub fn takes_started(&self) -> u64 {
        self.takes_started
    }

    /// Re-sync with the app's recording flag before deciding. A take can end
    /// without a surface (session cap, cancel chord, error, meeting); a take
    /// can also start through a path this machine did not decide (adopted as
    /// `started_by: None`). Arming is never cut short here: during warm-up the
    /// flag is still false by design.
    pub fn observe(&mut self, live: bool) {
        match (self.phase, live) {
            (TakePhase::Recording { .. }, false) | (TakePhase::Stopping, false) => {
                self.phase = TakePhase::Idle;
                self.hold = None;
            }
            (TakePhase::Idle, true) => {
                self.phase = TakePhase::Recording { started_by: None };
            }
            _ => {}
        }
    }

    /// THE decision for every surface.
    pub fn intent(&mut self, surface: Surface, intent: Intent, now_ms: u64) -> Decision {
        match (self.phase, intent) {
            // ── idle ────────────────────────────────────────────────────────
            (TakePhase::Idle, Intent::Start | Intent::Toggle) => self.begin(surface, None),
            (TakePhase::Idle, Intent::HoldPress(key)) => {
                self.begin(surface, Some(Hold { key, at_ms: now_ms }))
            }
            (TakePhase::Idle, Intent::Stop) => Decision::Ignore(IgnoreReason::NotLive),
            (TakePhase::Idle, Intent::HoldRelease(_)) => Decision::Ignore(IgnoreReason::NotLive),

            // ── stopping: nothing until the recorder reports stopped ────────
            (TakePhase::Stopping, _) => Decision::Ignore(IgnoreReason::Stopping),

            // ── arming (warm-up) ────────────────────────────────────────────
            (TakePhase::Arming { .. }, Intent::Start) => {
                Decision::Ignore(IgnoreReason::AlreadyLive)
            }
            (TakePhase::Arming { .. }, Intent::HoldPress(HoldKey::Fn)) => {
                Decision::Ignore(IgnoreReason::FnWhileLive)
            }
            (TakePhase::Arming { .. }, Intent::HoldRelease(key)) => {
                self.release(surface, key, now_ms)
            }
            (
                TakePhase::Arming {
                    started_by,
                    stop_by,
                },
                Intent::Stop | Intent::Toggle | Intent::HoldPress(HoldKey::Chord),
            ) => {
                self.phase = TakePhase::Arming {
                    started_by,
                    stop_by: stop_by.or(Some(surface)),
                };
                self.hold = None;
                Decision::StopWhenArmed
            }

            // ── recording ───────────────────────────────────────────────────
            (TakePhase::Recording { .. }, Intent::Start) => {
                Decision::Ignore(IgnoreReason::AlreadyLive)
            }
            (TakePhase::Recording { .. }, Intent::HoldPress(HoldKey::Fn)) => {
                Decision::Ignore(IgnoreReason::FnWhileLive)
            }
            (TakePhase::Recording { .. }, Intent::HoldRelease(key)) => {
                self.release(surface, key, now_ms)
            }
            (
                TakePhase::Recording { .. },
                Intent::Stop | Intent::Toggle | Intent::HoldPress(HoldKey::Chord),
            ) => self.end(),
        }
    }

    fn begin(&mut self, surface: Surface, hold: Option<Hold>) -> Decision {
        self.phase = TakePhase::Arming {
            started_by: surface,
            stop_by: None,
        };
        self.hold = hold;
        self.takes_started += 1;
        Decision::Start
    }

    fn end(&mut self) -> Decision {
        self.phase = TakePhase::Stopping;
        self.hold = None;
        Decision::Stop
    }

    fn release(&mut self, surface: Surface, key: HoldKey, now_ms: u64) -> Decision {
        let Some(hold) = self.hold.filter(|h| h.key == key) else {
            return Decision::Ignore(IgnoreReason::NotHeld);
        };
        // The press is consumed either way: a later release never re-matches.
        self.hold = None;
        let long_enough = match key {
            HoldKey::Chord => now_ms.saturating_sub(hold.at_ms) >= CARBON_HOLD_MS,
            // fn's own machine already judged tap vs hold before it sent this.
            HoldKey::Fn => true,
        };
        if !long_enough {
            return Decision::Ignore(IgnoreReason::TapNotHold);
        }
        match self.phase {
            TakePhase::Arming {
                started_by,
                stop_by,
            } => {
                self.phase = TakePhase::Arming {
                    started_by,
                    stop_by: stop_by.or(Some(surface)),
                };
                Decision::StopWhenArmed
            }
            _ => self.end(),
        }
    }

    /// The recorder finished opening. `ok == false`: the take never started
    /// (mic refused, license, busy) — back to Idle. A stop that landed during
    /// warm-up is returned as [`Decision::Stop`] so the dispatcher runs it now.
    pub fn armed(&mut self, ok: bool) -> Decision {
        let TakePhase::Arming {
            started_by,
            stop_by,
        } = self.phase
        else {
            return Decision::Ignore(IgnoreReason::NotLive);
        };
        if !ok {
            self.phase = TakePhase::Idle;
            self.hold = None;
            return Decision::Ignore(IgnoreReason::NotLive);
        }
        self.phase = TakePhase::Recording {
            started_by: Some(started_by),
        };
        if stop_by.is_some() {
            self.end()
        } else {
            Decision::Ignore(IgnoreReason::AlreadyLive)
        }
    }

    /// The surface whose stop landed during warm-up, if one did — the
    /// dispatcher reads it before [`TakeMachine::armed`] to name the stop.
    pub fn pending_stop_by(&self) -> Option<Surface> {
        match self.phase {
            TakePhase::Arming { stop_by, .. } => stop_by,
            _ => None,
        }
    }

    /// The recorder finished the stop (capture closed, take handed to decode).
    pub fn stopped(&mut self) {
        self.phase = TakePhase::Idle;
        self.hold = None;
    }
}

/// The recorder as the machine sees it. Production: `start_recording` /
/// `stop_take` in lib.rs. Tests: the `FakeRecorder` in this module's tests.
pub trait Recorder {
    /// Open the microphone. Returns whether capture is live.
    fn start(&mut self, surface: Surface) -> bool;
    /// Close the take and hand it on.
    fn stop(&mut self, surface: Surface);
}

/// Run one intent through the machine and the recorder, the exact order the
/// app's dispatcher (`lib.rs::take_intent`) uses: decide under the machine,
/// start, report armed (and run a stop that landed during warm-up), stop,
/// report stopped. Returns the first decision.
pub fn drive<R: Recorder + ?Sized>(
    machine: &mut TakeMachine,
    recorder: &mut R,
    surface: Surface,
    intent: Intent,
    live: bool,
    now_ms: u64,
) -> Decision {
    machine.observe(live);
    let decision = machine.intent(surface, intent, now_ms);
    match decision {
        Decision::Start => {
            let ok = recorder.start(surface);
            if machine.armed(ok) == Decision::Stop {
                recorder.stop(surface);
                machine.stopped();
            }
        }
        Decision::Stop => {
            recorder.stop(surface);
            machine.stopped();
        }
        Decision::StopWhenArmed | Decision::Ignore(_) => {}
    }
    decision
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A recorder that counts. `live` is the app's recording flag;
    /// `saved` counts takes handed on; `orphans` counts a start while capture
    /// was already open (a second recorder).
    #[derive(Debug, Default)]
    pub struct FakeRecorder {
        pub live: bool,
        pub starts: u32,
        pub saved: u32,
        pub orphans: u32,
        pub refuse_start: bool,
    }

    impl Recorder for FakeRecorder {
        fn start(&mut self, _surface: Surface) -> bool {
            if self.refuse_start {
                return false;
            }
            if self.live {
                self.orphans += 1;
            }
            self.starts += 1;
            self.live = true;
            true
        }
        fn stop(&mut self, _surface: Surface) {
            if self.live {
                self.saved += 1;
            }
            self.live = false;
        }
    }

    /// The intent a surface sends to START a take, and to STOP one.
    fn start_intent(s: Surface, hold_mode: bool) -> Intent {
        // The chord's press starts both a hold and a toggle tap; which one it
        // was is decided by the release (see `run_pair`).
        let _ = hold_mode;
        match s {
            Surface::Hotkey => Intent::HoldPress(HoldKey::Chord),
            _ => Intent::Toggle,
        }
    }

    fn step(m: &mut TakeMachine, r: &mut FakeRecorder, s: Surface, i: Intent, t: u64) -> Decision {
        let live = r.live;
        drive(m, r, s, i, live, t)
    }

    /// Start from `a`, stop from `b`. `hold_mode`: the hotkey start is a
    /// hold (released after the stop, or the release IS the stop for
    /// hotkey->hotkey); otherwise it is a toggle tap (released at once).
    fn run_pair(a: Surface, b: Surface, hold_mode: bool) -> (TakeMachine, FakeRecorder) {
        let mut m = TakeMachine::new();
        let mut r = FakeRecorder::default();
        assert_eq!(
            step(&mut m, &mut r, a, start_intent(a, hold_mode), 0),
            Decision::Start
        );
        if a == Surface::Hotkey && !hold_mode {
            // Toggle tap: the release comes 90 ms later and the take keeps running.
            assert_eq!(
                step(&mut m, &mut r, a, Intent::HoldRelease(HoldKey::Chord), 90),
                Decision::Ignore(IgnoreReason::TapNotHold)
            );
        }
        assert!(r.live, "{a:?} must start capture");
        assert!(m.phase().is_live());
        let stop = if b == Surface::Hotkey && a == Surface::Hotkey && hold_mode {
            Intent::HoldRelease(HoldKey::Chord)
        } else if b == Surface::Hotkey {
            Intent::HoldPress(HoldKey::Chord)
        } else {
            Intent::Toggle
        };
        assert_eq!(
            step(&mut m, &mut r, b, stop, 2_000),
            Decision::Stop,
            "start {a:?} stop {b:?} hold={hold_mode}"
        );
        // The hold's own release (after a click/menu stop) and the stop key's
        // release are no-ops: no second take, nothing stopped twice.
        if a == Surface::Hotkey && hold_mode && b != Surface::Hotkey {
            assert!(matches!(
                step(
                    &mut m,
                    &mut r,
                    a,
                    Intent::HoldRelease(HoldKey::Chord),
                    2_500
                ),
                Decision::Ignore(_)
            ));
        }
        if b == Surface::Hotkey && !(a == Surface::Hotkey && hold_mode) {
            assert!(matches!(
                step(
                    &mut m,
                    &mut r,
                    b,
                    Intent::HoldRelease(HoldKey::Chord),
                    2_080
                ),
                Decision::Ignore(_)
            ));
        }
        (m, r)
    }

    fn assert_clean(m: &TakeMachine, r: &FakeRecorder, label: &str) {
        assert_eq!(m.phase(), TakePhase::Idle, "{label}: ends Idle");
        assert_eq!(r.saved, 1, "{label}: exactly one saved take");
        assert_eq!(r.starts, 1, "{label}: no second take started");
        assert_eq!(r.orphans, 0, "{label}: no orphan recorder");
        assert!(!r.live, "{label}: recorder closed");
        assert!(m.hold().is_none(), "{label}: no hold left behind");
    }

    #[test]
    fn every_start_surface_stops_from_every_stop_surface() {
        let mut cases = 0;
        for a in Surface::ALL {
            for b in Surface::ALL {
                let (m, r) = run_pair(a, b, false);
                assert_clean(&m, &r, &format!("{}->{}", a.as_str(), b.as_str()));
                cases += 1;
            }
        }
        assert_eq!(cases, 16);
    }

    #[test]
    fn a_hotkey_hold_stops_from_every_surface_and_its_release_is_then_a_no_op() {
        for b in Surface::ALL {
            let (m, r) = run_pair(Surface::Hotkey, b, true);
            assert_clean(&m, &r, &format!("hotkey(hold)->{}", b.as_str()));
        }
    }

    #[test]
    fn a_click_or_menu_stop_during_a_hold_stops_and_the_release_does_not_restart() {
        for b in [Surface::Pill, Surface::MenuBar, Surface::DictateButton] {
            let mut m = TakeMachine::new();
            let mut r = FakeRecorder::default();
            step(
                &mut m,
                &mut r,
                Surface::Hotkey,
                Intent::HoldPress(HoldKey::Chord),
                0,
            );
            assert_eq!(step(&mut m, &mut r, b, Intent::Toggle, 500), Decision::Stop);
            assert_eq!(
                step(
                    &mut m,
                    &mut r,
                    Surface::Hotkey,
                    Intent::HoldRelease(HoldKey::Chord),
                    900
                ),
                Decision::Ignore(IgnoreReason::NotLive)
            );
            assert_clean(&m, &r, b.as_str());
        }
    }

    #[test]
    fn a_stale_hold_release_never_stops_a_take_another_surface_started_after() {
        let mut m = TakeMachine::new();
        let mut r = FakeRecorder::default();
        step(
            &mut m,
            &mut r,
            Surface::Hotkey,
            Intent::HoldPress(HoldKey::Chord),
            0,
        );
        step(&mut m, &mut r, Surface::Pill, Intent::Toggle, 400);
        // A new take from the pill while the chord is still physically down.
        assert_eq!(
            step(&mut m, &mut r, Surface::Pill, Intent::Toggle, 600),
            Decision::Start
        );
        assert_eq!(
            step(
                &mut m,
                &mut r,
                Surface::Hotkey,
                Intent::HoldRelease(HoldKey::Chord),
                900
            ),
            Decision::Ignore(IgnoreReason::NotHeld)
        );
        assert!(r.live);
    }

    #[test]
    fn fn_hold_and_fn_tap_stop_like_the_chord() {
        // fn hold: press starts, the fn machine's Stop (sent as Intent::Stop) ends it.
        let mut m = TakeMachine::new();
        let mut r = FakeRecorder::default();
        assert_eq!(
            step(
                &mut m,
                &mut r,
                Surface::Hotkey,
                Intent::HoldPress(HoldKey::Fn),
                0
            ),
            Decision::Start
        );
        // A fn press during a live take is never a second start.
        assert_eq!(
            step(
                &mut m,
                &mut r,
                Surface::Hotkey,
                Intent::HoldPress(HoldKey::Fn),
                100
            ),
            Decision::Ignore(IgnoreReason::FnWhileLive)
        );
        assert_eq!(
            step(&mut m, &mut r, Surface::Pill, Intent::Toggle, 900),
            Decision::Stop
        );
        assert_clean(&m, &r, "fn->pill");
        // Pill start, fn's Stop ends it.
        let mut m = TakeMachine::new();
        let mut r = FakeRecorder::default();
        step(&mut m, &mut r, Surface::Pill, Intent::Toggle, 0);
        assert_eq!(
            step(&mut m, &mut r, Surface::Hotkey, Intent::Stop, 900),
            Decision::Stop
        );
        assert_clean(&m, &r, "pill->fn");
    }

    #[test]
    fn a_stop_during_arming_stops_cleanly_once_the_recorder_is_armed() {
        for a in Surface::ALL {
            for b in Surface::ALL {
                let mut m = TakeMachine::new();
                let mut r = FakeRecorder::default();
                // Decide the start but do not arm yet: the recorder is warming up.
                m.observe(false);
                assert_eq!(m.intent(a, start_intent(a, false), 0), Decision::Start);
                // A stop from another surface lands during warm-up. The recording
                // flag is still false; it must NOT start a second take.
                m.observe(false);
                let stop = if b == Surface::Hotkey {
                    Intent::HoldPress(HoldKey::Chord)
                } else {
                    Intent::Toggle
                };
                assert_eq!(
                    m.intent(b, stop, 50),
                    Decision::StopWhenArmed,
                    "{a:?}->{b:?}"
                );
                // A second stop during warm-up is a no-op too.
                assert_eq!(m.intent(b, Intent::Stop, 60), Decision::StopWhenArmed);
                assert_eq!(m.pending_stop_by(), Some(b));
                assert!(r.start(a));
                assert_eq!(m.armed(true), Decision::Stop);
                r.stop(b);
                m.stopped();
                assert_clean(&m, &r, &format!("arming {a:?}->{b:?}"));
            }
        }
    }

    #[test]
    fn a_start_during_arming_is_never_a_second_take() {
        let mut m = TakeMachine::new();
        assert_eq!(
            m.intent(Surface::MenuBar, Intent::Toggle, 0),
            Decision::Start
        );
        assert_eq!(
            m.intent(Surface::Hotkey, Intent::HoldPress(HoldKey::Fn), 10),
            Decision::Ignore(IgnoreReason::FnWhileLive)
        );
        assert_eq!(
            m.intent(Surface::Pill, Intent::Start, 10),
            Decision::Ignore(IgnoreReason::AlreadyLive)
        );
        assert_eq!(m.takes_started(), 1);
    }

    #[test]
    fn a_failed_start_returns_to_idle() {
        let mut m = TakeMachine::new();
        let mut r = FakeRecorder {
            refuse_start: true,
            ..Default::default()
        };
        assert_eq!(
            step(&mut m, &mut r, Surface::DictateButton, Intent::Toggle, 0),
            Decision::Start
        );
        assert_eq!(m.phase(), TakePhase::Idle);
        assert_eq!(r.saved, 0);
    }

    #[test]
    fn a_double_stop_is_a_no_op() {
        for a in Surface::ALL {
            for b in Surface::ALL {
                let mut m = TakeMachine::new();
                let mut r = FakeRecorder::default();
                step(&mut m, &mut r, a, start_intent(a, false), 0);
                if a == Surface::Hotkey {
                    step(&mut m, &mut r, a, Intent::HoldRelease(HoldKey::Chord), 50);
                }
                assert_eq!(step(&mut m, &mut r, b, Intent::Stop, 1_000), Decision::Stop);
                assert_eq!(
                    step(&mut m, &mut r, b, Intent::Stop, 1_010),
                    Decision::Ignore(IgnoreReason::NotLive)
                );
                // Two stops racing before the recorder reports stopped.
                let mut m2 = TakeMachine::new();
                m2.intent(a, Intent::Toggle, 0);
                m2.armed(true);
                assert_eq!(m2.intent(b, Intent::Stop, 5), Decision::Stop);
                assert_eq!(
                    m2.intent(a, Intent::Toggle, 6),
                    Decision::Ignore(IgnoreReason::Stopping)
                );
                assert_clean(&m, &r, &format!("double stop {a:?}->{b:?}"));
            }
        }
    }

    #[test]
    fn a_take_that_ended_without_a_surface_resyncs_to_idle() {
        let mut m = TakeMachine::new();
        let mut r = FakeRecorder::default();
        step(&mut m, &mut r, Surface::Pill, Intent::Toggle, 0);
        // Session cap / cancel chord closed capture behind the machine's back.
        r.live = false;
        assert_eq!(
            step(
                &mut m,
                &mut r,
                Surface::Hotkey,
                Intent::HoldPress(HoldKey::Chord),
                10
            ),
            Decision::Start
        );
        assert_eq!(r.starts, 2);
        assert_eq!(r.orphans, 0);
    }

    #[test]
    fn hold_threshold_matches_the_hotkeys_module() {
        assert_eq!(CARBON_HOLD_MS, crate::hotkeys::CARBON_HOLD_MS);
    }
}
