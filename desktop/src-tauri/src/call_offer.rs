//! yap-call-1 — the call offer: "Zoom call — record it?", a 10 s countdown,
//! then ONE one-click chip for the life of the call, Offer / Never per app,
//! and "Call ended — stop recording?". It never starts or stops on its own.
//!
//! Shape: a pure state machine ([`CallOfferMachine::step`]) fed by the call
//! signal (`meeting_detect::evaluate_calls`), the 1 s meeting tick
//! (`lib.rs` `meeting_status_sink`) and the user's answers. Its output is a
//! list of [`Effect`]s; `lib.rs` applies them. The ONLY input that yields
//! [`Effect::StartRecording`] is [`UserChoice::Record`] carrying the LIVE
//! candidate's offer id — `tests/call_offer_policy.rs` tables that.
//!
//! The offer renders in its own small window ([`CALL_OFFER_WINDOW`]) so it can
//! be hidden from screen capture / screen share without hiding the pill's
//! recording dot: [`content_protected_for`] is the one rule, and
//! [`apply_content_protection`] is the one caller of `set_content_protected`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::call_signals::{AppKind, CallCandidate, CallCatalog, Via};

/// `settings_kv` key: `{ bundle_id: "offer" | "never" }`. A missing bundle is
/// "offer". There is no third state and no auto-record state anywhere.
pub const CALL_OFFER_POLICY_KEY: &str = "call_offer_policy_v1";
/// The offer's countdown ring. Expiry == Not now; never Record.
pub const OFFER_COUNTDOWN_MS: u64 = 10_000;
/// While recording a detected call: the app's mic input off this long → ask.
pub const CALL_ENDED_QUIET_MS: u64 = 20_000;
/// The offer window's label — the ONLY content-protected window.
pub const CALL_OFFER_WINDOW: &str = "call-offer";
/// Emitted with a [`CallOfferView`] every time the surface changes.
pub const CALL_OFFER_EVENT: &str = "call_offer";

/// The one content-protection rule. True only for the offer window; the pill
/// (label `float`) and its recording dot are never hidden from a share.
pub fn content_protected_for(label: &str) -> bool {
    label == CALL_OFFER_WINDOW
}

/// The ONE caller of `set_content_protected`, used where each window is built.
pub fn apply_content_protection<R: tauri::Runtime>(w: &tauri::WebviewWindow<R>) {
    let protect = content_protected_for(w.label());
    if let Err(e) = w.set_content_protected(protect) {
        log::warn!("call_offer content_protected label={} err={e}", w.label());
    }
}

// ─── per-app policy ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppPolicy {
    Offer,
    Never,
}

/// bundle id (lower-cased) → policy.
pub type PolicyMap = BTreeMap<String, AppPolicy>;

/// Parse the stored JSON. Unknown values / bad JSON → empty (= offer for all).
pub fn parse_policy(raw: Option<&str>) -> PolicyMap {
    let Some(raw) = raw else {
        return PolicyMap::new();
    };
    let parsed: BTreeMap<String, String> = serde_json::from_str(raw).unwrap_or_default();
    parsed
        .into_iter()
        .filter_map(|(k, v)| {
            let p = match v.as_str() {
                "offer" => AppPolicy::Offer,
                "never" => AppPolicy::Never,
                _ => return None,
            };
            Some((k.to_ascii_lowercase(), p))
        })
        .collect()
}

pub fn policy_json(map: &PolicyMap) -> String {
    serde_json::to_string(map).unwrap_or_else(|_| "{}".to_string())
}

pub fn policy_for(map: &PolicyMap, bundle_id: &str) -> AppPolicy {
    map.get(&bundle_id.to_ascii_lowercase())
        .copied()
        .unwrap_or(AppPolicy::Offer)
}

pub fn load_policy(db: &crate::db::Database) -> PolicyMap {
    parse_policy(db.setting_get(CALL_OFFER_POLICY_KEY).as_deref())
}

/// Set one app's policy; `None` resets it (back to offer). Returns the map.
pub fn set_app_policy(
    db: &crate::db::Database,
    bundle_id: &str,
    policy: Option<AppPolicy>,
) -> Result<PolicyMap, String> {
    let mut map = load_policy(db);
    let key = bundle_id.to_ascii_lowercase();
    match policy {
        // "offer" is the default: store nothing for it, so Reset == absent.
        Some(AppPolicy::Never) => {
            map.insert(key, AppPolicy::Never);
        }
        Some(AppPolicy::Offer) | None => {
            map.remove(&key);
        }
    }
    db.setting_set(CALL_OFFER_POLICY_KEY, &policy_json(&map))?;
    Ok(map)
}

/// Settings › Meetings: the apps set to Never, with their display names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NeverRow {
    pub bundle_id: String,
    pub name: String,
}

pub fn never_rows(map: &PolicyMap, catalog: &CallCatalog) -> Vec<NeverRow> {
    map.iter()
        .filter(|(_, p)| **p == AppPolicy::Never)
        .map(|(b, _)| NeverRow {
            bundle_id: b.clone(),
            name: catalog
                .app(b)
                .map(|a| a.name.clone())
                .unwrap_or_else(|| b.clone()),
        })
        .collect()
}

// ─── the candidate the machine sees ─────────────────────────────────────────

/// How the call was found — stored on the meeting row (`detected_via`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectedVia {
    MicInUse,
    Title,
    Calendar,
    Manual,
}

impl DetectedVia {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MicInUse => "mic_in_use",
            Self::Title => "title",
            Self::Calendar => "calendar",
            Self::Manual => "manual",
        }
    }
    pub fn from_via(via: Via) -> Self {
        match via {
            Via::WebkitTitle => Self::Title,
            _ => Self::MicInUse,
        }
    }
}

/// One scored, offering call, as the machine needs it (no id yet: the machine
/// mints offer ids, so the webview can never invent one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sighting {
    /// `CallCandidate::offer_key` — app + pid.
    pub key: String,
    pub bundle_id: String,
    pub name: String,
    /// `call_signals::offer_copy`.
    pub copy: String,
    /// FaceTime / Phone: the far side is not measured.
    pub your_side_only: bool,
    /// "site unknown" for a browser call no title matched.
    pub site_line: Option<String>,
    pub via: DetectedVia,
}

impl Sighting {
    pub fn from_candidate(c: &CallCandidate, catalog: &CallCatalog) -> Self {
        let row = catalog.app(&c.bundle_id);
        let your_side_only = row.is_some_and(|a| a.kind == AppKind::SystemDaemon);
        let browser_untitled = row.is_some_and(|a| a.kind == AppKind::Browser && a.name == c.name);
        let copy = if browser_untitled {
            "Browser call — record it?".to_string()
        } else {
            crate::call_signals::offer_copy(c, catalog)
        };
        Self {
            key: c.offer_key(),
            bundle_id: c.bundle_id.clone(),
            name: if browser_untitled { "Browser".to_string() } else { c.name.clone() },
            copy,
            your_side_only,
            site_line: browser_untitled.then(|| "site unknown".to_string()),
            via: DetectedVia::from_via(c.via),
        }
    }
}

// ─── the machine ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserChoice {
    Record,
    NotNow,
    Never,
    Stop,
    KeepGoing,
}

impl UserChoice {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "record" => Some(Self::Record),
            "not_now" => Some(Self::NotNow),
            "never" => Some(Self::Never),
            "stop" => Some(Self::Stop),
            "keep_going" => Some(Self::KeepGoing),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// The call signal's best offering candidate (or none), at `now_ms`.
    Seen { sighting: Option<Sighting>, now_ms: u64 },
    /// The 1 s meeting tick: is the detected app's mic input running?
    Tick { now_ms: u64, app_input_running: Option<bool> },
    /// The user's answer for `offer_id`.
    User { choice: UserChoice, offer_id: String },
    /// A recording started by any other path, or any recording stopped.
    RecordingStopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Draw the offer with its countdown.
    ShowOffer,
    /// The countdown ran out / Not now: the one-click chip stays.
    ShowChip,
    /// The candidate is gone (or Never): offer and chip disappear.
    Withdraw,
    /// The ONE path to a recording. Only from [`UserChoice::Record`].
    StartRecording {
        offer_id: String,
        bundle_id: String,
        title: String,
        detected_via: DetectedVia,
    },
    /// The id was stale, invented, or nothing was offerable.
    Rejected { reason: String },
    /// Persist Never for this app.
    SaveNever { bundle_id: String },
    /// "Call ended — stop recording?"
    ShowEndPrompt,
    HideEndPrompt,
    /// Only from [`UserChoice::Stop`] on the end prompt.
    StopRecording,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Offering,
    Chip,
    Recording,
    EndPrompt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Live {
    pub offer_id: String,
    pub sighting: Sighting,
}

#[derive(Debug, Clone)]
pub struct CallOfferMachine {
    pub phase: Phase,
    pub live: Option<Live>,
    offered_at_ms: u64,
    quiet_since_ms: Option<u64>,
    /// Keep going was pressed: no second "Call ended" for this recording.
    end_prompt_answered: bool,
    /// Bumped each time a call (re)appears, so a second call by the same app
    /// session is a NEW offer id.
    generation: u64,
    /// The key the user already recorded / declined while it stays visible.
    spent_key: Option<String>,
}

impl Default for CallOfferMachine {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            live: None,
            offered_at_ms: 0,
            quiet_since_ms: None,
            end_prompt_answered: false,
            generation: 0,
            spent_key: None,
        }
    }
}

impl CallOfferMachine {
    pub fn live_offer_id(&self) -> Option<&str> {
        self.live.as_ref().map(|l| l.offer_id.as_str())
    }

    fn withdraw(&mut self) -> Vec<Effect> {
        let had = matches!(self.phase, Phase::Offering | Phase::Chip);
        self.phase = Phase::Idle;
        self.live = None;
        if had {
            vec![Effect::Withdraw]
        } else {
            vec![]
        }
    }

    /// THE transition function. Pure: same state + input + policy → same effects.
    pub fn step(&mut self, input: Input, policy: &PolicyMap) -> Vec<Effect> {
        match input {
            Input::Seen { sighting, now_ms } => self.seen(sighting, now_ms, policy),
            Input::Tick { now_ms, app_input_running } => self.tick(now_ms, app_input_running),
            Input::User { choice, offer_id } => self.user(choice, &offer_id),
            Input::RecordingStopped => {
                self.quiet_since_ms = None;
                self.end_prompt_answered = false;
                let was_prompt = self.phase == Phase::EndPrompt;
                if matches!(self.phase, Phase::Recording | Phase::EndPrompt) {
                    // The recorded call is spent: it is not offered again.
                    self.spent_key = self.live.as_ref().map(|l| l.sighting.key.clone());
                    self.phase = Phase::Idle;
                    self.live = None;
                }
                if was_prompt {
                    vec![Effect::HideEndPrompt]
                } else {
                    vec![]
                }
            }
        }
    }

    fn seen(&mut self, sighting: Option<Sighting>, now_ms: u64, policy: &PolicyMap) -> Vec<Effect> {
        if matches!(self.phase, Phase::Recording | Phase::EndPrompt) {
            // The tick owns a recording; detection does not touch it.
            return vec![];
        }
        let Some(s) = sighting else {
            // Mic input dropped: the next sighting is a new call (re-arms).
            self.spent_key = None;
            return self.withdraw();
        };
        if policy_for(policy, &s.bundle_id) == AppPolicy::Never {
            return self.withdraw();
        }
        if self.spent_key.as_deref() == Some(s.key.as_str()) {
            return vec![];
        }
        if let Some(l) = &self.live {
            if l.sighting.key == s.key {
                // Same call: keep its id, refresh the copy (a title may match late).
                self.live = Some(Live { offer_id: l.offer_id.clone(), sighting: s });
                return if self.phase == Phase::Offering
                    && now_ms.saturating_sub(self.offered_at_ms) >= OFFER_COUNTDOWN_MS
                {
                    self.phase = Phase::Chip;
                    vec![Effect::ShowChip]
                } else {
                    vec![]
                };
            }
        }
        self.generation += 1;
        self.live = Some(Live {
            offer_id: format!("{}#{}", s.key, self.generation),
            sighting: s,
        });
        self.phase = Phase::Offering;
        self.offered_at_ms = now_ms;
        vec![Effect::ShowOffer]
    }

    fn tick(&mut self, now_ms: u64, app_input_running: Option<bool>) -> Vec<Effect> {
        match self.phase {
            Phase::Offering if now_ms.saturating_sub(self.offered_at_ms) >= OFFER_COUNTDOWN_MS => {
                self.phase = Phase::Chip;
                vec![Effect::ShowChip]
            }
            Phase::Recording => match app_input_running {
                Some(false) => {
                    let since = *self.quiet_since_ms.get_or_insert(now_ms);
                    if !self.end_prompt_answered
                        && now_ms.saturating_sub(since) >= CALL_ENDED_QUIET_MS
                    {
                        self.phase = Phase::EndPrompt;
                        vec![Effect::ShowEndPrompt]
                    } else {
                        vec![]
                    }
                }
                // Running again, or unreadable (never evidence of an end).
                _ => {
                    self.quiet_since_ms = None;
                    vec![]
                }
            },
            _ => vec![],
        }
    }

    fn user(&mut self, choice: UserChoice, offer_id: &str) -> Vec<Effect> {
        let is_live = self.live_offer_id() == Some(offer_id);
        if !is_live {
            return vec![Effect::Rejected {
                reason: "that call offer is no longer live".into(),
            }];
        }
        match (choice, self.phase) {
            (UserChoice::Record, Phase::Offering | Phase::Chip) => {
                let l = self.live.as_ref().expect("is_live");
                self.phase = Phase::Recording;
                self.quiet_since_ms = None;
                self.end_prompt_answered = false;
                let title = if l.sighting.your_side_only {
                    format!("{} call (your side)", l.sighting.name)
                } else {
                    format!("{} call", l.sighting.name)
                };
                vec![Effect::StartRecording {
                    offer_id: l.offer_id.clone(),
                    bundle_id: l.sighting.bundle_id.clone(),
                    title,
                    detected_via: l.sighting.via,
                }]
            }
            (UserChoice::NotNow, Phase::Offering) => {
                self.phase = Phase::Chip;
                vec![Effect::ShowChip]
            }
            (UserChoice::Never, Phase::Offering | Phase::Chip) => {
                let bundle_id = self.live.as_ref().expect("is_live").sighting.bundle_id.clone();
                let mut fx = vec![Effect::SaveNever { bundle_id }];
                fx.extend(self.withdraw());
                fx
            }
            (UserChoice::Stop, Phase::EndPrompt) => vec![Effect::StopRecording],
            (UserChoice::KeepGoing, Phase::EndPrompt) => {
                self.phase = Phase::Recording;
                self.end_prompt_answered = true;
                self.quiet_since_ms = None;
                vec![Effect::HideEndPrompt]
            }
            _ => vec![Effect::Rejected {
                reason: format!("{choice:?} does not apply right now"),
            }],
        }
    }

    /// What the offer window draws.
    pub fn view(&self, now_ms: u64) -> CallOfferView {
        let live = self.live.as_ref();
        CallOfferView {
            phase: self.phase,
            offer_id: live.map(|l| l.offer_id.clone()),
            app_name: live.map(|l| l.sighting.name.clone()),
            bundle_id: live.map(|l| l.sighting.bundle_id.clone()),
            copy: live.map(|l| l.sighting.copy.clone()),
            site_line: live.and_then(|l| l.sighting.site_line.clone()),
            your_side_only: live.is_some_and(|l| l.sighting.your_side_only),
            countdown_ms_left: if self.phase == Phase::Offering {
                OFFER_COUNTDOWN_MS.saturating_sub(now_ms.saturating_sub(self.offered_at_ms))
            } else {
                0
            },
        }
    }
}

/// The wire shape of [`CALL_OFFER_EVENT`] / `call_offer_state`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallOfferView {
    pub phase: Phase,
    pub offer_id: Option<String>,
    pub app_name: Option<String>,
    pub bundle_id: Option<String>,
    pub copy: Option<String>,
    pub site_line: Option<String>,
    pub your_side_only: bool,
    pub countdown_ms_left: u64,
}

// ─── runtime glue ───────────────────────────────────────────────────────────

fn machine() -> &'static std::sync::Mutex<CallOfferMachine> {
    static M: std::sync::OnceLock<std::sync::Mutex<CallOfferMachine>> = std::sync::OnceLock::new();
    M.get_or_init(|| std::sync::Mutex::new(CallOfferMachine::default()))
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Step the live machine. Returns the effects and the view after the step.
pub fn drive(input: Input, policy: &PolicyMap) -> (Vec<Effect>, CallOfferView, Phase) {
    let now = now_ms();
    match machine().lock() {
        Ok(mut m) => {
            let fx = m.step(input, policy);
            let view = m.view(now);
            let phase = m.phase;
            (fx, view, phase)
        }
        Err(_) => (
            vec![Effect::Rejected { reason: "call offer state is unavailable".into() }],
            CallOfferMachine::default().view(now),
            Phase::Idle,
        ),
    }
}

pub fn current_view() -> CallOfferView {
    let now = now_ms();
    machine()
        .lock()
        .map(|m| m.view(now))
        .unwrap_or_else(|_| CallOfferMachine::default().view(now))
}

/// The detected app of the call being recorded, if a recording came from an offer.
pub fn recording_bundle() -> Option<String> {
    machine().lock().ok().and_then(|m| match m.phase {
        Phase::Recording | Phase::EndPrompt => m.live.as_ref().map(|l| l.sighting.bundle_id.clone()),
        _ => None,
    })
}
