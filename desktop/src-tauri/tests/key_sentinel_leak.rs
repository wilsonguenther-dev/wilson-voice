//! yap-byom-0 — a saved API key never leaks.
//!
//! The key "sk-YAPSENTINEL-…" is saved through the fake Keychain, then Test
//! connection runs (the same `run_test` body the `test_model_provider` command
//! runs) against a 127.0.0.1 mock. The sentinel must be ABSENT from: every log
//! record, every `settings_kv` row, every emitted event payload, and the
//! command's return value — which carries only the last four characters.
//!
//! Non-vacuous: the mock asserts it RECEIVED the key in `Authorization`, so the
//! key really travelled the code path whose outputs are swept. The second case
//! is the classic leak — a 401 whose body echoes the submitted key — and the
//! error sentence must still not carry it.

mod support;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use support::{open_db, temp_dir};
use wilson_voice_lib::http_client::ClientCache;
use wilson_voice_lib::keychain::{MemoryKeychain, PRODUCTION_SERVICE};
use wilson_voice_lib::model_providers::{
    run_test, upsert_provider, BundledProvider, ChatProvider, ProviderConfig, ProviderKind,
    TEST_EVENT,
};
use wilson_voice_lib::polish::{PolishClient, PolishError};
use wilson_voice_lib::polish_protocol::PolishRequest;

const SENTINEL: &str = "sk-YAPSENTINEL-0123456789abcdefWXYZ";

static LOGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Capture;
impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, r: &log::Record<'_>) {
        LOGS.lock()
            .unwrap()
            .push(format!("{} {} {}", r.level(), r.target(), r.args()));
    }
    fn flush(&self) {}
}

fn install_logger() {
    let _ = log::set_boxed_logger(Box::new(Capture));
    log::set_max_level(log::LevelFilter::Trace);
}

/// A one-shot-per-connection HTTP mock that answers every request with
/// `status` + `body` and records each raw request.
fn mock(status: &'static str, body: String) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { break };
            s.set_read_timeout(Some(Duration::from_secs(5))).ok();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = s.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let len = text[..end]
                        .lines()
                        .find_map(|l| {
                            let l = l.to_ascii_lowercase();
                            l.strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if buf.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            log.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf).to_string());
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });
    (port, seen)
}

struct NoModel;
impl PolishClient for NoModel {
    fn rewrite(&self, _: &PolishRequest) -> Result<String, PolishError> {
        Err(PolishError::Unavailable)
    }
}

fn run_case(tag: &str, status: &'static str, body: String) -> (serde_json::Value, Vec<String>) {
    install_logger();
    let (port, seen) = mock(status, body);
    let dir = temp_dir(tag);
    let db = open_db(&dir);
    let keys = MemoryKeychain::new(PRODUCTION_SERVICE);
    let cfg = ProviderConfig {
        id: "mock".into(),
        kind: ProviderKind::OpenAiCompatible,
        label: "Mock".into(),
        base_url: format!("http://127.0.0.1:{port}/v1"),
        model: "mock-model".into(),
        context_tokens: 4096,
        timeout_s: 10,
    };
    upsert_provider(&db, &keys, cfg, Some(SENTINEL)).expect("save");

    let mut events: Vec<String> = Vec::new();
    let mut emit = |name: &str, payload: serde_json::Value| {
        events.push(format!("{name} {payload}"));
    };
    let bundled = |d: Duration| -> Box<dyn ChatProvider> {
        Box::new(BundledProvider::<NoModel>::new(None, d))
    };
    let result =
        run_test(&db, &keys, &ClientCache::new(), "mock", &bundled, &mut emit).expect("ran");
    let returned = serde_json::to_value(&result).unwrap();

    // The key really went to the mock (the sweep below is not vacuous).
    let reqs = seen.lock().unwrap().clone();
    assert_eq!(reqs.len(), 1, "exactly one request reached the mock");
    assert!(
        reqs[0].contains(&format!("Bearer {SENTINEL}")),
        "the mock never received the key"
    );
    assert!(
        reqs[0].contains("\"max_tokens\":10"),
        "Test connection is a 10-token completion"
    );

    // Return value: last 4 only.
    let ret = returned.to_string();
    assert!(!ret.contains("YAPSENTINEL"), "return value leaks: {ret}");
    assert_eq!(returned["keyHint"], "WXYZ");

    // Events.
    assert_eq!(events.len(), 2, "{events:?}");
    assert!(events.iter().all(|e| e.starts_with(TEST_EVENT)));
    for e in &events {
        assert!(!e.contains("YAPSENTINEL"), "event leaks: {e}");
    }

    // settings_kv.
    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).unwrap();
    let mut st = conn.prepare("SELECT key, value FROM settings_kv").unwrap();
    let rows: Vec<(String, String)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(!rows.is_empty());
    for (k, v) in &rows {
        assert!(!v.contains("YAPSENTINEL"), "settings_kv {k} leaks");
    }

    // Logs.
    let logs = LOGS.lock().unwrap().clone();
    assert!(
        logs.iter().any(|l| l.contains("model_providers test")),
        "the test path logged nothing: {logs:?}"
    );
    for l in &logs {
        assert!(!l.contains("YAPSENTINEL"), "log leaks: {l}");
    }
    (returned, logs)
}

#[test]
fn a_successful_test_never_leaks_the_key() {
    let (ret, _) = run_case(
        "sentinel-ok",
        "200 OK",
        r#"{"choices":[{"message":{"role":"assistant","content":"OK"}}]}"#.to_string(),
    );
    assert_eq!(ret["ok"], true);
    assert_eq!(ret["reply"], "OK");
}

#[test]
fn a_401_that_echoes_the_key_never_leaks_it() {
    let (ret, _) = run_case(
        "sentinel-401",
        "401 Unauthorized",
        format!(r#"{{"error":{{"message":"Incorrect API key provided: {SENTINEL}"}}}}"#),
    );
    assert_eq!(ret["ok"], false);
    assert_eq!(ret["error"], "the key was rejected (401)");
}
