//! WF-09 — the microphone picker.
//!
//! Before this module Yap always recorded from the system default input. Now
//! the user can pin one input device (tray → Microphone ▸, or Settings → Audio)
//! and the NEXT take opens it. The choice is saved per Mac in
//! `<data dir>/microphone.json`, keyed by cpal's STABLE device id
//! (`coreaudio:<device UID>` on macOS, which survives unplugging, reboots and
//! renames) — never by the display name, which two identical USB mics share.
//!
//! "System Default" is always the first entry and is what an empty file, a
//! missing file or a corrupt file means. A pinned device that is not plugged
//! in is never an error: the take falls back to the system default (see
//! [`resolve`]) and the picker says so.
//!
//! Three halves:
//! * [`resolve`] — the PURE decision "which device does the next take open",
//!   tested without a microphone;
//! * the persisted choice ([`load`], [`save`], [`current`], [`choose`]) plus a
//!   [`generation`] counter the capture worker compares against the stream it
//!   holds open, so a pick made while the warm stream is idle still lands on
//!   the very next take;
//! * the device-list watcher ([`start_watch`]) which re-enumerates inputs every
//!   [`WATCH_INTERVAL`] and publishes ONLY when the list (or the choice)
//!   changed — that is how a mic plugged in mid-session shows up in the tray
//!   submenu and the Settings picker without a restart.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};

/// The file the choice lives in, under `crate::data_dir()`.
pub const FILE_NAME: &str = "microphone.json";

/// The first entry of every picker.
pub const SYSTEM_DEFAULT_LABEL: &str = "System Default";

/// The frontend event carrying a fresh [`MicList`] whenever it changes.
pub const CHANGED_EVENT: &str = "microphones-changed";

/// How often the watcher re-enumerates input devices. Enumeration is a handful
/// of HAL property reads; a plugged-in mic appears in the picker within this.
pub const WATCH_INTERVAL: Duration = Duration::from_secs(3);

/// The persisted choice. `id == None` is "System Default".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MicChoice {
    /// cpal `DeviceId` in its `Display` form (`"coreaudio:<uid>"`).
    #[serde(default)]
    pub id: Option<String>,
    /// The name the device had when it was picked — shown while it is
    /// unplugged ("AirPods Pro — not connected").
    #[serde(default)]
    pub name: Option<String>,
}

/// One input device as the pickers list it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicDevice {
    pub id: String,
    pub name: String,
    /// The device macOS currently routes "default input" to.
    pub is_system_default: bool,
}

/// Everything both pickers draw from — one snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicList {
    pub devices: Vec<MicDevice>,
    /// The saved choice (`None` = System Default).
    pub chosen_id: Option<String>,
    pub chosen_name: Option<String>,
    /// The saved device is not plugged in; the next take uses the default.
    pub chosen_missing: bool,
    /// The device the next take will open (`None` = no input at all).
    pub in_use_id: Option<String>,
    pub in_use_name: Option<String>,
}

impl MicList {
    /// The name of the current system default input, if one is plugged in.
    pub fn system_default_name(&self) -> Option<&str> {
        self.devices
            .iter()
            .find(|d| d.is_system_default)
            .map(|d| d.name.as_str())
    }
}

/// Which device the next take opens, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution<D> {
    /// The pinned device is plugged in.
    Chosen(D),
    /// Nothing is pinned; the system default is used.
    SystemDefault(D),
    /// A device is pinned but missing; the system default is used instead.
    FellBackToDefault(D),
    /// No input device at all. `chosen_missing` is whether a pin was also lost.
    NoInput { chosen_missing: bool },
}

impl<D> Resolution<D> {
    pub fn device(self) -> Option<D> {
        match self {
            Resolution::Chosen(d)
            | Resolution::SystemDefault(d)
            | Resolution::FellBackToDefault(d) => Some(d),
            Resolution::NoInput { .. } => None,
        }
    }

    /// The `source=` value of the telemetry line.
    pub fn source(&self) -> &'static str {
        match self {
            Resolution::Chosen(_) => "chosen",
            Resolution::SystemDefault(_) => "system_default",
            Resolution::FellBackToDefault(_) => "fell_back_to_default",
            Resolution::NoInput { .. } => "no_input",
        }
    }

    /// Was a pinned device asked for and not found?
    pub fn chosen_missing(&self) -> bool {
        matches!(
            self,
            Resolution::FellBackToDefault(_)
                | Resolution::NoInput {
                    chosen_missing: true
                }
        )
    }
}

/// THE decision: the device the next take opens, by STABLE id.
///
/// `wanted` is the saved id (`None` = System Default), `available` every input
/// device plugged in now as `(stable id, device)`, `default` the system default
/// input. Matching is exact on the id — two mics with the same display name are
/// still two devices — and an empty id is never a match.
pub fn resolve<D: Clone>(
    wanted: Option<&str>,
    available: &[(String, D)],
    default: Option<D>,
) -> Resolution<D> {
    let wanted = wanted.filter(|w| !w.is_empty());
    match wanted {
        None => match default {
            Some(d) => Resolution::SystemDefault(d),
            None => Resolution::NoInput {
                chosen_missing: false,
            },
        },
        Some(w) => match available.iter().find(|(id, _)| !id.is_empty() && id == w) {
            Some((_, d)) => Resolution::Chosen(d.clone()),
            None => match default {
                Some(d) => Resolution::FellBackToDefault(d),
                None => Resolution::NoInput {
                    chosen_missing: true,
                },
            },
        },
    }
}

/// Build the picker snapshot from an enumeration (pure; the tests drive it).
pub fn build_list(
    devices: Vec<MicDevice>,
    default_id: Option<&str>,
    choice: &MicChoice,
) -> MicList {
    let devices: Vec<MicDevice> = devices
        .into_iter()
        .map(|mut d| {
            d.is_system_default = default_id.is_some_and(|x| !x.is_empty() && x == d.id);
            d
        })
        .collect();
    let pairs: Vec<(String, MicDevice)> =
        devices.iter().map(|d| (d.id.clone(), d.clone())).collect();
    let default = devices.iter().find(|d| d.is_system_default).cloned();
    let resolution = resolve(choice.id.as_deref(), &pairs, default);
    let chosen_missing = resolution.chosen_missing();
    let in_use = resolution.device();
    MicList {
        chosen_id: choice.id.clone().filter(|s| !s.is_empty()),
        chosen_name: choice.name.clone(),
        chosen_missing,
        in_use_id: in_use.as_ref().map(|d| d.id.clone()),
        in_use_name: in_use.map(|d| d.name),
        devices,
    }
}

// ─── The persisted choice ────────────────────────────────────────────────────

/// Read the choice. A missing file is System Default; an unreadable or corrupt
/// one is System Default too, logged — a broken pin must never stop dictation.
pub fn load(path: &Path) -> MicChoice {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<MicChoice>(&text) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("mic_select load_failed reason=parse error={e}");
                MicChoice::default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => MicChoice::default(),
        Err(e) => {
            log::warn!("mic_select load_failed reason=io kind={:?}", e.kind());
            MicChoice::default()
        }
    }
}

/// Write the choice atomically (temp file + rename).
pub fn save(path: &Path, choice: &MicChoice) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("create dir: {e}"))?;
    }
    let body = serde_json::to_string_pretty(choice).map_err(|e| format!("encode: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("write: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("rename: {e}")
    })
}

fn choice_path() -> std::path::PathBuf {
    crate::data_dir().join(FILE_NAME)
}

fn cached() -> &'static Mutex<Option<MicChoice>> {
    static CHOICE: OnceLock<Mutex<Option<MicChoice>>> = OnceLock::new();
    CHOICE.get_or_init(|| Mutex::new(None))
}

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Bumped on every successful [`choose`]. The capture worker remembers the
/// generation its warm stream was opened at and reopens on the next Arm when
/// it moved — that is what makes a pick apply to the very next take.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::SeqCst)
}

/// The saved choice (read from disk once, then cached).
pub fn current() -> MicChoice {
    let mut slot = cached().lock().unwrap_or_else(|p| p.into_inner());
    slot.get_or_insert_with(|| load(&choice_path())).clone()
}

/// Save a new choice (`id == None` = System Default) and wake the watcher so
/// both pickers redraw. `source` is the telemetry tag (`tray` / `settings`).
pub fn choose(id: Option<String>, name: Option<String>, source: &str) -> Result<(), String> {
    let id = id.filter(|s| !s.is_empty());
    let choice = MicChoice {
        name: if id.is_some() { name } else { None },
        id,
    };
    if let Err(e) = save(&choice_path(), &choice) {
        log::warn!("mic_select choose_failed source={source} error={e}");
        return Err(e);
    }
    let kind = if choice.id.is_some() {
        "device"
    } else {
        "system_default"
    };
    *cached().lock().unwrap_or_else(|p| p.into_inner()) = Some(choice);
    GENERATION.fetch_add(1, Ordering::SeqCst);
    log::info!("mic_select chosen source={source} kind={kind}");
    poke();
    Ok(())
}

// ─── cpal ────────────────────────────────────────────────────────────────────

/// A device's stable id string, or `None` if the HAL cannot name it.
pub fn device_id(device: &cpal::Device) -> Option<String> {
    device
        .id()
        .ok()
        .map(|id| id.to_string())
        .filter(|s| !s.is_empty())
}

/// A device's display name (empty when it has none).
pub fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_default()
}

/// Every input device plugged in now, as `(stable id, name, device)`.
fn enumerate_inputs(host: &cpal::Host) -> Vec<(String, String, cpal::Device)> {
    match host.input_devices() {
        Ok(devices) => devices
            .filter_map(|d| device_id(&d).map(|id| (id, device_name(&d), d)))
            .collect(),
        Err(e) => {
            log::warn!("mic_select enumerate_failed error={e}");
            Vec::new()
        }
    }
}

/// The device the next take opens and the name it reports — the one function
/// `record.rs` asks. `None` = no input device at all.
///
/// With nothing pinned this is exactly the old path (`default_input_device`,
/// no enumeration). With a pin it enumerates and matches by stable id, falling
/// back to the system default when the pinned mic is not plugged in.
pub fn input_device_for_next_take() -> Option<(cpal::Device, String)> {
    let host = cpal::default_host();
    let choice = current();
    let resolution = match choice.id.as_deref().filter(|s| !s.is_empty()) {
        None => resolve::<cpal::Device>(None, &[], host.default_input_device()),
        Some(wanted) => {
            let available: Vec<(String, cpal::Device)> = enumerate_inputs(&host)
                .into_iter()
                .map(|(id, _, d)| (id, d))
                .collect();
            resolve(Some(wanted), &available, host.default_input_device())
        }
    };
    if resolution.chosen_missing() {
        log::warn!(
            "mic_select resolve source={} chosen_missing=true",
            resolution.source()
        );
    } else {
        log::debug!("mic_select resolve source={}", resolution.source());
    }
    resolution.device().map(|d| {
        let name = device_name(&d);
        (d, name)
    })
}

/// A fresh picker snapshot (enumerates the hardware — never on the main thread).
pub fn list() -> MicList {
    let host = cpal::default_host();
    let default_id = host.default_input_device().as_ref().and_then(device_id);
    let devices = enumerate_inputs(&host)
        .into_iter()
        .map(|(id, name, _)| MicDevice {
            id,
            name,
            is_system_default: false,
        })
        .collect();
    build_list(devices, default_id.as_deref(), &current())
}

// ─── The device-list watcher ─────────────────────────────────────────────────

fn wake() -> &'static (Mutex<bool>, Condvar) {
    static WAKE: OnceLock<(Mutex<bool>, Condvar)> = OnceLock::new();
    WAKE.get_or_init(|| (Mutex::new(false), Condvar::new()))
}

/// Ask the watcher to re-enumerate and republish now instead of at its next
/// tick.
pub fn poke() {
    let (lock, cv) = wake();
    *lock.lock().unwrap_or_else(|p| p.into_inner()) = true;
    cv.notify_all();
}

/// Should the watcher publish `next`? Only when something a picker shows moved.
pub fn should_publish(last: Option<&MicList>, next: &MicList) -> bool {
    last != Some(next)
}

/// Start the one watcher thread. `publish` runs on the watcher thread with
/// every snapshot that differs from the last one published — the first
/// snapshot always publishes, so the tray submenu is filled at launch.
pub fn start_watch(publish: impl Fn(&MicList) + Send + 'static) {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        log::debug!("mic_select watch already_running");
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("wv-mic-watch".into())
        .spawn(move || {
            let mut last: Option<MicList> = None;
            // A poke (a pick from either picker) always republishes, even when
            // the snapshot is unchanged: re-picking the checked tray row
            // un-checks it natively, and only a redraw puts the check back.
            let mut forced = false;
            loop {
                let next = list();
                if forced || should_publish(last.as_ref(), &next) {
                    log::info!(
                        "mic_select list_changed devices={} chosen={} chosen_missing={}",
                        next.devices.len(),
                        next.chosen_id.is_some(),
                        next.chosen_missing
                    );
                    publish(&next);
                    last = Some(next);
                }
                let (lock, cv) = wake();
                let mut woken = lock.lock().unwrap_or_else(|p| p.into_inner());
                if !*woken {
                    woken = cv
                        .wait_timeout(woken, WATCH_INTERVAL)
                        .map(|(g, _)| g)
                        .unwrap_or_else(|p| p.into_inner().0);
                }
                forced = *woken;
                *woken = false;
            }
        });
    if let Err(e) = spawned {
        log::warn!("mic_select watch spawn_failed error={e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn avail() -> Vec<(String, &'static str)> {
        vec![
            ("coreaudio:BuiltInMicrophoneDevice".to_string(), "built-in"),
            (
                "coreaudio:AppleUSBAudioEngine:Shure:MV7:1".to_string(),
                "mv7",
            ),
            ("coreaudio:AirPods-1234".to_string(), "airpods"),
        ]
    }

    #[test]
    fn device_resolution_picks_the_saved_device_by_stable_id() {
        let r = resolve(
            Some("coreaudio:AppleUSBAudioEngine:Shure:MV7:1"),
            &avail(),
            Some("built-in"),
        );
        assert_eq!(r, Resolution::Chosen("mv7"));
        assert_eq!(r.source(), "chosen");
        assert!(!r.chosen_missing());
    }

    #[test]
    fn device_resolution_falls_back_to_default_when_the_saved_device_is_missing() {
        let r = resolve(Some("coreaudio:Unplugged-Rode"), &avail(), Some("built-in"));
        assert_eq!(r, Resolution::FellBackToDefault("built-in"));
        assert!(r.chosen_missing());
        assert_eq!(r.clone().device(), Some("built-in"));
    }

    #[test]
    fn device_resolution_with_nothing_saved_uses_the_system_default() {
        assert_eq!(
            resolve(None, &avail(), Some("airpods")),
            Resolution::SystemDefault("airpods")
        );
        // An empty saved id is "nothing saved", never a match on an unnamed device.
        let with_unnamed = vec![(String::new(), "unnamed")];
        assert_eq!(
            resolve(Some(""), &with_unnamed, Some("airpods")),
            Resolution::SystemDefault("airpods")
        );
    }

    #[test]
    fn device_resolution_matches_id_not_name() {
        // Two identical USB mics: same display name, different stable ids.
        let twins = vec![
            ("coreaudio:USB-A".to_string(), "left"),
            ("coreaudio:USB-B".to_string(), "right"),
        ];
        assert_eq!(
            resolve(Some("coreaudio:USB-B"), &twins, Some("left")),
            Resolution::Chosen("right")
        );
    }

    #[test]
    fn device_resolution_with_no_input_at_all() {
        let none: Vec<(String, &str)> = Vec::new();
        assert_eq!(
            resolve(Some("coreaudio:X"), &none, None),
            Resolution::NoInput {
                chosen_missing: true
            }
        );
        assert_eq!(
            resolve::<&str>(None, &none, None),
            Resolution::NoInput {
                chosen_missing: false
            }
        );
    }

    fn dev(id: &str, name: &str) -> MicDevice {
        MicDevice {
            id: id.into(),
            name: name.into(),
            is_system_default: false,
        }
    }

    #[test]
    fn list_marks_the_default_and_the_device_in_use() {
        let choice = MicChoice {
            id: Some("coreaudio:MV7".into()),
            name: Some("MV7".into()),
        };
        let l = build_list(
            vec![
                dev("coreaudio:Built-in", "MacBook Pro Microphone"),
                dev("coreaudio:MV7", "MV7"),
            ],
            Some("coreaudio:Built-in"),
            &choice,
        );
        assert_eq!(l.system_default_name(), Some("MacBook Pro Microphone"));
        assert_eq!(l.in_use_id.as_deref(), Some("coreaudio:MV7"));
        assert!(!l.chosen_missing);

        // Unplug the MV7: the pin is kept, the next take uses the default.
        let l = build_list(
            vec![dev("coreaudio:Built-in", "MacBook Pro Microphone")],
            Some("coreaudio:Built-in"),
            &choice,
        );
        assert!(l.chosen_missing);
        assert_eq!(l.chosen_id.as_deref(), Some("coreaudio:MV7"));
        assert_eq!(l.in_use_id.as_deref(), Some("coreaudio:Built-in"));
    }

    #[test]
    fn list_updates_when_a_device_is_plugged_in() {
        let choice = MicChoice::default();
        let before = build_list(
            vec![dev("coreaudio:Built-in", "MacBook Pro Microphone")],
            Some("coreaudio:Built-in"),
            &choice,
        );
        assert!(
            should_publish(None, &before),
            "the first snapshot always publishes"
        );
        assert!(!should_publish(Some(&before), &before.clone()));
        let after = build_list(
            vec![
                dev("coreaudio:Built-in", "MacBook Pro Microphone"),
                dev("coreaudio:MV7", "MV7"),
            ],
            Some("coreaudio:Built-in"),
            &choice,
        );
        assert!(
            should_publish(Some(&before), &after),
            "a plugged-in mic must redraw the pickers"
        );
        assert_eq!(after.devices.len(), 2);
    }

    #[test]
    fn choice_round_trips_and_a_corrupt_file_is_system_default() {
        let dir = std::env::temp_dir().join(format!("yap-mic-select-{}", uuid::Uuid::new_v4()));
        let path = dir.join(FILE_NAME);
        assert_eq!(
            load(&path),
            MicChoice::default(),
            "missing file = System Default"
        );
        let c = MicChoice {
            id: Some("coreaudio:MV7".into()),
            name: Some("MV7".into()),
        };
        save(&path, &c).expect("save");
        assert_eq!(load(&path), c);
        std::fs::write(&path, "{not json").expect("write corrupt");
        assert_eq!(load(&path), MicChoice::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
