//! yap-call-0 — the call signal matrix, as a table over every fixture in
//! `tests/fixtures/call_snapshots/`: fixture → (bundle_id, offer?, via, offer
//! text). Plus the privacy rule: no fixture carries a window title as text.

use std::path::{Path, PathBuf};

use serde_json::Value;
use wilson_voice_lib::call_signals::{catalog, offer_copy, score, CallMemory, CallSnapshot, Via};

/// (fixture, bundle_id, offers, via, offer text)
type Row = (
    &'static str,
    Option<&'static str>,
    bool,
    Option<Via>,
    Option<&'static str>,
);

const TABLE: &[Row] = &[
    ("idle-yap-holds-mic", None, false, None, None),
    ("idle-yap-quiet", None, false, None, None),
    ("yap-dictating-only", None, false, None, None),
    (
        "zoom-in-call",
        Some("us.zoom.xos"),
        true,
        Some(Via::Bundle),
        Some("Zoom call — record it?"),
    ),
    (
        "chrome-meet-in-call",
        Some("com.google.Chrome"),
        true,
        Some(Via::HelperParent),
        Some("Google Meet call — record it?"),
    ),
    ("chrome-youtube-playing", None, false, None, None),
    (
        "chrome-input-ax-unknown",
        Some("com.google.Chrome"),
        true,
        Some(Via::HelperParent),
        Some("Google Chrome call — record it?"),
    ),
    (
        "chrome-input-no-title-match",
        Some("com.google.Chrome"),
        false,
        Some(Via::HelperParent),
        None,
    ),
    ("safari-webkit-input-no-title", None, false, None, None),
    (
        "safari-meet-with-title",
        Some("com.apple.Safari"),
        true,
        Some(Via::WebkitTitle),
        Some("Google Meet call — record it?"),
    ),
    (
        "facetime-daemon-input",
        Some("com.apple.FaceTime"),
        true,
        Some(Via::Daemon),
        Some("FaceTime call — record your side?"),
    ),
    (
        "phone-callservicesd-input",
        Some("com.apple.mobilephone"),
        true,
        Some(Via::Daemon),
        Some("Phone call — record your side?"),
    ),
];

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/call_snapshots")
}

fn fixtures() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = std::fs::read_dir(dir())
        .expect("fixture dir")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| {
            (
                p.file_stem().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(&p).unwrap(),
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn call_signal_matrix_every_fixture_scores_as_tabled() {
    let all = fixtures();
    assert!(all.len() >= 8, "at least 8 fixtures, found {}", all.len());
    for (name, raw) in &all {
        let row = TABLE
            .iter()
            .find(|r| r.0 == name)
            .unwrap_or_else(|| panic!("fixture {name} has no row in TABLE"));
        let snap: CallSnapshot =
            serde_json::from_str(raw).unwrap_or_else(|e| panic!("{name}: {e}"));
        let got = score(&snap, catalog(), &CallMemory::default());
        let bundle = got.as_ref().map(|c| c.bundle_id.as_str());
        let offers = got.as_ref().is_some_and(|c| c.offers());
        let via = got.as_ref().map(|c| c.via);
        let text = got
            .as_ref()
            .filter(|c| c.offers())
            .map(|c| offer_copy(c, catalog()));
        assert_eq!(bundle, row.1, "{name}: bundle ({got:?})");
        assert_eq!(offers, row.2, "{name}: offer ({got:?})");
        assert_eq!(via, row.3, "{name}: via");
        assert_eq!(text.as_deref(), row.4, "{name}: offer text");
        // The fixture's own `expect` block agrees with the table.
        let v: Value = serde_json::from_str(raw).unwrap();
        let e = &v["expect"];
        assert_eq!(e["bundle_id"].as_str(), row.1, "{name}: expect.bundle_id");
        assert_eq!(e["offer"].as_bool(), Some(row.2), "{name}: expect.offer");
        assert_eq!(e["text"].as_str(), row.4, "{name}: expect.text");
    }
    for row in TABLE {
        assert!(
            all.iter().any(|(n, _)| n == row.0),
            "TABLE row {} has no fixture",
            row.0
        );
    }
}

fn title_string_fields(v: &Value, path: &str, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            for (k, val) in m {
                let lk = k.to_ascii_lowercase();
                if (lk == "title" || lk == "raw_title" || lk.ends_with("_title")) && val.is_string()
                {
                    out.push(format!("{path}.{k}"));
                }
                title_string_fields(val, &format!("{path}.{k}"), out);
            }
        }
        Value::Array(a) => {
            for (i, val) in a.iter().enumerate() {
                title_string_fields(val, &format!("{path}[{i}]"), out);
            }
        }
        _ => {}
    }
}

#[test]
fn call_signal_matrix_no_fixture_has_a_title_string_field() {
    // Prove the walker can see the shape we fear before trusting its silence.
    let bad: Value =
        serde_json::json!({"browser_titles": [{"raw_title": "Inbox", "len": 5}], "title": "x"});
    let mut hits = Vec::new();
    title_string_fields(&bad, "$", &mut hits);
    assert_eq!(hits.len(), 2, "{hits:?}");
    for (name, raw) in fixtures() {
        let v: Value = serde_json::from_str(&raw).unwrap();
        let mut hits = Vec::new();
        title_string_fields(&v, "$", &mut hits);
        assert!(hits.is_empty(), "{name} carries title text at {hits:?}");
        assert!(!raw.contains("\"title\""), "{name}: a \"title\" key");
    }
}
