//! YV121 acceptance — `SidecarPool`'s policy, ported to `diarize.rs` and proven
//! against a STUB PROCESS with zero model bytes.
//!
//! ```sh
//! cargo test --test diarize_sidecar_pool
//! ```
//!
//! The stubs are `/bin/sh` scripts. `DiarizePool` cannot tell one from the real
//! binary — it only ever sees a `Command`, a stdin and a stdout — so the
//! handshake, the restart budget, the idle unload, the refusal path and the
//! stale-response guard are all exercised with no ONNX, no models and no
//! staged binary, exactly the trick `polish.rs`'s own stub-process tests use.
//!
//! Every stub echoes back the id it was SENT (`sed` off the line), so these
//! tests drive the pool's real public API — `load_models`, `embed` — rather
//! than a hand-assembled request whose id was chosen to match a hard-coded
//! answer.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use wilson_voice_lib::diarize::{
    DiarizeError, DiarizeLauncher, DiarizePool, DiarizeState, DiarizeStatus,
};
use wilson_voice_lib::diarize_metrics::CosineDistance;
use wilson_voice_lib::diarize_protocol::{DiarizeRequest, ERR_MODEL_NOT_FOUND};

/// A stub sidecar: `/bin/sh` running `script`.
fn stub(script: &'static str) -> DiarizeLauncher {
    Box::new(move || {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(script);
        Ok(command)
    })
}

/// The same, counting launches — how "one restart, then failed" is proven.
fn counted_stub(script: &'static str, launches: Arc<AtomicUsize>) -> DiarizeLauncher {
    Box::new(move || {
        launches.fetch_add(1, Ordering::Relaxed);
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(script);
        Ok(command)
    })
}

/// Announce readiness, then answer every request with the id it carried.
///
/// The reply is `embedding_dim: 7` on purpose: no real model is 7-dimensional,
/// so a parent that hard-coded 512 (the width the pinned CAM++ actually reports)
/// or 192 (finding #19's figure, which does not describe that file) would fail
/// the assertion that reads it back.
const READY_STUB: &str = concat!(
    r#"printf '{"type":"ready","version":"stub"}\n'"#,
    "\n",
    "while read -r line; do\n",
    r#"  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')"#,
    "\n",
    r#"  printf '{"id":%s,"ok":true,"embedding_dim":7,"embedding":[0.5,0.5]}\n' "$id""#,
    "\ndone\n"
);

/// Ready, then an ECHO of the clustering threshold it was SENT.
///
/// This is the only stub that reads a field other than `id` off the request
/// line, and it exists because the distance/similarity unit is the one thing
/// the newtypes cannot protect past `.get()`. It `sed`s
/// `"clustering_distance_threshold":<n>` off the line and hands `<n>` back as
/// the segment's `start` **and** as the single element of its `embedding`, so
/// the number that physically crossed the child's stdin is observable from the
/// parent's return value. A `load_models` line carries no threshold, so the
/// `sed` finds nothing and the stub answers `embedding_dim` instead — the same
/// 7 as `READY_STUB`, for the same reason.
const THRESHOLD_ECHO_STUB: &str = concat!(
    r#"printf '{"type":"ready","version":"stub"}\n'"#,
    "\n",
    "while read -r line; do\n",
    r#"  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')"#,
    "\n",
    r#"  t=$(printf '%s' "$line" | sed -n 's/.*"clustering_distance_threshold":\([0-9.]*\).*/\1/p')"#,
    "\n",
    r#"  if [ -n "$t" ]; then"#,
    "\n",
    r#"    printf '{"id":%s,"ok":true,"segments":[{"start":%s,"end":9.0,"cluster":0,"embedding":[%s]}]}\n' "$id" "$t" "$t""#,
    "\n",
    "  else\n",
    r#"    printf '{"id":%s,"ok":true,"embedding_dim":7}\n' "$id""#,
    "\n",
    "  fi\n",
    "done\n"
);

/// Ready, then a REFUSAL to everything. The protocol working, not a death.
const REFUSING_STUB: &str = concat!(
    r#"printf '{"type":"ready","version":"stub"}\n'"#,
    "\n",
    "while read -r line; do\n",
    r#"  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')"#,
    "\n",
    r#"  printf '{"id":%s,"ok":false,"err":"model_not_found"}\n' "$id""#,
    "\ndone\n"
);

/// Ready, then an answer for a job nobody asked about — the line that must
/// never be handed to the caller waiting on a different id.
const WRONG_ID_STUB: &str = concat!(
    r#"printf '{"type":"ready","version":"stub"}\n'"#,
    "\n",
    "while read -r line; do\n",
    r#"  printf '{"id":999999,"ok":true,"embedding_dim":7}\n'"#,
    "\ndone\n"
);

/// Never announces anything and never exits — a process that is not starting.
const SILENT_STUB: &str = "sleep 30\n";

/// Announces readiness and immediately dies, so the request that follows finds
/// a process that is gone.
const DYING_STUB: &str = r#"printf '{"type":"ready","version":"stub"}\n'"#;

/// Paths the stubs ignore. A real file would only prove the test is reading
/// from disk when it must not.
fn model_pair() -> (PathBuf, PathBuf) {
    (
        PathBuf::from("/nonexistent/segmentation.onnx"),
        PathBuf::from("/nonexistent/embedding.onnx"),
    )
}

fn ready_pool(script: &'static str) -> DiarizePool {
    DiarizePool::new(stub(script), Duration::from_secs(10))
}

/// The dimension is the CHILD's to report — audit finding #19's fix, as a
/// mechanism rather than as a comment.
///
/// The plan's §5 schema assumed 512-dimension embeddings and finding #19 said
/// the shipped model was 192; the artefact `catalog.json` pins measures **512**,
/// so the audit was the document that drifted. The way that class of error
/// survives is a parent that "knows" the width. This stub reports **7**, and the
/// pool reports 7 — so a constant anywhere on the Rust side of the wire fails
/// here, whichever number somebody picked.
#[test]
fn the_embedding_dimension_comes_from_the_child_never_from_the_parent() {
    let pool = ready_pool(READY_STUB);
    let (segmentation, embedding) = model_pair();
    assert_eq!(
        pool.load_models(&segmentation, &embedding),
        Ok(7),
        "the parent reports what the child measured"
    );
    assert_eq!(pool.status(), DiarizeStatus::new(DiarizeState::Ready, None));
    // And the handshake was a real one: the pool waited for it rather than
    // writing into a process that could not read yet.
    assert!(pool.is_warm());
    pool.shutdown();
    assert!(!pool.is_warm());
    assert_eq!(pool.status(), DiarizeStatus::new(DiarizeState::NotStarted, None));
}

/// A refusal is the protocol WORKING: the caller is told which "no" it got, the
/// child stays warm, and the restart budget is untouched.
///
/// This is the distinction a naive port of `SidecarPool` loses — there, every
/// non-answer is a failure, because there is only one thing a rewrite can fail
/// at. Here `model_not_found` and "the process died" are different bugs with
/// different fixes, and the second one is the only one that may spend a
/// restart.
#[test]
fn a_refusal_is_not_a_death() {
    let launches = Arc::new(AtomicUsize::new(0));
    let pool = DiarizePool::new(
        counted_stub(REFUSING_STUB, Arc::clone(&launches)),
        Duration::from_secs(10),
    );
    let (segmentation, embedding) = model_pair();

    assert_eq!(
        pool.load_models(&segmentation, &embedding),
        Err(DiarizeError::Refused(ERR_MODEL_NOT_FOUND.to_string()))
    );
    assert!(pool.is_warm(), "a refusal does not kill the child");
    assert_eq!(
        pool.status(),
        DiarizeStatus::new(DiarizeState::Ready, None),
        "a child that answered is ready, whatever the answer was"
    );

    // Ten more refusals: still one process, still not failed.
    for _ in 0..10 {
        assert!(matches!(
            pool.embed(Path::new("/a.wav")),
            Err(DiarizeError::Refused(_))
        ));
    }
    assert_eq!(launches.load(Ordering::Relaxed), 1, "no respawn was spent");
    assert_eq!(pool.status().state, DiarizeState::Ready);
    pool.shutdown();
}

/// A child that dies is respawned exactly once per session; the second death is
/// `failed(died)` and no further process is launched.
#[test]
fn a_dead_child_respawns_once_then_stays_failed() {
    let launches = Arc::new(AtomicUsize::new(0));
    let pool = DiarizePool::new(
        counted_stub(DYING_STUB, Arc::clone(&launches)),
        Duration::from_secs(10),
    );
    let (segmentation, embedding) = model_pair();

    // Request after request against a child that dies on arrival. Bounded, so a
    // pool that never gave up fails this test rather than hanging it.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut status = pool.status();
    while Instant::now() < deadline && status.state != DiarizeState::Failed {
        let _ = pool.load_models(&segmentation, &embedding);
        status = pool.status();
    }
    assert_eq!(status, DiarizeStatus::new(DiarizeState::Failed, Some("died")));
    assert_eq!(
        launches.load(Ordering::Relaxed),
        2,
        "one spawn plus exactly one restart, per session"
    );

    // Sticky: a later request neither launches a third process nor blocks.
    let started = Instant::now();
    assert_eq!(
        pool.load_models(&segmentation, &embedding),
        Err(DiarizeError::Unavailable)
    );
    assert!(started.elapsed() < Duration::from_millis(200));
    assert_eq!(launches.load(Ordering::Relaxed), 2);
    assert_eq!(pool.status().reason, Some("died"));

    // And `shutdown` is a teardown, not a reset — a failed session that quietly
    // came back would spawn a dying child once per job forever.
    pool.shutdown();
    assert_eq!(pool.status(), DiarizeStatus::new(DiarizeState::Failed, Some("died")));
    assert_eq!(launches.load(Ordering::Relaxed), 2);
}

/// A child that never announces itself is killed at the readiness budget — the
/// budget bounds LATENESS, and it is measured from the spawn, so a second
/// request behind a slow start does not hand the child another full budget.
#[test]
fn a_silent_child_is_killed_at_the_readiness_budget() {
    let launches = Arc::new(AtomicUsize::new(0));
    let budget = Duration::from_millis(300);
    let pool = DiarizePool::new(counted_stub(SILENT_STUB, Arc::clone(&launches)), budget);
    let (segmentation, embedding) = model_pair();

    let started = Instant::now();
    assert_eq!(
        pool.load_models(&segmentation, &embedding),
        Err(DiarizeError::Unavailable)
    );
    let waited = started.elapsed();
    assert!(
        waited >= budget && waited < budget * 10,
        "waited {waited:?} against a {budget:?} budget"
    );
    assert_eq!(
        pool.status(),
        DiarizeStatus::new(DiarizeState::Failed, Some("ready_timeout"))
    );
    assert!(!pool.is_warm(), "the silent child was killed, not left running");
    // `ready_timeout` is a failure, not a death: it did not spend the restart
    // budget, and the stage stays failed rather than launching again.
    let _ = pool.load_models(&segmentation, &embedding);
    assert_eq!(launches.load(Ordering::Relaxed), 1);
}

/// A response for another id is never handed to the caller. The parent waits
/// out its own deadline instead — because returning it would attribute one
/// meeting's turns to another meeting's audio.
#[test]
fn a_response_for_another_id_is_never_returned_to_this_caller() {
    let pool = ready_pool(WRONG_ID_STUB);
    let req = DiarizeRequest::embed(1, Path::new("/a.wav"));
    let started = Instant::now();
    let answer = pool.request_with_deadline(&req, Duration::from_millis(400));
    assert_eq!(answer, Err(DiarizeError::Deadline));
    assert!(started.elapsed() >= Duration::from_millis(400));
    // A missed deadline leaves work running and a half-written line in the
    // pipe, so the child goes — that is why this is a process.
    assert!(!pool.is_warm());
}

/// An idle child is torn down, and the next job brings it back through the
/// ORDINARY spawn + handshake path — the unload spends none of the restart
/// budget.
///
/// The window is driven by an INJECTED clock, so this test jumps ten minutes
/// instead of sleeping through them.
#[test]
fn an_idle_child_is_torn_down_and_the_next_job_brings_it_back() {
    let base = Instant::now();
    let offset = Arc::new(AtomicU64::new(0));
    let hand = Arc::clone(&offset);
    let launches = Arc::new(AtomicUsize::new(0));
    let idle_unload = Duration::from_secs(10 * 60);
    let pool = DiarizePool::with_clock(
        counted_stub(READY_STUB, Arc::clone(&launches)),
        Duration::from_secs(10),
        idle_unload,
        Box::new(move || base + Duration::from_secs(hand.load(Ordering::Relaxed))),
    );
    let (segmentation, embedding) = model_pair();

    assert_eq!(pool.load_models(&segmentation, &embedding), Ok(7));
    assert!(pool.is_warm(), "the child is warm");

    // A minute later it is still warm: the gap between two requests of one job
    // must never cost a process launch.
    offset.store(60, Ordering::Relaxed);
    assert!(!pool.sweep_idle());
    assert!(pool.is_warm());
    assert_eq!(pool.status(), DiarizeStatus::new(DiarizeState::Ready, None));

    // Ten minutes unused — the process goes, and its models with it.
    offset.store(10 * 60, Ordering::Relaxed);
    assert!(pool.sweep_idle(), "an idle sidecar is terminated");
    assert!(!pool.is_warm(), "no child is left resident");
    // NOT a failure: `NotStarted` is the state a fresh session begins in.
    assert_eq!(pool.status(), DiarizeStatus::new(DiarizeState::NotStarted, None));
    // Sweeping again is a no-op — nothing to kill, nothing to log.
    assert!(!pool.sweep_idle());

    // The next job walks the ordinary path…
    assert_eq!(pool.load_models(&segmentation, &embedding), Ok(7));
    assert_eq!(pool.status(), DiarizeStatus::new(DiarizeState::Ready, None));
    assert_eq!(launches.load(Ordering::Relaxed), 2, "one spawn per job, no more");

    // …and the restart budget is untouched by the unload, so a child that dies
    // AFTER an idle sweep still gets its one restart. Proven by the state
    // machine rather than by reading the counter: the pool is not failed.
    assert_ne!(pool.status().state, DiarizeState::Failed);
    pool.shutdown();
}

/// The clustering API takes a `CosineDistance`, and the number that physically
/// reaches the CHILD is that distance — not the similarity that pairs with it.
///
/// The newtypes make every Rust-side call site a compile-time-enforced single
/// unit, but `DiarizePool::diarize` is where the type is spent (`.get()`, into
/// wire JSON) and a type cannot guard what happens after that: writing
/// `CosineSimilarity::from_distance(clustering).get()` on that line still
/// compiles, still type-checks, and sends `0.65` where sherpa's clustering
/// expects `0.35` — clustering three times looser than the identity decision it
/// feeds. So this test does not inspect a request the test built itself; it
/// reads the value back **off the child's stdin**, echoed by the stub, and
/// names the wrong number explicitly. Merged finding #20, closed by an
/// observation rather than by a signature.
#[test]
fn the_clustering_threshold_crosses_the_wire_as_a_distance() {
    let pool = ready_pool(THRESHOLD_ECHO_STUB);
    let (segmentation, embedding) = model_pair();
    assert_eq!(pool.load_models(&segmentation, &embedding), Ok(7));

    let segments = pool
        .diarize(Path::new("/a.wav"), CosineDistance::new(0.35))
        .expect("the echo stub answers every diarize");
    assert_eq!(segments.len(), 1, "one echoed turn");
    // The child was SENT 0.35. Both the `start` and the embedding element are
    // the same echoed field, so an inversion moves both.
    let sent = segments[0].start;
    assert!(
        (sent - 0.35).abs() < 1e-6,
        "the child's stdin carried {sent}, not the 0.35 cosine DISTANCE the \
         caller passed"
    );
    assert!(
        (sent - 0.65).abs() > 1e-6,
        "0.65 is 1 − 0.35: a cosine SIMILARITY crossed the wire where sherpa's \
         clustering reads a distance, and every cluster would merge three \
         times too eagerly"
    );
    assert_eq!(segments[0].embedding, vec![0.35_f32]);

    // …and a second, different threshold, so a stub (or a parent) that answers
    // with a constant 0.35 cannot pass this test either.
    let segments = pool
        .diarize(Path::new("/b.wav"), CosineDistance::new(0.20))
        .expect("the echo stub answers every diarize");
    let sent = segments[0].start;
    assert!(
        (sent - 0.20).abs() < 1e-6,
        "the child's stdin carried {sent}, not 0.20"
    );
    pool.shutdown();
}

/// A successful-looking line that carries no `segments` key at all is a
/// PROTOCOL failure, not "the meeting was silent" — silence is `Some(vec![])`,
/// and the two must never collapse into an empty transcript nobody questions.
#[test]
fn a_success_carrying_no_segments_is_a_protocol_failure_not_a_silent_meeting() {
    let pool = ready_pool(READY_STUB);
    let (segmentation, embedding) = model_pair();
    assert_eq!(pool.load_models(&segmentation, &embedding), Ok(7));
    assert_eq!(
        pool.diarize(Path::new("/a.wav"), CosineDistance::new(0.35)),
        Err(DiarizeError::Protocol)
    );
    pool.shutdown();
}
