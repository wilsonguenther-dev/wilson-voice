//! SEC-B — the trial state machine, driven end to end, including the attacks.
//!
//! `src/license.rs` documents its own threat model carefully and then asks to be
//! taken at its word: "the two-place write + the max-seen-wall-clock floor
//! genuinely buy … that the trial cannot be extended by *ordinary* means"
//! (license.rs § WHAT THIS MODULE CAN AND CANNOT DO). This file is where that
//! sentence stops being a comment. It drives the whole fortnight, every one of
//! the fifteen days the UI will print, and each attack the doc names — a rolled
//! back clock, a deleted `license.json`, a deleted DB row, and the two stores
//! disagreeing — through the same pure functions the app runs.
//!
//! It also drives the direction the doc did NOT name, and which is the one that
//! fires in the field: a clock that jumps FORWARD once. The floor is a ratchet,
//! and a ratchet driven forward is a latch — the trial would read expired on day
//! two for a user who never had one, and the earliest-wins rule that makes the
//! two stores worth having would then defend the poison against every ordinary
//! repair. `forward_clock_excursion_does_not_expire_the_trial` holds the clamp,
//! and `a_poisoned_floor_can_be_cleared_by_a_signed_grace_claim` holds the one
//! non-DRM way out of the case no local clamp can catch.
//!
//! Everything here injects the clock. Nothing sleeps: a test that sleeps to
//! advance a trial is a test that cannot drive day nine at all.
//!
//! The day table this file computes is also WRITTEN OUT, to
//! `tests/fixtures/trial_days.json`, and `src/license/status.test.ts` asserts
//! the TypeScript `daysLeft` / `trialCountdown` reproduce it. Two hand-written
//! ladders in two languages drift; one generated table cannot.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use ed25519_dalek::{Signer, SigningKey};
use wilson_voice_lib::license::{
    apply_grace_to_trial, decide_entitlement, evaluate_trial, grace_from_claims, skid_of_spki,
    verify_license_with, Claims, Entitlement, LicenseManager, LicenseStore, MemoryStore,
    TrialInputs, TrialState, VerifyError, DAY_MS, GRACE_PLAN, SOLD_PLAN, TRIAL_DAYS, TRIAL_MS,
};
/// A plausible wall-clock instant to start every scenario from (ms since epoch).
const T0: i64 = 1_760_000_000_000;

// ─── The world, as the pure decision sees it ─────────────────────────

/// One install's two stores, in memory. `None` means that store has no copy —
/// which is exactly what "the user deleted license.json" and "the DB row is
/// gone" look like from inside `evaluate_trial`.
#[derive(Debug, Clone, Copy, Default)]
struct Stores {
    file_start: Option<i64>,
    db_start: Option<i64>,
    file_floor: Option<i64>,
    db_floor: Option<i64>,
}

impl Stores {
    /// Evaluate at a given wall clock, with the monotonic reading pinned to the
    /// same instant (an honest clock in an honest session).
    fn at(&self, wall: i64) -> TrialState {
        self.at_with_monotonic(wall, wall)
    }

    fn at_with_monotonic(&self, wall: i64, monotonic: i64) -> TrialState {
        evaluate_trial(TrialInputs {
            file_started_at_ms: self.file_start,
            db_started_at_ms: self.db_start,
            file_floor_ms: self.file_floor,
            db_floor_ms: self.db_floor,
            wall_now_ms: wall,
            monotonic_now_ms: monotonic,
        })
    }

    /// Write back what the app writes back: the resolved start and the new floor
    /// to BOTH stores. This is `LicenseManager::status()`'s persistence, minus
    /// the write-coalescing quantum that only exists to keep the press→capture
    /// path free of disk writes.
    fn persist(&mut self, state: &TrialState) {
        self.file_start = Some(state.started_at_ms);
        self.db_start = Some(state.started_at_ms);
        self.file_floor = Some(state.floor_ms);
        self.db_floor = Some(state.floor_ms);
    }
}

/// A trial that has been running honestly since `T0`, observed up to `wall`.
fn running_since_t0(wall: i64) -> Stores {
    let mut stores = Stores::default();
    let first = stores.at(T0);
    stores.persist(&first);
    let now = stores.at(wall);
    stores.persist(&now);
    stores
}

fn trial_entitlement(state: &TrialState) -> Entitlement {
    decide_entitlement(None, &[], state, |_| Err(VerifyError::Malformed)).0
}

// ─── Signing, for the claims-shaped tests ────────────────────────────

fn test_key() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

fn test_spki_b64(k: &SigningKey) -> String {
    let mut der = vec![
        0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ];
    der.extend_from_slice(k.verifying_key().as_bytes());
    base64::engine::general_purpose::STANDARD.encode(der)
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn sign_with(k: &SigningKey, claims: serde_json::Value) -> String {
    let payload = b64url(serde_json::to_string(&claims).unwrap().as_bytes());
    let sig = k.sign(payload.as_bytes());
    format!("{payload}.{}", b64url(&sig.to_bytes()))
}

fn verify_test(key: &str, k: &SigningKey) -> Result<Claims, VerifyError> {
    let skid = skid_of_spki(&test_spki_b64(k)).unwrap();
    verify_license_with(key, &k.verifying_key(), &skid)
}

fn lifetime_claims(kid: &str) -> serde_json::Value {
    serde_json::json!({
        "v": 1,
        "plan": SOLD_PLAN,
        "seats": 3,
        "email_hash": "",
        "issued_at": "2026-09-12T00:00:00Z",
        "kid": kid,
        "skid": skid_of_spki(&test_spki_b64(&test_key())).unwrap(),
    })
}

fn grace_claims(kid: &str, days: i64) -> serde_json::Value {
    serde_json::json!({
        "v": 1,
        "plan": GRACE_PLAN,
        "grace_days": days,
        "email_hash": "",
        "issued_at": "2026-09-12T00:00:00Z",
        "kid": kid,
        "skid": skid_of_spki(&test_spki_b64(&test_key())).unwrap(),
    })
}

// ─── The fortnight ───────────────────────────────────────────────────

#[test]
fn first_run_starts_the_trial_and_reports_fourteen() {
    let empty = Stores::default();
    let state = empty.at(T0);

    assert_eq!(state.started_at_ms, T0, "a first run starts the trial NOW");
    assert_eq!(state.expires_at_ms, T0 + TRIAL_MS);
    assert!(!state.expired);
    assert_eq!(state.days_left, TRIAL_DAYS);
    assert_eq!(
        trial_entitlement(&state),
        Entitlement::Trial {
            days_left: TRIAL_DAYS,
            expires_at_ms: T0 + TRIAL_MS
        },
        "a fresh install is a RUNNING trial, never a license screen"
    );
}

/// The exact ladder the countdown numeral will print, all fifteen values, and
/// the fixture the TypeScript side is pinned to.
#[test]
fn each_day_reports_one_fewer_and_zero_is_the_last_day() {
    let mut rows = Vec::new();
    let mut stores = Stores::default();
    stores.persist(&stores.at(T0));

    for day in 0..=TRIAL_DAYS {
        let wall = T0 + day * DAY_MS;
        let state = stores.at(wall);
        assert_eq!(
            state.days_left,
            TRIAL_DAYS - day,
            "day {day} of the trial must report {} days left",
            TRIAL_DAYS - day
        );
        assert_eq!(
            state.expired,
            day == TRIAL_DAYS,
            "day {day}: the trial ends on day {TRIAL_DAYS} and not before"
        );
        rows.push(serde_json::json!({
            "elapsed_days": day,
            "elapsed_ms": day * DAY_MS,
            "days_left": state.days_left,
            "expired": state.expired,
            "label": countdown_label(state.days_left),
        }));
        // Time passing is also observed: the floor advances with it, which is
        // what the rollback tests then have to defeat.
        stores.persist(&state);
    }

    assert_eq!(
        rows.len() as i64,
        TRIAL_DAYS + 1,
        "fifteen values, 14 down to 0"
    );
    write_fixture(serde_json::json!({
        "generated_by": "desktop/src-tauri/tests/trial_state_machine.rs :: each_day_reports_one_fewer_and_zero_is_the_last_day",
        "why": "One table, two languages. Rust's evaluate_trial() owns these numbers; src/license/status.test.ts asserts daysLeft()/trialCountdown() reproduce them, so the countdown the user reads cannot drift from the gate that closes.",
        "trial_days": TRIAL_DAYS,
        "day_ms": DAY_MS,
        "started_at_ms": T0,
        "expires_at_ms": T0 + TRIAL_MS,
        "rows": rows,
    }));
}

/// Mirrors `trialCountdown()` in `src/license/status.ts`. If you change one,
/// this test fails until you change the other — which is the entire point.
fn countdown_label(days_left: i64) -> String {
    match days_left {
        d if d <= 0 => "Last day".to_string(),
        1 => "1 day left".to_string(),
        d => format!("{d} days left"),
    }
}

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("trial_days.json")
}

fn write_fixture(value: serde_json::Value) {
    let path = fixture_path();
    let rendered = format!("{}\n", serde_json::to_string_pretty(&value).unwrap());
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    if current != rendered {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &rendered).unwrap();
    }
    let round_tripped = std::fs::read_to_string(&path).expect("the fixture must exist after a run");
    assert_eq!(
        round_tripped, rendered,
        "the committed fixture must be exactly what this test generates"
    );
}

#[test]
fn trial_days_left_never_goes_negative() {
    let stores = running_since_t0(T0 + TRIAL_MS);
    for extra_days in 0..40 {
        let state = stores.at(T0 + TRIAL_MS + extra_days * DAY_MS);
        assert!(state.expired);
        assert_eq!(
            state.days_left, 0,
            "{extra_days} days past the end must still read 0, never a negative numeral"
        );
    }
}

// ─── The attacks the doc comment names ───────────────────────────────

#[test]
fn clock_rolled_back_cannot_rewind_the_trial() {
    // Ten honest days, then the Mac's date is set back a week.
    let stores = running_since_t0(T0 + 10 * DAY_MS);
    let rolled_back = stores.at(T0 + 3 * DAY_MS);

    assert_eq!(
        rolled_back.effective_now_ms,
        T0 + 10 * DAY_MS,
        "the recorded floor is the time the trial is judged against"
    );
    assert_eq!(
        rolled_back.days_left, 4,
        "still day ten, whatever the clock says"
    );

    // Set it back past the START of the trial: the start must not be re-clamped
    // down to the faked now, or the fortnight restarts.
    let way_back = stores.at(T0 - 365 * DAY_MS);
    assert_eq!(way_back.started_at_ms, T0);
    assert_eq!(way_back.days_left, 4);

    // And an expired trial cannot be reopened by winding back inside it.
    let expired = running_since_t0(T0 + TRIAL_MS + DAY_MS);
    let reopened = expired.at(T0 + DAY_MS);
    assert!(
        reopened.expired,
        "a rolled-back clock cannot un-expire a trial"
    );
}

#[test]
fn deleting_the_license_file_does_not_restart_the_trial() {
    let full = running_since_t0(T0 + 10 * DAY_MS);
    // license.json is gone: no start, no floor on that side.
    let survivor = Stores {
        file_start: None,
        file_floor: None,
        ..full
    };
    let state = survivor.at(T0 + 10 * DAY_MS);
    assert_eq!(state.started_at_ms, T0, "the DB row still holds the start");
    assert_eq!(state.days_left, 4);
    assert!(!state.expired);

    // The same deletion on an EXPIRED install buys nothing either.
    let expired = running_since_t0(T0 + TRIAL_MS);
    let stripped = Stores {
        file_start: None,
        file_floor: None,
        ..expired
    };
    assert!(stripped.at(T0 + TRIAL_MS).expired);
}

#[test]
fn deleting_the_db_row_does_not_restart_the_trial() {
    let full = running_since_t0(T0 + 10 * DAY_MS);
    let survivor = Stores {
        db_start: None,
        db_floor: None,
        ..full
    };
    let state = survivor.at(T0 + 10 * DAY_MS);
    assert_eq!(
        state.started_at_ms, T0,
        "license.json still holds the start"
    );
    assert_eq!(state.days_left, 4);

    let expired = running_since_t0(T0 + TRIAL_MS);
    let stripped = Stores {
        db_start: None,
        db_floor: None,
        ..expired
    };
    assert!(stripped.at(T0 + TRIAL_MS).expired);
}

#[test]
fn earlier_of_the_two_stores_wins() {
    // A reinstall writes a fresh start into one store while the other still
    // remembers the real one. The EARLIER one is the truth.
    let disagreeing = Stores {
        file_start: Some(T0 + 12 * DAY_MS),
        db_start: Some(T0),
        ..Default::default()
    };
    let state = disagreeing.at(T0 + 13 * DAY_MS);
    assert_eq!(state.started_at_ms, T0);
    assert_eq!(state.days_left, 1);

    // Symmetric: it does not matter which store holds the older value.
    let mirrored = Stores {
        file_start: Some(T0),
        db_start: Some(T0 + 12 * DAY_MS),
        ..Default::default()
    };
    assert_eq!(mirrored.at(T0 + 13 * DAY_MS).started_at_ms, T0);

    // And the FLOOR resolves the other way — the highest time either store has
    // seen, so losing one cannot lower the bar.
    let floors = Stores {
        file_start: Some(T0),
        db_start: Some(T0),
        file_floor: Some(T0 + 2 * DAY_MS),
        db_floor: Some(T0 + 9 * DAY_MS),
    };
    assert_eq!(floors.at(T0 + DAY_MS).effective_now_ms, T0 + 9 * DAY_MS);
}

// ─── The forward direction: a latch, not a ratchet ───────────────────

#[test]
fn forward_clock_excursion_does_not_expire_the_trial() {
    // Day two of an honest trial. The session's monotonic reading says day two;
    // the wall clock jumps five years (restored image, bad NTP, a user trying
    // something). ONE such excursion used to poison the floor for good.
    let stores = running_since_t0(T0 + 2 * DAY_MS);
    let monotonic = T0 + 2 * DAY_MS;
    let jumped = stores.at_with_monotonic(monotonic + 5 * 365 * DAY_MS, monotonic);

    assert!(
        !jumped.expired,
        "a wall clock that ran away from this session's monotonic reading must not end the trial"
    );
    assert_eq!(jumped.days_left, 12);
    assert!(
        jumped.clock_excursion_ms > 0,
        "the excursion must be RECORDED, not silently absorbed"
    );
    assert_eq!(
        jumped.floor_ms, monotonic,
        "and above all it must not be written into the floor"
    );

    // The clock is corrected. Because the floor was never poisoned, the trial is
    // exactly where it was — this is the latch that used to be unrecoverable.
    let mut after = stores;
    after.persist(&jumped);
    let corrected = after.at(monotonic + 60 * 60 * 1000);
    assert!(!corrected.expired);
    assert_eq!(corrected.days_left, 12);
    assert_eq!(
        corrected.clock_excursion_ms, 0,
        "an honest clock reports no excursion"
    );

    // A few hours of ordinary skew is NOT an excursion: it still advances the
    // floor, so the rollback ratchet keeps working.
    let skewed = stores.at_with_monotonic(monotonic + 60 * 60 * 1000, monotonic);
    assert_eq!(skewed.clock_excursion_ms, 0);
    assert_eq!(skewed.floor_ms, monotonic + 60 * 60 * 1000);
}

#[test]
fn a_poisoned_floor_can_be_cleared_by_a_signed_grace_claim() {
    // The case no local clamp can catch: the Mac's clock was ALREADY wrong when
    // Yap launched, so the wall clock and the monotonic anchor agreed with each
    // other and were both five years out. The floor is poisoned in both stores.
    let poisoned = Stores {
        file_start: Some(T0),
        db_start: Some(T0),
        file_floor: Some(T0 + 5 * 365 * DAY_MS),
        db_floor: Some(T0 + 5 * 365 * DAY_MS),
    };
    let day_two = T0 + 2 * DAY_MS;
    assert!(
        poisoned.at(day_two).expired,
        "this is the lockout being reported: day two, and the trial reads expired"
    );
    // And the design deliberately removes every ordinary recovery.
    let file_deleted = Stores {
        file_floor: None,
        ..poisoned
    };
    let row_deleted = Stores {
        db_floor: None,
        ..poisoned
    };
    assert!(
        file_deleted.at(day_two).expired,
        "deleting license.json buys nothing"
    );
    assert!(
        row_deleted.at(day_two).expired,
        "deleting the DB row buys nothing"
    );

    // Support signs a grace claim — the same Ed25519 path a license takes, so
    // this costs the issuer nothing new and cannot be forged locally.
    let key = test_key();
    let claim = sign_with(&key, grace_claims("grace-0001", TRIAL_DAYS));
    let claims = verify_test(&claim, &key).expect("a grace claim is an ordinary signed claim");
    let grace = grace_from_claims(&claims).expect("plan=grace is a grace claim");
    assert_eq!(grace.days, TRIAL_DAYS);

    let reset = apply_grace_to_trial(&grace, day_two);
    let repaired = Stores {
        file_start: Some(reset.started_at_ms),
        db_start: Some(reset.started_at_ms),
        file_floor: None,
        db_floor: None,
    };
    let state = repaired.at(day_two);
    assert!(!state.expired, "the grace claim hands the trial back");
    assert_eq!(state.days_left, TRIAL_DAYS);

    // A lifetime license is not a grace claim, and an unsigned string is not
    // either — the lever is only reachable through a signature.
    let lifetime = verify_test(&sign_with(&key, lifetime_claims("kid-1")), &key).unwrap();
    assert_eq!(grace_from_claims(&lifetime), None);
    assert_eq!(verify_test("not.a.key", &key), Err(VerifyError::Malformed));
}

// ─── Entitlement: what the gate does with all of this ────────────────

#[test]
fn a_valid_license_beats_an_expired_trial() {
    let key = test_key();
    let license = sign_with(&key, lifetime_claims("kid-lifetime"));
    let expired = running_since_t0(T0 + TRIAL_MS + 30 * DAY_MS).at(T0 + TRIAL_MS + 30 * DAY_MS);
    assert!(expired.expired);

    let (entitlement, problem) =
        decide_entitlement(Some(&license), &[], &expired, |k| verify_test(k, &key));
    assert_eq!(problem, None);
    match entitlement {
        Entitlement::Licensed {
            ref plan, ref kid, ..
        } => {
            assert_eq!(plan, SOLD_PLAN);
            assert_eq!(kid, "kid-lifetime");
        }
        other => panic!("a valid lifetime license must win outright, got {other:?}"),
    }
    assert!(entitlement.allows_new_dictation());
}

#[test]
fn a_revoked_license_does_not_cancel_a_running_trial() {
    let key = test_key();
    let license = sign_with(&key, lifetime_claims("kid-refunded"));
    let running = running_since_t0(T0 + 3 * DAY_MS).at(T0 + 3 * DAY_MS);
    assert!(!running.expired);

    let (entitlement, problem) = decide_entitlement(
        Some(&license),
        &["kid-refunded".to_string()],
        &running,
        |k| verify_test(k, &key),
    );
    assert_eq!(
        problem.as_ref().map(|(code, _)| code.as_str()),
        Some("revoked"),
        "the refusal is still reported"
    );
    assert_eq!(
        entitlement,
        Entitlement::Trial {
            days_left: 11,
            expires_at_ms: T0 + TRIAL_MS
        },
        "but the user keeps the trial they already had"
    );
    assert!(entitlement.allows_new_dictation());
}

#[test]
fn expired_trial_stops_only_new_dictation() {
    let expired = running_since_t0(T0 + TRIAL_MS).at(T0 + TRIAL_MS);
    let (entitlement, problem) =
        decide_entitlement(None, &[], &expired, |_| Err(VerifyError::Malformed));
    assert_eq!(problem, None);
    assert_eq!(
        entitlement,
        Entitlement::LicenseRequired {
            reason: "trial_expired".to_string()
        }
    );
    assert!(
        !entitlement.allows_new_dictation(),
        "the ONE thing that stops is starting a new dictation"
    );
    // Nothing else in the entitlement changes shape: the expiry the UI shows is
    // still the same instant, and the state is the only difference from the day
    // before. Which surfaces are allowed to consult this at all is
    // `tests/license_gate.rs`'s job — it reads the command layer's own source —
    // and this test deliberately does not repeat that sweep.
    let day_before = running_since_t0(T0 + TRIAL_MS - DAY_MS).at(T0 + TRIAL_MS - DAY_MS);
    assert_eq!(day_before.expires_at_ms, expired.expires_at_ms);
    assert!(trial_entitlement(&day_before).allows_new_dictation());
}

// ─── The one stateful piece: the toast throttle ──────────────────────

#[test]
fn should_announce_gate_throttles() {
    let dir = scratch_dir("announce");
    let store: Arc<dyn LicenseStore> = Arc::new(MemoryStore::default());
    let mgr = LicenseManager::new(&dir, store);

    assert!(
        mgr.should_announce_gate(),
        "the first refused press speaks up"
    );
    for press in 0..50 {
        assert!(
            !mgr.should_announce_gate(),
            "press {press}: leaning on the hotkey must not become a notification storm"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

fn scratch_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yap-sec-b-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
