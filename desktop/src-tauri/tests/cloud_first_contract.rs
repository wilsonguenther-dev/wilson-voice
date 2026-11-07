//! yap-1107-cloud-first-when-key-present — D2 (Wilson, binding): "Always
//! prioritise online when people have their API key in." With a key in and
//! the Mac online, cleanup, format and meeting notes go to the cloud BY
//! DEFAULT; this Mac runs them only offline, over the daily limit, when the
//! cloud keeps failing (breaker), or when the user explicitly picked "Always
//! on this Mac". No network: an in-memory settings store, an in-memory
//! keychain holding a fake key, and closures for the cloud.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use chrono::NaiveDate;
use wilson_voice_lib::cloud_route::{
    budget_for, cloud_available, decide, ensure_cloud_first_default, run_routed, CircuitBreaker,
    CloudError, CloudMode, CloudReply, Decision, FixedPath, Job, Ledger, PathStatus, Reason, Route,
    RouteContext, CLEANUP_MODEL, MODE_KEY, MODE_SOURCE_DEFAULT, MODE_SOURCE_EXPLICIT,
    MODE_SOURCE_KEY,
};
use wilson_voice_lib::keychain::MemoryKeychain;
use wilson_voice_lib::model_providers::{self, ProviderConfig, SettingsKv};
use wilson_voice_lib::resource_governor::{polish_plan, Level, PolishPlan};

const FAKE_KEY: &str = "test-key-not-real-0000WXYZ";

#[derive(Default)]
struct MemKv(RefCell<HashMap<String, String>>);
impl SettingsKv for MemKv {
    fn kv_get(&self, key: &str) -> Option<String> {
        self.0.borrow().get(key).cloned()
    }
    fn kv_set(&self, key: &str, value: &str) -> Result<(), String> {
        self.0.borrow_mut().insert(key.into(), value.into());
        Ok(())
    }
}

impl MemKv {
    fn snapshot(&self) -> Vec<(String, String)> {
        let mut rows: Vec<_> = self
            .0
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        rows.sort();
        rows
    }
}

fn today() -> NaiveDate {
    NaiveDate::parse_from_str("2026-11-07", "%Y-%m-%d").unwrap()
}

/// A settings store with the cloud-key provider saved and a fake key in the
/// keychain; `has_key` is what the app reads (`cloud_key_provider`).
fn keyed() -> (MemKv, MemoryKeychain, bool) {
    let kv = MemKv::default();
    let keys = MemoryKeychain::new("cloud-first-contract");
    model_providers::upsert_provider(&kv, &keys, ProviderConfig::cloud_key(), Some(FAKE_KEY))
        .expect("save the cloud-key provider");
    let has_key = model_providers::cloud_key_provider(&kv, &keys).is_some();
    (kv, keys, has_key)
}

fn ctx<'a>(
    kv: &'a MemKv,
    has_key: bool,
    probe: &'a FixedPath,
    breaker: &'a Mutex<CircuitBreaker>,
    job: Job,
) -> RouteContext<'a> {
    RouteContext {
        mode: CloudMode::load(kv),
        has_key,
        probe,
        breaker,
        kv,
        today: today(),
        job,
        model: CLEANUP_MODEL,
        budget: budget_for(job),
        constrained_notes_allowed: false,
    }
}

#[test]
fn a_key_in_and_online_sends_polish_format_and_notes_to_the_cloud() {
    let (kv, _keys, has_key) = keyed();
    assert!(has_key, "the fake key is read back like the real one");
    // No mode row: the user never picked one. The default with a key in is
    // "Cloud when online".
    assert_eq!(kv.kv_get(MODE_KEY), None);
    assert_eq!(
        ensure_cloud_first_default(&kv, has_key),
        Some(CloudMode::CloudWhenOnline)
    );
    let online = FixedPath(PathStatus::Satisfied);
    let breaker = Mutex::new(CircuitBreaker::new());
    let now = Instant::now();
    for job in [Job::Polish, Job::Format, Job::Notes] {
        let c = ctx(&kv, has_key, &online, &breaker, job);
        assert_eq!(decide(&c, now), Decision::cloud(), "{}", job.as_str());
    }
}

#[test]
fn offline_cap_breaker_and_always_local_fall_back_to_this_mac_with_the_reason() {
    let (kv, _keys, has_key) = keyed();
    ensure_cloud_first_default(&kv, has_key);
    let breaker = Mutex::new(CircuitBreaker::new());
    let now = Instant::now();

    // Offline -> Local(Offline).
    let offline = FixedPath(PathStatus::Unsatisfied);
    let c = ctx(&kv, has_key, &offline, &breaker, Job::Polish);
    assert_eq!(decide(&c, now), Decision::local(Reason::Offline));

    let online = FixedPath(PathStatus::Satisfied);

    // Breaker open (three failures in the window) -> Local(Breaker).
    {
        let open = Mutex::new(CircuitBreaker::new());
        {
            let mut b = open.lock().unwrap();
            b.record_failure(now);
            b.record_failure(now);
            b.record_failure(now);
        }
        let c = ctx(&kv, has_key, &online, &open, Job::Format);
        assert_eq!(decide(&c, now), Decision::local(Reason::Breaker));
    }

    // Explicit "Always on this Mac" -> Local(User), and the default never
    // overrides it.
    {
        let (kv2, _k2, has2) = keyed();
        CloudMode::save_explicit(&kv2, CloudMode::AlwaysLocal).unwrap();
        assert_eq!(ensure_cloud_first_default(&kv2, has2), None);
        let c = ctx(&kv2, has2, &online, &breaker, Job::Notes);
        assert_eq!(decide(&c, now), Decision::local(Reason::User));
    }

    // Daily limit reached -> Local(Cap). (Checked last: it spends the day.)
    let mut ledger = Ledger::load(&kv, today());
    ledger.record(today(), CLEANUP_MODEL, 500_000_000, 500_000_000);
    ledger.save(&kv).unwrap();
    let c = ctx(&kv, has_key, &online, &breaker, Job::Polish);
    assert_eq!(decide(&c, now), Decision::local(Reason::Cap));
}

#[test]
fn migration_sets_cloud_when_online_once_and_never_over_an_explicit_pick() {
    // Key present + no mode row -> cloud_when_online, recorded as the default.
    let (kv, _keys, has_key) = keyed();
    assert_eq!(
        ensure_cloud_first_default(&kv, has_key),
        Some(CloudMode::CloudWhenOnline)
    );
    assert_eq!(CloudMode::load(&kv), CloudMode::CloudWhenOnline);
    assert_eq!(
        kv.kv_get(MODE_SOURCE_KEY).as_deref(),
        Some(MODE_SOURCE_DEFAULT)
    );
    assert!(!CloudMode::is_explicit(&kv));
    // Running it twice changes nothing.
    let once = kv.snapshot();
    assert_eq!(ensure_cloud_first_default(&kv, has_key), None);
    assert_eq!(kv.snapshot(), once);

    // An explicit "Always on this Mac" row -> unchanged, twice.
    let (kv, _keys, has_key) = keyed();
    CloudMode::save_explicit(&kv, CloudMode::AlwaysLocal).unwrap();
    let before = kv.snapshot();
    assert_eq!(ensure_cloud_first_default(&kv, has_key), None);
    assert_eq!(ensure_cloud_first_default(&kv, has_key), None);
    assert_eq!(kv.snapshot(), before);
    assert_eq!(CloudMode::load(&kv), CloudMode::AlwaysLocal);

    // A row an older build stored when the user picked "Always on this Mac"
    // (no source row yet) counts as explicit: mode unchanged, pinned explicit
    // on the first run, nothing at all on the second.
    let (kv, _keys, has_key) = keyed();
    kv.kv_set(MODE_KEY, "always_local").unwrap();
    assert_eq!(ensure_cloud_first_default(&kv, has_key), None);
    assert_eq!(CloudMode::load(&kv), CloudMode::AlwaysLocal);
    assert_eq!(
        kv.kv_get(MODE_SOURCE_KEY).as_deref(),
        Some(MODE_SOURCE_EXPLICIT)
    );
    let pinned = kv.snapshot();
    assert_eq!(ensure_cloud_first_default(&kv, has_key), None);
    assert_eq!(kv.snapshot(), pinned);

    // No key -> nothing is written; the mode stays "Always on this Mac".
    let kv = MemKv::default();
    assert_eq!(ensure_cloud_first_default(&kv, false), None);
    assert!(kv.snapshot().is_empty());
    assert_eq!(CloudMode::load(&kv), CloudMode::AlwaysLocal);

    // A user who picks "Cloud only" keeps it.
    let (kv, _keys, has_key) = keyed();
    CloudMode::save_explicit(&kv, CloudMode::CloudOnly).unwrap();
    assert_eq!(ensure_cloud_first_default(&kv, has_key), None);
    assert_eq!(CloudMode::load(&kv), CloudMode::CloudOnly);
}

#[test]
fn full_level_with_a_usable_key_runs_dictation_polish_on_the_cloud() {
    let (kv, _keys, has_key) = keyed();
    ensure_cloud_first_default(&kv, has_key);
    let online = FixedPath(PathStatus::Satisfied);
    let breaker = Mutex::new(CircuitBreaker::new());
    let c = ctx(&kv, has_key, &online, &breaker, Job::Polish);
    let available = cloud_available(&c, Instant::now());
    assert!(
        available,
        "key in + online + under the cap + breaker closed"
    );
    // The governor at full no longer means "bundled only" when the cloud is usable.
    assert_eq!(polish_plan(Level::Full, available), PolishPlan::Cloud);
    assert_eq!(polish_plan(Level::Full, false), PolishPlan::Local);

    // The runner the dictation path uses (`cloud_polish` -> `cloud_run` ->
    // `run_routed`) asks the cloud once and never touches the bundled model.
    let cloud_calls = std::cell::Cell::new(0u32);
    let local_calls = std::cell::Cell::new(0u32);
    let mut cloud = |_b: wilson_voice_lib::cloud_route::Budget,
                     _attempt: u32|
     -> Result<CloudReply, CloudError> {
        cloud_calls.set(cloud_calls.get() + 1);
        Ok(CloudReply {
            text: "Hello, world.".into(),
            tokens_in: 20,
            tokens_out: 5,
            ttft_ms: 120,
            total_ms: 300,
        })
    };
    let accept = |t: &str| Some(t.to_string());
    let mut local = || {
        local_calls.set(local_calls.get() + 1);
        "bundled".to_string()
    };
    let routed = run_routed(&c, &mut cloud, &accept, &mut local);
    assert_eq!(routed.route, Route::Cloud);
    assert_eq!(routed.reason, Reason::Online);
    assert_eq!(routed.value, "Hello, world.");
    assert_eq!(cloud_calls.get(), 1);
    assert_eq!(local_calls.get(), 0);
}

/// The call-site audit, pinned: every dictation cleanup in lib.rs goes
/// through `cloud_polish` (cloud first, the bundled model as its fallback);
/// none calls the bundled polish directly.
#[test]
fn every_dictation_polish_in_lib_goes_through_the_cloud_route() {
    let lib = include_str!("../src/lib.rs");
    assert_eq!(
        lib.matches("polish::polish_llm(").count(),
        0,
        "a direct bundled-model polish bypasses the cloud route"
    );
    let direct = lib.matches("polish::polish_llm_result(").count();
    let wrapped = lib
        .match_indices("polish::polish_llm_result(")
        .filter(|(at, _)| {
            let mut from = at.saturating_sub(200);
            while !lib.is_char_boundary(from) {
                from += 1;
            }
            let window = &lib[from..*at];
            window.contains("cloud_polish(")
        })
        .count();
    assert!(direct >= 2, "live take + retry both polish");
    assert_eq!(
        wrapped, direct,
        "each bundled polish is cloud_polish's fallback"
    );
    // The live take's polishing stage asks the cloud route first.
    let stage = lib
        .find("take_stages.polishing(")
        .expect("the live take's polish stage");
    let after = &lib[stage..];
    assert!(
        after.find("cloud_polish(").unwrap() < after.find("polish::polish_llm_result(").unwrap()
    );
    // Meeting notes ask the cloud route before the bundled notes model.
    let notes = lib.find("fn summarize_meeting_blocking(").unwrap();
    let body = &lib[notes..];
    assert!(body.find("cloud_notes(").unwrap() < body.find("summarize_meeting_with(").unwrap());
}
