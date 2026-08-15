//! YV121 — the parent side of the `yap-diarize` sidecar.
//!
//! This is `polish.rs`'s [`SidecarPool`](crate::polish) ported, not copied
//! loosely: the same four policies (readiness handshake, bounded lateness, one
//! restart per session, idle unload), the same fail-toward-the-caller posture,
//! and the same launcher seam that makes all of it testable against a stub
//! PROCESS with zero model bytes. `tests/diarize_sidecar_pool.rs` drives it.
//!
//! Three things are deliberately different, and each difference is about
//! diarization rather than about the pattern:
//!
//! * **The readiness wait is real.** The polish pool must never block, because
//!   a keystroke is waiting behind it — a cold child there makes the take a
//!   no-op. Diarization runs once per COMPLETED meeting with nothing waiting on
//!   the keyboard, so this pool waits for the handshake the way
//!   `summarize::SidecarSession` does, bounded by [`READY_BUDGET`].
//! * **The child takes no model on argv.** Models arrive as a `load_models`
//!   request, so a model change is not a respawn, and the readiness budget
//!   covers a process start rather than a multi-second ONNX session build.
//! * **The job owns the child.** A diarization job is a burst — one
//!   `load_models`, then a `diarize` per track, then some `embed`s — and then
//!   nothing for hours. [`DiarizePool::shutdown`] at the end of a job is the
//!   primary teardown; [`DiarizePool::sweep_idle`] is the backstop for a job
//!   that was abandoned (an error path that dropped its handle, a quit
//!   mid-diarization).
//!
//! ## The unit discipline reaches the API, not just the docs
//!
//! [`DiarizePool::diarize`] takes a
//! [`CosineDistance`](crate::diarize_metrics::CosineDistance) — never an `f32`.
//! `sherpa_onnx`'s clustering threshold is a DISTANCE (smaller = more similar)
//! and the plan's enrollment bands are SIMILARITIES (larger = more similar);
//! read one as the other and clustering ends up looser than the identity
//! decision it feeds (merged finding #20). The single `.get()` that turns the
//! newtype into the wire's bare `f32` is in this file and nowhere else.
//!
//! ## No accuracy threshold ships here
//!
//! Not one. The two durations below are LIVENESS budgets — "is this process
//! alive", "has this job been abandoned" — and both are inherited from the
//! polish sidecar's shipped values rather than derived from anything about
//! diarization, because nothing has measured diarization on this machine yet.
//! Every accuracy number in this epic is an output of `diarize_metrics` against
//! a fixture (YV126 for clustering, YV129 for enrollment), never an input.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::diarize_metrics::CosineDistance;
use crate::diarize_protocol::{
    parse_ready, parse_response_for, DiarizeRequest, DiarizeResponse, DiarizeSegment,
};

/// Filename of the bundled sidecar. Tauri strips the target triple from
/// `bundle.externalBin` when it stages the binary next to the app executable,
/// and a workspace `cargo build` puts it in the same directory in dev.
const DIARIZE_BIN: &str = "yap-diarize";

/// How long a freshly spawned sidecar may take to announce readiness.
///
/// This bounds a PROCESS START, not a model load — the child writes its
/// readiness line before it opens any model (`diarize_protocol.rs` explains
/// why), so the work inside this window is exec + dynamic linking, plus (on a
/// freshly installed bundle) Gatekeeper's first-launch assessment of a
/// notarized binary. 10s is the polish path's shipped budget for strictly more
/// work than this one does; past it the child is not starting, it is stuck.
const READY_BUDGET: Duration = Duration::from_secs(10);

/// How long a warm sidecar may sit UNUSED before it is torn down.
///
/// Deliberately the same 10 minutes `polish.rs`'s `SIDECAR_IDLE_UNLOAD` uses,
/// and deliberately NOT tuned: a diarization job's real wall-clock on this
/// machine has never been measured (the plan's own ~30s-per-45-minutes figure
/// is a third-party blog number it flags as a prior, not a measurement), so a
/// tighter window would be a guess that could tear a live job's child out from
/// under it. The primary teardown is [`DiarizePool::shutdown`] at the end of a
/// job; this is the backstop, and being generous costs a resident process with
/// no model in it. YV126 may tighten it against a measured job duration.
const IDLE_UNLOAD: Duration = Duration::from_secs(10 * 60);

/// Respawns allowed per session after the child dies. One, for the same reason
/// the polish pool allows one: a single death is usually transient, a second is
/// the machine, and an unbounded respawn pays a process launch forever.
const MAX_RESTARTS: u64 = 1;

/// Cap on one logged stderr line from the child, and on an error tag echoed
/// from it. Its stderr is diagnostics, not text — but a runaway or garbled
/// child must not be able to flood the rotating log through either channel.
const LOG_CHARS: usize = 300;

/// The transport deadline for one request.
///
/// Deliberately generous, and deliberately NOT a performance claim: nothing has
/// measured how long a real diarization pass takes on this machine, and a tight
/// deadline invented ahead of that measurement would be a vendor number wearing
/// a `const`'s clothes. It bounds a WEDGED child and nothing finer. YV126
/// replaces it with one derived from a measured RTF on the eval harness, and
/// [`DiarizePool::request_with_deadline`] is how a caller that already knows
/// better says so.
pub const DEFAULT_REQUEST_DEADLINE: Duration = Duration::from_secs(600);

/// Monotonic request id. A response carrying an older one answers a job the
/// parent already gave up on and is discarded (`parse_response_for`).
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed)
}

/// Why a diarization request did not produce an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiarizeError {
    /// No sidecar binary, the spawn failed, or the child is gone.
    Unavailable,
    /// No answer inside the deadline.
    Deadline,
    /// The child answered something that is not a usable response line.
    Protocol,
    /// The child answered, and the answer is "no", carrying its tag. This is
    /// the protocol WORKING — a missing model file, an unsupported kind — so it
    /// never counts against the restart budget and never kills the child.
    Refused(String),
}

impl DiarizeError {
    /// A short tag for a log line or a diagnostics row. Never a sentence and
    /// never a path.
    pub fn tag(&self) -> &str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Deadline => "deadline",
            Self::Protocol => "protocol",
            Self::Refused(tag) => tag,
        }
    }
}

/// The longest tag this parent will echo. Every constant in
/// `diarize_protocol.rs` is well under it.
const MAX_TAG: usize = 32;

/// A tag from the child, or nothing.
///
/// The sidecar's own `DiarizeResponse::err` takes a `&'static str`, so a
/// `format!`ed path physically cannot be sent — this is the parent's half of
/// the same guarantee, for a line that arrived corrupted, truncated, or from a
/// binary that is not the one we shipped. A tag is `[a-z0-9_]{1,32}` or it is
/// not a tag: **filtering** a bad string would keep its recognisable pieces (a
/// path would arrive as its own account name with the slashes removed), so
/// anything that is not already tag-shaped becomes `protocol` outright.
fn sanitize_tag(raw: &str) -> String {
    let shaped = !raw.is_empty()
        && raw.len() <= MAX_TAG
        && raw
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if shaped {
        raw.to_string()
    } else {
        "protocol".to_string()
    }
}

/// The sidecar's lifecycle as Diagnostics sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiarizeState {
    /// Nothing spawned — a fresh session, or a child that was swept or shut
    /// down. Not a failure.
    NotStarted,
    /// Spawned, handshake not seen yet.
    Starting,
    /// Up and answering.
    Ready,
    /// Given up on for the rest of this session.
    Failed,
}

impl DiarizeState {
    pub fn tag(self) -> &'static str {
        match self {
            Self::NotStarted => "not-started",
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

/// The state plus the short reason a `Failed` carries — "that it failed" is not
/// a useful thing to show anybody.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiarizeStatus {
    pub state: DiarizeState,
    pub reason: Option<&'static str>,
}

impl DiarizeStatus {
    pub const fn new(state: DiarizeState, reason: Option<&'static str>) -> Self {
        Self { state, reason }
    }
}

/// How a sidecar process is started. Production builds the staged binary's
/// command; the tests build a stub process, so the whole state machine is
/// exercised with zero model bytes on disk.
pub type DiarizeLauncher = Box<dyn Fn() -> Result<Command, DiarizeError> + Send + Sync>;

/// The clock the idle window is measured on — injected so a test can jump ten
/// minutes instead of sleeping through them.
pub type DiarizeClock = Box<dyn Fn() -> Instant + Send + Sync>;

/// The production launcher: the staged `yap-diarize` next to the app executable.
fn staged_command() -> Result<Command, DiarizeError> {
    Ok(Command::new(
        diarize_binary().ok_or(DiarizeError::Unavailable)?,
    ))
}

/// The staged sidecar binary, next to the app executable — where Tauri puts an
/// `externalBin` in the bundle, and where a workspace build puts it in dev.
fn diarize_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let path = exe.parent()?.join(DIARIZE_BIN);
    path.is_file().then_some(path)
}

/// The app-wide pool, built on first use so the production launcher can be
/// swapped for a stub in the tests.
pub fn pool() -> &'static DiarizePool {
    static POOL: OnceLock<DiarizePool> = OnceLock::new();
    POOL.get_or_init(|| DiarizePool::new(Box::new(staged_command), READY_BUDGET))
}

/// The sidecar slot and the policy around it.
pub struct DiarizePool {
    slot: Mutex<Option<Sidecar>>,
    status: Mutex<DiarizeStatus>,
    /// Respawns spent this session — bounded by [`MAX_RESTARTS`].
    restarts: AtomicU64,
    launch: DiarizeLauncher,
    ready_budget: Duration,
    /// When the child was last asked for anything. Stamped on every request,
    /// which is the only way in.
    last_used: Mutex<Instant>,
    idle_unload: Duration,
    now: DiarizeClock,
}

impl DiarizePool {
    pub fn new(launch: DiarizeLauncher, ready_budget: Duration) -> Self {
        Self::with_clock(launch, ready_budget, IDLE_UNLOAD, Box::new(Instant::now))
    }

    /// A pool over a caller-supplied clock and idle window — how the tests
    /// drive the real state machine without sleeping through it.
    pub fn with_clock(
        launch: DiarizeLauncher,
        ready_budget: Duration,
        idle_unload: Duration,
        now: DiarizeClock,
    ) -> Self {
        let started = now();
        Self {
            slot: Mutex::new(None),
            status: Mutex::new(DiarizeStatus::new(DiarizeState::NotStarted, None)),
            restarts: AtomicU64::new(0),
            launch,
            ready_budget,
            last_used: Mutex::new(started),
            idle_unload,
            now,
        }
    }

    pub fn status(&self) -> DiarizeStatus {
        *self.status.lock()
    }

    /// Whether a child is currently held. Diagnostics, and the assertion a test
    /// makes about a refusal NOT being a death.
    pub fn is_warm(&self) -> bool {
        self.slot.lock().is_some()
    }

    fn set(&self, state: DiarizeState, reason: Option<&'static str>) {
        *self.status.lock() = DiarizeStatus::new(state, reason);
    }

    /// Give up on the sidecar for the rest of this session.
    fn fail(&self, reason: &'static str) {
        log::warn!("diarize sidecar failed: {reason}");
        self.set(DiarizeState::Failed, Some(reason));
    }

    /// The child is gone. One respawn per session; past that the stage is
    /// failed with a reason rather than relaunching a process that keeps dying.
    fn died(&self) {
        if self.restarts.fetch_add(1, Ordering::Relaxed) >= MAX_RESTARTS {
            self.fail("died");
        } else {
            log::warn!("diarize sidecar died — one restart left this session");
            self.set(DiarizeState::NotStarted, None);
        }
    }

    /// End of job: take the child down now rather than waiting for the idle
    /// sweep. Deliberately NOT a failure — the restart budget is untouched and
    /// the next job walks the ordinary spawn path.
    pub fn shutdown(&self) {
        let mut held = self.slot.lock();
        if let Some(child) = held.take() {
            child.kill();
            log::info!("diarize sidecar shut down at the end of a job");
        }
        // A session that FAILED stays failed: shutdown is a teardown, not a
        // reset, and clearing a sticky failure here would spawn a dying child
        // once per job forever.
        if self.status().state != DiarizeState::Failed {
            self.set(DiarizeState::NotStarted, None);
        }
    }

    /// Terminate a child that has gone [`idle_unload`](Self::with_clock)
    /// without a request. `true` when a process was actually killed.
    pub fn sweep_idle(&self) -> bool {
        let mut held = self.slot.lock();
        if held.is_none() {
            return false;
        }
        let idle = (self.now)().saturating_duration_since(*self.last_used.lock());
        if idle < self.idle_unload {
            return false;
        }
        if let Some(child) = held.take() {
            child.kill();
        }
        self.set(DiarizeState::NotStarted, None);
        log::info!(
            "diarize sidecar unloaded after {}s unused — it restarts on the next job",
            idle.as_secs()
        );
        true
    }

    /// Load a segmentation + embedding model pair.
    ///
    /// Returns the embedding dimension **the child reported**. There is no
    /// dimension constant anywhere on this side of the wire: the plan guessed a
    /// width, audit finding #19 "corrected" it to 192, and the shipped CAM++
    /// measures 512 (ResNet34, 256) — a parent that held an opinion about a
    /// model it has not opened is how that stays invisible, whichever number
    /// the opinion happens to be.
    pub fn load_models(&self, segmentation: &Path, embedding: &Path) -> Result<u32, DiarizeError> {
        let req = DiarizeRequest::load_models(next_id(), segmentation, embedding);
        self.request(&req)?
            .into_embedding_dim()
            .ok_or(DiarizeError::Protocol)
    }

    /// Diarize one track.
    ///
    /// `clustering` is a cosine **distance** — smaller is more similar. The
    /// newtype is the whole point: this is the one place in the crate where it
    /// becomes a bare `f32`, and it becomes one on the way OUT, into JSON.
    pub fn diarize(
        &self,
        wav: &Path,
        clustering: CosineDistance,
    ) -> Result<Vec<DiarizeSegment>, DiarizeError> {
        let req = DiarizeRequest::diarize(next_id(), wav, clustering.get());
        self.request(&req)?
            .into_segments()
            .ok_or(DiarizeError::Protocol)
    }

    /// Embed one enrollment utterance.
    pub fn embed(&self, wav: &Path) -> Result<Vec<f32>, DiarizeError> {
        let req = DiarizeRequest::embed(next_id(), wav);
        self.request(&req)?
            .into_embedding()
            .ok_or(DiarizeError::Protocol)
    }

    /// One request against the child, spawning and handshaking it if needed.
    pub fn request(&self, req: &DiarizeRequest) -> Result<DiarizeResponse, DiarizeError> {
        self.request_with_deadline(req, DEFAULT_REQUEST_DEADLINE)
    }

    /// The same, for a caller that knows its own deadline.
    pub fn request_with_deadline(
        &self,
        req: &DiarizeRequest,
        deadline: Duration,
    ) -> Result<DiarizeResponse, DiarizeError> {
        let mut held = self.slot.lock();
        // Stamped before any of the work below, so a request that FAILS still
        // counts as use: the idle sweep is for a child nobody is diarizing at,
        // not for a bad job.
        *self.last_used.lock() = (self.now)();

        if held.is_none() {
            // Failure is sticky: past the restart budget we stop paying a
            // process launch per request and stay honestly failed.
            if self.status().state == DiarizeState::Failed {
                return Err(DiarizeError::Unavailable);
            }
            match Sidecar::spawn(self.launch.as_ref()) {
                Ok(fresh) => {
                    log::info!("diarize sidecar starting");
                    self.set(DiarizeState::Starting, None);
                    *held = Some(fresh);
                }
                Err(e) => {
                    self.fail("spawn_failed");
                    return Err(e);
                }
            }
        }

        let sidecar = held.as_mut().expect("spawned above");
        // Unlike the polish path this WAITS: there is no keystroke behind a
        // diarization, and a job that starts by silently skipping the sidecar
        // would produce an unlabelled transcript with no reason attached.
        match sidecar.wait_ready(self.ready_budget) {
            Ok(()) => self.set(DiarizeState::Ready, None),
            Err(ReadyFailure::Timeout) => {
                held.take().expect("borrowed above").kill();
                self.fail("ready_timeout");
                return Err(DiarizeError::Unavailable);
            }
            Err(ReadyFailure::Gone) => {
                held.take().expect("borrowed above").kill();
                self.died();
                return Err(DiarizeError::Unavailable);
            }
        }

        match sidecar.exchange(req, deadline) {
            // A refusal is the protocol working. The child stays warm, the
            // restart budget is untouched, and the caller is told which "no" it
            // got — `model_not_found` and "the process died" are different
            // bugs with different fixes.
            Ok(response) => match response.err_tag() {
                Some(tag) => {
                    let tag = sanitize_tag(tag);
                    log::info!("diarize request refused: {tag}");
                    Err(DiarizeError::Refused(tag))
                }
                None => Ok(response),
            },
            Err(e) => {
                // A missed deadline leaves work running and a half-written line
                // in the pipe: kill it rather than read that line into the NEXT
                // request. This is why it is a process.
                if let Some(dead) = held.take() {
                    dead.kill();
                }
                if e == DiarizeError::Unavailable {
                    self.died();
                } else {
                    self.set(DiarizeState::NotStarted, None);
                }
                Err(e)
            }
        }
    }
}

/// Why a readiness wait ended without a handshake.
enum ReadyFailure {
    /// Still silent at the budget.
    Timeout,
    /// stdout closed — the child exited before announcing itself.
    Gone,
}

/// One sidecar process plus the reader threads that turn its stdout into lines
/// the parent can wait on with a timeout, and its stderr into log records.
struct Sidecar {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    /// The handshake, taken once. `None` after it has arrived.
    ready: Option<Receiver<String>>,
    /// When this process was launched — the clock the readiness budget runs on.
    started: Instant,
}

impl Sidecar {
    fn spawn(
        launch: &(dyn Fn() -> Result<Command, DiarizeError> + Send + Sync),
    ) -> Result<Self, DiarizeError> {
        let mut child = launch()?
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Piped and drained: a stderr nobody reads eventually fills its
            // pipe and blocks the child mid-write.
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| DiarizeError::Unavailable)?;
        let stdin = child.stdin.take().ok_or(DiarizeError::Unavailable)?;
        let stdout = child.stdout.take().ok_or(DiarizeError::Unavailable)?;
        let stderr = child.stderr.take().ok_or(DiarizeError::Unavailable)?;
        let (tx, lines) = mpsc::channel();
        let (ready_tx, ready) = mpsc::channel();
        // Reading a pipe cannot be given a timeout, so the read lives in a
        // thread and the parent waits on a channel instead. Both channels
        // disconnect when the child's stdout closes, which is how "it died" is
        // told from "it is slow".
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                // The handshake rides the SAME stream as the responses — that
                // is what makes it a protocol message rather than a log line.
                if let Some(hello) = parse_ready(&line) {
                    log::info!("diarize sidecar ready: version={}", hello.version);
                    let _ = ready_tx.send(hello.version);
                    continue;
                }
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                log::debug!(
                    "yap-diarize: {}",
                    line.chars().take(LOG_CHARS).collect::<String>()
                );
            }
        });
        Ok(Self {
            child,
            stdin,
            lines,
            ready: Some(ready),
            started: Instant::now(),
        })
    }

    /// Wait for the handshake, bounded by what is LEFT of `budget` since the
    /// spawn — so a second request behind a slow start does not restart the
    /// clock and hand the child another full budget.
    fn wait_ready(&mut self, budget: Duration) -> Result<(), ReadyFailure> {
        let Some(ready) = self.ready.as_ref() else {
            return Ok(());
        };
        let left = budget
            .checked_sub(self.started.elapsed())
            .ok_or(ReadyFailure::Timeout)?;
        match ready.recv_timeout(left) {
            Ok(_version) => {
                self.ready = None;
                Ok(())
            }
            Err(RecvTimeoutError::Timeout) => Err(ReadyFailure::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(ReadyFailure::Gone),
        }
    }

    /// One request, one answer, inside `deadline`.
    fn exchange(
        &mut self,
        req: &DiarizeRequest,
        deadline: Duration,
    ) -> Result<DiarizeResponse, DiarizeError> {
        let line = serde_json::to_string(req).map_err(|_| DiarizeError::Protocol)?;
        writeln!(self.stdin, "{line}").map_err(|_| DiarizeError::Unavailable)?;
        self.stdin.flush().map_err(|_| DiarizeError::Unavailable)?;
        let until = Instant::now() + deadline.max(Duration::from_millis(1));
        loop {
            let left = until
                .checked_duration_since(Instant::now())
                .ok_or(DiarizeError::Deadline)?;
            match self.lines.recv_timeout(left) {
                // A line for another id answers a job this process stopped
                // waiting for; returning it would attribute one meeting's turns
                // to another's audio.
                Ok(line) => {
                    if let Some(response) = parse_response_for(&line, req.id) {
                        return Ok(response);
                    }
                }
                Err(RecvTimeoutError::Timeout) => return Err(DiarizeError::Deadline),
                Err(RecvTimeoutError::Disconnected) => return Err(DiarizeError::Unavailable),
            }
        }
    }

    fn kill(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tag from the child can be logged; a sentence, a path or a transcript
    /// cannot. The sidecar's own `err` constructor takes a `&'static str`, so
    /// this is defence in depth against a corrupted or half-written line.
    #[test]
    fn a_child_error_tag_is_bounded_and_carries_no_prose() {
        // Every tag the protocol defines survives unchanged.
        for tag in [
            crate::diarize_protocol::ERR_BAD_REQUEST,
            crate::diarize_protocol::ERR_UNSUPPORTED_KIND,
            crate::diarize_protocol::ERR_MISSING_FIELD,
            crate::diarize_protocol::ERR_MODEL_NOT_FOUND,
            crate::diarize_protocol::ERR_AUDIO_NOT_FOUND,
            crate::diarize_protocol::ERR_NO_MODELS,
            crate::diarize_protocol::ERR_MODEL_LOAD_FAILED,
            crate::diarize_protocol::ERR_AUDIO_UNREADABLE,
            crate::diarize_protocol::ERR_SAMPLE_RATE,
            crate::diarize_protocol::ERR_AUDIO_TOO_SHORT,
            crate::diarize_protocol::ERR_BACKEND_FAILED,
        ] {
            assert_eq!(sanitize_tag(tag), tag);
        }
        // Nothing else does — and in particular a path does not arrive as its
        // own recognisable pieces, which a filtering implementation would allow.
        let path = "/Users/somebody/Library/Application Support/Yap/seg.onnx";
        assert_eq!(sanitize_tag(path), "protocol");
        assert!(!sanitize_tag(path).contains("somebody"));
        assert_eq!(sanitize_tag("we agreed to ship on friday"), "protocol");
        assert_eq!(sanitize_tag(""), "protocol");
        assert_eq!(sanitize_tag("Model_Not_Found"), "protocol");
        assert_eq!(sanitize_tag(&"x".repeat(MAX_TAG + 1)), "protocol");
        assert_eq!(sanitize_tag(&"x".repeat(MAX_TAG)), "x".repeat(MAX_TAG));
    }

    /// The clustering threshold crosses into JSON exactly once, and it crosses
    /// as the number the newtype holds — not as its similarity twin, which is
    /// the mixed-unit bug finding #20 describes.
    ///
    /// The assertion that matters here is the SECOND one. Building a request by
    /// hand and reading its field back proves only that `DiarizeRequest` stores
    /// what it was handed; the defect lives on the line where
    /// [`DiarizePool::diarize`] spends the newtype, and that line still compiles
    /// with `CosineSimilarity::from_distance(clustering).get()` written on it.
    /// So this test also drives the pool against a stub that echoes the
    /// threshold it was SENT, and checks the number that actually reached the
    /// child's stdin. (`tests/diarize_sidecar_pool.rs` holds the same line from
    /// outside the crate; both fail on an inversion, deliberately.)
    #[test]
    fn the_clustering_threshold_reaches_the_wire_as_a_distance() {
        let distance = CosineDistance::new(0.35);
        let req = DiarizeRequest::diarize(1, Path::new("/a.wav"), distance.get());
        assert_eq!(req.clustering_distance_threshold, Some(0.35));
        // The similarity that pairs with it is a DIFFERENT number, and nothing
        // in this file can put it on the wire — `diarize()` takes the distance
        // type, so writing a similarity there does not compile.
        let similarity = crate::diarize_metrics::CosineSimilarity::from_distance(distance);
        assert!((similarity.get() - 0.65).abs() < 1e-6);
        assert_ne!(Some(similarity.get()), req.clustering_distance_threshold);

        // …and the value the CHILD received is that same distance. The stub
        // hands back whatever `clustering_distance_threshold` the request line
        // carried, as the segment's `start`.
        const ECHO: &str = concat!(
            r#"printf '{"type":"ready","version":"stub"}\n'"#,
            "\n",
            "while read -r line; do\n",
            r#"  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')"#,
            "\n",
            r#"  t=$(printf '%s' "$line" | sed -n 's/.*"clustering_distance_threshold":\([0-9.]*\).*/\1/p')"#,
            "\n",
            r#"  printf '{"id":%s,"ok":true,"segments":[{"start":%s,"end":9.0,"cluster":0}]}\n' "$id" "${t:-0}""#,
            "\ndone\n"
        );
        let pool = DiarizePool::new(
            Box::new(|| {
                let mut command = Command::new("/bin/sh");
                command.arg("-c").arg(ECHO);
                Ok(command)
            }),
            Duration::from_secs(10),
        );
        let segments = pool
            .diarize(Path::new("/a.wav"), distance)
            .expect("the echo stub answers");
        let sent = segments[0].start;
        assert!(
            (sent - 0.35).abs() < 1e-6,
            "the child's stdin carried {sent}, not the 0.35 DISTANCE"
        );
        assert!(
            (sent - f64::from(similarity.get())).abs() > 1e-6,
            "a cosine SIMILARITY reached the child where a distance belongs"
        );
        pool.shutdown();
    }

    /// Request ids are monotonic per process, which is what makes a stale
    /// response identifiable at all.
    #[test]
    fn request_ids_never_repeat() {
        let a = next_id();
        let b = next_id();
        assert!(b > a);
    }
}
