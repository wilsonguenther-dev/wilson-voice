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
//!
//! WF-10 adds two rules on top:
//! * a **fallback order** ([`MicChoice::order`], ranked in Settings) — when the
//!   chosen mic is gone, the next take (and a take already running, see
//!   `record::watchdog_poll`) continues on the first ranked mic still plugged
//!   in, before the system default;
//! * a **virtual-device filter** ([`classify`]) — loopback / virtual / aggregate
//!   inputs (BlackHole, Krisp, Teams Audio, …) are decided by their CoreAudio
//!   TRANSPORT TYPE, never by name alone, and are never picked automatically:
//!   only an explicit pick opens one ([`resolve_ranked`]).

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
    /// WF-10 — the user's fallback ranking, best first. Missing from files
    /// written before WF-10, which is "no ranking" (`serde(default)`).
    #[serde(default)]
    pub order: Vec<RankedMic>,
}

/// One row of the fallback ranking: the stable id, plus the name it had when it
/// was ranked so Settings can show a mic that is unplugged right now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedMic {
    pub id: String,
    #[serde(default)]
    pub name: String,
}

impl MicChoice {
    /// The ranked ids, best first, empty ids dropped.
    pub fn order_ids(&self) -> Vec<String> {
        self.order
            .iter()
            .filter(|r| !r.id.is_empty())
            .map(|r| r.id.clone())
            .collect()
    }
}

/// One input device as the pickers list it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicDevice {
    pub id: String,
    pub name: String,
    /// The device macOS currently routes "default input" to.
    pub is_system_default: bool,
    /// WF-10 — a virtual / loopback / aggregate input ([`classify`]). Listed
    /// under "Other devices" and never picked automatically.
    pub is_virtual: bool,
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
    /// WF-10 — the fallback ranking, best first (may name unplugged mics).
    pub order: Vec<RankedMic>,
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
    /// WF-10 — the next device in the fallback ranking (or, with no ranked mic
    /// plugged in, the first physical input) is used: the pinned mic is gone,
    /// or the system default is a virtual device that is never auto-picked.
    FellBackToNext { device: D, chosen_missing: bool },
    /// No input device at all. `chosen_missing` is whether a pin was also lost.
    NoInput { chosen_missing: bool },
}

impl<D> Resolution<D> {
    pub fn device(self) -> Option<D> {
        match self {
            Resolution::Chosen(d)
            | Resolution::SystemDefault(d)
            | Resolution::FellBackToDefault(d) => Some(d),
            Resolution::FellBackToNext { device, .. } => Some(device),
            Resolution::NoInput { .. } => None,
        }
    }

    /// The `source=` value of the telemetry line.
    pub fn source(&self) -> &'static str {
        match self {
            Resolution::Chosen(_) => "chosen",
            Resolution::SystemDefault(_) => "system_default",
            Resolution::FellBackToDefault(_) => "fell_back_to_default",
            Resolution::FellBackToNext { .. } => "fell_back_to_next",
            Resolution::NoInput { .. } => "no_input",
        }
    }

    /// Was a pinned device asked for and not found?
    pub fn chosen_missing(&self) -> bool {
        matches!(
            self,
            Resolution::FellBackToDefault(_)
                | Resolution::FellBackToNext {
                    chosen_missing: true,
                    ..
                }
                | Resolution::NoInput {
                    chosen_missing: true
                }
        )
    }
}

/// WF-09's two-rule decision (pin, else system default), by STABLE id. The
/// live path is [`resolve_ranked`] since WF-10; this stays as the reference its
/// tests pin the id-matching rules against.
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

// ─── WF-10: device kind (transport type) ────────────────────────────────────

/// Physical microphone, or a virtual / loopback / aggregate input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceKind {
    Physical,
    Virtual,
}

/// CoreAudio `kAudioDevicePropertyTransportType` values (`AudioHardwareBase.h`).
pub mod transport {
    pub(crate) const fn fourcc(code: &[u8; 4]) -> u32 {
        ((code[0] as u32) << 24)
            | ((code[1] as u32) << 16)
            | ((code[2] as u32) << 8)
            | (code[3] as u32)
    }
    /// `kAudioDeviceTransportTypeUnknown` — the HAL will not say.
    pub const UNKNOWN: u32 = 0;
    pub const BUILT_IN: u32 = fourcc(b"bltn");
    pub const AGGREGATE: u32 = fourcc(b"grup");
    pub const AUTO_AGGREGATE: u32 = fourcc(b"fgrp");
    pub const VIRTUAL: u32 = fourcc(b"virt");
    pub const PCI: u32 = fourcc(b"pci ");
    pub const USB: u32 = fourcc(b"usb ");
    pub const FIREWIRE: u32 = fourcc(b"1394");
    pub const BLUETOOTH: u32 = fourcc(b"blue");
    pub const BLUETOOTH_LE: u32 = fourcc(b"blea");
    pub const HDMI: u32 = fourcc(b"hdmi");
    pub const DISPLAY_PORT: u32 = fourcc(b"dprt");
    pub const AIRPLAY: u32 = fourcc(b"airp");
    pub const AVB: u32 = fourcc(b"eavb");
    pub const THUNDERBOLT: u32 = fourcc(b"thun");
    pub const CONTINUITY_CAPTURE: u32 = fourcc(b"ccap");
    pub const CONTINUITY_CAPTURE_WIRED: u32 = fourcc(b"ccwd");
    pub const CONTINUITY_CAPTURE_WIRELESS: u32 = fourcc(b"ccwl");

    /// A transport that is always real hardware.
    pub fn is_physical(t: u32) -> bool {
        [
            BUILT_IN,
            PCI,
            USB,
            FIREWIRE,
            BLUETOOTH,
            BLUETOOTH_LE,
            HDMI,
            DISPLAY_PORT,
            AIRPLAY,
            AVB,
            THUNDERBOLT,
            CONTINUITY_CAPTURE,
            CONTINUITY_CAPTURE_WIRED,
            CONTINUITY_CAPTURE_WIRELESS,
        ]
        .contains(&t)
    }

    /// A transport that is software (a driver, a loopback, an aggregate).
    pub fn is_virtual(t: u32) -> bool {
        t == VIRTUAL || t == AGGREGATE || t == AUTO_AGGREGATE
    }
}

/// Name fragments of well-known virtual inputs — consulted ONLY when the HAL
/// reports no transport type (or one this table does not know).
const VIRTUAL_NAME_HINTS: &[&str] = &[
    "blackhole",
    "loopback",
    "soundflower",
    "krisp",
    "teams audio",
    "microsoft teams",
    "zoomaudiodevice",
    "zoom audio",
    "virtual",
    "aggregate",
];

fn name_looks_virtual(name: &str) -> bool {
    let n = name.to_lowercase();
    VIRTUAL_NAME_HINTS.iter().any(|h| n.contains(h))
}

/// THE classification: the transport type decides whenever the HAL gives one
/// it knows — a USB mic someone named "Virtual Studio" is still a USB mic, and
/// a `virt` driver with a friendly name is still virtual. The name is only the
/// tie-breaker for an UNKNOWN transport.
pub fn classify(transport_type: Option<u32>, name: &str) -> DeviceKind {
    match transport_type {
        Some(t) if transport::is_virtual(t) => DeviceKind::Virtual,
        Some(t) if transport::is_physical(t) => DeviceKind::Physical,
        _ if name_looks_virtual(name) => DeviceKind::Virtual,
        _ => DeviceKind::Physical,
    }
}

/// One input plugged in now, as the ranked resolver sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate<D> {
    pub id: String,
    pub device: D,
    pub is_virtual: bool,
}

/// WF-10 — THE decision with the fallback ranking and the virtual filter.
///
/// 1. The pinned device (`wanted`) is used whenever it is plugged in — even a
///    virtual one: that was an explicit pick.
/// 2. Nothing pinned: the system default, if it is physical.
/// 3. Otherwise the first mic in `order` that is plugged in and physical —
///    "continues on the next one".
/// 4. A pinned mic is gone and nothing ranked is here: the system default, if
///    physical.
/// 5. Otherwise the first physical input at all.
/// 6. Otherwise no input. A virtual device is NEVER reached by steps 2–5.
pub fn resolve_ranked<D: Clone>(
    wanted: Option<&str>,
    order: &[String],
    available: &[Candidate<D>],
    default_id: Option<&str>,
) -> Resolution<D> {
    let wanted = wanted.filter(|w| !w.is_empty());
    let find = |id: &str| available.iter().find(|c| !c.id.is_empty() && c.id == id);
    if let Some(c) = wanted.and_then(find) {
        return Resolution::Chosen(c.device.clone());
    }
    let chosen_missing = wanted.is_some();
    let physical_default = default_id
        .filter(|d| !d.is_empty())
        .and_then(find)
        .filter(|c| !c.is_virtual);
    if !chosen_missing {
        if let Some(c) = physical_default {
            return Resolution::SystemDefault(c.device.clone());
        }
    }
    let ranked = order
        .iter()
        .filter(|id| !id.is_empty() && Some(id.as_str()) != wanted)
        .filter_map(|id| find(id.as_str()))
        .find(|c| !c.is_virtual);
    if let Some(c) = ranked {
        return Resolution::FellBackToNext {
            device: c.device.clone(),
            chosen_missing,
        };
    }
    if chosen_missing {
        if let Some(c) = physical_default {
            return Resolution::FellBackToDefault(c.device.clone());
        }
    }
    match available.iter().find(|c| !c.is_virtual && !c.id.is_empty()) {
        Some(c) => Resolution::FellBackToNext {
            device: c.device.clone(),
            chosen_missing,
        },
        None => Resolution::NoInput { chosen_missing },
    }
}

/// Reorder the ranking: `ids` best first, names looked up in `devices` and then
/// in the previous ranking (so an unplugged mic keeps its name). Duplicates and
/// empty ids are dropped.
pub fn build_order(
    ids: &[String],
    devices: &[MicDevice],
    previous: &[RankedMic],
) -> Vec<RankedMic> {
    let mut out: Vec<RankedMic> = Vec::new();
    for id in ids {
        if id.is_empty() || out.iter().any(|r| &r.id == id) {
            continue;
        }
        let name = devices
            .iter()
            .find(|d| &d.id == id)
            .map(|d| d.name.clone())
            .or_else(|| {
                previous
                    .iter()
                    .find(|r| &r.id == id)
                    .map(|r| r.name.clone())
            })
            .unwrap_or_default();
        out.push(RankedMic {
            id: id.clone(),
            name,
        });
    }
    out
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
    let candidates: Vec<Candidate<MicDevice>> = devices
        .iter()
        .map(|d| Candidate {
            id: d.id.clone(),
            device: d.clone(),
            is_virtual: d.is_virtual,
        })
        .collect();
    let resolution = resolve_ranked(
        choice.id.as_deref(),
        &choice.order_ids(),
        &candidates,
        default_id,
    );
    let chosen_missing = resolution.chosen_missing();
    let in_use = resolution.device();
    MicList {
        chosen_id: choice.id.clone().filter(|s| !s.is_empty()),
        chosen_name: choice.name.clone(),
        chosen_missing,
        in_use_id: in_use.as_ref().map(|d| d.id.clone()),
        in_use_name: in_use.map(|d| d.name),
        order: choice.order.clone(),
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
        // WF-10 — a pick never forgets the ranking.
        order: current().order,
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

/// WF-10 — save a new fallback ranking (best first) from Settings. The pin is
/// kept; the generation moves so a warm idle stream reopens on the next take.
pub fn set_order(ids: Vec<String>) -> Result<MicList, String> {
    let snapshot = list();
    let default_id = snapshot
        .devices
        .iter()
        .find(|d| d.is_system_default)
        .map(|d| d.id.clone());
    let prev = current();
    let order = build_order(&ids, &snapshot.devices, &prev.order);
    let ranked = order.len();
    let choice = MicChoice { order, ..prev };
    if let Err(e) = save(&choice_path(), &choice) {
        log::warn!("mic_select order_failed error={e}");
        return Err(e);
    }
    *cached().lock().unwrap_or_else(|p| p.into_inner()) = Some(choice.clone());
    GENERATION.fetch_add(1, Ordering::SeqCst);
    log::info!("mic_select order_saved ranked={ranked}");
    poke();
    Ok(build_list(snapshot.devices, default_id.as_deref(), &choice))
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

/// WF-10 — the CoreAudio transport type of the device with this stable id
/// (`coreaudio:<UID>`), or `None` when the HAL cannot say. UID → AudioObjectID
/// via `kAudioHardwarePropertyTranslateUIDToDevice`, then
/// `kAudioDevicePropertyTransportType`. Two property reads; no allocation kept.
#[cfg(target_os = "macos")]
pub fn transport_type_for_id(stable_id: &str) -> Option<u32> {
    use core_foundation::base::TCFType;
    use core_foundation::string::{CFString, CFStringRef};
    use std::os::raw::c_void;

    #[repr(C)]
    struct AudioObjectPropertyAddress {
        selector: u32,
        scope: u32,
        element: u32,
    }
    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyData(
            in_object_id: u32,
            in_address: *const AudioObjectPropertyAddress,
            in_qualifier_data_size: u32,
            in_qualifier_data: *const c_void,
            io_data_size: *mut u32,
            out_data: *mut c_void,
        ) -> i32;
    }
    const SYSTEM_OBJECT: u32 = 1;
    const SCOPE_GLOBAL: u32 = transport::fourcc(b"glob");
    const TRANSLATE_UID_TO_DEVICE: u32 = transport::fourcc(b"uidd");
    const TRANSPORT_TYPE: u32 = transport::fourcc(b"tran");

    let uid = stable_id.strip_prefix("coreaudio:").unwrap_or(stable_id);
    if uid.is_empty() {
        return None;
    }
    let cf_uid = CFString::new(uid);
    let uid_ref: CFStringRef = cf_uid.as_concrete_TypeRef();
    let mut device: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let translate = AudioObjectPropertyAddress {
        selector: TRANSLATE_UID_TO_DEVICE,
        scope: SCOPE_GLOBAL,
        element: 0,
    };
    // SAFETY: the qualifier is a pointer to one live CFStringRef (`cf_uid`
    // outlives the call) and the out buffer is one u32 with its size passed.
    let status = unsafe {
        AudioObjectGetPropertyData(
            SYSTEM_OBJECT,
            &translate,
            std::mem::size_of::<CFStringRef>() as u32,
            &uid_ref as *const CFStringRef as *const c_void,
            &mut size,
            &mut device as *mut u32 as *mut c_void,
        )
    };
    if status != 0 || device == 0 {
        return None;
    }
    let read = AudioObjectPropertyAddress {
        selector: TRANSPORT_TYPE,
        scope: SCOPE_GLOBAL,
        element: 0,
    };
    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: one u32 out buffer with its size passed, no qualifier.
    let status = unsafe {
        AudioObjectGetPropertyData(
            device,
            &read,
            0,
            std::ptr::null(),
            &mut size,
            &mut value as *mut u32 as *mut c_void,
        )
    };
    (status == 0).then_some(value)
}

#[cfg(not(target_os = "macos"))]
pub fn transport_type_for_id(_stable_id: &str) -> Option<u32> {
    None
}

/// A device's kind from its stable id + name ([`classify`] over the HAL).
pub fn device_kind(stable_id: &str, name: &str) -> DeviceKind {
    classify(transport_type_for_id(stable_id), name)
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
    let wanted = choice.id.as_deref().filter(|s| !s.is_empty());
    let default = host.default_input_device();
    // Fast path (the old one): nothing pinned and the default is a real mic.
    if wanted.is_none() {
        if let Some(d) = default.as_ref() {
            let name = device_name(d);
            let kind = match device_id(d) {
                Some(id) => device_kind(&id, &name),
                None => classify(None, &name),
            };
            if kind == DeviceKind::Physical {
                log::debug!("mic_select resolve source=system_default");
                return Some((d.clone(), name));
            }
            log::info!("mic_select default_is_virtual skipping=true");
        }
    }
    let default_id = default.as_ref().and_then(device_id);
    let available: Vec<Candidate<cpal::Device>> = enumerate_inputs(&host)
        .into_iter()
        .map(|(id, name, d)| {
            let is_virtual = device_kind(&id, &name) == DeviceKind::Virtual;
            Candidate {
                id,
                device: d,
                is_virtual,
            }
        })
        .collect();
    let virtual_skipped = available.iter().filter(|c| c.is_virtual).count();
    let resolution = resolve_ranked(
        wanted,
        &choice.order_ids(),
        &available,
        default_id.as_deref(),
    );
    if resolution.chosen_missing() {
        log::warn!(
            "mic_select resolve source={} chosen_missing=true ranked={} virtual_skipped={virtual_skipped}",
            resolution.source(),
            choice.order.len()
        );
    } else {
        log::debug!(
            "mic_select resolve source={} virtual_skipped={virtual_skipped}",
            resolution.source()
        );
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
        .map(|(id, name, _)| {
            let is_virtual = device_kind(&id, &name) == DeviceKind::Virtual;
            MicDevice {
                id,
                name,
                is_system_default: false,
                is_virtual,
            }
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
            is_virtual: false,
        }
    }

    fn cand(id: &str, device: &'static str, is_virtual: bool) -> Candidate<&'static str> {
        Candidate {
            id: id.into(),
            device,
            is_virtual,
        }
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // ── WF-10 ────────────────────────────────────────────────────────────────

    #[test]
    fn virtual_devices_are_classified_by_transport_type_not_name() {
        // The transport decides whenever the HAL gives one it knows.
        assert_eq!(
            classify(Some(transport::VIRTUAL), "Studio Microphone"),
            DeviceKind::Virtual,
            "a `virt` driver with a friendly name is still virtual"
        );
        assert_eq!(
            classify(Some(transport::AGGREGATE), "My Setup"),
            DeviceKind::Virtual
        );
        assert_eq!(
            classify(Some(transport::USB), "BlackHole Virtual Interface"),
            DeviceKind::Physical,
            "a USB mic whose name looks virtual is still a USB mic"
        );
        assert_eq!(
            classify(Some(transport::BUILT_IN), "Microsoft Teams Audio"),
            DeviceKind::Physical
        );
        assert_eq!(
            classify(Some(transport::BLUETOOTH), "AirPods Pro"),
            DeviceKind::Physical
        );
        // Only an UNKNOWN transport falls back to the name.
        assert_eq!(
            classify(Some(transport::UNKNOWN), "BlackHole 2ch"),
            DeviceKind::Virtual
        );
        assert_eq!(classify(None, "Krisp Microphone"), DeviceKind::Virtual);
        assert_eq!(classify(None, "Shure MV7"), DeviceKind::Physical);
        assert_eq!(classify(None, ""), DeviceKind::Physical);
    }

    #[test]
    fn transport_fourccs_match_core_audio() {
        assert_eq!(transport::VIRTUAL, 0x7669_7274); // 'virt'
        assert_eq!(transport::USB, 0x7573_6220); // 'usb '
        assert_eq!(transport::BUILT_IN, 0x626c_746e); // 'bltn'
        assert!(transport::is_virtual(transport::AUTO_AGGREGATE));
        assert!(!transport::is_physical(transport::UNKNOWN));
        assert!(!transport::is_virtual(transport::UNKNOWN));
    }

    #[test]
    fn a_missing_pin_falls_to_the_next_ranked_mic_before_the_default() {
        let avail = vec![
            cand("coreaudio:Built-in", "built-in", false),
            cand("coreaudio:AirPods", "airpods", false),
        ];
        let r = resolve_ranked(
            Some("coreaudio:MV7"),
            &ids(&["coreaudio:MV7", "coreaudio:AirPods", "coreaudio:Built-in"]),
            &avail,
            Some("coreaudio:Built-in"),
        );
        assert_eq!(
            r,
            Resolution::FellBackToNext {
                device: "airpods",
                chosen_missing: true
            }
        );
        assert!(r.chosen_missing());
        assert_eq!(r.source(), "fell_back_to_next");
    }

    #[test]
    fn the_pinned_mic_wins_while_it_is_plugged_in() {
        let avail = vec![
            cand("coreaudio:Built-in", "built-in", false),
            cand("coreaudio:MV7", "mv7", false),
        ];
        assert_eq!(
            resolve_ranked(
                Some("coreaudio:MV7"),
                &ids(&["coreaudio:Built-in"]),
                &avail,
                Some("coreaudio:Built-in"),
            ),
            Resolution::Chosen("mv7")
        );
        // An explicit pick of a virtual device is honoured — only AUTO picks skip.
        let with_virtual = vec![
            cand("coreaudio:Built-in", "built-in", false),
            cand("coreaudio:Krisp", "krisp", true),
        ];
        assert_eq!(
            resolve_ranked(Some("coreaudio:Krisp"), &[], &with_virtual, None),
            Resolution::Chosen("krisp")
        );
    }

    #[test]
    fn a_virtual_device_is_never_auto_picked() {
        // The system default is BlackHole: skipped for the first physical mic.
        let avail = vec![
            cand("coreaudio:BlackHole", "blackhole", true),
            cand("coreaudio:Built-in", "built-in", false),
        ];
        assert_eq!(
            resolve_ranked(None, &[], &avail, Some("coreaudio:BlackHole")),
            Resolution::FellBackToNext {
                device: "built-in",
                chosen_missing: false
            }
        );
        // A virtual device ranked first is skipped in the fallback walk too.
        assert_eq!(
            resolve_ranked(
                Some("coreaudio:Gone"),
                &ids(&["coreaudio:BlackHole", "coreaudio:Built-in"]),
                &avail,
                Some("coreaudio:BlackHole"),
            ),
            Resolution::FellBackToNext {
                device: "built-in",
                chosen_missing: true
            }
        );
        // Only virtual inputs left: no input, never the loopback.
        let only_virtual = vec![
            cand("coreaudio:BlackHole", "blackhole", true),
            cand("coreaudio:Teams", "teams", true),
        ];
        assert_eq!(
            resolve_ranked(
                Some("coreaudio:MV7"),
                &ids(&["coreaudio:Teams"]),
                &only_virtual,
                Some("coreaudio:BlackHole"),
            ),
            Resolution::NoInput {
                chosen_missing: true
            }
        );
    }

    #[test]
    fn nothing_pinned_and_a_physical_default_is_the_old_path() {
        let avail = vec![
            cand("coreaudio:Built-in", "built-in", false),
            cand("coreaudio:MV7", "mv7", false),
        ];
        assert_eq!(
            resolve_ranked(
                None,
                &ids(&["coreaudio:MV7"]),
                &avail,
                Some("coreaudio:Built-in")
            ),
            Resolution::SystemDefault("built-in")
        );
        // A missing pin with nothing ranked plugged in: the physical default.
        assert_eq!(
            resolve_ranked(
                Some("coreaudio:Gone"),
                &ids(&["coreaudio:AlsoGone"]),
                &avail,
                Some("coreaudio:Built-in")
            ),
            Resolution::FellBackToDefault("built-in")
        );
    }

    #[test]
    fn build_order_keeps_names_of_unplugged_mics_and_drops_duplicates() {
        let prev = vec![RankedMic {
            id: "coreaudio:AirPods".into(),
            name: "AirPods Pro".into(),
        }];
        let order = build_order(
            &ids(&["coreaudio:MV7", "", "coreaudio:AirPods", "coreaudio:MV7"]),
            &[dev("coreaudio:MV7", "Shure MV7")],
            &prev,
        );
        assert_eq!(order.len(), 2);
        assert_eq!(order[0].name, "Shure MV7");
        assert_eq!(order[1].name, "AirPods Pro");
    }

    #[test]
    fn list_puts_the_ranking_on_the_snapshot_and_flags_virtual_inputs() {
        let mut blackhole = dev("coreaudio:BlackHole", "BlackHole 2ch");
        blackhole.is_virtual = true;
        let choice = MicChoice {
            id: Some("coreaudio:MV7".into()),
            name: Some("MV7".into()),
            order: vec![
                RankedMic {
                    id: "coreaudio:MV7".into(),
                    name: "MV7".into(),
                },
                RankedMic {
                    id: "coreaudio:AirPods".into(),
                    name: "AirPods Pro".into(),
                },
            ],
        };
        // MV7 unplugged, AirPods in, the system default is BlackHole.
        let l = build_list(
            vec![
                dev("coreaudio:Built-in", "MacBook Pro Microphone"),
                dev("coreaudio:AirPods", "AirPods Pro"),
                blackhole,
            ],
            Some("coreaudio:BlackHole"),
            &choice,
        );
        assert!(l.chosen_missing);
        assert_eq!(l.in_use_id.as_deref(), Some("coreaudio:AirPods"));
        assert_eq!(l.order.len(), 2);
        assert!(l
            .devices
            .iter()
            .any(|d| d.is_virtual && d.is_system_default));
    }

    #[test]
    fn a_pre_wf10_choice_file_still_loads_with_no_ranking() {
        let c: MicChoice =
            serde_json::from_str(r#"{"id":"coreaudio:MV7","name":"MV7"}"#).expect("parse");
        assert!(c.order.is_empty());
        assert_eq!(c.id.as_deref(), Some("coreaudio:MV7"));
    }

    #[test]
    fn list_marks_the_default_and_the_device_in_use() {
        let choice = MicChoice {
            id: Some("coreaudio:MV7".into()),
            name: Some("MV7".into()),
            ..MicChoice::default()
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
            ..MicChoice::default()
        };
        save(&path, &c).expect("save");
        assert_eq!(load(&path), c);
        std::fs::write(&path, "{not json").expect("write corrupt");
        assert_eq!(load(&path), MicChoice::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
