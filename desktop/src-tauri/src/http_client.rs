//! yap-byom-0 — the ONE outbound HTTP client factory for a destination the
//! user chose: a bring-your-own-model provider (yap-byom-0/1, the deferred
//! yap-byom-2) and, later, the res-0 webhook. Nothing else in the app may build
//! a client for those calls.
//!
//! **No redirect is ever followed.** reqwest 0.13.5's default policy is
//! `limited(10)` (redirect.rs:161) and across hosts it strips only
//! `Authorization` and `Cookie` (:239-249). A 307/308 from a provider would
//! therefore re-POST the meeting transcript to a host the "Leaves this Mac"
//! label never named, carrying `x-api-key` with it. So every client is built
//! with `Policy::none()` and a 3xx answer ends the call as
//! [`HttpError::Redirected`] — "redirected to <host> — not followed".
//!
//! RULE for any later change: nothing may follow a redirect. If one ever does,
//! it must strip `x-api-key`, `api-key`, `Authorization` and
//! `anthropic-workspace-id` across hosts. `tests/http_client_no_redirect.rs`
//! is the outbound allowlist test: the second host must receive ZERO
//! connections.
//!
//! **One keep-alive client per provider** ([`ClientCache`]): a cold TLS
//! handshake inside a short Test-connection deadline loses, so the client (and
//! its connection pool) is built once per provider and reused.
//!
//! Response bodies of a failed call are DROPPED, never logged and never put in
//! an error: some servers echo the submitted key back in a 401 body.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

// The only `reqwest` import for user-chosen destinations. Listed by name in
// tests/no_outbound_on_the_dictation_path.rs (NETWORK_CALL_SITES) and PRIVACY.md.
use reqwest::header::LOCATION;
use reqwest::redirect::Policy;
use reqwest::{Client, Url};

/// Why an outbound call failed, in words a person can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    /// Could not connect at all (DNS, refused, TLS).
    Unreachable { host: String },
    /// No answer inside the deadline.
    Timeout { secs: u64 },
    /// The server answered 3xx; the redirect was NOT followed.
    Redirected { host: String },
    /// Any other non-2xx status. The body is deliberately not kept.
    Status { code: u16 },
    /// The URL could not be parsed, or the client could not be built.
    BadRequest(String),
    /// The body could not be read.
    BadBody,
}

impl HttpError {
    pub fn plain(&self) -> String {
        match self {
            HttpError::Unreachable { host } => format!("{host} is unreachable"),
            HttpError::Timeout { secs } => format!("no answer within {secs} s"),
            HttpError::Redirected { host } => format!("redirected to {host} — not followed"),
            HttpError::Status { code } => format!("the server answered {code}"),
            HttpError::BadRequest(why) => format!("bad request: {why}"),
            HttpError::BadBody => "bad response".to_string(),
        }
    }
}

/// A 2xx answer.
#[derive(Debug, Clone)]
pub struct HttpReply {
    pub status: u16,
    pub body: String,
}

/// A built client. The only way to get one is [`build_client`].
#[derive(Clone)]
pub struct HttpClient {
    inner: Client,
    timeout: Duration,
}

/// Build the client every BYOM / destination call goes through:
/// `redirect::Policy::none()`, a whole-request timeout, keep-alive pooling.
pub fn build_client(timeout: Duration) -> Result<HttpClient, HttpError> {
    let inner = Client::builder()
        .redirect(Policy::none())
        .timeout(timeout)
        .connect_timeout(timeout.min(Duration::from_secs(10)))
        .pool_idle_timeout(Duration::from_secs(90))
        .build()
        .map_err(|e| {
            log::error!("http_client build failed error={e}");
            HttpError::BadRequest("the HTTP client could not be built".into())
        })?;
    Ok(HttpClient { inner, timeout })
}

/// The bare host of `url`, lowercased, without port, path or query — what a
/// Keychain account and a "Leaves this Mac" label are bound to.
pub fn host_of(url: &str) -> Option<String> {
    let u = Url::parse(url.trim()).ok()?;
    if u.scheme() != "http" && u.scheme() != "https" {
        return None;
    }
    u.host_str().filter(|h| !h.is_empty()).map(|h| {
        h.trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase()
    })
}

/// The host (and port, when it is not the scheme's default) of `url`, or
/// `None` when it does not parse. Never the path or the query.
pub fn host_label(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    let host = u.host_str()?.to_string();
    Some(match u.port() {
        Some(p) => format!("{host}:{p}"),
        None => host,
    })
}

/// POST a JSON body and return the 2xx answer. `headers` are added verbatim
/// (an auth header among them is never logged). A 3xx is
/// [`HttpError::Redirected`] and is NOT followed.
pub async fn post_json(
    client: &HttpClient,
    url: &str,
    headers: &[(&str, &str)],
    body: &serde_json::Value,
) -> Result<HttpReply, HttpError> {
    let parsed = Url::parse(url).map_err(|_| HttpError::BadRequest("invalid URL".into()))?;
    let host = host_label(url).unwrap_or_else(|| "the server".into());
    let started = Instant::now();
    log::info!("http_client post start host={host}");
    let mut req = client.inner.post(parsed.clone()).json(body);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = req.send().await.map_err(|e| {
        let err = if e.is_timeout() {
            HttpError::Timeout {
                secs: client.timeout.as_secs(),
            }
        } else {
            HttpError::Unreachable { host: host.clone() }
        };
        log::warn!(
            "http_client post failed host={host} reason=\"{}\" ms={}",
            err.plain(),
            started.elapsed().as_millis()
        );
        err
    })?;
    let status = resp.status();
    if status.is_redirection() {
        let target = resp
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|loc| parsed.join(loc).ok())
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_else(|| "another address".into());
        log::warn!(
            "http_client post redirect_refused host={host} status={} target={target}",
            status.as_u16()
        );
        return Err(HttpError::Redirected { host: target });
    }
    if !status.is_success() {
        log::warn!(
            "http_client post finish host={host} status={} ms={}",
            status.as_u16(),
            started.elapsed().as_millis()
        );
        return Err(HttpError::Status {
            code: status.as_u16(),
        });
    }
    let body = resp.text().await.map_err(|_| HttpError::BadBody)?;
    log::info!(
        "http_client post finish host={host} status={} ms={}",
        status.as_u16(),
        started.elapsed().as_millis()
    );
    Ok(HttpReply {
        status: status.as_u16(),
        body,
    })
}

/// [`post_json`] for a synchronous caller. Must not be called from inside an
/// async task (the Tauri command runs it on a blocking thread).
pub fn post_json_blocking(
    client: &HttpClient,
    url: &str,
    headers: &[(&str, &str)],
    body: &serde_json::Value,
) -> Result<HttpReply, HttpError> {
    tauri::async_runtime::block_on(post_json(client, url, headers, body))
}

/// One keep-alive [`HttpClient`] per provider id, rebuilt only when that
/// provider's timeout changes.
#[derive(Default)]
pub struct ClientCache {
    clients: Mutex<HashMap<String, HttpClient>>,
}

impl ClientCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn client_for(
        &self,
        provider_id: &str,
        timeout: Duration,
    ) -> Result<HttpClient, HttpError> {
        let mut map = self
            .clients
            .lock()
            .map_err(|_| HttpError::BadRequest("client cache poisoned".into()))?;
        if let Some(c) = map.get(provider_id) {
            if c.timeout == timeout {
                return Ok(c.clone());
            }
        }
        let c = build_client(timeout)?;
        map.insert(provider_id.to_string(), c.clone());
        Ok(c)
    }

    /// Forget a provider's client (it was deleted or its host changed).
    pub fn forget(&self, provider_id: &str) {
        if let Ok(mut map) = self.clients.lock() {
            map.remove(provider_id);
        }
    }
}
