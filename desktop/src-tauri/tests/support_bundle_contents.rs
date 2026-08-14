//! YV98 — what is actually inside `Yap-Diagnostics-<version>-<stamp>.zip`.
//!
//! The bundle is verified through `/usr/bin/unzip`, not through the writer that
//! produced it. A hand-rolled archive that only this crate can read would pass
//! a round-trip test and fail on the first support email, so the assertion has
//! to come from the tool on the other end.
//!
//! This file also carries the two no-network guards. `crash_no_network_surface`
//! is the backlog's own acceptance command: it re-runs `crash.rs`'s grep from
//! OUTSIDE that file, so "this item added no network code to crash.rs" is
//! checked by a test this item owns, while `crash.rs`'s own
//! `no_network_no_transcript_in_crash_rows` stays untouched.

use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{DateTime, Utc};
use wilson_voice_lib::db::CrashEvent;
use wilson_voice_lib::{crash, support};

const CRASH_RS: &str = include_str!("../src/crash.rs");
const SUPPORT_RS: &str = include_str!("../src/support.rs");

fn at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-08-12T14:15:30Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("yv98-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn one_crash() -> CrashEvent {
    CrashEvent {
        id: "c1".into(),
        occurred_at: at(),
        kind: crash::KIND_NATIVE.into(),
        signature: "EXC_CRASH (SIGABRT)".into(),
        source_file: "wilson-voice-2026-08-12-091400.ips".into(),
        details: "process: wilson-voice\nversion: 0.8.0\nexception: EXC_CRASH (SIGABRT)".into(),
        acknowledged: false,
    }
}

/// Build + write the real bundle, and hand back the zip's path plus the crash
/// summary text the app would have put in it.
fn write_bundle(dir: &Path) -> (PathBuf, String) {
    let summary = crash::summary_text(&[one_crash()]);
    let prepared = support::prepare(
        "0.8.0",
        support::BundleInputs {
            crash_summary: summary.clone(),
            logs: vec![
                (
                    "yap.log".into(),
                    "[2026-08-12T09:13:02Z INFO  wilson_voice_lib::polish] polish sidecar starting\n"
                        .into(),
                ),
                (
                    "yap.log.1".into(),
                    "[2026-08-11T09:13:02Z WARN  wilson_voice_lib::db] wal_checkpoint failed: database is locked\n"
                        .into(),
                ),
                (
                    "yap.log.2".into(),
                    "[2026-08-10T09:13:02Z INFO  wilson_voice_lib] recording cancelled\n".into(),
                ),
            ],
            environment: support::environment_block("macOS 26.5.2 (25F84)", "aarch64"),
            permissions: "accessibility: true\nmicrophone: true\n".into(),
            models: "selected_asr_model: small.en\n".into(),
            username: "wilsonguenther".into(),
            generated_at: at(),
        },
    );
    let path = dir.join(&prepared.file_name);
    support::write_zip(&path, &prepared.entries, prepared.generated_at).expect("write zip");
    (path, summary)
}

fn unzip(args: &[&str]) -> String {
    let out = Command::new("/usr/bin/unzip")
        .args(args)
        .output()
        .expect("run unzip");
    assert!(
        out.status.success(),
        "unzip {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn the_zip_is_a_zip_that_unzip_can_read() {
    let dir = temp_dir("valid");
    let (zip, _) = write_bundle(&dir);
    let report = unzip(&["-t", zip.to_str().unwrap()]);
    assert!(
        report.contains("No errors detected"),
        "unzip -t did not vouch for the archive:\n{report}"
    );
}

#[test]
fn the_zip_carries_the_crash_summary_the_logs_and_the_version_block() {
    let dir = temp_dir("contents");
    let (zip, summary) = write_bundle(&dir);
    let zip_path = zip.to_str().unwrap();

    // Every rotation is a member, under a `logs/` prefix.
    let listing = unzip(&["-Z1", zip_path]);
    for name in [
        "README.txt",
        "environment.txt",
        "crash-summary.txt",
        "permissions.txt",
        "models.txt",
        "logs/yap.log",
        "logs/yap.log.1",
        "logs/yap.log.2",
    ] {
        assert!(
            listing.lines().any(|l| l.trim() == name),
            "{name} is missing from the bundle:\n{listing}"
        );
    }

    // `crash::summary_text()` output, byte for byte.
    let packed_summary = unzip(&["-p", zip_path, "crash-summary.txt"]);
    assert_eq!(packed_summary, summary);
    assert!(packed_summary.contains("EXC_CRASH (SIGABRT)"));

    // The Info.plist version/OS block.
    let env_block = unzip(&["-p", zip_path, "environment.txt"]);
    assert!(env_block.contains("CFBundleIdentifier: com.wilsonguenther.wilson-voice"));
    assert!(
        env_block.contains(&format!(
            "CFBundleShortVersionString: {}",
            env!("CARGO_PKG_VERSION")
        )),
        "version block does not describe this binary:\n{env_block}"
    );
    assert!(env_block.contains("LSMinimumSystemVersion: "));
    assert!(env_block.contains("os: macOS 26.5.2 (25F84)"));
    assert!(env_block.contains("arch: aarch64"));

    // The rotations carry their own contents, not the newest log three times.
    assert!(unzip(&["-p", zip_path, "logs/yap.log"]).contains("polish sidecar starting"));
    assert!(unzip(&["-p", zip_path, "logs/yap.log.1"]).contains("wal_checkpoint failed"));
    assert!(unzip(&["-p", zip_path, "logs/yap.log.2"]).contains("recording cancelled"));
}

/// The whole dirty fixture — including the multi-line panic record — packed
/// through the REAL `prepare` + `write_zip` and read back with `/usr/bin/unzip`,
/// because that is the path a support email actually takes.
///
/// This exists because a redaction hole was found end to end rather than in a
/// unit: `logging.rs` writes `PANIC at {loc}: {msg}\nbacktrace:\n{bt}` as ONE
/// record, so a payload's continuation lines reach the redactor on their own,
/// and a line that merely began with `[` was trusted as an `env_logger` header
/// and shipped verbatim up to the first `]`. Asserting on `build_entries` alone
/// would not have caught the byte that came out of the zip.
#[test]
fn the_bytes_unzip_prints_carry_no_transcript() {
    const DIRTY: &str = include_str!("fixtures/support/dirty-yap.log");
    let dir = temp_dir("e2e-redaction");
    let prepared = support::prepare(
        "0.8.0",
        support::BundleInputs {
            crash_summary: crash::summary_text(&[one_crash()]),
            logs: vec![("yap.log".into(), DIRTY.to_string())],
            environment: support::environment_block("macOS 26.5.2 (25F84)", "aarch64"),
            permissions: "accessibility: true\n".into(),
            models: "selected_asr_model: small.en\n".into(),
            username: "wilsonguenther".into(),
            generated_at: at(),
        },
    );
    let path = dir.join(&prepared.file_name);
    support::write_zip(&path, &prepared.entries, prepared.generated_at).expect("write zip");

    let packed = unzip(&["-p", path.to_str().unwrap(), "logs/yap.log"]);
    for (label, needle) in [
        ("a quoted transcript", "tell the board about the fundraise"),
        (
            "a bracketed line masquerading as a log header",
            "her HIV result came back positive",
        ),
        (
            "the prose after that fake header",
            "and I have not told anyone at work",
        ),
        (
            "a fake header inside the backtrace",
            "she asked me not to put it in the notes",
        ),
        ("the account name", "wilsonguenther"),
    ] {
        assert!(
            !packed.contains(needle),
            "{label} came out of the zip:\n{packed}"
        );
    }
    // And the bundle is still a bundle: the panic LOCATION and the operational
    // lines are what support reads.
    assert!(
        packed.contains("PANIC at src/transcription.rs:212:9"),
        "{packed}"
    );
    assert!(packed.contains("polish sidecar starting"), "{packed}");
}

#[test]
fn the_preview_is_the_bytes_that_get_written() {
    let dir = temp_dir("preview");
    let summary = crash::summary_text(&[one_crash()]);
    let prepared = support::prepare(
        "0.8.0",
        support::BundleInputs {
            crash_summary: summary,
            logs: vec![(
                "yap.log".into(),
                "[2026-08-12T09:13:02Z INFO  wilson_voice_lib::polish] polish sidecar starting\n"
                    .into(),
            )],
            environment: support::environment_block("macOS 26.5.2 (25F84)", "aarch64"),
            permissions: "accessibility: true\n".into(),
            models: "selected_asr_model: small.en\n".into(),
            username: "wilsonguenther".into(),
            generated_at: at(),
        },
    );
    let preview = prepared.preview(true);
    assert_eq!(preview.recipient, "wilson@drivia.consulting");
    assert_eq!(
        preview.file_name,
        "Yap-Diagnostics-0.8.0-20260812-141530.zip"
    );
    assert_eq!(preview.entries.len(), prepared.entries.len());

    let path = dir.join(&prepared.file_name);
    support::write_zip(&path, &prepared.entries, prepared.generated_at).expect("write zip");
    let zip_path = path.to_str().unwrap();

    // Every previewed excerpt is a genuine prefix of the member that shipped —
    // the whole point of showing it before writing anything.
    for entry in &preview.entries {
        let packed = unzip(&["-p", zip_path, &entry.name]);
        assert!(
            packed.starts_with(&entry.excerpt),
            "preview of {} is not what was packed",
            entry.name
        );
        assert_eq!(packed.len(), entry.bytes);
    }
}

/// The backlog's acceptance command, and the reason it exists: this item must
/// not have put a network call into `crash.rs` on its way to shipping a send
/// button. Needles are assembled with `concat!` so this file cannot match
/// itself.
#[test]
fn crash_no_network_surface() {
    for needle in [
        concat!("req", "west"),
        concat!("Tcp", "Stream"),
        concat!("Udp", "Socket"),
        concat!("ur", "eq"),
        concat!("http", "://"),
        concat!("https", "://"),
        concat!("to_socket", "_addrs"),
    ] {
        assert!(
            !CRASH_RS.contains(needle),
            "crash telemetry must have no network surface, found {needle}"
        );
    }
}

/// And the new module holds the same line. The send button hands the file to
/// the user's mail client; it does not upload anything, and there is no
/// "off by default" client here to turn on later (O5 was declined).
#[test]
fn support_adds_no_network_surface() {
    for needle in [
        concat!("req", "west"),
        concat!("Tcp", "Stream"),
        concat!("Udp", "Socket"),
        concat!("ur", "eq"),
        concat!("to_socket", "_addrs"),
        concat!("Socket", "Addr"),
        concat!("tauri_plugin", "_http"),
    ] {
        assert!(
            !SUPPORT_RS.contains(needle),
            "the support bundle must have no network surface, found {needle}"
        );
    }
}

/// The manual half of the backlog's acceptance, made runnable.
///
/// Whether `NSSharingService(named: .composeEmail)` exists and whether
/// `canPerformWithItems:` accepts a real zip are facts about the machine, not
/// about this code — which is exactly why the guard is load-bearing. This test
/// writes a genuine bundle, asks AppKit the question against that file, and
/// prints the answer, so a PR can carry the result from a real Mac instead of
/// an assertion nobody ran:
///
///   cargo test --test support_bundle_contents manual_compose -- --ignored --nocapture
#[test]
#[ignore = "probes this machine's mail client; run manually"]
fn manual_compose_probe_on_this_mac() {
    let dir = temp_dir("probe");
    let (zip, _) = write_bundle(&dir);
    let available = support::compose_email(
        Some(&zip),
        &support::subject_line("0.8.0", "EXC_CRASH (SIGABRT)"),
        &support::body_text("0.8.0", &support::os_version(), "EXC_CRASH (SIGABRT)"),
        true,
    );
    println!("bundle: {}", zip.display());
    println!("bundle bytes: {}", std::fs::metadata(&zip).unwrap().len());
    println!("os: {}", support::os_version());
    println!("canPerformWithItems: {available}");
    println!(
        "=> the button would take the {} path on this Mac",
        if available {
            "COMPOSE"
        } else {
            "REVEAL + clipboard"
        }
    );
}

/// The other manual half: the fallback is not a dead end.
///
/// `canPerformWithItems:` cannot be forced to say no without changing the
/// machine's default mail client, so this exercises the branch it guards
/// directly — reveal the file in Finder and put the address on the clipboard —
/// and reads the pasteboard back to prove the address actually landed. The
/// user's own clipboard is saved and restored around it, because a test that
/// eats your clipboard is its own kind of bug.
///
///   cargo test --test support_bundle_contents manual_fallback -- --ignored --nocapture
#[test]
#[ignore = "opens Finder and touches the clipboard; run manually"]
fn manual_fallback_reveals_and_copies_the_address() {
    let saved = String::from_utf8_lossy(
        &Command::new("/usr/bin/pbpaste")
            .output()
            .expect("pbpaste")
            .stdout,
    )
    .into_owned();

    let dir = temp_dir("fallback");
    let (zip, _) = write_bundle(&dir);
    let outcome = support::fallback_outcome(&zip);

    let clipboard = String::from_utf8_lossy(
        &Command::new("/usr/bin/pbpaste")
            .output()
            .expect("pbpaste")
            .stdout,
    )
    .into_owned();

    println!("method: {}", outcome.method);
    println!("message: {}", outcome.message);
    println!("clipboard now: {clipboard}");

    assert_eq!(outcome.method, "reveal");
    assert_eq!(clipboard, support::SUPPORT_EMAIL);
    assert!(outcome.message.contains(support::SUPPORT_EMAIL));
    assert!(zip.exists(), "the file the user was just shown must exist");

    // Put the user's clipboard back.
    let mut restore = Command::new("/usr/bin/pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("pbcopy");
    use std::io::Write;
    restore
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(saved.as_bytes())
        .expect("restore clipboard");
    let _ = restore.wait();
}
