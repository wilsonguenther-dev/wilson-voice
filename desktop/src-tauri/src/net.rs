//! yap-notetaker-notes — the ONE door out of the app for HTTP.
//!
//! Every outbound HTTP request the Rust side makes gets its client from
//! [`http_client`], tagged with a `purpose`. Today that is exactly one
//! purpose — `model-download` in `models::download_file_cancellable`, the
//! catalog fetch of a speech / notes / diarize model — and nothing on the
//! meeting-notes path asks for one at all.
//!
//! The point of routing it through one function is that "meeting notes stay
//! on this Mac" becomes a claim a test can CHECK instead of a sentence in a
//! doc: `tests/meeting_notes_stay_local.rs` installs [`DenyAll`], which panics
//! on the first request, and runs the real summary path end to end. A future
//! change that sends a transcript anywhere has to come through here, and the
//! test fails the moment it does.
//!
//! Counted ([`egress_requests`]) so a test can also assert the positive: the
//! seam was reached when a download really was needed — a seam nothing calls
//! would make the privacy test vacuous.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

/// Builds the HTTP client for one outbound purpose.
pub trait HttpEgress: Send + Sync {
    fn egress_client(&self, purpose: &str) -> Result<reqwest::Client, String>;
}

/// The shipping egress: a plain `reqwest` client.
pub struct Direct;

impl HttpEgress for Direct {
    fn egress_client(&self, _purpose: &str) -> Result<reqwest::Client, String> {
        Ok(reqwest::Client::new())
    }
}

/// The privacy test's egress: any request at all is a bug, so it panics with
/// the purpose that asked.
pub struct DenyAll;

impl HttpEgress for DenyAll {
    fn egress_client(&self, purpose: &str) -> Result<reqwest::Client, String> {
        panic!("net: network egress denied purpose={purpose}");
    }
}

static EGRESS: OnceLock<RwLock<Arc<dyn HttpEgress>>> = OnceLock::new();
static REQUESTS: AtomicU64 = AtomicU64::new(0);

fn slot() -> &'static RwLock<Arc<dyn HttpEgress>> {
    EGRESS.get_or_init(|| RwLock::new(Arc::new(Direct)))
}

/// Replace the process's egress. Tests only in practice; the app never calls
/// it, so it ships with [`Direct`].
pub fn install(egress: Arc<dyn HttpEgress>) {
    *slot().write().unwrap_or_else(|p| p.into_inner()) = egress;
}

/// How many times anything asked for an HTTP client in this process.
pub fn egress_requests() -> u64 {
    REQUESTS.load(Ordering::SeqCst)
}

/// The HTTP client for `purpose`. Logged once per request so a support log
/// shows every time the app reached for the network and why.
pub fn http_client(purpose: &str) -> Result<reqwest::Client, String> {
    let n = REQUESTS.fetch_add(1, Ordering::SeqCst) + 1;
    log::info!("net egress start purpose={purpose} count={n}");
    let egress = Arc::clone(&slot().read().unwrap_or_else(|p| p.into_inner()));
    egress.egress_client(purpose).inspect_err(|e| {
        log::warn!("net egress failure purpose={purpose} reason={e}");
    })
}
