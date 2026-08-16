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

use crate::diarize_metrics::{cosine_similarity, CosineDistance};
use crate::diarize_protocol::{
    parse_ready, parse_response_for, DiarizeRequest, DiarizeResponse, DiarizeSegment,
};
use crate::meetings::{
    cluster_speaker_label, diarization_target, DiarizationTarget, MeetingKind, MeetingSegment,
    OTHER_SPEAKER_LABEL, MIC_TRACK, SYSTEM_TRACK,
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
    ///
    /// `min_embed` is the shortest span of audio the caller considers worth
    /// embedding, and it is a `Duration` for the same reason `clustering` is a
    /// newtype: "2.0" is a plausible-looking value in seconds and in
    /// milliseconds, and only one of them is a turn. **There is deliberately no
    /// default** — not here, not in `yap-diarize`, not in `catalog.json`. A
    /// turn whose exclusive audio falls under it comes back with an empty
    /// embedding rather than a full-width vector nothing measured; see
    /// `min_embed_seconds` on the wire contract for the sweep behind that, and
    /// `diarize_wire_unit_discipline.rs` for the guard that keeps a constant
    /// from appearing before somebody measures one on real speech.
    pub fn diarize(
        &self,
        wav: &Path,
        clustering: CosineDistance,
        min_embed: Duration,
    ) -> Result<Vec<DiarizeSegment>, DiarizeError> {
        let req =
            DiarizeRequest::diarize(next_id(), wav, clustering.get(), min_embed.as_secs_f32());
        self.request(&req)?
            .into_segments()
            .ok_or(DiarizeError::Protocol)
    }

    /// Embed one enrollment utterance.
    ///
    /// Same floor, same absence of a default. Enrollment gets a REFUSAL rather
    /// than an empty vector when the clip is under it — an `embed` response is
    /// nothing but the vector, so "too short" has to arrive as `audio_too_short`
    /// or it does not arrive.
    pub fn embed(&self, wav: &Path, min_embed: Duration) -> Result<Vec<f32>, DiarizeError> {
        let req = DiarizeRequest::embed(next_id(), wav, min_embed.as_secs_f32());
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

// ═══════════════════════════════════════════════════════════════════════════
// YV126 — clustering: the threshold, the smaller task, and the surfacing floor
// ═══════════════════════════════════════════════════════════════════════════
//
// Three things live below, and they are one item because they are one argument
// about the same mechanism ceiling.
//
//   1. **The threshold is a DISTANCE and the clustering that applies it is
//      OURS.** [`cluster_by_distance_threshold`] is complete-linkage
//      agglomerative clustering over the per-turn embeddings the wire already
//      carries. It could have been left to `sherpa-onnx`'s own
//      `FastClusteringConfig`, and the child is still SENT the threshold so a
//      backend that must cluster in order to segment can. But the number this
//      repo tunes against YV120's harness has to be applied by code this repo
//      can score, step through and fail — merged finding #20's lesson is that a
//      threshold whose semantics live in somebody else's default is a number
//      nobody here can check. When the child returns embeddings, the parent's
//      assignment is the one that ships; when it does not, the child's own
//      cluster ids are used and that fallback is logged rather than hidden.
//
//   2. **Past the mechanism ceiling the fix is a smaller task, not a better
//      threshold** (merged finding #5). pyannote-segmentation-3.0 caps at 3
//      speakers per 10 s window and 2 simultaneous, and sherpa's pipeline
//      deletes every overlapped frame before embedding. A six-person far-field
//      classroom exceeds that by construction — fixture (f) is built to — so
//      full N-way clustering there is not mistuned, it is impossible.
//      [`TargetMode::EnrolledVsEveryoneElse`] is the 2-class problem that is
//      achievable far-field and is what a student actually wants from a lecture
//      recording: isolate the instructor.
//
//   3. **A cluster count is not a verdict** (merged finding #25). The original
//      plan rejected a whole diarization pass when the cluster count exceeded
//      `max(8, attendees × 2)`; a manually started meeting has no attendee
//      count, so that cap is 8, and a real six-person room produces 10–15 raw
//      clusters before any merge. [`rank_and_floor`] replaces the reject with a
//      ranking and a floor: chips for the clusters carrying real speech, one
//      "Other" bucket for the rest, and nothing thrown away.
//
// Nothing in this section carries a tuned accuracy number. The clustering
// threshold is a PARAMETER — there is deliberately no default constant for it
// anywhere in this crate, because the only honest source for one is a
// measurement against fixture (e), and this build has no inference backend to
// measure with (`yap-diarize`'s `load_backend` still answers `no_backend`;
// YV122 is the item that changes that). The two numbers that DO appear
// ([`CHIP_FLOOR_SECONDS`], [`CHIP_FLOOR_TURNS`]) are surfacing floors from the
// audit's own finding #25, not accuracy thresholds: they change what a person
// is shown, never what is computed or stored.

/// One clustered turn on one track, ready to be attributed to transcript rows.
///
/// Times are seconds from the start of that track's WAV — the same base
/// `meeting_segments.start_seconds` is on after YV107's host-time rebase, which
/// is what makes [`attribute_clusters`] a comparison rather than a guess.
#[derive(Debug, Clone, PartialEq)]
pub struct DiarizedSegment {
    /// Which recorded track this turn came from: [`MIC_TRACK`] under
    /// [`DiarizationTarget::ClusterTrackA`], [`SYSTEM_TRACK`] under
    /// [`DiarizationTarget::MicIsMe`]. Carried so the attribution step cannot
    /// apply one track's clusters to another track's rows.
    pub track: i64,
    pub start_seconds: f64,
    pub end_seconds: f64,
    /// The cluster this turn belongs to, local to this meeting and this pass.
    /// Cluster 0 of two meetings is two different people until an enrolled
    /// profile says otherwise (YV128/YV129).
    pub cluster_index: i64,
}

impl DiarizedSegment {
    pub fn new(track: i64, start_seconds: f64, end_seconds: f64, cluster_index: i64) -> Self {
        Self {
            track,
            start_seconds,
            end_seconds,
            cluster_index,
        }
    }

    pub fn duration(&self) -> f64 {
        (self.end_seconds - self.start_seconds).max(0.0)
    }
}

/// The enrolled voice a binary-mode pass is looking for.
///
/// `cluster` is which RAW cluster that profile matched — a decision made by
/// comparing the profile's centroid against each cluster's, in cosine
/// SIMILARITY, against a band tuned in YV129. YV126 does not make it and does
/// not guess it: there is no enrollment threshold in this file, and inventing
/// one to pick "probably the loudest cluster is the instructor" is exactly the
/// vendor-number failure this backlog exists to avoid. The caller says which
/// cluster; this file collapses everything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnrolledSpeaker {
    /// The `speaker_profiles` row (YV128). Carried through so the caller that
    /// writes the attribution knows whose it is.
    pub profile_id: i64,
    /// The raw `cluster_index` YV129's matcher identified as that profile.
    pub cluster: i64,
}

impl EnrolledSpeaker {
    pub fn matched(profile_id: i64, cluster: i64) -> Self {
        Self {
            profile_id,
            cluster,
        }
    }
}

/// What task the clustering pass is being asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetMode {
    /// Every distinct voice gets its own cluster. Correct where the audio stays
    /// inside pyannote-segmentation-3.0's ceiling — a near-field room of two or
    /// three (fixture (e)) — and the default ask.
    FullClustering,
    /// Collapse to two classes: the enrolled voice, and everyone else.
    ///
    /// Merged finding #5's reframe. This is not a degraded version of full
    /// clustering, it is a DIFFERENT and smaller question, and it is the one a
    /// six-person far-field classroom can actually answer.
    EnrolledVsEveryoneElse(EnrolledSpeaker),
}

/// The cluster the enrolled voice is relabelled to in binary mode.
pub const ENROLLED_CLUSTER: i64 = 0;
/// The cluster every other voice is relabelled to in binary mode. Not "unknown"
/// and not one-per-speaker: the claim is precisely "this is not the enrolled
/// voice", and a mechanism that cannot separate six far-field speakers may not
/// keep six ids around implying that it did.
pub const EVERYONE_ELSE_CLUSTER: i64 = 1;

/// Total speech, in seconds, a cluster must carry before it gets a chip of its
/// own. From merged finding #25's own fix ("show chips only above a floor —
/// ~30s total, ≥3 turns"), and it is a SURFACING floor, not an accuracy
/// threshold: a cluster below it is still clustered, still stored, and still
/// correctable (YV130). Nothing about it is measured against the eval harness
/// because nothing about it decides who spoke.
pub const CHIP_FLOOR_SECONDS: f64 = 30.0;
/// …and how many separate turns, for the same reason. One 45-second monologue
/// from a passer-by is not a participant; three turns is the cheapest available
/// evidence that a voice was part of the conversation.
pub const CHIP_FLOOR_TURNS: usize = 3;

/// The two WAVs a meeting may have, as the clustering stage sees them.
#[derive(Debug, Clone, Copy)]
pub struct MeetingTracks<'a> {
    /// Track A — the microphone. Always present for a recorded meeting.
    pub mic_wav: &'a Path,
    /// Track B — the system-audio tap's WAV, when one was recorded and kept.
    /// `None` is the answer for every in-person meeting, every meeting whose
    /// tap was denied or revoked, and every meeting whose audio has aged out.
    pub system_wav: Option<&'a Path>,
}

/// Cluster the track this meeting's [`MeetingKind`] says has to be clustered.
///
/// The branch is YV125's [`diarization_target`], unchanged and un-duplicated:
///
/// * [`DiarizationTarget::ClusterTrackA`] — in-person, unknown, or a virtual
///   meeting whose tap never delivered. The microphone carries the room, so the
///   microphone is what gets clustered.
/// * [`DiarizationTarget::MicIsMe`] — a virtual meeting with a live second
///   track. The microphone is one voice by mechanism (nobody else is in the
///   room), so the track that needs splitting is Track B, the mixed stream of
///   every remote participant.
///
/// `threshold` is a cosine **distance** and there is no default for it in this
/// crate: see this section's header. `mode` picks the task
/// ([`TargetMode`]).
///
/// Errors are the sidecar's, unchanged and un-swallowed — in particular
/// `Refused("no_backend")` on any build whose sidecar has no inference backend
/// compiled in, which is every build until YV122. A diarization that cannot run
/// must fail loudly enough for the caller to leave the transcript unattributed;
/// returning an empty segment list would read as "nobody spoke".
pub fn cluster_track(
    pool: &DiarizePool,
    tracks: MeetingTracks<'_>,
    kind: MeetingKind,
    threshold: CosineDistance,
    mode: TargetMode,
) -> Result<Vec<DiarizedSegment>, DiarizeError> {
    let target = diarization_target(kind, tracks.system_wav.is_some());
    let (wav, track) = match target {
        DiarizationTarget::ClusterTrackA => (tracks.mic_wav, MIC_TRACK),
        // `MicIsMe` is only reachable when `system_wav` is `Some` — that is
        // literally the input `diarization_target` branched on — so this is a
        // total match rather than an unwrap on a maybe.
        DiarizationTarget::MicIsMe => match tracks.system_wav {
            Some(sys) => (sys, SYSTEM_TRACK),
            None => (tracks.mic_wav, MIC_TRACK),
        },
    };

    let raw = pool.diarize(wav, threshold)?;
    let clustered = assign_clusters(&raw, threshold);
    let segments: Vec<DiarizedSegment> = raw
        .iter()
        .zip(clustered)
        .map(|(seg, cluster_index)| DiarizedSegment::new(track, seg.start, seg.end, cluster_index))
        .collect();

    Ok(match mode {
        TargetMode::FullClustering => segments,
        TargetMode::EnrolledVsEveryoneElse(enrolled) => {
            collapse_to_enrolled_vs_everyone_else(&segments, enrolled.cluster)
        }
    })
}

/// Assign a cluster to each turn the sidecar returned.
///
/// Prefers the parent's own [`cluster_by_distance_threshold`] over the embeddings
/// on the wire, and falls back to the child's ids only when the child sent none
/// — a fallback that is logged, because a silent one would make the tuned
/// threshold stop being the thing that decided the answer.
fn assign_clusters(raw: &[DiarizeSegment], threshold: CosineDistance) -> Vec<i64> {
    let embeddings: Vec<&[f32]> = raw.iter().map(|s| s.embedding.as_slice()).collect();
    let usable = !raw.is_empty()
        && embeddings.iter().all(|e| !e.is_empty())
        && embeddings.iter().all(|e| e.len() == embeddings[0].len());
    if usable {
        return cluster_by_distance_threshold(&embeddings, threshold)
            .into_iter()
            .map(|c| c as i64)
            .collect();
    }
    if !raw.is_empty() {
        log::warn!(
            "diarize returned {} turns without usable embeddings — falling back to the \
             child's own cluster ids, which were NOT assigned by this build's threshold",
            raw.len()
        );
    }
    raw.iter().map(|s| i64::from(s.cluster)).collect()
}

/// Complete-linkage agglomerative clustering of embeddings, cut at a cosine
/// **distance** threshold.
///
/// Returns one cluster index per input, numbered by first appearance so the ids
/// are small, stable and readable in a database row.
///
/// **Complete linkage, deliberately.** The threshold then means something a
/// person can state: *every* pair of turns inside a cluster is within
/// `threshold` of each other. Single linkage — the other obvious choice, and
/// what a naive "connect everything closer than t" implementation gives you —
/// means only that a CHAIN of such pairs exists, and chaining is the dominant
/// failure mode of exactly this workload: over a 50-minute meeting a slow
/// acoustic drift (a speaker turning away from the mic, an AGC ramp) links six
/// people into one cluster through a sequence of near neighbours, each hop
/// legitimately under the threshold. That failure looks like a mistuned number
/// and is not one.
///
/// The implementation is the standard nearest-neighbour-chain algorithm, which
/// is O(n²) rather than the naive O(n³) and is exact for complete linkage
/// (a reducible metric). `nn_chain_agrees_with_brute_force_complete_linkage`
/// holds it to a brute-force reference over randomised inputs, including ties
/// and duplicate points, because "clever and exact" is a claim and not a fact.
pub fn cluster_by_distance_threshold(
    embeddings: &[&[f32]],
    threshold: CosineDistance,
) -> Vec<usize> {
    let n = embeddings.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![0];
    }
    let mut distances = vec![vec![0.0f64; n]; n];
    for i in 0..n {
        for j in (i + 1)..n {
            let similarity = cosine_similarity(embeddings[i], embeddings[j]);
            let d = f64::from(CosineDistance::from_similarity(similarity).get());
            distances[i][j] = d;
            distances[j][i] = d;
        }
    }
    let merges = complete_linkage_merges(&mut distances);

    // Cut the dendrogram: apply every merge at or below the threshold. Complete
    // linkage produces monotone merge heights, so this is the same partition a
    // greedy "merge the closest pair while it is within the threshold" produces
    // — which is what the brute-force reference in the tests actually does.
    let cut = f64::from(threshold.get());
    let mut parent: Vec<usize> = (0..n).collect();
    for (a, b, height) in merges {
        if height <= cut + 1e-9 {
            union(&mut parent, a, b);
        }
    }

    let mut label_of: Vec<Option<usize>> = vec![None; n];
    let mut out = Vec::with_capacity(n);
    let mut next = 0usize;
    for i in 0..n {
        let root = find(&mut parent, i);
        let label = match label_of[root] {
            Some(l) => l,
            None => {
                label_of[root] = Some(next);
                next += 1;
                next - 1
            }
        };
        out.push(label);
    }
    out
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        parent[rb.max(ra)] = rb.min(ra);
    }
}

/// The merge sequence of complete-linkage clustering: `(a, b, height)` with `a`
/// the surviving representative, in the order the nearest-neighbour chain
/// produced them.
fn complete_linkage_merges(distances: &mut [Vec<f64>]) -> Vec<(usize, usize, f64)> {
    let n = distances.len();
    let mut active = vec![true; n];
    let mut remaining = n;
    let mut chain: Vec<usize> = Vec::with_capacity(n);
    let mut merges = Vec::with_capacity(n.saturating_sub(1));

    while remaining > 1 {
        if chain.is_empty() {
            let seed = (0..n).find(|i| active[*i]).expect("an active cluster");
            chain.push(seed);
        }
        let a = *chain.last().expect("non-empty chain");
        // The nearest active neighbour of `a`, ties broken by lowest index so
        // the walk is deterministic and cannot cycle between equidistant pairs.
        let mut best = usize::MAX;
        let mut best_distance = f64::INFINITY;
        for (b, is_active) in active.iter().enumerate() {
            if !is_active || b == a {
                continue;
            }
            if distances[a][b] < best_distance {
                best_distance = distances[a][b];
                best = b;
            }
        }
        let b = best;
        if chain.len() >= 2 && chain[chain.len() - 2] == b {
            // Reciprocal nearest neighbours: this pair is a merge in the exact
            // dendrogram, whichever order the chain reached them in.
            chain.pop();
            chain.pop();
            let (keep, gone) = (a.min(b), a.max(b));
            merges.push((keep, gone, best_distance));
            for k in 0..n {
                if !active[k] || k == keep || k == gone {
                    continue;
                }
                // Lance-Williams for complete linkage: the distance to a merged
                // cluster is the FARTHEST of the two, which is what makes the
                // threshold a guarantee about every pair rather than about one.
                let far = distances[keep][k].max(distances[gone][k]);
                distances[keep][k] = far;
                distances[k][keep] = far;
            }
            active[gone] = false;
            remaining -= 1;
        } else {
            chain.push(b);
        }
    }
    merges
}

/// Collapse a clustered track to the 2-class task: the enrolled voice, and
/// everyone else.
///
/// Merged finding #5. The output carries at most two distinct cluster indices —
/// exactly two whenever the enrolled cluster actually occurs — and the turn
/// boundaries are untouched: the mechanism can find WHERE the speaker changes
/// far-field, it is telling six far-field voices apart that it cannot do.
pub fn collapse_to_enrolled_vs_everyone_else(
    segments: &[DiarizedSegment],
    enrolled_cluster: i64,
) -> Vec<DiarizedSegment> {
    segments
        .iter()
        .map(|s| DiarizedSegment {
            cluster_index: if s.cluster_index == enrolled_cluster {
                ENROLLED_CLUSTER
            } else {
                EVERYONE_ELSE_CLUSTER
            },
            ..s.clone()
        })
        .collect()
}

/// One cluster's weight in a meeting, and what it is called on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedCluster {
    pub cluster_index: i64,
    pub speech_seconds: f64,
    pub turns: usize,
    /// 1-based position in speech-time order among the SURFACED clusters, or
    /// `None` for one below the floor. This is what
    /// [`cluster_speaker_label`] numbers — never the `cluster_index`.
    pub rank: Option<usize>,
    /// `Speaker 1` … or [`OTHER_SPEAKER_LABEL`].
    pub label: String,
}

/// The surfacing decision for one clustered track: chips, and one bucket.
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterRanking {
    /// Clusters at or above both floors, most speech first.
    pub surfaced: Vec<RankedCluster>,
    /// Everything else, most speech first. Present rather than dropped: these
    /// rows keep their stored `cluster_index`, and YV130's correction UX is how
    /// one of them gets promoted or merged into a real speaker.
    pub other: Vec<RankedCluster>,
}

impl ClusterRanking {
    /// The chips a surface draws, in order. Never more than one per surfaced
    /// cluster, plus [`OTHER_SPEAKER_LABEL`] when anything landed in the
    /// bucket.
    pub fn chips(&self) -> Vec<String> {
        let mut chips: Vec<String> = self.surfaced.iter().map(|c| c.label.clone()).collect();
        if !self.other.is_empty() {
            chips.push(OTHER_SPEAKER_LABEL.to_string());
        }
        chips
    }

    /// What a given stored `cluster_index` is called. Unknown ids read as
    /// [`OTHER_SPEAKER_LABEL`] rather than panicking: a transcript that renders
    /// beats one that refuses over a number.
    pub fn label_for(&self, cluster_index: i64) -> String {
        self.surfaced
            .iter()
            .find(|c| c.cluster_index == cluster_index)
            .map(|c| c.label.clone())
            .unwrap_or_else(|| OTHER_SPEAKER_LABEL.to_string())
    }

    pub fn other_speech_seconds(&self) -> f64 {
        self.other.iter().map(|c| c.speech_seconds).sum()
    }
}

/// Rank a track's clusters by speech time and split them at the surfacing floor
/// — merged finding #25's replacement for the plan's hard reject.
///
/// **Never rejects.** The gate it replaces threw a whole diarization pass away
/// when the cluster count exceeded `max(8, attendees × 2)`, which fires on a
/// manually started six-person room (no attendee count ⇒ cap 8, 10–15 raw
/// clusters) — the case this backlog prioritises, thrown away where it is most
/// needed. Ranking cannot fail: a pass that produced fifteen clusters shows the
/// four that carry the conversation and rolls eleven into "Other".
pub fn rank_and_floor(segments: &[DiarizedSegment]) -> ClusterRanking {
    let mut totals: Vec<(i64, f64, usize)> = Vec::new();
    for segment in segments {
        match totals
            .iter_mut()
            .find(|(id, _, _)| *id == segment.cluster_index)
        {
            Some(entry) => {
                entry.1 += segment.duration();
                entry.2 += 1;
            }
            None => totals.push((segment.cluster_index, segment.duration(), 1)),
        }
    }
    // Most speech first; ties by cluster index so the order is total and a
    // re-render cannot reshuffle two equal speakers.
    totals.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));

    let mut surfaced = Vec::new();
    let mut other = Vec::new();
    for (cluster_index, speech_seconds, turns) in totals {
        let above = speech_seconds >= CHIP_FLOOR_SECONDS && turns >= CHIP_FLOOR_TURNS;
        if above {
            let rank = surfaced.len() + 1;
            surfaced.push(RankedCluster {
                cluster_index,
                speech_seconds,
                turns,
                rank: Some(rank),
                label: cluster_speaker_label(rank),
            });
        } else {
            other.push(RankedCluster {
                cluster_index,
                speech_seconds,
                turns,
                rank: None,
                label: OTHER_SPEAKER_LABEL.to_string(),
            });
        }
    }
    ClusterRanking { surfaced, other }
}

/// Attribute stored transcript rows to clusters, for
/// [`crate::db::Database::set_segment_clusters`].
///
/// Each transcript segment on the clustered track takes the cluster it shares
/// the most TIME with — not the one it starts inside, which mis-attributes
/// every row whose first word lands in the previous speaker's trailing frames.
/// A row that overlaps no clustered turn at all gets `None`: silence, a
/// hallucinated span, or audio the segmenter dropped is honestly unattributed,
/// and `0` there would be a claim about a person.
///
/// Rows on OTHER tracks are not returned at all — not even as `None`. One pass
/// clusters one track, and a virtual meeting's mic rows are filed as "Me" by
/// the YV125 branch rather than by clustering; emitting `None` for them would
/// clear an attribution this pass never looked at.
pub fn attribute_clusters(
    segments: &[MeetingSegment],
    diarized: &[DiarizedSegment],
) -> Vec<(String, Option<i64>)> {
    let track = match diarized.first() {
        Some(first) => first.track,
        None => return Vec::new(),
    };
    segments
        .iter()
        .filter(|s| s.track == track)
        .map(|s| {
            let mut best: Option<(i64, f64)> = None;
            for turn in diarized.iter().filter(|t| t.track == track) {
                let overlap =
                    turn.end_seconds.min(s.end_seconds) - turn.start_seconds.max(s.start_seconds);
                if overlap <= 0.0 {
                    continue;
                }
                match best {
                    Some((_, most)) if most >= overlap => {}
                    _ => best = Some((turn.cluster_index, overlap)),
                }
            }
            (s.id.clone(), best.map(|(cluster, _)| cluster))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The floor this module's requests carry.
    ///
    /// A test fixture and nothing more. `min_embed_seconds` has no default in
    /// `src/` — `diarize_wire_unit_discipline.rs` fails the build if one
    /// appears — so a test that needs a request has to state one, exactly like
    /// a caller does.
    const TEST_FLOOR: Duration = Duration::from_secs(2);

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
        let req = DiarizeRequest::diarize(
            1,
            Path::new("/a.wav"),
            distance.get(),
            TEST_FLOOR.as_secs_f32(),
        );
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
            .diarize(Path::new("/a.wav"), distance, TEST_FLOOR)
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

    // ── YV126 ───────────────────────────────────────────────────────────────

    /// Complete-linkage clustering, done the slow obvious way: merge the
    /// closest pair while it is within the threshold, recomputing every
    /// distance from the raw points each round.
    ///
    /// This is the DEFINITION the fast implementation has to match. It is
    /// O(n³) and would be unusable on a three-hour meeting, which is why it
    /// lives here and not in `src/`.
    fn brute_force_complete_linkage(points: &[&[f32]], threshold: CosineDistance) -> Vec<usize> {
        let n = points.len();
        let mut clusters: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
        let cut = f64::from(threshold.get());
        loop {
            let mut best: Option<(usize, usize, f64)> = None;
            for i in 0..clusters.len() {
                for j in (i + 1)..clusters.len() {
                    // Complete linkage: the farthest pair between the two.
                    let mut far = 0.0f64;
                    for a in &clusters[i] {
                        for b in &clusters[j] {
                            let similarity = cosine_similarity(points[*a], points[*b]);
                            let d = f64::from(CosineDistance::from_similarity(similarity).get());
                            far = far.max(d);
                        }
                    }
                    if best.is_none_or(|(_, _, best_d)| far < best_d) {
                        best = Some((i, j, far));
                    }
                }
            }
            match best {
                Some((i, j, d)) if d <= cut + 1e-9 => {
                    let moved = clusters.remove(j);
                    clusters[i].extend(moved);
                }
                _ => break,
            }
        }
        let mut labels = vec![usize::MAX; n];
        for (label, cluster) in clusters.iter().enumerate() {
            for member in cluster {
                labels[*member] = label;
            }
        }
        // Renumber by first appearance, exactly as the shipped function does,
        // so two correct answers that differ only in cluster NAMES compare equal.
        renumber(&labels)
    }

    fn renumber(labels: &[usize]) -> Vec<usize> {
        let mut seen: Vec<usize> = Vec::new();
        labels
            .iter()
            .map(|l| match seen.iter().position(|s| s == l) {
                Some(i) => i,
                None => {
                    seen.push(*l);
                    seen.len() - 1
                }
            })
            .collect()
    }

    /// A deterministic little PRNG, so a failure is reproducible from the seed
    /// printed in the assertion rather than "it went red once on CI".
    fn xorshift(state: &mut u64) -> f32 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        ((*state >> 40) as f32 / 8_388_608.0) - 1.0
    }

    /// The fast path is the slow path. Randomised, including the two cases an
    /// NN-chain implementation gets wrong when its tie-break is sloppy:
    /// duplicate points (distance exactly 0) and quantised coordinates that
    /// produce many equal distances.
    #[test]
    fn nn_chain_agrees_with_brute_force_complete_linkage() {
        let mut state = 0x5EED_1234_ABCD_0001u64;
        // Agreement is worthless if every case is trivially all-singletons or
        // trivially one cluster — two implementations of "return 0..n" agree
        // too. This counts the cases that actually partitioned.
        let mut interesting = 0usize;
        for case in 0..120 {
            let n = 2 + (case % 11);
            let dim = 8;
            let quantise = case % 3 == 0;
            let mut points: Vec<Vec<f32>> = Vec::new();
            for i in 0..n {
                // Every third case repeats an earlier point verbatim, so exact
                // ties are exercised rather than hoped for.
                if i > 0 && case % 4 == 0 && i % 3 == 0 {
                    points.push(points[i - 1].clone());
                    continue;
                }
                let mut v: Vec<f32> = (0..dim).map(|_| xorshift(&mut state)).collect();
                if quantise {
                    v = v.iter().map(|x| (x * 4.0).round() / 4.0).collect();
                }
                if v.iter().all(|x| *x == 0.0) {
                    v[0] = 1.0;
                }
                points.push(v);
            }
            let refs: Vec<&[f32]> = points.iter().map(|p| p.as_slice()).collect();
            for cut in [0.05f32, 0.2, 0.5, 0.9, 1.4] {
                let threshold = CosineDistance::new(cut);
                let fast = cluster_by_distance_threshold(&refs, threshold);
                let slow = brute_force_complete_linkage(&refs, threshold);
                assert_eq!(
                    fast, slow,
                    "case {case} (n={n}, cut={cut}): the nearest-neighbour chain \
                     disagreed with the definition"
                );
                let groups = fast.iter().max().copied().unwrap_or(0) + 1;
                if groups > 1 && groups < n {
                    interesting += 1;
                }
            }
        }
        assert!(
            interesting >= 100,
            "only {interesting} of the randomised cases produced a non-trivial \
             partition — this comparison is not exercising the algorithm"
        );
    }

    /// The threshold is a guarantee about EVERY pair, which is the property
    /// complete linkage was chosen for: single linkage would chain a slow
    /// acoustic drift into one cluster whose extremes are nothing like each
    /// other.
    #[test]
    fn a_cluster_never_contains_a_pair_beyond_the_threshold() {
        // Three points in a line, each hop 0.25 apart in cosine distance but
        // the ends 0.5 apart — single linkage would call this one cluster.
        let a = [1.0f32, 0.0, 0.0];
        let b = [0.75f32, 0.66143783, 0.0];
        let c = [0.5f32 * 0.75, 0.66143783 * 0.75 + 0.25, 0.0];
        let points: Vec<&[f32]> = vec![&a, &b, &c];
        let threshold = CosineDistance::new(0.3);
        let labels = cluster_by_distance_threshold(&points, threshold);
        for i in 0..points.len() {
            for j in (i + 1)..points.len() {
                if labels[i] != labels[j] {
                    continue;
                }
                let d = CosineDistance::from_similarity(cosine_similarity(points[i], points[j]));
                assert!(
                    d.get() <= threshold.get() + 1e-6,
                    "points {i} and {j} are {} apart but share a cluster",
                    d.get()
                );
            }
        }
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
