//! yap-1106-byom-cloud-fast-path — the cloud route against a mock streaming
//! server on 127.0.0.1 (no real key, no real network). One case per
//! acceptance line: stream OK, slow TTFT → local fallback, 429 → one retry,
//! 401 → no retry + local + settings flag, breaker open / half-open,
//! offline → no request, spend cap reached → local, and "Always on this Mac"
//! makes zero network calls.

mod support;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use support::{open_db, temp_dir};
use wilson_voice_lib::cloud_client::{CloudKeyClient, CLEANUP_SYSTEM};
use wilson_voice_lib::cloud_route::{
    budget_for, daily_cap, decide, key_needs_attention, run_routed, set_daily_cap, AssumeOnline,
    BreakerState, Budget, CircuitBreaker, CloudError, CloudMode, Decision, Fallback, FixedPath,
    Job, Ledger, OnlineProbe, PathStatus, Reason, Route, RouteContext, CLEANUP_MODEL,
};
use wilson_voice_lib::db::Database;
use wilson_voice_lib::http_client::build_client;
use wilson_voice_lib::summary_jobs::CancelToken;

/// One scripted answer: status line, extra headers, then body pieces each
/// written after its delay. No Content-Length: the body ends with the close.
#[derive(Clone)]
struct Scripted {
    status: &'static str,
    headers: Vec<String>,
    pieces: Vec<(Duration, String)>,
}

fn sse_ok(text_pieces: &[&str]) -> Scripted {
    let mut pieces = vec![(
        Duration::ZERO,
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"usage\":{\"input_tokens\":30,\"output_tokens\":1}}}\n\n".to_string(),
    )];
    for t in text_pieces {
        pieces.push((
            Duration::from_millis(20),
            format!(
                "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{t}\"}}}}\n\n"
            ),
        ));
    }
    pieces.push((
        Duration::from_millis(10),
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":7}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string(),
    ));
    Scripted {
        status: "200 OK",
        headers: vec!["Content-Type: text/event-stream".into()],
        pieces,
    }
}

fn status(status: &'static str, body: &str) -> Scripted {
    Scripted {
        status,
        headers: vec!["Content-Type: application/json".into()],
        pieces: vec![(Duration::ZERO, body.to_string())],
    }
}

/// Serve the script in order (one answer per connection); count requests.
fn mock(script: Vec<Scripted>) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        let mut script = script.into_iter();
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
            let Some(answer) = script.next() else {
                let _ =
                    s.write_all(b"HTTP/1.1 500 Internal Server Error\r\nConnection: close\r\n\r\n");
                continue;
            };
            let mut head = format!("HTTP/1.1 {}\r\nConnection: close\r\n", answer.status);
            for h in &answer.headers {
                head.push_str(h);
                head.push_str("\r\n");
            }
            head.push_str("\r\n");
            // The first piece's delay runs BEFORE the headers, so a slow
            // server is slow to the first byte, as a real one would be.
            if let Some((d, _)) = answer.pieces.first() {
                std::thread::sleep(*d);
            }
            let _ = s.write_all(head.as_bytes());
            let _ = s.flush();
            for (i, (d, body)) in answer.pieces.iter().enumerate() {
                if i > 0 {
                    std::thread::sleep(*d);
                }
                let _ = s.write_all(body.as_bytes());
                let _ = s.flush();
            }
        }
    });
    (port, seen)
}

const FAST: Budget = Budget {
    ttft: Duration::from_millis(400),
    total: Duration::from_secs(3),
};

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 6).unwrap()
}

struct Harness {
    dir: std::path::PathBuf,
    db: Database,
    breaker: Mutex<CircuitBreaker>,
    port: u16,
    seen: Arc<Mutex<Vec<String>>>,
    cloud_calls: Mutex<u32>,
}

impl Harness {
    fn new(tag: &str, script: Vec<Scripted>) -> Self {
        let (port, seen) = mock(script);
        let dir = temp_dir(tag);
        let db = open_db(&dir);
        Self {
            dir,
            db,
            breaker: Mutex::new(CircuitBreaker::new()),
            port,
            seen,
            cloud_calls: Mutex::new(0),
        }
    }

    fn ctx<'a>(
        &'a self,
        mode: CloudMode,
        probe: &'a dyn OnlineProbe,
        job: Job,
    ) -> RouteContext<'a> {
        RouteContext {
            mode,
            has_key: true,
            probe,
            breaker: &self.breaker,
            kv: &self.db,
            today: today(),
            job,
            model: CLEANUP_MODEL,
            budget: FAST,
            constrained_notes_allowed: false,
        }
    }

    /// The real client against the mock, routed; local = "local".
    fn run(&self, ctx: &RouteContext<'_>) -> wilson_voice_lib::cloud_route::Routed<String> {
        let client = CloudKeyClient::new(
            &format!("http://127.0.0.1:{}", self.port),
            "mock-key-never-real".into(),
            build_client(Duration::from_secs(10)).unwrap(),
        );
        let cancel = CancelToken::new();
        let mut cloud = |budget: Budget, _attempt: u32| -> Result<_, CloudError> {
            *self.cloud_calls.lock().unwrap() += 1;
            let mut sink = |_: &str| {};
            client.complete_streaming(
                CLEANUP_MODEL,
                Job::Polish,
                CLEANUP_SYSTEM,
                "um so the meeting is on monday no tuesday",
                128,
                budget,
                &cancel,
                &mut sink,
            )
        };
        run_routed(ctx, &mut cloud, &|t| Some(t.to_string()), &mut || {
            "local".to_string()
        })
    }

    fn requests(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

#[test]
fn stream_ok_is_the_cloud_route_and_the_ledger_counts_it() {
    let h = Harness::new(
        "cloud-ok",
        vec![sse_ok(&["The meeting is on ", "Tuesday."])],
    );
    let r = h.run(&h.ctx(CloudMode::CloudWhenOnline, &AssumeOnline, Job::Polish));
    assert_eq!(r.value, "The meeting is on Tuesday.");
    assert_eq!(
        (r.route, r.reason, r.fallback),
        (Route::Cloud, Reason::Online, Fallback::None)
    );
    assert_eq!((r.tokens_in, r.tokens_out), (30, 7));
    assert!(r.ttft_ms <= 400, "ttft_ms={}", r.ttft_ms);
    assert!(r.cost_usd > 0.0);
    assert_eq!(h.requests(), 1);
    let ledger = Ledger::load(&h.db, today());
    assert_eq!(ledger.day_calls, 1);
    assert_eq!(ledger.day_tokens_out, 7);
    assert_eq!(
        h.breaker.lock().unwrap().state(Instant::now()),
        BreakerState::Closed
    );
    let req = h.seen.lock().unwrap()[0].clone();
    assert!(req.contains("x-api-key: mock-key-never-real"));
    assert!(req.contains("anthropic-version: 2026-10-21"));
}

#[test]
fn slow_first_token_falls_back_to_local_and_keeps_the_take() {
    let mut slow = sse_ok(&["late"]);
    slow.pieces[0].0 = Duration::from_millis(1200); // past the 400 ms TTFT
    let h = Harness::new("cloud-slow", vec![slow]);
    let r = h.run(&h.ctx(CloudMode::CloudWhenOnline, &AssumeOnline, Job::Polish));
    assert_eq!(r.value, "local");
    assert_eq!(
        (r.route, r.reason, r.fallback),
        (Route::Local, Reason::Timeout, Fallback::Timeout)
    );
    assert_eq!(h.requests(), 1, "a timeout is never retried");
    assert_eq!(
        Ledger::load(&h.db, today()).day_calls,
        0,
        "nothing was billed"
    );
}

#[test]
fn a_429_is_retried_exactly_once_then_local() {
    let h = Harness::new(
        "cloud-429",
        vec![
            status("429 Too Many Requests", "{\"type\":\"error\"}"),
            status("429 Too Many Requests", "{\"type\":\"error\"}"),
            sse_ok(&["never reached"]),
        ],
    );
    let r = h.run(&h.ctx(CloudMode::CloudWhenOnline, &AssumeOnline, Job::Polish));
    assert_eq!(r.value, "local");
    assert_eq!(r.attempts, 2);
    assert_eq!(h.requests(), 2, "one retry, not two");
    assert_eq!(r.fallback, Fallback::Error);
    assert!(!key_needs_attention(&h.db), "a 429 is not a key problem");
}

#[test]
fn a_429_whose_retry_succeeds_is_still_the_cloud_route() {
    let h = Harness::new(
        "cloud-429-ok",
        vec![
            Scripted {
                status: "429 Too Many Requests",
                headers: vec![
                    "Retry-After: 1".into(),
                    "Content-Type: application/json".into(),
                ],
                pieces: vec![(Duration::ZERO, "{}".into())],
            },
            sse_ok(&["Second try."]),
        ],
    );
    let r = h.run(&h.ctx(CloudMode::CloudWhenOnline, &AssumeOnline, Job::Polish));
    assert_eq!(r.value, "Second try.");
    assert_eq!(r.route, Route::Cloud);
    assert_eq!(r.attempts, 2);
    assert_eq!(h.requests(), 2);
}

#[test]
fn a_401_is_not_retried_goes_local_and_flags_the_key() {
    let h = Harness::new(
        "cloud-401",
        vec![status(
            "401 Unauthorized",
            "{\"type\":\"error\",\"error\":{\"type\":\"authentication_error\"}}",
        )],
    );
    let r = h.run(&h.ctx(CloudMode::CloudWhenOnline, &AssumeOnline, Job::Polish));
    assert_eq!(r.value, "local");
    assert_eq!(r.attempts, 1);
    assert_eq!(h.requests(), 1, "no retry on an auth error");
    assert!(
        key_needs_attention(&h.db),
        "Settings shows 'Cloud key needs attention'"
    );
    // The next successful cloud call clears the flag.
}

#[test]
fn the_breaker_opens_after_three_failures_and_half_opens_with_one_probe() {
    let h = Harness::new(
        "cloud-breaker",
        vec![
            status("500 Internal Server Error", "{}"),
            status("500 Internal Server Error", "{}"),
            status("503 Service Unavailable", "{}"),
            status("503 Service Unavailable", "{}"),
            status("503 Service Unavailable", "{}"),
            status("503 Service Unavailable", "{}"),
            sse_ok(&["probe ok"]),
        ],
    );
    let ctx = h.ctx(CloudMode::CloudWhenOnline, &AssumeOnline, Job::Polish);
    // Three failed jobs (each with its one retry = 6 requests).
    for _ in 0..3 {
        let r = h.run(&ctx);
        assert_eq!(r.value, "local");
    }
    assert_eq!(h.requests(), 6);
    assert!(h.breaker.lock().unwrap().is_open(Instant::now()));
    // Open: no request at all.
    let r = h.run(&ctx);
    assert_eq!((r.route, r.reason), (Route::Local, Reason::Breaker));
    assert_eq!(h.requests(), 6, "an open breaker makes no request");
    assert_eq!(
        decide(&ctx, Instant::now()),
        Decision::local(Reason::Breaker)
    );
    // Half-open after the window: exactly one probe, and it succeeds.
    {
        let mut b = h.breaker.lock().unwrap();
        *b = CircuitBreaker::new();
        let long_ago = Instant::now() - Duration::from_secs(301);
        b.record_failure(long_ago);
        b.record_failure(long_ago);
        b.record_failure(long_ago);
        assert_eq!(b.state(Instant::now()), BreakerState::HalfOpen);
    }
    let r = h.run(&ctx);
    assert_eq!(r.value, "probe ok");
    assert_eq!(r.route, Route::Cloud);
    assert_eq!(h.requests(), 7);
    assert_eq!(
        h.breaker.lock().unwrap().state(Instant::now()),
        BreakerState::Closed
    );
}

#[test]
fn offline_makes_no_request() {
    let h = Harness::new("cloud-offline", vec![sse_ok(&["unreachable"])]);
    let off = FixedPath(PathStatus::Unsatisfied);
    let r = h.run(&h.ctx(CloudMode::CloudWhenOnline, &off, Job::Polish));
    assert_eq!(r.value, "local");
    assert_eq!((r.route, r.reason), (Route::Local, Reason::Offline));
    assert_eq!(h.requests(), 0);
    assert_eq!(*h.cloud_calls.lock().unwrap(), 0);
    // A constrained path still allows cleanup, but holds notes back by default.
    let slow = FixedPath(PathStatus::Constrained);
    assert_eq!(
        decide(
            &h.ctx(CloudMode::CloudWhenOnline, &slow, Job::Notes),
            Instant::now()
        ),
        Decision::local(Reason::Constrained)
    );
    assert_eq!(
        decide(
            &h.ctx(CloudMode::CloudWhenOnline, &slow, Job::Polish),
            Instant::now()
        ),
        Decision::cloud()
    );
}

#[test]
fn the_daily_cap_routes_locally_and_holds_across_a_restart() {
    let h = Harness::new("cloud-cap", vec![sse_ok(&["one"]), sse_ok(&["two"])]);
    set_daily_cap(&h.db, 0.25).unwrap();
    assert_eq!(daily_cap(&h.db), 0.25);
    // Spend past the cap with a priced call written straight to the ledger.
    let mut l = Ledger::load(&h.db, today());
    l.record(today(), CLEANUP_MODEL, 0, 30_000); // $0.30
    l.save(&h.db).unwrap();
    let r = h.run(&h.ctx(CloudMode::CloudWhenOnline, &AssumeOnline, Job::Polish));
    assert_eq!((r.route, r.reason), (Route::Local, Reason::Cap));
    assert_eq!(h.requests(), 0, "over the cap, no request");
    // "Restart": a fresh Database handle over the same file sees the same cap.
    drop(r);
    let db2 = open_db(&h.dir);
    let breaker2 = Mutex::new(CircuitBreaker::new());
    let ctx2 = RouteContext {
        mode: CloudMode::CloudWhenOnline,
        has_key: true,
        probe: &AssumeOnline,
        breaker: &breaker2,
        kv: &db2,
        today: today(),
        job: Job::Polish,
        model: CLEANUP_MODEL,
        budget: FAST,
        constrained_notes_allowed: false,
    };
    assert_eq!(decide(&ctx2, Instant::now()), Decision::local(Reason::Cap));
    // Tomorrow it is back.
    let ctx3 = RouteContext {
        today: today().succ_opt().unwrap(),
        ..ctx2
    };
    assert_eq!(decide(&ctx3, Instant::now()), Decision::cloud());
    assert_eq!(budget_for(Job::Notes).total, Duration::from_secs(90));
}

#[test]
fn always_on_this_mac_makes_zero_network_calls() {
    let h = Harness::new("cloud-always-local", vec![sse_ok(&["never"])]);
    for job in [Job::Polish, Job::Format, Job::Notes] {
        let r = h.run(&h.ctx(CloudMode::AlwaysLocal, &AssumeOnline, job));
        assert_eq!(r.value, "local");
        assert_eq!((r.route, r.reason), (Route::Local, Reason::User));
    }
    // Give a stray connection every chance to show up.
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(h.requests(), 0, "Always on this Mac reached the network");
    assert_eq!(
        *h.cloud_calls.lock().unwrap(),
        0,
        "the cloud closure was never even called"
    );
    assert_eq!(
        CloudMode::load(&h.db),
        CloudMode::AlwaysLocal,
        "the default for a new user"
    );
}
