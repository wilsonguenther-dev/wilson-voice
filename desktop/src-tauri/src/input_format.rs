//! Input format-change handling for a long capture (YV92, plan finding OS-9).
//!
//! ## The defect this module exists for
//!
//! Putting AirPods in mid-recording is not a *device* change the capture path
//! can shrug off — it is a **format** change. AirPods report 24000 Hz in normal
//! operation against 48000 Hz for the built-in mic, and engaging the AirPods
//! *microphone* renegotiates the link to HFP/SCO, which drags the rate down
//! again. macOS announces this with a `kAudioDevicePropertyStreamFormat` /
//! `kAudioDevicePropertyNominalSampleRate` notification.
//!
//! Before this module nothing listened for it. The resample ratio was computed
//! once at stream open and never revisited; the device/config cache was keyed on
//! the device *name*, so the same-named device at a new rate re-used the stale
//! config; and the stream-error flag was read only at Arm and Disarm. A
//! five-second dictation always has an Arm coming, so the damage was invisible.
//! A three-hour meeting has no Arm at all: the remainder of the session is
//! either silence or — worse, because it sounds fine — resampled at the wrong
//! ratio, a clean, half-speed, wrongly-timestamped track discovered only at
//! stop.
//!
//! ## The shape of the fix
//!
//! Two halves, deliberately separated so the interesting one needs no hardware:
//!
//! * [`InputFormatWatch`] — a **pure** state machine, in the same spirit as
//!   `sysaudio::restore_plan`. Feed it what the OS says the current input device
//!   and format are; it answers with the exact action the capture path must
//!   take: close the current spill segment, write a `device_change` marker
//!   carrying the host time, and reopen with a NEW ratio. A ratio must never
//!   survive a format change, so the ratio is part of the answer rather than
//!   something the caller is trusted to recompute.
//! * the macOS listener half — `AudioObjectAddPropertyListener` on
//!   `kAudioHardwarePropertyDefaultInputDevice` and on the live input device's
//!   `kAudioDevicePropertyStreamFormat` / `kAudioDevicePropertyNominalSampleRate`.
//!   It does one thing when it fires: record which selector changed in an atomic
//!   and return. It never allocates, never locks and never touches the audio —
//!   the capture worker's watchdog picks the edge up and drives the state
//!   machine above. That way promptness comes from the listener and *correctness*
//!   comes from code that is testable without an audio device.
//!
//! Polling alone would also work (the watchdog re-reads the format every 60 s
//! regardless, so a machine where the listener cannot be installed still
//! recovers), but a minute of half-speed audio is a minute too many.

use std::sync::atomic::{AtomicU32, Ordering};

/// Everything downstream of the mic runs at 16 kHz mono; the watch is
/// constructed with the target rate rather than assuming it, so the ratio it
/// hands back is always derived, never hard-coded.
pub const DEFAULT_TARGET_RATE: u32 = 16_000;

/// The nominal input format a capture stream is running at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputFormat {
    pub sample_rate_hz: u32,
    pub channels: u16,
}

impl InputFormat {
    pub fn new(sample_rate_hz: u32, channels: u16) -> Self {
        Self {
            sample_rate_hz,
            channels,
        }
    }

    /// A format we refuse to act on: a zero rate or zero channels is CoreAudio
    /// (or cpal) telling us it does not know, and installing a ratio from it
    /// would be worse than keeping the old one.
    pub fn is_usable(&self) -> bool {
        self.sample_rate_hz > 0 && self.channels > 0
    }
}

/// Which OS notification (or the watchdog's own re-read) produced an
/// observation. Carried into the marker so a recovered journal says *why* the
/// segment ended, not merely that it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatEventSource {
    /// `kAudioHardwarePropertyDefaultInputDevice` — the user picked another mic.
    DefaultInputDevice,
    /// `kAudioDevicePropertyStreamFormat` — the live device renegotiated.
    StreamFormat,
    /// `kAudioDevicePropertyNominalSampleRate` — ditto, rate only.
    NominalSampleRate,
    /// No notification: the capture watchdog re-read the format on its own tick.
    /// This is the belt-and-braces path for a machine where the listener could
    /// not be installed.
    Watchdog,
}

impl FormatEventSource {
    /// Stable string for the journal marker and the logs.
    pub fn as_str(self) -> &'static str {
        match self {
            FormatEventSource::DefaultInputDevice => "default_input_device",
            FormatEventSource::StreamFormat => "stream_format",
            FormatEventSource::NominalSampleRate => "nominal_sample_rate",
            FormatEventSource::Watchdog => "watchdog",
        }
    }

    /// Map a CoreAudio selector fourcc back to the source. Unknown selectors
    /// read as a watchdog-grade event: still worth acting on, just unlabelled.
    pub fn from_selector(selector: u32) -> Self {
        match selector {
            s if s == selectors::DEFAULT_INPUT_DEVICE => FormatEventSource::DefaultInputDevice,
            s if s == selectors::STREAM_FORMAT => FormatEventSource::StreamFormat,
            s if s == selectors::NOMINAL_SAMPLE_RATE => FormatEventSource::NominalSampleRate,
            _ => FormatEventSource::Watchdog,
        }
    }
}

/// CoreAudio property selectors, as fourcc constants. Public because the
/// format-change test asserts against the real selector values rather than
/// against a test-only enum — the state machine has to be driven by the same
/// numbers the HAL sends.
pub mod selectors {
    const fn fourcc(code: &[u8; 4]) -> u32 {
        ((code[0] as u32) << 24)
            | ((code[1] as u32) << 16)
            | ((code[2] as u32) << 8)
            | (code[3] as u32)
    }

    /// `kAudioHardwarePropertyDefaultInputDevice`
    pub const DEFAULT_INPUT_DEVICE: u32 = fourcc(b"dIn ");
    /// `kAudioDevicePropertyStreamFormat`
    pub const STREAM_FORMAT: u32 = fourcc(b"sfmt");
    /// `kAudioDevicePropertyNominalSampleRate`
    pub const NOMINAL_SAMPLE_RATE: u32 = fourcc(b"nsrt");
    /// `kAudioObjectPropertyScopeInput`
    pub const SCOPE_INPUT: u32 = fourcc(b"inpt");
    /// `kAudioObjectPropertyScopeGlobal`
    pub const SCOPE_GLOBAL: u32 = fourcc(b"glob");
}

/// One reading of "what is the input device and what format is it in", plus the
/// two anchors that make a marker useful: the mach host time of the event and
/// how many 16 kHz output samples had already been written when it happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputObservation {
    pub device_name: String,
    pub format: InputFormat,
    /// `mach_absolute_time` ticks — YV91's capture anchor. Zero when the caller
    /// has none (the marker then carries only the sample index).
    pub host_time: u64,
    /// Cumulative 16 kHz output samples written before this event.
    pub output_sample_index: u64,
    pub source: FormatEventSource,
}

/// The resample ratio a conversion runs at. Handed back by the state machine so
/// "the ratio after the event matches the NEW nominal rate" is something a test
/// can assert directly instead of inferring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResampleRatio {
    pub from_hz: u32,
    pub to_hz: u32,
}

impl ResampleRatio {
    /// Input samples per output sample.
    pub fn value(&self) -> f64 {
        if self.from_hz == 0 || self.to_hz == 0 {
            return 1.0;
        }
        self.from_hz as f64 / self.to_hz as f64
    }
}

/// The record written into the capture journal at a format change. This is the
/// 22-A shape of "close the current spill segment and open a new one": the
/// spill itself stays one continuous 16 kHz stream (that is what makes a
/// truncated file still a valid prefix), and the segment boundary is this
/// marker — the output sample index it carries is exactly where segment
/// `segment_index - 1` ends and segment `segment_index` begins. When YV91's
/// multi-file meeting journal lands, the same plan drives a real file rollover
/// with no change to the state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceChangeMarker {
    /// The segment this marker OPENS. The first segment of a take is 0, so a
    /// marker always carries 1 or more, and the *n*-th change of one take
    /// carries *n* — which is what makes the sidecar's lines orderable, and why
    /// the watch that produces them lives on the stream rather than on the poll.
    pub segment_index: u32,
    pub host_time: u64,
    pub output_sample_index: u64,
    pub from_device: String,
    pub to_device: String,
    pub from: InputFormat,
    pub to: InputFormat,
    pub source: FormatEventSource,
}

impl DeviceChangeMarker {
    /// The JSON line appended to the journal's marker sidecar. Written by the
    /// capture worker (never the audio callback) — see `CaptureJournal`.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": "device_change",
            "version": 1,
            "segment_index": self.segment_index,
            "host_time": self.host_time,
            "output_sample_index": self.output_sample_index,
            "from_device": self.from_device,
            "to_device": self.to_device,
            "from_sample_rate": self.from.sample_rate_hz,
            "from_channels": self.from.channels,
            "to_sample_rate": self.to.sample_rate_hz,
            "to_channels": self.to.channels,
            "source": self.source.as_str(),
        })
    }
}

/// What the capture path must do about an observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatChangeAction {
    /// Same device, same format. This is the common case *and* the debounce:
    /// one renegotiation makes the HAL fire several selectors, and every one
    /// after the first sees a state that already matches.
    Unchanged,
    /// The observation was not actionable (a zero rate or zero channels — the
    /// OS saying "I don't know"). The live ratio is kept; the next tick asks
    /// again.
    Ignored(&'static str),
    /// The format changed: close the current spill segment, write `marker`, and
    /// reopen the conversion at `ratio`.
    Reconfigure {
        marker: DeviceChangeMarker,
        ratio: ResampleRatio,
    },
}

impl FormatChangeAction {
    pub fn marker(&self) -> Option<&DeviceChangeMarker> {
        match self {
            FormatChangeAction::Reconfigure { marker, .. } => Some(marker),
            _ => None,
        }
    }

    pub fn is_reconfigure(&self) -> bool {
        matches!(self, FormatChangeAction::Reconfigure { .. })
    }
}

/// The pure state machine. Holds what the live stream is actually running (not
/// what a cache thinks it is) and turns observations into actions.
///
/// **It has to be owned by the live stream, not by the poll.** The segment
/// counter is the only thing that orders the markers of one take, so a watch
/// rebuilt on every watchdog tick would hand every marker the same
/// `segment_index: 1` and a downstream rollover would collide on every file
/// after the first. `record::LiveStream` therefore keeps one of these for the
/// life of the stream and carries it across a reopen with
/// [`InputFormatWatch::resync`]; [`InputFormatWatch::restart_segments`] is what
/// starts a new take's numbering at zero.
#[derive(Debug, Clone)]
pub struct InputFormatWatch {
    device_name: String,
    format: InputFormat,
    target_rate: u32,
    segment_index: u32,
}

impl InputFormatWatch {
    /// Start watching a stream that was just opened on `device_name` at
    /// `format`, feeding a `target_rate` conversion.
    pub fn new(device_name: impl Into<String>, format: InputFormat, target_rate: u32) -> Self {
        Self {
            device_name: device_name.into(),
            format,
            target_rate,
            segment_index: 0,
        }
    }

    /// The device the live stream is on.
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// The format the live stream is running at.
    pub fn format(&self) -> InputFormat {
        self.format
    }

    /// The conversion currently in force — the thing that must never survive a
    /// format change.
    pub fn ratio(&self) -> ResampleRatio {
        ResampleRatio {
            from_hz: self.format.sample_rate_hz,
            to_hz: self.target_rate,
        }
    }

    /// Which spill segment the capture is writing (0 until the first change).
    pub fn segment_index(&self) -> u32 {
        self.segment_index
    }

    /// The stream was reopened and the HAL handed back a device/format that may
    /// not be exactly what was observed a moment ago (a Bluetooth link settles,
    /// cpal picks a different default config). Track what actually opened —
    /// **without** touching the segment counter, because a reopen driven by a
    /// change already counted is the same segment, and a reopen after a plain
    /// stream error is not a segment boundary at all.
    pub fn resync(&mut self, device_name: impl Into<String>, format: InputFormat) {
        self.device_name = device_name.into();
        if format.is_usable() {
            self.format = format;
        }
    }

    /// A new take began on the same live stream. Segment numbering is per-take
    /// (the marker sidecar is retired with the take), so the counter restarts
    /// here rather than carrying yesterday's swaps into today's journal.
    pub fn restart_segments(&mut self) {
        self.segment_index = 0;
    }

    /// Fold one observation in. The ONLY mutation point: after a
    /// `Reconfigure` the watch already describes the new world, so a caller
    /// that fails to reopen must not pretend it succeeded — it re-observes on
    /// the next tick and gets `Unchanged`, with the stream's own error flag
    /// (polled by the same watchdog) as the backstop.
    pub fn observe(&mut self, obs: InputObservation) -> FormatChangeAction {
        if !obs.format.is_usable() {
            return FormatChangeAction::Ignored("the OS reported a zero rate or channel count");
        }
        if obs.device_name == self.device_name && obs.format == self.format {
            return FormatChangeAction::Unchanged;
        }
        self.segment_index += 1;
        let from_device = std::mem::replace(&mut self.device_name, obs.device_name);
        let from = std::mem::replace(&mut self.format, obs.format);
        let marker = DeviceChangeMarker {
            segment_index: self.segment_index,
            host_time: obs.host_time,
            output_sample_index: obs.output_sample_index,
            from_device,
            to_device: self.device_name.clone(),
            from,
            to: self.format,
            source: obs.source,
        };
        FormatChangeAction::Reconfigure {
            marker,
            ratio: self.ratio(),
        }
    }
}

// ── The macOS listener half ─────────────────────────────────────────────────
// One job: turn a HAL notification into an edge the capture watchdog can see.
// It records the selector that fired and returns; every decision is made by the
// state machine above, on the worker thread, where it is safe to allocate, log
// and touch the disk.

/// The selector of the last property change the HAL announced, or 0 for none.
/// A plain atomic (not a channel, not a lock) because the listener callback
/// runs on a HAL thread with the same rules as an audio callback.
static LAST_EVENT: AtomicU32 = AtomicU32::new(0);

/// Take the pending format-change edge, if any. Edge-triggered: the flag is
/// cleared by the read, so a burst of selectors during one renegotiation is
/// collapsed to a single wake-up (and the state machine collapses whatever the
/// burst reports to a single `Reconfigure`).
pub fn take_event() -> Option<FormatEventSource> {
    match LAST_EVENT.swap(0, Ordering::AcqRel) {
        0 => None,
        selector => Some(FormatEventSource::from_selector(selector)),
    }
}

/// Record an event exactly as the HAL callback does. Public for the tests that
/// drive the edge without an audio device.
pub fn record_event(selector: u32) {
    LAST_EVENT.store(selector, Ordering::Release);
}

#[cfg(target_os = "macos")]
mod imp {
    use super::selectors;
    use std::os::raw::c_void;
    use std::sync::Mutex;

    type OSStatus = i32;
    type AudioObjectID = u32;

    #[repr(C)]
    struct AudioObjectPropertyAddress {
        selector: u32,
        scope: u32,
        element: u32,
    }

    const K_AUDIO_OBJECT_SYSTEM_OBJECT: AudioObjectID = 1;
    const K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN: u32 = 0;

    type ListenerProc = extern "C" fn(
        AudioObjectID,
        u32,
        *const AudioObjectPropertyAddress,
        *mut c_void,
    ) -> OSStatus;

    // CoreAudio is already linked transitively via cpal; declare the framework
    // link explicitly so this module never depends on that being the case.
    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyData(
            in_object_id: AudioObjectID,
            in_address: *const AudioObjectPropertyAddress,
            in_qualifier_data_size: u32,
            in_qualifier_data: *const c_void,
            io_data_size: *mut u32,
            out_data: *mut c_void,
        ) -> OSStatus;

        fn AudioObjectAddPropertyListener(
            in_object_id: AudioObjectID,
            in_address: *const AudioObjectPropertyAddress,
            in_listener: ListenerProc,
            in_client_data: *mut c_void,
        ) -> OSStatus;

        fn AudioObjectRemovePropertyListener(
            in_object_id: AudioObjectID,
            in_address: *const AudioObjectPropertyAddress,
            in_listener: ListenerProc,
            in_client_data: *mut c_void,
        ) -> OSStatus;
    }

    /// The listener body. RT-grade on purpose: read the selector, store it in a
    /// static atomic, return. No allocation, no lock, no logging — the client
    /// data pointer is deliberately null so there is no lifetime to get wrong.
    extern "C" fn on_property_changed(
        _object: AudioObjectID,
        count: u32,
        addresses: *const AudioObjectPropertyAddress,
        _client: *mut c_void,
    ) -> OSStatus {
        if count > 0 && !addresses.is_null() {
            // SAFETY: CoreAudio guarantees `addresses` points at `count`
            // initialised structs for the duration of the call.
            let selector = unsafe { (*addresses).selector };
            super::record_event(selector);
        }
        0
    }

    fn addr(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            selector,
            scope,
            element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        }
    }

    /// The addresses a live capture listens on: the system's default-input
    /// selection, and the device's own format/rate.
    fn watched(device: AudioObjectID) -> Vec<(AudioObjectID, AudioObjectPropertyAddress)> {
        vec![
            (
                K_AUDIO_OBJECT_SYSTEM_OBJECT,
                addr(selectors::DEFAULT_INPUT_DEVICE, selectors::SCOPE_GLOBAL),
            ),
            (
                device,
                addr(selectors::STREAM_FORMAT, selectors::SCOPE_INPUT),
            ),
            (
                device,
                addr(selectors::NOMINAL_SAMPLE_RATE, selectors::SCOPE_GLOBAL),
            ),
        ]
    }

    /// The device whose listeners are installed, so arming is idempotent and
    /// disarming removes exactly what was added.
    static ARMED: Mutex<Option<AudioObjectID>> = Mutex::new(None);

    fn default_input_device() -> Option<AudioObjectID> {
        let address = addr(selectors::DEFAULT_INPUT_DEVICE, selectors::SCOPE_GLOBAL);
        let mut device: AudioObjectID = 0;
        let mut size = std::mem::size_of::<AudioObjectID>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                K_AUDIO_OBJECT_SYSTEM_OBJECT,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                &mut device as *mut _ as *mut c_void,
            )
        };
        (status == 0 && device != 0).then_some(device)
    }

    pub fn arm() -> bool {
        let Some(device) = default_input_device() else {
            return false;
        };
        let Ok(mut armed) = ARMED.lock() else {
            return false;
        };
        if *armed == Some(device) {
            return true;
        }
        if let Some(previous) = armed.take() {
            remove_for(previous);
        }
        let mut any = false;
        for (object, address) in watched(device) {
            let status = unsafe {
                AudioObjectAddPropertyListener(
                    object,
                    &address,
                    on_property_changed,
                    std::ptr::null_mut(),
                )
            };
            any |= status == 0;
        }
        if any {
            *armed = Some(device);
        }
        any
    }

    fn remove_for(device: AudioObjectID) {
        for (object, address) in watched(device) {
            unsafe {
                AudioObjectRemovePropertyListener(
                    object,
                    &address,
                    on_property_changed,
                    std::ptr::null_mut(),
                );
            }
        }
    }

    pub fn disarm() {
        let Ok(mut armed) = ARMED.lock() else {
            return;
        };
        if let Some(device) = armed.take() {
            remove_for(device);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn arm() -> bool {
        false
    }
    pub fn disarm() {}
}

/// Install the CoreAudio property listeners for the current default input
/// device. Idempotent, cheap, and never fatal: `false` means the app falls back
/// to the watchdog's own 60 s re-read, which is slower but not wrong.
pub fn arm_listeners() -> bool {
    imp::arm()
}

/// Remove the listeners (the capture stream closed).
pub fn disarm_listeners() {
    imp::disarm();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(name: &str, rate: u32, at: u64) -> InputObservation {
        InputObservation {
            device_name: name.to_string(),
            format: InputFormat::new(rate, 1),
            host_time: at,
            output_sample_index: at / 2,
            source: FormatEventSource::StreamFormat,
        }
    }

    #[test]
    fn an_unchanged_format_is_not_a_segment_boundary() {
        let mut watch = InputFormatWatch::new(
            "MacBook Pro Microphone",
            InputFormat::new(48_000, 1),
            DEFAULT_TARGET_RATE,
        );
        for _ in 0..5 {
            assert_eq!(
                watch.observe(obs("MacBook Pro Microphone", 48_000, 10)),
                FormatChangeAction::Unchanged
            );
        }
        assert_eq!(watch.segment_index(), 0);
        assert_eq!(watch.ratio().from_hz, 48_000);
    }

    #[test]
    fn a_zero_rate_never_installs_a_ratio() {
        let mut watch = InputFormatWatch::new("Mic", InputFormat::new(48_000, 1), 16_000);
        let action = watch.observe(InputObservation {
            device_name: "Mic".into(),
            format: InputFormat::new(0, 1),
            host_time: 1,
            output_sample_index: 1,
            source: FormatEventSource::NominalSampleRate,
        });
        assert!(matches!(action, FormatChangeAction::Ignored(_)));
        assert_eq!(watch.ratio().from_hz, 48_000);
        assert_eq!(watch.segment_index(), 0);
    }

    #[test]
    fn the_marker_names_both_sides_of_the_change() {
        let mut watch = InputFormatWatch::new("Built-in", InputFormat::new(48_000, 2), 16_000);
        let action = watch.observe(obs("AirPods Pro", 24_000, 900));
        let marker = action.marker().expect("a format change is a reconfigure");
        assert_eq!(marker.from_device, "Built-in");
        assert_eq!(marker.to_device, "AirPods Pro");
        assert_eq!(marker.from, InputFormat::new(48_000, 2));
        assert_eq!(marker.to, InputFormat::new(24_000, 1));
        assert_eq!(marker.host_time, 900);
        let json = marker.to_json();
        assert_eq!(json["kind"], "device_change");
        assert_eq!(json["to_sample_rate"], 24_000);
    }

    #[test]
    fn a_reopen_resync_is_not_a_new_segment() {
        // The reopen half of the swap: the watch already counted the change, so
        // being told what actually opened must not count it a second time.
        let mut watch = InputFormatWatch::new("Built-in", InputFormat::new(48_000, 1), 16_000);
        assert!(watch
            .observe(obs("AirPods Pro", 24_000, 10))
            .is_reconfigure());
        watch.resync("AirPods Pro", InputFormat::new(24_000, 1));
        assert_eq!(watch.segment_index(), 1, "a resync never bumps the counter");
        assert_eq!(
            watch.observe(obs("AirPods Pro", 24_000, 11)),
            FormatChangeAction::Unchanged,
            "and the next tick agrees with what actually opened"
        );
        // An unreadable format from the reopen leaves the tracked format alone.
        watch.resync("AirPods Pro", InputFormat::new(0, 0));
        assert_eq!(watch.format(), InputFormat::new(24_000, 1));
    }

    #[test]
    fn each_change_of_one_take_gets_the_next_segment_and_a_new_take_restarts() {
        // The whole AirPods sequence inside ONE take: 48k → 24k → 16k (HFP) →
        // 48k. Three markers, three DIFFERENT indices — a constant index cannot
        // order a sidecar.
        let mut watch = InputFormatWatch::new("Built-in", InputFormat::new(48_000, 1), 16_000);
        let mut indices = Vec::new();
        for (device, rate) in [
            ("AirPods Pro", 24_000),
            ("AirPods Pro", 16_000),
            ("Built-in", 48_000),
        ] {
            let action = watch.observe(obs(device, rate, 100));
            indices.push(
                action
                    .marker()
                    .expect("a change reconfigures")
                    .segment_index,
            );
            watch.resync(device, InputFormat::new(rate, 1));
        }
        assert_eq!(indices, vec![1, 2, 3]);

        watch.restart_segments();
        assert_eq!(watch.segment_index(), 0, "a new take starts at segment 0");
        let action = watch.observe(obs("AirPods Pro", 24_000, 200));
        assert_eq!(action.marker().unwrap().segment_index, 1);
    }

    #[test]
    fn selectors_map_back_to_their_sources() {
        assert_eq!(
            FormatEventSource::from_selector(selectors::STREAM_FORMAT),
            FormatEventSource::StreamFormat
        );
        assert_eq!(
            FormatEventSource::from_selector(selectors::DEFAULT_INPUT_DEVICE),
            FormatEventSource::DefaultInputDevice
        );
        assert_eq!(
            FormatEventSource::from_selector(selectors::NOMINAL_SAMPLE_RATE),
            FormatEventSource::NominalSampleRate
        );
        assert_eq!(
            FormatEventSource::from_selector(0),
            FormatEventSource::Watchdog
        );
    }
}
