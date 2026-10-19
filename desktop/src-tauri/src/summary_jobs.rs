//! #250 round 3 — the notes worker: ONE thread that fetches the notes model and
//! writes meeting notes, off the transcription worker.
//!
//! Why its own worker (review B3): the summary used to run inline on the single
//! FIFO transcription worker, so a first meeting's 1.1 GB model download plus
//! its summary held up every meeting queued behind it, and the launch-time
//! resume thread could summarize the SAME meeting at the same moment — two
//! writers on the model's `.partial`, two 1.1 GB sidecars. Here:
//!
//! * every trigger (the live "transcript landed" hook, the launch resume, the
//!   card's Retry, the opt-in backlog) goes through [`SummaryJobs::submit`],
//!   which is a per-meeting job lock: a meeting that is already queued or
//!   running is not queued again, so one meeting summarizes once;
//! * one worker thread runs the jobs in order, so at most one sidecar is alive
//!   and the model download has one writer;
//! * `submit` never blocks — the transcription worker hands the meeting over
//!   and goes straight on to the next queued meeting.
//!
//! Every job ends in a state the card can show (review B1): `notes_ready`,
//! `notes_failed` with a plain-words reason, or `notes_cancelled` (review B2).
//! A cancel is per job ([`CancelToken`]), never a process-global flag, so a
//! cancel sent during the download is not wiped when the summary starts.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One job's cancel flag. Cloned into the download loop and the summary
/// client; nothing else can clear it.
#[derive(Clone, Default, Debug)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Where a meeting's notes are. Spelled on the wire as the `state` of a
/// `meeting:progress` event (see [`NotesPhase::as_str`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum NotesPhase {
    Queued,
    PreparingNotes,
    Summarizing,
    Ready,
    Failed,
    Cancelled,
}

impl NotesPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "notes_queued",
            Self::PreparingNotes => "preparing_notes",
            Self::Summarizing => "summarizing",
            Self::Ready => "notes_ready",
            Self::Failed => "notes_failed",
            Self::Cancelled => "notes_cancelled",
        }
    }
    /// Still going — the card offers Cancel.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::PreparingNotes | Self::Summarizing)
    }
}

/// What the card shows for one meeting's notes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesStatus {
    pub id: String,
    #[serde(serialize_with = "phase_str")]
    pub phase: NotesPhase,
    pub percent: u8,
    /// Plain words, only on `notes_failed`.
    pub reason: Option<String>,
}

fn phase_str<S: serde::Serializer>(p: &NotesPhase, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(p.as_str())
}

/// Why a notes job did not finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotesError {
    /// The model could not be fetched: no network, a stalled or refused request.
    Offline,
    /// Not enough free space (the free-space check's own plain message).
    DiskFull(String),
    /// The download finished but did not match its sha256.
    Checksum,
    /// A3 — the server answered with an HTTP error (403, 404, 5xx) while the
    /// Mac was online: not "offline", and not the checksum copy.
    Server(u16),
    /// The notes model is on disk but its sidecar would not start.
    ModelStart,
    /// The summary itself failed; plain words.
    Summary(String),
    Cancelled,
}

/// The copy for offline, shared with the UI's test so the two cannot drift.
pub const OFFLINE_REASON: &str =
    "Yap couldn't reach the internet to get the notes model. Try again when online.";

impl NotesError {
    pub fn plain(&self) -> String {
        match self {
            Self::Offline => OFFLINE_REASON.to_string(),
            Self::DiskFull(msg) => msg.clone(),
            Self::Checksum => {
                "the notes model didn't download correctly. Retry to fetch it again.".to_string()
            }
            Self::Server(code) => {
                format!("Download failed (HTTP {code}). Retry in a little while.")
            }
            Self::ModelStart => "the notes model couldn't start on this Mac.".to_string(),
            Self::Summary(msg) => msg.clone(),
            Self::Cancelled => "cancelled".to_string(),
        }
    }
}

/// A1 — the reason a job that panicked shows: "Notes failed — internal error".
pub const INTERNAL_ERROR_REASON: &str = "internal error";

/// Sort a `models::download_file` error into the reason a person can act on.
pub fn classify_download_error(msg: &str) -> NotesError {
    let lower = msg.to_lowercase();
    if lower.contains(crate::models::DOWNLOAD_CANCELLED) {
        NotesError::Cancelled
    } else if lower.contains("free space")
        || lower.contains("no space")
        || lower.contains("disk full")
        || lower.contains("os error 28")
    {
        NotesError::DiskFull(if lower.contains("free space") {
            msg.to_string()
        } else {
            "there isn't enough free space on this Mac for the notes model.".to_string()
        })
    } else if lower.contains("sha256 mismatch") || lower.contains("checksum") {
        // A3: never "verif" — TLS's "certificate verify failed" is a network
        // problem, not a bad download.
        NotesError::Checksum
    } else if let Some(code) = http_status_in(&lower) {
        NotesError::Server(code)
    } else {
        NotesError::Offline
    }
}

/// The `NNN` of the last "returned HTTP NNN" in a download error.
fn http_status_in(lower: &str) -> Option<u16> {
    let at = lower.rfind("http ")?;
    let digits: String = lower[at + 5..].chars().take_while(char::is_ascii_digit).collect();
    let code: u16 = digits.parse().ok()?;
    (400..600).contains(&code).then_some(code)
}

/// The work a job does. The app's implementation lives in `lib.rs`; the
/// tests stub it.
pub trait NotesBackend: Send + Sync {
    /// Is a notes model already on disk?
    fn model_ready(&self) -> bool;
    /// Fetch the notes model, calling `progress` with 0–100. Must stop between
    /// chunks once `cancel` is set and return [`NotesError::Cancelled`].
    fn fetch_model(
        &self,
        progress: &mut dyn FnMut(u8),
        cancel: &CancelToken,
    ) -> Result<(), NotesError>;
    /// Summarize one meeting and store the notes. A cancelled run stores
    /// NOTHING and returns [`NotesError::Cancelled`].
    fn summarize(&self, id: &str, cancel: &CancelToken) -> Result<(), NotesError>;
}

pub type Reporter = Arc<dyn Fn(&NotesStatus) + Send + Sync>;

struct Queue {
    pending: VecDeque<String>,
    running: Option<(String, CancelToken)>,
    status: HashMap<String, NotesStatus>,
}

struct Inner {
    q: Mutex<Queue>,
    cv: Condvar,
    backend: Arc<dyn NotesBackend>,
    report: Reporter,
}

impl Inner {
    fn set(&self, status: NotesStatus) {
        {
            let mut q = self.q.lock().unwrap_or_else(|p| p.into_inner());
            q.status.insert(status.id.clone(), status.clone());
        }
        self.cv.notify_all();
        // Outside the lock: the reporter reads the database and emits.
        (self.report)(&status);
    }
}

/// The handle every trigger shares. Cheap to clone.
#[derive(Clone)]
pub struct SummaryJobs {
    inner: Arc<Inner>,
}

impl SummaryJobs {
    /// Start the one notes worker.
    pub fn spawn(backend: Arc<dyn NotesBackend>, report: Reporter) -> Self {
        let inner = Arc::new(Inner {
            q: Mutex::new(Queue {
                pending: VecDeque::new(),
                running: None,
                status: HashMap::new(),
            }),
            cv: Condvar::new(),
            backend,
            report,
        });
        let worker = inner.clone();
        std::thread::Builder::new()
            .name("wv-summary".into())
            .spawn(move || worker_loop(&worker))
            .expect("spawn the notes worker");
        Self { inner }
    }

    /// Queue `id` for notes. `false` when that meeting is already queued or
    /// running — the per-meeting job lock. Never blocks on the job.
    pub fn submit(&self, id: &str) -> bool {
        {
            let mut q = self.inner.q.lock().unwrap_or_else(|p| p.into_inner());
            let running = q.running.as_ref().is_some_and(|(r, _)| r == id);
            if running || q.pending.iter().any(|p| p == id) {
                return false;
            }
            q.pending.push_back(id.to_string());
        }
        self.inner.set(NotesStatus {
            id: id.to_string(),
            phase: NotesPhase::Queued,
            percent: 0,
            reason: None,
        });
        true
    }

    /// Cancel `id`'s notes, queued or running. `false` when it is neither.
    pub fn cancel(&self, id: &str) -> bool {
        let dequeued = {
            let mut q = self.inner.q.lock().unwrap_or_else(|p| p.into_inner());
            if let Some((r, token)) = q.running.as_ref() {
                if r == id {
                    token.cancel();
                    return true;
                }
            }
            let before = q.pending.len();
            q.pending.retain(|p| p != id);
            q.pending.len() != before
        };
        if dequeued {
            self.inner.set(NotesStatus {
                id: id.to_string(),
                phase: NotesPhase::Cancelled,
                percent: 0,
                reason: None,
            });
        }
        dequeued
    }

    pub fn status(&self, id: &str) -> Option<NotesStatus> {
        let q = self.inner.q.lock().unwrap_or_else(|p| p.into_inner());
        q.status.get(id).cloned()
    }

    pub fn statuses(&self) -> Vec<NotesStatus> {
        let q = self.inner.q.lock().unwrap_or_else(|p| p.into_inner());
        q.status.values().cloned().collect()
    }

    /// Block until `id` is no longer queued or running (or `timeout`), and
    /// return its last status. For the manual re-summarize command and tests.
    pub fn wait_for(&self, id: &str, timeout: Duration) -> Option<NotesStatus> {
        let deadline = Instant::now() + timeout;
        let mut q = self.inner.q.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            let busy = q.running.as_ref().is_some_and(|(r, _)| r == id)
                || q.pending.iter().any(|p| p == id);
            if !busy {
                return q.status.get(id).cloned();
            }
            let now = Instant::now();
            if now >= deadline {
                return q.status.get(id).cloned();
            }
            q = self
                .inner
                .cv
                .wait_timeout(q, deadline - now)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }

    /// Block until nothing is queued or running (or `timeout`).
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut q = self.inner.q.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if q.running.is_none() && q.pending.is_empty() {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            q = self
                .inner
                .cv
                .wait_timeout(q, deadline - now)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }
}

fn worker_loop(inner: &Arc<Inner>) {
    loop {
        let (id, token) = {
            let mut q = inner.q.lock().unwrap_or_else(|p| p.into_inner());
            loop {
                if let Some(id) = q.pending.pop_front() {
                    let token = CancelToken::new();
                    q.running = Some((id.clone(), token.clone()));
                    break (id, token);
                }
                q = inner.cv.wait(q).unwrap_or_else(|p| p.into_inner());
            }
        };
        // A1: a panic in the model fetch or the summary ends THIS job as
        // failed (with Retry) — it never kills the worker or leaves
        // `running` set, which would park every later meeting at "queued".
        let end = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_one(inner, &id, &token)
        }))
        .unwrap_or_else(|_| {
            log::error!("summary: the notes job for meeting {id} panicked — marked failed");
            NotesStatus {
                id: id.clone(),
                phase: NotesPhase::Failed,
                percent: 0,
                reason: Some(INTERNAL_ERROR_REASON.to_string()),
            }
        });
        // A2: the slot is cleared and the end status written under ONE lock,
        // so a submit (Retry, the live hook, resume) cannot land between the
        // two and have its Queued status overwritten by this job's end.
        {
            let mut q = inner.q.lock().unwrap_or_else(|p| p.into_inner());
            q.running = None;
            q.status.insert(end.id.clone(), end.clone());
        }
        inner.cv.notify_all();
        (inner.report)(&end);
    }
}

fn run_one(inner: &Arc<Inner>, id: &str, token: &CancelToken) -> NotesStatus {
    let status = |phase, percent, reason| NotesStatus {
        id: id.to_string(),
        phase,
        percent,
        reason,
    };
    let end = |e: NotesError| match e {
        NotesError::Cancelled => status(NotesPhase::Cancelled, 0, None),
        other => status(NotesPhase::Failed, 0, Some(other.plain())),
    };
    if !inner.backend.model_ready() {
        inner.set(status(NotesPhase::PreparingNotes, 0, None));
        let mut last = 0u8;
        let mut progress = |pct: u8| {
            if pct != last {
                last = pct;
                inner.set(status(NotesPhase::PreparingNotes, pct, None));
            }
        };
        if let Err(e) = inner.backend.fetch_model(&mut progress, token) {
            return end(e);
        }
    }
    if token.is_cancelled() {
        return end(NotesError::Cancelled);
    }
    inner.set(status(NotesPhase::Summarizing, 0, None));
    match inner.backend.summarize(id, token) {
        Ok(()) => status(NotesPhase::Ready, 100, None),
        Err(_) if token.is_cancelled() => end(NotesError::Cancelled),
        Err(e) => end(e),
    }
}

// ── the upgrade backlog (review A3) ─────────────────────────────────────────

/// What the person said about notes for meetings recorded before notes existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BacklogChoice {
    #[default]
    Pending,
    Accepted,
    Dismissed,
}

/// `notes-jobs.json`: when automatic notes started on this install, and the
/// answer to the one "Summarize N past meetings?" banner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotesLedger {
    pub since: DateTime<Utc>,
    #[serde(default)]
    pub backlog: BacklogChoice,
}

impl NotesLedger {
    /// Read the ledger, creating it (automatic notes start NOW) on first run.
    /// A fresh install has no meetings, so everything after is automatic; an
    /// upgrade's existing meetings are the opt-in backlog.
    pub fn load_or_create(path: &std::path::Path, now: DateTime<Utc>) -> Self {
        if let Some(l) = std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<NotesLedger>(&b).ok())
        {
            return l;
        }
        let l = NotesLedger {
            since: now,
            backlog: BacklogChoice::Pending,
        };
        l.save(path);
        l
    }

    pub fn save(&self, path: &std::path::Path) {
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, bytes).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }
}

/// One meeting that has a transcript and no notes.
#[derive(Debug, Clone)]
pub struct NeedsNotes {
    pub id: String,
    pub started_at: DateTime<Utc>,
}

/// A5 — rows a crash left in DB state `summarizing` (this version or an older
/// one): `(id, state)` pairs in, the ids to put back to `complete` out. A row
/// whose job is live in this process (`active`) is really being summarized
/// and is left alone. Without this the card says "Writing notes" forever and
/// offers no button once the backlog banner is dismissed.
pub fn stale_summarizing(rows: &[(String, String)], active: &dyn Fn(&str) -> bool) -> Vec<String> {
    rows.iter()
        .filter(|(id, state)| state == "summarizing" && !active(id))
        .map(|(id, _)| id.clone())
        .collect()
}

/// Split the meetings that need notes into the ones resumed automatically
/// (recorded since automatic notes started, or the backlog was accepted) and
/// the backlog that waits for the banner's click. Never a 1.1 GB download on
/// upgrade without that click.
pub fn plan_resume(rows: &[NeedsNotes], ledger: &NotesLedger) -> (Vec<String>, Vec<String>) {
    let mut resume = Vec::new();
    let mut backlog = Vec::new();
    for r in rows {
        if r.started_at >= ledger.since || ledger.backlog == BacklogChoice::Accepted {
            resume.push(r.id.clone());
        } else {
            backlog.push(r.id.clone());
        }
    }
    (resume, backlog)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;

    /// A backend whose summary blocks until released, and counts how many
    /// summaries run at once. Every test owns its own — nothing global.
    struct Stub {
        ready: AtomicBool,
        fetch_fail: Mutex<Option<NotesError>>,
        sum_fail: Mutex<Option<NotesError>>,
        gate: Mutex<Option<mpsc::Receiver<()>>>,
        calls: AtomicUsize,
        fetches: AtomicUsize,
        live: AtomicUsize,
        max_live: AtomicUsize,
        stored: Mutex<Vec<String>>,
    }

    impl Stub {
        fn new(ready: bool) -> Arc<Self> {
            Arc::new(Self {
                ready: AtomicBool::new(ready),
                fetch_fail: Mutex::new(None),
                sum_fail: Mutex::new(None),
                gate: Mutex::new(None),
                calls: AtomicUsize::new(0),
                fetches: AtomicUsize::new(0),
                live: AtomicUsize::new(0),
                max_live: AtomicUsize::new(0),
                stored: Mutex::new(Vec::new()),
            })
        }
        fn gated(self: &Arc<Self>) -> mpsc::Sender<()> {
            let (tx, rx) = mpsc::channel();
            *self.gate.lock().unwrap() = Some(rx);
            tx
        }
    }

    impl NotesBackend for Stub {
        fn model_ready(&self) -> bool {
            self.ready.load(Ordering::SeqCst)
        }
        fn fetch_model(
            &self,
            progress: &mut dyn FnMut(u8),
            cancel: &CancelToken,
        ) -> Result<(), NotesError> {
            self.fetches.fetch_add(1, Ordering::SeqCst);
            if let Some(e) = self.fetch_fail.lock().unwrap().take() {
                return Err(e);
            }
            // Ten "chunks"; the cancel flag is checked between each.
            for i in 1..=10u8 {
                if cancel.is_cancelled() {
                    return Err(NotesError::Cancelled);
                }
                progress(i * 10);
                if i == 3 {
                    if let Some(rx) = self.gate.lock().unwrap().as_ref() {
                        let _ = rx.recv_timeout(Duration::from_secs(5));
                    }
                }
            }
            self.ready.store(true, Ordering::SeqCst);
            Ok(())
        }
        fn summarize(&self, id: &str, cancel: &CancelToken) -> Result<(), NotesError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let now = self.live.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_live.fetch_max(now, Ordering::SeqCst);
            if self.ready.load(Ordering::SeqCst) {
                if let Some(rx) = self.gate.lock().unwrap().as_ref() {
                    let _ = rx.recv_timeout(Duration::from_secs(5));
                }
            }
            self.live.fetch_sub(1, Ordering::SeqCst);
            if cancel.is_cancelled() {
                return Err(NotesError::Cancelled); // nothing stored
            }
            if let Some(e) = self.sum_fail.lock().unwrap().take() {
                return Err(e);
            }
            self.stored.lock().unwrap().push(id.to_string());
            Ok(())
        }
    }

    fn jobs(stub: &Arc<Stub>) -> (SummaryJobs, Arc<Mutex<Vec<NotesStatus>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let j = SummaryJobs::spawn(
            stub.clone(),
            Arc::new(move |st: &NotesStatus| s.lock().unwrap().push(st.clone())),
        );
        (j, seen)
    }

    fn wait_phase(j: &SummaryJobs, id: &str, phase: NotesPhase) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while j.status(id).map(|s| s.phase) != Some(phase) {
            assert!(Instant::now() < deadline, "{id} never reached {phase:?}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    // B3 — two triggers for one meeting (the live hook and the launch resume)
    // make ONE job; a second meeting waits its turn, so one sidecar at a time.
    #[test]
    fn two_triggers_for_one_meeting_run_one_job_one_sidecar_at_a_time() {
        let stub = Stub::new(true);
        let release = stub.gated();
        let (j, _) = jobs(&stub);
        let a = j.clone();
        let b = j.clone();
        let t1 = std::thread::spawn(move || a.submit("m1"));
        let t2 = std::thread::spawn(move || b.submit("m1"));
        let accepted = [t1.join().unwrap(), t2.join().unwrap()];
        assert_eq!(accepted.iter().filter(|x| **x).count(), 1, "one job per meeting");
        wait_phase(&j, "m1", NotesPhase::Summarizing);
        assert!(!j.submit("m1"), "a running meeting is not queued again");
        assert!(j.submit("m2"));
        for _ in 0..2 {
            release.send(()).unwrap();
        }
        assert!(j.wait_idle(Duration::from_secs(5)));
        assert_eq!(stub.calls.load(Ordering::SeqCst), 2);
        assert_eq!(stub.max_live.load(Ordering::SeqCst), 1, "never two sidecars");
        assert_eq!(*stub.stored.lock().unwrap(), vec!["m1", "m2"]);
    }

    // B3 — the transcription worker hands a meeting over and goes on: a queued
    // meeting transcribes while the first one's summary is still running.
    #[test]
    fn a_queued_meeting_transcribes_while_a_summary_runs() {
        let stub = Stub::new(true);
        let release = stub.gated();
        let (j, _) = jobs(&stub);
        let order = Arc::new(Mutex::new(Vec::<String>::new()));
        let (o, jj) = (order.clone(), j.clone());
        // The shape of the pipeline worker: transcribe, call the hook, next.
        let worker = std::thread::spawn(move || {
            for id in ["m1", "m2"] {
                o.lock().unwrap().push(format!("transcribed {id}"));
                jj.submit(id);
            }
        });
        worker.join().unwrap();
        wait_phase(&j, "m1", NotesPhase::Summarizing);
        assert_eq!(
            *order.lock().unwrap(),
            vec!["transcribed m1", "transcribed m2"],
            "m2 transcribed while m1's summary was still blocked"
        );
        assert!(stub.stored.lock().unwrap().is_empty());
        release.send(()).unwrap();
        release.send(()).unwrap();
        assert!(j.wait_idle(Duration::from_secs(5)));
    }

    // B1 — every failure path ends on a card state with a plain reason, and
    // Retry re-runs the job.
    #[test]
    fn failures_are_visible_and_retry_reruns() {
        // Reasons are compared against `plain()` itself, never re-typed: a
        // test literal must not duplicate shipped copy.
        let cases = [
            (NotesError::Offline, true),
            (NotesError::DiskFull("stub: 1.6 GB needed, 0.2 GB free".into()), true),
            (NotesError::Checksum, true),
            (NotesError::Summary("stub: the summary refused".into()), false),
            (NotesError::ModelStart, false),
        ];
        for (err, in_download) in cases {
            let reason = err.plain();
            let stub = Stub::new(!in_download);
            if in_download {
                *stub.fetch_fail.lock().unwrap() = Some(err.clone());
            } else {
                *stub.sum_fail.lock().unwrap() = Some(err.clone());
            }
            let (j, _) = jobs(&stub);
            assert!(j.submit("m1"));
            let st = j.wait_for("m1", Duration::from_secs(5)).unwrap();
            assert_eq!(st.phase, NotesPhase::Failed, "{err:?}");
            assert_eq!(st.reason.as_deref(), Some(reason.as_str()), "{err:?}");
            assert!(stub.stored.lock().unwrap().is_empty());
            // Retry: the same submit, and this time it lands.
            assert!(j.submit("m1"), "a failed meeting can be queued again");
            let st = j.wait_for("m1", Duration::from_secs(5)).unwrap();
            assert_eq!(st.phase, NotesPhase::Ready, "{err:?} retry");
            assert_eq!(*stub.stored.lock().unwrap(), vec!["m1"]);
        }
    }

    #[test]
    fn download_errors_sort_into_plain_reasons() {
        assert_eq!(
            classify_download_error("download failed after 4 attempts: request to https://x failed: dns error"),
            NotesError::Offline
        );
        assert_eq!(
            classify_download_error("download failed after 4 attempts: download from x stalled at 10 bytes"),
            NotesError::Offline
        );
        assert_eq!(
            classify_download_error("download failed after 4 attempts: sha256 mismatch: expected a, got b"),
            NotesError::Checksum
        );
        assert!(matches!(
            classify_download_error("write /m.partial: No space left on device (os error 28)"),
            NotesError::DiskFull(_)
        ));
        assert_eq!(
            classify_download_error(crate::models::DOWNLOAD_CANCELLED),
            NotesError::Cancelled
        );
    }

    // B2 — Cancel during the download stops it between chunks and the summary
    // never starts; the cancel is not wiped by the summary phase.
    #[test]
    fn cancel_during_download_stops_between_chunks() {
        let stub = Stub::new(false);
        let release = stub.gated();
        let (j, seen) = jobs(&stub);
        assert!(j.submit("m1"));
        wait_phase(&j, "m1", NotesPhase::PreparingNotes);
        let deadline = Instant::now() + Duration::from_secs(5);
        while j.status("m1").map(|s| s.percent) != Some(30) {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(j.cancel("m1"));
        release.send(()).unwrap();
        let st = j.wait_for("m1", Duration::from_secs(5)).unwrap();
        assert_eq!(st.phase, NotesPhase::Cancelled);
        assert_eq!(stub.calls.load(Ordering::SeqCst), 0, "no summary after a cancelled download");
        let max_pct = seen.lock().unwrap().iter().map(|s| s.percent).max().unwrap();
        assert_eq!(max_pct, 30, "no chunk after the cancel");
        // Resume / Retry redoes it.
        *stub.gate.lock().unwrap() = None;
        assert!(j.submit("m1"));
        assert_eq!(j.wait_for("m1", Duration::from_secs(5)).unwrap().phase, NotesPhase::Ready);
    }

    // B2 — Cancel during the summary stores nothing and says "cancelled".
    #[test]
    fn cancel_during_summary_stores_nothing() {
        let stub = Stub::new(true);
        let release = stub.gated();
        let (j, _) = jobs(&stub);
        assert!(j.submit("m1"));
        wait_phase(&j, "m1", NotesPhase::Summarizing);
        assert!(j.cancel("m1"));
        release.send(()).unwrap();
        let st = j.wait_for("m1", Duration::from_secs(5)).unwrap();
        assert_eq!(st.phase, NotesPhase::Cancelled);
        assert!(stub.stored.lock().unwrap().is_empty(), "no partial stored as final");
    }

    #[test]
    fn cancel_a_queued_meeting_dequeues_it() {
        let stub = Stub::new(true);
        let release = stub.gated();
        let (j, _) = jobs(&stub);
        assert!(j.submit("m1"));
        assert!(j.submit("m2"));
        wait_phase(&j, "m1", NotesPhase::Summarizing);
        assert!(j.cancel("m2"));
        assert_eq!(j.status("m2").unwrap().phase, NotesPhase::Cancelled);
        release.send(()).unwrap();
        assert!(j.wait_idle(Duration::from_secs(5)));
        assert_eq!(*stub.stored.lock().unwrap(), vec!["m1"]);
        assert!(!j.cancel("m3"));
    }

    // A4 — tokens are per job; cancelling one never touches another.
    #[test]
    fn cancel_tokens_are_independent() {
        let a = CancelToken::new();
        let b = CancelToken::new();
        let a2 = a.clone();
        a.cancel();
        assert!(a.is_cancelled() && a2.is_cancelled());
        assert!(!b.is_cancelled());
    }

    // A3 — on upgrade, old meetings wait for the banner; new ones resume.
    #[test]
    fn upgrade_backlog_is_opt_in() {
        let since: DateTime<Utc> = "2026-10-19T12:00:00Z".parse().unwrap();
        let row = |id: &str, at: &str| NeedsNotes { id: id.into(), started_at: at.parse().unwrap() };
        let rows = vec![
            row("old1", "2026-09-01T10:00:00Z"),
            row("old2", "2026-10-18T10:00:00Z"),
            row("new1", "2026-10-19T13:00:00Z"),
        ];
        let mut ledger = NotesLedger { since, backlog: BacklogChoice::Pending };
        let (resume, backlog) = plan_resume(&rows, &ledger);
        assert_eq!(resume, vec!["new1"]);
        assert_eq!(backlog, vec!["old1", "old2"]);
        ledger.backlog = BacklogChoice::Dismissed;
        assert_eq!(plan_resume(&rows, &ledger).0, vec!["new1"]);
        ledger.backlog = BacklogChoice::Accepted;
        let (resume, backlog) = plan_resume(&rows, &ledger);
        assert_eq!(resume.len(), 3);
        assert!(backlog.is_empty());
    }

    #[test]
    fn ledger_is_created_once_and_read_back() {
        let dir = std::env::temp_dir().join(format!("yap-notes-ledger-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notes-jobs.json");
        let t1: DateTime<Utc> = "2026-10-19T12:00:00Z".parse().unwrap();
        let t2: DateTime<Utc> = "2026-10-20T12:00:00Z".parse().unwrap();
        let l = NotesLedger::load_or_create(&path, t1);
        assert_eq!(l.since, t1);
        let mut l2 = NotesLedger::load_or_create(&path, t2);
        assert_eq!(l2.since, t1, "the first run's moment sticks");
        l2.backlog = BacklogChoice::Accepted;
        l2.save(&path);
        assert_eq!(NotesLedger::load_or_create(&path, t2).backlog, BacklogChoice::Accepted);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A5: a crash's leftover `summarizing` row is reset; a live job is not.
    #[test]
    fn stale_summarizing_rows_are_reset_but_live_jobs_are_not() {
        let rows = vec![
            ("old".to_string(), "summarizing".to_string()),
            ("live".to_string(), "summarizing".to_string()),
            ("done".to_string(), "complete".to_string()),
        ];
        let active = |id: &str| id == "live";
        assert_eq!(stale_summarizing(&rows, &active), vec!["old".to_string()]);
    }


    /// A6: every reason a card can show is real copy — non-empty and
    /// distinct per variant — without re-typing any shipped string here.
    #[test]
    fn every_failure_reason_is_non_empty_and_distinct() {
        let variants = [
            NotesError::Offline,
            NotesError::DiskFull(String::from("stub disk")),
            NotesError::Checksum,
            NotesError::Server(503),
            NotesError::ModelStart,
            NotesError::Summary(String::from("stub summary")),
            NotesError::Cancelled,
        ];
        let reasons: Vec<String> = variants.iter().map(NotesError::plain).collect();
        for (i, r) in reasons.iter().enumerate() {
            assert!(r.trim().len() > 3, "empty reason for {:?}", variants[i]);
            for (j, other) in reasons.iter().enumerate().skip(i + 1) {
                assert_ne!(r, other, "{:?} and {:?} read the same", variants[i], variants[j]);
            }
        }
        assert!(NotesError::Server(503).plain().contains("503"));
        assert_ne!(NotesError::Server(404).plain(), NotesError::Server(503).plain());
    }

}
