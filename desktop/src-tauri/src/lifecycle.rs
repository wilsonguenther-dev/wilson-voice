//! yap-lifecycle — what "open" and "quit" mean for a menu-bar app with a Dock
//! icon, decided in pure functions the run loop only dispatches.
//!
//! Three ways to bring the main window back exist and they all used to differ:
//! a Dock click / Finder double-click on a running Yap (`RunEvent::Reopen`,
//! which nothing handled — "I can't open the app"), a second launch caught by
//! the single-instance plugin, and the tray's "Open Yap". All three now ask
//! [`activation_action`] what to do and run ONE presenter in `lib.rs`.
//!
//! Three ways to quit exist too — Cmd-Q (app menu), the Dock's Quit and the
//! tray's "Quit Yap". All three end in `RunEvent::Exit`, and that arm runs ONE
//! teardown: [`quit_plan`] fixes the order (the pill is ordered out FIRST, so a
//! slow teardown never leaves it on screen) and [`run_bounded`] gives every step
//! a deadline inside a hard [`QUIT_BUDGET`], logging and skipping a step that
//! misses it instead of hanging the quit.
//!
//! The stale-instance probe ([`handoff_to_running`]) runs BEFORE the Tauri
//! builder: the plugin's own handoff is a one-way write followed by a silent
//! `exit(0)`, so a wedged first instance swallowed every relaunch. Ours waits
//! (bounded) for the running app to acknowledge that it actually presented its
//! window, and says so when it does not.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

// ─── Activation (Dock reopen / second instance / tray "Open Yap") ─────────

/// Who is asking for the main window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationSource {
    /// `RunEvent::Reopen` — a Dock click, a Finder double-click, or `open -a Yap`
    /// on an already-running Yap (LaunchServices re-activates the running
    /// process; no second process is ever started, which is why the
    /// single-instance callback almost never fires). `has_visible_windows` is
    /// AppKit's flag and it counts the floating pill — an NSPanel — as a visible
    /// window, so it is recorded but never trusted on its own.
    DockReopen { has_visible_windows: bool },
    /// The single-instance plugin handed us a second launch's argv.
    SecondInstance,
    /// The tray menu's "Open Yap" (and "Settings", which also navigates).
    TrayOpen,
}

/// What the main window looks like right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MainWindowState {
    pub exists: bool,
    pub visible: bool,
    pub minimized: bool,
    pub focused: bool,
}

/// What to do with the main window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAction {
    /// Nothing to do (or nothing that can be done: there is no window).
    Nothing,
    /// Show it, unminimize it when asked, and give it focus.
    Present { unminimize: bool },
}

/// The ONE decision for every activation path.
///
/// A window that is already on screen, unminimized and focused needs nothing.
/// Everything else — hidden by the red X, minimized to the Dock, or merely
/// behind another app — is presented. `has_visible_windows == true` does NOT
/// short-circuit a Dock reopen: with the pill on screen AppKit reports a
/// visible window while the main window is hidden, and acting on that flag
/// would reproduce the exact bug this module exists to fix.
pub fn activation_action(source: ActivationSource, w: MainWindowState) -> WindowAction {
    let _ = source;
    if !w.exists {
        return WindowAction::Nothing;
    }
    if w.visible && !w.minimized && w.focused {
        return WindowAction::Nothing;
    }
    WindowAction::Present {
        unminimize: w.minimized,
    }
}

// ─── The red X: hide, and say once where Yap went ────────────────────────

/// The event the main webview listens for to show the one-time hide hint.
pub const HIDE_HINT_EVENT: &str = "window_hidden_hint";

/// The marker that records the hint was shown. A file beside the history (the
/// data dir itself is never renamed), so the hint is once per install, not once
/// per launch.
pub fn hide_hint_marker(root: &Path) -> PathBuf {
    root.join(".hide-hint-shown")
}

/// Should this hide show the hint? Only the first one ever.
pub fn should_show_hide_hint(already_shown: bool) -> bool {
    !already_shown
}

/// Consume the one hint: `true` exactly once per marker, writing the marker.
/// A marker that cannot be written still returns `true` this once (the hint
/// is worth more than the bookkeeping) and is retried on the next hide.
pub fn take_hide_hint(root: &Path) -> bool {
    let marker = hide_hint_marker(root);
    if !should_show_hide_hint(marker.exists()) {
        return false;
    }
    if let Err(e) = std::fs::write(&marker, b"1\n") {
        log::warn!("hide hint: could not record {}: {e}", marker.display());
    }
    true
}

// ─── Quit: one plan, every step bounded ──────────────────────────────────

/// The whole quit, from the first step starting to the last one returning or
/// being abandoned.
pub const QUIT_BUDGET: Duration = Duration::from_millis(3_000);

/// One step of the quit teardown, in plan order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitStep {
    /// Order the floating pill out. Main thread, first, always.
    PillOrderOut,
    /// Finalize a live meeting so an hour of audio is never left rowless.
    MeetingStop,
    /// Cancel the in-flight decode and free the Metal device (YV69/YV70).
    AsrDrain,
    /// Let a drained dictation land its row.
    AwaitTake,
    /// Never leave the Mac muted (YV28).
    RestoreOutput,
    /// Flush the WAL into the .db.
    DbCheckpoint,
}

impl QuitStep {
    pub fn name(self) -> &'static str {
        match self {
            QuitStep::PillOrderOut => "pill_order_out",
            QuitStep::MeetingStop => "meeting_stop",
            QuitStep::AsrDrain => "asr_drain",
            QuitStep::AwaitTake => "await_take",
            QuitStep::RestoreOutput => "restore_output",
            QuitStep::DbCheckpoint => "db_checkpoint",
        }
    }

    /// Each step's own deadline. They sum to [`QUIT_BUDGET`] exactly, so even a
    /// quit where EVERY step times out ends inside the budget.
    pub fn deadline(self) -> Duration {
        Duration::from_millis(match self {
            QuitStep::PillOrderOut => 100,
            QuitStep::MeetingStop => 700,
            QuitStep::AsrDrain => 1_000,
            QuitStep::AwaitTake => 700,
            QuitStep::RestoreOutput => 200,
            QuitStep::DbCheckpoint => 300,
        })
    }

    /// Runs on the calling (main) thread rather than a worker: AppKit objects
    /// may only be touched there, and the run loop's Exit arm IS the main
    /// thread.
    pub fn on_main_thread(self) -> bool {
        matches!(self, QuitStep::PillOrderOut)
    }
}

/// The quit order. The pill goes first — before the meeting stop and the WAL
/// checkpoint — so whatever the rest costs, nothing is left hovering on screen
/// after the user said quit. The checkpoint is last so every row an earlier
/// step wrote is in it.
pub fn quit_plan() -> [QuitStep; 6] {
    [
        QuitStep::PillOrderOut,
        QuitStep::MeetingStop,
        QuitStep::AsrDrain,
        QuitStep::AwaitTake,
        QuitStep::RestoreOutput,
        QuitStep::DbCheckpoint,
    ]
}

/// A step ready to run under [`run_bounded`].
pub struct BoundedStep {
    pub name: &'static str,
    pub deadline: Duration,
    /// Run on the calling thread (unbounded by construction — keep it tiny).
    pub inline: bool,
    pub run: Box<dyn FnOnce() + Send + 'static>,
}

impl BoundedStep {
    pub fn new(
        name: &'static str,
        deadline: Duration,
        inline: bool,
        run: impl FnOnce() + Send + 'static,
    ) -> Self {
        Self {
            name,
            deadline,
            inline,
            run: Box::new(run),
        }
    }
}

/// How one step ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    Done,
    /// Missed its deadline; abandoned (its thread may still be running when the
    /// process exits).
    TimedOut,
    /// Never started: the overall budget was already spent.
    Skipped,
    /// The step panicked; contained so the rest of the quit still runs.
    Panicked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepReport {
    pub name: &'static str,
    pub elapsed: Duration,
    pub outcome: StepOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeardownReport {
    pub total: Duration,
    pub steps: Vec<StepReport>,
}

impl TeardownReport {
    /// The single log line: `exit teardown <ms>ms: name=<ms>ms … ` with a
    /// suffix on any step that did not finish.
    pub fn log_line(&self) -> String {
        let parts: Vec<String> = self
            .steps
            .iter()
            .map(|s| {
                let tag = match s.outcome {
                    StepOutcome::Done => "",
                    StepOutcome::TimedOut => "(timeout)",
                    StepOutcome::Skipped => "(skipped)",
                    StepOutcome::Panicked => "(panic)",
                };
                format!("{}={}ms{}", s.name, s.elapsed.as_millis(), tag)
            })
            .collect();
        format!(
            "exit teardown {}ms: {}",
            self.total.as_millis(),
            parts.join(" ")
        )
    }

    pub fn outcome_of(&self, name: &str) -> Option<StepOutcome> {
        self.steps
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.outcome)
    }
}

/// Run `steps` in order, each within `min(its deadline, what is left of
/// budget)`. A step that misses its deadline is logged, abandoned and the next
/// one starts; once the budget is spent the remaining steps are skipped. The
/// call returns within `budget` plus the cost of any inline step.
pub fn run_bounded(steps: Vec<BoundedStep>, budget: Duration) -> TeardownReport {
    let start = Instant::now();
    let mut reports = Vec::with_capacity(steps.len());
    for step in steps {
        let step_start = Instant::now();
        let left = budget.saturating_sub(start.elapsed());
        if left.is_zero() {
            log::warn!("exit: {} skipped — quit budget spent", step.name);
            reports.push(StepReport {
                name: step.name,
                elapsed: Duration::ZERO,
                outcome: StepOutcome::Skipped,
            });
            continue;
        }
        let outcome = if step.inline {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(step.run)) {
                Ok(()) => StepOutcome::Done,
                Err(_) => StepOutcome::Panicked,
            }
        } else {
            let wait = step.deadline.min(left);
            let (tx, rx) = mpsc::channel::<bool>();
            let run = step.run;
            let spawned = std::thread::Builder::new()
                .name(format!("yap-exit-{}", step.name))
                .spawn(move || {
                    let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).is_ok();
                    let _ = tx.send(ok);
                });
            match spawned {
                Err(e) => {
                    log::warn!("exit: {} could not start a thread: {e}", step.name);
                    StepOutcome::Skipped
                }
                Ok(_) => match rx.recv_timeout(wait) {
                    Ok(true) => StepOutcome::Done,
                    Ok(false) => StepOutcome::Panicked,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        log::warn!(
                            "exit: {} missed its {}ms deadline — skipped",
                            step.name,
                            wait.as_millis()
                        );
                        StepOutcome::TimedOut
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => StepOutcome::Panicked,
                },
            }
        };
        reports.push(StepReport {
            name: step.name,
            elapsed: step_start.elapsed(),
            outcome,
        });
    }
    TeardownReport {
        total: start.elapsed(),
        steps: reports,
    }
}

// ─── Stale instance: a handoff that must be acknowledged ─────────────────

/// How long a second launch waits for the running Yap to say it presented its
/// window before it reports a stale instance.
pub const HANDOFF_ACK_TIMEOUT: Duration = Duration::from_millis(2_000);
const HANDOFF_ACK_POLL: Duration = Duration::from_millis(20);

/// The single-instance plugin's socket for this bundle id — the same path the
/// plugin derives (`/tmp/<identifier with . and - as _>_si.sock`, no semver
/// suffix: the plugin's `semver` feature is off).
pub fn handoff_socket(identifier: &str) -> PathBuf {
    PathBuf::from(format!("/tmp/{}_si.sock", sanitize(identifier)))
}

/// The file the running instance touches after it has presented its window.
pub fn handoff_ack_file(identifier: &str) -> PathBuf {
    PathBuf::from(format!("/tmp/{}_si.ack", sanitize(identifier)))
}

fn sanitize(identifier: &str) -> String {
    identifier.replace(['.', '-'], "_")
}

/// The running instance's half: record that a handoff was served.
pub fn write_handoff_ack(ack: &Path) {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    if let Err(e) = std::fs::write(ack, format!("{} {}\n", std::process::id(), nanos)) {
        log::warn!(
            "single instance: could not write ack {}: {e}",
            ack.display()
        );
    }
}

/// What a launch found when it tried to hand off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffVerdict {
    /// No live listener: this process is the instance; launch normally.
    NoRunningInstance,
    /// The running Yap took the handoff and acknowledged it.
    Handed { ack_ms: u128 },
    /// The running Yap accepted the connection but never acknowledged.
    Stale { waited_ms: u128 },
    /// Something else went wrong talking to the socket; launch normally and let
    /// the plugin decide (it does the same on errors it does not recognize).
    Unknown(String),
}

/// Pure: classify a connect result.
pub fn classify_connect(err: &std::io::Error) -> HandoffVerdict {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::NotFound | ErrorKind::ConnectionRefused => HandoffVerdict::NoRunningInstance,
        _ => HandoffVerdict::Unknown(err.to_string()),
    }
}

fn ack_stamp(ack: &Path) -> Option<String> {
    std::fs::read_to_string(ack).ok()
}

/// Hand this launch's argv to a running Yap over the plugin's own wire format
/// (`cwd \0\0 args joined by \0`) and wait up to `timeout` for its ack file to
/// change. Never touches a GUI; safe to call with test paths.
pub fn handoff_to_running(
    socket: &Path,
    ack: &Path,
    cwd: &str,
    args: &[String],
    timeout: Duration,
) -> HandoffVerdict {
    let before = ack_stamp(ack);
    let mut stream = match UnixStream::connect(socket) {
        Ok(s) => s,
        Err(e) => return classify_connect(&e),
    };
    let mut payload = Vec::new();
    payload.extend_from_slice(cwd.as_bytes());
    payload.extend_from_slice(b"\0\0");
    payload.extend_from_slice(args.join("\0").as_bytes());
    if let Err(e) = stream.write_all(&payload).and_then(|_| stream.flush()) {
        return HandoffVerdict::Unknown(format!("write: {e}"));
    }
    drop(stream); // EOF: the listener's read_to_string returns
    let start = Instant::now();
    loop {
        let now = ack_stamp(ack);
        if now.is_some() && now != before {
            return HandoffVerdict::Handed {
                ack_ms: start.elapsed().as_millis(),
            };
        }
        if start.elapsed() >= timeout {
            return HandoffVerdict::Stale {
                waited_ms: start.elapsed().as_millis(),
            };
        }
        std::thread::sleep(HANDOFF_ACK_POLL);
    }
}

/// The bundle id the handoff socket and ack file are named after: the RUNNING
/// bundle's own id (`NSBundle.mainBundle`), falling back to the shipped id only
/// for an unbundled dev binary. Hard-coding the shipped id made a Yap Test
/// launch hand off to the PRODUCTION Yap's socket — production 0.9.2 never
/// writes the ack, so Yap Test reported itself "stale" and refused to start.
pub fn handoff_identifier() -> String {
    crate::perm_truth::main_bundle_id().unwrap_or_else(|| crate::BUNDLE_IDENTIFIER.to_string())
}

/// What a launch does with a [`HandoffVerdict`]. Pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffAction {
    /// Become the instance.
    Launch,
    /// The running Yap took it; this process exits 0.
    Exit,
    /// The holder accepted the connection but never answered within
    /// [`HANDOFF_ACK_TIMEOUT`]: it is dead to the user. Replace it — unlink its
    /// socket so this launch's single-instance plugin binds a fresh one — and
    /// launch, instead of refusing and leaving the user with no Yap at all.
    ReplaceDeadHolder,
}

pub fn handoff_action(verdict: &HandoffVerdict) -> HandoffAction {
    match verdict {
        HandoffVerdict::NoRunningInstance | HandoffVerdict::Unknown(_) => HandoffAction::Launch,
        HandoffVerdict::Handed { .. } => HandoffAction::Exit,
        HandoffVerdict::Stale { .. } => HandoffAction::ReplaceDeadHolder,
    }
}

/// The pid the last ack was written by, if the ack file names one.
pub fn ack_holder_pid(ack: &Path) -> Option<u32> {
    ack_stamp(ack)?.split_whitespace().next()?.parse().ok()
}

/// Unlink a dead holder's socket. `Ok(true)` = removed, `Ok(false)` = already
/// gone. The hung process keeps its (now unreachable) listener; every later
/// launch talks to the new instance.
pub fn replace_dead_holder(socket: &Path) -> std::io::Result<bool> {
    match std::fs::remove_file(socket) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// The log line for a replaced holder.
pub fn replaced_message(socket: &Path, waited_ms: u128, holder: Option<u32>) -> String {
    let who = holder
        .map(|p| format!(" (last ack from pid {p})"))
        .unwrap_or_default();
    format!(
        "the holder of {}{who} did not answer within {waited_ms} ms — replacing it: socket unlinked, this launch becomes the instance",
        socket.display()
    )
}

/// The words a stale handoff says, instead of the old silent exit.
pub fn stale_message(waited_ms: u128) -> String {
    format!(
        "Yap is already running but did not answer within {waited_ms} ms. \
         Quit it from the menu bar (or Activity Monitor) and open Yap again."
    )
}

// ─── RSS soak: the pure verdict the soak script also applies ─────────────

/// Did RSS grow more than `max_growth_pct` after the first `warmup` samples?
/// Compares the last sample against the sample at the end of warm-up.
pub fn rss_growth_exceeds(samples_kb: &[u64], warmup: usize, max_growth_pct: f64) -> bool {
    if samples_kb.len() <= warmup || warmup == 0 {
        return false;
    }
    let base = samples_kb[warmup - 1] as f64;
    let last = *samples_kb.last().unwrap() as f64;
    base > 0.0 && (last - base) / base * 100.0 > max_growth_pct
}
