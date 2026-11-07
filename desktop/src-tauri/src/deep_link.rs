//! yap-1106-pill-trial-clickable — the deep link BACK from the site.
//!
//! The "Trial ended" pill opens the drivia.consulting Yap page
//! (`license::PURCHASE_PAGE_URL`). Once a person has bought, the page hands
//! the license straight back to Yap with one link:
//!
//! ```text
//! yapvoice://license/activate?key=DCL-XXXX-XXXX-XXXX
//! ```
//!
//! macOS routes the scheme (Info.plist `CFBundleURLTypes`) to the running app
//! as `RunEvent::Opened`; lib.rs hands every URL to [`parse`] and, for an
//! [`DeepLink::Activate`], runs the SAME `license_client::LicenseClient::activate`
//! a pasted key runs, then emits `license` and [`YAPPY_HOP_EVENT`] so Yappy hops.
//!
//! A URL SCHEME IS REACHABLE FROM ANY WEB PAGE, so this parser is strict and
//! pure: one scheme, one host, one path, one query key, a `DCL-` key of plain
//! letters, digits and dashes, bounded length. Anything else is refused with a
//! reason that is logged; the key itself is NEVER logged (rules.telemetry). A
//! refused link does nothing at all — no window, no network call.

/// The URL scheme registered in Info.plist. Not `yap`: a three-letter scheme
/// is the kind another app already claims.
pub const SCHEME: &str = "yapvoice";

/// Backend → float webview: a license arrived by deep link; Yappy hops once.
/// Twinned with `YAPPY_HOP_EVENT` in `desktop/src/pill/gateClick.ts`.
pub const YAPPY_HOP_EVENT: &str = "yappy-hop";

/// Longest key accepted. Real `DCL-` keys are far shorter; this only bounds
/// what a hostile page can push through the parser.
pub const MAX_KEY_LEN: usize = 128;

/// What a deep link asks for. One variant today, on purpose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeepLink {
    /// Activate this `DCL-` key on this Mac (the same path as a paste).
    Activate { key: String },
}

/// Why a link was refused. `as_str` is the `reason=` in the log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    WrongScheme,
    UnknownPath,
    MissingKey,
    BadKey,
}

impl Refusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Refusal::WrongScheme => "wrong_scheme",
            Refusal::UnknownPath => "unknown_path",
            Refusal::MissingKey => "missing_key",
            Refusal::BadKey => "bad_key",
        }
    }
}

/// Parse one opened URL. Pure: no I/O, no logging, no clock.
pub fn parse(raw: &str) -> Result<DeepLink, Refusal> {
    let raw = raw.trim();
    let scheme_end = raw.find("://").ok_or(Refusal::WrongScheme)?;
    if !raw[..scheme_end].eq_ignore_ascii_case(SCHEME) {
        return Err(Refusal::WrongScheme);
    }
    let rest = &raw[scheme_end + 3..];
    // No fragment is ever expected; a `#` means the link was not ours.
    if rest.contains('#') {
        return Err(Refusal::UnknownPath);
    }
    let (path, query) = match rest.split_once('?') {
        Some((p, q)) => (p, q),
        None => (rest, ""),
    };
    let path = path.trim_end_matches('/');
    if !path.eq_ignore_ascii_case("license/activate") {
        return Err(Refusal::UnknownPath);
    }
    let mut key: Option<&str> = None;
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        if name == "key" {
            if key.is_some() {
                // Two keys in one link is not a link this site writes.
                return Err(Refusal::BadKey);
            }
            key = Some(value);
        }
    }
    let key = key.filter(|k| !k.is_empty()).ok_or(Refusal::MissingKey)?;
    if key.len() > MAX_KEY_LEN
        || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        || !crate::license_client::is_dcl_key(key)
    {
        return Err(Refusal::BadKey);
    }
    Ok(DeepLink::Activate {
        key: key.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO_PLIST: &str = include_str!("../Info.plist");
    const LIB_RS: &str = include_str!("lib.rs");
    const GATE_CLICK_TS: &str = include_str!("../../src/pill/gateClick.ts");

    #[test]
    fn deep_link_activate_parses_a_dcl_key() {
        assert_eq!(
            parse("yapvoice://license/activate?key=DCL-ABCD-1234-EFGH"),
            Ok(DeepLink::Activate {
                key: "DCL-ABCD-1234-EFGH".to_string()
            })
        );
        // Case-insensitive scheme and path, a trailing slash, extra params.
        assert_eq!(
            parse("YAPVOICE://License/Activate/?src=site&key=DCL-ABCD-1234"),
            Ok(DeepLink::Activate {
                key: "DCL-ABCD-1234".to_string()
            })
        );
    }

    #[test]
    fn deep_link_refuses_everything_else() {
        let cases: &[(&str, Refusal)] = &[
            ("https://drivia.consulting/yap", Refusal::WrongScheme),
            ("yap://license/activate?key=DCL-ABCD", Refusal::WrongScheme),
            ("no scheme here", Refusal::WrongScheme),
            (
                "yapvoice://license/deactivate?key=DCL-ABCD",
                Refusal::UnknownPath,
            ),
            (
                "yapvoice://open?url=https://evil.example",
                Refusal::UnknownPath,
            ),
            (
                "yapvoice://license/activate#key=DCL-ABCD",
                Refusal::UnknownPath,
            ),
            ("yapvoice://license/activate", Refusal::MissingKey),
            ("yapvoice://license/activate?key=", Refusal::MissingKey),
            (
                "yapvoice://license/activate?key=v1.token.sig",
                Refusal::BadKey,
            ),
            (
                "yapvoice://license/activate?key=DCL-AB%20CD",
                Refusal::BadKey,
            ),
            (
                "yapvoice://license/activate?key=DCL-AB&key=DCL-CD",
                Refusal::BadKey,
            ),
        ];
        for (raw, want) in cases {
            assert_eq!(parse(raw), Err(*want), "{raw}");
        }
        let long = format!(
            "yapvoice://license/activate?key=DCL-{}",
            "A".repeat(MAX_KEY_LEN)
        );
        assert_eq!(parse(&long), Err(Refusal::BadKey));
    }

    #[test]
    fn deep_link_scheme_is_registered_in_info_plist() {
        assert!(INFO_PLIST.contains("<key>CFBundleURLTypes</key>"));
        assert!(INFO_PLIST.contains(&format!("<string>{SCHEME}</string>")));
    }

    #[test]
    fn deep_link_is_wired_into_the_run_loop() {
        assert!(LIB_RS.contains("tauri::RunEvent::Opened"));
        assert!(LIB_RS.contains("fn handle_deep_links(app: &AppHandle, urls: Vec<String>)"));
        let arm: String = LIB_RS[LIB_RS.find("tauri::RunEvent::Opened").unwrap()..]
            .chars()
            .take(300)
            .collect();
        assert!(arm.contains("handle_deep_links("), "{arm}");
        // The hop event is one name on both sides.
        assert!(GATE_CLICK_TS.contains(&format!("YAPPY_HOP_EVENT = \"{YAPPY_HOP_EVENT}\"")));
    }

    #[test]
    fn gate_click_command_is_registered_and_names_no_url() {
        assert!(LIB_RS.contains("fn pill_gate_click("));
        assert!(LIB_RS.contains("            pill_gate_click,"));
        assert!(GATE_CLICK_TS.contains("GATE_CLICK_COMMAND = \"pill_gate_click\""));
        assert!(!GATE_CLICK_TS.contains("https://"));
        assert!(!GATE_CLICK_TS.contains(concat!("buy.", "stripe", ".com")));
    }
}
