//! Detect whether the frontmost focused UI element accepts text (Wispr Flow behavior).
//!
//! Apple Accessibility:
//!   AXUIElementCreateSystemWide → kAXFocusedUIElementAttribute → kAXRoleAttribute
//! If role is a text control, we paste; otherwise clipboard-only.
//! Also resolves frontmost app name for transcript hygiene (source_app), the
//! current selection (YV49 command mode) and the text just before the caret
//! (YV50 context awareness).

/// Hard bound on the pre-caret context read by [`text_before_cursor`]: at most
/// this many characters immediately before the caret ever leave Accessibility.
///
/// PRIVACY (YV50): that string is a formatting signal only. It lives in the
/// dictation call stack and is dropped there — never logged, never persisted,
/// never emitted to the UI, never sent anywhere.
pub const CONTEXT_CHAR_LIMIT: usize = 500;

#[cfg(target_os = "macos")]
mod ax {
    use std::ffi::c_void;
    use core_foundation::base::{CFGetTypeID, CFRange, CFRelease, CFTypeRef, TCFType};
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
        /// Unwraps an AXValue (here: kAXValueTypeCFRange) into a plain C struct.
        fn AXValueGetValue(value: CFTypeRef, the_type: u32, out: *mut c_void) -> u8;
    }

    const AX_OK: i32 = 0;
    /// `kAXValueTypeCFRange` — the AXValue wrapper AXSelectedTextRange uses.
    const AX_VALUE_CF_RANGE: u32 = 4;

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

    /// Same as [`attr_string`] but TYPE-CHECKED: `AXValue` is a CFString on a
    /// text control and a CFNumber/CFBoolean elsewhere, so the value is only
    /// unwrapped once CoreFoundation confirms it really is a string.
    fn attr_cfstring(element: AXUIElementRef, name: &str) -> Option<String> {
        unsafe {
            let key = CFString::new(name);
            let mut val: CFTypeRef = std::ptr::null_mut();
            let err = AXUIElementCopyAttributeValue(element, key.as_concrete_TypeRef(), &mut val);
            if err != AX_OK || val.is_null() {
                return None;
            }
            if CFGetTypeID(val) != CFString::type_id() {
                CFRelease(val);
                return None;
            }
            let s = CFString::wrap_under_create_rule(val as CFStringRef);
            Some(s.to_string())
        }
    }

    /// Caret offset (UTF-16 units, the unit AX ranges are expressed in) from
    /// AXSelectedTextRange. `None` when the element exposes no selection.
    fn caret_offset(element: AXUIElementRef) -> Option<usize> {
        unsafe {
            let key = CFString::new("AXSelectedTextRange");
            let mut val: CFTypeRef = std::ptr::null_mut();
            let err = AXUIElementCopyAttributeValue(element, key.as_concrete_TypeRef(), &mut val);
            if err != AX_OK || val.is_null() {
                return None;
            }
            let mut range = CFRange {
                location: 0,
                length: 0,
            };
            let ok = AXValueGetValue(
                val,
                AX_VALUE_CF_RANGE,
                &mut range as *mut CFRange as *mut c_void,
            );
            CFRelease(val);
            if ok == 0 || range.location < 0 {
                return None;
            }
            Some(range.location as usize)
        }
    }

    /// The text immediately BEFORE the caret in the focused text control, capped
    /// at `limit` characters (YV50). `None` on a secure field, an element with no
    /// value/caret, or nothing focused.
    pub fn text_before_cursor(limit: usize) -> Option<String> {
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
            // Never read a password field, even though AX would hand us its value.
            let secure = attr_string(el, "AXRole").unwrap_or_default() == "AXSecureTextField"
                || attr_string(el, "AXSubrole")
                    .unwrap_or_default()
                    .contains("Secure");
            let context = if secure {
                None
            } else {
                match (attr_cfstring(el, "AXValue"), caret_offset(el)) {
                    (Some(value), Some(caret)) => Some(super::prefix_before_caret(&value, caret, limit)),
                    _ => None,
                }
            };
            CFRelease(focused);
            context
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

/// The last `limit` characters of `value` that sit before `caret` — the pure,
/// testable half of the YV50 context read.
///
/// `caret` is an AX offset, i.e. a count of UTF-16 units, so the prefix is cut
/// in UTF-16 space (and only that prefix is materialised, never the whole
/// document) before being bounded to `limit` CHARACTERS.
fn prefix_before_caret(value: &str, caret: usize, limit: usize) -> String {
    let units: Vec<u16> = value.encode_utf16().take(caret).collect();
    let prefix = String::from_utf16_lossy(&units);
    let chars = prefix.chars().count();
    if chars <= limit {
        return prefix;
    }
    prefix.chars().skip(chars - limit).collect()
}

/// Up to [`CONTEXT_CHAR_LIMIT`] characters of the text immediately before the
/// caret in the focused text control (YV50 context awareness).
///
/// `None` — treated by every caller as "no signal, format as-is" — whenever
/// Accessibility is denied, the focused element is a secure/password field, or
/// the element exposes no value or selection. `Some("")` is DIFFERENT and
/// meaningful: an empty field, i.e. the start of a fresh sentence.
///
/// PRIVACY: the returned string never leaves the dictation call stack. It is
/// never logged, never written to the history DB, never emitted to the UI —
/// only the formatting decisions derived from it are.
pub fn text_before_cursor() -> Option<String> {
    if !crate::permissions::is_accessibility_trusted() {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        ax::text_before_cursor(CONTEXT_CHAR_LIMIT)
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

#[cfg(test)]
mod tests {
    use super::{prefix_before_caret, CONTEXT_CHAR_LIMIT};

    // YV50: the context read is BOUNDED — a whole document behind the caret can
    // never be pulled into the pipeline, only the last CONTEXT_CHAR_LIMIT chars.
    #[test]
    fn context_read_is_bounded_to_500_chars_before_the_caret() {
        assert_eq!(CONTEXT_CHAR_LIMIT, 500);
        let doc = "x".repeat(4000) + "the wire went out on ";
        let caret = doc.encode_utf16().count();

        let ctx = prefix_before_caret(&doc, caret, CONTEXT_CHAR_LIMIT);
        assert_eq!(ctx.chars().count(), CONTEXT_CHAR_LIMIT);
        // It is the text ADJACENT to the caret that is kept, not the head.
        assert!(ctx.ends_with("the wire went out on "), "{ctx}");
    }

    #[test]
    fn prefix_stops_at_the_caret_and_counts_utf16_units() {
        // Caret mid-string: nothing after it is context.
        assert_eq!(prefix_before_caret("hello world", 6, 500), "hello ");
        // Caret at 0 (empty field / start of line) → empty context, not None.
        assert_eq!(prefix_before_caret("hello", 0, 500), "");
        // Emoji are 2 UTF-16 units — an AX offset past one lands correctly.
        assert_eq!(prefix_before_caret("ok 🚀 go", 5, 500), "ok 🚀");
        // A caret beyond the value (stale read) is clamped, never panics.
        assert_eq!(prefix_before_caret("hello", 99, 500), "hello");
    }
}
