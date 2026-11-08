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

/// How long a stub or a test waits on a [`Gate`] before giving up. A hang
/// guard for a broken build, never a healthy-path deadline.
const GATE_HANG_GUARD: Duration = Duration::from_secs(60);

/// yap-1108-timing-src-ordering-resume — a one-shot gate between a stub
/// server and the test, so the stub holds its response until the event the
/// test means has happened, instead of pacing its bytes and hoping the other
/// thread keeps up. Once opened it stays open.
#[derive(Default)]
struct Gate {
    open: std::sync::Mutex<bool>,
    opened: std::sync::Condvar,
}

impl Gate {
    fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.opened.notify_all();
    }

    /// Block until the gate opens. Stub threads call this, so a broken build
    /// logs and goes on (a panic on a server thread would fail nothing).
    fn wait_open(&self, what: &str) -> bool {
        let until = Instant::now() + GATE_HANG_GUARD;
        let mut open = self.open.lock().unwrap();
        while !*open {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                eprintln!("gave up after {GATE_HANG_GUARD:?} waiting for {what}");
                return false;
            }
            open = self.opened.wait_timeout(open, left).unwrap().0;
        }
        true
    }
}

/// A cancel the test raises, with WHEN it was actually raised.
#[derive(Default)]
struct RaisedCancel {
    at: std::sync::Mutex<Option<Instant>>,
    /// Opened right after the flag is raised: what a held stub waits on.
    raised: Gate,
}

/// Once `armed` opens, wait `delay`, then raise `stop` and record WHEN it was
/// actually raised.
///
/// The cancel-latency assertions used to measure `t0.elapsed() - 300 ms`,
/// assuming the setter thread woke on time. On a loaded runner it wakes late,
/// and that lateness was charged to the download's cancel path: a shared
/// deadline between two independent threads. Measuring from the recorded
/// instant charges the download only for its own latency.
///
/// yap-1108-timing-src-ordering-resume: the delay now counts from the stub's
/// own event (`armed`: the first 503 served, the stalled head written) rather
/// than from the test's start, and every stub holds its NEXT step until the
/// cancel is raised. So a late setter can only move WHERE the cancel lands
/// inside the held window; it can never let the download run out of attempts
/// or finish first. The `delay` is not an ordering sleep: it is what places the
/// cancel MID-backoff / MID-stall on an idle machine, so a build whose sleep or
/// chunk read ignores the cancel still fails the latency bound.
fn cancel_after(
    cancel: &Arc<RaisedCancel>,
    stop: Arc<AtomicBool>,
    armed: Arc<Gate>,
    delay: Duration,
) {
    let c2 = Arc::clone(cancel);
    std::thread::spawn(move || {
        // A stub that never arms is a broken build; raise anyway so the
        // download returns and the assertions name the failure.
        armed.wait_open("the stub to reach the point the cancel lands after");
        std::thread::sleep(delay);
        // Stamp first, then raise: the stamp can only be EARLIER than the
        // moment the download can observe the flag, never later.
        *c2.at.lock().unwrap() = Some(Instant::now());
        stop.store(true, Ordering::SeqCst);
        c2.raised.open();
    });
}

/// Latency from the recorded raise to `returned`.
fn since_raise(cancel: &RaisedCancel, returned: Instant) -> Duration {
    let raised = cancel
        .at
        .lock()
        .unwrap()
        .expect("the download returned DOWNLOAD_CANCELLED, so the flag was raised");
    returned.saturating_duration_since(raised)
}

/// Serve every connection with `respond`, counting connections.
fn serve<R>(respond: R) -> (u16, Arc<AtomicUsize>)
where
    R: Fn(std::net::TcpStream) + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    let respond = Arc::new(respond);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            h.fetch_add(1, Ordering::SeqCst);
            let respond = Arc::clone(&respond);
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

/// The body chunk the slow stub parks after, until the test opens its gate.
const PARK_AFTER_CHUNKS: usize = 8;

/// A paced 200 that PARKS after [`PARK_AFTER_CHUNKS`] chunks until `resume`
/// opens (yap-1108-timing-src-ordering-resume). While it is parked the writer
/// holds the slot mid-body with real, partial progress, so "Settings is pressed
/// while the notes job is mid-download" is a state the test holds still rather
/// than a race against 24 x 15 ms of pacing.
fn slow_ok(resume: Arc<Gate>) -> impl Fn(std::net::TcpStream) + Send + Sync + 'static {
    move |mut s: std::net::TcpStream| {
        let body = body();
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
            body.len()
        );
        if s.write_all(head.as_bytes()).is_err() {
            return;
        }
        for (i, c) in body.chunks(CHUNK).enumerate() {
            if i == PARK_AFTER_CHUNKS {
                resume.wait_open("the test to see Settings waiting on the slot");
            }
            if s.write_all(c).is_err() || s.flush().is_err() {
                return;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }
}

#[test]
fn settings_and_a_notes_job_share_one_download() {
    let resume = Arc::new(Gate::default());
    let (port, hits) = serve(slow_ok(resume.clone()));
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
    // (yap-1107-timing-tests-deterministic). The stub parks mid-body until the
    // gate below opens, so the slot stays held past this point by construction.
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
    // yap-1108-timing-src-ordering-resume: Settings is WAITING on the job's slot
    // before the job may finish. A waiter mirrors the writer's progress; with
    // the stub parked mid-body that progress is non-zero and short of the total,
    // which nothing but the wait loop can report (the job cannot finish, and so
    // cannot free the slot, while the stub is parked).
    wait_until(
        "Settings to wait on the job's slot and mirror its progress",
        GATE_HANG_GUARD,
        || settings_seen.load(Ordering::SeqCst) > 0,
    );
    let mirrored = settings_seen.load(Ordering::SeqCst);
    assert!(
        mirrored < total,
        "Settings saw {mirrored}/{total} while the job was parked mid-body: it did not wait on the job"
    );
    assert!(
        models::download_in_progress(&dest).is_some(),
        "the job still holds the slot while Settings waits"
    );
    resume.open();
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

/// [`always_503`], except that it opens `first_served` once the first 503 is
/// written (the download is now in, or entering, its 1 s backoff) and holds
/// every LATER attempt until `cancel` has been raised
/// (yap-1108-timing-src-ordering-resume). A late cancel can then only land on a
/// held retry, never after the download has run out of attempts.
fn always_503_holding_retries(
    first_served: Arc<Gate>,
    cancel: Arc<RaisedCancel>,
) -> impl Fn(std::net::TcpStream) + Send + Sync + 'static {
    let connections = AtomicUsize::new(0);
    move |s: std::net::TcpStream| {
        if connections.fetch_add(1, Ordering::SeqCst) == 0 {
            always_503(s);
            first_served.open();
            return;
        }
        cancel.raised.wait_open("the test to raise its cancel");
        always_503(s);
    }
}

#[test]
fn cancel_interrupts_the_backoff_sleep() {
    let first_served = Arc::new(Gate::default());
    let raised_at = Arc::new(RaisedCancel::default());
    let (port, _) = serve(always_503_holding_retries(
        first_served.clone(),
        raised_at.clone(),
    ));
    let dir = temp_dir("backoff");
    let stop = Arc::new(AtomicBool::new(false));
    cancel_after(
        &raised_at,
        stop.clone(),
        first_served,
        Duration::from_millis(300),
    );
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

/// A 200 that sends three bytes of a megabyte and then stalls until the CLIENT
/// hangs up (yap-1108-timing-src-ordering-resume; it used to stall a fixed
/// 10 s). It opens `stalled` once the head is written. A download that honours
/// the cancel drops the connection; one that ignores it waits out the 60 s hang
/// guard and fails the latency bound. A late cancel can no longer race the
/// stub's own close into a retry.
fn stalls(stalled: Arc<Gate>) -> impl Fn(std::net::TcpStream) + Send + Sync + 'static {
    move |mut s: std::net::TcpStream| {
        let _ = s.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\nabc",
        );
        let _ = s.flush();
        stalled.open();
        let _ = s.set_read_timeout(Some(GATE_HANG_GUARD));
        let mut buf = [0u8; 1024];
        while let Ok(n) = s.read(&mut buf) {
            if n == 0 {
                break;
            }
        }
    }
}

#[test]
fn cancel_interrupts_a_stalled_chunk() {
    let stalled = Arc::new(Gate::default());
    let (port, _) = serve(stalls(stalled.clone()));
    let dir = temp_dir("stall");
    let stop = Arc::new(AtomicBool::new(false));
    let raised_at = Arc::new(RaisedCancel::default());
    cancel_after(
        &raised_at,
        stop.clone(),
        stalled,
        Duration::from_millis(300),
    );
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
