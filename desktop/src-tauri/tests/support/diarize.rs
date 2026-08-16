//! YV124 — the SHIPPED diarize sidecar, resolved and launched from a test, and
//! an honest answer when it cannot produce embeddings.
//!
//! `tests/diarize_sidecar_handshake.rs` already drives the sidecar, and does it
//! deliberately by hand: that file's subject is the WIRE contract, so it speaks
//! the protocol over a raw pipe with its own resolver. What it does not give a
//! caller is the other need — the shipped PARENT client ([`DiarizePool`])
//! pointed at the shipped child with the shipped catalog's models, and a typed
//! reason when there is no backend to embed with.
//!
//! That distinction matters for an eval arm. A gate that quietly returns early
//! whenever anything is missing measures nothing and says nothing; [`embedder`]
//! returns exactly one of two states, and only one of them is a skip. A spawn
//! failure, a protocol error or a wedged child are **not** among them — those
//! panic, because they are bugs in the thing under test rather than a machine
//! that has no model on it.
//!
//! **This file lost a state when YV122 merged, and that is the point of the
//! item.** It carried a third variant, `NoBackend`, for the era when
//! `yap-diarize` was YV121's scaffold and answered `load_models` with
//! `no_backend` on every machine on earth. YV122 compiled sherpa-onnx into the
//! sidecar and RETIRED that tag from `diarize_protocol.rs` outright, so the
//! variant is deleted rather than left unreachable: the only honest way to
//! have no embedder now is not to have the model files, and that is one state,
//! not two. The rebase turned the missing constant into a compile error, which
//! is exactly the behaviour YV122 was aiming for when it spent the tag.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use wilson_voice_lib::diarize::{DiarizeError, DiarizeLauncher, DiarizePool};
use wilson_voice_lib::diarize_protocol::ERR_MODEL_NOT_FOUND;
use wilson_voice_lib::models::{
    diarize_model_for_role, diarize_model_path, is_diarize_downloaded, DiarizeModelRole,
};

/// Readiness budget for a spawned sidecar. The parent's own `READY_BUDGET` is
/// private to `diarize.rs`; this is the same 10 s, and it bounds a process
/// start rather than a model load (the child announces readiness before it
/// opens anything — see `diarize_protocol.rs`).
const READY_BUDGET: Duration = Duration::from_secs(10);

/// Override for the sidecar binary, same variable name the handshake test uses.
const BIN_ENV: &str = "YAP_DIARIZE_BIN";

/// The built `yap-diarize`, wherever this checkout has one.
///
/// `tauri.conf.json`'s `bundle.externalBin` makes the staged binary a
/// precondition of this crate COMPILING, so the last branch is not a hope: a
/// tree that produced this test executable has one.
pub fn binary() -> PathBuf {
    if let Ok(named) = std::env::var(BIN_ENV) {
        let path = PathBuf::from(named);
        assert!(path.is_file(), "{BIN_ENV} is not a file: {path:?}");
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
    let staged = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
    std::fs::read_dir(&staged)
        .unwrap_or_else(|e| panic!("no staged sidecars at {staged:?}: {e}"))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("yap-diarize-"))
        })
        .unwrap_or_else(|| {
            panic!(
                "no yap-diarize binary: build one with `cargo build -p yap-diarize` \
                 or point {BIN_ENV} at it"
            )
        })
}

/// A launcher for the real sidecar, in the shape [`DiarizePool`] takes.
pub fn launcher() -> DiarizeLauncher {
    Box::new(|| Ok(Command::new(binary())))
}

/// What a test asking for real embeddings gets back.
///
/// Named for what the caller wants rather than for the sidecar's internals: a
/// test needs an EMBEDDER, and exactly one of these two states is the honest
/// way a machine can fail to have one.
pub enum Embedder {
    /// A real inference backend, with the embedding width the CHILD reported —
    /// never a constant on this side of the wire (`models.rs` explains why that
    /// number has already been wrong twice on paper).
    Ready {
        pool: DiarizePool,
        embedding_dim: u32,
    },
    /// The catalog's diarization models are not installed on this machine.
    /// Carries which one is missing, so the skip line names a file rather than
    /// shrugging.
    ///
    /// Since YV122 this is the ONLY skip. The sidecar always has a backend now;
    /// a machine either has the two model files or it does not.
    ModelsMissing(String),
}

impl Embedder {
    /// The STABLE tag for why there are no embeddings, or `None` when there
    /// are.
    ///
    /// Separate from [`Embedder::skip_reason`] because the two are read by
    /// different audiences. `skip_reason` is prose for a transcript and carries
    /// which model is missing; this is the machine-readable half, and it is
    /// what a machine with no embedder has to name in
    /// `YAP_EER_UNMEASURED_OK` before the anti-alias EER arm will pass.
    ///
    /// Naming the REASON rather than a bare `1` is what stops a declaration
    /// from outliving the state it described. That mechanism has now been
    /// exercised in anger: a stale `export YAP_EER_UNMEASURED_OK=no_backend`
    /// stopped counting the day YV122 landed, because `no_backend` is not a
    /// reason any machine can have any more — `skip_tag` cannot return it, and
    /// `diarize_protocol.rs` no longer defines the tag it was named after.
    pub fn skip_tag(&self) -> Option<&'static str> {
        match self {
            Embedder::Ready { .. } => None,
            Embedder::ModelsMissing(_) => Some("models_missing"),
        }
    }

    /// One line naming why there are no embeddings, or `None` when there are.
    pub fn skip_reason(&self) -> Option<String> {
        match self {
            Embedder::Ready { .. } => None,
            Embedder::ModelsMissing(what) => {
                Some(format!("diarization model not installed: {what}"))
            }
        }
    }
}

/// Spawn the shipped sidecar and hand it the shipped catalog's model pair.
///
/// # Panics
/// On anything that is not one of [`Embedder`]'s two states — a failed spawn,
/// a missed deadline, a garbled response, or a refusal with a tag this function
/// does not know. Those are defects, and a defect that presents as a skipped
/// eval arm is worse than a red test. `model_load_failed` is deliberately among
/// them: since YV122 a build that cannot open the catalog's own pinned models
/// is a broken install, not a machine without them.
pub fn embedder() -> Embedder {
    let mut paths = Vec::new();
    for role in [DiarizeModelRole::Segmentation, DiarizeModelRole::Embedding] {
        let Some(model) = diarize_model_for_role(role) else {
            return Embedder::ModelsMissing(format!("{role:?}: no catalog entry"));
        };
        if !is_diarize_downloaded(model) {
            return Embedder::ModelsMissing(format!("{} ({role:?})", model.id));
        }
        paths.push(diarize_model_path(model));
    }

    let pool = DiarizePool::new(launcher(), READY_BUDGET);
    match pool.load_models(&paths[0], &paths[1]) {
        Ok(embedding_dim) => Embedder::Ready {
            pool,
            embedding_dim,
        },
        Err(DiarizeError::Refused(tag)) if tag == ERR_MODEL_NOT_FOUND => {
            pool.shutdown();
            // The catalog said the files are there and the child says they are
            // not. That is a real disagreement worth naming, not a shrug.
            Embedder::ModelsMissing(format!(
                "the child could not open {:?} / {:?}",
                paths[0], paths[1]
            ))
        }
        Err(other) => {
            pool.shutdown();
            panic!(
                "yap-diarize load_models failed in a way that is not a skip: {other:?} \
                 (status {:?})",
                pool.status()
            )
        }
    }
}
