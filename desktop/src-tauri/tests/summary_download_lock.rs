//! #250 round 4 — the model download, against a local HTTP stub.
//!
//! R2-B4: Settings' Download and a notes job fetching the same model at once
//!        → ONE writer, ONE request, ONE sha256 pass; the waiter sees the
//!        writer's progress and gets the verified file.
//! A3:    an HTTP 503 is "Download failed (HTTP 503)", not offline.
//! A4:    a cancel interrupts the backoff sleep and a stalled chunk ≤ 500 ms.
//!
//! Its own test binary: `models::sha_passes()` is process-wide.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use wilson_voice_lib::models::{self, download_file_cancellable, DOWNLOAD_CANCELLED};
use wilson_voice_lib::summary_jobs::{classify_download_error, NotesError};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("yap-f250d-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// yap-1107-timing-tests-deterministic — wait for an EVENT, not for a sleep.
/// `deadline` is a hang guard only, never a measurement.
fn wait_until(what: &str, deadline: Duration, mut cond: impl FnMut() -> bool) {
    let start = Instant::now();
    while !cond() {
        assert!(
            start.elapsed() < deadline,
            "timed out after {deadline:?} waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Raise `stop` after `after`, and record WHEN it was actually raised.
///
/// The cancel-latency assertions used to measure `t0.elapsed() - 300 ms`,
/// assuming the setter thread woke on time. On a loaded runner it wakes late,
/// and that lateness was charged to the download's cancel path: a shared
/// deadline between two independent threads. Measuring from the recorded
/// instant charges the download only for its own latency.
fn cancel_after(stop: Arc<AtomicBool>, after: Duration) -> Arc<std::sync::Mutex<Option<Instant>>> {
    let raised_at = Arc::new(std::sync::Mutex::new(None));
    let r2 = Arc::clone(&raised_at);
    std::thread::spawn(move || {
        std::thread::sleep(after);
        // Stamp first, then raise: the stamp can only be EARLIER than the
        // moment the download can observe the flag, never later.
        *r2.lock().unwrap() = Some(Instant::now());
        stop.store(true, Ordering::SeqCst);
    });
    raised_at
}

/// Latency from the recorded raise to `returned`.
fn since_raise(raised_at: &std::sync::Mutex<Option<Instant>>, returned: Instant) -> Duration {
    let raised = raised_at
        .lock()
        .unwrap()
        .expect("the download returned DOWNLOAD_CANCELLED, so the flag was raised");
    returned.saturating_duration_since(raised)
}

/// Serve every connection with `respond`, counting connections.
fn serve(respond: fn(std::net::TcpStream)) -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            h.fetch_add(1, Ordering::SeqCst);
            std::thread::spawn(move || {
                let mut stream = stream;
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                respond(stream);
            });
        }
    });
    (port, hits)
}

const CHUNK: usize = 16 * 1024;
const CHUNKS: usize = 24;

fn body() -> Vec<u8> {
    (0..CHUNK * CHUNKS).map(|i| (i % 251) as u8).collect()
}

fn slow_ok(mut s: std::net::TcpStream) {
    let body = body();
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
        body.len()
    );
    if s.write_all(head.as_bytes()).is_err() {
        return;
    }
    for c in body.chunks(CHUNK) {
        if s.write_all(c).is_err() || s.flush().is_err() {
            return;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

#[test]
fn settings_and_a_notes_job_share_one_download() {
    let (port, hits) = serve(slow_ok);
    let dir = temp_dir("lock");
    let dest = dir.join("model.gguf");
    let reference = dir.join("reference.bin");
    std::fs::write(&reference, body()).unwrap();
    let sha = models::sha256_hex(&reference).unwrap();
    let total = body().len() as u64;
    let url = format!("http://127.0.0.1:{port}/model.gguf");
    let passes_before = models::sha_passes();

    let run = |seen: Arc<AtomicU64>| {
        let (url, dest, sha) = (url.clone(), dest.clone(), sha.clone());
        std::thread::spawn(move || {
            tauri::async_runtime::block_on(download_file_cancellable(
                &[url],
                &dest,
                total,
                &sha,
                move |done, _| {
                    seen.fetch_max(done, Ordering::SeqCst);
                },
                &|| false,
            ))
        })
    };
    let job_seen = Arc::new(AtomicU64::new(0));
    let settings_seen = Arc::new(AtomicU64::new(0));
    let job = run(job_seen.clone());
    // Settings is pressed while the notes job is mid-download. Wait for the job
    // to TAKE the slot rather than sleeping 60 ms and hoping its thread ran
    // (yap-1107-timing-tests-deterministic). The stub paces the body over
    // 24 x 15 ms, so the slot stays held well past this point.
    wait_until(
        "the notes job to take the download slot",
        Duration::from_secs(30),
        || models::download_in_progress(&dest).is_some(),
    );
    assert!(
        models::download_in_progress(&dest).is_some(),
        "the job holds the slot"
    );
    let settings = run(settings_seen.clone());
    assert_eq!(job.join().unwrap().unwrap(), dest);
    assert_eq!(settings.join().unwrap().unwrap(), dest);

    assert_eq!(hits.load(Ordering::SeqCst), 1, "one writer, one request");
    assert_eq!(models::sha_passes() - passes_before, 1, "one checksum pass");
    assert_eq!(
        settings_seen.load(Ordering::SeqCst),
        total,
        "the waiter saw the progress"
    );
    assert_eq!(std::fs::read(&dest).unwrap(), body());
    assert!(models::download_in_progress(&dest).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

fn always_503(mut s: std::net::TcpStream) {
    let _ = s.write_all(
        b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
}

#[test]
fn a_server_error_is_named_and_retries_are_capped() {
    let (port, hits) = serve(always_503);
    let dir = temp_dir("503");
    let t0 = Instant::now();
    let err = tauri::async_runtime::block_on(download_file_cancellable(
        &[format!("http://127.0.0.1:{port}/m.gguf")],
        &dir.join("m.gguf"),
        10,
        "00",
        |_, _| {},
        &|| false,
    ))
    .unwrap_err();
    assert!(err.contains("HTTP 503"), "{err}");
    assert_eq!(hits.load(Ordering::SeqCst), 3, "capped at 3 attempts");
    assert!(
        t0.elapsed() < Duration::from_secs(6),
        "1 s + 2 s backoff, no storm"
    );
    let reason = classify_download_error(&err);
    assert_eq!(reason, NotesError::Server(503));
    assert!(reason.plain().contains("HTTP 503"), "{}", reason.plain());
    // A TLS verify failure is a network problem, not a bad download.
    assert_ne!(
        classify_download_error("request failed: certificate verify failed"),
        NotesError::Checksum
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cancel_interrupts_the_backoff_sleep() {
    let (port, _) = serve(always_503);
    let dir = temp_dir("backoff");
    let stop = Arc::new(AtomicBool::new(false));
    let raised_at = cancel_after(stop.clone(), Duration::from_millis(300));
    let cancel = move || stop.load(Ordering::SeqCst);
    let err = tauri::async_runtime::block_on(download_file_cancellable(
        &[format!("http://127.0.0.1:{port}/m.gguf")],
        &dir.join("m.gguf"),
        10,
        "00",
        |_, _| {},
        &cancel,
    ))
    .unwrap_err();
    assert_eq!(err, DOWNLOAD_CANCELLED);
    let after_cancel = since_raise(&raised_at, Instant::now());
    assert!(
        after_cancel <= Duration::from_millis(500),
        "took {after_cancel:?} after the cancel"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn stalls(mut s: std::net::TcpStream) {
    let _ =
        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\nabc");
    let _ = s.flush();
    std::thread::sleep(Duration::from_secs(10));
}

#[test]
fn cancel_interrupts_a_stalled_chunk() {
    let (port, _) = serve(stalls);
    let dir = temp_dir("stall");
    let stop = Arc::new(AtomicBool::new(false));
    let raised_at = cancel_after(stop.clone(), Duration::from_millis(300));
    let cancel = move || stop.load(Ordering::SeqCst);
    let err = tauri::async_runtime::block_on(download_file_cancellable(
        &[format!("http://127.0.0.1:{port}/m.gguf")],
        &dir.join("m.gguf"),
        1_000_000,
        "00",
        |_, _| {},
        &cancel,
    ))
    .unwrap_err();
    assert_eq!(err, DOWNLOAD_CANCELLED);
    let after_cancel = since_raise(&raised_at, Instant::now());
    assert!(
        after_cancel <= Duration::from_millis(500),
        "took {after_cancel:?} after the cancel"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
