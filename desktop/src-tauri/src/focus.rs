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
    use core_foundation::base::{CFGetTypeID, CFRange, CFRelease, CFTypeRef, TCFType};
    use core_foundation::string::{CFString, CFStringRef};
    use std::ffi::c_void;

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
        fn AXUIElementSetAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: CFTypeRef,
        ) -> i32;
        fn AXUIElementIsAttributeSettable(
            element: AXUIElementRef,
            attribute: CFStringRef,
            settable: *mut u8,
        ) -> i32;
        fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout: f32) -> i32;
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
            let err =
                AXUIElementCopyAttributeValue(system, key.as_concrete_TypeRef(), &mut focused);
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
            let err =
                AXUIElementCopyAttributeValue(system, key.as_concrete_TypeRef(), &mut focused);
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

    /// yap-p0-insertion-bridge-fallback — insert `text` by setting
    /// `kAXSelectedText` on the focused element (replaces the selection, or
    /// inserts at the caret). `Err` = the element refused BEFORE anything was
    /// written (a pre-set check failed, or the set returned a code that proves
    /// refusal — see `super::set_error_outcome`), and only then may the caller
    /// fall back to the paste bridge.
    /// Once the set returned success the answer is never `Err`: a value that
    /// reads back changed is `Verified`, anything else (async accessibility
    /// trees in Chromium/Electron, a timed-out re-read, identical replacement
    /// text) is `Unverified` — the text may be there, so no ⌘V may follow.
    pub fn insert_selected_text(text: &str) -> Result<super::AxSet, String> {
        unsafe {
            let system = AXUIElementCreateSystemWide();
            if system.is_null() {
                return Err("AXUIElementCreateSystemWide failed".into());
            }
            let key = CFString::new("AXFocusedUIElement");
            let mut focused: CFTypeRef = std::ptr::null_mut();
            let err =
                AXUIElementCopyAttributeValue(system, key.as_concrete_TypeRef(), &mut focused);
            CFRelease(system as CFTypeRef);
            if err != AX_OK || focused.is_null() {
                return Err(format!("no focused element (AX error {err})"));
            }
            let el = focused as AXUIElementRef;
            // Per-element bound: a hung target must not hold the take for the
            // system default of 6 s. Never set on the system-wide element (that
            // would change every AX call Yap makes).
            let _ = AXUIElementSetMessagingTimeout(el, 1.0);
            let sel = CFString::new("AXSelectedText");
            let mut settable: u8 = 0;
            let e = AXUIElementIsAttributeSettable(el, sel.as_concrete_TypeRef(), &mut settable);
            if e != AX_OK || settable == 0 {
                CFRelease(focused);
                return Err(format!("AXSelectedText is not settable (AX error {e})"));
            }
            let Some(before) = attr_cfstring(el, "AXValue") else {
                CFRelease(focused);
                return Err("AXValue is unreadable, so an AX insert could not be verified".into());
            };
            let value = CFString::new(text);
            let e = AXUIElementSetAttributeValue(
                el,
                sel.as_concrete_TypeRef(),
                value.as_concrete_TypeRef() as CFTypeRef,
            );
            if e != AX_OK {
                CFRelease(focused);
                // Fix round 2 (independent BLOCKING 1): only a code that PROVES
                // the set was refused may become `Err` (→ paste bridge). Any
                // other code — kAXErrorCannotComplete from the 1 s messaging
                // timeout above first of all — may still be applied by a busy
                // target, so it is `Unverified` (copy + card, zero ⌘V).
                return super::set_error_outcome(e);
            }
            // Give an async accessibility tree a moment to catch up before
            // calling the set unverified (5 reads over ~120 ms at most).
            let mut changed = false;
            for i in 0..5 {
                if i > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(30));
                }
                if matches!(attr_cfstring(el, "AXValue"), Some(a) if a != before) {
                    changed = true;
                    break;
                }
            }
            CFRelease(focused);
            Ok(if changed {
                super::AxSet::Verified
            } else {
                super::AxSet::Unverified(
                    "the element accepted AXSelectedText but its value never read back changed"
                        .into(),
                )
            })
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
            let err =
                AXUIElementCopyAttributeValue(system, key.as_concrete_TypeRef(), &mut focused);
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
                    (Some(value), Some(caret)) => {
                        Some(super::prefix_before_caret(&value, caret, limit))
                    }
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

/// Should we try to insert? With Accessibility, only when a text control is
/// focused. WITHOUT it the focused element is invisible to Yap, so this says
/// yes and `paste::choose_insert_path` decides: a receipt-checked paste bridge
/// when PostEvent is granted, the Copy card otherwise (yap-p0-insertion-bridge-
/// fallback — answering "no" here was the silent clipboard-only take).
pub fn should_auto_paste() -> bool {
    if !crate::permissions::is_accessibility_trusted() {
        return true;
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

/// yap-format-structure — the frontmost app's bundle id (`notion.id`,
/// `com.apple.mail`, …): the key `structure::structure_format_for` and the
/// per-app Structure overrides are written against. Read from NSWorkspace, so
/// it needs no Accessibility grant. `None` off macOS or with no frontmost app.
pub fn frontmost_bundle_id() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
        let app = workspace.frontmostApplication()?;
        let id = app.bundleIdentifier()?;
        Some(id.to_string()).filter(|s| !s.is_empty())
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// What a `kAXSelectedText` set that macOS ACCEPTED observed afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AxSet {
    /// The focused element's value read back changed.
    Verified,
    /// Accepted, but no change was observed — the text may still have landed.
    Unverified(String),
}

/// AXError codes (HIServices `AXError.h`) that, returned by
/// `AXUIElementSetAttributeValue(kAXSelectedText)`, PROVE nothing was written:
/// the argument, the element, the attribute or the API itself was rejected
/// before the target applied anything. This is an ALLOWLIST on purpose.
const AX_SET_REFUSED: [(i32, &str); 6] = [
    (-25201, "kAXErrorIllegalArgument"),
    (-25202, "kAXErrorInvalidUIElement"),
    (-25205, "kAXErrorAttributeUnsupported"),
    (-25206, "kAXErrorActionUnsupported"),
    (-25208, "kAXErrorNotImplemented"),
    (-25211, "kAXErrorAPIDisabled"),
];

/// kAXErrorCannotComplete — what a messaging timeout returns. AXUIElement.h:
/// it "does not necessarily mean that the function has failed".
pub(crate) const AX_ERROR_CANNOT_COMPLETE: i32 = -25204;

/// Fix round 2 (independent BLOCKING 1) — the outcome of a NON-OK return from
/// the `kAXSelectedText` set. `Err` (refused, the caller may fall back to the
/// ⌘V bridge) ONLY for a code in [`AX_SET_REFUSED`]; every other code
/// (kAXErrorCannotComplete, kAXErrorFailure, kAXErrorNoValue, anything
/// unknown) fails CLOSED to `Unverified`: the text may still land, so it is
/// copied and carded and no ⌘V may follow it. `code` must not be 0.
pub(crate) fn set_error_outcome(code: i32) -> Result<AxSet, String> {
    match AX_SET_REFUSED.iter().find(|(c, _)| *c == code) {
        Some((_, name)) => Err(format!(
            "setting AXSelectedText was refused ({name}, AX error {code})"
        )),
        None if code == AX_ERROR_CANNOT_COMPLETE => Ok(AxSet::Unverified(format!(
            "setting AXSelectedText timed out (kAXErrorCannotComplete, AX error {code}); a busy \
             target may still apply it"
        ))),
        None => Ok(AxSet::Unverified(format!(
            "setting AXSelectedText returned AX error {code}, which does not prove nothing was \
             written"
        ))),
    }
}

/// yap-p0-insertion-bridge-fallback — AX insertion (see [`ax::insert_selected_text`]).
/// `Err` only when nothing was written (see [`set_error_outcome`]).
pub fn insert_selected_text(text: &str) -> Result<AxSet, String> {
    #[cfg(target_os = "macos")]
    {
        ax::insert_selected_text(text)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = text;
        Err("AX insertion is macOS-only".into())
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
    use super::{
        prefix_before_caret, set_error_outcome, AxSet, AX_ERROR_CANNOT_COMPLETE, AX_SET_REFUSED,
        CONTEXT_CHAR_LIMIT,
    };

    // Fix round 2 (independent BLOCKING 1): a set that timed out may still be
    // applied, so it must never read as "refused" (which would earn a ⌘V).
    #[test]
    fn timed_out_ax_set_is_unverified_never_refused() {
        assert_eq!(AX_ERROR_CANNOT_COMPLETE, -25204);
        assert!(matches!(
            set_error_outcome(AX_ERROR_CANNOT_COMPLETE),
            Ok(AxSet::Unverified(_))
        ));
        // kAXErrorFailure, kAXErrorNoValue and codes nobody listed fail closed too.
        for code in [-25200, -25212, -25299, -1, 1] {
            assert!(
                matches!(set_error_outcome(code), Ok(AxSet::Unverified(_))),
                "{code} must not count as a proven refusal"
            );
        }
        // Only the allowlisted codes are proven refusals.
        for (code, name) in AX_SET_REFUSED {
            match set_error_outcome(code) {
                Err(why) => assert!(why.contains(name), "{why}"),
                other => panic!("{code} ({name}) should be refused, got {other:?}"),
            }
        }
    }

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
