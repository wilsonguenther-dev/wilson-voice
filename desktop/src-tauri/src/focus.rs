//! Detect whether the frontmost focused UI element accepts text (Wispr Flow behavior).
//!
//! Apple Accessibility:
//!   AXUIElementCreateSystemWide → kAXFocusedUIElementAttribute → kAXRoleAttribute
//! If role is a text control, we paste; otherwise clipboard-only.
//! Also resolves frontmost app name for transcript hygiene (source_app), the
//! current selection (YV49 command mode) and the text just before the caret
//! (YV50 context awareness).
//!
//! PERM-F widens the context read: `AXSelectedText`, the text after the caret
//! and — for Chrome, Safari and Firefox — the focused page's URL, all read
//! IN-PROCESS in one pass ([`read_ax_context`]). Every text field is cut to a
//! named budget before it leaves this module, and the URL never leaves it at
//! all: it is turned into a dictation mode inside [`context_from_reads`] and
//! dropped. No page HTML, no screenshot, no subprocess.

/// Hard bound on the pre-caret context read by [`read_ax_context`]: at most
/// this many characters immediately before the caret ever leave Accessibility.
///
/// PRIVACY (YV50): that string is a formatting signal only. It lives in the
/// dictation call stack and is dropped there — never logged, never persisted,
/// never emitted to the UI, never sent anywhere.
pub const CONTEXT_CHAR_LIMIT: usize = 500;

/// PERM-F budget for the text AFTER the caret: a whole document below the
/// caret is cut to this many characters (the ones adjacent to the caret)
/// before it reaches the pipeline. Same privacy rule as [`CONTEXT_CHAR_LIMIT`].
pub const AFTER_CARET_CHAR_LIMIT: usize = 200;

/// PERM-F budget for `AXSelectedText` in the dictation context: a select-all
/// over a long document is cut to its first this-many characters.
pub const SELECTION_CHAR_LIMIT: usize = 500;

/// How far up the `AXParent` chain the URL read looks for the `AXWebArea`.
const MAX_AX_PARENT_HOPS: usize = 32;

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
            // yap-insert-bridge-2026-10-26: the caret state BEFORE the set, so an
            // unverified set can be settled by whether anything moved.
            let caret_before = caret_snapshot(el);
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
            if changed {
                CFRelease(focused);
                return Ok(super::AxSet::Verified);
            }
            // yap-insert-bridge-2026-10-26 DO 3: AXValue never read back changed
            // (Chromium/Electron). Re-read the caret for ~400 ms: a real change
            // moves AXNumberOfCharacters or AXSelectedTextRange (Inserted, zero
            // ⌘V); neither moving means nothing landed — a refusal, so the paste
            // bridge runs exactly once instead of stranding the take on the card.
            let step = 50u64;
            let mut rereads = Vec::new();
            for _ in 0..(crate::paste::AX_REREAD_WINDOW_MS / step) {
                std::thread::sleep(std::time::Duration::from_millis(step));
                let now = caret_snapshot(el);
                let moved = now.moved_from(&caret_before);
                rereads.push(now);
                if moved {
                    break;
                }
            }
            CFRelease(focused);
            match crate::paste::settle_unverified(
                "the element accepted AXSelectedText but its value never read back changed",
                &caret_before,
                &rereads,
            ) {
                crate::paste::AxInsert::Inserted => Ok(super::AxSet::Verified),
                crate::paste::AxInsert::Refused(why) => Err(why),
                crate::paste::AxInsert::Unverified(why) => Ok(super::AxSet::Unverified(why)),
            }
        }
    }

    /// `AXNumberOfCharacters` as an integer (type-checked CFNumber).
    fn attr_i64(element: AXUIElementRef, name: &str) -> Option<i64> {
        use core_foundation::number::{CFNumber, CFNumberRef};
        unsafe {
            let key = CFString::new(name);
            let mut val: CFTypeRef = std::ptr::null_mut();
            let err = AXUIElementCopyAttributeValue(element, key.as_concrete_TypeRef(), &mut val);
            if err != AX_OK || val.is_null() {
                return None;
            }
            if CFGetTypeID(val) != CFNumber::type_id() {
                CFRelease(val);
                return None;
            }
            CFNumber::wrap_under_create_rule(val as CFNumberRef).to_i64()
        }
    }

    /// `AXSelectedTextRange` as (location, length).
    fn selected_range(element: AXUIElementRef) -> Option<(i64, i64)> {
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
            if ok == 0 {
                return None;
            }
            Some((range.location as i64, range.length as i64))
        }
    }

    fn caret_snapshot(element: AXUIElementRef) -> crate::paste::AxCaretSnapshot {
        crate::paste::AxCaretSnapshot {
            chars: attr_i64(element, "AXNumberOfCharacters"),
            range: selected_range(element),
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

    /// `name` as a URL string: a CFURL (`AXURL` on a web area) or a CFString
    /// (`AXDocument` on a browser window). Type-checked like [`attr_cfstring`].
    fn attr_url_string(element: AXUIElementRef, name: &str) -> Option<String> {
        use core_foundation::url::{CFURLRef, CFURL};
        unsafe {
            let key = CFString::new(name);
            let mut val: CFTypeRef = std::ptr::null_mut();
            let err = AXUIElementCopyAttributeValue(element, key.as_concrete_TypeRef(), &mut val);
            if err != AX_OK || val.is_null() {
                return None;
            }
            let ty = CFGetTypeID(val);
            if ty == CFURL::type_id() {
                return Some(
                    CFURL::wrap_under_create_rule(val as CFURLRef)
                        .get_string()
                        .to_string(),
                );
            }
            if ty == CFString::type_id() {
                return Some(CFString::wrap_under_create_rule(val as CFStringRef).to_string());
            }
            CFRelease(val);
            None
        }
    }

    /// PERM-F — the URL of the web page that holds the focused element: walk
    /// `AXParent` up to the enclosing `AXWebArea` (bounded by
    /// [`super::MAX_AX_PARENT_HOPS`]) and read its `AXURL`; failing that, the
    /// focused window's `AXDocument`. In-process, no subprocess, no AppleScript
    /// — the bundle stays the only TCC row. The string is handed straight to
    /// [`super::context_from_reads`], which classifies and drops it.
    fn focused_page_url(focused: AXUIElementRef) -> Option<String> {
        unsafe {
            let mut owned: Option<CFTypeRef> = None;
            let mut cur = focused;
            let mut url = None;
            for _ in 0..super::MAX_AX_PARENT_HOPS {
                if attr_string(cur, "AXRole").as_deref() == Some("AXWebArea") {
                    url = attr_url_string(cur, "AXURL");
                    break;
                }
                let key = CFString::new("AXParent");
                let mut parent: CFTypeRef = std::ptr::null_mut();
                let err =
                    AXUIElementCopyAttributeValue(cur, key.as_concrete_TypeRef(), &mut parent);
                if let Some(prev) = owned.take() {
                    CFRelease(prev);
                }
                if err != AX_OK || parent.is_null() {
                    break;
                }
                owned = Some(parent);
                cur = parent as AXUIElementRef;
            }
            if let Some(prev) = owned {
                CFRelease(prev);
            }
            if url.is_some() {
                return url;
            }
            let key = CFString::new("AXWindow");
            let mut window: CFTypeRef = std::ptr::null_mut();
            let err =
                AXUIElementCopyAttributeValue(focused, key.as_concrete_TypeRef(), &mut window);
            if err != AX_OK || window.is_null() {
                return None;
            }
            let doc = attr_url_string(window as AXUIElementRef, "AXDocument");
            CFRelease(window);
            doc
        }
    }

    /// PERM-F — every raw Accessibility field the dictation pipeline reads, in
    /// ONE pass over the focused element: the value and the selection range
    /// (for the text before AND after the caret), `AXSelectedText`, and — only
    /// when `want_url` (one of [`super::URL_BROWSERS`] is frontmost) — the page
    /// URL. A secure/password field yields nothing but the URL. Every failure
    /// is a `None` field, never an error: the take never depends on this read.
    pub fn read_raw(want_url: bool) -> super::AxRaw {
        let mut raw = super::AxRaw::default();
        unsafe {
            let system = AXUIElementCreateSystemWide();
            if system.is_null() {
                return raw;
            }
            let key = CFString::new("AXFocusedUIElement");
            let mut focused: CFTypeRef = std::ptr::null_mut();
            let err =
                AXUIElementCopyAttributeValue(system, key.as_concrete_TypeRef(), &mut focused);
            CFRelease(system as CFTypeRef);
            if err != AX_OK || focused.is_null() {
                return raw;
            }
            let el = focused as AXUIElementRef;
            // Never read a password field, even though AX would hand us its value.
            raw.secure = attr_string(el, "AXRole").unwrap_or_default() == "AXSecureTextField"
                || attr_string(el, "AXSubrole")
                    .unwrap_or_default()
                    .contains("Secure");
            if !raw.secure {
                raw.value = attr_cfstring(el, "AXValue");
                raw.range = selected_range(el)
                    .filter(|(location, _)| *location >= 0)
                    .map(|(location, length)| (location as usize, length.max(0) as usize));
                raw.selected = attr_cfstring(el, "AXSelectedText");
            }
            if want_url {
                raw.url = focused_page_url(el);
            }
            CFRelease(focused);
        }
        raw
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

/// PERM-F — the first `limit` characters of `value` from the AX offset `start`
/// (UTF-16 units) on: the text after the caret, bounded. Only `2 * limit` units
/// are ever materialised, so a whole document below the caret costs nothing.
fn suffix_after_caret(value: &str, start: usize, limit: usize) -> String {
    let units: Vec<u16> = value
        .encode_utf16()
        .skip(start)
        .take(limit.saturating_mul(2))
        .collect();
    String::from_utf16_lossy(&units)
        .chars()
        .take(limit)
        .collect()
}

/// PERM-F — the raw Accessibility reads, exactly as macOS returned them. Lives
/// for one call: [`context_from_reads`] bounds the text fields, turns the URL
/// into a mode and drops it. Deliberately NOT `Debug`, so not even a stray
/// `{:?}` can print a raw field (the URL above all).
#[derive(Default)]
pub struct AxRaw {
    /// AXSecureTextField / a Secure subrole: no text field is read at all.
    pub secure: bool,
    /// `AXValue` of the focused element.
    pub value: Option<String>,
    /// `AXSelectedTextRange` as (location, length), in UTF-16 units.
    pub range: Option<(usize, usize)>,
    /// `AXSelectedText` of the focused element.
    pub selected: Option<String>,
    /// The focused page's URL (browsers in [`URL_BROWSERS`] only).
    pub url: Option<String>,
}

/// PERM-F — what the dictation pipeline is allowed to see of the focused
/// field. Three bounded text fields and a MODE; the browser URL itself has no
/// field here on purpose, so it cannot reach the DB, a log, an event or a
/// support bundle — it died inside [`context_from_reads`].
///
/// `Default` (all `None`) is exactly today's no-Accessibility path.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct AxContext {
    /// Up to [`CONTEXT_CHAR_LIMIT`] chars immediately before the caret (YV50).
    /// `Some("")` = an empty field / start of the text.
    pub before_caret: Option<String>,
    /// Up to [`SELECTION_CHAR_LIMIT`] chars of `AXSelectedText`; `None` when
    /// nothing is selected.
    pub selection: Option<String>,
    /// Up to [`AFTER_CARET_CHAR_LIMIT`] chars immediately after the caret (or
    /// after the selection the take will replace).
    pub after_caret: Option<String>,
    /// The dictation mode the focused browser page implies (Gmail → Email,
    /// a Google Doc → Document), derived from the URL and nothing else.
    pub browser_mode: Option<crate::dictation::DictationMode>,
}

/// Lengths only — the content of the context is never formatted anywhere.
impl std::fmt::Debug for AxContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = |s: &Option<String>| s.as_ref().map(|s| s.chars().count());
        f.debug_struct("AxContext")
            .field("before_caret_chars", &len(&self.before_caret))
            .field("selection_chars", &len(&self.selection))
            .field("after_caret_chars", &len(&self.after_caret))
            .field("browser_mode", &self.browser_mode)
            .finish()
    }
}

/// The three browsers whose page URL Yap resolves (Chrome, Safari, Firefox),
/// by bundle id. Anything else never has its URL read.
pub const URL_BROWSERS: &[&str] = &[
    "com.google.Chrome",
    "com.apple.Safari",
    "org.mozilla.firefox",
];

/// True when `bundle_id` is one of [`URL_BROWSERS`].
pub fn is_url_browser(bundle_id: &str) -> bool {
    URL_BROWSERS
        .iter()
        .any(|b| b.eq_ignore_ascii_case(bundle_id.trim()))
}

/// The pure half of the PERM-F read: the grant check, the budgets and the URL
/// classification, with the AX calls injected as `read` so a test can drive it.
///
/// WITHOUT the Accessibility grant `read` is never called and the result is
/// [`AxContext::default`] — byte for byte what the pipeline saw before PERM-F,
/// when `text_before_cursor` returned `None` on the same check.
pub fn context_from_reads(trusted: bool, read: impl FnOnce() -> AxRaw) -> AxContext {
    if !trusted {
        return AxContext::default();
    }
    let raw = read();
    // The URL is consumed HERE and goes no further: only the mode it implies
    // leaves this function.
    let browser_mode = raw
        .url
        .as_deref()
        .and_then(crate::dictation::mode_for_browser_url);
    if raw.secure {
        return AxContext {
            browser_mode,
            ..AxContext::default()
        };
    }
    let (before_caret, after_caret) = match (raw.value.as_deref(), raw.range) {
        (Some(value), Some((location, length))) => (
            Some(prefix_before_caret(value, location, CONTEXT_CHAR_LIMIT)),
            Some(suffix_after_caret(
                value,
                location.saturating_add(length),
                AFTER_CARET_CHAR_LIMIT,
            )),
        ),
        _ => (None, None),
    };
    let selection = raw
        .selected
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(SELECTION_CHAR_LIMIT).collect());
    AxContext {
        before_caret,
        selection,
        after_caret,
        browser_mode,
    }
}

/// PERM-F (supersedes YV50's `text_before_cursor`) — the focused field's
/// context for one take: before-caret, selection, after-caret and, for the
/// browsers in [`URL_BROWSERS`], the mode the page URL implies.
///
/// Never fails: Accessibility denied, nothing focused, a secure field or any
/// AX error degrades to (part of) [`AxContext::default`], which is the
/// pre-PERM-F path. Nothing here is logged, stored, emitted or sent.
pub fn read_ax_context(bundle_id: Option<&str>) -> AxContext {
    let want_url = bundle_id.is_some_and(is_url_browser);
    context_from_reads(crate::permissions::is_accessibility_trusted(), || {
        #[cfg(target_os = "macos")]
        {
            ax::read_raw(want_url)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = want_url;
            AxRaw::default()
        }
    })
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

/// yap-insert-bridge-2026-10-26 — the frontmost app's bundle path (NSWorkspace,
/// no Accessibility needed), for the Electron check in `paste`.
pub fn frontmost_bundle_path() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
        let app = workspace.frontmostApplication()?;
        let url = app.bundleURL()?;
        let path = url.path()?;
        Some(path.to_string()).filter(|s| !s.is_empty())
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
