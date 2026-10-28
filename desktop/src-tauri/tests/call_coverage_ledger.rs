//! yap-call-0 (absorbs yap-call-4) — the catalog, the fixtures and
//! docs/loop/CALL-COVERAGE.md agree:
//! - every catalog bundle id has a ledger row;
//! - every verified:true row has a NON-synthetic fixture expecting it;
//! - a browser row is verified only from a fixture whose ax_source is "yap";
//! - every fixture scores as its `expect` block says;
//! - every system_daemon row with verified:false yields the "your side" copy.

use std::path::{Path, PathBuf};

use serde_json::Value;
use wilson_voice_lib::call_signals::{
    catalog, offer_copy, score, AppKind, AxSource, CallCandidate, CallCatalog, CallMemory,
    CallSnapshot, Via, CATALOG_JSON,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn fixtures() -> Vec<(String, Value, CallSnapshot)> {
    let dir = root().join("tests/fixtures/call_snapshots");
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).expect("fixture dir") {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "json") {
            let raw = std::fs::read_to_string(&p).unwrap();
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            let v: Value = serde_json::from_str(&raw).unwrap();
            let s: CallSnapshot =
                serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{name}: {e}"));
            out.push((name, v, s));
        }
    }
    out
}

fn ledger_rows() -> Vec<String> {
    let doc = std::fs::read_to_string(root().join("../../docs/loop/CALL-COVERAGE.md"))
        .expect("docs/loop/CALL-COVERAGE.md");
    doc.lines()
        .filter(|l| l.trim_start().starts_with('|'))
        .map(str::to_string)
        .collect()
}

#[test]
fn call_coverage_ledger_catalog_parses() {
    let c = CallCatalog::parse(CATALOG_JSON).expect("bundled catalog parses");
    assert_eq!(&c, catalog());
    assert!(!c.apps.is_empty());
}

#[test]
fn call_coverage_ledger_every_catalog_bundle_has_a_row() {
    let rows = ledger_rows();
    for app in &catalog().apps {
        let needle = format!("`{}`", app.bundle_id);
        assert!(
            rows.iter().any(|r| r.contains(&needle)),
            "CALL-COVERAGE.md has no row for {needle}"
        );
    }
}

#[test]
fn call_coverage_ledger_verified_rows_have_recorded_fixtures() {
    let fx = fixtures();
    for app in catalog().apps.iter().filter(|a| a.verified) {
        let recorded: Vec<_> = fx
            .iter()
            .filter(|(_, v, s)| {
                !s.synthetic && v["expect"]["bundle_id"].as_str() == Some(&app.bundle_id)
            })
            .collect();
        assert!(
            !recorded.is_empty(),
            "{} is verified:true with no recorded fixture",
            app.bundle_id
        );
        if app.kind == AppKind::Browser {
            assert!(
                recorded
                    .iter()
                    .any(|(_, _, s)| s.ax_source == AxSource::Yap),
                "{} (browser) is verified only from a fixture whose ax_source is yap",
                app.bundle_id
            );
        }
    }
}

#[test]
fn call_coverage_ledger_every_fixture_scores_as_its_row_says() {
    for (name, v, s) in fixtures() {
        let e = &v["expect"];
        assert!(e.is_object(), "{name}: no expect block");
        let got = score(&s, catalog(), &CallMemory::default());
        if let Some(b) = e["bundle_id"].as_str() {
            assert!(
                catalog().app(b).is_some(),
                "{name}: expect.bundle_id {b} not in catalog"
            );
        }
        assert_eq!(
            got.as_ref().map(|c| c.bundle_id.as_str()),
            e["bundle_id"].as_str(),
            "{name}"
        );
        assert_eq!(
            Some(got.as_ref().is_some_and(|c| c.offers())),
            e["offer"].as_bool(),
            "{name}"
        );
        let via = got.as_ref().map(|c| serde_json::to_value(c.via).unwrap());
        assert_eq!(
            via.as_ref().and_then(|v| v.as_str()),
            e["via"].as_str(),
            "{name}: via"
        );
        let text = got
            .as_ref()
            .filter(|c| c.offers())
            .map(|c| offer_copy(c, catalog()));
        assert_eq!(text.as_deref(), e["text"].as_str(), "{name}: text");
    }
}

#[test]
fn call_coverage_ledger_unverified_daemons_say_your_side() {
    let mut seen = 0;
    for app in catalog()
        .apps
        .iter()
        .filter(|a| a.kind == AppKind::SystemDaemon)
    {
        let c = CallCandidate {
            bundle_id: app.bundle_id.clone(),
            name: app.name.clone(),
            confidence: 70,
            via: Via::Daemon,
            pid: 1,
        };
        let copy = offer_copy(&c, catalog());
        if !app.verified {
            seen += 1;
            assert!(copy.contains("your side"), "{}: {copy}", app.bundle_id);
        }
    }
    assert!(seen >= 2, "FaceTime and Phone ship unverified");
    // And the ledger never claims a two-sided FaceTime/Phone recording.
    for r in ledger_rows() {
        if r.contains("`com.apple.FaceTime`") || r.contains("`com.apple.mobilephone`") {
            assert!(r.contains("your side"), "{r}");
        }
    }
}
