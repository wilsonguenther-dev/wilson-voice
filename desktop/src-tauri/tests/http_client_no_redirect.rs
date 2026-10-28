//! yap-byom-0 — the outbound allowlist test: NOTHING follows a redirect.
//!
//! A mock on 127.0.0.1:A answers 307 → 127.0.0.1:B. Port B must receive ZERO
//! connections, and the call must end "redirected to <host> — not followed".
//!
//! Two controls keep the zero honest: B demonstrably counts a connection made
//! to it directly, and reqwest's DEFAULT client (`limited(10)`) DOES reach B —
//! which is exactly what `http_client.rs`'s `Policy::none()` exists to stop.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use wilson_voice_lib::http_client::{build_client, post_json_blocking, HttpError};
use wilson_voice_lib::model_providers::{
    ChatProvider, OpenAiCompatibleProvider, ProviderConfig, ProviderKind,
};
use wilson_voice_lib::summary_jobs::CancelToken;

/// Port B: counts every connection, answers 200 `{}`.
fn counter() -> (u16, Arc<AtomicUsize>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { break };
            h.fetch_add(1, Ordering::SeqCst);
            s.set_read_timeout(Some(Duration::from_millis(300))).ok();
            let mut buf = [0u8; 8192];
            let _ = s.read(&mut buf);
            let _ =
                s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
        }
    });
    (port, hits)
}

/// Port A: answers every request 307 → `location`.
fn redirector(location: String) -> (u16, Arc<AtomicUsize>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { break };
            h.fetch_add(1, Ordering::SeqCst);
            s.set_read_timeout(Some(Duration::from_millis(300))).ok();
            let mut buf = [0u8; 8192];
            let _ = s.read(&mut buf);
            let resp = format!(
                "HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });
    (port, hits)
}

#[test]
fn a_307_to_a_second_port_is_never_followed() {
    let (b, b_hits) = counter();
    let (a, a_hits) = redirector(format!("http://127.0.0.1:{b}/v1/chat/completions"));
    let client = build_client(Duration::from_secs(5)).unwrap();
    let out = post_json_blocking(
        &client,
        &format!("http://127.0.0.1:{a}/v1/chat/completions"),
        &[
            ("Authorization", "Bearer sk-should-not-travel"),
            ("x-api-key", "k"),
        ],
        &serde_json::json!({"model": "m", "messages": []}),
    );
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        a_hits.load(Ordering::SeqCst),
        1,
        "the first host was called"
    );
    assert_eq!(
        b_hits.load(Ordering::SeqCst),
        0,
        "the redirect target got a connection"
    );
    let err = out.expect_err("a 3xx is an error");
    assert_eq!(
        err,
        HttpError::Redirected {
            host: "127.0.0.1".into()
        }
    );
    assert_eq!(err.plain(), "redirected to 127.0.0.1 — not followed");

    // Control: B does count a connection made to it.
    drop(TcpStream::connect(("127.0.0.1", b)).unwrap());
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(b_hits.load(Ordering::SeqCst), 1, "the counter works");
}

#[test]
fn the_provider_path_reports_the_redirect_in_plain_words() {
    let (b, b_hits) = counter();
    let (a, _) = redirector(format!("http://127.0.0.1:{b}/v1/chat/completions"));
    let cfg = ProviderConfig {
        id: "r".into(),
        kind: ProviderKind::OpenAiCompatible,
        label: "R".into(),
        base_url: format!("http://127.0.0.1:{a}/v1"),
        model: "m".into(),
        context_tokens: 4096,
        timeout_s: 5,
    };
    let p = OpenAiCompatibleProvider::new(
        cfg,
        Some("sk-should-not-travel".into()),
        build_client(Duration::from_secs(5)).unwrap(),
    );
    let err = p.complete("s", "u", 10, &CancelToken::new()).unwrap_err();
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(err.plain(), "redirected to 127.0.0.1 — not followed");
    assert_eq!(b_hits.load(Ordering::SeqCst), 0);
}

/// Control: the DEFAULT reqwest policy follows the 307 to B. This is the
/// behaviour `Policy::none()` removes; if it ever stops reaching B, the zero in
/// the tests above would prove nothing.
#[test]
fn control_the_default_policy_would_have_followed() {
    let (b, b_hits) = counter();
    let (a, _) = redirector(format!("http://127.0.0.1:{b}/v1/chat/completions"));
    let default = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{a}/v1/chat/completions");
    let status = tauri::async_runtime::block_on(async move {
        default
            .post(url)
            .json(&serde_json::json!({"model": "m"}))
            .send()
            .await
            .map(|r| r.status().as_u16())
    })
    .expect("default client completes");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(status, 200, "the default client ended at B");
    assert_eq!(
        b_hits.load(Ordering::SeqCst),
        1,
        "the default client followed to B"
    );
}
