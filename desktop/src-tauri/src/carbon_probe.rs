//! yap-p0-hotkeys-no-permission, fix round 2: an EXCLUSIVE Carbon probe.
//!
//! global-hotkey 0.8.0 (the global-shortcut plugin's backend) calls
//! `RegisterEventHotKey(.., inOptions = 0, ..)` — a NON-exclusive register. On
//! macOS that returns `noErr` for a chord another app or the system already
//! holds (⌘Space for Spotlight, a Raycast / Alfred / BTT chord), so its `Ok`
//! proves nothing about whether Yap will actually receive the chord. This
//! module asks the question that does: register the chord with
//! `kEventHotKeyExclusive`, which fails with `eventHotKeyExistsErr` (-9878)
//! when anyone else holds it, then unregister at once. The dictate-toggle
//! registrar runs it BEFORE the plugin register, so a held chord comes back as
//! an `Err` (status `Failed`, a rebind refused, the old chord kept) instead
//! of a false `Registered`.

use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

/// `kEventHotKeyExclusive` (CarbonEvents.h).
pub const K_EVENT_HOT_KEY_EXCLUSIVE: u32 = 1 << 0;
/// `eventHotKeyExistsErr` (CarbonEvents.h): the chord is registered elsewhere.
pub const EVENT_HOT_KEY_EXISTS_ERR: i32 = -9878;

/// Carbon modifier mask, built exactly like global-hotkey 0.8.0 builds it.
pub fn carbon_mods(m: Modifiers) -> u32 {
    let mut mods: u32 = 0;
    if m.contains(Modifiers::SHIFT) {
        mods |= 512; // shiftKey
    }
    if m.intersects(Modifiers::SUPER | Modifiers::META) {
        mods |= 256; // cmdKey
    }
    if m.contains(Modifiers::ALT) {
        mods |= 2048; // optionKey
    }
    if m.contains(Modifiers::CONTROL) {
        mods |= 4096; // controlKey
    }
    mods
}

/// The Carbon virtual key code global-hotkey 0.8.0 registers for `code`
/// (copied verbatim from its `platform_impl::macos::key_to_scancode`, so the
/// probe checks the very key the plugin registers). `None` means the plugin
/// does not use Carbon for it (media keys go through a CGEventTap).
pub fn carbon_keycode(code: Code) -> Option<u32> {
    match code {
        Code::KeyA => Some(0x00),
        Code::KeyS => Some(0x01),
        Code::KeyD => Some(0x02),
        Code::KeyF => Some(0x03),
        Code::KeyH => Some(0x04),
        Code::KeyG => Some(0x05),
        Code::KeyZ => Some(0x06),
        Code::KeyX => Some(0x07),
        Code::KeyC => Some(0x08),
        Code::KeyV => Some(0x09),
        Code::KeyB => Some(0x0b),
        Code::KeyQ => Some(0x0c),
        Code::KeyW => Some(0x0d),
        Code::KeyE => Some(0x0e),
        Code::KeyR => Some(0x0f),
        Code::KeyY => Some(0x10),
        Code::KeyT => Some(0x11),
        Code::Digit1 => Some(0x12),
        Code::Digit2 => Some(0x13),
        Code::Digit3 => Some(0x14),
        Code::Digit4 => Some(0x15),
        Code::Digit6 => Some(0x16),
        Code::Digit5 => Some(0x17),
        Code::Equal => Some(0x18),
        Code::Digit9 => Some(0x19),
        Code::Digit7 => Some(0x1a),
        Code::Minus => Some(0x1b),
        Code::Digit8 => Some(0x1c),
        Code::Digit0 => Some(0x1d),
        Code::BracketRight => Some(0x1e),
        Code::KeyO => Some(0x1f),
        Code::KeyU => Some(0x20),
        Code::BracketLeft => Some(0x21),
        Code::KeyI => Some(0x22),
        Code::KeyP => Some(0x23),
        Code::Enter => Some(0x24),
        Code::KeyL => Some(0x25),
        Code::KeyJ => Some(0x26),
        Code::Quote => Some(0x27),
        Code::KeyK => Some(0x28),
        Code::Semicolon => Some(0x29),
        Code::Backslash => Some(0x2a),
        Code::Comma => Some(0x2b),
        Code::Slash => Some(0x2c),
        Code::KeyN => Some(0x2d),
        Code::KeyM => Some(0x2e),
        Code::Period => Some(0x2f),
        Code::Tab => Some(0x30),
        Code::Space => Some(0x31),
        Code::Backquote => Some(0x32),
        Code::Backspace => Some(0x33),
        Code::Escape => Some(0x35),
        Code::F17 => Some(0x40),
        Code::NumpadDecimal => Some(0x41),
        Code::NumpadMultiply => Some(0x43),
        Code::NumpadAdd => Some(0x45),
        Code::NumLock => Some(0x47),
        Code::AudioVolumeUp => Some(0x48),
        Code::AudioVolumeDown => Some(0x49),
        Code::AudioVolumeMute => Some(0x4a),
        Code::NumpadDivide => Some(0x4b),
        Code::NumpadEnter => Some(0x4c),
        Code::NumpadSubtract => Some(0x4e),
        Code::F18 => Some(0x4f),
        Code::F19 => Some(0x50),
        Code::NumpadEqual => Some(0x51),
        Code::Numpad0 => Some(0x52),
        Code::Numpad1 => Some(0x53),
        Code::Numpad2 => Some(0x54),
        Code::Numpad3 => Some(0x55),
        Code::Numpad4 => Some(0x56),
        Code::Numpad5 => Some(0x57),
        Code::Numpad6 => Some(0x58),
        Code::Numpad7 => Some(0x59),
        Code::F20 => Some(0x5a),
        Code::Numpad8 => Some(0x5b),
        Code::Numpad9 => Some(0x5c),
        Code::F5 => Some(0x60),
        Code::F6 => Some(0x61),
        Code::F7 => Some(0x62),
        Code::F3 => Some(0x63),
        Code::F8 => Some(0x64),
        Code::F9 => Some(0x65),
        Code::F11 => Some(0x67),
        Code::F13 => Some(0x69),
        Code::F16 => Some(0x6a),
        Code::F14 => Some(0x6b),
        Code::F10 => Some(0x6d),
        Code::F12 => Some(0x6f),
        Code::F15 => Some(0x71),
        Code::Insert => Some(0x72),
        Code::Home => Some(0x73),
        Code::PageUp => Some(0x74),
        Code::Delete => Some(0x75),
        Code::F4 => Some(0x76),
        Code::End => Some(0x77),
        Code::F2 => Some(0x78),
        Code::PageDown => Some(0x79),
        Code::F1 => Some(0x7a),
        Code::ArrowLeft => Some(0x7b),
        Code::ArrowRight => Some(0x7c),
        Code::ArrowDown => Some(0x7d),
        Code::ArrowUp => Some(0x7e),
        Code::CapsLock => Some(0x39),
        Code::PrintScreen => Some(0x46),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
mod ffi {
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct EventHotKeyID {
        pub signature: u32,
        pub id: u32,
    }
    pub type EventHotKeyRef = *mut std::ffi::c_void;
    pub type EventTargetRef = *mut std::ffi::c_void;

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        pub fn GetApplicationEventTarget() -> EventTargetRef;
        pub fn RegisterEventHotKey(
            in_hot_key_code: u32,
            in_hot_key_modifiers: u32,
            in_hot_key_id: EventHotKeyID,
            in_target: EventTargetRef,
            in_options: u32,
            out_ref: *mut EventHotKeyRef,
        ) -> i32;
        pub fn UnregisterEventHotKey(in_hot_key: EventHotKeyRef) -> i32;
    }
}

/// Raw `RegisterEventHotKey` with `options`; on success the hot key is
/// unregistered again before returning. Returns the OSStatus. Call it on the
/// main thread in the app (the registrar does); a test calls it directly.
#[cfg(target_os = "macos")]
pub fn register_then_release(keycode: u32, mods: u32, options: u32) -> i32 {
    // 'yprb' — distinct from global-hotkey's 'htrs' signature.
    let id = ffi::EventHotKeyID {
        signature: u32::from_be_bytes(*b"yprb"),
        id: 1,
    };
    // SAFETY: plain Carbon calls with a valid out-pointer; the ref is only
    // passed back to UnregisterEventHotKey when the register succeeded.
    unsafe {
        let mut r: ffi::EventHotKeyRef = std::ptr::null_mut();
        let st = ffi::RegisterEventHotKey(
            keycode,
            mods,
            id,
            ffi::GetApplicationEventTarget(),
            options,
            &mut r,
        );
        if st == 0 && !r.is_null() {
            ffi::UnregisterEventHotKey(r);
        }
        st
    }
}

/// Map an exclusive-probe OSStatus to the registrar's result. The text is
/// label-free: callers already say which chord ("⌃⌥⌘Space did not register (…)").
pub fn probe_result(status: i32) -> Result<(), String> {
    match status {
        0 => Ok(()),
        EVENT_HOT_KEY_EXISTS_ERR => Err(format!(
            "another app or macOS already uses it, eventHotKeyExistsErr {status}"
        )),
        s => Err(format!(
            "exclusive RegisterEventHotKey failed, OSStatus {s}"
        )),
    }
}

/// Is `chord` free for Yap? `Ok` when no other app or macOS holds it (or when
/// the plugin does not use Carbon for its key, so there is nothing to probe);
/// `Err` naming the conflict otherwise. Never leaves anything registered.
pub fn probe_chord(chord: &Shortcut) -> Result<(), String> {
    let Some(keycode) = carbon_keycode(chord.key) else {
        return Ok(());
    };
    #[cfg(target_os = "macos")]
    {
        probe_result(register_then_release(
            keycode,
            carbon_mods(chord.mods),
            K_EVENT_HOT_KEY_EXCLUSIVE,
        ))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = keycode;
        Ok(())
    }
}
