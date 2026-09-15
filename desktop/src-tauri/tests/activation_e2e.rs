//! LIC-A — the leg between a payment and a working dictation, walked end to end
//! in a clean data directory.
//!
//! Everything either side of this was already tested: the issuer's signing half
//! now has a vitest suite (`desktop/src/license/*.test.ts` over
//! `supabase/functions/_shared/*`), and `license.rs`'s unit tests prove the
//! verifier. What NOTHING owned was the join: a key that a real issuer would
//! emit, pasted into a real `LicenseManager`, over a real `license.json`, on a
//! Mac whose trial has run out — and the entitlement actually flipping, so the
//! next press of the hotkey records.
//!
//! The signing key here is a throwaway. The production private half is not in
//! this repository and never will be, which is exactly why `LicenseManager`
//! takes its verifier as an injected dependency.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ed25519_dalek::{Signer, SigningKey};
use wilson_voice_lib::license::{
    self, Claims, Entitlement, LicenseManager, LicenseStore, VerifyError,
};

/// A fixed wall clock: 2026-09-15T12:00:00Z, in ms.
const NOW_MS: i64 = 1_789_214_400_000;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

static SEQ: AtomicU64 = AtomicU64::new(0);

/// A clean `YAP_DATA_DIR` (Y0-D shape): nothing in it, nothing shared with
/// another test binary, and it takes its leftovers with `cargo clean`.
fn clean_data_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "yap-activation-e2e-{tag}-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp data dir");
    assert!(
        std::fs::read_dir(&dir).unwrap().next().is_none(),
        "the walk has to start from an EMPTY data dir or it proves nothing"
    );
    dir
}

/// The corroborating store, seeded so the trial is already over — the state a
/// buyer is actually in when they pay.
#[derive(Default)]
struct MapStore(Mutex<HashMap<String, String>>);

impl MapStore {
    fn with_trial_started(at_ms: i64) -> Arc<Self> {
        let s = Self::default();
        s.0.lock()
            .unwrap()
            .insert(license::DB_KEY_TRIAL_STARTED.to_string(), at_ms.to_string());
        Arc::new(s)
    }
}

impl LicenseStore for MapStore {
    fn get(&self, key: &str) -> Option<String> {
        self.0.lock().unwrap().get(key).cloned()
    }
    fn set(&self, key: &str, value: &str) {
        self.0
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
    }
}

// ── the throwaway issuer ──

fn issuer_key() -> SigningKey {
    SigningKey::from_bytes(&[42u8; 32])
}

fn spki_b64(k: &SigningKey) -> String {
    use base64::Engine as _;
    let mut der = vec![
        0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ];
    der.extend_from_slice(k.verifying_key().as_bytes());
    base64::engine::general_purpose::STANDARD.encode(der)
}

fn b64url(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Mint a key exactly the way `supabase/functions/_shared/claims.ts` does:
/// `base64url(claimsJson) "." base64url(sig)`, the signature over the ASCII
/// BYTES of the first segment.
fn sign(k: &SigningKey, claims: serde_json::Value) -> String {
    let segment = b64url(serde_json::to_string(&claims).unwrap().as_bytes());
    let sig = k.sign(segment.as_bytes());
    format!("{segment}.{}", b64url(&sig.to_bytes()))
}

/// The claims a real purchase produces. `plan` is the LITERAL string the issuer
/// signs, not `license::SOLD_PLAN` — a test that reads the same constant as the
/// code under test would still pass if that constant changed underneath both,
/// and the whole point of this file is that a key minted YESTERDAY still works.
fn purchased_claims(k: &SigningKey) -> serde_json::Value {
    serde_json::json!({
        "v": 1,
        "plan": "lifetime",
        "seats": 1,
        "email_hash": "b".repeat(64),
        "issued_at": "2026-09-15T12:00:00Z",
        "kid": "5a0b1c2d3e4f6071",
        "skid": license::skid_of_spki(&spki_b64(k)).unwrap(),
    })
}

fn manager(dir: &Path, store: Arc<MapStore>, k: SigningKey) -> LicenseManager {
    let spki = spki_b64(&k);
    let skid = license::skid_of_spki(&spki).unwrap();
    let verifying = license::parse_spki_ed25519(&spki).unwrap();
    LicenseManager::with_verify(
        dir,
        store,
        Box::new(|| NOW_MS),
        Box::new(move |key: &str| -> Result<Claims, VerifyError> {
            license::verify_license_with(key, &verifying, &skid)
        }),
    )
}

#[test]
fn a_signed_key_flips_the_entitlement_and_dictation_resumes() {
    let dir = clean_data_dir("activate");
    let store = MapStore::with_trial_started(NOW_MS - 20 * DAY_MS);
    let key_pair = issuer_key();
    let mgr = manager(&dir, store.clone(), key_pair.clone());

    // BEFORE: fortnight gone, nothing bought. New dictation is refused, and
    // that is the only thing that is.
    let before = mgr.status();
    assert!(
        !before.allows_new_dictation(),
        "a 20-day-old trial must be over, or this test is measuring nothing"
    );
    assert!(matches!(
        before.entitlement,
        Entitlement::LicenseRequired { .. }
    ));
    assert!(!before.has_stored_license);

    // The purchase: a key the issuer would have signed and emailed.
    let license_key = sign(&key_pair, purchased_claims(&key_pair));
    let after = mgr
        .activate(&license_key)
        .expect("a freshly issued lifetime key must activate");

    // AFTER: entitled, and the gate that stands in front of the microphone
    // (`LicenseStatus::allows_new_dictation`, the one `start_recording` asks)
    // now says yes — dictation resumes.
    match &after.entitlement {
        Entitlement::Licensed { plan, seats, kid } => {
            assert_eq!(plan, "lifetime");
            assert_eq!(*seats, 1);
            assert_eq!(kid, "5a0b1c2d3e4f6071");
        }
        other => panic!("a signed lifetime key must license the Mac, got {other:?}"),
    }
    assert!(after.allows_new_dictation());
    assert!(mgr.allows_new_dictation());
    assert!(after.license_problem.is_none());

    // And it SURVIVES a relaunch, because the key is on disk in the data dir
    // and re-verified from scratch — there is no "logged in" bit anywhere.
    assert!(dir.join(license::LICENSE_FILE).is_file());
    let relaunched = manager(&dir, store, key_pair);
    assert!(
        relaunched.allows_new_dictation(),
        "the license must survive a restart without touching the network"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_activation_has_copy_and_an_action() {
    let dir = clean_data_dir("refused");
    let store = MapStore::with_trial_started(NOW_MS - 20 * DAY_MS);
    let key_pair = issuer_key();
    let mgr = manager(&dir, store, key_pair.clone());

    // Three ways a real person fails: half a key out of a wrapped email, a key
    // from somewhere else entirely, and a genuine signature for another product.
    let other_issuer = SigningKey::from_bytes(&[9u8; 32]);
    let truncated = {
        let full = sign(&key_pair, purchased_claims(&key_pair));
        full[..full.len() - 8].to_string()
    };
    let wrong_key = sign(&other_issuer, purchased_claims(&other_issuer));
    let wrong_plan = {
        let mut c = purchased_claims(&key_pair);
        c["plan"] = serde_json::json!("some-other-product");
        sign(&key_pair, c)
    };

    for (label, bad) in [
        ("truncated", truncated),
        ("wrong issuer", wrong_key),
        ("wrong plan", wrong_plan),
        ("empty", String::new()),
    ] {
        let err = mgr
            .activate(&bad)
            .expect_err("a key that does not check out must never activate");

        // A STABLE CODE the UI can branch on...
        assert!(
            !err.code().is_empty()
                && err
                    .code()
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_'),
            "{label}: the code is what the frontend switches on"
        );
        // ...and a SENTENCE, with a next step. Not "invalid". Not a Rust enum
        // name. Not a transport error. This is the whole finding LIC-A opened
        // with: a failed activation with no copy is a refund.
        let message = err.message();
        assert!(
            message.len() > 40 && message.ends_with('.'),
            "{label}: a refusal must be a written sentence, got {message:?}"
        );
        assert!(
            !message.contains(err.code()) && !message.contains("Err"),
            "{label}: never show a person the machine code"
        );
        assert!(
            ["Paste", "Copy", "Update", "Use the key", "re-send"]
                .iter()
                .any(|action| message.contains(action)),
            "{label}: every refusal must name what to DO next, got {message:?}"
        );

        // And nothing was written. A bad paste cannot leave a Mac looking
        // licensed to a later, sloppier reader.
        let status = mgr.status();
        assert!(!status.has_stored_license, "{label}: nothing may be stored");
        assert!(!status.allows_new_dictation());
    }

    // The retrieval failures have copy too, and theirs has to do one more job:
    // say plainly that the offline path is unaffected.
    for (code, message) in [
        (
            license::RETRIEVAL_FAILED_CODE,
            license::RETRIEVAL_FAILED_MESSAGE,
        ),
        (
            license::RETRIEVAL_NOT_FOUND_CODE,
            license::RETRIEVAL_NOT_FOUND_MESSAGE,
        ),
    ] {
        assert!(!code.is_empty() && message.len() > 40 && message.ends_with('.'));
    }
    assert!(
        license::RETRIEVAL_FAILED_MESSAGE.contains("no internet"),
        "the network copy must tell the customer their emailed key still works"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn retrieval_failure_never_blocks_offline_verification() {
    let dir = clean_data_dir("offline");
    let store = MapStore::with_trial_started(NOW_MS - 20 * DAY_MS);
    let key_pair = issuer_key();
    let mgr = manager(&dir, store, key_pair.clone());
    mgr.activate(&sign(&key_pair, purchased_claims(&key_pair)))
        .expect("activation is offline — it never calls the issuer");

    let licensed = mgr.status();
    assert!(licensed.allows_new_dictation());

    // Every way the issuer can be unreachable, fed to the ONE decision point
    // that consumes a failed fetch. This is the production function, not a
    // stand-in: `spawn_revocation_refresh` hands it exactly this.
    for failure in [
        "error sending request for url",
        "dns error: failed to lookup address information",
        "http_500",
        "http_404",
        "invalid peer certificate",
        "expected value at line 1 column 1",
    ] {
        assert!(
            !mgr.apply_fetch_result(Err(failure.to_string())),
            "a failed fetch must apply nothing"
        );
        let now = mgr.status();
        assert_eq!(
            now.entitlement, licensed.entitlement,
            "'{failure}' changed the entitlement — a customer on a plane just lost Yap"
        );
        assert!(now.allows_new_dictation());
    }

    // The issuer is ONE host, reached for two things, and never on the
    // dictation path. If a build was shipped without an issuer provisioned,
    // that host cannot resolve at all — which is the honest failure, and still
    // costs a licensed user nothing.
    let revocation_url = license::revocation_url();
    let retrieval_url = license::license_retrieval_url();
    let revocation_host = host_of(&revocation_url);
    assert_eq!(revocation_host, host_of(&retrieval_url));
    assert_eq!(revocation_host, license::issuer_host());
    if !license::ISSUER_CONFIGURED {
        assert!(
            license::ISSUER_BASE_URL.ends_with("/yap-license"),
            "the unconfigured default must still be the issuer's shape"
        );
        assert!(
            revocation_host.ends_with(".invalid"),
            "an unprovisioned build must point at a host that can never resolve, \
             not at a plausible domain someone else could register"
        );
    }

    // Belt and braces on the property this whole test exists for: a manager
    // that has NEVER seen a revocation list, on a machine with no network,
    // still licenses from the signature alone.
    let fresh_dir = clean_data_dir("never-fetched");
    let fresh = manager(
        &fresh_dir,
        MapStore::with_trial_started(NOW_MS - 20 * DAY_MS),
        key_pair.clone(),
    );
    fresh
        .activate(&sign(&key_pair, purchased_claims(&key_pair)))
        .unwrap();
    assert!(fresh.status().revocation_checked_at_ms.is_none());
    assert!(fresh.allows_new_dictation());

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&fresh_dir);
}

fn host_of(url: &str) -> &str {
    url.trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap()
}
