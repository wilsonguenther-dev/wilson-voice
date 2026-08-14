//! YV95 / OS-12 fix (3) — instrument the meeting, don't guess at it.
//!
//! OS-12's argument is that capture is thermally free and the *always-visible
//! three-hour pill* is not, and that the plan had no way to tell a bad session
//! from a bad opinion afterwards. Its fix names two OS readings and asks for
//! them to be written into the meeting row so a session that went wrong is
//! diagnosable after the fact:
//!
//!   * `ProcessInfo.thermalState` — nominal / fair / serious / critical.
//!   * `IOPSCopyPowerSourcesInfo` + `IOPSGetTimeRemainingEstimate` — the battery
//!     preflight: on AC or not, percent, and the OS's own minutes-remaining
//!     estimate (which is NOT percent ÷ drain-rate; it is IOKit's smoothed
//!     estimate, and it is the number a user would recognise from the menu bar).
//!
//! Two deliberate departures from the letter of the finding, both narrower:
//!
//! 1. **Thermal transitions are polled on the 1 Hz elapsed tick that already
//!    exists, not observed through `NSProcessInfoThermalStateDidChangeNotification`.**
//!    The notification would need a block-based `NSNotificationCenter` observer
//!    (a new `block2` dependency and an observer whose lifetime has to be
//!    unregistered on a path that can also be a crash), to learn the same thing
//!    a single `objc_msgSend` on a timer that is already running tells us within
//!    one second. Thermal state changes on the scale of minutes; one-second
//!    resolution over a three-hour meeting is not the weak part of this
//!    measurement. The transition LIST is what the finding actually asked for,
//!    and that is what gets stored.
//! 2. **The battery reading is a preflight plus a final sample, not a series.**
//!    A per-second battery series is 10,800 rows of noise for a number that
//!    moves in percent steps; start and end answer "did this session run the
//!    battery down, and did it start with enough" — which is the question.
//!
//! Everything here degrades to `Unknown` rather than failing: a diagnostic that
//! can abort a recording is a worse bug than the one it was added to explain.

use serde::{Deserialize, Serialize};

/// `ProcessInfo.thermalState`, as a value that can be stored and compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThermalState {
    Nominal,
    Fair,
    Serious,
    Critical,
    /// The read failed, or this is not macOS.
    Unknown,
}

impl ThermalState {
    pub fn as_str(self) -> &'static str {
        match self {
            ThermalState::Nominal => "nominal",
            ThermalState::Fair => "fair",
            ThermalState::Serious => "serious",
            ThermalState::Critical => "critical",
            ThermalState::Unknown => "unknown",
        }
    }

    /// `NSProcessInfoThermalState` is an `NSInteger` 0..=3; anything else is a
    /// newer OS telling us about a state this build does not know.
    pub fn from_ns_integer(v: isize) -> ThermalState {
        match v {
            0 => ThermalState::Nominal,
            1 => ThermalState::Fair,
            2 => ThermalState::Serious,
            3 => ThermalState::Critical,
            _ => ThermalState::Unknown,
        }
    }
}

/// One battery sample. Every field is optional because every field can
/// legitimately be unavailable — a Mac mini has no battery at all, and IOKit
/// answers "unknown" for the time estimate for a minute or so after a plug
/// change while it re-converges.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryReading {
    /// `true` when the machine is running on wall power (or has no battery).
    pub on_ac: bool,
    /// Charge percent, 0..=100.
    pub percent: Option<u8>,
    /// The OS's own estimate. `None` for "unlimited" (AC) and for "unknown".
    pub minutes_remaining: Option<i64>,
    /// `false` when there is no internal battery source at all.
    pub has_battery: bool,
}

impl Default for BatteryReading {
    fn default() -> Self {
        // The safe unknown: assume AC (so nothing is refused on a bad read) and
        // say plainly that there is no battery data.
        BatteryReading {
            on_ac: true,
            percent: None,
            minutes_remaining: None,
            has_battery: false,
        }
    }
}

/// IOKit's sentinel for "this power source will not run out" — i.e. AC.
pub const TIME_REMAINING_UNLIMITED: f64 = -2.0;
/// IOKit's sentinel for "still working it out".
pub const TIME_REMAINING_UNKNOWN: f64 = -1.0;

/// The pure half of [`battery`] — everything except the four IOKit calls.
///
/// Split out because this is where the arithmetic and the sentinel handling can
/// be wrong, and neither of those needs a Mac with a battery in it to test.
/// `state` is `kIOPSPowerSourceStateKey`: `"AC Power"` or `"Battery Power"`.
pub fn battery_from_raw(
    state: Option<&str>,
    current_capacity: Option<i64>,
    max_capacity: Option<i64>,
    time_remaining_seconds: f64,
) -> BatteryReading {
    let has_battery = state.is_some() && max_capacity.unwrap_or(0) > 0;
    let on_ac = match state {
        Some("Battery Power") => false,
        // "AC Power", "Off Line" (which only appears with a UPS), and an absent
        // source all read as "not draining a battery".
        _ => true,
    };
    let percent = match (current_capacity, max_capacity) {
        (Some(cur), Some(max)) if max > 0 => Some(
            ((cur.max(0) as f64 / max as f64) * 100.0)
                .round()
                .clamp(0.0, 100.0) as u8,
        ),
        _ => None,
    };
    let minutes_remaining = if time_remaining_seconds > 0.0 && time_remaining_seconds.is_finite() {
        Some((time_remaining_seconds / 60.0).round() as i64)
    } else {
        // Both sentinels (and a nonsense negative) mean "no estimate to show".
        None
    };
    BatteryReading {
        on_ac,
        percent,
        minutes_remaining,
        has_battery,
    }
}

/// One observed thermal change, stamped with where in the meeting it happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThermalTransition {
    /// Seconds into the meeting.
    pub at_seconds: f64,
    pub from: ThermalState,
    pub to: ThermalState,
}

/// What gets written into `meetings.diagnostics` as JSON.
///
/// One column, one JSON blob, deliberately: this is diagnostic exhaust that no
/// query filters on, and giving each field a column would mean a migration every
/// time a later phase learns to record one more thing about a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingDiagnostics {
    /// Schema marker for the blob itself, so a later reader can tell what it is
    /// looking at without guessing from which keys are present.
    pub version: u32,
    pub thermal_at_start: ThermalState,
    pub thermal_at_end: Option<ThermalState>,
    /// Every change observed while recording, in order.
    pub thermal_transitions: Vec<ThermalTransition>,
    /// OS-12's battery preflight, read before the first sample is captured.
    pub battery_at_start: BatteryReading,
    pub battery_at_end: Option<BatteryReading>,
    /// Was the always-on-top pill visible for this session? The whole point of
    /// OS-12 is that the pill, not the capture, is the energy cost — so a power
    /// number is uninterpretable without knowing whether it was on screen.
    pub pill_visible: bool,
    /// How the meeting ended, in the controller's words.
    pub stop_reason: Option<String>,
}

impl MeetingDiagnostics {
    /// The current blob schema. Bump when a field's MEANING changes; adding an
    /// optional field does not need it.
    pub const VERSION: u32 = 1;

    pub fn start(thermal: ThermalState, battery: BatteryReading, pill_visible: bool) -> Self {
        MeetingDiagnostics {
            version: Self::VERSION,
            thermal_at_start: thermal,
            thermal_at_end: None,
            thermal_transitions: Vec::new(),
            battery_at_start: battery,
            battery_at_end: None,
            pill_visible,
            stop_reason: None,
        }
    }

    /// Feed a fresh thermal sample. Records a transition only when the value
    /// actually changed — 10,800 identical samples are not a log, they are a
    /// leak. Returns `true` when something was recorded.
    pub fn observe_thermal(&mut self, at_seconds: f64, sample: ThermalState) -> bool {
        let current = self
            .thermal_transitions
            .last()
            .map(|t| t.to)
            .unwrap_or(self.thermal_at_start);
        if sample == current {
            return false;
        }
        self.thermal_transitions.push(ThermalTransition {
            at_seconds,
            from: current,
            to: sample,
        });
        true
    }

    /// The thermal state as of the last observation.
    pub fn thermal_now(&self) -> ThermalState {
        self.thermal_transitions
            .last()
            .map(|t| t.to)
            .unwrap_or(self.thermal_at_start)
    }

    pub fn finish(&mut self, battery: BatteryReading, stop_reason: impl Into<String>) {
        self.thermal_at_end = Some(self.thermal_now());
        self.battery_at_end = Some(battery);
        self.stop_reason = Some(stop_reason.into());
    }

    /// Serialize for the `meetings.diagnostics` column. A blob that cannot be
    /// encoded must not take a meeting down with it, so this cannot fail.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
    }
}

/// Read `ProcessInfo.thermalState`. Returns [`ThermalState::Unknown`] off macOS.
#[cfg(target_os = "macos")]
pub fn thermal() -> ThermalState {
    let info = objc2_foundation::NSProcessInfo::processInfo();
    ThermalState::from_ns_integer(info.thermalState().0)
}

#[cfg(not(target_os = "macos"))]
pub fn thermal() -> ThermalState {
    ThermalState::Unknown
}

/// Read the battery through IOKit's power-source API.
#[cfg(target_os = "macos")]
pub fn battery() -> BatteryReading {
    use core_foundation::array::{CFArray, CFArrayRef};
    use core_foundation::base::{CFType, CFTypeRef, TCFType};
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
        fn IOPSCopyPowerSourcesList(blob: CFTypeRef) -> CFArrayRef;
        fn IOPSGetPowerSourceDescription(blob: CFTypeRef, ps: CFTypeRef) -> CFDictionaryRef;
        fn IOPSGetTimeRemainingEstimate() -> f64;
    }

    unsafe {
        let time_remaining = IOPSGetTimeRemainingEstimate();
        let blob = IOPSCopyPowerSourcesInfo();
        if blob.is_null() {
            return BatteryReading::default();
        }
        // Both wrapped under the CREATE rule: `IOPSCopyPowerSourcesInfo` and
        // `IOPSCopyPowerSourcesList` are Copy functions, so we own a reference
        // to each and the wrappers release them on drop. The DESCRIPTION is a
        // Get function — borrowed from the blob, never released here.
        let blob_owned = CFType::wrap_under_create_rule(blob);
        let list_ref = IOPSCopyPowerSourcesList(blob);
        if list_ref.is_null() {
            return BatteryReading::default();
        }
        let list: CFArray<CFType> = CFArray::wrap_under_create_rule(list_ref);

        let key_state = CFString::from_static_string("Power Source State");
        let key_current = CFString::from_static_string("Current Capacity");
        let key_max = CFString::from_static_string("Max Capacity");
        let key_type = CFString::from_static_string("Type");
        let internal_battery = "InternalBattery";

        let mut reading = BatteryReading::default();
        for source in list.iter() {
            let desc_ref = IOPSGetPowerSourceDescription(blob, source.as_CFTypeRef());
            if desc_ref.is_null() {
                continue;
            }
            let desc: CFDictionary<CFString, CFType> = CFDictionary::wrap_under_get_rule(desc_ref);
            let string_at = |k: &CFString| -> Option<String> {
                desc.find(k)
                    .and_then(|v| v.downcast::<CFString>())
                    .map(|s| s.to_string())
            };
            let int_at = |k: &CFString| -> Option<i64> {
                desc.find(k)
                    .and_then(|v| v.downcast::<CFNumber>())
                    .and_then(|n| n.to_i64())
            };
            // A UPS is a power source too. Only the internal battery answers
            // "will this Mac survive the meeting".
            if let Some(kind) = string_at(&key_type) {
                if !kind.contains(internal_battery) {
                    continue;
                }
            }
            reading = battery_from_raw(
                string_at(&key_state).as_deref(),
                int_at(&key_current),
                int_at(&key_max),
                time_remaining,
            );
            break;
        }
        drop(blob_owned);
        reading
    }
}

#[cfg(not(target_os = "macos"))]
pub fn battery() -> BatteryReading {
    BatteryReading::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ac_power_is_not_a_drain() {
        let r = battery_from_raw(
            Some("AC Power"),
            Some(4200),
            Some(5000),
            TIME_REMAINING_UNLIMITED,
        );
        assert!(r.on_ac);
        assert_eq!(r.percent, Some(84));
        assert_eq!(r.minutes_remaining, None, "unlimited is not a number");
        assert!(r.has_battery);
    }

    #[test]
    fn battery_power_carries_an_estimate() {
        let r = battery_from_raw(Some("Battery Power"), Some(2500), Some(5000), 96.0 * 60.0);
        assert!(!r.on_ac);
        assert_eq!(r.percent, Some(50));
        assert_eq!(r.minutes_remaining, Some(96));
    }

    #[test]
    fn an_unknown_estimate_is_not_a_negative_number_of_minutes() {
        let r = battery_from_raw(
            Some("Battery Power"),
            Some(100),
            Some(100),
            TIME_REMAINING_UNKNOWN,
        );
        assert_eq!(r.minutes_remaining, None);
        assert_eq!(r.percent, Some(100));
    }

    #[test]
    fn a_machine_with_no_battery_reads_as_ac_without_pretending_to_have_one() {
        let r = battery_from_raw(None, None, None, TIME_REMAINING_UNLIMITED);
        assert!(r.on_ac);
        assert!(!r.has_battery);
        assert_eq!(r.percent, None);
    }

    #[test]
    fn only_real_changes_become_transitions() {
        let mut d =
            MeetingDiagnostics::start(ThermalState::Nominal, BatteryReading::default(), true);
        assert!(!d.observe_thermal(1.0, ThermalState::Nominal));
        assert!(d.observe_thermal(60.0, ThermalState::Fair));
        assert!(!d.observe_thermal(61.0, ThermalState::Fair));
        assert!(d.observe_thermal(900.0, ThermalState::Serious));
        assert!(d.observe_thermal(1800.0, ThermalState::Fair));
        assert_eq!(d.thermal_transitions.len(), 3);
        assert_eq!(d.thermal_now(), ThermalState::Fair);
        assert_eq!(d.thermal_transitions[0].from, ThermalState::Nominal);
        assert_eq!(d.thermal_transitions[2].to, ThermalState::Fair);
    }

    #[test]
    fn the_blob_round_trips() {
        let mut d = MeetingDiagnostics::start(ThermalState::Fair, BatteryReading::default(), false);
        d.observe_thermal(10.0, ThermalState::Serious);
        d.finish(BatteryReading::default(), "user stopped");
        let json = d.to_json();
        let back: MeetingDiagnostics = serde_json::from_str(&json).expect("round-trip");
        assert_eq!(back, d);
        assert_eq!(back.thermal_at_end, Some(ThermalState::Serious));
    }
}
