//! YV121 acceptance — the readiness handshake and the refusal path, against the
//! REAL `yap-diarize` binary, with **zero model bytes**.
//!
//! ```sh
//! cargo test --test diarize_sidecar_handshake
//! ```
//!
//! ## Why this can never silently skip
//!
//! `tauri.conf.json`'s `bundle.externalBin` now lists `binaries/yap-diarize`,
//! and `tauri-build` checks that file on EVERY `cargo build` of this crate —
//! not only at bundle time. So the staged binary is a precondition of this test
//! binary COMPILING: if it were absent, `cargo test` would have failed before
//! reaching a single assertion. That is the opposite posture from
//! `meeting_eval.rs`'s corpus-absent skip, and it is available here precisely
//! because this sidecar ships no model — there is nothing to be missing.
//!
//! The resolution order below still prefers an explicitly named binary
//! (`YAP_DIARIZE_BIN`) and a plain workspace build, so `cargo test` from a dev
//! tree that just ran `cargo build -p yap-diarize` exercises what it just built.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};

use wilson_voice_lib::diarize_protocol::{
    parse_ready, parse_response_for, DiarizeRequest, DiarizeResponse, ERR_BAD_REQUEST,
    ERR_MODEL_NOT_FOUND, ERR_NO_MODELS, ERR_UNSUPPORTED_KIND,
};

/// The parent's shipped readiness budget. Asserted against, so a sidecar that
/// grew a multi-second startup would fail here rather than in production.
const READY_BUDGET: Duration = Duration::from_secs(10);

/// How long one stub-scale request may take. Everything this build does is a
/// path check, so a second is enormous.
const REQUEST_BUDGET: Duration = Duration::from_secs(5);

/// A path that certainly does not exist, in a directory that certainly does not
/// either — nothing here may depend on the machine it runs on.
const MISSING_MODEL: &str = "/nonexistent/yap-diarize-fixture/segmentation.onnx";

/// The built `yap-diarize`, wherever this checkout has one.
fn diarize_binary() -> PathBuf {
    if let Ok(named) = std::env::var("YAP_DIARIZE_BIN") {
        let path = PathBuf::from(named);
        assert!(path.is_file(), "YAP_DIARIZE_BIN is not a file: {path:?}");
        return path;
    }
    // Next to the test executable — where a workspace `cargo build` puts it.
    if let Some(sibling) = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.join("yap-diarize")))
        .filter(|p| p.is_file())
    {
        return sibling;
    }
    // The staged sidecar. Its presence is a precondition of this file having
    // compiled at all (see the module docs), so this branch is the one that
    // makes the test non-skippable in CI.
    let staged = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
    let found = std::fs::read_dir(&staged)
        .unwrap_or_else(|e| panic!("no staged sidecars at {staged:?}: {e}"))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("yap-diarize-"))
        });
    found.unwrap_or_else(|| {
        panic!(
            "no yap-diarize binary: build one with `cargo build -p yap-diarize` \
             or point YAP_DIARIZE_BIN at it"
        )
    })
}

/// A spawned sidecar and the two ends of its protocol stream.
struct Spawned {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl Spawned {
    fn start() -> Self {
        let mut child = Command::new(diarize_binary())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherited rather than piped: this test never reads stderr, and an
            // unread pipe would eventually block the child mid-write.
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the sidecar spawns");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Self {
            child,
            stdin,
            stdout,
        }
    }

    fn read_line(&mut self) -> String {
        let mut line = String::new();
        let read = self.stdout.read_line(&mut line).expect("read stdout");
        assert!(read > 0, "the sidecar closed stdout instead of answering");
        line
    }

    /// Send a raw line and read the one answer it produces.
    fn exchange_raw(&mut self, line: &str) -> String {
        writeln!(self.stdin, "{line}").expect("write stdin");
        self.stdin.flush().expect("flush stdin");
        self.read_line()
    }

    fn exchange(&mut self, req: &DiarizeRequest) -> DiarizeResponse {
        let line = serde_json::to_string(req).expect("encode");
        let answer = self.exchange_raw(&line);
        parse_response_for(&answer, req.id)
            .unwrap_or_else(|| panic!("not a response for id {}: {answer}", req.id))
    }
}

impl Drop for Spawned {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The FIRST line on stdout is the handshake, it arrives well inside the
/// parent's budget, and it announces a PROCESS — not a model.
///
/// The last part is the shape difference from `yap-polish` that the whole
/// protocol rests on: `model_loaded` in the handshake would put an ONNX session
/// build inside the parent's spawn budget and make "wedged" indistinguishable
/// from "big model", which is the confusion YV75 had to remove from the polish
/// path after the fact.
#[test]
fn the_sidecar_announces_itself_before_any_model_is_loaded() {
    let started = Instant::now();
    let mut sidecar = Spawned::start();
    let first = sidecar.read_line();
    let elapsed = started.elapsed();

    let ready = parse_ready(&first)
        .unwrap_or_else(|| panic!("the first stdout line is not the handshake: {first}"));
    assert_eq!(ready.kind, "ready");
    assert!(!ready.version.is_empty(), "the handshake carries a version");
    assert!(
        !first.contains("model_loaded"),
        "readiness here is the PROCESS; the model answer is `load_models`: {first}"
    );
    assert!(
        elapsed < READY_BUDGET,
        "spawn → ready took {elapsed:?}, past the parent's {READY_BUDGET:?} budget"
    );
    // Printed so the PR carries the measured headroom rather than a claim.
    println!("yap-diarize v{}: spawn → ready in {elapsed:?}", ready.version);
}

/// A `load_models` naming a path that is not there is a clean refusal — and the
/// process is still alive afterwards, which is the half that makes it a
/// refusal rather than a crash with a tidy exit line.
#[test]
fn a_missing_model_is_refused_cleanly_and_the_child_survives_it() {
    let mut sidecar = Spawned::start();
    assert!(parse_ready(&sidecar.read_line()).is_some(), "handshake first");

    let missing = std::path::Path::new(MISSING_MODEL);
    let started = Instant::now();
    let response = sidecar.exchange(&DiarizeRequest::load_models(1, missing, missing));
    assert!(started.elapsed() < REQUEST_BUDGET, "a path check is not slow");

    assert!(!response.ok, "a model that is not there did not load");
    assert_eq!(response.err_tag(), Some(ERR_MODEL_NOT_FOUND));
    assert_eq!(response.embedding_dim, None, "nothing was measured");
    // The tag is a tag: no path, no prose, nothing from this machine.
    let encoded = serde_json::to_string(&response).expect("encode");
    assert!(
        !encoded.contains("nonexistent") && !encoded.contains(".onnx"),
        "an error tag must never carry a path: {encoded}"
    );

    // Still alive, still answering, still holding no models — a crash would
    // fail the next exchange rather than this assertion.
    let after = sidecar.exchange(&DiarizeRequest::embed(2, missing));
    assert_eq!(after.err_tag(), Some(ERR_NO_MODELS));
}

/// Every other malformed line gets an ANSWER too. The parent is sitting on a
/// deadline for each one, and a sidecar that answers nothing turns a typo into
/// a ten-minute wait.
#[test]
fn a_malformed_line_is_answered_rather_than_swallowed() {
    let mut sidecar = Spawned::start();
    assert!(parse_ready(&sidecar.read_line()).is_some(), "handshake first");

    // Valid JSON, unparseable as a request, but the id is recoverable.
    let answer = sidecar.exchange_raw(r#"{"id":3,"kind":42}"#);
    let response = parse_response_for(&answer, 3).expect("an answer for id 3");
    assert_eq!(response.err_tag(), Some(ERR_BAD_REQUEST));

    // A kind this build does not implement — an app/sidecar version skew.
    let skew = sidecar.exchange_raw(r#"{"id":4,"kind":"transcribe"}"#);
    assert_eq!(
        parse_response_for(&skew, 4).expect("an answer for id 4").err_tag(),
        Some(ERR_UNSUPPORTED_KIND)
    );

    // Two lines with nothing to correlate: not JSON at all, and JSON with no
    // id. Neither can be answered, and neither may take the process down — the
    // request that follows proves it.
    writeln!(sidecar.stdin, "this is not json").expect("write");
    writeln!(sidecar.stdin, r#"{{"kind":"embed"}}"#).expect("write");
    writeln!(sidecar.stdin).expect("write");
    sidecar.stdin.flush().expect("flush");
    let alive = sidecar.exchange(&DiarizeRequest::embed(5, std::path::Path::new("/nope.wav")));
    assert_eq!(alive.err_tag(), Some(ERR_NO_MODELS));
    assert_eq!(alive.id, 5, "the answer is for the request that could be read");
}

/// The sidecar takes no model on argv. A caller that passes one is a version
/// skew with a stale staged binary, and guessing at what it meant is worse than
/// refusing — so it exits non-zero instead of coming up half-configured.
#[test]
fn an_argument_is_refused_rather_than_guessed_at() {
    let output = Command::new(diarize_binary())
        .arg("--model")
        .arg("/some/model.onnx")
        .stdin(Stdio::null())
        .output()
        .expect("the sidecar runs");
    assert!(!output.status.success(), "an unknown argument is not accepted");
    assert!(
        output.stdout.is_empty(),
        "a refusal must not put anything on the protocol stream: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unknown argument"),
        "the reason goes to stderr"
    );
}
