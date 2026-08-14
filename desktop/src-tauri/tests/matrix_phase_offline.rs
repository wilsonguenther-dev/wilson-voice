//! YV99 · the machine-checkable half of the phase-closing acceptance.
//!
//! The phase's stated acceptance is a 10-minute meeting completing start to
//! finish **with Wi-Fi off**, demonstrated on camera (finding #30). The camera
//! half is a human's job and is scripted in `docs/yap22a-phase-demo.md`. The
//! part that should not depend on someone remembering to pull the Wi-Fi is the
//! claim underneath it: *nothing on the meeting path can reach the network at
//! all.* That is checkable now, on every commit, and it is what makes the demo a
//! confirmation rather than the only evidence.
//!
//! The instrument is the one `crash.rs` already established for exactly this
//! claim (`no_network_no_transcript_in_crash_rows`): read the source of the
//! modules on the path and assert there is no network surface in them. Needles
//! are assembled with `concat!` so this file cannot match itself.
//!
//! **The honest carve-out.** Two things in the app DO use the network, neither
//! on this path, and pretending otherwise would be the false claim this test
//! exists to prevent:
//!   * model acquisition — `models.rs` downloads the ASR model and `vad.rs`
//!     fetches Silero once, both cached and offline forever after. The demo
//!     therefore runs on an install whose models are already present, which is
//!     also every real user after first launch.
//!   * `license.rs` refreshes a refund list in the background. It is not
//!     reachable from meeting capture, transcription, storage or export, and
//!     every way it can fail leaves the app exactly as it was (YP2).

use std::fs;
use std::path::PathBuf;

/// Every module a meeting's audio, transcript, and export actually flows
/// through, on this branch. Anything added to the meeting path belongs here.
const MEETING_PATH: &[&str] = &[
    "meetings.rs",       // the schema, the row types, the Markdown export
    "meeting_matrix.rs", // YV99's policy
    "record.rs",         // capture + the crash-safe journal
    "input_format.rs",   // the AirPods / format-change state machine
    "resample.rs",       // anti-alias decimation to 16 kHz
    "asr_engine.rs",     // the local Parakeet/whisper.cpp engine wrapper
    "transcription.rs",  // the engine lease and the timeout ladder
    "db.rs",             // storage, FTS, delete-cascade
];

fn src(file: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(file);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn the_whole_meeting_path_has_no_network_surface() {
    let needles = [
        concat!("req", "west"),
        concat!("ur", "eq"),
        concat!("Tcp", "Stream"),
        concat!("Udp", "Socket"),
        concat!("to_socket", "_addrs"),
        concat!("http", "://"),
        concat!("https", "://"),
    ];

    for module in MEETING_PATH {
        let body = src(module);
        for needle in needles {
            assert!(
                !body.contains(needle),
                "{module} is on the meeting path and must have no network surface — found `{needle}`. \
                 If this module legitimately needs the network, the phase's \"records with Wi-Fi off\" \
                 claim is no longer true and the claim has to change, not this test."
            );
        }
    }
}

/// The list above is only as good as its being kept current. A module that
/// vanishes (a rename, a split) must fail loudly rather than silently stop being
/// checked — a passing test over an empty set is the worst possible outcome
/// here.
#[test]
fn every_module_on_the_list_still_exists() {
    assert!(MEETING_PATH.len() >= 8);
    for module in MEETING_PATH {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join(module);
        assert!(
            path.exists(),
            "{module} is listed on the meeting path but no longer exists — if it was renamed or \
             split, update MEETING_PATH so its replacement is still checked"
        );
    }
}
