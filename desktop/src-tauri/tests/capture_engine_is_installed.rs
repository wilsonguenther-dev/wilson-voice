//! YV95 / finding #6 — the merge gate that keeps the Record button from staying
//! dead.
//!
//! Finding #6 is classified a *phase merge gate*: "22 ships a feature nobody can
//! reach." YV95 builds all four entry points, but the thing behind them —
//! `meeting::MeetingSession`, YV91 / PR #108 — is not on `main` yet. On this
//! branch alone every entry point is honestly disabled, with
//! [`NO_ENGINE_MESSAGE`] as the reason, and nothing claims a recording is
//! happening.
//!
//! That is defensible for exactly as long as the capture module does not exist.
//! The moment it does, a `main` with four disabled controls and no
//! `install_capture_engine` call is a permanently dead Record button that no
//! test complains about — which is how a merge gate quietly stops gating.
//!
//! So this file is a source-level gate rather than a behaviour test: it reads
//! `src/` and fails the build the first time the capture module lands without
//! the one line that connects it. Whichever of #108 / #112 merges second cannot
//! land green without doing the wiring.

mod support;

use std::path::{Path, PathBuf};

use wilson_voice_lib::meeting_control::{capture_available, NO_ENGINE_MESSAGE};

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn read(name: &str) -> String {
    let p = src_dir().join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// Comments do not install anything. `lib.rs` carries the wiring instructions as
/// a marked comment block on purpose, so a naive `contains()` over the whole
/// file would report the gate satisfied by its own TODO.
fn calls_installer(source: &str) -> bool {
    source
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("//"))
        .any(|l| l.contains("install_capture_engine("))
}

/// True once YV91's capture module is actually in the tree. Keyed on the type
/// the engine is built from, not merely on the file name, so an unrelated
/// `meeting.rs` stub cannot trip the gate.
fn capture_module_landed() -> bool {
    let p = src_dir().join("meeting.rs");
    match std::fs::read_to_string(p) {
        Ok(s) => s.contains("MeetingSession"),
        Err(_) => false,
    }
}

/// The gate. Passes today (no `meeting.rs`); fails the instant #108 lands
/// without the install line.
#[test]
fn the_record_button_cannot_stay_dead_once_the_capture_module_lands() {
    let lib = read("lib.rs");
    let installed = calls_installer(&lib);

    if capture_module_landed() {
        assert!(
            installed,
            "src/meeting.rs is in the tree (YV91 landed), but nothing calls \
             meeting_control::install_capture_engine() outside a comment. That \
             ships a Record button, a ⌃⌘M chord, a pill control and an empty-state \
             CTA that are all permanently disabled — finding #6's phase merge gate, \
             reopened. Fix: at the marked spot in lib.rs::setup, install an engine \
             adapting meeting::MeetingSession to meeting_control::CaptureEngine, \
             then run the timed repro in docs/pr-screenshots/YV95/\
             manual-repro-10-second-start.md."
        );
    } else {
        // Not an error — it is the branch state this gate exists to time-box.
        // Asserted so the "why is this allowed" answer lives in the test output
        // rather than in a reviewer's memory.
        assert!(
            !installed,
            "an engine is installed but src/meeting.rs is missing — the wiring \
             and the capture implementation must land in the same tree"
        );
    }
}

/// The other half of honesty: while no engine is installed, the control plane
/// says so in words instead of failing silently. This is what makes the disabled
/// state defensible rather than a false capability claim.
#[test]
fn with_no_engine_installed_every_entry_point_says_why() {
    let _guard = support::exclusive();
    // Deliberately does NOT install the fake engine: this asserts the shipped
    // binary's state on this branch.
    assert!(
        !capture_available(),
        "no engine may be installed by default — the tests that need one install \
         the synthetic engine themselves"
    );

    let dir = support::temp_dir("yv95-no-engine");
    let db = std::sync::Arc::new(support::open_db(&dir));
    let c = wilson_voice_lib::meeting_control::MeetingController::new(
        db,
        std::sync::Arc::new(|_: &wilson_voice_lib::meeting_control::MeetingStatus| {}),
    );

    let s = c.status();
    assert!(!s.recording);
    assert!(!s.capture_available, "the UI's enabled state reads this");
    assert_eq!(
        s.unavailable_reason.as_deref(),
        Some(NO_ENGINE_MESSAGE),
        "a disabled control that does not say why is its own bug report"
    );

    let err = c
        .start(&dir, None)
        .expect_err("start must refuse, not pretend");
    assert_eq!(err, NO_ENGINE_MESSAGE);
}
