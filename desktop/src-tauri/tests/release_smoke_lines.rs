//! yap-p0-release-smoke — the release-smoke parser is proven against lines the
//! app ACTUALLY emits.
//!
//! Every fixture in scripts/qa/fixtures/release-smoke-matrix/ is a GOLDEN file
//! written from the real `smoke.rs` formatters. `golden_fixtures_match_formatter`
//! regenerates each one and compares: on a mismatch (or a missing file) it
//! rewrites the file — so `git diff` shows exactly what drifted — and fails.
//! `parser_enforces_the_matrix` then runs `scripts/qa/release-smoke.sh --parse`
//! over each and checks the exit code its matrix row demands.

use std::path::PathBuf;
use std::process::Command;

use wilson_voice_lib::hotkeys::{CarbonStatus, FnTapStatus};
use wilson_voice_lib::paste::InsertPath;
use wilson_voice_lib::perm_probe::{ProbeState, Service};
use wilson_voice_lib::smoke::{hotkeys_line, insert_line, probe_line, quit_line};

const PREFIX: &str = "[2026-10-19T00:00:00Z INFO  wilson_voice_lib::smoke] ";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn matrix_dir() -> PathBuf {
    repo_root().join("scripts/qa/fixtures/release-smoke-matrix")
}

/// (service, state, ledger) for all four, in `Service::ALL` order.
type Probes = [(Service, ProbeState, bool); 4];

fn all(state: ProbeState) -> Probes {
    [
        (Service::Accessibility, state, true),
        (Service::ListenEvent, state, true),
        (Service::PostEvent, state, false),
        (Service::Microphone, state, false),
    ]
}

struct Row {
    name: &'static str,
    exit: i32,
    probes: Probes,
    carbon: CarbonStatus,
    fn_tap: FnTapStatus,
    insert: (InsertPath, bool),
    quit: bool,
}

fn rows() -> Vec<Row> {
    use ProbeState::*;
    let ok_row = |name, exit| Row {
        name,
        exit,
        probes: all(Granted),
        carbon: CarbonStatus::Registered,
        fn_tap: FnTapStatus::Delivering,
        insert: (InsertPath::Ax, true),
        quit: true,
    };
    vec![
        ok_row("granted-all-ax", 0),
        Row {
            probes: [
                (Service::Accessibility, NotDetermined, false),
                (Service::ListenEvent, NotDetermined, false),
                (Service::PostEvent, Denied, false),
                (Service::Microphone, NotDetermined, false),
            ],
            fn_tap: FnTapStatus::Blocked("CGEventTapCreate returned null".into()),
            insert: (InsertPath::CopyCard, true),
            ..ok_row("absent-grants-copy-card", 0)
        },
        Row {
            probes: all(Denied),
            fn_tap: FnTapStatus::Blocked("CGEventTapCreate returned null".into()),
            insert: (InsertPath::CopyCard, true),
            ..ok_row("all-dead-ledger-granted", 1)
        },
        Row {
            probes: [
                (Service::Accessibility, Denied, true),
                (Service::ListenEvent, Granted, true),
                (Service::PostEvent, Granted, false),
                (Service::Microphone, Granted, false),
            ],
            insert: (InsertPath::PasteBridge, true),
            ..ok_row("stale-ledger-granted-probe-denied", 1)
        },
        Row {
            insert: (InsertPath::CopyCard, true),
            ..ok_row("granted-but-copy-card", 1)
        },
        Row {
            carbon: CarbonStatus::Failed("HotKey already registered".into()),
            ..ok_row("carbon-failed", 1)
        },
        Row {
            quit: false,
            ..ok_row("no-quit", 1)
        },
    ]
}

fn render(row: &Row) -> String {
    let mut out = format!(
        "# golden: release-smoke matrix row {} (expect exit {}) — written by\n\
         # desktop/src-tauri/tests/release_smoke_lines.rs from the smoke.rs formatters. Do not hand-edit.\n",
        row.name, row.exit
    );
    let mut push = |l: String| {
        out.push_str(PREFIX);
        out.push_str(&l);
        out.push('\n');
    };
    for (s, st, ledger) in row.probes {
        push(probe_line(s, st, ledger));
    }
    push(hotkeys_line(&row.carbon, &row.fn_tap));
    push(insert_line(row.insert.0, row.insert.1));
    if row.quit {
        push(quit_line());
    }
    out
}

#[test]
fn golden_fixtures_match_formatter() {
    let dir = matrix_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let mut drift = Vec::new();
    for row in rows() {
        let path = dir.join(format!("{}.log", row.name));
        let want = render(&row);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(want.as_str()) {
            std::fs::write(&path, &want).unwrap();
            drift.push(row.name);
        }
    }
    assert!(
        drift.is_empty(),
        "golden fixtures drifted from the formatters and were rewritten: {drift:?}"
    );
}

#[test]
fn parser_enforces_the_matrix() {
    let script = repo_root().join("scripts/qa/release-smoke.sh");
    for row in rows() {
        // Parse what the formatter renders right now, not what is on disk.
        let tmp = std::env::temp_dir().join(format!("yap-release-smoke-{}.log", row.name));
        std::fs::write(&tmp, render(&row)).unwrap();
        let out = Command::new("bash")
            .arg(&script)
            .arg("--parse")
            .arg(&tmp)
            .output()
            .expect("run release-smoke.sh");
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(
            out.status.code(),
            Some(row.exit),
            "{}: {}",
            row.name,
            String::from_utf8_lossy(&out.stdout)
        );
    }
}
