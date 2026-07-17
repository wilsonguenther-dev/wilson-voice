//! Force Microphone TCC registration for this process.
//!
//! Apple requires the *app that uses the mic* to request access.
//! A short cpal open of the default input device triggers the system dialog
//! and makes **Wilson Voice** appear under System Settings → Microphone.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Returns true if we can open the default input stream (authorized / working).
pub fn request_microphone_access() -> bool {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let Some(device) = host.default_input_device() else {
        log::warn!("mic request: no default input device");
        return false;
    };
    let Ok(supported) = device.default_input_config() else {
        log::warn!("mic request: default_input_config failed (denied or busy)");
        return false;
    };
    let conf: cpal::StreamConfig = supported.clone().into();
    let got = Arc::new(Mutex::new(false));
    let got2 = got.clone();

    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            conf,
            move |data: &[f32], _| {
                if !data.is_empty() {
                    if let Ok(mut g) = got2.lock() {
                        *g = true;
                    }
                }
            },
            |e| log::error!("mic probe stream: {e}"),
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            conf,
            move |data: &[i16], _| {
                if !data.is_empty() {
                    if let Ok(mut g) = got2.lock() {
                        *g = true;
                    }
                }
            },
            |e| log::error!("mic probe stream: {e}"),
            None,
        ),
        other => {
            log::warn!("mic request: unsupported format {other:?}");
            return false;
        }
    };

    let Ok(stream) = stream else {
        log::warn!("mic request: build_input_stream failed — user may need to Allow in dialog");
        return false;
    };
    if stream.play().is_err() {
        return false;
    }
    // Hold open long enough for TCC dialog + first buffers
    thread::sleep(Duration::from_millis(800));
    drop(stream);
    got.lock().map(|g| *g).unwrap_or(false)
}

pub fn microphone_authorized() -> Option<bool> {
    // Best-effort: try default config; PermissionDenied-ish surfaces as Err
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    match host.default_input_device() {
        None => Some(false),
        Some(d) => match d.default_input_config() {
            Ok(_) => Some(true),
            Err(_) => Some(false),
        },
    }
}
