//! yap-1106-lic-03 (spec `yap-lic-02-activation-client`) — the activation
//! client for drivia.consulting licensing.
//!
//! A pasted `DCL-` license KEY is sent ONCE to
//! `POST {LICENSING_BASE}/activate` with `X-Drivia-App: <APP_KEY>`. The token
//! in the 200 is handed to `LicenseManager::install_token`, which verifies it
//! OFFLINE with `license_v2` (EMBEDDED_KEYS only, kid `dcl-2026-09-24`; the
//! 30-day grace is `license_v2::GRACE_S`) before anything is written. From then
//! on entitlement is local: the network is only touched to activate, to
//! refresh a token that is close to `exp` (at most once per 24 h), and to free
//! the seat on deactivate.
//!
//! What this module never does (Panel 2026-10-19, BLOCKING): fetch a key list,
//! add a key to the trusted set, or take a host from anywhere but
//! [`LICENSING_BASE`]. Release code has no runtime base override; only the
//! `#[cfg(test)]` code in this file builds a client on a stub server's base.
//!
//! Responses (drivia-consulting docs/licensing/CLIENT-INTEGRATION.md):
//! * 200 stores the token (after offline verification) and lifts any hold.
//! * FAIL CLOSED — 403 revoked / expired / deactivated and 401
//!   reactivate_required / device_mismatch delete the token and show why.
//! * 403 disputed keeps the token but is NOT entitled; the hold is persisted
//!   (license.json + SQLite) and only a 200 lifts it. A held license refreshes
//!   on every launch.
//! * 403 blocked, 400, 413, 401 invalid_token / unknown_app, 5xx, timeouts and
//!   offline never downgrade a stored token. 429 honours `Retry-After`.
//! * Deactivate frees the seat server-side BEFORE clearing locally.
//!
//! Telemetry: one `license_client <event> key=value` line per action and
//! failure, with a duration. Never the key, the token, the device id or the
//! device name.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::keychain::SecretStore;
use crate::license::{LicenseManager, LicenseStatus, LicenseTransport, TransportReply};
use crate::license_v2::TokenError;

/// The ONE licensing host. The custom domain only: every Vercel-assigned host
/// sits behind Vercel Authentication and answers 302/401 to apps
/// (client-config.json `forbiddenHostPatterns`).
pub const LICENSING_BASE: &str = "https://drivia.consulting/api/v1/licensing";

/// Yap's public app key (`X-Drivia-App`), copied from drivia-consulting
/// docs/licensing/client-config.json `apps.yap.appKey`. Routing, not a secret.
pub const APP_KEY: &str = "dca_1d3c77f3b36954d84955c8e6";

/// Activation accepts drivia.consulting license keys only.
pub const KEY_PREFIX: &str = "DCL-";

/// Keychain account of the install UUID (service: [`keychain_service_for`]).
pub const DEVICE_ID_ACCOUNT: &str = "device_id";

/// Appended to the running bundle id to name the licensing Keychain service.
pub const KEYCHAIN_SERVICE_SUFFIX: &str = ".license";

/// A refresh that got an answer is not repeated inside this window…
pub const REFRESH_MIN_INTERVAL_MS: i64 = 24 * 60 * 60 * 1000;

/// …and no refresh is attempted while more than this remains before `exp`.
pub const REFRESH_WHEN_UNDER_S: i64 = 7 * 24 * 60 * 60;

/// One licensing request never waits longer than this.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// `429` without a usable `Retry-After`: wait an hour.
pub const DEFAULT_RETRY_AFTER_S: u64 = 60 * 60;

/// Replies above this size are refused unread (the API answers in < 4 KB).
const MAX_REPLY_BYTES: usize = 64 * 1024;

/// The licensing Keychain service for a running bundle id.
pub fn keychain_service_for(bundle_id: &str) -> String {
    format!("{}{KEYCHAIN_SERVICE_SUFFIX}", bundle_id.trim())
}

// ─── Errors a human sees ─────────────────────────────────────────────

/// One device holding a seat (409 seat_limit).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatDevice {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub last_seen_at: Option<String>,
}

/// A refused activation / deactivation: a stable `code` and ONE plain
/// sentence for the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClientError {
    pub code: String,
    pub message: String,
    /// 409 seat_limit only: the Macs that hold the seats.
    pub devices: Vec<SeatDevice>,
}

impl ClientError {
    fn new(code: &str) -> Self {
        Self {
            code: code.to_string(),
            message: message_for(code).to_string(),
            devices: Vec::new(),
        }
    }
}

/// The one sentence shown for each refusal code.
pub fn message_for(code: &str) -> &'static str {
    match code {
        "bad_key_format" => "That is not a Yap license key, which starts with DCL- and comes in your receipt email.",
        "invalid_key" => "That license key was not recognised, so check it against your receipt email and paste it again.",
        "seat_limit" => "This license is already active on all of its Macs, so free one under Manage Macs at drivia.consulting and try again.",
        "revoked" => "This license was revoked, so Yap needs a new license key to keep dictating.",
        "expired" => "This license has expired, so paste a current license key to keep dictating.",
        "deactivated" => "This Mac was removed from your license, so paste your license key again to reactivate it.",
        "reactivate_required" | "device_mismatch" | "unknown_kid" => {
            "Yap needs you to paste your license key again to reactivate this Mac."
        }
        "disputed" => "This license is on hold while a payment question is reviewed, so dictation is paused for now.",
        "blocked" => "This Mac cannot be activated on this license, so contact support at drivia.consulting.",
        "rate_limited" => "There were too many license attempts just now, so wait a few minutes and try again.",
        "invalid_request" | "unknown_app" => {
            "The licensing service could not read Yap's request, so update Yap and try again."
        }
        "bad_token" => "The license Yap received could not be verified on this Mac, so update Yap and try again.",
        "offline" => "Yap could not reach drivia.consulting, so connect to the internet once and try again.",
        "not_activated" => "This Mac has no drivia.consulting license to remove.",
        _ => "The licensing service is having trouble right now, so try again in a few minutes.",
    }
}

/// 409 seat_limit, naming the Macs that hold the seats (at most three names).
fn seat_limit_message(devices: &[SeatDevice]) -> String {
    let names: Vec<&str> = devices
        .iter()
        .map(|d| d.name.trim())
        .filter(|n| !n.is_empty())
        .take(3)
        .collect();
    if names.is_empty() {
        return message_for("seat_limit").to_string();
    }
    format!(
        "This license is already active on {}, so free one under Manage Macs at drivia.consulting and try again.",
        names.join(", ")
    )
}

/// Codes after which the seat or license is gone: the token is deleted.
fn fails_closed(status: u16, code: &str) -> bool {
    matches!(
        (status, code),
        (403, "revoked")
            | (403, "expired")
            | (403, "deactivated")
            | (401, "reactivate_required")
            | (401, "device_mismatch")
    )
}

// ─── Key + request shape ─────────────────────────────────────────────

/// Trim and upper-case a pasted key; refuse anything but a `DCL-` key.
pub fn normalize_key(raw: &str) -> Result<String, ClientError> {
    let key = raw.trim().to_ascii_uppercase();
    let ok = key.starts_with(KEY_PREFIX)
        && (KEY_PREFIX.len() + 4..=96).contains(&key.len())
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if ok {
        Ok(key)
    } else {
        Err(ClientError::new("bad_key_format"))
    }
}

/// True when the paste is meant for this client (a `DCL-` key), so the
/// command can route it here instead of the local token path.
pub fn is_dcl_key(raw: &str) -> bool {
    raw.trim().to_ascii_uppercase().starts_with(KEY_PREFIX)
}

fn device_name() -> String {
    let name = std::process::Command::new("/usr/sbin/scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Mac".to_string());
    name.chars().take(64).collect()
}

fn valid_device_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id.trim()).is_ok()
}

#[derive(Debug, Default, Deserialize)]
struct ApiBody {
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    devices: Vec<SeatDevice>,
}

fn parse_body(reply: &TransportReply) -> ApiBody {
    serde_json::from_slice(&reply.body).unwrap_or_default()
}

// ─── Refresh schedule (pure) ─────────────────────────────────────────

/// Is a refresh due? Pure, so the rule is tested without a clock or a server.
///
/// * never inside a `429 Retry-After` window;
/// * a HELD (403 disputed) license: every launch;
/// * otherwise only when fewer than [`REFRESH_WHEN_UNDER_S`] remain before
///   `exp` (this includes the grace period), and then at most once per
///   [`REFRESH_MIN_INTERVAL_MS`]. A fresh token is never refreshed, so a
///   launch with a fresh token makes no licensing request at all.
pub fn refresh_due(
    exp_s: i64,
    last_refresh_at_ms: Option<i64>,
    retry_after_until_ms: Option<i64>,
    held: bool,
    now_ms: i64,
) -> bool {
    if retry_after_until_ms.is_some_and(|until| now_ms < until) {
        return false;
    }
    if held {
        return true;
    }
    let remaining_s = exp_s.saturating_sub(now_ms.div_euclid(1000));
    if remaining_s >= REFRESH_WHEN_UNDER_S {
        return false;
    }
    last_refresh_at_ms.is_none_or(|t| now_ms.saturating_sub(t) >= REFRESH_MIN_INTERVAL_MS)
}

/// What a launch-time refresh check did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshOutcome {
    /// No stored drivia.consulting token.
    NoToken,
    /// The token is fresh (or refreshed inside 24 h, or inside Retry-After):
    /// no request was made.
    NotDue,
    /// 200: a new token verified offline and stored; any hold lifted.
    Refreshed,
    /// The service said the seat / license is gone; the token was deleted.
    Deleted(String),
    /// 403 disputed: token kept, hold persisted, not entitled.
    Held,
    /// Anything else (offline, 5xx, 429, 400, 413, 403 blocked, 401
    /// invalid_token): the stored token is kept as it was.
    Kept(String),
}

impl RefreshOutcome {
    /// Did entitlement possibly change (so the UI should re-read status)?
    pub fn changes_status(&self) -> bool {
        matches!(self, Self::Refreshed | Self::Deleted(_) | Self::Held)
    }
}

// ─── The client ──────────────────────────────────────────────────────

/// Activate / refresh / deactivate against [`LICENSING_BASE`].
pub struct LicenseClient {
    /// Private: [`LicenseClient::new`] always passes [`LICENSING_BASE`].
    base: String,
    manager: Arc<LicenseManager>,
    transport: Arc<dyn LicenseTransport>,
    secrets: Arc<dyn SecretStore>,
    /// One licensing request at a time (a double-click is one activation).
    in_flight: parking_lot::Mutex<()>,
}

impl LicenseClient {
    /// The production client: [`LICENSING_BASE`], no override.
    pub fn new(
        manager: Arc<LicenseManager>,
        transport: Arc<dyn LicenseTransport>,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        Self::build(LICENSING_BASE, manager, transport, secrets)
    }

    fn build(
        base: &str,
        manager: Arc<LicenseManager>,
        transport: Arc<dyn LicenseTransport>,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            manager,
            transport,
            secrets,
            in_flight: parking_lot::Mutex::new(()),
        }
    }

    pub fn status(&self) -> LicenseStatus {
        self.manager.status()
    }

    /// The install UUID: Keychain first, then the license.json mirror (so a
    /// Keychain failure re-activates the SAME seat), else a new one. Written
    /// back to both. A Keychain failure is logged, never a lock-out.
    pub fn device_id(&self) -> String {
        let from_keychain = match self.secrets.get(DEVICE_ID_ACCOUNT) {
            Ok(Some(id)) if valid_device_id(&id) => Some(id.trim().to_string()),
            Ok(_) => None,
            Err(e) => {
                log::warn!("license_client keychain_read failure reason={e}");
                None
            }
        };
        let id = match from_keychain {
            Some(id) => id,
            None => {
                let mirrored = self
                    .manager
                    .client_record()
                    .device_id
                    .filter(|id| valid_device_id(id));
                let source = if mirrored.is_some() { "mirror" } else { "new" };
                let id = mirrored.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                log::info!("license_client device_id source={source}");
                if let Err(e) = self.secrets.set(DEVICE_ID_ACCOUNT, &id) {
                    log::warn!("license_client keychain_write failure reason={e}");
                }
                id
            }
        };
        self.manager.set_device_id_mirror(&id);
        id
    }

    fn post(&self, endpoint: &str, body: &serde_json::Value) -> Result<TransportReply, String> {
        let url = format!("{}/{endpoint}", self.base);
        let bytes = serde_json::to_vec(body).map_err(|e| e.to_string())?;
        self.transport.post_json(&url, APP_KEY, &bytes)
    }

    fn note_retry_after(&self, reply: &TransportReply) {
        let secs = reply
            .retry_after_s
            .unwrap_or(DEFAULT_RETRY_AFTER_S)
            .min(24 * 60 * 60);
        let until = self.manager.now_ms().saturating_add((secs as i64) * 1000);
        self.manager.set_retry_after_until(Some(until));
    }

    /// Store a token from a 200, after offline verification. A token that does
    /// not verify (e.g. an unknown kid: this build needs an update) is refused.
    fn store_token(&self, reply: &TransportReply) -> Result<LicenseStatus, TokenError> {
        let token = parse_body(reply).token.unwrap_or_default();
        let status = self.manager.install_token(&token)?;
        self.manager.set_disputed_hold(None);
        self.manager.set_retry_after_until(None);
        self.manager.note_refresh(self.manager.now_ms());
        Ok(status)
    }

    /// Activate a pasted `DCL-` key: exactly ONE `POST /activate`.
    pub fn activate(&self, raw_key: &str) -> Result<LicenseStatus, ClientError> {
        let started = Instant::now();
        let key = normalize_key(raw_key).inspect_err(|_| {
            log::info!("license_client activate refused code=bad_key_format requests=0");
        })?;
        let _one = self.in_flight.lock();
        let rec = self.manager.client_record();
        if rec
            .retry_after_until_ms
            .is_some_and(|until| self.manager.now_ms() < until)
        {
            log::info!("license_client activate refused code=rate_limited requests=0");
            return Err(ClientError::new("rate_limited"));
        }
        let body = serde_json::json!({
            "license_key": key,
            "device_id": self.device_id(),
            "device_name": device_name(),
            "platform": "macos",
            "os_version": crate::support::os_version(),
            "app_version": env!("CARGO_PKG_VERSION"),
        });
        log::info!("license_client activate start");
        let reply = self.post("activate", &body);
        let dur_ms = started.elapsed().as_millis();
        let reply = match reply {
            Ok(r) => r,
            Err(e) => {
                log::warn!(
                    "license_client activate failure code=offline reason={e} dur_ms={dur_ms}"
                );
                return Err(ClientError::new("offline"));
            }
        };
        let api = parse_body(&reply);
        let code = api.error.clone().unwrap_or_default();
        let result = match reply.status {
            200 => self.store_token(&reply).map_err(|e| {
                log::warn!("license_client activate token_refused code={}", e.code());
                ClientError::new("bad_token")
            }),
            404 => Err(ClientError::new("invalid_key")),
            409 => Err(ClientError {
                code: "seat_limit".into(),
                message: seat_limit_message(&api.devices),
                devices: api.devices.clone(),
            }),
            403 | 401 if fails_closed(reply.status, &code) => {
                // A re-activation the service refuses outright also ends any
                // older token on this Mac.
                self.manager.drop_token();
                Err(ClientError::new(&code))
            }
            403 if code == "disputed" => Err(ClientError {
                code: "disputed".into(),
                message: api
                    .message
                    .clone()
                    .filter(|m| !m.trim().is_empty())
                    .unwrap_or_else(|| message_for("disputed").to_string()),
                devices: Vec::new(),
            }),
            403 => Err(ClientError::new("blocked")),
            429 => {
                self.note_retry_after(&reply);
                Err(ClientError::new("rate_limited"))
            }
            400 | 413 => Err(ClientError::new("invalid_request")),
            401 => Err(ClientError::new("unknown_app")),
            _ => Err(ClientError::new("server")),
        };
        match &result {
            Ok(_) => log::info!("license_client activate ok status=200 dur_ms={dur_ms}"),
            Err(e) => log::warn!(
                "license_client activate refused status={} code={} dur_ms={dur_ms}",
                reply.status,
                e.code
            ),
        }
        result
    }

    /// The launch-time check: refresh ONLY when [`refresh_due`] says so.
    pub fn refresh_if_due(&self) -> RefreshOutcome {
        self.refresh(false)
    }

    fn refresh(&self, force: bool) -> RefreshOutcome {
        let _one = self.in_flight.lock();
        let rec = self.manager.client_record();
        let Some(token) = rec.token.clone() else {
            return RefreshOutcome::NoToken;
        };
        let claims = match self.manager.stored_token_claims() {
            Some(Ok(c)) => c,
            Some(Err(TokenError::UnknownKid)) => {
                // Never fetch a key: an unknown kid means re-activate.
                log::warn!(
                    "license_client refresh token_refused code=unknown_kid action=reactivate"
                );
                self.manager.drop_token();
                return RefreshOutcome::Deleted("unknown_kid".into());
            }
            Some(Err(e)) => {
                // Past grace / rolled back / malformed: the service would
                // refuse it too (401 invalid_token). Status already shows why.
                log::info!("license_client refresh skipped code={}", e.code());
                return RefreshOutcome::NotDue;
            }
            None => return RefreshOutcome::NoToken,
        };
        let now = self.manager.effective_now_ms();
        let held = rec.disputed_hold.is_some();
        if !force
            && !refresh_due(
                claims.exp,
                rec.last_refresh_at_ms,
                rec.retry_after_until_ms,
                held,
                now,
            )
        {
            return RefreshOutcome::NotDue;
        }
        let started = Instant::now();
        let body = serde_json::json!({
            "token": token,
            "device_id": self.device_id(),
            "app_version": env!("CARGO_PKG_VERSION"),
            "os_version": crate::support::os_version(),
        });
        log::info!("license_client refresh start held={held}");
        let reply = self.post("refresh", &body);
        let dur_ms = started.elapsed().as_millis();
        let reply = match reply {
            Ok(r) => r,
            Err(e) => {
                // Never downgrade a user because our server is unreachable.
                log::warn!(
                    "license_client refresh failure code=offline reason={e} dur_ms={dur_ms}"
                );
                return RefreshOutcome::Kept("offline".into());
            }
        };
        let api = parse_body(&reply);
        let code = api.error.clone().unwrap_or_default();
        let outcome = match reply.status {
            200 => match self.store_token(&reply) {
                Ok(_) => RefreshOutcome::Refreshed,
                Err(e) => {
                    log::warn!("license_client refresh token_refused code={}", e.code());
                    RefreshOutcome::Kept("bad_token".into())
                }
            },
            403 | 401 if fails_closed(reply.status, &code) => {
                self.manager.drop_token();
                RefreshOutcome::Deleted(code.clone())
            }
            403 if code == "disputed" => {
                let message = api
                    .message
                    .clone()
                    .filter(|m| !m.trim().is_empty())
                    .unwrap_or_else(|| message_for("disputed").to_string());
                self.manager.set_disputed_hold(Some(message));
                self.manager.note_refresh(self.manager.now_ms());
                RefreshOutcome::Held
            }
            429 => {
                self.note_retry_after(&reply);
                RefreshOutcome::Kept("rate_limited".into())
            }
            400 | 401 | 403 | 413 => {
                // A client-side problem or a block: the token stays, and the
                // 24 h rule keeps this from repeating every launch.
                self.manager.note_refresh(self.manager.now_ms());
                RefreshOutcome::Kept(if code.is_empty() {
                    format!("http_{}", reply.status)
                } else {
                    code.clone()
                })
            }
            _ => RefreshOutcome::Kept(format!("http_{}", reply.status)),
        };
        log::info!(
            "license_client refresh done status={} outcome={outcome:?} dur_ms={dur_ms}",
            reply.status
        );
        outcome
    }

    /// Free the seat server-side FIRST, then clear locally. Offline / 5xx /
    /// 429 leave everything as it was (the seat is still taken). A Mac with no
    /// drivia.consulting token (a v1 key, or nothing) clears locally only.
    pub fn deactivate(&self) -> Result<LicenseStatus, ClientError> {
        let started = Instant::now();
        let _one = self.in_flight.lock();
        let rec = self.manager.client_record();
        if let Some(token) = rec.token {
            log::info!("license_client deactivate start");
            let reply = self.post("deactivate", &serde_json::json!({ "token": token }));
            let dur_ms = started.elapsed().as_millis();
            match reply {
                // 401: the service no longer knows this token, so there is no
                // seat left to free; clearing locally is correct.
                Ok(r) if r.status == 200 || r.status == 401 => {
                    log::info!(
                        "license_client deactivate ok status={} dur_ms={dur_ms}",
                        r.status
                    )
                }
                Ok(r) if r.status == 429 => {
                    self.note_retry_after(&r);
                    log::warn!("license_client deactivate refused status=429 dur_ms={dur_ms}");
                    return Err(ClientError::new("rate_limited"));
                }
                Ok(r) => {
                    log::warn!(
                        "license_client deactivate refused status={} dur_ms={dur_ms}",
                        r.status
                    );
                    return Err(ClientError::new("server"));
                }
                Err(e) => {
                    log::warn!(
                        "license_client deactivate failure code=offline reason={e} dur_ms={dur_ms}"
                    );
                    return Err(ClientError::new("offline"));
                }
            }
        } else {
            log::info!("license_client deactivate local_only requests=0");
        }
        self.manager.deactivate();
        Ok(self.manager.status())
    }
}

// ─── The HTTP transport ──────────────────────────────────────────────

/// The `LicenseTransport` that reaches the licensing service, through the
/// app's one egress door (`net::http_client("licensing")`). POST only: there
/// is no GET, so there is no way to fetch a key list.
pub struct HttpTransport;

impl LicenseTransport for HttpTransport {
    fn get(&self, _url: &str) -> Result<Vec<u8>, String> {
        Err("license_client makes no GET request".into())
    }

    fn post_json(&self, url: &str, app_key: &str, body: &[u8]) -> Result<TransportReply, String> {
        let client = crate::net::http_client("licensing")?;
        let request = client
            .post(url)
            .header("X-Drivia-App", app_key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .timeout(REQUEST_TIMEOUT)
            .body(body.to_vec());
        // Called from a blocking thread (commands use spawn_blocking; the
        // launch check runs on its own thread), never on a runtime worker.
        tauri::async_runtime::block_on(async move {
            let response = request.send().await.map_err(|e| {
                if e.is_timeout() {
                    "timeout".to_string()
                } else {
                    "connect".to_string()
                }
            })?;
            let status = response.status().as_u16();
            let retry_after_s = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse::<u64>().ok());
            let bytes = response.bytes().await.map_err(|_| "read".to_string())?;
            if bytes.len() > MAX_REPLY_BYTES {
                return Err("reply_too_large".to_string());
            }
            Ok::<TransportReply, String>(TransportReply {
                status,
                retry_after_s,
                body: bytes.to_vec(),
            })
        })
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keychain::MemoryKeychain;
    use crate::license::{Entitlement, LicenseStore, MemoryStore};
    use base64::Engine;
    use ed25519_dalek::{Signer, SigningKey};
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::path::{Path, PathBuf};

    const DAY_S: i64 = 24 * 60 * 60;
    const TEST_KID: &str = "test-kid-lic03";

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let p = std::env::temp_dir().join(format!("yap-lic03-{tag}-{nanos}"));
            std::fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn wall_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
    }

    fn test_key() -> SigningKey {
        SigningKey::from_bytes(&[42u8; 32])
    }

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }

    /// A token signed by the throwaway key, `iat`/`exp` relative to now.
    fn mint(iat_from_now_s: i64, exp_from_now_s: i64) -> String {
        let now_s = wall_ms() / 1000;
        let header = serde_json::json!({"alg": "EdDSA", "kid": TEST_KID, "typ": "JWT"});
        let payload = serde_json::json!({
            "sub": "lic_test", "act": "act_test", "prd": "yap", "plan": "lifetime",
            "ent": {"dictation": true}, "dev": "opaque",
            "iat": now_s + iat_from_now_s, "exp": now_s + exp_from_now_s,
        });
        let input = format!(
            "{}.{}",
            b64(header.to_string().as_bytes()),
            b64(payload.to_string().as_bytes())
        );
        let sig = test_key().sign(input.as_bytes());
        format!("{input}.{}", b64(&sig.to_bytes()))
    }

    fn manager(dir: &Path, store: Arc<dyn LicenseStore>) -> Arc<LicenseManager> {
        let pk = test_key().verifying_key().to_bytes();
        Arc::new(LicenseManager::with_token_verifier(
            dir,
            store,
            Box::new(wall_ms),
            Box::new(move |t: &str, now: i64| {
                crate::license_v2::verify_with(&[(TEST_KID, pk)], t, now)
            }),
        ))
    }

    fn reply(status: u16, body: serde_json::Value) -> Result<TransportReply, String> {
        Ok(TransportReply {
            status,
            retry_after_s: None,
            body: body.to_string().into_bytes(),
        })
    }

    #[derive(Debug, Clone)]
    struct Call {
        url: String,
        app_key: String,
        body: serde_json::Value,
    }

    /// Scripted transport: pops one canned reply per POST, records every call.
    /// A GET is a test failure (there is no key fetch).
    #[derive(Default)]
    struct Scripted {
        replies: parking_lot::Mutex<VecDeque<Result<TransportReply, String>>>,
        calls: parking_lot::Mutex<Vec<Call>>,
    }
    impl Scripted {
        fn push(&self, r: Result<TransportReply, String>) {
            self.replies.lock().push_back(r);
        }
        fn calls(&self) -> Vec<Call> {
            self.calls.lock().clone()
        }
    }
    impl LicenseTransport for Scripted {
        fn get(&self, url: &str) -> Result<Vec<u8>, String> {
            panic!("license_client made a GET to {url}");
        }
        fn post_json(
            &self,
            url: &str,
            app_key: &str,
            body: &[u8],
        ) -> Result<TransportReply, String> {
            self.calls.lock().push(Call {
                url: url.to_string(),
                app_key: app_key.to_string(),
                body: serde_json::from_slice(body).unwrap(),
            });
            self.replies
                .lock()
                .pop_front()
                .unwrap_or_else(|| Err("no scripted reply".into()))
        }
    }

    struct Env {
        dir: TempDir,
        store: Arc<dyn LicenseStore>,
        mgr: Arc<LicenseManager>,
        net: Arc<Scripted>,
        keychain: Arc<MemoryKeychain>,
        client: LicenseClient,
    }

    fn env(tag: &str) -> Env {
        let dir = TempDir::new(tag);
        let store: Arc<dyn LicenseStore> = Arc::new(MemoryStore::default());
        let mgr = manager(dir.path(), store.clone());
        let net = Arc::new(Scripted::default());
        let keychain = Arc::new(MemoryKeychain::new("com.test.yap-lic03.license"));
        let client = LicenseClient::build(
            "http://stub.invalid/api/v1/licensing",
            mgr.clone(),
            net.clone(),
            keychain.clone(),
        );
        Env {
            dir,
            store,
            mgr,
            net,
            keychain,
            client,
        }
    }

    const KEY: &str = "DCL-ABCDE-FGHIJ-KLMNO-PQRST-UVWXY";

    /// Activate with a token whose exp is `exp_from_now_s` away.
    fn activated(tag: &str, exp_from_now_s: i64) -> Env {
        let e = env(tag);
        e.net.push(reply(
            200,
            serde_json::json!({"token": mint(-60, exp_from_now_s)}),
        ));
        e.client.activate(KEY).expect("activation stores the token");
        e
    }

    fn is_licensed(s: &LicenseStatus) -> bool {
        matches!(s.entitlement, Entitlement::Licensed { .. })
    }

    // ── host + key shape ──

    #[test]
    fn licensing_base_is_drivia_consulting_only() {
        assert_eq!(LICENSING_BASE, "https://drivia.consulting/api/v1/licensing");
        assert!(!LICENSING_BASE.contains("vercel"));
        let client = LicenseClient::new(
            env("base").mgr,
            Arc::new(Scripted::default()),
            Arc::new(MemoryKeychain::new("x")),
        );
        assert_eq!(client.base, LICENSING_BASE);
    }

    #[test]
    fn activate_refuses_non_dcl_key_without_request() {
        let e = env("non-dcl");
        let err = e.client.activate("eyJhbGc.eyJzdWIi.c2ln").unwrap_err();
        assert_eq!(err.code, "bad_key_format");
        assert!(e.net.calls().is_empty());
    }

    // ── activation ──

    #[test]
    fn activate_posts_once_with_app_header_and_stores_token() {
        let e = env("activate-200");
        let token = mint(-60, 30 * DAY_S);
        e.net.push(reply(
            200,
            serde_json::json!({"token": token, "plan": "lifetime"}),
        ));
        let status = e
            .client
            .activate(&format!("  {}  ", KEY.to_lowercase()))
            .unwrap();
        assert!(is_licensed(&status), "{status:?}");
        let calls = e.net.calls();
        assert_eq!(calls.len(), 1, "exactly one activation request");
        assert!(calls[0].url.ends_with("/activate"));
        assert_eq!(calls[0].app_key, APP_KEY);
        assert_eq!(calls[0].body["license_key"], KEY);
        assert_eq!(calls[0].body["platform"], "macos");
        let device_id = calls[0].body["device_id"].as_str().unwrap().to_string();
        assert!(valid_device_id(&device_id));
        let rec = e.mgr.client_record();
        assert_eq!(rec.token.as_deref(), Some(token.as_str()));
        assert_eq!(
            rec.device_id.as_deref(),
            Some(device_id.as_str()),
            "device_id mirrored"
        );
        assert_eq!(
            e.keychain.get(DEVICE_ID_ACCOUNT).unwrap().as_deref(),
            Some(device_id.as_str())
        );
    }

    #[test]
    fn activate_refuses_token_that_does_not_verify_offline() {
        let e = env("activate-bad-token");
        let mut forged = mint(-60, 30 * DAY_S);
        forged.push('A');
        e.net.push(reply(200, serde_json::json!({"token": forged})));
        let err = e.client.activate(KEY).unwrap_err();
        assert_eq!(err.code, "bad_token");
        assert!(e.mgr.client_record().token.is_none());
    }

    #[test]
    fn invalid_key_404_shows_one_sentence() {
        let e = env("activate-404");
        e.net
            .push(reply(404, serde_json::json!({"error": "invalid_key"})));
        let err = e.client.activate(KEY).unwrap_err();
        assert_eq!(err.code, "invalid_key");
        assert_eq!(err.message, message_for("invalid_key"));
        assert!(e.mgr.client_record().token.is_none());
    }

    #[test]
    fn seat_limit_409_lists_devices() {
        let e = env("activate-409");
        e.net.push(reply(
            409,
            serde_json::json!({"error": "seat_limit", "devices_used": 3, "devices_max": 3, "devices": [
                {"name": "Studio", "platform": "macos", "last_seen_at": "2026-11-01T00:00:00Z"},
                {"name": "Air", "platform": "macos", "last_seen_at": null},
                {"name": "Mini", "platform": "macos"}
            ]}),
        ));
        let err = e.client.activate(KEY).unwrap_err();
        assert_eq!(err.code, "seat_limit");
        assert_eq!(err.devices.len(), 3);
        assert!(err.message.contains("Studio, Air, Mini"), "{}", err.message);
        assert!(err.message.contains("Manage Macs"));
    }

    #[test]
    fn revoked_403_on_activate_shows_one_sentence() {
        let e = env("activate-revoked");
        e.net
            .push(reply(403, serde_json::json!({"error": "revoked"})));
        let err = e.client.activate(KEY).unwrap_err();
        assert_eq!(err.code, "revoked");
        assert_eq!(err.message, message_for("revoked"));
    }

    #[test]
    fn refusal_messages_are_one_plain_sentence() {
        for code in [
            "bad_key_format",
            "invalid_key",
            "seat_limit",
            "revoked",
            "expired",
            "deactivated",
            "reactivate_required",
            "device_mismatch",
            "disputed",
            "blocked",
            "rate_limited",
            "invalid_request",
            "unknown_app",
            "bad_token",
            "offline",
            "server",
        ] {
            let m = message_for(code);
            assert!(m.ends_with('.'), "{code}: {m}");
            assert!(!m.contains(". "), "{code} is more than one sentence: {m}");
            assert!(!m.contains('_'), "{code} leaks a code: {m}");
        }
    }

    // ── refresh: fail closed ──

    fn refresh_with(tag: &str, status: u16, code: &str) -> (Env, RefreshOutcome) {
        let e = activated(tag, 3 * DAY_S);
        e.net
            .push(reply(status, serde_json::json!({"error": code})));
        let out = e.client.refresh(true);
        (e, out)
    }

    fn assert_deleted(tag: &str, status: u16, code: &str) {
        let (e, out) = refresh_with(tag, status, code);
        assert_eq!(out, RefreshOutcome::Deleted(code.to_string()));
        assert!(
            e.mgr.client_record().token.is_none(),
            "{code}: token must be deleted"
        );
        assert!(!is_licensed(&e.mgr.status()));
        let call = e.net.calls().pop().unwrap();
        assert!(call.url.ends_with("/refresh"));
        assert_eq!(call.app_key, APP_KEY);
    }

    #[test]
    fn revoked_403_deletes_token() {
        assert_deleted("revoked", 403, "revoked");
    }
    #[test]
    fn expired_403_deletes_token() {
        assert_deleted("expired", 403, "expired");
    }
    #[test]
    fn deactivated_403_deletes_token() {
        assert_deleted("deactivated", 403, "deactivated");
    }
    #[test]
    fn reactivate_required_401_deletes_token() {
        assert_deleted("reactivate", 401, "reactivate_required");
    }
    #[test]
    fn device_mismatch_401_deletes_token() {
        assert_deleted("mismatch", 401, "device_mismatch");
    }

    // ── refresh: never downgrade ──

    fn assert_kept(tag: &str, status: u16, code: &str) {
        let (e, out) = refresh_with(tag, status, code);
        assert!(matches!(out, RefreshOutcome::Kept(_)), "{out:?}");
        assert!(
            e.mgr.client_record().token.is_some(),
            "{status} {code}: token must stay"
        );
        assert!(is_licensed(&e.mgr.status()));
    }

    #[test]
    fn blocked_403_keeps_token() {
        assert_kept("blocked", 403, "blocked");
    }
    #[test]
    fn bad_request_400_keeps_token() {
        assert_kept("400", 400, "invalid_request");
    }
    #[test]
    fn too_large_413_keeps_token() {
        assert_kept("413", 413, "too_large");
    }
    #[test]
    fn invalid_token_401_keeps_token() {
        assert_kept("401-invalid", 401, "invalid_token");
    }
    #[test]
    fn server_503_keeps_token() {
        assert_kept("503", 503, "unavailable");
    }

    #[test]
    fn offline_keeps_token() {
        let e = activated("offline", 3 * DAY_S);
        e.net.push(Err("timeout".into()));
        assert_eq!(
            e.client.refresh(true),
            RefreshOutcome::Kept("offline".into())
        );
        assert!(is_licensed(&e.mgr.status()));
    }

    #[test]
    fn retry_after_429_honoured() {
        let e = activated("429", 3 * DAY_S);
        e.net.push(Ok(TransportReply {
            status: 429,
            retry_after_s: Some(600),
            body: br#"{"error":"rate_limited"}"#.to_vec(),
        }));
        assert_eq!(
            e.client.refresh(true),
            RefreshOutcome::Kept("rate_limited".into())
        );
        let until = e
            .mgr
            .client_record()
            .retry_after_until_ms
            .expect("Retry-After recorded");
        assert!(until >= wall_ms() + 590_000);
        // Inside the window: no request, even though the token is due.
        e.mgr.note_refresh(wall_ms() - 2 * REFRESH_MIN_INTERVAL_MS);
        let before = e.net.calls().len();
        assert_eq!(e.client.refresh_if_due(), RefreshOutcome::NotDue);
        assert_eq!(e.client.activate(KEY).unwrap_err().code, "rate_limited");
        assert_eq!(e.net.calls().len(), before, "no request inside Retry-After");
        assert!(is_licensed(&e.mgr.status()));
    }

    // ── refresh: schedule ──

    #[test]
    fn launch_check_makes_no_request_when_token_fresh() {
        let e = activated("fresh", 30 * DAY_S);
        e.mgr.note_refresh(wall_ms() - 10 * REFRESH_MIN_INTERVAL_MS);
        let before = e.net.calls().len();
        assert_eq!(e.client.refresh_if_due(), RefreshOutcome::NotDue);
        assert_eq!(
            e.net.calls().len(),
            before,
            "a fresh token makes no licensing request"
        );
    }

    #[test]
    fn refresh_skipped_inside_24h() {
        let e = activated("inside-24h", 3 * DAY_S);
        // Activation just answered: inside 24 h even though < 7 d remain.
        let before = e.net.calls().len();
        assert_eq!(e.client.refresh_if_due(), RefreshOutcome::NotDue);
        assert_eq!(e.net.calls().len(), before);
        assert!(!refresh_due(
            wall_ms() / 1000 + 3 * DAY_S,
            Some(wall_ms() - 3_600_000),
            None,
            false,
            wall_ms()
        ));
    }

    #[test]
    fn refresh_forced_under_7d() {
        let e = activated("under-7d", 3 * DAY_S);
        e.mgr
            .note_refresh(wall_ms() - REFRESH_MIN_INTERVAL_MS - 1000);
        e.net.push(reply(
            200,
            serde_json::json!({"token": mint(0, 30 * DAY_S)}),
        ));
        assert_eq!(e.client.refresh_if_due(), RefreshOutcome::Refreshed);
        assert!(is_licensed(&e.mgr.status()));
        let now = wall_ms();
        assert!(refresh_due(now / 1000 + 6 * DAY_S, None, None, false, now));
        assert!(!refresh_due(now / 1000 + 8 * DAY_S, None, None, false, now));
    }

    #[test]
    fn refresh_in_grace_keeps_working_then_refreshes() {
        // exp 5 days ago: inside the 30-day grace, still licensed offline.
        let e = activated("grace", -5 * DAY_S);
        assert!(is_licensed(&e.mgr.status()), "inside grace is licensed");
        e.mgr
            .note_refresh(wall_ms() - REFRESH_MIN_INTERVAL_MS - 1000);
        e.net.push(Err("connect".into()));
        assert_eq!(
            e.client.refresh_if_due(),
            RefreshOutcome::Kept("offline".into())
        );
        assert!(
            is_licensed(&e.mgr.status()),
            "offline in grace never downgrades"
        );
        e.net.push(reply(
            200,
            serde_json::json!({"token": mint(0, 30 * DAY_S)}),
        ));
        assert_eq!(e.client.refresh_if_due(), RefreshOutcome::Refreshed);
    }

    // ── 403 disputed ──

    fn held(tag: &str) -> Env {
        let e = activated(tag, 20 * DAY_S);
        e.net.push(reply(
            403,
            serde_json::json!({"error": "disputed", "message": "Your license is on hold while a payment dispute is reviewed."}),
        ));
        assert_eq!(e.client.refresh(true), RefreshOutcome::Held);
        e
    }

    #[test]
    fn disputed_403_keeps_token() {
        let e = held("disputed-keeps");
        assert!(e.mgr.client_record().token.is_some());
    }

    #[test]
    fn disputed_403_not_entitled() {
        let e = held("disputed-not-entitled");
        let s = e.mgr.status();
        assert!(
            matches!(&s.entitlement, Entitlement::LicenseRequired { reason } if reason == "disputed")
        );
        assert_eq!(s.license_problem.as_deref(), Some("disputed"));
        assert!(s.license_problem_message.as_deref().unwrap_or("").contains("on hold"));
        assert!(!s.allows_new_dictation());
    }

    #[test]
    fn disputed_hold_survives_relaunch_offline() {
        let e = held("disputed-relaunch");
        // Relaunch: a new manager on the same dir + store, then offline.
        let mgr2 = manager(e.dir.path(), e.store.clone());
        assert!(
            !mgr2.status().allows_new_dictation(),
            "hold survives relaunch"
        );
        let net2 = Arc::new(Scripted::default());
        net2.push(Err("connect".into()));
        let client2 = LicenseClient::build(
            "http://stub.invalid/x",
            mgr2.clone(),
            net2.clone(),
            e.keychain.clone(),
        );
        assert_eq!(
            client2.refresh_if_due(),
            RefreshOutcome::Kept("offline".into())
        );
        assert!(
            !mgr2.status().allows_new_dictation(),
            "a valid token never re-entitles a hold"
        );
        // Deleting license.json does not lift it either (SQLite copy).
        let _ = std::fs::remove_file(e.dir.path().join("license.json"));
        assert!(
            mgr2.client_record().disputed_hold.is_some()
                || e.store.get(crate::license::DB_KEY_DISPUTED_HOLD).is_some()
        );
    }

    #[test]
    fn disputed_hold_retries_inside_24h() {
        let e = held("disputed-retry");
        // The hold was answered moments ago, yet the launch check still asks.
        e.net
            .push(reply(403, serde_json::json!({"error": "disputed"})));
        let before = e.net.calls().len();
        assert_eq!(e.client.refresh_if_due(), RefreshOutcome::Held);
        assert_eq!(e.net.calls().len(), before + 1);
    }

    #[test]
    fn disputed_hold_cleared_by_200() {
        let e = held("disputed-cleared");
        e.net.push(reply(
            200,
            serde_json::json!({"token": mint(0, 30 * DAY_S)}),
        ));
        assert_eq!(e.client.refresh_if_due(), RefreshOutcome::Refreshed);
        assert!(e.mgr.client_record().disputed_hold.is_none());
        assert!(is_licensed(&e.mgr.status()));
    }

    // ── device identity ──

    #[test]
    fn keychain_delete_reactivates_same_seat() {
        let e = activated("same-seat", 30 * DAY_S);
        let first = e.net.calls()[0].body["device_id"]
            .as_str()
            .unwrap()
            .to_string();
        // The Keychain item is gone (e.g. a signing-identity change).
        e.keychain.delete(DEVICE_ID_ACCOUNT).unwrap();
        e.net.push(reply(
            200,
            serde_json::json!({"token": mint(0, 30 * DAY_S)}),
        ));
        e.client.activate(KEY).unwrap();
        let second = e.net.calls().last().unwrap().body["device_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(first, second, "re-activation sends the mirrored device_id");
        assert_eq!(
            e.keychain.get(DEVICE_ID_ACCOUNT).unwrap().as_deref(),
            Some(first.as_str())
        );
    }

    // ── deactivate ──

    #[test]
    fn deactivate_frees_seat_before_clearing() {
        let e = activated("deactivate", 30 * DAY_S);
        let token = e.mgr.client_record().token.unwrap();
        e.net.push(reply(200, serde_json::json!({"ok": true})));
        let status = e.client.deactivate().unwrap();
        let call = e.net.calls().pop().unwrap();
        assert!(call.url.ends_with("/deactivate"));
        assert_eq!(call.body["token"], token.as_str());
        assert!(!is_licensed(&status));
        assert!(e.mgr.client_record().token.is_none());
        assert!(
            e.mgr.client_record().device_id.is_some(),
            "the install UUID stays"
        );
    }

    #[test]
    fn deactivate_offline_keeps_license() {
        let e = activated("deactivate-offline", 30 * DAY_S);
        e.net.push(Err("connect".into()));
        assert_eq!(e.client.deactivate().unwrap_err().code, "offline");
        assert!(
            is_licensed(&e.mgr.status()),
            "the seat is still taken, so the license stays"
        );
    }

    // ── the wire, against a local stub HTTP server ──

    /// One-connection-per-request HTTP/1.1 stub on 127.0.0.1. Serves the
    /// scripted (status, headers, body) replies in order and records each
    /// request's raw head + body. Stops after the last reply.
    fn stub_server(
        replies: Vec<(u16, Vec<(&'static str, String)>, String)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/api/v1/licensing", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for (status, headers, body) in replies {
                let (mut sock, _) = listener.accept().unwrap();
                sock.set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = sock.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let len = text[..end]
                            .lines()
                            .find_map(|l| {
                                let (k, v) = l.split_once(':')?;
                                k.eq_ignore_ascii_case("content-length")
                                    .then(|| v.trim().parse::<usize>().ok())?
                            })
                            .unwrap_or(0);
                        if buf.len() >= end + 4 + len {
                            break;
                        }
                    }
                }
                seen.push(String::from_utf8_lossy(&buf).to_string());
                let mut head = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n",
                    body.len()
                );
                for (k, v) in headers {
                    head.push_str(&format!("{k}: {v}\r\n"));
                }
                head.push_str("\r\n");
                let _ = sock.write_all(head.as_bytes());
                let _ = sock.write_all(body.as_bytes());
                let _ = sock.flush();
            }
            seen
        });
        (base, handle)
    }

    #[test]
    fn http_transport_sends_x_drivia_app_and_reads_retry_after() {
        let token = mint(-60, 30 * DAY_S);
        let (base, server) = stub_server(vec![
            (200, vec![], serde_json::json!({"token": token}).to_string()),
            (
                429,
                vec![("Retry-After", "120".to_string())],
                r#"{"error":"rate_limited"}"#.to_string(),
            ),
        ]);
        let dir = TempDir::new("wire");
        let store: Arc<dyn LicenseStore> = Arc::new(MemoryStore::default());
        let mgr = manager(dir.path(), store);
        let client = LicenseClient::build(
            &base,
            mgr.clone(),
            Arc::new(HttpTransport),
            Arc::new(MemoryKeychain::new("wire")),
        );
        assert!(is_licensed(&client.activate(KEY).unwrap()));
        assert_eq!(
            client.refresh(true),
            RefreshOutcome::Kept("rate_limited".into())
        );
        let seen = server.join().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(
            seen[0].starts_with("POST /api/v1/licensing/activate "),
            "{}",
            seen[0]
        );
        assert!(seen[0]
            .to_ascii_lowercase()
            .contains(&format!("x-drivia-app: {APP_KEY}")));
        assert!(seen[0].contains(KEY));
        assert!(seen[1].starts_with("POST /api/v1/licensing/refresh "));
        let until = mgr.client_record().retry_after_until_ms.unwrap();
        assert!(until >= wall_ms() + 110_000, "Retry-After header honoured");
    }

    // ── SCOPE NOTE: no Stripe Payment Link anywhere in desktop/ or site/ ──

    #[test]
    fn no_stripe_payment_link_in_desktop_or_site() {
        let needle = concat!("buy", ".stripe", ".com");
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut hits = Vec::new();
        fn walk(dir: &Path, needle: &str, hits: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let p = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if matches!(
                    name.as_str(),
                    "node_modules" | "target" | "dist" | ".git" | "binaries"
                ) {
                    continue;
                }
                if p.is_dir() {
                    walk(&p, needle, hits);
                } else if let Ok(text) = std::fs::read_to_string(&p) {
                    if text.contains(needle) {
                        hits.push(p.display().to_string());
                    }
                }
            }
        }
        for sub in ["desktop", "site"] {
            walk(&root.join(sub), needle, &mut hits);
        }
        assert!(hits.is_empty(), "Stripe Payment Link found: {hits:#?}");
    }

    // ── LIVE: the consulting TEST stack (never production) ──

    /// Activates, refreshes and deactivates a comp license on the consulting
    /// TEST stack, then purges what it wrote: the seat is freed server-side
    /// by deactivate, and the temp data dir + in-memory Keychain are dropped.
    /// Run by hand:
    ///   YAP_LICENSING_TEST_BASE=<test stack>/api/v1/licensing \
    ///   YAP_LICENSING_TEST_KEY=DCL-… [YAP_LICENSING_TEST_JWK_X=<base64url x of the test signing key>] \
    ///   cargo test --lib license_client::tests::live_consulting_test_stack_roundtrip -- --ignored
    /// Refuses the production host.
    #[test]
    #[ignore = "live: needs the consulting test stack and a comp DCL key"]
    fn live_consulting_test_stack_roundtrip() {
        let (Ok(base), Ok(key)) = (
            std::env::var("YAP_LICENSING_TEST_BASE"),
            std::env::var("YAP_LICENSING_TEST_KEY"),
        ) else {
            eprintln!("skipped: YAP_LICENSING_TEST_BASE / YAP_LICENSING_TEST_KEY not set");
            return;
        };
        assert!(
            !base.starts_with("https://drivia.consulting"),
            "the live test runs against the TEST stack only"
        );
        let dir = TempDir::new("live");
        let store: Arc<dyn LicenseStore> = Arc::new(MemoryStore::default());
        let mgr = match std::env::var("YAP_LICENSING_TEST_JWK_X") {
            Ok(x) => {
                let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(x.trim())
                    .expect("JWK x is base64url");
                let pk: [u8; 32] = raw.try_into().expect("32-byte Ed25519 key");
                Arc::new(LicenseManager::with_token_verifier(
                    dir.path(),
                    store,
                    Box::new(wall_ms),
                    Box::new(move |t: &str, now: i64| {
                        let kid = crate::license_v2::EMBEDDED_KEYS[0].0;
                        crate::license_v2::verify_with(&[(kid, pk)], t, now)
                            .or_else(|_| crate::license_v2::verify_token(t, now))
                    }),
                ))
            }
            Err(_) => Arc::new(LicenseManager::new(dir.path(), store)),
        };
        let client = LicenseClient::build(
            &base,
            mgr.clone(),
            Arc::new(HttpTransport),
            Arc::new(MemoryKeychain::new("live")),
        );
        let activated = client.activate(&key).expect("comp license activates");
        assert!(is_licensed(&activated));
        assert_eq!(client.refresh(true), RefreshOutcome::Refreshed);
        let after = client.deactivate().expect("deactivate frees the seat");
        assert!(!is_licensed(&after));
        assert!(mgr.client_record().token.is_none());
        drop(client);
        drop(dir); // purge the local fixture
    }
}
