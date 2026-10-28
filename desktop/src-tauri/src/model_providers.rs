//! yap-byom-0 — bring your own model, the frame.
//!
//! A *provider* is where a task's text is sent to be completed: the bundled
//! local model (the default, always present, never deletable) or — from
//! yap-byom-1 — an OpenAI-compatible endpoint the user names. Two tasks pick a
//! provider independently: **notes** (the whole meeting transcript) and
//! **polish** (every dictation).
//!
//! The one fact the UI must never get wrong is whether a choice sends text off
//! this Mac. [`leaves_this_mac`] answers it from the provider's own config:
//! Bundled and a loopback base URL are "No"; any other host is "Yes — <host>";
//! and a loopback URL whose model is an Ollama *cloud* model (`-cloud` /
//! `:cloud`, docs.ollama.com/cloud) is "Yes — ollama.com", because Ollama runs
//! those on ollama.com THROUGH localhost.
//!
//! Storage: the provider list is `settings_kv` row [`PROVIDERS_KEY`] and the
//! two picks are [`NOTES_PROVIDER_KEY`] / [`POLISH_PROVIDER_KEY`] (empty =
//! Bundled). [`ProviderConfig`] has no key field at all, so no serialization of
//! it can carry one: keys live only in the Keychain (`keychain.rs`), bound to
//! the provider's host. `polish_model` keeps its meaning — which bundled model
//! file polish loads.
//!
//! Every outbound call goes through `http_client.rs` (redirects never
//! followed). Test connection takes a provider ID only, never a URL or a key
//! from the webview, so a webview bug cannot aim a stored key at a new host.

// `core::net::IpAddr` is a pure address parser (no socket); it is the same
// type as std's, named from `core` because this file makes no network call.
use core::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::http_client::{self, ClientCache, HttpError};
use crate::keychain::{self, SecretStore};
use crate::polish::{PolishClient, PolishError};
use crate::polish_protocol::PolishRequest;
use crate::summary_jobs::CancelToken;

/// `settings_kv` row holding the provider list (JSON array, NO keys).
pub const PROVIDERS_KEY: &str = "model_providers_v1";
/// `settings_kv` row: the provider id notes use (empty = Bundled).
pub const NOTES_PROVIDER_KEY: &str = "notes_provider";
/// `settings_kv` row: the provider id polish uses (empty = Bundled).
pub const POLISH_PROVIDER_KEY: &str = "polish_provider";
/// The id of the always-present bundled provider.
pub const BUNDLED_ID: &str = "bundled";
/// Default context window for a provider that does not say.
pub const DEFAULT_CONTEXT_TOKENS: u32 = 4096;
/// Default whole-request timeout for a provider that does not say.
pub const DEFAULT_TIMEOUT_S: u32 = 60;
/// Test connection: a 10-token completion…
pub const TEST_MAX_TOKENS: u32 = 10;
/// …with a 10 s deadline.
pub const TEST_TIMEOUT: Duration = Duration::from_secs(10);
/// The event Test connection emits when it starts and when it finishes.
pub const TEST_EVENT: &str = "model_provider_test";
/// The label of a loopback provider serving an Ollama cloud model.
pub const OLLAMA_CLOUD_HOST: &str = "ollama.com";

/// What kind of provider this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// The model bundled with Yap, run by the local sidecar.
    Bundled,
    /// Any server speaking the OpenAI chat-completions API.
    OpenAiCompatible,
}

impl ProviderKind {
    /// Is this kind wired end to end in THIS build? The UI hides every kind
    /// that is not, and saving one is refused — no "coming soon" placeholder,
    /// never a fake success. OpenAI-compatible is wired by yap-byom-1.
    pub fn is_wired(self) -> bool {
        matches!(self, ProviderKind::Bundled)
    }

    pub fn all() -> [ProviderKind; 2] {
        [ProviderKind::Bundled, ProviderKind::OpenAiCompatible]
    }
}

fn default_context_tokens() -> u32 {
    DEFAULT_CONTEXT_TOKENS
}
fn default_timeout_s() -> u32 {
    DEFAULT_TIMEOUT_S
}

/// One provider. There is deliberately NO key field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub id: String,
    pub kind: ProviderKind,
    pub label: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    #[serde(default = "default_context_tokens")]
    pub context_tokens: u32,
    #[serde(default = "default_timeout_s")]
    pub timeout_s: u32,
}

impl ProviderConfig {
    /// The always-present bundled provider.
    pub fn bundled() -> Self {
        Self {
            id: BUNDLED_ID.into(),
            kind: ProviderKind::Bundled,
            label: "Bundled model (on this Mac)".into(),
            base_url: String::new(),
            model: String::new(),
            context_tokens: DEFAULT_CONTEXT_TOKENS,
            timeout_s: DEFAULT_TIMEOUT_S,
        }
    }

    /// The host this provider's key is bound to (`None` for Bundled).
    pub fn host(&self) -> Option<String> {
        match self.kind {
            ProviderKind::Bundled => None,
            _ => http_client::host_of(&self.base_url),
        }
    }
}

/// Does a choice send text off this Mac, and where to?
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "leaves")]
pub enum Leaves {
    No,
    Yes { host: String },
}

impl Leaves {
    pub fn is_yes(&self) -> bool {
        matches!(self, Leaves::Yes { .. })
    }
    /// "No" or "Yes — <host>".
    pub fn label(&self) -> String {
        match self {
            Leaves::No => "No".into(),
            Leaves::Yes { host } => format!("Yes — {host}"),
        }
    }
}

/// Is `host` this Mac (127.0.0.0/8, ::1, localhost)?
pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim().trim_start_matches('[').trim_end_matches(']');
    h.eq_ignore_ascii_case("localhost")
        || h.parse::<IpAddr>()
            .map(|a| a.is_loopback())
            .unwrap_or(false)
}

/// An Ollama cloud model name (`gpt-oss:120b-cloud`, `kimi-k2:cloud`).
pub fn is_ollama_cloud_model(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    m.contains("-cloud") || m.contains(":cloud")
}

/// The answer the "Leaves this Mac" label shows. An unparseable URL is treated
/// as leaving (the label must never under-claim).
pub fn leaves_this_mac(cfg: &ProviderConfig) -> Leaves {
    if cfg.kind == ProviderKind::Bundled {
        return Leaves::No;
    }
    match cfg.host() {
        Some(h) if is_loopback_host(&h) => {
            if is_ollama_cloud_model(&cfg.model) {
                Leaves::Yes {
                    host: OLLAMA_CLOUD_HOST.into(),
                }
            } else {
                Leaves::No
            }
        }
        Some(h) => Leaves::Yes { host: h },
        None => Leaves::Yes {
            host: "an unknown host".into(),
        },
    }
}

/// Why a provider call failed, in plain words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    Unreachable { host: String },
    Timeout { secs: u64 },
    KeyRejected,
    ModelNotFound { model: String },
    RateLimited,
    Server { code: u16 },
    Http { code: u16 },
    Redirected { host: String },
    BadResponse,
    NotInstalled,
    BundledUnavailable,
    NotWired,
    Cancelled,
    UnknownProvider { id: String },
    CannotDeleteBundled,
    Invalid(String),
    Keychain(String),
    Storage(String),
}

impl ProviderError {
    pub fn plain(&self) -> String {
        match self {
            ProviderError::Unreachable { host } => format!("{host} is unreachable"),
            ProviderError::Timeout { secs } => format!("timed out — no answer within {secs} s"),
            ProviderError::KeyRejected => "the key was rejected (401)".into(),
            ProviderError::ModelNotFound { model } => format!("model {model} not found (404)"),
            ProviderError::RateLimited => "rate limited (429) — try again shortly".into(),
            ProviderError::Server { code } => format!("server error ({code})"),
            ProviderError::Http { code } => format!("the server answered {code}"),
            ProviderError::Redirected { host } => format!("redirected to {host} — not followed"),
            ProviderError::BadResponse => "bad response".into(),
            ProviderError::NotInstalled => {
                "the bundled model is not downloaded yet — install it under Settings → Advanced"
                    .into()
            }
            ProviderError::BundledUnavailable => "the bundled model could not start".into(),
            ProviderError::NotWired => {
                "this kind of provider is not available in this version of Yap".into()
            }
            ProviderError::Cancelled => "cancelled".into(),
            ProviderError::UnknownProvider { id } => format!("no provider named {id}"),
            ProviderError::CannotDeleteBundled => "the bundled model cannot be deleted".into(),
            ProviderError::Invalid(why) => why.clone(),
            ProviderError::Keychain(why) => why.clone(),
            ProviderError::Storage(why) => format!("settings could not be saved: {why}"),
        }
    }

    /// A short machine tag for the log line (never the message text).
    pub fn tag(&self) -> &'static str {
        match self {
            ProviderError::Unreachable { .. } => "unreachable",
            ProviderError::Timeout { .. } => "timeout",
            ProviderError::KeyRejected => "key_rejected",
            ProviderError::ModelNotFound { .. } => "model_not_found",
            ProviderError::RateLimited => "rate_limited",
            ProviderError::Server { .. } => "server_error",
            ProviderError::Http { .. } => "http_status",
            ProviderError::Redirected { .. } => "redirected",
            ProviderError::BadResponse => "bad_response",
            ProviderError::NotInstalled => "not_installed",
            ProviderError::BundledUnavailable => "bundled_unavailable",
            ProviderError::NotWired => "not_wired",
            ProviderError::Cancelled => "cancelled",
            ProviderError::UnknownProvider { .. } => "unknown_provider",
            ProviderError::CannotDeleteBundled => "cannot_delete_bundled",
            ProviderError::Invalid(_) => "invalid",
            ProviderError::Keychain(_) => "keychain",
            ProviderError::Storage(_) => "storage",
        }
    }

    /// Map an HTTP failure for a call to `model`.
    pub fn from_http(e: HttpError, model: &str) -> Self {
        match e {
            HttpError::Unreachable { host } => ProviderError::Unreachable { host },
            HttpError::Timeout { secs } => ProviderError::Timeout { secs },
            HttpError::Redirected { host } => ProviderError::Redirected { host },
            HttpError::Status { code: 401 } | HttpError::Status { code: 403 } => {
                ProviderError::KeyRejected
            }
            HttpError::Status { code: 404 } => ProviderError::ModelNotFound {
                model: model.to_string(),
            },
            HttpError::Status { code: 429 } => ProviderError::RateLimited,
            HttpError::Status { code } if (500..600).contains(&code) => {
                ProviderError::Server { code }
            }
            HttpError::Status { code } => ProviderError::Http { code },
            HttpError::BadRequest(why) => ProviderError::Invalid(why),
            HttpError::BadBody => ProviderError::BadResponse,
        }
    }
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.plain())
    }
}

/// Where a task's text is completed.
pub trait ChatProvider: Send + Sync {
    fn complete(
        &self,
        system: &str,
        user: &str,
        max_tokens: u32,
        cancel: &CancelToken,
    ) -> Result<String, ProviderError>;
    fn leaves_this_mac(&self) -> bool;
}

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1 << 40);

/// The bundled model behind [`ChatProvider`], wrapping the existing sidecar
/// path (`polish::SidecarClient` in the app). The sidecar owns its own system
/// turn, so `system` is not sent; `user` is the text, `max_tokens` its cap.
/// `None` client = the bundled model is not downloaded.
pub struct BundledProvider<C: PolishClient + Send + Sync> {
    client: Option<C>,
    deadline: Duration,
}

impl<C: PolishClient + Send + Sync> BundledProvider<C> {
    pub fn new(client: Option<C>, deadline: Duration) -> Self {
        Self { client, deadline }
    }
}

impl<C: PolishClient + Send + Sync> ChatProvider for BundledProvider<C> {
    fn complete(
        &self,
        _system: &str,
        user: &str,
        max_tokens: u32,
        cancel: &CancelToken,
    ) -> Result<String, ProviderError> {
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        let client = self.client.as_ref().ok_or(ProviderError::NotInstalled)?;
        let req = PolishRequest::polish(
            NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed),
            "plain",
            "default",
            max_tokens,
            self.deadline.as_millis() as u64,
            user.to_string(),
        );
        client.rewrite(&req).map_err(|e| match e {
            PolishError::Deadline => ProviderError::Timeout {
                secs: self.deadline.as_secs(),
            },
            PolishError::Unavailable => ProviderError::BundledUnavailable,
            _ => ProviderError::BadResponse,
        })
    }

    fn leaves_this_mac(&self) -> bool {
        false
    }
}

/// An OpenAI-compatible chat-completions endpoint. Used in this item ONLY by
/// Test connection (a provider of this kind cannot be saved until yap-byom-1
/// flips [`ProviderKind::is_wired`]); the notes and polish pipelines do not
/// route to it yet. The key is held for the call and never formatted.
pub struct OpenAiCompatibleProvider {
    cfg: ProviderConfig,
    key: Option<String>,
    client: http_client::HttpClient,
}

impl OpenAiCompatibleProvider {
    pub fn new(cfg: ProviderConfig, key: Option<String>, client: http_client::HttpClient) -> Self {
        Self { cfg, key, client }
    }

    fn endpoint(&self) -> String {
        format!(
            "{}/chat/completions",
            self.cfg.base_url.trim().trim_end_matches('/')
        )
    }
}

impl ChatProvider for OpenAiCompatibleProvider {
    fn complete(
        &self,
        system: &str,
        user: &str,
        max_tokens: u32,
        cancel: &CancelToken,
    ) -> Result<String, ProviderError> {
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        let body = serde_json::json!({
            "model": self.cfg.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "max_tokens": max_tokens,
            "stream": false,
        });
        let auth = self.key.as_ref().map(|k| format!("Bearer {k}"));
        let mut headers: Vec<(&str, &str)> = Vec::new();
        if let Some(a) = auth.as_deref() {
            headers.push(("Authorization", a));
        }
        let reply =
            http_client::post_json_blocking(&self.client, &self.endpoint(), &headers, &body)
                .map_err(|e| ProviderError::from_http(e, &self.cfg.model))?;
        let v: serde_json::Value =
            serde_json::from_str(&reply.body).map_err(|_| ProviderError::BadResponse)?;
        v["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_string)
            .ok_or(ProviderError::BadResponse)
    }

    fn leaves_this_mac(&self) -> bool {
        leaves_this_mac(&self.cfg).is_yes()
    }
}

// ── Settings ─────────────────────────────────────────────────────────────────

/// The two rows of `settings_kv` this module reads and writes.
pub trait SettingsKv {
    fn kv_get(&self, key: &str) -> Option<String>;
    fn kv_set(&self, key: &str, value: &str) -> Result<(), String>;
}

impl SettingsKv for crate::db::Database {
    fn kv_get(&self, key: &str) -> Option<String> {
        self.setting_get(key)
    }
    fn kv_set(&self, key: &str, value: &str) -> Result<(), String> {
        self.setting_set(key, value)
    }
}

/// Which task a provider is picked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    Notes,
    Polish,
}

impl Task {
    fn key(self) -> &'static str {
        match self {
            Task::Notes => NOTES_PROVIDER_KEY,
            Task::Polish => POLISH_PROVIDER_KEY,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Task::Notes => "notes",
            Task::Polish => "polish",
        }
    }
}

/// The stored provider settings. Bundled is always first and always present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSettings {
    pub providers: Vec<ProviderConfig>,
    pub notes_provider: String,
    pub polish_provider: String,
}

impl ProviderSettings {
    pub fn find(&self, id: &str) -> Option<&ProviderConfig> {
        let id = if id.trim().is_empty() {
            BUNDLED_ID
        } else {
            id.trim()
        };
        self.providers.iter().find(|p| p.id == id)
    }

    /// The provider a task uses (an unknown id falls back to Bundled).
    pub fn for_task(&self, task: Task) -> ProviderConfig {
        let id = match task {
            Task::Notes => &self.notes_provider,
            Task::Polish => &self.polish_provider,
        };
        self.find(id)
            .cloned()
            .unwrap_or_else(ProviderConfig::bundled)
    }
}

/// Read the settings. A malformed list is logged and read as "Bundled only".
pub fn load(kv: &dyn SettingsKv) -> ProviderSettings {
    let mut providers: Vec<ProviderConfig> = match kv.kv_get(PROVIDERS_KEY) {
        Some(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
            log::warn!("model_providers load malformed key={PROVIDERS_KEY} error={e}");
            Vec::new()
        }),
        None => Vec::new(),
    };
    providers.retain(|p| p.kind != ProviderKind::Bundled && p.id != BUNDLED_ID);
    let mut seen = std::collections::HashSet::new();
    providers.retain(|p| seen.insert(p.id.clone()));
    providers.insert(0, ProviderConfig::bundled());
    ProviderSettings {
        providers,
        notes_provider: kv.kv_get(NOTES_PROVIDER_KEY).unwrap_or_default(),
        polish_provider: kv.kv_get(POLISH_PROVIDER_KEY).unwrap_or_default(),
    }
}

/// Write the settings (Bundled is implicit and not stored).
pub fn save(kv: &dyn SettingsKv, s: &ProviderSettings) -> Result<(), ProviderError> {
    let stored: Vec<&ProviderConfig> = s
        .providers
        .iter()
        .filter(|p| p.kind != ProviderKind::Bundled)
        .collect();
    let json = serde_json::to_string(&stored).map_err(|e| ProviderError::Storage(e.to_string()))?;
    kv.kv_set(PROVIDERS_KEY, &json)
        .map_err(ProviderError::Storage)?;
    kv.kv_set(NOTES_PROVIDER_KEY, &s.notes_provider)
        .map_err(ProviderError::Storage)?;
    kv.kv_set(POLISH_PROVIDER_KEY, &s.polish_provider)
        .map_err(ProviderError::Storage)?;
    Ok(())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Add or update a provider. `new_key` (non-empty) is saved to the Keychain
/// under `<id>@<host>`. Changing the base URL's HOST deletes the old key: the
/// user re-enters it for the new host.
pub fn upsert_provider(
    kv: &dyn SettingsKv,
    keys: &dyn SecretStore,
    cfg: ProviderConfig,
    new_key: Option<&str>,
) -> Result<ProviderConfig, ProviderError> {
    if cfg.kind == ProviderKind::Bundled || cfg.id == BUNDLED_ID {
        return Err(ProviderError::Invalid(
            "the bundled model is built in and cannot be edited".into(),
        ));
    }
    if !valid_id(&cfg.id) {
        return Err(ProviderError::Invalid(
            "a provider id is lowercase letters, digits, - or _".into(),
        ));
    }
    if cfg.label.trim().is_empty() {
        return Err(ProviderError::Invalid("give the provider a name".into()));
    }
    let host = cfg
        .host()
        .ok_or_else(|| ProviderError::Invalid("the address must be an http(s) URL".into()))?;
    if cfg.model.trim().is_empty() {
        return Err(ProviderError::Invalid("name the model to use".into()));
    }
    let mut s = load(kv);
    if let Some(old) = s.providers.iter().find(|p| p.id == cfg.id) {
        if let Some(old_host) = old.host() {
            if old_host != host {
                keys.delete(&keychain::account_for(&old.id, &old_host))
                    .map_err(ProviderError::Keychain)?;
                log::info!(
                    "model_providers key_deleted id={} reason=host_changed",
                    cfg.id
                );
            }
        }
    }
    if let Some(k) = new_key.map(str::trim).filter(|k| !k.is_empty()) {
        keys.set(&keychain::account_for(&cfg.id, &host), k)
            .map_err(ProviderError::Keychain)?;
        log::info!("model_providers key_saved id={}", cfg.id);
    }
    let cfg = ProviderConfig {
        context_tokens: if cfg.context_tokens == 0 {
            DEFAULT_CONTEXT_TOKENS
        } else {
            cfg.context_tokens
        },
        timeout_s: if cfg.timeout_s == 0 {
            DEFAULT_TIMEOUT_S
        } else {
            cfg.timeout_s
        },
        ..cfg
    };
    match s.providers.iter_mut().find(|p| p.id == cfg.id) {
        Some(slot) => *slot = cfg.clone(),
        None => s.providers.push(cfg.clone()),
    }
    save(kv, &s)?;
    log::info!(
        "model_providers upsert id={} kind={:?} leaves={}",
        cfg.id,
        cfg.kind,
        leaves_this_mac(&cfg).is_yes()
    );
    Ok(cfg)
}

/// Delete a provider and its Keychain item; a task that used it falls back to
/// Bundled. Bundled cannot be deleted.
pub fn delete_provider(
    kv: &dyn SettingsKv,
    keys: &dyn SecretStore,
    id: &str,
) -> Result<(), ProviderError> {
    if id.trim().is_empty() || id == BUNDLED_ID {
        return Err(ProviderError::CannotDeleteBundled);
    }
    let mut s = load(kv);
    let old = s
        .providers
        .iter()
        .find(|p| p.id == id)
        .cloned()
        .ok_or_else(|| ProviderError::UnknownProvider { id: id.into() })?;
    if let Some(host) = old.host() {
        keys.delete(&keychain::account_for(&old.id, &host))
            .map_err(ProviderError::Keychain)?;
    }
    s.providers.retain(|p| p.id != id);
    if s.notes_provider == id {
        s.notes_provider.clear();
    }
    if s.polish_provider == id {
        s.polish_provider.clear();
    }
    save(kv, &s)?;
    log::info!("model_providers delete id={id}");
    Ok(())
}

/// Pick the provider a task uses (`""` or `"bundled"` = Bundled).
pub fn set_task_provider(kv: &dyn SettingsKv, task: Task, id: &str) -> Result<(), ProviderError> {
    let s = load(kv);
    let id = if id.trim() == BUNDLED_ID {
        ""
    } else {
        id.trim()
    };
    if !id.is_empty() && s.find(id).is_none() {
        return Err(ProviderError::UnknownProvider { id: id.into() });
    }
    kv.kv_set(task.key(), id).map_err(ProviderError::Storage)?;
    log::info!(
        "model_providers pick task={} id={}",
        task.as_str(),
        if id.is_empty() { BUNDLED_ID } else { id }
    );
    Ok(())
}

/// One provider as the UI sees it: never the key, only whether one is saved
/// and its last four characters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub id: String,
    pub kind: ProviderKind,
    pub label: String,
    pub base_url: String,
    pub model: String,
    pub context_tokens: u32,
    pub timeout_s: u32,
    pub leaves: bool,
    pub leaves_label: String,
    pub leaves_host: Option<String>,
    pub key_saved: bool,
    pub key_hint: String,
    pub deletable: bool,
}

/// A task whose provider sends text off this Mac.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveLeave {
    pub task: Task,
    pub host: String,
}

/// Everything Settings → Model providers renders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvidersView {
    pub providers: Vec<ProviderView>,
    pub notes_provider: String,
    pub polish_provider: String,
    pub wired_kinds: Vec<ProviderKind>,
    pub active_leaves: Vec<ActiveLeave>,
}

fn view_of(p: &ProviderConfig, keys: &dyn SecretStore) -> ProviderView {
    let leaves = leaves_this_mac(p);
    let (key_saved, key_hint) = match p.host() {
        Some(host) => match keys.get(&keychain::account_for(&p.id, &host)) {
            Ok(Some(k)) => (true, keychain::key_hint(&k)),
            Ok(None) => (false, String::new()),
            Err(e) => {
                log::warn!("model_providers key_read failed id={} error=\"{e}\"", p.id);
                (false, String::new())
            }
        },
        None => (false, String::new()),
    };
    ProviderView {
        id: p.id.clone(),
        kind: p.kind,
        label: p.label.clone(),
        base_url: p.base_url.clone(),
        model: p.model.clone(),
        context_tokens: p.context_tokens,
        timeout_s: p.timeout_s,
        leaves: leaves.is_yes(),
        leaves_label: leaves.label(),
        leaves_host: match &leaves {
            Leaves::Yes { host } => Some(host.clone()),
            Leaves::No => None,
        },
        key_saved,
        key_hint,
        deletable: p.kind != ProviderKind::Bundled,
    }
}

/// The view Settings renders.
pub fn list_view(kv: &dyn SettingsKv, keys: &dyn SecretStore) -> ProvidersView {
    let s = load(kv);
    let mut active_leaves = Vec::new();
    for task in [Task::Notes, Task::Polish] {
        if let Leaves::Yes { host } = leaves_this_mac(&s.for_task(task)) {
            active_leaves.push(ActiveLeave { task, host });
        }
    }
    ProvidersView {
        providers: s.providers.iter().map(|p| view_of(p, keys)).collect(),
        notes_provider: s.notes_provider.clone(),
        polish_provider: s.polish_provider.clone(),
        wired_kinds: ProviderKind::all()
            .into_iter()
            .filter(|k| k.is_wired())
            .collect(),
        active_leaves,
    }
}

/// What Test connection returns. Never the key — `key_hint` is its last four.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub provider_id: String,
    pub ok: bool,
    pub latency_ms: u64,
    pub reply: Option<String>,
    pub error: Option<String>,
    pub key_hint: String,
}

/// Builds the bundled provider for a test (the app passes the sidecar; tests
/// pass a stub).
pub type BundledFactory<'a> = &'a dyn Fn(Duration) -> Box<dyn ChatProvider>;

/// Test connection, by provider ID only: a [`TEST_MAX_TOKENS`] completion with
/// a [`TEST_TIMEOUT`] deadline. Emits [`TEST_EVENT`] at start and finish via
/// `emit`. This is the whole body of the `test_model_provider` command.
pub fn run_test(
    kv: &dyn SettingsKv,
    keys: &dyn SecretStore,
    clients: &ClientCache,
    provider_id: &str,
    bundled: BundledFactory<'_>,
    emit: &mut dyn FnMut(&str, serde_json::Value),
) -> Result<TestResult, ProviderError> {
    let s = load(kv);
    let cfg = s
        .find(provider_id)
        .cloned()
        .ok_or_else(|| ProviderError::UnknownProvider {
            id: provider_id.into(),
        })?;
    let host = cfg.host();
    let (provider, key_hint): (Box<dyn ChatProvider>, String) = match cfg.kind {
        ProviderKind::Bundled => (bundled(TEST_TIMEOUT), String::new()),
        ProviderKind::OpenAiCompatible => {
            let host = host.clone().ok_or_else(|| {
                ProviderError::Invalid("the address must be an http(s) URL".into())
            })?;
            let key = keys
                .get(&keychain::account_for(&cfg.id, &host))
                .map_err(ProviderError::Keychain)?;
            let hint = key.as_deref().map(keychain::key_hint).unwrap_or_default();
            let client = clients
                .client_for(&cfg.id, TEST_TIMEOUT)
                .map_err(|e| ProviderError::from_http(e, &cfg.model))?;
            (
                Box::new(OpenAiCompatibleProvider::new(cfg.clone(), key, client)),
                hint,
            )
        }
    };
    emit(
        TEST_EVENT,
        serde_json::json!({"providerId": cfg.id, "phase": "start"}),
    );
    log::info!(
        "model_providers test start id={} kind={:?} leaves={}",
        cfg.id,
        cfg.kind,
        provider.leaves_this_mac()
    );
    let started = Instant::now();
    let outcome = provider.complete(
        "You are a connection test. Reply with the single word OK.",
        "Say OK.",
        TEST_MAX_TOKENS,
        &CancelToken::new(),
    );
    let latency_ms = started.elapsed().as_millis() as u64;
    crate::diagnostics::record_duration("model_providers", "test_connection", latency_ms);
    let result = match outcome {
        Ok(reply) => {
            log::info!(
                "model_providers test finish id={} ok=true ms={latency_ms}",
                cfg.id
            );
            TestResult {
                provider_id: cfg.id.clone(),
                ok: true,
                latency_ms,
                reply: Some(reply.chars().take(200).collect()),
                error: None,
                key_hint,
            }
        }
        Err(e) => {
            log::warn!(
                "model_providers test failed id={} reason={} ms={latency_ms}",
                cfg.id,
                e.tag()
            );
            TestResult {
                provider_id: cfg.id.clone(),
                ok: false,
                latency_ms,
                reply: None,
                error: Some(e.plain()),
                key_hint,
            }
        }
    };
    emit(
        TEST_EVENT,
        serde_json::json!({
            "providerId": result.provider_id,
            "phase": "finish",
            "ok": result.ok,
            "latencyMs": result.latency_ms,
        }),
    );
    Ok(result)
}
