//! Microphone AUTHORIZATION for Wilson Voice — the question macOS actually answers.
//!
//! PERM-A. The module this replaces asked cpal whether the default input device
//! would hand over a config and called that answer "permission". It cannot see a
//! denial: `default_input_config()` describes HARDWARE. On a Mac with a built-in
//! mic and a revoked Microphone grant it returns `Ok`, so the app reported "mic
//! ready", opened a stream, and recorded silence.
//!
//! The real question is
//! `+[AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio]`, and
//! from here on it is the ONLY source of truth for authorization in this app.
//! The device probe survives as [`input_device_present`], which answers a
//! genuinely different question — *is a microphone attached* — and is never
//! allowed to stand in for authorization. There is deliberately NO fallback to
//! the probe when the Objective-C call fails: "couldn't ask macOS, so assume
//! yes" is the exact bug this item deletes.
//!
//! # Nothing here waits
//!
//! [`request_access`] is reachable from `start_recording`, which runs on the
//! AppKit main thread (the PTT tap hops to main before it touches app state).
//! Pausing that thread for the length of a human decision on the TCC dialog
//! stops the run loop: the pill cannot repaint, webview IPC cannot deliver an
//! event, and macOS marks the process unresponsive. AVCaptureDevice's completion
//! handler is documented as arriving on an arbitrary dispatch queue, so a
//! main-thread wait can also deadlock outright. Therefore
//! [`authorization_status`] is a pure, prompt-free read (the UI calls it
//! constantly) and [`request_access`] fires the request and returns immediately,
//! delivering the outcome through a callback.
//!
//! # A bundle is required
//!
//! `requestAccessForMediaType:` reads `NSMicrophoneUsageDescription` out of the
//! MAIN BUNDLE's Info.plist, and TCC kills the process outright when the key is
//! absent. Tauri merges `Info.plist` into the `.app` at bundle time only, so
//! `npm run tauri dev` and `cargo test` run a bare Mach-O with no bundle at all.
//! [`request_access`] checks for the key first and, when it is missing, logs once
//! and reports `NotDetermined` rather than calling into TCC.

/// `AVAuthorizationStatus`, verbatim.
///
/// `NS_ENUM(NSInteger)` — so `isize`, not `i32`. The 32-bit form happens to
/// survive on arm64 because 0..3 fits the low word of the returned register,
/// which makes it a latent bug rather than one that shows up in a test, and
/// `objc2`'s `msg_send!` does not verify return encodings.
#[repr(isize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicAuth {
    NotDetermined = 0,
    Restricted = 1,
    Denied = 2,
    Authorized = 3,
}

impl MicAuth {
    /// The wire form the UI branches on.
    pub fn as_str(self) -> &'static str {
        match self {
            MicAuth::NotDetermined => "not_determined",
            MicAuth::Restricted => "restricted",
            MicAuth::Denied => "denied",
            MicAuth::Authorized => "authorized",
        }
    }

    /// Transmute-free decode of the `NSInteger` AVFoundation returned.
    ///
    /// An unrecognised value is mapped to `Restricted`, never to `Authorized`:
    /// a status we cannot read is a status we must not treat as a grant, and
    /// `Restricted` is the one variant whose prose ("Yap cannot request this")
    /// stays true when the cause is our own ignorance.
    pub fn from_raw(raw: isize) -> MicAuth {
        match raw {
            0 => MicAuth::NotDetermined,
            1 => MicAuth::Restricted,
            2 => MicAuth::Denied,
            3 => MicAuth::Authorized,
            other => {
                log::error!(
                    "mic auth: AVAuthorizationStatus returned unknown value {other}; \
                     treating as Restricted (never as authorized)"
                );
                MicAuth::Restricted
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod sys {
    use super::MicAuth;
    use block2::RcBlock;
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2_foundation::NSString;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {
        /// `AVMediaTypeAudio` — an `NSString * const` exported by AVFoundation.
        /// Reading the real symbol instead of hard-coding `@"soun"` means a
        /// future SDK cannot drift out from under us silently.
        static AVMediaTypeAudio: *const NSString;
    }

    fn media_type_audio() -> &'static NSString {
        // SAFETY: AVFoundation is linked above, so the constant is non-null and
        // lives for the lifetime of the process.
        unsafe { &*AVMediaTypeAudio }
    }

    fn capture_device_class() -> Option<&'static AnyClass> {
        AnyClass::get(c"AVCaptureDevice")
    }

    pub fn authorization_status() -> MicAuth {
        let Some(cls) = capture_device_class() else {
            log::error!(
                "mic auth: +[AVCaptureDevice class] not found — AVFoundation did not load. \
                 Reporting Restricted; the device probe is NOT consulted."
            );
            return MicAuth::Restricted;
        };
        // SAFETY: class method on AVCaptureDevice, one NSString argument,
        // NSInteger return.
        let raw: isize =
            unsafe { msg_send![cls, authorizationStatusForMediaType: media_type_audio()] };
        MicAuth::from_raw(raw)
    }

    /// Is `NSMicrophoneUsageDescription` readable from the MAIN bundle?
    ///
    /// TCC terminates the process when `requestAccessForMediaType:` is called
    /// without it, and an un-bundled binary (`cargo test`, `tauri dev`) has no
    /// Info.plist at all.
    pub fn usage_description_present() -> bool {
        let Some(cls) = AnyClass::get(c"NSBundle") else {
            return false;
        };
        // SAFETY: +[NSBundle mainBundle] -> NSBundle* (may be nil).
        let bundle: *mut AnyObject = unsafe { msg_send![cls, mainBundle] };
        if bundle.is_null() {
            return false;
        }
        let key = NSString::from_str("NSMicrophoneUsageDescription");
        // SAFETY: -[NSBundle objectForInfoDictionaryKey:] -> id (may be nil).
        let value: *mut AnyObject = unsafe { msg_send![bundle, objectForInfoDictionaryKey: &*key] };
        !value.is_null()
    }

    static WARNED_NO_USAGE_STRING: AtomicBool = AtomicBool::new(false);

    pub fn request_access<F>(on_result: F)
    where
        F: Fn(MicAuth) + Send + 'static,
    {
        let status = authorization_status();
        if status != MicAuth::NotDetermined {
            // macOS shows the dialog ONLY from NotDetermined. From Denied the
            // call returns false immediately and the real next step is the
            // Settings deep link, so do not pretend to have asked.
            on_result(status);
            return;
        }
        if !usage_description_present() {
            if !WARNED_NO_USAGE_STRING.swap(true, Ordering::Relaxed) {
                log::warn!(
                    "mic auth: NSMicrophoneUsageDescription is not in the main bundle's \
                     Info.plist (running un-bundled, e.g. `tauri dev` or `cargo test`). \
                     Skipping requestAccessForMediaType: — calling it without the key makes \
                     TCC kill the process. Reporting NotDetermined."
                );
            }
            on_result(MicAuth::NotDetermined);
            return;
        }
        let Some(cls) = capture_device_class() else {
            on_result(MicAuth::Restricted);
            return;
        };
        let handler = RcBlock::new(move |_granted: Bool| {
            // Re-read rather than trusting the BOOL: the status string is what
            // the rest of the app branches on, and the two can only ever agree
            // if there is one reader.
            on_result(authorization_status());
        });
        // SAFETY: class method, (NSString*, void(^)(BOOL)) -> void. The block is
        // copied by the callee, so dropping our reference here is correct.
        unsafe {
            let _: () = msg_send![
                cls,
                requestAccessForMediaType: media_type_audio(),
                completionHandler: &*handler,
            ];
        }
    }
}

/// Off-Mac stub, mirroring what `permissions.rs` already does for Accessibility:
/// there is no TCC, so there is nothing to deny.
#[cfg(not(target_os = "macos"))]
mod sys {
    use super::MicAuth;

    pub fn authorization_status() -> MicAuth {
        MicAuth::Authorized
    }

    pub fn usage_description_present() -> bool {
        true
    }

    pub fn request_access<F>(on_result: F)
    where
        F: Fn(MicAuth) + Send + 'static,
    {
        on_result(MicAuth::Authorized)
    }
}

/// The authoritative TCC read. Pure: it never prompts, so the UI may call it as
/// often as it likes.
pub fn authorization_status() -> MicAuth {
    sys::authorization_status()
}

/// True when the main bundle carries `NSMicrophoneUsageDescription`.
pub fn usage_description_present() -> bool {
    sys::usage_description_present()
}

/// Fire `+[AVCaptureDevice requestAccessForMediaType:completionHandler:]` and
/// RETURN. `on_result` runs on whatever queue AVFoundation chose — treat it as
/// an arbitrary background thread.
pub fn request_access<F>(on_result: F)
where
    F: Fn(MicAuth) + Send + 'static,
{
    sys::request_access(on_result)
}

/// HARDWARE, not permission: is a default input device attached at all?
///
/// Kept separate on purpose. Conflating this with authorization is the defect
/// PERM-A exists to remove, so it must never appear in a function that answers
/// a permission question.
pub fn input_device_present() -> bool {
    use cpal::traits::HostTrait;
    cpal::default_host().default_input_device().is_some()
}

/// Can Yap actually record right now? Authorized by TCC **and** a device exists.
pub fn microphone_ready() -> bool {
    authorization_status() == MicAuth::Authorized && input_device_present()
}
