//! Detect whether the frontmost focused UI element accepts text (Wispr Flow behavior).
//!
//! Apple Accessibility:
//!   AXUIElementCreateSystemWide → kAXFocusedUIElementAttribute → kAXRoleAttribute
//! If role is a text control, we paste; otherwise clipboard-only.
//! Also resolves frontmost app name for transcript hygiene (source_app).

#[cfg(target_os = "macos")]
mod ax {
    use std::ffi::c_void;
    use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
    use core_foundation::string::{CFString, CFStringRef};

    pub type AXUIElementRef = *mut c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> i32;
    }

    const AX_OK: i32 = 0;

    fn attr_string(element: AXUIElementRef, name: &str) -> Option<String> {
        unsafe {
            let key = CFString::new(name);
            let mut val: CFTypeRef = std::ptr::null_mut();
            let err = AXUIElementCopyAttributeValue(element, key.as_concrete_TypeRef(), &mut val);
            if err != AX_OK || val.is_null() {
                return None;
            }
            let s = CFString::wrap_under_create_rule(val as CFStringRef);
            Some(s.to_string())
        }
    }

    /// true if focused element looks like it can receive typed/pasted text
    pub fn focused_is_text_input() -> bool {
        unsafe {
            let system = AXUIElementCreateSystemWide();
            if system.is_null() {
                return false;
            }
            let key = CFString::new("AXFocusedUIElement");
            let mut focused: CFTypeRef = std::ptr::null_mut();
            let err = AXUIElementCopyAttributeValue(system, key.as_concrete_TypeRef(), &mut focused);
            CFRelease(system as CFTypeRef);
            if err != AX_OK || focused.is_null() {
                return false;
            }
            let el = focused as AXUIElementRef;
            let role = attr_string(el, "AXRole").unwrap_or_default();
            let sub = attr_string(el, "AXSubrole").unwrap_or_default();
            let editable = attr_string(el, "AXEditable")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            CFRelease(focused);

            let role_ok = matches!(
                role.as_str(),
                "AXTextField"
                    | "AXTextArea"
                    | "AXComboBox"
                    | "AXSearchField"
                    | "AXWebArea"
                    | "AXGroup" // some Electron apps
            ) || role.to_lowercase().contains("text")
                || role.to_lowercase().contains("field");

            // Contenteditable-ish
            let sub_ok = sub.contains("Text") || sub.contains("Search");

            role_ok || sub_ok || editable
        }
    }

    /// Text currently selected in the focused element, via AXSelectedText.
    ///
    /// YV49 (command mode) reads the thing the user is about to edit. `None`
    /// when nothing is focused, the element does not expose a selection, or the
    /// selection is empty/whitespace — the caller treats all three the same
    /// way: there is nothing to act on, so no take starts.
    pub fn selected_text() -> Option<String> {
        unsafe {
            let system = AXUIElementCreateSystemWide();
            if system.is_null() {
                return None;
            }
            let key = CFString::new("AXFocusedUIElement");
            let mut focused: CFTypeRef = std::ptr::null_mut();
            let err = AXUIElementCopyAttributeValue(system, key.as_concrete_TypeRef(), &mut focused);
            CFRelease(system as CFTypeRef);
            if err != AX_OK || focused.is_null() {
                return None;
            }
            let el = focused as AXUIElementRef;
            let selection = attr_string(el, "AXSelectedText").filter(|s| !s.trim().is_empty());
            CFRelease(focused);
            selection
        }
    }

    /// Frontmost application title via AX (no NSWorkspace dep).
    pub fn frontmost_app_name() -> Option<String> {
        unsafe {
            let system = AXUIElementCreateSystemWide();
            if system.is_null() {
                return None;
            }
            let key = CFString::new("AXFocusedApplication");
            let mut app: CFTypeRef = std::ptr::null_mut();
            let err = AXUIElementCopyAttributeValue(system, key.as_concrete_TypeRef(), &mut app);
            CFRelease(system as CFTypeRef);
            if err != AX_OK || app.is_null() {
                return None;
            }
            let el = app as AXUIElementRef;
            let title = attr_string(el, "AXTitle")
                .or_else(|| attr_string(el, "AXDescription"))
                .filter(|s| !s.is_empty());
            CFRelease(app);
            title
        }
    }
}

/// Should we auto-paste? Requires Accessibility trust + a focused text control.
/// Always return false if AX not trusted (caller still copies to clipboard).
pub fn should_auto_paste() -> bool {
    if !crate::permissions::is_accessibility_trusted() {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        ax::focused_is_text_input()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// The current selection in the focused text control (YV49 command mode).
/// `None` whenever Accessibility is denied or nothing is selected — command
/// mode then refuses to start rather than editing something it cannot see.
pub fn selected_text() -> Option<String> {
    if !crate::permissions::is_accessibility_trusted() {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        ax::selected_text()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Best-effort focused app name for transcript hygiene. None if AX denied.
pub fn frontmost_app_name() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        ax::frontmost_app_name()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
