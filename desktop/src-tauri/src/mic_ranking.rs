//! PERM-H — microphone ranking and device intelligence.
//!
//! Yap used to record from whatever macOS called the default input. That is a
//! single device pick the user never made: plug in a USB mic and unplug it, and
//! the next take goes wherever the system decided. This module owns the four
//! things that replace it:
//!
//! 1. **An ordered preference list** ([`MicRanking`]), persisted in
//!    `AppSettings::mic_ranking`. At take start the HIGHEST-RANKED PRESENT
//!    device wins; with no ranked device present the system default does, so
//!    an empty list behaves exactly like the app did before. "Forget" removes a
//!    row, so an old headset stops winning.
//! 2. **The AirPods warning.** Bluetooth input runs at a low-bandwidth headset
//!    profile and is noticeably worse for speech recognition — the single most
//!    common cause of a bad transcript that looks like a model problem. Said
//!    ONCE per selection ([`WarningLatch`]), with the built-in mic offered.
//! 3. **The clamshell warning.** With the lid closed the built-in mic is
//!    muffled or unavailable. Detected from IOKit's `AppleClamshellState`.
//! 4. **A device change mid-take** ([`mid_take_decision`]). The take either
//!    continues on the new device — and the user is TOLD, never switched
//!    silently — or, with nothing left to record from, is parked: every sample
//!    captured so far is kept and the take is stamped with the device failure
//!    (record.rs, YV67) instead of being silently truncated.
//!
//! Presence is a HARDWARE question and is answered here from the device list.
//! Authorization is a TCC question and lives in [`crate::mic_auth`]. PERM-A
//! split them deliberately and nothing in this file asks TCC anything — see
//! `presence_and_authorization_stay_separate`.
//!
//! Not done, on purpose: vendor wear detection (Jabra's "headset is on"
//! auto-switch). It is vendor-specific HID and out of scope for this item.

use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

/// More rows than this is a list nobody curates; the oldest fall off the end.
pub const MAX_RANKED: usize = 16;

/// The settings key (camelCase in settings.json) the ranking is stored under.
pub const SETTINGS_FIELD: &str = "micRanking";

/// CoreAudio `kAudioDeviceTransportType*` four-char codes we care about.
pub mod transport {
    /// `'blue'` — classic Bluetooth (AirPods on the HFP/SCO input profile).
    pub const BLUETOOTH: u32 = u32::from_be_bytes(*b"blue");
    /// `'blea'` — Bluetooth LE.
    pub const BLUETOOTH_LE: u32 = u32::from_be_bytes(*b"blea");
    /// `'bltn'` — the Mac's own built-in device.
    pub const BUILT_IN: u32 = u32::from_be_bytes(*b"bltn");
}

/// What kind of microphone a device is, as far as transcript quality goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceClass {
    /// The Mac's own microphone — muffled with the lid closed.
    BuiltIn,
    /// AirPods-class input: Bluetooth, low-bandwidth, worse for ASR.
    Bluetooth,
    /// USB, Thunderbolt, aggregate, virtual — nothing to warn about.
    Other,
}

/// Classify a device. The CoreAudio transport type wins when we have it; the
/// name is the fallback (and the only signal off macOS or when the HAL query
/// fails), so the name rules are deliberately narrow.
pub fn classify(name: &str, transport_type: Option<u32>) -> DeviceClass {
    match transport_type {
        Some(transport::BLUETOOTH) | Some(transport::BLUETOOTH_LE) => {
            return DeviceClass::Bluetooth
        }
        Some(transport::BUILT_IN) => return DeviceClass::BuiltIn,
        Some(_) => return DeviceClass::Other,
        None => {}
    }
    let lower = name.to_lowercase();
    if ["airpods", "beats", "bluetooth", "hands-free", "handsfree"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        DeviceClass::Bluetooth
    } else if ["macbook", "built-in", "imac"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        DeviceClass::BuiltIn
    } else {
        DeviceClass::Other
    }
}

/// Why a device was chosen at take start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "rank")]
pub enum PickReason {
    /// The highest-ranked device that is present; the number is its 0-based rank.
    Ranked(usize),
    /// No ranked device is present — macOS's default input.
    SystemDefault,
    /// No ranked device and no system default: the first input listed.
    FirstPresent,
}

/// The device a take should open, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub name: String,
    pub reason: PickReason,
}

/// The user's ordered microphone preference list. Index 0 is the favourite.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MicRanking {
    order: Vec<String>,
}

impl MicRanking {
    /// Build from the stored list: blank names dropped, duplicates collapsed
    /// to their FIRST (highest) position, capped at [`MAX_RANKED`].
    pub fn from_settings(stored: &[String]) -> Self {
        let mut order: Vec<String> = Vec::new();
        for name in stored {
            let name = name.trim();
            if name.is_empty() || order.iter().any(|n| n == name) {
                continue;
            }
            order.push(name.to_string());
        }
        order.truncate(MAX_RANKED);
        Self { order }
    }

    /// The ranked names, favourite first. This is what settings.json stores.
    pub fn order(&self) -> &[String] {
        &self.order
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// 0-based rank of `name`, or `None` when it is not on the list.
    pub fn rank_of(&self, name: &str) -> Option<usize> {
        self.order.iter().position(|n| n == name)
    }

    /// The user picked `name`: it moves (or is added) to the top.
    pub fn select(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        self.order.retain(|n| n != name);
        self.order.insert(0, name.to_string());
        self.order.truncate(MAX_RANKED);
    }

    /// Drag-to-reorder: the list becomes `new_order`, normalised the same way
    /// a stored list is.
    pub fn reorder(&mut self, new_order: &[String]) {
        *self = Self::from_settings(new_order);
    }

    /// Remove `name` so it can no longer win. `true` when a row was removed.
    pub fn forget(&mut self, name: &str) -> bool {
        let before = self.order.len();
        self.order.retain(|n| n != name);
        self.order.len() != before
    }

    /// Take-start resolution: the highest-ranked device in `present` wins;
    /// otherwise the system default (when it is actually present); otherwise
    /// the first present device. `None` only when nothing is present.
    pub fn pick(&self, present: &[String], system_default: Option<&str>) -> Option<Pick> {
        if let Some((rank, name)) = self
            .order
            .iter()
            .enumerate()
            .find(|(_, name)| present.iter().any(|p| p == *name))
        {
            return Some(Pick {
                name: name.clone(),
                reason: PickReason::Ranked(rank),
            });
        }
        if let Some(default) = system_default {
            if present.iter().any(|p| p == default) {
                return Some(Pick {
                    name: default.to_string(),
                    reason: PickReason::SystemDefault,
                });
            }
        }
        present.first().map(|name| Pick {
            name: name.clone(),
            reason: PickReason::FirstPresent,
        })
    }
}

/// What a user is told. Serialized as-is to the `mic-notice` event and shown
/// as a macOS notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicNotice {
    pub kind: NoticeKind,
    /// The device the notice is about.
    pub device: String,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    /// An AirPods-class (Bluetooth) mic was selected.
    Airpods,
    /// The lid is closed and the built-in mic was selected.
    Clamshell,
    /// The input changed during a take and the take continued on a new device.
    SwitchedMidTake,
    /// The input went away during a take and nothing else could take over.
    ParkedMidTake,
}

impl MicNotice {
    pub fn airpods(device: &str) -> Self {
        Self {
            kind: NoticeKind::Airpods,
            device: device.to_string(),
            title: "Bluetooth mics make transcripts worse".into(),
            body: format!(
                "{device} sends low-quality audio over Bluetooth, so dictation comes out \
                 noticeably worse. Use your Mac's built-in microphone for better results."
            ),
        }
    }

    pub fn clamshell(device: &str) -> Self {
        Self {
            kind: NoticeKind::Clamshell,
            device: device.to_string(),
            title: "Your Mac's lid is closed".into(),
            body: format!(
                "With the lid closed, {device} is muffled or unavailable. Open the lid \
                 or connect another microphone."
            ),
        }
    }

    pub fn switched(from: &str, to: &str) -> Self {
        Self {
            kind: NoticeKind::SwitchedMidTake,
            device: to.to_string(),
            title: "Microphone changed while recording".into(),
            body: format!(
                "{from} stopped, so Yap kept recording on {to}. Nothing recorded so far was lost."
            ),
        }
    }

    pub fn parked(from: &str) -> Self {
        Self {
            kind: NoticeKind::ParkedMidTake,
            device: from.to_string(),
            title: "Microphone disconnected while recording".into(),
            body: format!(
                "{from} stopped and no other microphone is available. Yap kept everything \
                 recorded so far; connect a mic to keep going."
            ),
        }
    }
}

/// What a running take does when the input device set changes under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidTake {
    /// Keep recording on the device the take is already on.
    Stay,
    /// Carry the take across to `to` and tell the user with `notice`.
    SwitchTo { to: String, notice: MicNotice },
    /// Nothing to record from. The take is kept (parked) and the user told.
    Park { notice: MicNotice },
}

/// The ONE rule for an input change during a take. Never returns a silent
/// switch: every `SwitchTo` and `Park` carries the notice the user sees.
///
/// * The current device is still present and the user has a ranking: STAY.
///   A higher-ranked device arriving mid-take (AirPods reconnecting) does not
///   yank the take; it wins at the next take start.
/// * The current device is present and there is no ranking: follow macOS's
///   default input, which is what Yap did before rankings existed (YV92).
/// * The current device is gone: switch to the take-start pick, or park.
pub fn mid_take_decision(
    ranking: &MicRanking,
    current: &str,
    present: &[String],
    system_default: Option<&str>,
) -> MidTake {
    let current_present = present.iter().any(|p| p == current);
    if current_present {
        if !ranking.is_empty() {
            return MidTake::Stay;
        }
        return match system_default {
            Some(default) if default != current && present.iter().any(|p| p == default) => {
                MidTake::SwitchTo {
                    to: default.to_string(),
                    notice: MicNotice::switched(current, default),
                }
            }
            _ => MidTake::Stay,
        };
    }
    match ranking.pick(present, system_default) {
        Some(pick) => MidTake::SwitchTo {
            notice: MicNotice::switched(current, &pick.name),
            to: pick.name,
        },
        None => MidTake::Park {
            notice: MicNotice::parked(current),
        },
    }
}

/// Fires each warning ONCE PER SELECTION. Picking the same AirPods again (or
/// starting ten takes on them) says nothing; picking another device and then
/// coming back is a new selection and warns again.
#[derive(Debug, Default)]
pub struct WarningLatch {
    selection: Option<String>,
    clamshell_warned: bool,
}

impl WarningLatch {
    pub const fn new() -> Self {
        Self {
            selection: None,
            clamshell_warned: false,
        }
    }

    /// Fold in the device that is now selected/opened and return what to say.
    pub fn observe(
        &mut self,
        device: &str,
        class: DeviceClass,
        lid_closed: bool,
    ) -> Vec<MicNotice> {
        let mut out = Vec::new();
        let new_selection = self.selection.as_deref() != Some(device);
        if new_selection {
            self.selection = Some(device.to_string());
            self.clamshell_warned = false;
            if class == DeviceClass::Bluetooth {
                out.push(MicNotice::airpods(device));
            }
        }
        if class == DeviceClass::BuiltIn && lid_closed {
            if !self.clamshell_warned {
                self.clamshell_warned = true;
                out.push(MicNotice::clamshell(device));
            }
        } else if !lid_closed {
            // Opening the lid re-arms it: closing it again is worth a word.
            self.clamshell_warned = false;
        }
        out
    }
}

// ── the process-wide half: the live ranking, the latch, the notifier ──────────

static RANKING: Mutex<MicRanking> = Mutex::new(MicRanking { order: Vec::new() });
static LATCH: Mutex<WarningLatch> = Mutex::new(WarningLatch::new());
/// The device a parked take last announced, so a parked take is announced once
/// and not every watchdog tick. Cleared at every take start.
static PARKED: Mutex<Option<String>> = Mutex::new(None);
type Notifier = Box<dyn Fn(&MicNotice) + Send + Sync>;
static NOTIFIER: OnceLock<Notifier> = OnceLock::new();

/// Make `ranking` the one the capture path resolves against.
pub fn install(ranking: MicRanking) {
    if let Ok(mut live) = RANKING.lock() {
        *live = ranking;
    }
}

/// The live ranking.
pub fn current() -> MicRanking {
    RANKING.lock().map(|r| r.clone()).unwrap_or_default()
}

/// Wire the user-facing sink (a macOS notification + the `mic-notice` event).
/// Set once at app setup; later calls are ignored.
pub fn set_notifier(f: Notifier) {
    let _ = NOTIFIER.set(f);
}

/// Tell the user. Logged either way, so the support bundle carries it.
pub fn announce(notice: &MicNotice) {
    log::warn!("mic notice ({:?}): {}", notice.kind, notice.title);
    if let Some(notify) = NOTIFIER.get() {
        notify(notice);
    }
}

/// Run the latch for a device the user just picked in Settings. Returns the
/// notices so the screen can show them inline (with the built-in mic offered).
pub fn on_selected(device: &str) -> Vec<MicNotice> {
    let class = classify(device, transport_type_of(device));
    let lid = lid_closed().unwrap_or(false);
    LATCH
        .lock()
        .map(|mut latch| latch.observe(device, class, lid))
        .unwrap_or_default()
}

/// A take just opened on `device`. Warn (once per selection) about AirPods
/// and about a closed lid. Called OFF the arm path — the HAL and IOKit
/// queries cost milliseconds the hotkey must not wait for.
pub fn on_take_start(device: &str) {
    if let Ok(mut parked) = PARKED.lock() {
        *parked = None;
    }
    for notice in on_selected(device) {
        announce(&notice);
    }
}

/// A take parked on `from`. Announced once per take.
pub fn announce_parked_once(notice: &MicNotice) {
    let first = PARKED
        .lock()
        .map(|mut parked| {
            let first = parked.as_deref() != Some(notice.device.as_str());
            *parked = Some(notice.device.clone());
            first
        })
        .unwrap_or(true);
    if first {
        announce(notice);
    }
}

// ── hardware reads (presence only — never authorization) ─────────────────────

/// The names of every input device that is attached right now, and the name
/// of macOS's default input. A HARDWARE answer: an attached mic Yap is not
/// allowed to use is still present.
pub fn present_inputs() -> (Vec<String>, Option<String>) {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let name_of = |d: &cpal::Device| {
        d.description()
            .map(|d| d.name().to_string())
            .unwrap_or_default()
    };
    let present = host
        .input_devices()
        .map(|devices| {
            devices
                .map(|d| name_of(&d))
                .filter(|n| !n.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let default = host.default_input_device().map(|d| name_of(&d));
    (present, default)
}

/// One row of the Settings list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicDeviceRow {
    pub name: String,
    /// Attached right now (hardware). Says nothing about permission.
    pub present: bool,
    /// 0-based rank, `None` for an attached device the user has not ranked.
    pub rank: Option<usize>,
    pub class: DeviceClass,
    pub system_default: bool,
    /// This is the device the next take will open.
    pub wins: bool,
}

/// What the Settings screen renders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicDevicesView {
    pub rows: Vec<MicDeviceRow>,
    /// `Some(true)` with the lid closed; `None` where it cannot be read
    /// (a desktop Mac, or off macOS).
    pub lid_closed: Option<bool>,
}

/// Ranked rows first (present or not, so an unplugged favourite can still be
/// forgotten), then attached devices nobody ranked yet. Pure, for tests.
pub fn rows(
    ranking: &MicRanking,
    present: &[String],
    system_default: Option<&str>,
    class_of: impl Fn(&str) -> DeviceClass,
) -> Vec<MicDeviceRow> {
    let winner = ranking.pick(present, system_default).map(|p| p.name);
    let row = |name: &str, rank: Option<usize>| MicDeviceRow {
        name: name.to_string(),
        present: present.iter().any(|p| p == name),
        rank,
        class: class_of(name),
        system_default: system_default == Some(name),
        wins: winner.as_deref() == Some(name),
    };
    let mut out: Vec<MicDeviceRow> = ranking
        .order()
        .iter()
        .enumerate()
        .map(|(i, name)| row(name, Some(i)))
        .collect();
    for name in present {
        if ranking.rank_of(name).is_none() && !out.iter().any(|r| &r.name == name) {
            out.push(row(name, None));
        }
    }
    out
}

/// The live view: the device list, ranked, with the lid state.
pub fn devices_view() -> MicDevicesView {
    let (present, default) = present_inputs();
    let transports = sys::transport_types();
    let class_of = |name: &str| {
        classify(
            name,
            transports.iter().find(|(n, _)| n == name).map(|(_, t)| *t),
        )
    };
    MicDevicesView {
        rows: rows(&current(), &present, default.as_deref(), class_of),
        lid_closed: lid_closed(),
    }
}

/// The CoreAudio transport type of the input named `name`, when the HAL knows it.
pub fn transport_type_of(name: &str) -> Option<u32> {
    sys::transport_types()
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, t)| t)
}

/// Is the laptop lid closed? IOKit's `AppleClamshellState`. `None` when the
/// property does not exist (desktop Macs) or cannot be read.
pub fn lid_closed() -> Option<bool> {
    sys::lid_closed()
}

#[cfg(target_os = "macos")]
mod sys {
    use core_foundation::base::{CFType, CFTypeRef, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::string::{CFString, CFStringRef};
    use std::ffi::c_void;

    type AudioObjectID = u32;
    type OSStatus = i32;

    #[repr(C)]
    struct AudioObjectPropertyAddress {
        selector: u32,
        scope: u32,
        element: u32,
    }

    const SYSTEM_OBJECT: AudioObjectID = 1;
    const SCOPE_GLOBAL: u32 = u32::from_be_bytes(*b"glob");
    const ELEMENT_MAIN: u32 = 0;
    const DEVICES: u32 = u32::from_be_bytes(*b"dev#");
    const NAME: u32 = u32::from_be_bytes(*b"lnam");
    const TRANSPORT: u32 = u32::from_be_bytes(*b"tran");

    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyDataSize(
            object: AudioObjectID,
            address: *const AudioObjectPropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            out_size: *mut u32,
        ) -> OSStatus;
        fn AudioObjectGetPropertyData(
            object: AudioObjectID,
            address: *const AudioObjectPropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            io_size: *mut u32,
            out_data: *mut c_void,
        ) -> OSStatus;
    }

    type IoObject = u32;
    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOServiceMatching(name: *const std::os::raw::c_char) -> *mut c_void;
        fn IOServiceGetMatchingService(main_port: u32, matching: *mut c_void) -> IoObject;
        fn IORegistryEntryCreateCFProperty(
            entry: IoObject,
            key: CFStringRef,
            allocator: *const c_void,
            options: u32,
        ) -> CFTypeRef;
        fn IOObjectRelease(object: IoObject) -> i32;
    }

    fn addr(selector: u32) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            selector,
            scope: SCOPE_GLOBAL,
            element: ELEMENT_MAIN,
        }
    }

    /// `(name, transport type)` for every audio device the HAL lists.
    pub fn transport_types() -> Vec<(String, u32)> {
        let mut size: u32 = 0;
        let devices_addr = addr(DEVICES);
        // SAFETY: valid address struct, null qualifier, valid out-param.
        let status = unsafe {
            AudioObjectGetPropertyDataSize(
                SYSTEM_OBJECT,
                &devices_addr,
                0,
                std::ptr::null(),
                &mut size,
            )
        };
        if status != 0 || size == 0 {
            return Vec::new();
        }
        let count = size as usize / std::mem::size_of::<AudioObjectID>();
        let mut ids: Vec<AudioObjectID> = vec![0; count];
        // SAFETY: `ids` has exactly `size` bytes of capacity.
        let status = unsafe {
            AudioObjectGetPropertyData(
                SYSTEM_OBJECT,
                &devices_addr,
                0,
                std::ptr::null(),
                &mut size,
                ids.as_mut_ptr().cast(),
            )
        };
        if status != 0 {
            return Vec::new();
        }
        ids.truncate(size as usize / std::mem::size_of::<AudioObjectID>());
        ids.into_iter()
            .filter_map(|id| Some((device_name(id)?, device_transport(id)?)))
            .collect()
    }

    fn device_name(id: AudioObjectID) -> Option<String> {
        let mut name: CFStringRef = std::ptr::null();
        let mut size = std::mem::size_of::<CFStringRef>() as u32;
        // SAFETY: the property is a CFStringRef returned at +1 (create rule).
        let status = unsafe {
            AudioObjectGetPropertyData(
                id,
                &addr(NAME),
                0,
                std::ptr::null(),
                &mut size,
                (&mut name as *mut CFStringRef).cast(),
            )
        };
        if status != 0 || name.is_null() {
            return None;
        }
        // SAFETY: non-null, owned by us under the create rule.
        Some(unsafe { CFString::wrap_under_create_rule(name) }.to_string())
    }

    fn device_transport(id: AudioObjectID) -> Option<u32> {
        let mut value: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: the property is a UInt32.
        let status = unsafe {
            AudioObjectGetPropertyData(
                id,
                &addr(TRANSPORT),
                0,
                std::ptr::null(),
                &mut size,
                (&mut value as *mut u32).cast(),
            )
        };
        (status == 0).then_some(value)
    }

    pub fn lid_closed() -> Option<bool> {
        // SAFETY: a NUL-terminated literal; IOServiceGetMatchingService
        // consumes the matching dictionary's reference.
        let service = unsafe {
            let matching = IOServiceMatching(c"IOPMrootDomain".as_ptr());
            if matching.is_null() {
                return None;
            }
            IOServiceGetMatchingService(0, matching)
        };
        if service == 0 {
            return None;
        }
        let key = CFString::from_static_string("AppleClamshellState");
        // SAFETY: `service` is a live registry entry; the key outlives the call.
        let value = unsafe {
            IORegistryEntryCreateCFProperty(service, key.as_concrete_TypeRef(), std::ptr::null(), 0)
        };
        // SAFETY: `service` came from IOServiceGetMatchingService.
        unsafe { IOObjectRelease(service) };
        if value.is_null() {
            return None;
        }
        // SAFETY: non-null, returned under the create rule.
        let value = unsafe { CFType::wrap_under_create_rule(value) };
        value.downcast::<CFBoolean>().map(bool::from)
    }
}

#[cfg(not(target_os = "macos"))]
mod sys {
    pub fn transport_types() -> Vec<(String, u32)> {
        Vec::new()
    }
    pub fn lid_closed() -> Option<bool> {
        None
    }
}

/// Every HAL device with its transport type — for the hardware probe test.
#[doc(hidden)]
pub fn sys_transport_types_for_probe() -> Vec<(String, u32)> {
    sys::transport_types()
}
