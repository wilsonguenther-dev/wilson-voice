//! YV95 — the manual start/stop control plane for a meeting.
//!
//! Finding #6, classified a phase merge gate: "yap22 has no way to start a
//! meeting… as scoped, 22 ships a feature nobody can reach." YV94 built the
//! storage and the Meetings tab; YV91 (#108) builds the capture session. This
//! module is the thing in the middle — the one place that knows a meeting is
//! running, and the only place that opens or closes one, no matter which of the
//! four entry points the user reached for:
//!
//!   1. the tray item ("Record a meeting" / "Stop meeting"),
//!   2. the ⌃⌘M global toggle ([`crate::shortcuts::MEETING_TOGGLE`]),
//!   3. the pill's own stop control,
//!   4. the Meetings tab's empty-state button.
//!
//! ## Why capture arrives through a trait
//!
//! The audio side is YV91's item and lands as `meeting::MeetingSession` — an
//! RT-safe ring, host-time anchors, a bounded journal, a power assertion, and
//! the dictation fan-out. Re-implementing any of that here to make a button feel
//! finished would put two capture paths in one app, which is how a recorder
//! starts losing audio in exactly one of them. So the control plane depends on a
//! [`CaptureEngine`], installed once at startup by whoever owns capture, and
//! **states plainly when nothing is installed**: [`capture_available`] is what
//! the tray item and the empty-state button are enabled from, so this app never
//! ships a Record button that does nothing when pressed. Wiring YV91 in is
//! [`install_capture_engine`] with an adapter over `meeting::MeetingSession` —
//! one line, in `run()`'s setup.
//!
//! Disabled-but-honest is a *time-boxed* state, not a resting place: finding #6
//! is a phase merge gate, and `tests/capture_engine_is_installed.rs` turns it
//! into a build failure the moment `src/meeting.rs` lands without that line.
//!
//! ## Energy discipline (OS-12)
//!
//! The elapsed readout is a **1 Hz emit from this module's ticker thread**, not
//! a timer in the webview: the pill's canvas loop stays parked (YV81) and the
//! recording pulse is a CSS compositor animation, so a three-hour meeting does
//! not wake the JS thread 648,000 times to redraw a clock that changes once a
//! second. The same tick is where [`meeting_energy`] samples thermal state, so
//! the instrumentation OS-12 asks for costs one extra `objc_msgSend` per second
//! rather than a notification observer with a lifetime of its own.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::meeting_energy::{self, BatteryReading, MeetingDiagnostics, ThermalState};
use crate::meetings::{self, MeetingState};

/// OS-12 fix (1): one elapsed emit per second, with the canvas parked.
pub const TICK_INTERVAL: Duration = Duration::from_secs(1);

/// What the UI says when no capture engine is installed. Shown, never guessed
/// at: a disabled control that does not say why is its own bug report.
pub const NO_ENGINE_MESSAGE: &str =
    "Meeting recording needs the capture engine — it is not installed in this build.";

// ─────────────────────────────── the capture seam ───────────────────────────

/// A finished capture, as the control plane needs to record it.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureOutcome {
    /// The finalized WAV, if one was written. `None` means nothing landed on
    /// disk, which is a `partial` meeting, not a silent success.
    pub wav_path: Option<PathBuf>,
    /// Seconds of audio actually captured (NOT wall time — a stalled device
    /// makes those two differ, and the honest one is this).
    pub seconds: f64,
    /// A note worth showing the user, e.g. "stopped early: disk below 1 GB".
    pub note: Option<String>,
}

/// A capture in flight.
pub trait ActiveCapture: Send {
    /// Seconds captured so far. Used for the honest duration when the wall
    /// clock and the audio clock disagree.
    fn seconds(&self) -> f64;
    fn stop(self: Box<Self>) -> Result<CaptureOutcome, String>;
}

/// Whoever owns audio capture implements this and installs it once.
pub trait CaptureEngine: Send + Sync + 'static {
    /// Begin capturing into `dir`.
    fn start(&self, dir: &Path) -> Result<Box<dyn ActiveCapture>, String>;
}

static ENGINE: OnceLock<Arc<dyn CaptureEngine>> = OnceLock::new();

/// Install the capture engine. First call wins; returns `false` if one was
/// already installed (a second engine would mean two recorders).
pub fn install_capture_engine(engine: Arc<dyn CaptureEngine>) -> bool {
    ENGINE.set(engine).is_ok()
}

pub fn capture_engine() -> Option<Arc<dyn CaptureEngine>> {
    ENGINE.get().cloned()
}

/// Is there anything behind the Record button? Every entry point's enabled
/// state reads this.
pub fn capture_available() -> bool {
    ENGINE.get().is_some()
}

// ──────────────────────────── environment probes ────────────────────────────

/// The readings the diagnostics blob is built from. A trait so a test can drive
/// a thermal ramp without a hot Mac.
pub trait EnergyProbe: Send + Sync + 'static {
    fn thermal(&self) -> ThermalState;
    fn battery(&self) -> BatteryReading;
    /// Is the always-on-top pill on screen? OS-12's whole argument is about the
    /// pill's cost, so a power reading with this unknown is uninterpretable.
    fn pill_visible(&self) -> bool;
}

/// The real one: `ProcessInfo.thermalState`, IOKit power sources, and the pill's
/// own YV81 visibility flag.
pub struct SystemProbe;

impl EnergyProbe for SystemProbe {
    fn thermal(&self) -> ThermalState {
        meeting_energy::thermal()
    }
    fn battery(&self) -> BatteryReading {
        meeting_energy::battery()
    }
    fn pill_visible(&self) -> bool {
        crate::float_pill::is_shown()
    }
}

// ──────────────────────────────── the status ────────────────────────────────

/// The single payload every meeting entry point reflects: emitted as the
/// `meeting` event to both the main window and the pill, and returned by the
/// `meeting_status` command so a freshly-opened window is never blank.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingStatus {
    pub recording: bool,
    pub id: Option<String>,
    pub title: Option<String>,
    /// Whole seconds since the meeting started. Whole, because it is rendered as
    /// `hh:mm:ss` and a float here would be a rounding argument in the UI.
    pub elapsed_seconds: u64,
    /// `hh:mm:ss`, rendered once in Rust so the pill and the window cannot
    /// disagree about the clock.
    pub elapsed_label: String,
    pub capture_available: bool,
    /// Why the entry points are disabled, when they are.
    pub unavailable_reason: Option<String>,
}

impl MeetingStatus {
    fn idle() -> Self {
        let available = capture_available();
        MeetingStatus {
            recording: false,
            id: None,
            title: None,
            elapsed_seconds: 0,
            elapsed_label: meetings::format_offset(0.0),
            capture_available: available,
            unavailable_reason: if available {
                None
            } else {
                Some(NO_ENGINE_MESSAGE.to_string())
            },
        }
    }
}

/// The status broadcast the controller pushes on every transition and on every
/// 1 Hz tick. Named because three call sites pass one and `clippy::type_complexity`
/// is right that `Arc<dyn Fn(&MeetingStatus) + Send + Sync>` spelled out four
/// times is not clearer for it.
pub type StatusSink = Arc<dyn Fn(&MeetingStatus) + Send + Sync>;

/// What `stop` gives back to the caller that asked for it.
#[derive(Debug, Clone, PartialEq)]
pub struct StoppedMeeting {
    pub id: String,
    pub title: String,
    pub duration_seconds: f64,
    pub state: MeetingState,
    pub note: Option<String>,
}

/// `Meeting 3:04 PM` — the title a manually-started meeting gets, so the list
/// row says something before the user renames it (YV94 already has rename).
pub fn default_title(now: DateTime<Local>) -> String {
    format!("Meeting {}", now.format("%-l:%M %p"))
}

// ────────────────────────────── the controller ──────────────────────────────

struct Active {
    id: String,
    title: String,
    started_at: DateTime<Utc>,
    started: Instant,
    capture: Box<dyn ActiveCapture>,
    diagnostics: Arc<Mutex<MeetingDiagnostics>>,
    stop: Arc<AtomicBool>,
    ticker: Option<JoinHandle<()>>,
}

/// The one owner of "is a meeting running". Held in `AppState`.
pub struct MeetingController {
    db: Arc<Database>,
    sink: StatusSink,
    probe: Arc<dyn EnergyProbe>,
    tick: Duration,
    active: Mutex<Option<Active>>,
}

impl MeetingController {
    pub fn new(db: Arc<Database>, sink: StatusSink) -> Self {
        MeetingController {
            db,
            sink,
            probe: Arc::new(SystemProbe),
            tick: TICK_INTERVAL,
            active: Mutex::new(None),
        }
    }

    /// Swap the energy probe. Used by `tests/meeting_diagnostics_row.rs` to walk
    /// a thermal ramp; production keeps [`SystemProbe`].
    pub fn with_probe(mut self, probe: Arc<dyn EnergyProbe>) -> Self {
        self.probe = probe;
        self
    }

    /// Shorten the elapsed tick. Tests only — production is [`TICK_INTERVAL`],
    /// and a test that waited a real second per tick would be a slow test that
    /// proves the same thing.
    pub fn with_tick_interval(mut self, tick: Duration) -> Self {
        self.tick = tick;
        self
    }

    pub fn is_recording(&self) -> bool {
        self.active.lock().map(|a| a.is_some()).unwrap_or(false)
    }

    pub fn status(&self) -> MeetingStatus {
        let guard = match self.active.lock() {
            Ok(g) => g,
            // A poisoned lock means a panic while a meeting was live. Reporting
            // "idle" is the honest answer for a control plane that has lost
            // track, and it leaves the entry points usable.
            Err(_) => return MeetingStatus::idle(),
        };
        match guard.as_ref() {
            None => MeetingStatus::idle(),
            Some(a) => {
                let elapsed = a.started.elapsed().as_secs();
                MeetingStatus {
                    recording: true,
                    id: Some(a.id.clone()),
                    title: Some(a.title.clone()),
                    elapsed_seconds: elapsed,
                    elapsed_label: meetings::format_offset(elapsed as f64),
                    capture_available: true,
                    unavailable_reason: None,
                }
            }
        }
    }

    fn emit(&self) {
        let status = self.status();
        (self.sink)(&status);
    }

    /// Start a meeting. `dir` is where the capture engine writes its audio.
    ///
    /// Order matters: capture is started BEFORE the row is created, so a machine
    /// that cannot record does not leave a trail of empty `failed` meetings in
    /// the user's list every time they press the hotkey by accident.
    pub fn start(&self, dir: &Path, title: Option<String>) -> Result<String, String> {
        let mut guard = self
            .active
            .lock()
            .map_err(|_| "the meeting controller is wedged; restart Yap".to_string())?;
        if guard.is_some() {
            return Err("a meeting is already recording".into());
        }
        let engine = capture_engine().ok_or_else(|| NO_ENGINE_MESSAGE.to_string())?;

        // OS-12 fix (3): the preflight readings are taken before the first
        // sample, which is the only moment "did this session start on a dying
        // battery" is answerable.
        let thermal_at_start = self.probe.thermal();
        let battery_at_start = self.probe.battery();
        let pill_visible = self.probe.pill_visible();

        std::fs::create_dir_all(dir).map_err(|e| format!("meeting audio folder: {e}"))?;
        let capture = engine.start(dir)?;

        let title = title
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| default_title(Local::now()));
        let meeting = match self.db.create_meeting(&title, "manual") {
            Ok(m) => m,
            Err(e) => {
                // The row is the only thing that makes a recording findable, so
                // a capture we cannot record is stopped rather than orphaned.
                let _ = capture.stop();
                return Err(e);
            }
        };

        let diagnostics = Arc::new(Mutex::new(MeetingDiagnostics::start(
            thermal_at_start,
            battery_at_start,
            pill_visible,
        )));
        if let Ok(d) = diagnostics.lock() {
            let _ = self.db.set_meeting_diagnostics(&meeting.id, &d.to_json());
        }
        log::info!(
            "meeting {} started — thermal {}, battery {}%{}, pill {}",
            meeting.id,
            thermal_at_start.as_str(),
            battery_at_start
                .percent
                .map(|p| p.to_string())
                .unwrap_or_else(|| "?".into()),
            if battery_at_start.on_ac { " (AC)" } else { "" },
            if pill_visible { "visible" } else { "hidden" }
        );

        let stop = Arc::new(AtomicBool::new(false));
        let ticker = self.spawn_ticker(
            meeting.id.clone(),
            meeting.title.clone(),
            Instant::now(),
            Arc::clone(&diagnostics),
            Arc::clone(&stop),
        );

        *guard = Some(Active {
            id: meeting.id.clone(),
            title: meeting.title.clone(),
            started_at: meeting.started_at,
            started: Instant::now(),
            capture,
            diagnostics,
            stop,
            ticker: Some(ticker),
        });
        drop(guard);
        self.emit();
        Ok(meeting.id)
    }

    /// OS-12 fix (1) — the 1 Hz elapsed emit, and the only place thermal state
    /// is sampled. One thread per meeting, parked on a sleep between ticks, and
    /// it exits with the meeting rather than living for the process.
    fn spawn_ticker(
        &self,
        id: String,
        title: String,
        started: Instant,
        diagnostics: Arc<Mutex<MeetingDiagnostics>>,
        stop: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        let sink = Arc::clone(&self.sink);
        let probe = Arc::clone(&self.probe);
        let tick = self.tick;
        thread::Builder::new()
            .name("yap-meeting-tick".into())
            .spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    thread::sleep(tick);
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let elapsed = started.elapsed();
                    let secs = elapsed.as_secs();
                    // The extra work this tick does beyond the clock: ONE
                    // thermalState read (OS-12 fix 3).
                    let sample = probe.thermal();
                    if let Ok(mut d) = diagnostics.lock() {
                        if d.observe_thermal(elapsed.as_secs_f64(), sample) {
                            log::info!(
                                "meeting {id}: thermal → {} at {}",
                                sample.as_str(),
                                meetings::format_offset(elapsed.as_secs_f64())
                            );
                        }
                    }
                    sink(&MeetingStatus {
                        recording: true,
                        id: Some(id.clone()),
                        title: Some(title.clone()),
                        elapsed_seconds: secs,
                        elapsed_label: meetings::format_offset(secs as f64),
                        capture_available: true,
                        unavailable_reason: None,
                    });
                }
            })
            .expect("spawn meeting ticker")
    }

    /// Stop the running meeting. `reason` is recorded in the diagnostics blob so
    /// "the user pressed stop" and "the watchdog stopped it" are distinguishable
    /// a week later.
    pub fn stop(&self, reason: &str) -> Result<StoppedMeeting, String> {
        let mut guard = self
            .active
            .lock()
            .map_err(|_| "the meeting controller is wedged; restart Yap".to_string())?;
        let mut active = guard.take().ok_or("no meeting is recording")?;
        drop(guard);

        active.stop.store(true, Ordering::SeqCst);
        if let Some(t) = active.ticker.take() {
            let _ = t.join();
        }

        let wall_seconds = active.started.elapsed().as_secs_f64();
        let outcome = active.capture.stop();
        let battery_at_end = self.probe.battery();

        let (duration, wav, note, state) = match outcome {
            Ok(o) => {
                let state = if o.wav_path.is_some() {
                    // Capture landed. YV93's transcription pipeline moves it on
                    // to `complete`; until it has run, `transcribing` is what is
                    // actually true.
                    MeetingState::Transcribing
                } else {
                    MeetingState::Partial
                };
                // The audio clock is the honest duration; fall back to the wall
                // clock only when the engine reports nothing.
                let secs = if o.seconds > 0.0 {
                    o.seconds
                } else {
                    wall_seconds
                };
                (secs, o.wav_path, o.note, state)
            }
            Err(e) => {
                log::warn!("meeting {} capture stop failed: {e}", active.id);
                (wall_seconds, None, Some(e), MeetingState::Partial)
            }
        };

        if let Ok(mut d) = active.diagnostics.lock() {
            d.finish(battery_at_end, reason);
            let _ = self.db.set_meeting_diagnostics(&active.id, &d.to_json());
        }
        self.db
            .finish_meeting(&active.id, duration, wav.as_deref())?;
        self.db
            .set_meeting_state(&active.id, state, note.as_deref())?;

        log::info!(
            "meeting {} stopped after {} ({reason}) → {}",
            active.id,
            meetings::format_offset(duration),
            state.as_str()
        );
        self.emit();
        Ok(StoppedMeeting {
            id: active.id,
            title: active.title,
            duration_seconds: duration,
            state,
            note,
        })
    }

    /// The single toggle every entry point calls — tray item, ⌃⌘M, the pill's
    /// stop control and the empty state's button all land here, so the four can
    /// never disagree about what pressing them does.
    pub fn toggle(&self, dir: &Path, title: Option<String>) -> Result<MeetingStatus, String> {
        if self.is_recording() {
            self.stop("user stopped")?;
        } else {
            self.start(dir, title)?;
        }
        Ok(self.status())
    }

    /// When the app is quitting with a meeting live, the recording is finalized
    /// rather than abandoned: a meeting the user has to lose to close the app is
    /// the failure this whole phase exists to avoid.
    pub fn stop_if_running(&self, reason: &str) {
        if self.is_recording() {
            if let Err(e) = self.stop(reason) {
                log::warn!("stopping the live meeting on shutdown failed: {e}");
            }
        }
    }

    /// When the meeting started, for a UI that wants to show a wall-clock start.
    pub fn started_at(&self) -> Option<DateTime<Utc>> {
        self.active.lock().ok()?.as_ref().map(|a| a.started_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn a_manual_meeting_is_named_for_when_it_started() {
        let t = Local.with_ymd_and_hms(2026, 8, 11, 15, 4, 0).unwrap();
        assert_eq!(default_title(t), "Meeting 3:04 PM");
        let morning = Local.with_ymd_and_hms(2026, 8, 11, 9, 30, 0).unwrap();
        assert_eq!(default_title(morning), "Meeting 9:30 AM");
    }

    #[test]
    fn an_idle_status_says_why_it_cannot_record() {
        let s = MeetingStatus::idle();
        assert!(!s.recording);
        assert_eq!(s.elapsed_label, "00:00:00");
        // No engine is installed in a unit-test process.
        assert_eq!(s.capture_available, capture_available());
        assert_eq!(s.unavailable_reason.is_none(), s.capture_available);
    }
}
