//! Microphone TCC for Wilson Voice only.
//!
//! Call `request_microphone_access` **once** from Permissions (not on every Dictate).
//! Dictate uses cpal record; after the user clicks Allow once, macOS should not
//! re-prompt unless the app is re-signed (ad-hoc rebuild) or revoked in Settings.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// True if default input device + config are available (mic present / often authorized).
pub fn microphone_ready() -> bool {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    match host.default_input_device() {
        None => false,
        Some(d) => d.default_input_config().is_ok(),
    }
}

/// One-shot TCC registration: open a short input stream so macOS attributes
/// Microphone permission to com.wilsonguenther.wilson-voice.
///
/// Do **not** call this on every Dictate — only from Permissions → Request Microphone.
pub fn request_microphone_access() -> bool {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    // If we can already open config and a brief stream without user action, skip long hold.
    if microphone_ready() {
        // Still open stream once so TCC row exists after fresh install / re-sign.
        log::info!("mic request: probing stream (may show Allow once after reinstall)");
    }

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
        log::warn!("mic request: build_input_stream failed — click Allow in the system dialog");
        return false;
    };
    if stream.play().is_err() {
        return false;
    }
    // Hold open long enough for TCC dialog + first buffers (first install only)
    thread::sleep(Duration::from_millis(600));
    drop(stream);
    got.lock().map(|g| *g).unwrap_or(false)
}

pub fn microphone_authorized() -> Option<bool> {
    Some(microphone_ready())
}
