//! yap-1106-byom-cloud-fast-path — the cloud route.
//!
//! When the user has entered a key for the "Cloud model (your key)" provider
//! and picked **Cloud when online**, dictation cleanup and meeting notes run on
//! the hosted model first, because it is much faster than the bundled one.
//! Every cloud attempt has a budget; past it — or on any error — the SAME job
//! runs on the bundled local model. Falling back is a route, never a failure,
//! and the take is never lost.
//!
//! This module is pure policy, so every rule is a unit test:
//! * [`CloudMode`] — Always on this Mac (default) / Cloud when online / Cloud only.
//! * [`budget_for`] — TTFT ≤ 1.5 s + total ≤ 4 s for cleanup, 5 s + 90 s for notes.
//! * [`retry_delay`] — at most ONE retry, only on 429 / 5xx / overloaded /
//!   connection reset, jittered 250–750 ms, `retry-after` honoured when it fits.
//! * [`CircuitBreaker`] — 3 failures in 2 min → local for 5 min → one half-open probe.
//! * [`Ledger`] — a per-day token ledger priced with [`price_for`]; the daily
//!   cap (default $6.50) holds across restarts because it lives in `settings_kv`.
//! * [`decide`] — mode + path + breaker + cap → [`Decision`].
//! * [`run_routed`] — the runner every cloud-capable job goes through, with the
//!   one `byom route=… reason=… job=… …` telemetry line per take.
//!
//! No network identifier lives here: the HTTP streaming is `cloud_client.rs`
//! over `http_client.rs`, and a test drives this module with closures.
//!
//! Offline detection: the shipped probe is [`AssumeOnline`] guarded by the
//! breaker (an unreachable host fails fast and opens the breaker). A real
//! `NWPathMonitor` probe needs a Network.framework binding this crate does
//! not carry yet; [`OnlineProbe`] is the seam it plugs into.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::model_providers::SettingsKv;

/// `settings_kv` row: the mode (`always_local` default).
pub const MODE_KEY: &str = "cloud_mode";
/// `settings_kv` row: the daily cap in USD (text, default [`DEFAULT_DAILY_CAP_USD`]).
pub const CAP_KEY: &str = "cloud_daily_cap_usd";
/// `settings_kv` row: the token ledger (JSON [`Ledger`]).
pub const LEDGER_KEY: &str = "cloud_spend_ledger_v1";
/// `settings_kv` row: `"1"` after a 4xx auth / validation answer, cleared on
/// the next cloud success or a new key. Settings shows one line for it.
pub const KEY_ATTENTION_KEY: &str = "cloud_key_needs_attention";
/// `settings_kv` row: may notes use the cloud on a constrained path?
/// (`"allow"` / `"ask"` default — "ask" means: not this time.)
pub const CONSTRAINED_NOTES_KEY: &str = "cloud_constrained_notes";
/// `settings_kv` row: the notes model pick (`fast` default / `best`).
pub const NOTES_MODEL_KEY: &str = "cloud_notes_model";

/// $200 / month ÷ 31 days, rounded to the cent.
pub const DEFAULT_DAILY_CAP_USD: f64 = 6.50;
/// Where the 80 % "quiet note" starts.
pub const CAP_WARN_FRACTION: f64 = 0.8;

/// The price table below was read from the API reference on this date.
pub const PRICES_AS_OF: &str = "2026-11-06";
/// Cleanup, formatting and (fast) summaries. Diagnostics-only: never shown.
pub const CLEANUP_MODEL: &str = "claude-sonnet-5-5";
/// The selectable "best" notes model. Diagnostics-only: never shown.
pub const NOTES_MODEL_BEST: &str = "claude-opus-5-5";
/// The customer-facing name of the provider. The only label the UI uses.
pub const CLOUD_LABEL: &str = "Cloud model (your key)";
/// The pill chip texts.
pub const CHIP_CLOUD: &str = "Cloud";
pub const CHIP_LOCAL: &str = "On this Mac";
/// The pill line when the cap is reached.
pub const CAP_REACHED_LINE: &str = "Cloud limit reached today, using this Mac";
/// The Settings line after a 4xx auth / validation answer.
pub const KEY_ATTENTION_LINE: &str = "Cloud key needs attention";

pub const BREAKER_FAILURES: usize = 3;
pub const BREAKER_WINDOW: Duration = Duration::from_secs(120);
pub const BREAKER_OPEN_FOR: Duration = Duration::from_secs(300);
pub const RETRY_JITTER_MIN: Duration = Duration::from_millis(250);
pub const RETRY_JITTER_SPAN_MS: u64 = 500;

// ── Mode ─────────────────────────────────────────────────────────────────────

/// Settings › Models & privacy › mode switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CloudMode {
    /// The default for a new user: nothing leaves this Mac.
    #[default]
    AlwaysLocal,
    /// "Cloud when online (faster)".
    CloudWhenOnline,
    /// "Cloud only": the cloud is always preferred; a failed cloud call STILL
    /// runs locally so the take is never lost (that is a route, not an error).
    CloudOnly,
}

impl CloudMode {
    pub fn as_str(self) -> &'static str {
        match self {
            CloudMode::AlwaysLocal => "always_local",
            CloudMode::CloudWhenOnline => "cloud_when_online",
            CloudMode::CloudOnly => "cloud_only",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "always_local" => Some(CloudMode::AlwaysLocal),
            "cloud_when_online" => Some(CloudMode::CloudWhenOnline),
            "cloud_only" => Some(CloudMode::CloudOnly),
            _ => None,
        }
    }

    pub fn load(kv: &dyn SettingsKv) -> Self {
        kv.kv_get(MODE_KEY)
            .and_then(|s| Self::parse(&s))
            .unwrap_or_default()
    }

    pub fn save(kv: &dyn SettingsKv, mode: CloudMode) -> Result<(), String> {
        kv.kv_set(MODE_KEY, mode.as_str())?;
        log::info!("byom mode_set mode={}", mode.as_str());
        Ok(())
    }

    pub fn wants_cloud(self) -> bool {
        !matches!(self, CloudMode::AlwaysLocal)
    }
}

// ── Jobs and budgets ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Job {
    Polish,
    Format,
    Notes,
}

impl Job {
    pub fn as_str(self) -> &'static str {
        match self {
            Job::Polish => "polish",
            Job::Format => "format",
            Job::Notes => "notes",
        }
    }
}

/// Time to first token and the whole-request ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub ttft: Duration,
    pub total: Duration,
}

/// DO 4: cleanup 1.5 s / 4 s; notes 5 s / 90 s (chunked by the caller).
pub fn budget_for(job: Job) -> Budget {
    match job {
        Job::Polish | Job::Format => Budget {
            ttft: Duration::from_millis(1500),
            total: Duration::from_secs(4),
        },
        Job::Notes => Budget {
            ttft: Duration::from_secs(5),
            total: Duration::from_secs(90),
        },
    }
}

// ── Offline detection ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathStatus {
    Satisfied,
    Unsatisfied,
    /// Expensive (hotspot) or constrained (Low Data Mode).
    Constrained,
}

pub trait OnlineProbe: Send + Sync {
    fn path(&self) -> PathStatus;
}

/// The shipped probe until an `NWPathMonitor` binding exists: assume online
/// and let the breaker catch an unreachable host.
pub struct AssumeOnline;
impl OnlineProbe for AssumeOnline {
    fn path(&self) -> PathStatus {
        PathStatus::Satisfied
    }
}

/// A fixed answer (tests, and the "offline → no request" acceptance).
pub struct FixedPath(pub PathStatus);
impl OnlineProbe for FixedPath {
    fn path(&self) -> PathStatus {
        self.0
    }
}

// ── Decision ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    Cloud,
    Local,
}

impl Route {
    pub fn as_str(self) -> &'static str {
        match self {
            Route::Cloud => "cloud",
            Route::Local => "local",
        }
    }
    /// The pill chip text.
    pub fn chip(self) -> &'static str {
        match self {
            Route::Cloud => CHIP_CLOUD,
            Route::Local => CHIP_LOCAL,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Online,
    Offline,
    Breaker,
    Timeout,
    Cap,
    User,
    Error,
    Constrained,
    /// No key for the cloud provider, or the provider is not configured.
    NoKey,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Online => "online",
            Reason::Offline => "offline",
            Reason::Breaker => "breaker",
            Reason::Timeout => "timeout",
            Reason::Cap => "cap",
            Reason::User => "user",
            Reason::Error => "error",
            Reason::Constrained => "constrained",
            Reason::NoKey => "no_key",
        }
    }

    /// The chip's tooltip, in the user's words. No vendor, no model.
    pub fn tooltip(self, route: Route) -> &'static str {
        match (route, self) {
            (Route::Cloud, _) => "Written in the cloud with your key",
            (Route::Local, Reason::Offline) => "Offline — written on this Mac",
            (Route::Local, Reason::Breaker) => {
                "The cloud failed a few times — using this Mac for a few minutes"
            }
            (Route::Local, Reason::Timeout) => "The cloud was slow — finished on this Mac",
            (Route::Local, Reason::Cap) => CAP_REACHED_LINE,
            (Route::Local, Reason::User) => "Always on this Mac (your setting)",
            (Route::Local, Reason::Error) => "The cloud answered with an error — finished on this Mac",
            (Route::Local, Reason::Constrained) => {
                "Low-data connection — notes stay on this Mac"
            }
            (Route::Local, Reason::NoKey) => "No cloud key yet — written on this Mac",
            (Route::Local, Reason::Online) => "Written on this Mac",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub route: Route,
    pub reason: Reason,
}

impl Decision {
    pub const fn local(reason: Reason) -> Self {
        Self {
            route: Route::Local,
            reason,
        }
    }
    pub const fn cloud() -> Self {
        Self {
            route: Route::Cloud,
            reason: Reason::Online,
        }
    }
}

// ── Circuit breaker ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerState {
    Closed,
    Open,
    HalfOpen,
}

/// 3 failures in 2 minutes → everything local for 5 minutes → one half-open
/// probe; a success closes it, a failure re-opens it for another 5 minutes.
#[derive(Debug, Default)]
pub struct CircuitBreaker {
    failures: Vec<Instant>,
    opened_at: Option<Instant>,
    probe_in_flight: bool,
}

impl CircuitBreaker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self, now: Instant) -> BreakerState {
        match self.opened_at {
            None => BreakerState::Closed,
            Some(at) if now.duration_since(at) >= BREAKER_OPEN_FOR => BreakerState::HalfOpen,
            Some(_) => BreakerState::Open,
        }
    }

    /// May a cloud request go out right now? Half-open admits exactly one
    /// probe until it reports back.
    pub fn allow(&mut self, now: Instant) -> bool {
        match self.state(now) {
            BreakerState::Closed => true,
            BreakerState::Open => false,
            BreakerState::HalfOpen => {
                if self.probe_in_flight {
                    false
                } else {
                    self.probe_in_flight = true;
                    true
                }
            }
        }
    }

    pub fn record_success(&mut self) {
        let was_open = self.opened_at.is_some();
        self.failures.clear();
        self.opened_at = None;
        self.probe_in_flight = false;
        if was_open {
            log::info!("byom breaker_closed reason=probe_ok");
        }
    }

    pub fn record_failure(&mut self, now: Instant) {
        if self.opened_at.is_some() {
            // A failed half-open probe re-opens for another full window.
            self.opened_at = Some(now);
            self.probe_in_flight = false;
            log::warn!("byom breaker_reopened reason=probe_failed");
            return;
        }
        self.failures
            .retain(|t| now.duration_since(*t) < BREAKER_WINDOW);
        self.failures.push(now);
        if self.failures.len() >= BREAKER_FAILURES {
            self.opened_at = Some(now);
            self.failures.clear();
            log::warn!(
                "byom breaker_opened failures={} window_s={} open_s={}",
                BREAKER_FAILURES,
                BREAKER_WINDOW.as_secs(),
                BREAKER_OPEN_FOR.as_secs()
            );
        }
    }

    pub fn is_open(&self, now: Instant) -> bool {
        self.state(now) == BreakerState::Open
    }
}

// ── Prices and the ledger ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prices {
    pub input_per_million: f64,
    pub output_per_million: f64,
}

/// USD per million tokens, as of [`PRICES_AS_OF`]. An unknown model is priced
/// at the dearer row so the cap can never under-count.
pub fn price_for(model: &str) -> Prices {
    match model.trim() {
        "claude-sonnet-5-5" => Prices {
            input_per_million: 2.0,
            output_per_million: 10.0,
        },
        "claude-opus-5-5" => Prices {
            input_per_million: 4.0,
            output_per_million: 20.0,
        },
        _ => Prices {
            input_per_million: 4.0,
            output_per_million: 20.0,
        },
    }
}

pub fn cost_usd(model: &str, tokens_in: u64, tokens_out: u64) -> f64 {
    let p = price_for(model);
    (tokens_in as f64 * p.input_per_million + tokens_out as f64 * p.output_per_million) / 1e6
}

/// The daily cap (USD). Clamped to a sane range on write.
pub fn daily_cap(kv: &dyn SettingsKv) -> f64 {
    kv.kv_get(CAP_KEY)
        .and_then(|s| s.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(DEFAULT_DAILY_CAP_USD)
}

pub fn set_daily_cap(kv: &dyn SettingsKv, usd: f64) -> Result<f64, String> {
    if !usd.is_finite() || usd <= 0.0 {
        return Err("the daily limit must be a positive amount".into());
    }
    let v = usd.clamp(0.25, 1000.0);
    kv.kv_set(CAP_KEY, &format!("{v:.2}"))?;
    log::info!("byom cap_set usd={v:.2}");
    Ok(v)
}

/// The token ledger: one day row and one month row, both rolled over by date.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Ledger {
    pub day: String,
    pub day_tokens_in: u64,
    pub day_tokens_out: u64,
    pub day_cost_usd: f64,
    pub day_calls: u64,
    pub month: String,
    pub month_tokens_in: u64,
    pub month_tokens_out: u64,
    pub month_cost_usd: f64,
    pub month_calls: u64,
}

impl Ledger {
    /// Read the ledger and roll it to `today` (a new day zeroes the day row, a
    /// new month zeroes both). A malformed row is logged and starts fresh.
    pub fn load(kv: &dyn SettingsKv, today: NaiveDate) -> Self {
        let mut l: Ledger = kv
            .kv_get(LEDGER_KEY)
            .and_then(|raw| {
                serde_json::from_str(&raw)
                    .map_err(|e| {
                        log::warn!("byom ledger_malformed error={e}");
                        e
                    })
                    .ok()
            })
            .unwrap_or_default();
        l.roll(today);
        l
    }

    fn roll(&mut self, today: NaiveDate) {
        let day = today.format("%Y-%m-%d").to_string();
        let month = today.format("%Y-%m").to_string();
        if self.month != month {
            *self = Ledger {
                day: day.clone(),
                month,
                ..Default::default()
            };
        }
        if self.day != day {
            self.day = day;
            self.day_tokens_in = 0;
            self.day_tokens_out = 0;
            self.day_cost_usd = 0.0;
            self.day_calls = 0;
        }
    }

    pub fn save(&self, kv: &dyn SettingsKv) -> Result<(), String> {
        let json = serde_json::to_string(self).map_err(|e| e.to_string())?;
        kv.kv_set(LEDGER_KEY, &json)
    }

    /// Add one call's tokens; returns its cost in USD.
    pub fn record(&mut self, today: NaiveDate, model: &str, tokens_in: u64, tokens_out: u64) -> f64 {
        self.roll(today);
        let cost = cost_usd(model, tokens_in, tokens_out);
        self.day_tokens_in += tokens_in;
        self.day_tokens_out += tokens_out;
        self.day_cost_usd += cost;
        self.day_calls += 1;
        self.month_tokens_in += tokens_in;
        self.month_tokens_out += tokens_out;
        self.month_cost_usd += cost;
        self.month_calls += 1;
        cost
    }

    pub fn day_fraction(&self, cap_usd: f64) -> f64 {
        if cap_usd <= 0.0 {
            return 1.0;
        }
        self.day_cost_usd / cap_usd
    }

    pub fn cap_reached(&self, cap_usd: f64) -> bool {
        self.day_fraction(cap_usd) >= 1.0
    }

    pub fn warn_reached(&self, cap_usd: f64) -> bool {
        self.day_fraction(cap_usd) >= CAP_WARN_FRACTION
    }

    /// The `byom daily …` summary line.
    pub fn summary_line(&self, cap_usd: f64) -> String {
        format!(
            "byom daily day={} calls={} tokens_in={} tokens_out={} cost_usd={:.4} cap_usd={:.2} mtd_usd={:.4}",
            self.day,
            self.day_calls,
            self.day_tokens_in,
            self.day_tokens_out,
            self.day_cost_usd,
            cap_usd,
            self.month_cost_usd
        )
    }
}

/// What Settings shows: the cap, today's and the month's spend.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendView {
    pub cap_usd: f64,
    pub today_usd: f64,
    pub month_to_date_usd: f64,
    pub today_fraction: f64,
    pub cap_reached: bool,
    pub warn: bool,
    pub prices_as_of: &'static str,
}

pub fn spend_view(kv: &dyn SettingsKv, today: NaiveDate) -> SpendView {
    let cap = daily_cap(kv);
    let l = Ledger::load(kv, today);
    SpendView {
        cap_usd: cap,
        today_usd: l.day_cost_usd,
        month_to_date_usd: l.month_cost_usd,
        today_fraction: l.day_fraction(cap),
        cap_reached: l.cap_reached(cap),
        warn: l.warn_reached(cap),
        prices_as_of: PRICES_AS_OF,
    }
}

pub fn key_needs_attention(kv: &dyn SettingsKv) -> bool {
    kv.kv_get(KEY_ATTENTION_KEY).as_deref() == Some("1")
}

pub fn set_key_attention(kv: &dyn SettingsKv, on: bool) {
    if let Err(e) = kv.kv_set(KEY_ATTENTION_KEY, if on { "1" } else { "" }) {
        log::warn!("byom key_attention_save_failed error={e}");
    }
}

// ── Errors and the retry policy ──────────────────────────────────────────────

/// Why a cloud attempt did not produce a complete, validated answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudError {
    /// A non-2xx status; `retry_after` from the header when present.
    Status {
        code: u16,
        retry_after: Option<Duration>,
    },
    /// 529 / `overloaded_error`.
    Overloaded { retry_after: Option<Duration> },
    ConnectionReset,
    Unreachable,
    /// No first token inside the budget.
    Ttft,
    /// The whole answer did not arrive inside the budget.
    Total,
    /// The stream could not be parsed.
    Protocol,
    /// The caller's validator refused the complete answer.
    Rejected,
    Cancelled,
}

impl CloudError {
    pub fn tag(&self) -> &'static str {
        match self {
            CloudError::Status { code, .. } => match code {
                401 | 403 => "auth",
                400 | 404 | 413 | 422 => "invalid",
                429 => "rate_limited",
                500..=599 => "server",
                _ => "http",
            },
            CloudError::Overloaded { .. } => "overloaded",
            CloudError::ConnectionReset => "conn_reset",
            CloudError::Unreachable => "unreachable",
            CloudError::Ttft => "ttft",
            CloudError::Total => "total",
            CloudError::Protocol => "protocol",
            CloudError::Rejected => "rejected",
            CloudError::Cancelled => "cancelled",
        }
    }

    /// DO 5: 429, 5xx, overloaded, connection reset — nothing else.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            CloudError::Status { code: 429, .. }
                | CloudError::Status {
                    code: 500..=599,
                    ..
                }
                | CloudError::Overloaded { .. }
                | CloudError::ConnectionReset
        )
    }

    /// A 4xx auth or validation answer: straight to local, flag the key.
    pub fn needs_key_attention(&self) -> bool {
        matches!(
            self,
            CloudError::Status {
                code: 400 | 401 | 403 | 404 | 413 | 422,
                ..
            }
        )
    }

    pub fn is_timeout(&self) -> bool {
        matches!(self, CloudError::Ttft | CloudError::Total)
    }

    fn retry_after(&self) -> Option<Duration> {
        match self {
            CloudError::Status { retry_after, .. } | CloudError::Overloaded { retry_after } => {
                *retry_after
            }
            _ => None,
        }
    }
}

/// The wait before the ONE retry, or `None` for no retry. `jitter` is in
/// `0.0..=1.0` (the caller draws it); the backoff is 250 + 500 × jitter ms.
/// A `retry-after` replaces the backoff when it still fits inside the budget;
/// one that does not fit means no retry at all.
pub fn retry_delay(
    err: &CloudError,
    attempt: u32,
    elapsed: Duration,
    budget: Budget,
    jitter: f64,
) -> Option<Duration> {
    if attempt >= 1 || !err.retryable() {
        return None;
    }
    let jitter = jitter.clamp(0.0, 1.0);
    let backoff =
        RETRY_JITTER_MIN + Duration::from_millis((RETRY_JITTER_SPAN_MS as f64 * jitter) as u64);
    let wait = match err.retry_after() {
        Some(ra) if ra > backoff => ra,
        _ => backoff,
    };
    // The retry must still leave room for a first token.
    if elapsed + wait + budget.ttft > budget.total {
        return None;
    }
    Some(wait)
}

/// A cheap jitter draw in `0.0..1.0` (no RNG crate on the take path).
pub fn jitter_now() -> f64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    (nanos % 1000) as f64 / 1000.0
}

// ── The routed run ───────────────────────────────────────────────────────────

/// A complete cloud answer with what it cost.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CloudReply {
    pub text: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub ttft_ms: u64,
    pub total_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Fallback {
    None,
    Timeout,
    Error,
}

impl Fallback {
    pub fn as_str(self) -> &'static str {
        match self {
            Fallback::None => "none",
            Fallback::Timeout => "timeout",
            Fallback::Error => "error",
        }
    }
}

/// What a routed job reports back beside its value.
#[derive(Debug, Clone, PartialEq)]
pub struct Routed<R> {
    pub value: R,
    pub route: Route,
    pub reason: Reason,
    pub fallback: Fallback,
    pub attempts: u32,
    pub ttft_ms: u64,
    pub total_ms: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_usd: f64,
}

/// Everything [`decide`] and [`run_routed`] need, borrowed for one job.
pub struct RouteContext<'a> {
    pub mode: CloudMode,
    pub has_key: bool,
    pub probe: &'a dyn OnlineProbe,
    pub breaker: &'a Mutex<CircuitBreaker>,
    pub kv: &'a dyn SettingsKv,
    pub today: NaiveDate,
    pub job: Job,
    pub model: &'a str,
    pub budget: Budget,
    /// `Some(true)`: notes may use a constrained path (user allowed it).
    pub constrained_notes_allowed: bool,
}

/// DO 7/8/6/11 in order: mode, key, path, breaker, cap.
pub fn decide(ctx: &RouteContext<'_>, now: Instant) -> Decision {
    if !ctx.mode.wants_cloud() {
        return Decision::local(Reason::User);
    }
    if !ctx.has_key {
        return Decision::local(Reason::NoKey);
    }
    match ctx.probe.path() {
        PathStatus::Unsatisfied => return Decision::local(Reason::Offline),
        PathStatus::Constrained if ctx.job == Job::Notes && !ctx.constrained_notes_allowed => {
            return Decision::local(Reason::Constrained)
        }
        _ => {}
    }
    let cap = daily_cap(ctx.kv);
    if Ledger::load(ctx.kv, ctx.today).cap_reached(cap) {
        return Decision::local(Reason::Cap);
    }
    let allowed = ctx
        .breaker
        .lock()
        .map(|mut b| b.allow(now))
        .unwrap_or(false);
    if !allowed {
        return Decision::local(Reason::Breaker);
    }
    Decision::cloud()
}

/// Run `job` on the cloud when [`decide`] says so, with ONE retry per
/// [`retry_delay`], and on any failure run the SAME job locally. `cloud` is
/// called with the budget and the attempt number; `accept` validates the
/// complete cloud text (a `None` is a [`CloudError::Rejected`] → local).
/// `local` always produces a value: the take is never lost.
pub fn run_routed<R>(
    ctx: &RouteContext<'_>,
    cloud: &mut dyn FnMut(Budget, u32) -> Result<CloudReply, CloudError>,
    accept: &dyn Fn(&str) -> Option<R>,
    local: &mut dyn FnMut() -> R,
) -> Routed<R> {
    let started = Instant::now();
    let decision = decide(ctx, started);
    let mut out = Routed {
        value: None::<R>,
        route: decision.route,
        reason: decision.reason,
        fallback: Fallback::None,
        attempts: 0,
        ttft_ms: 0,
        total_ms: 0,
        tokens_in: 0,
        tokens_out: 0,
        cost_usd: 0.0,
    };

    if decision.route == Route::Cloud {
        let mut attempt = 0u32;
        loop {
            out.attempts = attempt + 1;
            let result = cloud(ctx.budget, attempt);
            match result {
                Ok(reply) => {
                    out.ttft_ms = reply.ttft_ms;
                    out.total_ms = reply.total_ms;
                    out.tokens_in = reply.tokens_in;
                    out.tokens_out = reply.tokens_out;
                    // Spend is real whether or not the answer is accepted.
                    let mut ledger = Ledger::load(ctx.kv, ctx.today);
                    out.cost_usd =
                        ledger.record(ctx.today, ctx.model, reply.tokens_in, reply.tokens_out);
                    if let Err(e) = ledger.save(ctx.kv) {
                        log::warn!("byom ledger_save_failed error={e}");
                    }
                    let cap = daily_cap(ctx.kv);
                    if ledger.cap_reached(cap) {
                        log::warn!("byom cap_reached day={} cost_usd={:.4} cap_usd={cap:.2}", ledger.day, ledger.day_cost_usd);
                        crate::diagnostics::set_state("byom.cap", "reached");
                    } else if ledger.warn_reached(cap) {
                        log::info!("byom cap_warn day={} fraction={:.2}", ledger.day, ledger.day_fraction(cap));
                        crate::diagnostics::set_state("byom.cap", "warn");
                    } else {
                        crate::diagnostics::set_state("byom.cap", "ok");
                    }
                    log::info!("{}", ledger.summary_line(cap));
                    if let Ok(mut b) = ctx.breaker.lock() {
                        b.record_success();
                    }
                    set_key_attention(ctx.kv, false);
                    match accept(&reply.text) {
                        Some(v) => {
                            out.value = Some(v);
                        }
                        None => {
                            log::warn!(
                                "byom cloud_rejected job={} reason=validator",
                                ctx.job.as_str()
                            );
                            out.fallback = Fallback::Error;
                            out.reason = Reason::Error;
                        }
                    }
                    break;
                }
                Err(err) => {
                    let elapsed = started.elapsed();
                    log::warn!(
                        "byom cloud_failed job={} attempt={} error={} elapsed_ms={}",
                        ctx.job.as_str(),
                        attempt + 1,
                        err.tag(),
                        elapsed.as_millis()
                    );
                    if err.needs_key_attention() {
                        set_key_attention(ctx.kv, true);
                    }
                    if err == CloudError::Cancelled {
                        out.route = Route::Local;
                        out.reason = Reason::Error;
                        out.fallback = Fallback::Error;
                        break;
                    }
                    if let Some(wait) = retry_delay(&err, attempt, elapsed, ctx.budget, jitter_now()) {
                        log::info!(
                            "byom retry job={} after_ms={} error={}",
                            ctx.job.as_str(),
                            wait.as_millis(),
                            err.tag()
                        );
                        std::thread::sleep(wait);
                        attempt += 1;
                        continue;
                    }
                    if let Ok(mut b) = ctx.breaker.lock() {
                        b.record_failure(Instant::now());
                    }
                    out.fallback = if err.is_timeout() {
                        Fallback::Timeout
                    } else {
                        Fallback::Error
                    };
                    out.reason = if err.is_timeout() {
                        Reason::Timeout
                    } else {
                        Reason::Error
                    };
                    break;
                }
            }
        }
    }

    let value = match out.value.take() {
        Some(v) => v,
        None => {
            out.route = Route::Local;
            let v = local();
            if out.total_ms == 0 {
                out.total_ms = started.elapsed().as_millis() as u64;
            }
            v
        }
    };
    let done = Routed {
        value,
        route: out.route,
        reason: out.reason,
        fallback: out.fallback,
        attempts: out.attempts,
        ttft_ms: out.ttft_ms,
        total_ms: out.total_ms,
        tokens_in: out.tokens_in,
        tokens_out: out.tokens_out,
        cost_usd: out.cost_usd,
    };
    log::info!(
        "byom route={} reason={} job={} ttft_ms={} total_ms={} tokens_in={} tokens_out={} cost_usd={:.5} fallback={} attempts={}",
        done.route.as_str(),
        done.reason.as_str(),
        ctx.job.as_str(),
        done.ttft_ms,
        done.total_ms,
        done.tokens_in,
        done.tokens_out,
        done.cost_usd,
        done.fallback.as_str(),
        done.attempts
    );
    crate::diagnostics::set_state(
        "byom.route",
        format!(
            "{} ({}) job={} model={}",
            done.route.as_str(),
            done.reason.as_str(),
            ctx.job.as_str(),
            ctx.model
        ),
    );
    done
}

/// What the pill reads: the last route, for the chip + tooltip (DO 9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteChip {
    pub route: Route,
    pub reason: Reason,
    pub label: String,
    pub tooltip: String,
    pub job: Job,
}

impl RouteChip {
    pub fn new(route: Route, reason: Reason, job: Job) -> Self {
        Self {
            route,
            reason,
            label: route.chip().to_string(),
            tooltip: reason.tooltip(route).to_string(),
            job,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct Kv(StdMutex<HashMap<String, String>>);
    impl SettingsKv for Kv {
        fn kv_get(&self, key: &str) -> Option<String> {
            self.0.lock().unwrap().get(key).cloned()
        }
        fn kv_set(&self, key: &str, value: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }
    }

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn mode_defaults_to_always_local_and_round_trips() {
        let kv = Kv::default();
        assert_eq!(CloudMode::load(&kv), CloudMode::AlwaysLocal);
        CloudMode::save(&kv, CloudMode::CloudWhenOnline).unwrap();
        assert_eq!(CloudMode::load(&kv), CloudMode::CloudWhenOnline);
        assert_eq!(CloudMode::parse("garbage"), None);
    }

    #[test]
    fn budgets_match_the_spec() {
        let p = budget_for(Job::Polish);
        assert_eq!(p.ttft, Duration::from_millis(1500));
        assert_eq!(p.total, Duration::from_secs(4));
        let n = budget_for(Job::Notes);
        assert_eq!(n.ttft, Duration::from_secs(5));
        assert_eq!(n.total, Duration::from_secs(90));
    }

    #[test]
    fn retry_only_once_only_on_retryable_errors() {
        let b = budget_for(Job::Polish);
        let e429 = CloudError::Status {
            code: 429,
            retry_after: None,
        };
        let d = retry_delay(&e429, 0, Duration::from_millis(100), b, 0.5).unwrap();
        assert_eq!(d, Duration::from_millis(500));
        assert!(retry_delay(&e429, 1, Duration::from_millis(100), b, 0.5).is_none());
        for code in [400u16, 401, 403, 404, 422] {
            let e = CloudError::Status {
                code,
                retry_after: None,
            };
            assert!(retry_delay(&e, 0, Duration::ZERO, b, 0.0).is_none(), "{code}");
            assert!(e.needs_key_attention(), "{code}");
        }
        assert!(retry_delay(&CloudError::Ttft, 0, Duration::ZERO, b, 0.0).is_none());
        assert!(retry_delay(&CloudError::ConnectionReset, 0, Duration::ZERO, b, 0.0).is_some());
        assert!(retry_delay(&CloudError::Overloaded { retry_after: None }, 0, Duration::ZERO, b, 1.0)
            == Some(Duration::from_millis(750)));
        // retry-after inside the budget is honoured; outside it, no retry.
        let ra = CloudError::Status {
            code: 503,
            retry_after: Some(Duration::from_millis(1200)),
        };
        assert_eq!(
            retry_delay(&ra, 0, Duration::ZERO, b, 0.0),
            Some(Duration::from_millis(1200))
        );
        let ra_far = CloudError::Status {
            code: 503,
            retry_after: Some(Duration::from_secs(30)),
        };
        assert!(retry_delay(&ra_far, 0, Duration::ZERO, b, 0.0).is_none());
        // No room for a first token after the wait → no retry.
        assert!(retry_delay(&e429, 0, Duration::from_millis(2600), b, 0.0).is_none());
    }

    #[test]
    fn breaker_opens_after_three_in_two_minutes_then_half_opens() {
        let mut b = CircuitBreaker::new();
        let t0 = Instant::now();
        assert!(b.allow(t0));
        b.record_failure(t0);
        b.record_failure(t0 + Duration::from_secs(10));
        assert!(b.allow(t0 + Duration::from_secs(11)));
        b.record_failure(t0 + Duration::from_secs(20));
        assert_eq!(b.state(t0 + Duration::from_secs(21)), BreakerState::Open);
        assert!(!b.allow(t0 + Duration::from_secs(21)));
        assert!(!b.allow(t0 + Duration::from_secs(299)));
        // Half-open: exactly one probe.
        let t_half = t0 + Duration::from_secs(20) + BREAKER_OPEN_FOR;
        assert_eq!(b.state(t_half), BreakerState::HalfOpen);
        assert!(b.allow(t_half));
        assert!(!b.allow(t_half));
        // A failed probe re-opens for another window.
        b.record_failure(t_half);
        assert!(!b.allow(t_half + Duration::from_secs(1)));
        // A successful probe closes it.
        let t2 = t_half + BREAKER_OPEN_FOR;
        assert!(b.allow(t2));
        b.record_success();
        assert_eq!(b.state(t2), BreakerState::Closed);
        assert!(b.allow(t2));
    }

    #[test]
    fn breaker_window_forgets_old_failures() {
        let mut b = CircuitBreaker::new();
        let t0 = Instant::now();
        b.record_failure(t0);
        b.record_failure(t0 + Duration::from_secs(1));
        // The third failure lands after the first two aged out.
        b.record_failure(t0 + BREAKER_WINDOW + Duration::from_secs(5));
        assert_eq!(b.state(t0 + BREAKER_WINDOW + Duration::from_secs(6)), BreakerState::Closed);
    }

    #[test]
    fn ledger_prices_rolls_and_persists_across_restarts() {
        let kv = Kv::default();
        let d1 = day("2026-11-06");
        let mut l = Ledger::load(&kv, d1);
        let c = l.record(d1, CLEANUP_MODEL, 1_000_000, 100_000);
        assert!((c - 3.0).abs() < 1e-9, "{c}");
        l.save(&kv).unwrap();
        // "Restart": a fresh load sees the same day row.
        let again = Ledger::load(&kv, d1);
        assert_eq!(again.day_cost_usd, l.day_cost_usd);
        assert_eq!(again.day_calls, 1);
        // Next day: day resets, month keeps.
        let d2 = day("2026-11-07");
        let rolled = Ledger::load(&kv, d2);
        assert_eq!(rolled.day_cost_usd, 0.0);
        assert!((rolled.month_cost_usd - 3.0).abs() < 1e-9);
        // Next month: both reset.
        let m2 = Ledger::load(&kv, day("2026-12-01"));
        assert_eq!(m2.month_cost_usd, 0.0);
        // Unknown models are priced at the dearer row.
        assert!(cost_usd("mystery", 1_000_000, 0) >= cost_usd(CLEANUP_MODEL, 1_000_000, 0));
        assert_eq!(price_for(NOTES_MODEL_BEST).output_per_million, 20.0);
    }

    #[test]
    fn cap_defaults_and_holds_across_restarts() {
        let kv = Kv::default();
        assert_eq!(daily_cap(&kv), DEFAULT_DAILY_CAP_USD);
        assert!(set_daily_cap(&kv, -1.0).is_err());
        set_daily_cap(&kv, 1.0).unwrap();
        let d = day("2026-11-06");
        let mut l = Ledger::load(&kv, d);
        l.record(d, CLEANUP_MODEL, 0, 85_000); // $0.85
        l.save(&kv).unwrap();
        let v = spend_view(&kv, d);
        assert!(v.warn && !v.cap_reached, "{v:?}");
        let mut l = Ledger::load(&kv, d);
        l.record(d, CLEANUP_MODEL, 0, 20_000); // +$0.20 → $1.05
        l.save(&kv).unwrap();
        // A restart (fresh load + fresh decision) still sees the cap.
        let v = spend_view(&kv, d);
        assert!(v.cap_reached, "{v:?}");
        let breaker = Mutex::new(CircuitBreaker::new());
        let ctx = RouteContext {
            mode: CloudMode::CloudWhenOnline,
            has_key: true,
            probe: &AssumeOnline,
            breaker: &breaker,
            kv: &kv,
            today: d,
            job: Job::Polish,
            model: CLEANUP_MODEL,
            budget: budget_for(Job::Polish),
            constrained_notes_allowed: false,
        };
        assert_eq!(decide(&ctx, Instant::now()), Decision::local(Reason::Cap));
        // Tomorrow the cloud is back.
        let ctx2 = RouteContext {
            today: day("2026-11-07"),
            ..ctx
        };
        assert_eq!(decide(&ctx2, Instant::now()), Decision::cloud());
    }

    #[test]
    fn decide_order_user_key_path_breaker() {
        let kv = Kv::default();
        let breaker = Mutex::new(CircuitBreaker::new());
        let base = RouteContext {
            mode: CloudMode::AlwaysLocal,
            has_key: true,
            probe: &AssumeOnline,
            breaker: &breaker,
            kv: &kv,
            today: day("2026-11-06"),
            job: Job::Polish,
            model: CLEANUP_MODEL,
            budget: budget_for(Job::Polish),
            constrained_notes_allowed: false,
        };
        let now = Instant::now();
        assert_eq!(decide(&base, now), Decision::local(Reason::User));
        let on = RouteContext {
            mode: CloudMode::CloudWhenOnline,
            ..base
        };
        assert_eq!(decide(&on, now), Decision::cloud());
        let nokey = RouteContext {
            has_key: false,
            ..on
        };
        assert_eq!(decide(&nokey, now), Decision::local(Reason::NoKey));
        let off = FixedPath(PathStatus::Unsatisfied);
        let offline = RouteContext { probe: &off, ..on };
        assert_eq!(decide(&offline, now), Decision::local(Reason::Offline));
        let slow = FixedPath(PathStatus::Constrained);
        let notes = RouteContext {
            probe: &slow,
            job: Job::Notes,
            ..on
        };
        assert_eq!(decide(&notes, now), Decision::local(Reason::Constrained));
        let notes_ok = RouteContext {
            constrained_notes_allowed: true,
            ..notes
        };
        assert_eq!(decide(&notes_ok, now), Decision::cloud());
        // Cleanup on a constrained path is allowed by default.
        let polish_slow = RouteContext { probe: &slow, ..on };
        assert_eq!(decide(&polish_slow, now), Decision::cloud());
        {
            let mut b = breaker.lock().unwrap();
            b.record_failure(now);
            b.record_failure(now);
            b.record_failure(now);
        }
        assert_eq!(decide(&on, now), Decision::local(Reason::Breaker));
    }

    #[test]
    fn run_routed_falls_back_and_never_loses_the_take() {
        let kv = Kv::default();
        let breaker = Mutex::new(CircuitBreaker::new());
        let ctx = RouteContext {
            mode: CloudMode::CloudWhenOnline,
            has_key: true,
            probe: &AssumeOnline,
            breaker: &breaker,
            kv: &kv,
            today: day("2026-11-06"),
            job: Job::Polish,
            model: CLEANUP_MODEL,
            budget: budget_for(Job::Polish),
            constrained_notes_allowed: false,
        };
        // OK path.
        let mut calls = 0;
        let r = run_routed(
            &ctx,
            &mut |_, _| {
                calls += 1;
                Ok(CloudReply {
                    text: "Polished.".into(),
                    tokens_in: 10,
                    tokens_out: 5,
                    ttft_ms: 120,
                    total_ms: 400,
                })
            },
            &|t| Some(t.to_string()),
            &mut || "local".to_string(),
        );
        assert_eq!(r.value, "Polished.");
        assert_eq!((r.route, r.reason, r.fallback), (Route::Cloud, Reason::Online, Fallback::None));
        assert!(r.cost_usd > 0.0);
        assert_eq!(calls, 1);
        // Timeout → local, fallback=timeout, breaker counts it.
        let r = run_routed(
            &ctx,
            &mut |_, _| Err(CloudError::Ttft),
            &|t| Some(t.to_string()),
            &mut || "local".to_string(),
        );
        assert_eq!(r.value, "local");
        assert_eq!((r.route, r.reason, r.fallback), (Route::Local, Reason::Timeout, Fallback::Timeout));
        // 401 → no retry, key flagged, local.
        let mut calls = 0;
        let r = run_routed(
            &ctx,
            &mut |_, _| {
                calls += 1;
                Err(CloudError::Status {
                    code: 401,
                    retry_after: None,
                })
            },
            &|t| Some(t.to_string()),
            &mut || "local".to_string(),
        );
        assert_eq!(calls, 1);
        assert_eq!(r.value, "local");
        assert!(key_needs_attention(&kv));
        // 429 → exactly one retry, then local.
        let mut calls = 0;
        let r = run_routed(
            &ctx,
            &mut |_, _| {
                calls += 1;
                Err(CloudError::Status {
                    code: 429,
                    retry_after: Some(Duration::from_millis(1)),
                })
            },
            &|t| Some(t.to_string()),
            &mut || "local".to_string(),
        );
        assert_eq!(calls, 2);
        assert_eq!(r.fallback, Fallback::Error);
        // Three failures so far (ttft, 401, 429) → breaker open → no call.
        let mut calls = 0;
        let r = run_routed(
            &ctx,
            &mut |_, _| {
                calls += 1;
                Ok(CloudReply::default())
            },
            &|t| Some(t.to_string()),
            &mut || "local".to_string(),
        );
        assert_eq!(calls, 0);
        assert_eq!(r.reason, Reason::Breaker);
        // Validator rejection → local, fallback=error, but spend recorded.
        let breaker2 = Mutex::new(CircuitBreaker::new());
        let ctx2 = RouteContext {
            breaker: &breaker2,
            ..ctx
        };
        let r = run_routed(
            &ctx2,
            &mut |_, _| {
                Ok(CloudReply {
                    text: "garbage".into(),
                    tokens_in: 1,
                    tokens_out: 1,
                    ..Default::default()
                })
            },
            &|_| None::<String>,
            &mut || "local".to_string(),
        );
        assert_eq!(r.value, "local");
        assert_eq!(r.fallback, Fallback::Error);
        assert!(r.cost_usd > 0.0);
        // Always on this Mac: zero cloud calls.
        let ctx3 = RouteContext {
            mode: CloudMode::AlwaysLocal,
            ..ctx2
        };
        let mut calls = 0;
        let r = run_routed(
            &ctx3,
            &mut |_, _| {
                calls += 1;
                Ok(CloudReply::default())
            },
            &|t| Some(t.to_string()),
            &mut || "local".to_string(),
        );
        assert_eq!(calls, 0);
        assert_eq!(r.reason, Reason::User);
    }

    #[test]
    fn chips_and_tooltips_name_no_vendor() {
        for route in [Route::Cloud, Route::Local] {
            for reason in [
                Reason::Online,
                Reason::Offline,
                Reason::Breaker,
                Reason::Timeout,
                Reason::Cap,
                Reason::User,
                Reason::Error,
                Reason::Constrained,
                Reason::NoKey,
            ] {
                let chip = RouteChip::new(route, reason, Job::Polish);
                let text = format!("{} {}", chip.label, chip.tooltip).to_ascii_lowercase();
                for bad in ["anthropic", "claude", "openai", "gpt", "sonnet", "opus"] {
                    assert!(!text.contains(bad), "{text} names {bad}");
                }
            }
        }
        assert_eq!(Route::Cloud.chip(), "Cloud");
        assert_eq!(Route::Local.chip(), "On this Mac");
    }
}
