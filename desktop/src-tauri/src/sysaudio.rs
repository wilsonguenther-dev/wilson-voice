//! System-output auto-mute while dictating (YV28).
//!
//! When dictation is RECORDING we mute the Mac's default output device so
//! nothing (music, a video, a notification) plays over the user while they
//! talk, then restore the EXACT prior mute + volume the instant recording
//! stops. Implemented against CoreAudio's `AudioObject` property API on the
//! default output device (`kAudioHardwarePropertyDefaultOutputDevice`) reading
//! and writing `kAudioDevicePropertyMute` / `kAudioDevicePropertyVolumeScalar`.
//!
//! SAFETY-CRITICAL: the app must NEVER leave the Mac muted. Callers restore on
//! stop, on cancel, on every error path, and on app exit. `restore` is a
//! best-effort no-panic operation, and the pure `restore_plan` helper (tested)
//! captures exactly which values are written back.

/// Captured output-device state to restore after a dictation take. `volume` is
/// `None` when the device exposes no scalar-volume property (e.g. some digital
/// / aggregate outputs) — in that case only the mute flag is restored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputAudioState {
    pub muted: bool,
    pub volume: Option<f32>,
}

/// The concrete CoreAudio values a restore writes back — pure, no FFI. Kept
/// separate from `OutputAudioState` so the save→restore mapping is unit-testable
/// without touching audio hardware.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RestorePlan {
    /// `kAudioDevicePropertyMute` value: 1 = muted, 0 = unmuted.
    pub mute: u32,
    /// `kAudioDevicePropertyVolumeScalar` to re-apply, or `None` to leave it.
    pub volume: Option<f32>,
}

impl OutputAudioState {
    /// Build the restore plan from a saved state. Pure — this is the exact,
    /// tested contract for what gets written back to the device on stop.
    pub fn restore_plan(&self) -> RestorePlan {
        RestorePlan {
            mute: if self.muted { 1 } else { 0 },
            volume: self.volume,
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{OutputAudioState, RestorePlan};
    use std::os::raw::c_void;

    type OSStatus = i32;
    type AudioObjectID = u32;

    #[repr(C)]
    struct AudioObjectPropertyAddress {
        selector: u32,
        scope: u32,
        element: u32,
    }

    // FourCC helper for CoreAudio property selectors ('dOut', 'mute', …).
    const fn fourcc(code: &[u8; 4]) -> u32 {
        ((code[0] as u32) << 24)
            | ((code[1] as u32) << 16)
            | ((code[2] as u32) << 8)
            | (code[3] as u32)
    }

    const K_AUDIO_OBJECT_SYSTEM_OBJECT: AudioObjectID = 1;
    const K_AUDIO_HARDWARE_PROPERTY_DEFAULT_OUTPUT_DEVICE: u32 = fourcc(b"dOut");
    const K_AUDIO_DEVICE_PROPERTY_MUTE: u32 = fourcc(b"mute");
    const K_AUDIO_DEVICE_PROPERTY_VOLUME_SCALAR: u32 = fourcc(b"volm");
    const K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT: u32 = fourcc(b"outp");
    const K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL: u32 = fourcc(b"glob");
    // Deprecated alias `kAudioObjectPropertyElementMaster` is also 0.
    const K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN: u32 = 0;

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

        fn AudioObjectSetPropertyData(
            in_object_id: AudioObjectID,
            in_address: *const AudioObjectPropertyAddress,
            in_qualifier_data_size: u32,
            in_qualifier_data: *const c_void,
            in_data_size: u32,
            in_data: *const c_void,
        ) -> OSStatus;

        fn AudioObjectHasProperty(
            in_object_id: AudioObjectID,
            in_address: *const AudioObjectPropertyAddress,
        ) -> u8;
    }

    fn addr(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            selector,
            scope,
            element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        }
    }

    /// The system default output device, or `None` if CoreAudio can't resolve it.
    fn default_output_device() -> Option<AudioObjectID> {
        let address = addr(
            K_AUDIO_HARDWARE_PROPERTY_DEFAULT_OUTPUT_DEVICE,
            K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
        );
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
        if status == 0 && device != 0 {
            Some(device)
        } else {
            None
        }
    }

    fn has_property(device: AudioObjectID, selector: u32) -> bool {
        let address = addr(selector, K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT);
        unsafe { AudioObjectHasProperty(device, &address) != 0 }
    }

    fn read_u32(device: AudioObjectID, selector: u32) -> Option<u32> {
        if !has_property(device, selector) {
            return None;
        }
        let address = addr(selector, K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT);
        let mut value: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                device,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                &mut value as *mut _ as *mut c_void,
            )
        };
        (status == 0).then_some(value)
    }

    fn read_f32(device: AudioObjectID, selector: u32) -> Option<f32> {
        if !has_property(device, selector) {
            return None;
        }
        let address = addr(selector, K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT);
        let mut value: f32 = 0.0;
        let mut size = std::mem::size_of::<f32>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                device,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                &mut value as *mut _ as *mut c_void,
            )
        };
        (status == 0).then_some(value)
    }

    fn write_u32(device: AudioObjectID, selector: u32, value: u32) -> bool {
        if !has_property(device, selector) {
            return false;
        }
        let address = addr(selector, K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT);
        let status = unsafe {
            AudioObjectSetPropertyData(
                device,
                &address,
                0,
                std::ptr::null(),
                std::mem::size_of::<u32>() as u32,
                &value as *const _ as *const c_void,
            )
        };
        status == 0
    }

    fn write_f32(device: AudioObjectID, selector: u32, value: f32) -> bool {
        if !has_property(device, selector) {
            return false;
        }
        let address = addr(selector, K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT);
        let status = unsafe {
            AudioObjectSetPropertyData(
                device,
                &address,
                0,
                std::ptr::null(),
                std::mem::size_of::<f32>() as u32,
                &value as *const _ as *const c_void,
            )
        };
        status == 0
    }

    /// Snapshot the default output device's mute + volume, then mute it.
    /// Returns the saved state so the caller can restore it verbatim on stop.
    /// Returns `None` when there is no usable output device or the mute property
    /// is unavailable — in which case nothing was changed and there is nothing
    /// to restore.
    pub fn mute_and_save() -> Option<OutputAudioState> {
        let device = default_output_device()?;
        // Require a readable mute flag — without it we can neither mute nor
        // guarantee a correct restore, so we leave the system untouched.
        let muted = read_u32(device, K_AUDIO_DEVICE_PROPERTY_MUTE)? != 0;
        let volume = read_f32(device, K_AUDIO_DEVICE_PROPERTY_VOLUME_SCALAR);
        let saved = OutputAudioState { muted, volume };
        // Only touch the device if it isn't already muted.
        if !muted {
            write_u32(device, K_AUDIO_DEVICE_PROPERTY_MUTE, 1);
        }
        Some(saved)
    }

    /// Restore the device to a previously-saved state. Best-effort and
    /// panic-free: re-resolves the current default output device (the user may
    /// have swapped outputs mid-take) and writes back the exact prior mute +
    /// volume. Safe to call even if `mute_and_save` failed (caller passes the
    /// saved state only when it succeeded).
    pub fn restore(saved: OutputAudioState) {
        let plan: RestorePlan = saved.restore_plan();
        let Some(device) = default_output_device() else {
            return;
        };
        write_u32(device, K_AUDIO_DEVICE_PROPERTY_MUTE, plan.mute);
        if let Some(v) = plan.volume {
            write_f32(device, K_AUDIO_DEVICE_PROPERTY_VOLUME_SCALAR, v);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::OutputAudioState;

    pub fn mute_and_save() -> Option<OutputAudioState> {
        None
    }

    pub fn restore(_saved: OutputAudioState) {}
}

pub use imp::{mute_and_save, restore};

#[cfg(test)]
mod tests {
    use super::OutputAudioState;

    // YV28: the save→restore mapping is the safety-critical contract — a stop
    // must write back the EXACT prior mute + volume so we never strand the Mac
    // muted or at the wrong level. Proven purely, no audio hardware involved.
    #[test]
    fn restore_plan_round_trips_saved_state() {
        // Was unmuted at 70% — restore must un-mute (0) and re-apply the volume.
        let unmuted = OutputAudioState {
            muted: false,
            volume: Some(0.7),
        };
        let plan = unmuted.restore_plan();
        assert_eq!(plan.mute, 0);
        assert_eq!(plan.volume, Some(0.7));

        // Was ALREADY muted — restore must leave it muted (1), never un-mute
        // something the user had intentionally silenced.
        let muted = OutputAudioState {
            muted: true,
            volume: Some(0.25),
        };
        let plan = muted.restore_plan();
        assert_eq!(plan.mute, 1);
        assert_eq!(plan.volume, Some(0.25));

        // No scalar-volume property on the device — mute is still restored, and
        // no volume write is attempted.
        let no_volume = OutputAudioState {
            muted: false,
            volume: None,
        };
        let plan = no_volume.restore_plan();
        assert_eq!(plan.mute, 0);
        assert_eq!(plan.volume, None);
    }
}
