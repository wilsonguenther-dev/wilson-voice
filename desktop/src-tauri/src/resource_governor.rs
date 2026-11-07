//! yap-1106-resource-governor — never run a local model on 5% battery or a
//! hot, starved Mac (absorbs YAP-FX-03).
//!
//! `meeting_energy` READS the thermal state and the battery, but only writes
//! them into a meeting's diagnostics; nothing decided anything from them, and
//! a 3.8 GB meeting decode happily ran on 4% battery. This module is the one
//! place that decides. It keeps a live [`Verdict`] built from:
//!
//!   * `ProcessInfo.thermalState` and the IOKit battery (shared with
//!     `meeting_energy`, through the `meeting_control::EnergyProbe` trait so a
//!     test can fake them),
//!   * `ProcessInfo.isLowPowerModeEnabled`,
//!   * the charger's wattage from `IOPSCopyExternalPowerAdapterDetails`,
//!   * memory pressure from a `DISPATCH_SOURCE_TYPE_MEMORYPRESSURE` source
//!     (normal / warn / critical), plus the process RSS for the log line.
//!
//! and sorts the Mac into three levels ([`Level`]):
//!
//!   * **full** — everything runs.
//!   * **light** — on battery at or under the "pause heavy work" percent
//!     (default 20) or under 15% whatever that setting says (FX-03's floor),
//!     on a charger under 45 W with the battery under 20%, Low Power Mode on,
//!     thermal serious, or memory warn.
//!   * **minimal** — on battery ≤ 10%, thermal critical, or memory critical.
//!
//! What each consumer does with it (the pure predicates below are what they
//! call; they never read the OS themselves):
//!
//!   * Dictation ASR ALWAYS runs. It is the product.
//!   * Polish ([`polish_plan`]): at light it goes to the cloud when a cloud
//!     polish is configured and online, otherwise it is skipped with the
//!     `saving_battery` note; at minimal it is skipped.
//!   * Meeting ASR, notes and diarization ([`Governor::heavy_work_deferred`])
//!     — and the meeting RECOVERY decode, which runs through the same
//!     pipeline queue — are deferred at light and at minimal, and queue until
//!     the Mac is back on wall power or at full. The meeting still RECORDS —
//!     capture is cheap (OS-12). FX-03 counts thermal-serious and memory-warn
//!     as constrained too: a 3.8 GB decode is exactly what starves a Mac that
//!     is already short of memory, so it waits for full like the rest.
//!   * Dictation keeps exactly ONE engine while constrained
//!     ([`keeps_one_engine`]): the warm polish sidecar is unloaded on the way
//!     into light/minimal, leaving only the ASR model resident.
//!   * The live transcript ([`Governor::live_transcript_paused`]) pauses at
//!     minimal.
//!
//! "Run anyway" is a per-job override ([`Governor::run_anyway`]): the user
//! can push one meeting's transcription through on 8% battery, and that
//! choice does not leak to the next meeting.
//!
//! Every transition logs ONE line:
//! `governor level=<from>-><to> reason=<battery|lowpower|thermal|memory|adapter_<W>w_battery_<pct>> value=…`.
//!
//! Polling: the OS readings are re-read on every consumer ask (four cheap
//! calls — a decode is minutes long and a meeting queue is asked once per
//! job), on a 30 s background tick for the UI (the app's floor for polling),
//! and immediately when the memory-pressure source fires. The
//! `NSProcessInfoPowerStateDidChange` notification would need a block-based
//! observer for a fact the 30 s tick learns anyway; the 1 Hz meeting tick
//! already made that call in `meeting_energy`, and it holds here.
//!
//! Everything degrades to **full** rather than refusing work: a governor that
//! cannot read the battery must not hold a meeting's notes hostage.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::meeting_control::EnergyProbe;
use crate::meeting_energy::{BatteryReading, ThermalState};

/// The three budgets. Ordered: `Full < Light < Minimal` in severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Full,
    Light,
    Minimal,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Full => "full",
            Level::Light => "light",
            Level::Minimal => "minimal",
        }
    }
}

/// `DISPATCH_SOURCE_TYPE_MEMORYPRESSURE`'s three states, plus "never heard".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryPressure {
    Normal,
    Warn,
    Critical,
    Unknown,
}

impl MemoryPressure {
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryPressure::Normal => "normal",
            MemoryPressure::Warn => "warn",
            MemoryPressure::Critical => "critical",
            MemoryPressure::Unknown => "unknown",
        }
    }
}

/// Everything the verdict is decided from. Pure data, so [`level_for`] is a
/// table-driven unit test and never a Mac with a flat battery.
#[derive(Debug, Clone, PartialEq)]
pub struct Readings {
    pub thermal: ThermalState,
    pub battery: BatteryReading,
    pub low_power: bool,
    pub memory: MemoryPressure,
    /// The charger's rated wattage, when one is plugged in and IOKit knows.
    pub adapter_watts: Option<u32>,
    /// Process resident set in MB — written into the log line, never a
    /// reason on its own (the kernel's pressure signal is the honest one).
    pub rss_mb: f64,
}

impl Default for Readings {
    fn default() -> Self {
        Readings {
            thermal: ThermalState::Unknown,
            battery: BatteryReading::default(),
            low_power: false,
            memory: MemoryPressure::Unknown,
            adapter_watts: None,
            rss_mb: 0.0,
        }
    }
}

impl Readings {
    /// Draining the internal battery right now.
    pub fn on_battery(&self) -> bool {
        self.battery.has_battery && !self.battery.on_ac
    }
}

/// Battery percent at or under which the Mac is `minimal` on battery.
pub const MINIMAL_BATTERY_PERCENT: u8 = 10;
/// Default for the "Pause heavy work on battery below N%" setting.
pub const DEFAULT_PAUSE_BELOW_PERCENT: u8 = 20;
/// FX-03: on battery under this percent is light whatever the user's
/// "pause heavy work" setting says. The setting can raise the line, never
/// lower it under this floor.
pub const CONSTRAINED_BATTERY_PERCENT: u8 = 15;
/// A charger under this wattage cannot keep up with a decode (FX-03).
pub const WEAK_ADAPTER_WATTS: u32 = 45;
/// …and only matters while the battery is under this.
pub const WEAK_ADAPTER_BATTERY_PERCENT: u8 = 20;
/// The UI tick. Never under the app's 30 s polling floor.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(30);
/// How often a deferred queue asks again.
pub const DEFER_TICK: Duration = Duration::from_secs(1);

/// The decision: a level and the one reason that put it there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub level: Level,
    /// `battery`, `lowpower`, `thermal`, `memory`, or `adapter_<W>w_battery_<pct>`.
    /// `None` at full.
    pub reason: Option<String>,
    /// The reading behind the reason: `4%`, `on`, `critical`, `warn`, `20w`.
    pub value: Option<String>,
    /// Was the reason a POWER reason (battery, Low Power Mode, weak charger)?
    /// Only the pill wording reads it ("Saving battery" vs "Running hot").
    pub power_limited: bool,
}

impl Verdict {
    pub fn full() -> Self {
        Verdict {
            level: Level::Full,
            reason: None,
            value: None,
            power_limited: false,
        }
    }
    fn of(level: Level, reason: impl Into<String>, value: impl Into<String>, power: bool) -> Self {
        Verdict {
            level,
            reason: Some(reason.into()),
            value: Some(value.into()),
            power_limited: power,
        }
    }
}

/// The pure decision. `pause_below` is the user's "Pause heavy work on battery
/// below N%" (percent, clamped 1..=99 by the setter).
///
/// Most severe first, so a 4% battery that is ALSO thermally serious says
/// `minimal reason=battery` rather than `light reason=thermal`.
pub fn level_for(r: &Readings, pause_below: u8) -> Verdict {
    let pct = r.battery.percent;
    // ── minimal ──
    if r.on_battery() {
        if let Some(p) = pct {
            if p <= MINIMAL_BATTERY_PERCENT.min(pause_below) {
                return Verdict::of(Level::Minimal, "battery", format!("{p}%"), true);
            }
        }
    }
    if r.thermal == ThermalState::Critical {
        return Verdict::of(Level::Minimal, "thermal", "critical", false);
    }
    if r.memory == MemoryPressure::Critical {
        return Verdict::of(Level::Minimal, "memory", "critical", false);
    }
    // ── light ──
    if r.on_battery() {
        if let Some(p) = pct {
            if p <= pause_below || p < CONSTRAINED_BATTERY_PERCENT {
                return Verdict::of(Level::Light, "battery", format!("{p}%"), true);
            }
        }
    }
    if r.low_power {
        return Verdict::of(Level::Light, "lowpower", "on", true);
    }
    if r.battery.has_battery && r.battery.on_ac {
        if let (Some(w), Some(p)) = (r.adapter_watts, pct) {
            if w < WEAK_ADAPTER_WATTS && p < WEAK_ADAPTER_BATTERY_PERCENT {
                return Verdict::of(
                    Level::Light,
                    format!("adapter_{w}w_battery_{p}"),
                    format!("{w}w"),
                    true,
                );
            }
        }
    }
    if r.thermal == ThermalState::Serious {
        return Verdict::of(Level::Light, "thermal", "serious", false);
    }
    if r.memory == MemoryPressure::Warn {
        return Verdict::of(Level::Light, "memory", "warn", false);
    }
    Verdict::full()
}

/// What the polish stage does at a level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolishPlan {
    /// Run the local model.
    Local,
    /// Light, and a cloud polish is configured and online: use it.
    Cloud,
    /// Skip; the take ships as plain text with the `saving_battery` note.
    Skip,
}

/// Polish at `level`. `cloud_available` is "a cloud polish is on AND online"
/// (BYOM item 22) — `false` until that lands, so light skips.
pub fn polish_plan(level: Level, cloud_available: bool) -> PolishPlan {
    match level {
        Level::Full => PolishPlan::Local,
        Level::Light if cloud_available => PolishPlan::Cloud,
        Level::Light | Level::Minimal => PolishPlan::Skip,
    }
}

/// The closed-set polish skip tag the take records (see `polish::note_polish_skip`).
pub const POLISH_SKIP_TAG: &str = "saving_battery";

/// Does `verdict` hold heavy work (meeting ASR, notes, diarization) back?
/// `Some(reason)` when it does.
pub fn heavy_work_deferred_by(verdict: &Verdict) -> Option<String> {
    match verdict.level {
        Level::Full => None,
        Level::Light | Level::Minimal => verdict.reason.clone(),
    }
}

/// FX-03: "dictation keeps exactly one engine" while constrained. `true` at
/// light and minimal — the caller unloads every warm model that is not the
/// dictation ASR (today: the polish sidecar; notes and diarization are
/// already deferred, so they load nothing new).
pub fn keeps_one_engine(level: Level) -> bool {
    level != Level::Full
}

/// The live transcript pauses at minimal only.
pub fn live_transcript_paused_at(level: Level) -> bool {
    level == Level::Minimal
}

// ──────────────────────────────── the probe ────────────────────────────────

/// Where the readings come from. The real one reads the OS; tests hand in a
/// [`FakeProbe`].
pub trait ResourceProbe: Send + Sync + 'static {
    fn readings(&self) -> Readings;
}

/// The real probe: thermal + battery through the meeting's `EnergyProbe`
/// (shared readers, one truth), the rest from this module's own readers.
pub struct SystemResourceProbe {
    pub energy: Arc<dyn EnergyProbe>,
}

impl ResourceProbe for SystemResourceProbe {
    fn readings(&self) -> Readings {
        Readings {
            thermal: self.energy.thermal(),
            battery: self.energy.battery(),
            low_power: low_power_mode(),
            memory: memory_pressure(),
            adapter_watts: adapter_watts(),
            rss_mb: LAST_RSS_MB.lock().map(|v| *v).unwrap_or(0.0),
        }
    }
}

/// A probe whose readings a test sets. Implements `EnergyProbe` too, so the
/// meeting controller's fakes and the governor's are the same object.
#[derive(Clone, Default)]
pub struct FakeProbe(pub Arc<Mutex<Readings>>);

impl FakeProbe {
    pub fn new(r: Readings) -> Self {
        FakeProbe(Arc::new(Mutex::new(r)))
    }
    pub fn set(&self, r: Readings) {
        if let Ok(mut g) = self.0.lock() {
            *g = r;
        }
    }
}

impl ResourceProbe for FakeProbe {
    fn readings(&self) -> Readings {
        self.0.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

impl EnergyProbe for FakeProbe {
    fn thermal(&self) -> ThermalState {
        self.readings().thermal
    }
    fn battery(&self) -> BatteryReading {
        self.readings().battery
    }
    fn pill_visible(&self) -> bool {
        false
    }
}

// ─────────────────────────────── the governor ──────────────────────────────

/// What the UI reads: the pill chip, Settings › General's sentence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GovernorView {
    pub level: Level,
    pub reason: Option<String>,
    pub battery_percent: Option<u8>,
    pub on_ac: bool,
    pub low_power: bool,
    pub thermal: ThermalState,
    pub memory: MemoryPressure,
    pub adapter_watts: Option<u32>,
    pub pause_below_percent: u8,
    /// Settings › General: "Yap is going easy: 9% battery". `None` at full.
    pub sentence: Option<String>,
    /// The pill's small state: "Saving battery: notes after you plug in".
    pub pill_label: Option<String>,
    /// The main window's banner (FX-03): "Yap is saving power — your meeting
    /// will transcribe when you're charged". `None` unless heavy work is held.
    pub main_sentence: Option<String>,
    /// Is heavy work (meeting ASR, notes, diarization) being held?
    pub heavy_work_deferred: bool,
    /// The jobs currently waiting (meeting ids, `notes:<id>`, `diarize`).
    pub waiting_jobs: Vec<String>,
}

/// The one sentence for Settings. Plain words, the reading in them.
pub fn sentence_for(v: &Verdict) -> Option<String> {
    let reason = v.reason.as_deref()?;
    let value = v.value.as_deref().unwrap_or("");
    Some(match reason {
        "battery" => format!("Yap is going easy: {value} battery"),
        "lowpower" => "Yap is going easy: Low Power Mode is on".to_string(),
        "thermal" => "Yap is going easy: this Mac is running hot".to_string(),
        "memory" => "Yap is going easy: this Mac is low on memory".to_string(),
        _ if reason.starts_with("adapter_") => {
            format!("Yap is going easy: a {value} charger can't keep up")
        }
        _ => "Yap is going easy on this Mac".to_string(),
    })
}

/// The pill's chip. Says what is deferred, in one short clause.
pub fn pill_label_for(v: &Verdict) -> Option<String> {
    let reason = v.reason.as_deref()?;
    let power = v.power_limited;
    Some(match (v.level, reason) {
        (Level::Minimal, "battery") => "Saving battery: notes after you plug in".into(),
        (Level::Minimal, "thermal") => "Cooling down: notes when this Mac cools".into(),
        (Level::Minimal, "memory") => "Low on memory: notes when it frees up".into(),
        (Level::Light, r) if r.starts_with("adapter_") => {
            "Slow charger: notes when the battery is up".into()
        }
        (Level::Light, _) if power => "Saving battery: notes after you plug in".into(),
        (Level::Light, "thermal") => "Running hot: notes when this Mac cools".into(),
        (Level::Light, "memory") => "Low on memory: notes when it frees up".into(),
        _ => "Yap is going easy on this Mac".into(),
    })
}

/// The main window's one plain sentence (FX-03) while a meeting's
/// transcription is being held. `None` when nothing heavy is held.
pub fn main_sentence_for(v: &Verdict) -> Option<String> {
    let reason = heavy_work_deferred_by(v)?;
    Some(match reason.as_str() {
        "thermal" => {
            "Yap is letting this Mac cool down — your meeting will transcribe when it does".into()
        }
        "memory" => {
            "Yap is saving memory — your meeting will transcribe when this Mac frees some up".into()
        }
        _ => "Yap is saving power — your meeting will transcribe when you're charged".into(),
    })
}

struct Inner {
    verdict: Verdict,
    readings: Readings,
    pause_below: u8,
    /// Per-job "Run anyway" overrides. Keyed by the job's id (a meeting id,
    /// `notes:<id>`, `diarize:<id>`); cleared once the Mac is back at full.
    overrides: HashSet<String>,
    /// Jobs that asked and were told to wait — what "Run anyway" acts on.
    waiting: Vec<String>,
    last_read: Option<Instant>,
}

type Emitter = Box<dyn Fn(&GovernorView) + Send + Sync>;

/// The live budget. One per process; consumers reach it through
/// [`installed`] / the free functions below, tests build their own.
pub struct Governor {
    probe: Box<dyn ResourceProbe>,
    inner: Mutex<Inner>,
    emit: Mutex<Option<Emitter>>,
}

impl Governor {
    pub fn new(probe: impl ResourceProbe, pause_below: u8) -> Self {
        Governor {
            probe: Box::new(probe),
            inner: Mutex::new(Inner {
                verdict: Verdict::full(),
                readings: Readings::default(),
                pause_below: clamp_pause_below(pause_below),
                overrides: HashSet::new(),
                waiting: Vec::new(),
                last_read: None,
            }),
            emit: Mutex::new(None),
        }
    }

    /// Where a changed view goes (the Tauri `governor` event in the app).
    pub fn set_emitter(&self, f: impl Fn(&GovernorView) + Send + Sync + 'static) {
        if let Ok(mut e) = self.emit.lock() {
            *e = Some(Box::new(f));
        }
    }

    /// Re-read the probe and re-decide. Logs the transition line when the
    /// level or the reason changed, and emits the view. Returns the verdict.
    pub fn refresh(&self) -> Verdict {
        let readings = self.probe.readings();
        let (verdict, changed, view) = {
            let Ok(mut g) = self.inner.lock() else {
                return Verdict::full();
            };
            let verdict = level_for(&readings, g.pause_below);
            let changed = verdict != g.verdict;
            if changed {
                log::info!(
                    "governor level={}->{} reason={} value={} battery={} on_ac={} lowpower={} thermal={} memory={} rss_mb={:.0}",
                    g.verdict.level.as_str(),
                    verdict.level.as_str(),
                    verdict.reason.as_deref().unwrap_or("none"),
                    verdict.value.as_deref().unwrap_or("-"),
                    readings
                        .battery
                        .percent
                        .map(|p| format!("{p}%"))
                        .unwrap_or_else(|| "?".into()),
                    readings.battery.on_ac,
                    readings.low_power,
                    readings.thermal.as_str(),
                    readings.memory.as_str(),
                    readings.rss_mb,
                );
                crate::diagnostics::set_state("governor.level", verdict.level.as_str());
                if verdict.level == Level::Full && !g.overrides.is_empty() {
                    log::info!("governor overrides_cleared count={}", g.overrides.len());
                    g.overrides.clear();
                }
            }
            g.verdict = verdict.clone();
            g.readings = readings;
            g.last_read = Some(Instant::now());
            let view = Self::view_of(&g);
            (verdict, changed, view)
        };
        if changed {
            if let Ok(e) = self.emit.lock() {
                if let Some(f) = e.as_ref() {
                    f(&view);
                }
            }
        }
        verdict
    }

    /// The current verdict, re-read when the last read is older than `max_age`.
    pub fn verdict(&self, max_age: Duration) -> Verdict {
        let fresh = self
            .inner
            .lock()
            .ok()
            .and_then(|g| g.last_read.map(|t| t.elapsed() <= max_age))
            .unwrap_or(false);
        if fresh {
            self.inner
                .lock()
                .map(|g| g.verdict.clone())
                .unwrap_or_else(|_| Verdict::full())
        } else {
            self.refresh()
        }
    }

    pub fn level(&self) -> Level {
        self.verdict(Duration::from_secs(5)).level
    }

    /// Heavy work for `job` is held: `Some(reason)`. `None` when it may run,
    /// including when the user pressed "Run anyway" for this job.
    pub fn heavy_work_deferred(&self, job: &str) -> Option<String> {
        let verdict = self.verdict(Duration::from_secs(5));
        let held = heavy_work_deferred_by(&verdict).filter(|_| {
            !self
                .inner
                .lock()
                .map(|g| g.overrides.contains(job))
                .unwrap_or(false)
        });
        // Track who is waiting so the UI can offer "Run anyway" per job.
        if let Ok(mut g) = self.inner.lock() {
            let idx = g.waiting.iter().position(|w| w == job);
            match (held.is_some(), idx) {
                (true, None) => g.waiting.push(job.to_string()),
                (false, Some(i)) => {
                    g.waiting.remove(i);
                }
                _ => {}
            }
        }
        held
    }

    pub fn live_transcript_paused(&self) -> bool {
        live_transcript_paused_at(self.level())
    }

    pub fn polish_plan(&self, cloud_available: bool) -> PolishPlan {
        polish_plan(self.level(), cloud_available)
    }

    /// "Run anyway" for one job. Logged; the override dies at the next full.
    pub fn run_anyway(&self, job: &str) {
        if let Ok(mut g) = self.inner.lock() {
            g.overrides.insert(job.to_string());
            log::info!(
                "governor run_anyway job={} level={} reason={}",
                job_basename(job),
                g.verdict.level.as_str(),
                g.verdict.reason.as_deref().unwrap_or("none")
            );
        }
    }

    /// The "Pause heavy work on battery below N%" setting. Re-decides at once.
    pub fn set_pause_below(&self, percent: u8) -> GovernorView {
        if let Ok(mut g) = self.inner.lock() {
            let next = clamp_pause_below(percent);
            if next != g.pause_below {
                log::info!("governor pause_below {}->{}", g.pause_below, next);
            }
            g.pause_below = next;
        }
        self.refresh();
        self.view()
    }

    pub fn pause_below(&self) -> u8 {
        self.inner
            .lock()
            .map(|g| g.pause_below)
            .unwrap_or(DEFAULT_PAUSE_BELOW_PERCENT)
    }

    pub fn view(&self) -> GovernorView {
        self.inner
            .lock()
            .map(|g| Self::view_of(&g))
            .unwrap_or_else(|_| {
                Self::view_of_parts(
                    &Verdict::full(),
                    &Readings::default(),
                    DEFAULT_PAUSE_BELOW_PERCENT,
                )
            })
    }

    fn view_of(g: &Inner) -> GovernorView {
        let mut v = Self::view_of_parts(&g.verdict, &g.readings, g.pause_below);
        v.waiting_jobs = g.waiting.clone();
        v
    }

    fn view_of_parts(v: &Verdict, r: &Readings, pause_below: u8) -> GovernorView {
        GovernorView {
            level: v.level,
            reason: v.reason.clone(),
            battery_percent: r.battery.percent,
            on_ac: r.battery.on_ac,
            low_power: r.low_power,
            thermal: r.thermal,
            memory: r.memory,
            adapter_watts: r.adapter_watts,
            pause_below_percent: pause_below,
            sentence: sentence_for(v),
            pill_label: pill_label_for(v),
            main_sentence: main_sentence_for(v),
            heavy_work_deferred: heavy_work_deferred_by(v).is_some(),
            waiting_jobs: Vec::new(),
        }
    }
}

/// 1..=99 — 0 would never pause, 100 would always.
pub fn clamp_pause_below(percent: u8) -> u8 {
    percent.clamp(1, 99)
}

/// A meeting id is a uuid; a `notes:<uuid>` key is not a path — but keep the
/// log rule (basename only) honest for any caller that passes one.
fn job_basename(job: &str) -> &str {
    job.rsplit('/').next().unwrap_or(job)
}

// ──────────────────────────── the installed one ─────────────────────────────

/// The Tauri event a changed view is emitted on (pill + main window).
pub const GOVERNOR_EVENT: &str = "governor";

static INSTALLED: OnceLock<Arc<Governor>> = OnceLock::new();

/// Make `g` the process-wide governor. First call wins.
pub fn install(g: Arc<Governor>) -> bool {
    INSTALLED.set(g).is_ok()
}

pub fn installed() -> Option<&'static Arc<Governor>> {
    INSTALLED.get()
}

/// Heavy work for `job` is held, per the installed governor. `None` (run)
/// when none is installed — a test or the CLI without a governor is `full`.
pub fn heavy_work_deferred(job: &str) -> Option<String> {
    INSTALLED.get().and_then(|g| g.heavy_work_deferred(job))
}

pub fn live_transcript_paused() -> bool {
    INSTALLED.get().is_some_and(|g| g.live_transcript_paused())
}

pub fn polish_plan_now(cloud_available: bool) -> PolishPlan {
    INSTALLED
        .get()
        .map(|g| g.polish_plan(cloud_available))
        .unwrap_or(PolishPlan::Local)
}

pub fn level_now() -> Level {
    INSTALLED.get().map(|g| g.level()).unwrap_or(Level::Full)
}

/// The installed governor's view (re-read when stale); a full view when none.
pub fn view_now() -> GovernorView {
    match INSTALLED.get() {
        Some(g) => {
            g.verdict(Duration::from_secs(5));
            g.view()
        }
        None => Governor::view_of_parts(
            &Verdict::full(),
            &Readings::default(),
            DEFAULT_PAUSE_BELOW_PERCENT,
        ),
    }
}

/// Start the 30 s UI tick on the installed governor, plus the memory-pressure
/// source. Call once, after [`install`].
pub fn start_background(quit: Arc<dyn Fn() -> bool + Send + Sync>) {
    let Some(g) = INSTALLED.get() else {
        return;
    };
    install_memory_pressure_source();
    let g = Arc::clone(g);
    let spawned = std::thread::Builder::new()
        .name("wv-resource-governor".into())
        .spawn(move || {
            loop {
                if let Ok(mut v) = LAST_RSS_MB.lock() {
                    *v = crate::hygiene::resident_mb();
                }
                g.refresh();
                let mut waited = Duration::ZERO;
                while waited < REFRESH_INTERVAL {
                    if quit() {
                        return;
                    }
                    // The memory-pressure handler flags; this tick acts.
                    if MEMORY_EVENT.swap(0, Ordering::SeqCst) != 0 {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(500));
                    waited += Duration::from_millis(500);
                }
            }
        });
    if let Err(e) = spawned {
        log::error!("governor failure reason=spawn {e}");
    }
}

static LAST_RSS_MB: Mutex<f64> = Mutex::new(0.0);

// ───────────────────────────────── readers ──────────────────────────────────

/// `ProcessInfo.isLowPowerModeEnabled`.
#[cfg(target_os = "macos")]
pub fn low_power_mode() -> bool {
    objc2_foundation::NSProcessInfo::processInfo().isLowPowerModeEnabled()
}

#[cfg(not(target_os = "macos"))]
pub fn low_power_mode() -> bool {
    false
}

/// The charger's rated wattage (`IOPSCopyExternalPowerAdapterDetails`,
/// `kIOPSPowerAdapterWattsKey`). `None` on battery, with no battery, or when
/// IOKit does not know.
#[cfg(target_os = "macos")]
pub fn adapter_watts() -> Option<u32> {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPSCopyExternalPowerAdapterDetails() -> CFDictionaryRef;
    }
    unsafe {
        let d = IOPSCopyExternalPowerAdapterDetails();
        if d.is_null() {
            return None;
        }
        // Copy rule: we own it; the wrapper releases on drop.
        let dict: CFDictionary<CFString, CFType> = CFDictionary::wrap_under_create_rule(d);
        let key = CFString::from_static_string("Watts");
        dict.find(&key)
            .and_then(|v| v.downcast::<CFNumber>())
            .and_then(|n| n.to_i64())
            .filter(|w| *w > 0)
            .map(|w| w as u32)
    }
}

#[cfg(not(target_os = "macos"))]
pub fn adapter_watts() -> Option<u32> {
    None
}

/// The last memory-pressure state the dispatch source reported.
/// 0 = never heard, 1 = normal, 2 = warn, 4 = critical (the dispatch flags).
static MEMORY_STATE: AtomicU8 = AtomicU8::new(0);
/// Set by the handler, consumed by the background tick.
static MEMORY_EVENT: AtomicU8 = AtomicU8::new(0);

pub fn memory_pressure() -> MemoryPressure {
    memory_pressure_from_flags(MEMORY_STATE.load(Ordering::SeqCst) as u64)
}

/// `DISPATCH_MEMORYPRESSURE_*` flags → state. Pure, so the mapping is tested.
pub fn memory_pressure_from_flags(flags: u64) -> MemoryPressure {
    if flags & 0x4 != 0 {
        MemoryPressure::Critical
    } else if flags & 0x2 != 0 {
        MemoryPressure::Warn
    } else if flags & 0x1 != 0 {
        MemoryPressure::Normal
    } else {
        MemoryPressure::Unknown
    }
}

/// Test seam / handler target: record a flags word.
pub fn note_memory_flags(flags: u64) {
    MEMORY_STATE.store((flags & 0x7) as u8, Ordering::SeqCst);
    MEMORY_EVENT.store(1, Ordering::SeqCst);
}

#[cfg(target_os = "macos")]
fn install_memory_pressure_source() {
    use std::ffi::c_void;
    use std::sync::atomic::AtomicBool;

    static SOURCE_INSTALLED: AtomicBool = AtomicBool::new(false);
    if SOURCE_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }

    #[repr(C)]
    struct DispatchSourceType {
        _private: [u8; 0],
    }
    type DispatchSourceT = *mut c_void;
    type DispatchQueueT = *mut c_void;

    extern "C" {
        static _dispatch_source_type_memorypressure: DispatchSourceType;
        fn dispatch_source_create(
            kind: *const DispatchSourceType,
            handle: usize,
            mask: usize,
            queue: DispatchQueueT,
        ) -> DispatchSourceT;
        fn dispatch_source_set_event_handler_f(
            source: DispatchSourceT,
            handler: extern "C" fn(*mut c_void),
        );
        fn dispatch_set_context(object: *mut c_void, context: *mut c_void);
        fn dispatch_source_get_data(source: DispatchSourceT) -> usize;
        fn dispatch_resume(object: *mut c_void);
        fn dispatch_get_global_queue(identifier: isize, flags: usize) -> DispatchQueueT;
    }

    extern "C" fn on_memory_pressure(ctx: *mut c_void) {
        // The context IS the source; the data is the flags word.
        let flags = unsafe { dispatch_source_get_data(ctx) } as u64;
        note_memory_flags(flags);
        log::info!(
            "governor memory_pressure state={}",
            memory_pressure_from_flags(flags).as_str()
        );
    }

    // QOS_CLASS_UTILITY = 0x11: a background reading, never the main thread.
    unsafe {
        let queue = dispatch_get_global_queue(0x11, 0);
        let source = dispatch_source_create(
            &_dispatch_source_type_memorypressure,
            0,
            0x1 | 0x2 | 0x4,
            queue,
        );
        if source.is_null() {
            log::warn!("governor memory_pressure failure reason=source_create");
            return;
        }
        dispatch_set_context(source, source);
        dispatch_source_set_event_handler_f(source, on_memory_pressure);
        dispatch_resume(source);
        // Deliberately leaked: the source lives as long as the process.
    }
    // Until the kernel says otherwise, assume normal — "unknown" never
    // decides anything, but a reader of the view should see a state.
    if MEMORY_STATE.load(Ordering::SeqCst) == 0 {
        MEMORY_STATE.store(1, Ordering::SeqCst);
    }
    log::info!("governor memory_pressure source=installed");
}

#[cfg(not(target_os = "macos"))]
fn install_memory_pressure_source() {}

// ───────────────────────────────── tests ────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn batt(on_ac: bool, pct: u8) -> BatteryReading {
        BatteryReading {
            on_ac,
            percent: Some(pct),
            minutes_remaining: None,
            has_battery: true,
        }
    }

    fn r(on_ac: bool, pct: u8) -> Readings {
        Readings {
            thermal: ThermalState::Nominal,
            battery: batt(on_ac, pct),
            low_power: false,
            memory: MemoryPressure::Normal,
            adapter_watts: if on_ac { Some(96) } else { None },
            rss_mb: 400.0,
        }
    }

    #[test]
    fn level_table() {
        let table: Vec<(&str, Readings, Level, Option<&str>)> = vec![
            ("4% on AC is full", r(true, 4), Level::Full, None),
            (
                "4% on battery is minimal",
                r(false, 4),
                Level::Minimal,
                Some("battery"),
            ),
            (
                "10% on battery is minimal",
                r(false, 10),
                Level::Minimal,
                Some("battery"),
            ),
            (
                "11% on battery is light",
                r(false, 11),
                Level::Light,
                Some("battery"),
            ),
            (
                "20% on battery is light",
                r(false, 20),
                Level::Light,
                Some("battery"),
            ),
            ("21% on battery is full", r(false, 21), Level::Full, None),
            ("80% on battery is full", r(false, 80), Level::Full, None),
            (
                "low power on AC is light",
                Readings {
                    low_power: true,
                    ..r(true, 90)
                },
                Level::Light,
                Some("lowpower"),
            ),
            (
                "thermal serious is light",
                Readings {
                    thermal: ThermalState::Serious,
                    ..r(true, 90)
                },
                Level::Light,
                Some("thermal"),
            ),
            (
                "thermal critical is minimal",
                Readings {
                    thermal: ThermalState::Critical,
                    ..r(true, 90)
                },
                Level::Minimal,
                Some("thermal"),
            ),
            (
                "memory warn is light",
                Readings {
                    memory: MemoryPressure::Warn,
                    ..r(true, 90)
                },
                Level::Light,
                Some("memory"),
            ),
            (
                "memory critical is minimal",
                Readings {
                    memory: MemoryPressure::Critical,
                    ..r(true, 90)
                },
                Level::Minimal,
                Some("memory"),
            ),
            (
                "20 W charger at 4% is light (adapter)",
                Readings {
                    adapter_watts: Some(20),
                    ..r(true, 4)
                },
                Level::Light,
                Some("adapter_20w_battery_4"),
            ),
            (
                "20 W charger at 50% is full",
                Readings {
                    adapter_watts: Some(20),
                    ..r(true, 50)
                },
                Level::Full,
                None,
            ),
            (
                "96 W charger at 4% is full",
                Readings {
                    adapter_watts: Some(96),
                    ..r(true, 4)
                },
                Level::Full,
                None,
            ),
            (
                "no battery at all is full",
                Readings {
                    battery: BatteryReading::default(),
                    ..r(true, 0)
                },
                Level::Full,
                None,
            ),
            (
                "unknown everything is full",
                Readings::default(),
                Level::Full,
                None,
            ),
            (
                "4% battery AND thermal serious says battery",
                Readings {
                    thermal: ThermalState::Serious,
                    ..r(false, 4)
                },
                Level::Minimal,
                Some("battery"),
            ),
        ];
        for (name, readings, level, reason) in table {
            let v = level_for(&readings, DEFAULT_PAUSE_BELOW_PERCENT);
            assert_eq!(v.level, level, "{name}");
            assert_eq!(v.reason.as_deref(), reason, "{name}");
        }
    }

    #[test]
    fn the_pause_below_setting_moves_the_light_line() {
        assert_eq!(level_for(&r(false, 30), 35).level, Level::Light);
        assert_eq!(level_for(&r(false, 30), 20).level, Level::Full);
        // A setting UNDER 10 pulls minimal down with it: 5% means "no
        // minimal until 5", not "minimal at 10 anyway" — but FX-03's 15%
        // floor still makes 8% light.
        assert_eq!(level_for(&r(false, 8), 5).level, Level::Light);
        assert_eq!(level_for(&r(false, 5), 5).level, Level::Minimal);
        // The floor: under 15% on battery is light whatever the setting.
        assert_eq!(level_for(&r(false, 14), 5).level, Level::Light);
        assert_eq!(
            level_for(&r(false, 14), 5).reason.as_deref(),
            Some("battery")
        );
        assert_eq!(level_for(&r(false, 15), 5).level, Level::Full);
        assert_eq!(level_for(&r(false, 14), 1).level, Level::Light);
        assert_eq!(clamp_pause_below(0), 1);
        assert_eq!(clamp_pause_below(100), 99);
    }

    #[test]
    fn consumers_per_level() {
        assert_eq!(polish_plan(Level::Full, false), PolishPlan::Local);
        assert_eq!(polish_plan(Level::Light, false), PolishPlan::Skip);
        assert_eq!(polish_plan(Level::Light, true), PolishPlan::Cloud);
        assert_eq!(polish_plan(Level::Minimal, true), PolishPlan::Skip);
        assert!(!live_transcript_paused_at(Level::Light));
        assert!(live_transcript_paused_at(Level::Minimal));
        // Heavy work: light defers only for power; minimal always.
        assert_eq!(
            heavy_work_deferred_by(&level_for(&r(false, 15), 20)).as_deref(),
            Some("battery")
        );
        // FX-03: thermal serious and memory warn are constrained too — the
        // meeting decode waits for full even on wall power.
        assert_eq!(
            heavy_work_deferred_by(&level_for(
                &Readings {
                    thermal: ThermalState::Serious,
                    ..r(true, 90)
                },
                20
            ))
            .as_deref(),
            Some("thermal")
        );
        assert_eq!(
            heavy_work_deferred_by(&level_for(
                &Readings {
                    memory: MemoryPressure::Warn,
                    ..r(true, 90)
                },
                20
            ))
            .as_deref(),
            Some("memory")
        );
        assert_eq!(
            heavy_work_deferred_by(&level_for(
                &Readings {
                    low_power: true,
                    ..r(true, 90)
                },
                20
            ))
            .as_deref(),
            Some("lowpower")
        );
        assert_eq!(heavy_work_deferred_by(&level_for(&r(true, 90), 20)), None);
        // Dictation keeps exactly one engine whenever the Mac is constrained.
        assert!(!keeps_one_engine(Level::Full));
        assert!(keeps_one_engine(Level::Light));
        assert!(keeps_one_engine(Level::Minimal));
        assert_eq!(
            heavy_work_deferred_by(&level_for(
                &Readings {
                    thermal: ThermalState::Critical,
                    ..r(true, 90)
                },
                20
            ))
            .as_deref(),
            Some("thermal")
        );
        assert_eq!(
            heavy_work_deferred_by(&level_for(
                &Readings {
                    adapter_watts: Some(20),
                    ..r(true, 4)
                },
                20
            ))
            .as_deref(),
            Some("adapter_20w_battery_4")
        );
    }

    #[test]
    fn memory_flags_map() {
        assert_eq!(memory_pressure_from_flags(0), MemoryPressure::Unknown);
        assert_eq!(memory_pressure_from_flags(1), MemoryPressure::Normal);
        assert_eq!(memory_pressure_from_flags(2), MemoryPressure::Warn);
        assert_eq!(memory_pressure_from_flags(4), MemoryPressure::Critical);
        assert_eq!(memory_pressure_from_flags(6), MemoryPressure::Critical);
    }

    #[test]
    fn the_governor_gates_consumers_through_an_injected_probe() {
        let probe = FakeProbe::new(r(true, 90));
        let g = Governor::new(probe.clone(), DEFAULT_PAUSE_BELOW_PERCENT);
        let seen = Arc::new(Mutex::new(Vec::<GovernorView>::new()));
        let sink = Arc::clone(&seen);
        g.set_emitter(move |v| sink.lock().unwrap().push(v.clone()));

        assert_eq!(g.level(), Level::Full);
        assert_eq!(g.heavy_work_deferred("m1"), None);
        assert!(!g.live_transcript_paused());
        assert_eq!(g.polish_plan(false), PolishPlan::Local);
        assert!(
            seen.lock().unwrap().is_empty(),
            "full→full is not a transition"
        );

        // Unplug at 4%.
        probe.set(r(false, 4));
        assert_eq!(g.refresh().level, Level::Minimal);
        assert_eq!(g.heavy_work_deferred("m1").as_deref(), Some("battery"));
        assert!(g.live_transcript_paused());
        assert_eq!(g.polish_plan(true), PolishPlan::Skip);
        {
            let s = seen.lock().unwrap();
            assert_eq!(s.len(), 1);
            assert_eq!(s[0].level, Level::Minimal);
            assert_eq!(
                s[0].sentence.as_deref(),
                Some("Yap is going easy: 4% battery")
            );
            assert_eq!(
                s[0].pill_label.as_deref(),
                Some("Saving battery: notes after you plug in")
            );
            assert!(s[0].heavy_work_deferred);
            assert_eq!(
                s[0].main_sentence.as_deref(),
                Some("Yap is saving power — your meeting will transcribe when you're charged")
            );
        }

        assert_eq!(g.view().waiting_jobs, vec!["m1".to_string()]);
        // "Run anyway" for m1 lets m1 through and nobody else.
        g.run_anyway("m1");
        assert_eq!(g.heavy_work_deferred("m1"), None);
        assert_eq!(g.heavy_work_deferred("m2").as_deref(), Some("battery"));
        assert_eq!(g.view().waiting_jobs, vec!["m2".to_string()]);

        // Plug in: full, overrides cleared, one more transition.
        probe.set(r(true, 4));
        assert_eq!(g.refresh().level, Level::Full);
        assert_eq!(g.heavy_work_deferred("m2"), None);
        assert_eq!(seen.lock().unwrap().len(), 2);
        assert_eq!(seen.lock().unwrap()[1].sentence, None);

        // Unplug again at 4%: m1's old override is gone.
        probe.set(r(false, 4));
        g.refresh();
        assert_eq!(g.heavy_work_deferred("m1").as_deref(), Some("battery"));

        // The setting re-decides at once.
        probe.set(r(false, 30));
        g.refresh();
        assert_eq!(g.level(), Level::Full);
        let v = g.set_pause_below(40);
        assert_eq!(v.level, Level::Light);
        assert_eq!(v.pause_below_percent, 40);
        assert_eq!(g.heavy_work_deferred("m3").as_deref(), Some("battery"));
    }

    #[test]
    fn the_fake_probe_is_also_an_energy_probe() {
        let probe = FakeProbe::new(Readings {
            thermal: ThermalState::Fair,
            ..r(false, 50)
        });
        let e: &dyn EnergyProbe = &probe;
        assert_eq!(e.thermal(), ThermalState::Fair);
        assert_eq!(e.battery().percent, Some(50));
        assert!(!e.battery().on_ac);
    }

    #[test]
    fn sentences_read_as_plain_words() {
        let v = level_for(
            &Readings {
                low_power: true,
                ..r(true, 90)
            },
            20,
        );
        assert_eq!(
            sentence_for(&v).as_deref(),
            Some("Yap is going easy: Low Power Mode is on")
        );
        let v = level_for(
            &Readings {
                adapter_watts: Some(20),
                ..r(true, 4)
            },
            20,
        );
        assert_eq!(
            sentence_for(&v).as_deref(),
            Some("Yap is going easy: a 20w charger can't keep up")
        );
        assert_eq!(
            pill_label_for(&v).as_deref(),
            Some("Slow charger: notes when the battery is up")
        );
        assert_eq!(
            main_sentence_for(&v).as_deref(),
            Some("Yap is saving power — your meeting will transcribe when you're charged")
        );
        let v = level_for(
            &Readings {
                thermal: ThermalState::Serious,
                ..r(true, 90)
            },
            20,
        );
        assert_eq!(
            pill_label_for(&v).as_deref(),
            Some("Running hot: notes when this Mac cools")
        );
        assert_eq!(
            main_sentence_for(&v).as_deref(),
            Some("Yap is letting this Mac cool down — your meeting will transcribe when it does")
        );
        let v = level_for(
            &Readings {
                memory: MemoryPressure::Warn,
                ..r(true, 90)
            },
            20,
        );
        assert_eq!(
            pill_label_for(&v).as_deref(),
            Some("Low on memory: notes when it frees up")
        );
        let v = level_for(&r(false, 4), 20);
        assert_eq!(
            main_sentence_for(&v).as_deref(),
            Some("Yap is saving power — your meeting will transcribe when you're charged")
        );
        assert_eq!(main_sentence_for(&Verdict::full()), None);
        assert_eq!(sentence_for(&Verdict::full()), None);
        assert_eq!(pill_label_for(&Verdict::full()), None);
    }

    #[test]
    fn readers_do_not_panic_on_this_machine() {
        // Real IOKit / ProcessInfo calls: the value is whatever this Mac says.
        let _ = low_power_mode();
        let _ = adapter_watts();
        let _ = memory_pressure();
    }
}
