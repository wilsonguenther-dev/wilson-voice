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
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::meeting_energy::{self, BatteryReading, MeetingDiagnostics, ThermalState};
use crate::meetings::{self, MeetingKind, MeetingState};
use crate::syscapture;

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
    /// The finalized MIC WAV, if one was written. `None` means nothing landed on
    /// disk, which is a `partial` meeting, not a silent success.
    pub wav_path: Option<PathBuf>,
    /// YV106 — the finalized SYSTEM-AUDIO WAV of a two-track meeting.
    ///
    /// `None` for every mic-only meeting, and for a two-track meeting whose tap
    /// never delivered a sample — matrix row #2's degrade, where the mic track
    /// is still a whole recording and the row must not claim a second one.
    #[allow(dead_code)] // read by `MeetingController::stop`
    pub sys_wav_path: Option<PathBuf>,
    /// YV104 / YV106 — the tap's rebuild log as JSON, when the meeting needed a
    /// rebuild at all. `None` is the common case and stays `NULL` in the row.
    pub tap_rebuilds: Option<String>,
    /// YV107 / OS-2 — each track's measured true rate, straight off the
    /// finalize. Rides the `diagnostics` blob under `track_rates`, never a
    /// column: it is exhaust nothing queries on, which is the policy migration 2
    /// stated for that column when it introduced it.
    ///
    /// Empty when the meeting was too short to measure — "never measured" is not
    /// "measured nothing".
    pub track_rates: Vec<crate::meeting::TrackRate>,
    /// Seconds of audio actually captured (NOT wall time — a stalled device
    /// makes those two differ, and the honest one is this).
    pub seconds: f64,
    /// A note worth showing the user, e.g. "stopped early: disk below 1 GB".
    pub note: Option<String>,
    /// The engine's own verdict that this recording is incomplete — a watchdog
    /// stop, or a session that stopped being the registered capture partway
    /// through (YV91 `MeetingState::Partial`).
    ///
    /// Separate from `wav_path`, because the two say different things: a wav
    /// landed AND the recording is short is exactly the case that must not be
    /// filed as a clean take. Without this the control plane would read "a file
    /// exists" as "the meeting is whole", and YV93 would transcribe it onward to
    /// `complete` with nothing left saying otherwise.
    pub partial: bool,
}

/// A capture in flight.
pub trait ActiveCapture: Send {
    /// Seconds captured so far. Used for the honest duration when the wall
    /// clock and the audio clock disagree.
    fn seconds(&self) -> f64;

    /// YV110 — a reader for what this meeting has to say about its
    /// SYSTEM-AUDIO track, callable on any thread at any time.
    ///
    /// A *probe* and not a plain `Option<String>` because the sentence changes
    /// while the meeting runs (a tap that was attached at T-0 and died at
    /// minute nine is matrix row 2) and because the surface that renders it is
    /// the control plane's 1 Hz ticker thread, which deliberately holds no
    /// reference to the controller — reaching back through it would deadlock
    /// against the `stop` that joins the ticker.
    ///
    /// Two sentences reach it: the T-0 reason a meeting is mic-only (matrix
    /// rows 1 and 12), and YV104's degrade banner when an attached tap is lost
    /// (matrix row 2). `None` means both tracks are recording and there is
    /// nothing to say.
    ///
    /// Defaulted, so a capture engine with no tap (every test's fake, and every
    /// 22-A meeting) says nothing rather than inventing a state.
    fn system_audio_probe(&self) -> Option<SystemAudioProbe> {
        None
    }

    fn stop(self: Box<Self>) -> Result<CaptureOutcome, String>;
}

/// A cheap, thread-safe read of the meeting's system-audio badge. See
/// [`ActiveCapture::system_audio_probe`].
pub type SystemAudioProbe = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// Whoever owns audio capture implements this and installs it once.
pub trait CaptureEngine: Send + Sync + 'static {
    /// Begin capturing into `dir`, for a meeting of `kind` (YV125).
    ///
    /// The kind is passed to the ENGINE and not only written on the row because
    /// the session carries it for its whole life (see
    /// `meeting::SessionConfig::kind`) — the recording and the row describing it
    /// are set from the same value at the same instant, so they cannot come
    /// apart if one write fails.
    fn start(&self, dir: &Path, kind: MeetingKind) -> Result<Box<dyn ActiveCapture>, String>;
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
    /// YV110 — the honest system-audio badge for the meeting that is running:
    /// why it is mic-only, or that its second track was lost. `None` while both
    /// tracks are recording, and `None` when nothing is recording at all.
    ///
    /// See [`ActiveCapture::system_audio`] for why it rides this payload.
    pub system_audio: Option<String>,
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
            system_audio: None,
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

// ─────────────────────────── YV125 · the kind picker ────────────────────────
//
// There is no calendar in this backlog (yap24), so nothing can infer whether a
// meeting is a call or a room — it is ASKED, once, at the moment the user is
// already looking at a start-of-meeting surface. Three choices, one line, and
// skipping is a supported answer rather than a dead end: the picker is a HINT
// that improves the diarization target, never a gate in front of the record
// button. `tests/meeting_kind_default_on_skip.rs` is what holds that open.
//
// The list lives here, in Rust, rather than being written out twice: the tray
// submenu and the Meetings tab both render these three, so a fourth option (or
// a re-worded one) cannot appear on one surface and not the other.

/// One choice on the start-of-meeting kind picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KindChoice {
    pub kind: MeetingKind,
    /// What the user reads. Plain words about the room, not about diarization:
    /// nobody starting a recording is thinking in clustering targets.
    pub label: &'static str,
    /// The tray menu item's id. Namespaced so `on_menu_event`'s match cannot
    /// collide with an unrelated item.
    pub menu_id: &'static str,
}

/// The three choices, in the order both surfaces show them.
///
/// "Not sure" is LAST and is the same answer as not choosing at all — it exists
/// as a visible option so a user who wants to answer the question can answer it
/// honestly, rather than being pushed into guessing "Virtual" because the menu
/// only offered the two confident answers.
pub const KIND_PICKER: [KindChoice; 3] = [
    KindChoice {
        kind: MeetingKind::InPerson,
        label: "In person",
        menu_id: "meeting_start_in_person",
    },
    KindChoice {
        kind: MeetingKind::Virtual,
        label: "Virtual",
        menu_id: "meeting_start_virtual",
    },
    KindChoice {
        kind: MeetingKind::Unknown,
        label: "Not sure",
        menu_id: "meeting_start_unknown",
    },
];

/// The choice a tray menu id names, if any.
pub fn kind_for_menu_id(id: &str) -> Option<MeetingKind> {
    KIND_PICKER.iter().find(|c| c.menu_id == id).map(|c| c.kind)
}

// ────────────────────────────── the controller ──────────────────────────────

struct Active {
    id: String,
    title: String,
    started_at: DateTime<Utc>,
    started: Instant,
    capture: Box<dyn ActiveCapture>,
    /// YV110 — the capture's system-audio badge reader, lifted out of the
    /// capture so the ticker thread can read it without reaching back through
    /// the controller (which would deadlock against the `stop` that joins it).
    system_audio: Option<SystemAudioProbe>,
    diagnostics: Arc<Mutex<MeetingDiagnostics>>,
    stop: Arc<TickerStop>,
    ticker: Option<JoinHandle<()>>,
}

/// The ticker's parking spot.
///
/// **Why this is not an `AtomicBool` + `thread::sleep` (fix round 3).** It was,
/// and `stop()` — which runs on the macOS MAIN THREAD when the user reaches for
/// the tray item or ⌃⌘M — set the flag and then `join()`ed a thread that was in
/// the middle of a one-second `sleep`. The join could not return until that
/// sleep expired, so every stop from a menu or a hotkey froze the UI for up to a
/// full second before any of the actual finalize work even began. A recorder
/// whose Stop button beachballs is a recorder people press twice.
///
/// A condvar makes "stop" an event the sleeping thread is woken FOR rather than
/// a flag it discovers when it happens to look, so the join returns in
/// microseconds. The tick interval becomes a `wait_timeout` deadline, which is
/// what it always meant.
#[derive(Default)]
struct TickerStop {
    stopped: Mutex<bool>,
    woken: Condvar,
}

impl TickerStop {
    fn stop(&self) {
        if let Ok(mut g) = self.stopped.lock() {
            *g = true;
        }
        self.woken.notify_all();
    }

    /// Park until `tick` elapses or someone calls [`TickerStop::stop`].
    /// Returns `true` if the caller should run another tick.
    ///
    /// Two things this has to get right, and the first one was wrong when it was
    /// written — `stopping_immediately_after_starting_is_not_a_race` caught it:
    ///
    /// * **The flag is checked BEFORE parking.** `stop()` can land before the
    ///   ticker thread has even been scheduled, and a `notify_all` with nobody
    ///   waiting is a notification that never happened. Waiting first would then
    ///   sit out the whole interval for a meeting that is already over — the
    ///   exact freeze this type exists to remove, moved one thread over.
    /// * **A spurious wakeup is not a tick.** `wait_timeout_while` re-parks for
    ///   the remaining time unless the flag is actually set, so the 1 Hz emit
    ///   stays 1 Hz.
    fn wait(&self, tick: Duration) -> bool {
        let Ok(guard) = self.stopped.lock() else {
            return false;
        };
        if *guard {
            return false;
        }
        match self
            .woken
            .wait_timeout_while(guard, tick, |stopped| !*stopped)
        {
            Ok((g, _)) => !*g,
            Err(_) => false,
        }
    }
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
                    system_audio: a.system_audio.as_ref().and_then(|probe| probe()),
                }
            }
        }
    }

    fn emit(&self) {
        let status = self.status();
        (self.sink)(&status);
    }

    /// Start a meeting with the picker SKIPPED — the ⌃⌘M path, the tray's
    /// primary item, and every caller that has no opinion.
    ///
    /// [`MeetingKind::Unknown`] is a real answer here and not a placeholder:
    /// `meetings::diarization_target` treats it as the general case (cluster
    /// Track A) precisely so that skipping the question can never be the reason
    /// a room recording gets filed as one speaker.
    pub fn start(&self, dir: &Path, title: Option<String>) -> Result<String, String> {
        self.start_with_kind(dir, title, MeetingKind::Unknown)
    }

    /// Start a meeting. `dir` is where the capture engine writes its audio.
    ///
    /// Order matters: capture is started BEFORE the row is created, so a machine
    /// that cannot record does not leave a trail of empty `failed` meetings in
    /// the user's list every time they press the hotkey by accident.
    pub fn start_with_kind(
        &self,
        dir: &Path,
        title: Option<String>,
        kind: MeetingKind,
    ) -> Result<String, String> {
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
        let capture = engine.start(dir, kind)?;

        let title = title
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| default_title(Local::now()));
        let meeting = match self.db.create_meeting_with_kind(&title, "manual", kind) {
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
            "meeting {} started — kind {}, thermal {}, battery {}%{}, pill {}",
            meeting.id,
            kind.as_str(),
            thermal_at_start.as_str(),
            battery_at_start
                .percent
                .map(|p| p.to_string())
                .unwrap_or_else(|| "?".into()),
            if battery_at_start.on_ac { " (AC)" } else { "" },
            if pill_visible { "visible" } else { "hidden" }
        );

        let stop = Arc::new(TickerStop::default());
        let system_audio = capture.system_audio_probe();
        let ticker = self.spawn_ticker(
            meeting.id.clone(),
            meeting.title.clone(),
            Instant::now(),
            Arc::clone(&diagnostics),
            Arc::clone(&stop),
            system_audio.clone(),
        );

        *guard = Some(Active {
            id: meeting.id.clone(),
            title: meeting.title.clone(),
            started_at: meeting.started_at,
            started: Instant::now(),
            capture,
            system_audio,
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
        stop: Arc<TickerStop>,
        system_audio: Option<SystemAudioProbe>,
    ) -> JoinHandle<()> {
        let sink = Arc::clone(&self.sink);
        let probe = Arc::clone(&self.probe);
        let tick = self.tick;
        thread::Builder::new()
            .name("yap-meeting-tick".into())
            .spawn(move || {
                // `wait` is the tick AND the stop signal: it returns false the
                // moment `stop()` fires, so a user stop never waits out a tick.
                while stop.wait(tick) {
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
                        // YV110 — re-read every tick, not snapshotted at start:
                        // a tap lost at minute nine has to reach the pill on the
                        // next second, not at the end of the meeting.
                        system_audio: system_audio.as_ref().and_then(|probe| probe()),
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

        // Wake the ticker rather than waiting for it to notice. This runs on
        // the main thread whenever the user stops from the tray item or ⌃⌘M, and
        // a `join()` against a sleeping thread is a frozen menu bar for as long
        // as the sleep had left to run. See [`TickerStop`].
        active.stop.stop();
        if let Some(t) = active.ticker.take() {
            let _ = t.join();
        }

        let wall_seconds = active.started.elapsed().as_secs_f64();
        let outcome = active.capture.stop();
        let battery_at_end = self.probe.battery();

        let (duration, wav, sys_wav, tap_rebuilds, track_rates, note, state) = match outcome {
            Ok(o) => {
                let state = if o.wav_path.is_some() && !o.partial {
                    // Capture landed WHOLE. YV93's transcription pipeline moves
                    // it on to `complete`; until it has run, `transcribing` is
                    // what is actually true.
                    MeetingState::Transcribing
                } else {
                    // Nothing landed, or the engine says the recording is short.
                    // Either way `complete` would be a lie, and this is the one
                    // place that decision is made.
                    MeetingState::Partial
                };
                // The audio clock is the honest duration; fall back to the wall
                // clock only when the engine reports nothing.
                let secs = if o.seconds > 0.0 {
                    o.seconds
                } else {
                    wall_seconds
                };
                (
                    secs,
                    o.wav_path,
                    o.sys_wav_path,
                    o.tap_rebuilds,
                    o.track_rates,
                    o.note,
                    state,
                )
            }
            Err(e) => {
                log::warn!("meeting {} capture stop failed: {e}", active.id);
                (
                    wall_seconds,
                    None,
                    None,
                    None,
                    Vec::new(),
                    Some(e),
                    MeetingState::Partial,
                )
            }
        };

        if let Ok(mut d) = active.diagnostics.lock() {
            // YV107 / OS-2 — BEFORE `finish`, because `finish` is what stamps
            // the blob and the very next line is what writes it. A measurement
            // banked after the write is a measurement nobody ever reads.
            d.record_track_rates(&track_rates);
            d.finish(battery_at_end, reason);
            let _ = self.db.set_meeting_diagnostics(&active.id, &d.to_json());
        }
        self.db
            .finish_meeting(&active.id, duration, wav.as_deref())?;
        // YV106 — the second track and the tap's rebuild log, both written ONLY
        // when they exist. A mic-only meeting leaves both columns NULL, which is
        // the difference between "there was no second track" and "there was one
        // and it was empty".
        if sys_wav.is_some() {
            self.db
                .set_meeting_sys_wav_path(&active.id, sys_wav.as_deref())?;
        }
        if let Some(log) = tap_rebuilds.as_deref() {
            let _ = self.db.set_meeting_tap_rebuilds(&active.id, log);
        }
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
    ///
    /// The picker is skipped on this path (see [`MeetingController::start`]).
    pub fn toggle(&self, dir: &Path, title: Option<String>) -> Result<MeetingStatus, String> {
        self.toggle_with_kind(dir, title, MeetingKind::Unknown)
    }

    /// The same toggle, with the kind the user picked. `kind` is ignored on a
    /// STOP for the obvious reason: a meeting that is already running was
    /// started under whatever it was started under, and a picker press cannot
    /// retroactively change what was in the room.
    pub fn toggle_with_kind(
        &self,
        dir: &Path,
        title: Option<String>,
        kind: MeetingKind,
    ) -> Result<MeetingStatus, String> {
        if self.is_recording() {
            self.stop("user stopped")?;
        } else {
            self.start_with_kind(dir, title, kind)?;
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

// ───────────────────────── the real engine (YV91 → YV95) ────────────────────
//
// This is the one line the module docs above promised, made real. YV91 (#108)
// is merged, so `meeting::MeetingSession` exists and the control plane is no
// longer allowed to ship four disabled buttons —
// `tests/capture_engine_is_installed.rs` fails the build if `lib.rs::setup`
// does not install this.

/// The shipping [`CaptureEngine`]: YV91's `MeetingSession` behind YV95's seam,
/// and — since YV110 — YV100's system-audio tap alongside it.
///
/// It holds the database because the T-0 question "does this meeting attach
/// Track B?" is answered from YV102's `settings_kv` row, and that row is also
/// what the meeting rewrites on the way out when its own discriminator learns
/// something the 200 ms pre-warm could not.
pub struct SessionEngine {
    db: Arc<Database>,
}

impl SessionEngine {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// The T-0 decision this machine makes right now: YV101's runtime OS gate
    /// and YV102's setup row, through the one pure function that reads them.
    fn track_b_plan(&self) -> syscapture::TrackBPlan {
        syscapture::track_b_plan(
            crate::os_version_gate::system_audio_gate_now(),
            &self.db.system_audio_setup(),
        )
    }
}

/// Open the real CoreAudio tap and wrap it as the consumer half a meeting
/// drives. **This is `start_system_tap`'s first caller** — the wiring matrix
/// rows 1 and 2 have been published as missing since YV100 merged.
///
/// The 14.4 gate is the caller's (`SessionEngine::track_b_plan` above); this
/// checks nothing about the OS itself, because `CoreAudioPlatform::new` already
/// refuses on a machine with no process-tap API and a second copy of that check
/// would be a second thing to keep in step.
/// ## Why the tap is opened on a thread of its own
///
/// `imp::SystemTap` is **not `Send`**, and that is correct rather than
/// inconvenient: it owns the `RcBlock` registered with
/// `AudioDeviceCreateIOProcIDWithBlock` and the `DispatchQueue` behind it, both
/// of which are reference-counted by non-atomic ObjC machinery. A meeting's tap
/// has to be torn down from the stop path, which runs on the macOS main thread,
/// while the ring is drained from the pump thread — so *something* has to cross
/// a thread boundary.
///
/// What crosses is the ring (`Arc<RtCapture>`, `Send` by construction) and a
/// stop signal, never the CoreAudio graph: this thread creates the tap, hands
/// back the ring, parks, and performs the four teardown calls itself. The tap
/// object is created and destroyed on one thread and is never touched from
/// another, which is exactly the guarantee `!Send` is asking for — and it needs
/// no `unsafe impl`, of which this codebase has none and should keep having
/// none.
#[cfg(target_os = "macos")]
fn open_meeting_tap() -> Result<Arc<syscapture::MeetingTap>, syscapture::TapError> {
    use std::sync::mpsc;

    let (opened_tx, opened_rx) = mpsc::channel();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let handle = thread::Builder::new()
        .name("wv-meeting-system-tap".into())
        .spawn(move || match syscapture::imp::start_system_tap() {
            Ok(tap) => {
                let handed = opened_tx.send(Ok((Arc::clone(tap.capture()), tap.format())));
                if handed.is_ok() {
                    // Park until the meeting is over. A disconnected channel
                    // (the wiring panicked, the `MeetingTap` was dropped without
                    // a stop) is also a stop — the tap must not outlive its
                    // owner under any exit.
                    let _ = stop_rx.recv();
                }
                // The tap's OWN four calls, in `TEARDOWN_ORDER`, on the thread
                // that created it.
                tap.stop();
            }
            Err(error) => {
                let _ = opened_tx.send(Err(error));
            }
        })
        .map_err(|_| syscapture::TapError::ProcessTapApiUnavailable {
            symbol: "AudioHardwareCreateProcessTap",
        })?;

    let (ring, format) = match opened_rx.recv() {
        Ok(Ok(opened)) => opened,
        Ok(Err(error)) => {
            let _ = handle.join();
            return Err(error);
        }
        // The thread died without answering. Nothing was handed back, so there
        // is nothing to tear down here.
        Err(_) => {
            let _ = handle.join();
            return Err(syscapture::TapError::PanicDuringSetup {
                stage: syscapture::TapStage::CreateTap,
            });
        }
    };
    Ok(syscapture::MeetingTap::new(ring, format, move || {
        let _ = stop_tx.send(());
        // Joining is what makes `MeetingTap::stop` mean "the tap is down", not
        // "the tap has been asked" — the drain that follows it in
        // `TapPumpThread::finish` depends on the IOProc having actually stopped.
        let _ = handle.join();
    }))
}

/// Off macOS there is no process tap at all — the gate above never returns
/// `Attach`, and this exists so the wiring compiles on a Linux CI box.
#[cfg(not(target_os = "macos"))]
fn open_meeting_tap() -> Result<Arc<syscapture::MeetingTap>, syscapture::TapError> {
    Err(syscapture::TapError::ProcessTapApiUnavailable {
        symbol: "AudioHardwareCreateProcessTap",
    })
}

/// **The wiring, with the tap passed in.**
///
/// Split out of [`SessionEngine::start`] so the whole of it — two-track config,
/// the tap's own format announced to track 1, the pump thread, the teardown
/// order on the way out, the setup row rewritten from what the meeting actually
/// heard — is drivable from a test with a fake tap over the existing
/// `TapPlatform` seam. Everything above this line needs a 14.4 Mac and a TCC
/// grant; nothing below it does.
///
/// `plan` and `tap` are passed together rather than derived from each other
/// because they disagree in one real case: the plan said `Attach` and the tap
/// failed to open. That meeting is mic-only with an honest badge, which is
/// [`SessionEngine::start`]'s job to say and this function's job to record.
/// `stream` is the input this meeting holds open for its whole duration:
/// `MicStream` in production, and `ExternalStream` for a test that feeds blocks
/// by hand — the same split `meeting.rs`'s own lifecycle tests use, and the
/// reason a two-track test needs no microphone.
// Seven parameters, and each one is a thing the caller genuinely decides: where
// the audio goes, the mic's format, whether Track B attaches, the tap itself,
// where the permission verdict is written back, and which stream the meeting
// holds. Collapsing them into a struct would move the same list one line up.
#[allow(clippy::too_many_arguments)]
pub fn start_capture_session(
    dir: &Path,
    native_rate: u32,
    channels: u16,
    plan: syscapture::TrackBPlan,
    tap: Option<Arc<syscapture::MeetingTap>>,
    db: Option<Arc<Database>>,
    stream: Arc<dyn crate::meeting::CaptureStream>,
    kind: MeetingKind,
) -> Result<Box<dyn ActiveCapture>, String> {
    // `SessionConfig::virtual_meeting` and NOT `syscapture::virtual_meeting_config`:
    // that one swaps the held stream for `ExternalStream`, which is right for a
    // tap-only session and wrong here. A mic+tap meeting must keep holding the
    // microphone for its whole duration or `record.rs`'s worker closes an idle
    // stream after a minute and track 0 ends sixty seconds in —
    // `SessionConfig::virtual_meeting`'s own doc comment says so in as many
    // words, and it is the seam built for exactly this pair of producers.
    let mut config = if tap.is_some() {
        crate::meeting::SessionConfig::virtual_meeting(dir, native_rate, channels)
    } else {
        crate::meeting::SessionConfig::new(dir, native_rate, channels)
    };
    config.stream = stream;
    // YV125 — what the USER said, never what the tap implies. A hybrid meeting
    // opens the same tap as a call and must still cluster Track A.
    config.kind = kind;
    if let Some(tap) = tap.as_ref() {
        // The environment that finally answers `Some` to `tap_liveness`, which
        // is what puts YV104's ghost watchdog in effect inside a real meeting.
        config.env = Arc::new(syscapture::TappedEnv::new(
            dir.to_path_buf(),
            Arc::clone(tap),
        ));
    }
    let session = crate::meeting::MeetingSession::start(config).map_err(|e| e.to_string())?;
    let pump = tap.map(|tap| {
        // Every track starts on the MIC's format because that is the only one
        // known at `start`; the tap's own (`kAudioTapPropertyFormat`) is
        // announced here, before its first block, through the same epoch-banking
        // retune YV92 built for an AirPods swap.
        let format = tap.format();
        session.capture().retune_track(
            crate::meeting::SYSTEM_TRACK,
            format.sample_rate,
            format.channels,
        );
        syscapture::TapPumpThread::spawn(tap)
    });
    Ok(Box::new(SessionCapture {
        session: Some(session),
        pump,
        badge: plan.badge().map(str::to_string),
        db,
    }))
}

impl CaptureEngine for SessionEngine {
    fn start(&self, dir: &Path, kind: MeetingKind) -> Result<Box<dyn ActiveCapture>, String> {
        // Hold the input stream FIRST. This opens it if it is closed and blocks
        // until it is actually running, which is the only moment its true native
        // format is knowable — and a meeting whose resampler was configured from
        // a guessed rate is three hours of audio at the wrong speed.
        //
        // `MeetingSession::start` re-holds the same stream through its own
        // `MicStream`; the worker's hold is a flag, not a count, and the session
        // releases it on every path out, so this extra hold is balanced by the
        // session's own release rather than leaked.
        let format = crate::record::hold_stream_for_meeting()?;
        // The tap is opened BEFORE the session, so a tap that cannot open costs
        // a badge rather than a half-built meeting — and so the journal is
        // opened with the track count the meeting will actually record.
        let (plan, tap) = match self.track_b_plan() {
            syscapture::TrackBPlan::Attach => match open_meeting_tap() {
                Ok(tap) => (syscapture::TrackBPlan::Attach, Some(tap)),
                Err(error) => {
                    // Not a reason to refuse the meeting: 22-A recording is the
                    // product, the tap is the second half of it. The failure is
                    // recorded where the user can see it and where the next
                    // meeting will read it.
                    log::warn!("meeting system-audio tap could not open ({error}) — mic only");
                    if let Err(e) = self
                        .db
                        .record_system_audio_setup(crate::meetings::SetupVerdict::Failed)
                    {
                        log::warn!("could not record the system-audio setup verdict: {e}");
                    }
                    (
                        syscapture::TrackBPlan::MicOnly {
                            badge: syscapture::SETUP_FAILED_MESSAGE,
                        },
                        None,
                    )
                }
            },
            mic_only => (mic_only, None),
        };
        match start_capture_session(
            dir,
            format.sample_rate_hz.max(1),
            format.channels.max(1),
            plan,
            tap,
            Some(Arc::clone(&self.db)),
            Arc::new(crate::meeting::MicStream),
            kind,
        ) {
            Ok(capture) => Ok(capture),
            Err(e) => {
                crate::record::release_stream_for_meeting();
                Err(e)
            }
        }
    }
}

/// A live `MeetingSession`, seen through the control plane's narrow window.
struct SessionCapture {
    /// `Option` only so `stop` can move the session out of a `Box<Self>`.
    session: Option<crate::meeting::MeetingSession>,
    /// YV110 — the tap's drain thread, when this meeting has a Track B.
    pump: Option<syscapture::TapPumpThread>,
    /// The T-0 reason this meeting is mic-only, if it is.
    badge: Option<String>,
    /// Where the meeting's own permission discriminator is written back to.
    db: Option<Arc<Database>>,
}

impl ActiveCapture for SessionCapture {
    fn seconds(&self) -> f64 {
        self.session
            .as_ref()
            .map(|s| s.capture().elapsed().as_secs_f64())
            .unwrap_or(0.0)
    }

    fn system_audio_probe(&self) -> Option<SystemAudioProbe> {
        let badge = self.badge.clone();
        // The session's own handle, not a snapshot: the watchdog thread
        // republishes this log after every action, so a probe built once at
        // start still reads the verdict a degrade at minute nine produced.
        let log = self.session.as_ref()?.tap_rebuilds();
        Some(Arc::new(move || {
            // The watchdog's verdict wins over the T-0 badge: a meeting that
            // attached Track B and then lost it has nothing left to say about
            // setup, and "your microphone track is still recording" is the
            // sentence row 2 requires.
            if let Some(verdict) = log.lock().verdict() {
                return Some(verdict.banner().to_string());
            }
            badge.clone()
        }))
    }

    fn stop(mut self: Box<Self>) -> Result<CaptureOutcome, String> {
        // The tap goes first, and its own `finish` is what gets the order right:
        // park the pump, tear the tap down through `TEARDOWN_ORDER`, then drain
        // the last of the ring into the journal — which is still open, because
        // the session below has not been stopped yet.
        let mut tap_notes: Vec<String> = Vec::new();
        if let Some(mut pump) = self.pump.take() {
            pump.finish();
            let ran_for = pump.elapsed();
            let permission = pump.tap().permission(ran_for);
            if let Some(db) = self.db.as_ref() {
                // What the meeting HEARD, written back over what the 200 ms
                // pre-warm could only guess at. `Unknown` writes nothing: a
                // short meeting with a quiet room is not evidence of anything,
                // and overwriting a good row with it would lose a real grant.
                let verdict = match permission {
                    syscapture::SystemAudioPermission::Granted => {
                        Some(crate::meetings::SetupVerdict::Granted)
                    }
                    syscapture::SystemAudioPermission::LooksDenied => {
                        Some(crate::meetings::SetupVerdict::LooksDenied)
                    }
                    syscapture::SystemAudioPermission::Unknown => None,
                };
                if let Some(verdict) = verdict {
                    if let Err(e) = db.record_system_audio_setup(verdict) {
                        log::warn!("could not record the system-audio setup verdict: {e}");
                    }
                }
            }
            if permission.looks_denied() {
                tap_notes.push(syscapture::LOOKS_DENIED_MESSAGE.to_string());
            }
        } else if let Some(badge) = self.badge.clone() {
            tap_notes.push(badge);
        }
        let session = self
            .session
            .take()
            .ok_or_else(|| "the meeting capture was already stopped".to_string())?;
        // A watchdog stop is the engine's own verdict and it is read BEFORE the
        // finalize consumes the session, so the reason survives into the note.
        let watchdog = session.watchdog_stop();
        // YV104's log, for the same reason and at the same moment: the session
        // owns it and `stop()` consumes the session.
        let rebuilds = session.tap_rebuild_log();
        // Row 2's banner, persisted onto the meeting rather than only shown in
        // the pill for as long as the pill was open. It wins over the T-0 badge
        // for the same reason it does live: a meeting that lost the track has
        // nothing left to say about setup.
        if let Some(verdict) = rebuilds.verdict() {
            tap_notes.clear();
            tap_notes.push(verdict.banner().to_string());
        }
        let tap_rebuilds = if rebuilds.is_empty() {
            None
        } else {
            Some(rebuilds.to_json().to_string())
        };
        match session.stop() {
            Some(finalized) => {
                let partial = finalized.state != crate::meeting::MeetingState::Complete;
                let mut notes: Vec<String> = tap_notes;
                if let Some(reason) = watchdog {
                    notes.push(format!("stopped early: {reason}"));
                } else if partial {
                    notes.push(
                        "the recording ended early — part of this meeting is missing".to_string(),
                    );
                }
                if finalized.spliced_silence_samples > 0 {
                    notes.push(format!(
                        "{} of audio never reached disk and was spliced as silence",
                        meetings::format_offset(
                            finalized.spliced_silence_samples as f64 / 16_000.0
                        )
                    ));
                }
                Ok(CaptureOutcome {
                    // YV106 — BY TRACK, never by position. A two-track meeting
                    // whose mic never delivered finalizes with one wav, and
                    // `tracks.first()` would file the room as the user.
                    wav_path: finalized.wav_for_track(crate::meeting::MIC_TRACK).cloned(),
                    sys_wav_path: finalized
                        .wav_for_track(crate::meeting::SYSTEM_TRACK)
                        .cloned(),
                    tap_rebuilds,
                    track_rates: finalized.track_rates.clone(),
                    seconds: finalized.seconds,
                    note: if notes.is_empty() {
                        None
                    } else {
                        Some(notes.join("; "))
                    },
                    partial,
                })
            }
            // No journal to finalize: the session held a stream but nothing ever
            // reached disk. `partial` rather than an error, because the row must
            // still close and say why.
            None => Ok(CaptureOutcome {
                wav_path: None,
                sys_wav_path: None,
                tap_rebuilds,
                track_rates: Vec::new(),
                seconds: 0.0,
                note: Some("no audio reached disk for this meeting".to_string()),
                partial: true,
            }),
        }
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

    /// YV125 — the picker offers each kind exactly once, under an id nothing
    /// else in the tray claims, and "Not sure" is the same answer as skipping.
    #[test]
    fn the_kind_picker_covers_every_kind_once() {
        let kinds: Vec<MeetingKind> = KIND_PICKER.iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            vec![
                MeetingKind::InPerson,
                MeetingKind::Virtual,
                MeetingKind::Unknown
            ]
        );
        for c in KIND_PICKER {
            assert_eq!(kind_for_menu_id(c.menu_id), Some(c.kind));
            assert!(
                c.menu_id.starts_with("meeting_start_"),
                "{} must be namespaced so the tray match cannot collide",
                c.menu_id
            );
            assert!(!c.label.is_empty());
        }
        assert_eq!(kind_for_menu_id("meeting_toggle"), None);
        // The skip default and the last visible option are the same answer, so
        // a user who chooses "Not sure" and a user who never opened the submenu
        // land on the same branch.
        assert_eq!(KIND_PICKER[2].kind, MeetingKind::default());
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
