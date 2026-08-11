//! YP2 — Yap's license core: offline Ed25519 verification, a corroborated
//! 14-day trial, and the one honest gate that stands in front of a NEW
//! dictation.
//!
//! WHAT YAP SELLS
//! --------------
//! A $29 one-time lifetime license (a $19 `FOUNDING19` promo runs against the
//! same SKU). There is no Yap account, no login, and no license-server call on
//! the hot path: a license is a signed blob the customer pastes in, and this
//! module verifies it against a public key compiled into the binary.
//!
//! THE WIRE FORMAT (must match the issuer exactly)
//! ----------------------------------------------
//! The issuer is `drivia-forge:server/src/yap-license.ts` (`signClaims`), live
//! on the Forge box. It emits:
//!
//! ```text
//! <base64url(claimsJson)> "." <base64url(ed25519 signature)>
//! ```
//!
//! and the signature covers **the ASCII bytes of the first segment**, not a
//! re-serialized claims object. That detail is the whole security story: this
//! module therefore verifies the exact bytes it received and only *then* parses
//! them. Re-serializing before verifying is the classic way a signed-claims
//! scheme silently becomes forgeable (JSON key order, unicode escaping and
//! whitespace all become attacker-chosen).
//!
//! Claims: `{ v, plan, seats, email_hash, issued_at, kid, skid }`
//!   * `kid`  — the per-LICENSE id, and the unit of revocation.
//!   * `skid` — the SIGNING-KEY id (sha256 of the SPKI DER, first 16 hex). It
//!     exists so a future key rotation is verifiable rather than guessed at.
//!     Revocation never keys off `skid`: revoking a signing key would revoke
//!     every customer at once.
//!
//! WHAT THIS MODULE CAN AND CANNOT DO (read this before trusting it)
//! ----------------------------------------------------------------
//! * The **signature** is a real cryptographic boundary. Nobody can mint a
//!   license without the private key that lives root-owned, mode 0400, on the
//!   Forge box. Tampering with a stored license — claims, signature or the file
//!   around them — fails verification on the very next read, because there is no
//!   cached "licensed" boolean anywhere: entitlement is *recomputed from the
//!   signature* every single time it is asked for.
//! * The **trial** is not, and cannot be, a cryptographic boundary. A local
//!   trial clock on a machine the user controls is deterrence, not security:
//!   the timestamp lives in a file and in a corroborating SQLite row, and a
//!   determined user with a hex editor wins. What the two-place write + the
//!   max-seen-wall-clock floor genuinely buy is that the trial cannot be
//!   extended by *ordinary* means — deleting one of the two stores, or setting
//!   the Mac's clock back. Saying more than that would be a lie, and this file
//!   is where somebody would look to find out.
//!
//! THE GATE IS DELIBERATELY NARROW
//! ------------------------------
//! When the trial has run out and there is no valid license, exactly one thing
//! stops: starting a NEW dictation. History, search, export, settings, the
//! dictionary, snippets, model management and every other surface keep working
//! forever. Words the user already spoke are theirs. `tests/license_gate.rs`
//! holds that line by reading this crate's own source.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use ed25519_dalek::{Signature, VerifyingKey};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ─── Pinned issuer identity ──────────────────────────────────────────
//
// The public half of the Ed25519 signing key that the deployed issuer holds at
// /etc/forge/yap/license-signing-ed25519.pem (root:root, 0400). Captured with
//
//   openssl pkey -in /etc/forge/yap/license-signing-ed25519.pem \
//     -pubout -outform DER | base64 -w0
//
// SPKI DER, base64 — the same encoding `loadSigningKey()` reports as
// `publicKeySpkiB64`, so the two can be compared byte for byte. This is a
// PUBLIC key: it is safe in the repo, safe in a shipped binary, and useless to
// anyone who wants to mint a license.
//
// Rotating it is a breaking change for every copy of Yap already installed —
// see docs/YAP-LICENSING.md. `skid` exists so a future build can pin two keys
// and select by id instead of trial-and-error.

/// Issuer Ed25519 public key, SPKI DER, base64.
pub const ISSUER_PUBLIC_KEY_SPKI_B64: &str =
    "MCowBQYDK2VwAyEAaLM6TsEhTwO810p7TZmKxPy4w++lqxB6iNLe/2KO2Ao=";

/// sha256(SPKI DER) truncated to 16 hex — the issuer's `skid`, derived from the
/// constant above and asserted against it in tests.
pub const ISSUER_SKID: &str = "fead1639a3f37d87";

/// The claims schema this build understands. Anything else is refused rather
/// than best-effort parsed: an unknown `v` means the issuer changed the meaning
/// of a field, and guessing at a licence's meaning is how a paying customer
/// gets locked out (or a non-paying one gets in).
pub const CLAIMS_VERSION: u32 = 1;

/// The only plan Yap sells. Entitlement requires it verbatim: a signed blob
/// that is not this plan verifies but grants nothing.
pub const SOLD_PLAN: &str = "lifetime";

/// Full-feature trial length. After this, only NEW dictation stops.
pub const TRIAL_DAYS: i64 = 14;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// Trial length in milliseconds.
pub const TRIAL_MS: i64 = TRIAL_DAYS * DAY_MS;

/// The ONLY host this module ever contacts, and only for the public revocation
/// list. No telemetry, no activation call, no license check-in — a Yap that
/// never sees the internet again works exactly the same.
pub const ISSUER_HOST: &str = "forge.87-99-149-214.sslip.io";
/// Public revocation list (`{version, updatedAt, kids[]}`, `max-age=300`).
pub const REVOCATION_URL: &str = "https://forge.87-99-149-214.sslip.io/v1/yap/revoked.json";

/// Where "Buy Yap" goes — the **one** place this URL is written down in the
/// app (YP3). The frontend never holds it: the Buy button invokes
/// `open_purchase_page`, which hands this constant to `open(1)` as a single
/// argv element, so no string the webview can influence ever reaches a process
/// launch.
///
/// Cited from the issuer's own record, `drivia-forge` `docs/YAP-LICENSING.md`
/// § "Live objects": Payment Link `plink_1U2yNFBc7RJSrX28KrzOuXwk`, price
/// `price_1U2yN4Bc7RJSrX287I33050v` ($29.00 one-time), product
/// `prod_V34VzbYj8scmQC`, `allow_promotion_codes: true` so `FOUNDING19` (coupon
/// `z8UZkIH2`, −$10, 500 redemptions, expires 2026-11-08) applies at checkout.
///
/// The link is currently **`active: false`** on Stripe — deliberately
/// deactivated until the delivery path is proven end to end (see that doc's
/// "Why the link is deactivated"). Yap ships pointing at it anyway: this is the
/// URL, and re-activating it in Stripe is what turns purchasing on, with no
/// client change and no release. Until then the page says the link is not yet
/// open, which is the honest state of the world and not a bug in Yap.
pub const PAYMENT_LINK_URL: &str = "https://buy.stripe.com/4gM00i88q3Fs5Dzbag1B602";

/// Filename under `data_dir()`.
pub const LICENSE_FILE: &str = "license.json";

/// DB keys for the corroborating rows (see `LicenseStore`).
pub const DB_KEY_TRIAL_STARTED: &str = "license.trial_started_at_ms";
pub const DB_KEY_CLOCK_FLOOR: &str = "license.max_seen_wall_ms";

// ─── Base64url ───────────────────────────────────────────────────────

/// Decode a base64url segment. The issuer emits UNPADDED base64url; we accept
/// padding and the standard `+/` alphabet too, because a license key travels
/// through email clients and a paste buffer before it gets here, and being
/// strict about *cosmetics* only ever punishes the customer. Being strict about
/// the SIGNATURE is what matters, and that is not negotiable below.
fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    let normalized: String = s
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '=')
        .map(|c| match c {
            '+' => '-',
            '/' => '_',
            other => other,
        })
        .collect();
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(normalized.as_bytes())
        .ok()
}

fn b64_standard_decode(s: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(s.trim().as_bytes())
        .ok()
}

// ─── Claims + verification ───────────────────────────────────────────

/// The claims the issuer signs. Unknown future fields are ignored on purpose
/// (serde default), but an unknown `v` is not — see `CLAIMS_VERSION`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    pub v: u32,
    pub plan: String,
    #[serde(default)]
    pub seats: u32,
    #[serde(default)]
    pub email_hash: String,
    #[serde(default)]
    pub issued_at: String,
    pub kid: String,
    /// Signing-key id. Absent in a hypothetical older issuance; when present it
    /// must name the key we pin, or the blob was signed by something else.
    #[serde(default)]
    pub skid: String,
}

/// Why a license blob was refused. Every variant is a *stable string* the UI can
/// branch on, and none of them leaks a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyError {
    /// Not `<segment>.<segment>`.
    Malformed,
    /// Signature segment is not 64 decodable bytes, or does not verify.
    BadSignature,
    /// Signature verified but the payload is not the JSON we expect.
    MalformedClaims,
    /// Signature verified, claims parsed, `v` is from another era.
    UnsupportedVersion,
    /// Signature verified but `skid` names a key that is not the one we pin.
    WrongSigningKey,
    /// Genuinely signed by Yap, but not the plan this app sells. Verification
    /// and ENTITLEMENT are two different questions, and this variant is the
    /// line between them.
    WrongPlan,
}

impl VerifyError {
    pub fn code(self) -> &'static str {
        match self {
            VerifyError::Malformed => "malformed",
            VerifyError::BadSignature => "bad_signature",
            VerifyError::MalformedClaims => "malformed_claims",
            VerifyError::UnsupportedVersion => "unsupported_version",
            VerifyError::WrongSigningKey => "wrong_signing_key",
            VerifyError::WrongPlan => "wrong_plan",
        }
    }

    /// What a human is told. Never "invalid" with no next step.
    pub fn message(self) -> &'static str {
        match self {
            VerifyError::Malformed | VerifyError::MalformedClaims => {
                "That does not look like a complete Yap license key. Paste the whole key from your email — it is one long line."
            }
            VerifyError::BadSignature => {
                "That license key did not check out. Copy it again straight from your email; if it still fails, reply to that email and we will re-send it."
            }
            VerifyError::UnsupportedVersion => {
                "That license key was issued for a newer version of Yap. Update Yap and try again."
            }
            VerifyError::WrongSigningKey => {
                "That license key was not issued by Yap. Use the key from your purchase email."
            }
            VerifyError::WrongPlan => {
                "That key is signed by Yap but is not a license for this app."
            }
        }
    }
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

/// Parse the pinned SPKI DER into an Ed25519 verifying key.
///
/// An Ed25519 SPKI is a fixed 44 bytes: a 12-byte prefix naming the algorithm
/// (`302a300506032b6570032100`) followed by the raw 32-byte key. We check the
/// prefix rather than pulling in an ASN.1 parser for a structure that has
/// exactly one legal shape.
pub fn parse_spki_ed25519(spki_b64: &str) -> Option<VerifyingKey> {
    const ED25519_SPKI_PREFIX: [u8; 12] = [
        0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ];
    let der = b64_standard_decode(spki_b64)?;
    if der.len() != 44 || der[..12] != ED25519_SPKI_PREFIX {
        return None;
    }
    let mut raw = [0u8; 32];
    raw.copy_from_slice(&der[12..]);
    VerifyingKey::from_bytes(&raw).ok()
}

/// Derive an `skid` the way the issuer does: sha256 of the SPKI DER, first 16
/// hex characters.
pub fn skid_of_spki(spki_b64: &str) -> Option<String> {
    let der = b64_standard_decode(spki_b64)?;
    let digest = Sha256::digest(&der);
    Some(hex_lower(&digest)[..16].to_string())
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// The pinned issuer key. A malformed constant is a build bug, not a runtime
/// condition — `license_pinned_public_key_parses` fails the build's test gate
/// long before a user could meet it, and returning `None` here would silently
/// turn every license invalid in the field.
pub fn issuer_public_key() -> &'static VerifyingKey {
    use std::sync::OnceLock;
    static KEY: OnceLock<VerifyingKey> = OnceLock::new();
    KEY.get_or_init(|| {
        parse_spki_ed25519(ISSUER_PUBLIC_KEY_SPKI_B64)
            .expect("the pinned issuer public key must be a valid Ed25519 SPKI")
    })
}

/// Verify a license key against the pinned issuer key. This is the only
/// entry point production code should use.
pub fn verify_license(license_key: &str) -> Result<Claims, VerifyError> {
    verify_license_with(license_key, issuer_public_key(), ISSUER_SKID)
}

/// Verify against an arbitrary key — the same code path, parameterised so tests
/// can sign with a throwaway key instead of needing the production private half
/// (which lives on the box and is never leaving it).
pub fn verify_license_with(
    license_key: &str,
    verifying_key: &VerifyingKey,
    expected_skid: &str,
) -> Result<Claims, VerifyError> {
    let trimmed = license_key.trim();
    let mut parts = trimmed.split('.');
    let (Some(payload), Some(sig_part), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(VerifyError::Malformed);
    };
    if payload.is_empty() || sig_part.is_empty() {
        return Err(VerifyError::Malformed);
    }

    let sig_bytes = b64url_decode(sig_part).ok_or(VerifyError::BadSignature)?;
    // Ed25519 signatures are exactly 64 bytes. Reject any other length up front
    // so a length oracle is not reachable through the verifier.
    let sig_array: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| VerifyError::BadSignature)?;
    let signature = Signature::from_bytes(&sig_array);

    // THE bytes: the payload segment exactly as received. Nothing is
    // re-encoded, re-ordered or normalised before this line.
    if verifying_key
        .verify_strict(payload.as_bytes(), &signature)
        .is_err()
    {
        return Err(VerifyError::BadSignature);
    }

    let claims_json = b64url_decode(payload).ok_or(VerifyError::MalformedClaims)?;
    let claims: Claims =
        serde_json::from_slice(&claims_json).map_err(|_| VerifyError::MalformedClaims)?;
    if claims.v != CLAIMS_VERSION {
        return Err(VerifyError::UnsupportedVersion);
    }
    if claims.kid.trim().is_empty() {
        return Err(VerifyError::MalformedClaims);
    }
    if !claims.skid.is_empty() && claims.skid != expected_skid {
        return Err(VerifyError::WrongSigningKey);
    }
    Ok(claims)
}

// ─── Corroborating store ─────────────────────────────────────────────

/// The second place the trial's timestamps live. Backed by SQLite in the app
/// (`Database`), by a map in tests. Two independent stores is what makes
/// "delete the license file to restart the trial" stop working.
pub trait LicenseStore: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str);
}

impl LicenseStore for crate::db::Database {
    fn get(&self, key: &str) -> Option<String> {
        self.license_state_get(key)
    }
    fn set(&self, key: &str, value: &str) {
        if let Err(e) = self.license_state_set(key, value) {
            log::warn!("license: could not write corroborating row {key}: {e}");
        }
    }
}

/// In-memory store (tests, and the last-resort in-memory DB path).
#[derive(Default)]
pub struct MemoryStore {
    map: Mutex<std::collections::HashMap<String, String>>,
    /// How many writes this store has taken. The gate runs on the
    /// press→capture path, so "does the steady state write?" is a property
    /// worth being able to assert on.
    writes: std::sync::atomic::AtomicUsize,
}

impl MemoryStore {
    pub fn writes(&self) -> usize {
        self.writes.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl LicenseStore for MemoryStore {
    fn get(&self, key: &str) -> Option<String> {
        self.map.lock().get(key).cloned()
    }
    fn set(&self, key: &str, value: &str) {
        self.writes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.map.lock().insert(key.to_string(), value.to_string());
    }
}

// ─── On-disk state ───────────────────────────────────────────────────

fn file_version_default() -> u32 {
    1
}

/// What `license.json` holds.
///
/// Note what is NOT here: any boolean saying the app is licensed, any cached
/// "valid until", any decoded copy of the claims. The signed key is the single
/// source of truth and it is re-verified on every read, so flipping a byte in
/// this file cannot buy anything — the worst it does is un-license a machine
/// whose owner then re-pastes their key.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LicenseFile {
    #[serde(default = "file_version_default")]
    pub v: u32,
    /// The signed license key, verbatim as the customer pasted it.
    #[serde(default)]
    pub license_key: Option<String>,
    #[serde(default)]
    pub activated_at_ms: Option<i64>,
    #[serde(default)]
    pub trial_started_at_ms: Option<i64>,
    /// Highest wall-clock value ever observed. A clock set backwards cannot go
    /// below this, so the trial cannot be replayed by rolling the date back.
    #[serde(default)]
    pub max_seen_wall_ms: Option<i64>,
    /// Cached revocation list. Best-effort and possibly stale by design.
    #[serde(default)]
    pub revoked_kids: Vec<String>,
    #[serde(default)]
    pub revoked_checked_at_ms: Option<i64>,
}

// ─── Trial arithmetic (pure) ─────────────────────────────────────────

/// Everything the trial decision depends on, gathered in one struct so the
/// decision itself is a pure function and can be tested without a filesystem,
/// a database or a clock.
#[derive(Debug, Clone, Copy)]
pub struct TrialInputs {
    pub file_started_at_ms: Option<i64>,
    pub db_started_at_ms: Option<i64>,
    pub file_floor_ms: Option<i64>,
    pub db_floor_ms: Option<i64>,
    /// What the OS says the time is right now.
    pub wall_now_ms: i64,
    /// Wall clock at process start + monotonic elapsed since. Immune to a clock
    /// change made *while Yap is running*.
    pub monotonic_now_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrialState {
    pub started_at_ms: i64,
    pub expires_at_ms: i64,
    /// The time the trial is judged against: never lower than any time this
    /// install has already seen.
    pub effective_now_ms: i64,
    pub expired: bool,
    pub days_left: i64,
    /// The value that should be written back to both stores as the new floor.
    pub floor_ms: i64,
}

/// Decide the trial from its inputs.
///
/// Three rules, in order:
///  1. **Earliest start wins.** If the file and the DB disagree about when the
///     trial began, the earlier one is the truth. Deleting one store therefore
///     buys nothing — the survivor is authoritative — and re-installing over a
///     live DB cannot silently hand out a second trial.
///  2. **Time never goes backwards.** `effective_now` is the max of the wall
///     clock, the monotonic reading, and the highest value ever recorded. So
///     setting the Mac's clock back cannot rewind the trial.
///  3. **A start in the future is clamped to now.** Otherwise setting the clock
///     forward once would push the expiry out by that much for good.
pub fn evaluate_trial(inputs: TrialInputs) -> TrialState {
    let recorded_floor = match (inputs.file_floor_ms, inputs.db_floor_ms) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };
    let effective_now = inputs
        .wall_now_ms
        .max(inputs.monotonic_now_ms)
        .max(recorded_floor.unwrap_or(i64::MIN));

    let stored_start = match (inputs.file_started_at_ms, inputs.db_started_at_ms) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };
    // First run: the trial starts now. Any recorded start is clamped so a clock
    // that ran fast once cannot buy extra days.
    let started_at = stored_start.unwrap_or(effective_now).min(effective_now);
    let expires_at = started_at.saturating_add(TRIAL_MS);
    let remaining = expires_at - effective_now;
    TrialState {
        started_at_ms: started_at,
        expires_at_ms: expires_at,
        effective_now_ms: effective_now,
        expired: remaining <= 0,
        // Round UP, so "1 day left" never displays while there are still hours.
        days_left: if remaining <= 0 {
            0
        } else {
            (remaining + DAY_MS - 1) / DAY_MS
        },
        floor_ms: effective_now,
    }
}

// ─── Entitlement ─────────────────────────────────────────────────────

/// What the app is allowed to do right now. Serialised to the frontend as
/// `{ "state": "trial", ... }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Entitlement {
    /// A verified, unrevoked, lifetime license is installed.
    Licensed {
        plan: String,
        seats: u32,
        kid: String,
    },
    /// No license, trial still running.
    Trial { days_left: i64, expires_at_ms: i64 },
    /// Trial over and no usable license. NEW dictation stops here; nothing else
    /// does.
    LicenseRequired { reason: String },
}

/// The typed code the Rust command layer returns, and the string the frontend
/// branches on. One constant, one spelling, everywhere.
pub const LICENSE_REQUIRED_CODE: &str = "license_required";

impl Entitlement {
    /// The gate. `false` stops a NEW dictation and NOTHING else.
    pub fn allows_new_dictation(&self) -> bool {
        matches!(
            self,
            Entitlement::Licensed { .. } | Entitlement::Trial { .. }
        )
    }
}

/// A refused stored license: a stable code plus the sentence a human is shown.
pub type LicenseProblem = (String, String);

/// Turn (stored key, revocation list, trial) into an entitlement — the whole
/// decision, as a pure function, with the verifier injected so tests can drive
/// every branch with a throwaway signing key instead of needing the production
/// private half (which lives root-only on the Forge box and is staying there).
///
/// Order matters and is deliberate:
///   * a VALID license wins outright, whatever the trial says — that is what
///     "lifetime" means;
///   * a revoked or unverifiable license does NOT cancel a running trial; the
///     user simply keeps the trial they already had;
///   * only when there is no usable license AND the trial is over does the gate
///     close.
pub fn decide_entitlement<F>(
    stored_key: Option<&str>,
    revoked_kids: &[String],
    trial: &TrialState,
    verify: F,
) -> (Entitlement, Option<LicenseProblem>)
where
    F: Fn(&str) -> Result<Claims, VerifyError>,
{
    let revoked: BTreeSet<&str> = revoked_kids.iter().map(String::as_str).collect();
    let mut problem: Option<LicenseProblem> = None;
    let mut licensed: Option<Claims> = None;

    if let Some(key) = stored_key.map(str::trim).filter(|s| !s.is_empty()) {
        match verify(key) {
            Ok(claims) if revoked.contains(claims.kid.as_str()) => {
                problem = Some((
                    "revoked".into(),
                    "This license was refunded or charged back, so it no longer activates Yap. Reply to your purchase email if that is a mistake.".into(),
                ));
            }
            Ok(claims) if claims.plan != SOLD_PLAN => {
                problem = Some((
                    "wrong_plan".into(),
                    "That key is signed by Yap but is not a license for this app.".into(),
                ));
            }
            Ok(claims) => licensed = Some(claims),
            Err(e) => problem = Some((e.code().to_string(), e.message().to_string())),
        }
    }

    let entitlement = match (licensed, trial.expired) {
        (Some(c), _) => Entitlement::Licensed {
            plan: c.plan,
            seats: c.seats,
            kid: c.kid,
        },
        (None, false) => Entitlement::Trial {
            days_left: trial.days_left,
            expires_at_ms: trial.expires_at_ms,
        },
        (None, true) => Entitlement::LicenseRequired {
            reason: problem
                .as_ref()
                .map(|(code, _)| code.clone())
                .unwrap_or_else(|| "trial_expired".to_string()),
        },
    };
    (entitlement, problem)
}

/// The full picture: the entitlement plus the context a settings screen needs
/// (why a stored license was refused, how much trial is left even while
/// licensed, when the revocation list was last refreshed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LicenseStatus {
    #[serde(flatten)]
    pub entitlement: Entitlement,
    /// Present when a license IS stored but did not grant anything: `revoked`,
    /// `bad_signature`, `wrong_plan`, …
    pub license_problem: Option<String>,
    pub license_problem_message: Option<String>,
    pub has_stored_license: bool,
    pub trial_days_left: i64,
    pub trial_expires_at_ms: i64,
    pub revocation_checked_at_ms: Option<i64>,
    pub revoked_count: usize,
}

impl LicenseStatus {
    pub fn allows_new_dictation(&self) -> bool {
        self.entitlement.allows_new_dictation()
    }
}

/// The user-facing sentence when the gate closes. Gentle on purpose: this is a
/// person who used Yap for a fortnight and pressed their hotkey out of habit.
pub const LICENSE_REQUIRED_MESSAGE: &str =
    "Your 14-day Yap trial has ended. Your history, exports and settings all still work — a $29 one-time license turns dictation back on.";

// ─── Manager ─────────────────────────────────────────────────────────

type NowFn = Box<dyn Fn() -> i64 + Send + Sync>;
type VerifyFn = Box<dyn Fn(&str) -> Result<Claims, VerifyError> + Send + Sync>;

/// Owns `license.json`, the corroborating rows, and the cached revocation list.
///
/// Every public read re-derives entitlement from the stored signature. There is
/// no "logged in" state to go stale and nothing to invalidate.
pub struct LicenseManager {
    path: PathBuf,
    store: Arc<dyn LicenseStore>,
    file: Mutex<LicenseFile>,
    /// Wall clock at construction + a monotonic anchor; see `TrialInputs`.
    anchor_wall_ms: i64,
    anchor: Instant,
    now: NowFn,
    /// Always `verify_license` in the app. Injected only so tests can exercise
    /// the storage / revocation / gate paths with a throwaway signing key —
    /// the production private key is root-only on the Forge box.
    verify: VerifyFn,
    /// Throttle for the hotkey toast, so leaning on the key is not a
    /// notification storm.
    last_notice: Mutex<Option<Instant>>,
}

/// How often the gate is allowed to speak up on the hotkey path.
const GATE_NOTICE_INTERVAL_SECS: u64 = 15;

/// How coarse the rollback floor is allowed to be.
///
/// The floor exists to stop the Mac's clock being wound back to replay the
/// trial, and an hour of slack costs an attacker one hour out of fourteen days.
/// What it buys is that `status()` — which the gate calls on EVERY dictation
/// start, on the press→capture path YV35 measures — does no disk write in its
/// steady state. Making this 0 would put a SQLite write and a file rewrite in
/// front of the user's first syllable, every single time they talk.
const FLOOR_QUANTUM_MS: i64 = 60 * 60 * 1000;

fn wall_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        // A clock before 1970 is not a real condition; 0 keeps the floor logic
        // honest (it can only ever raise the effective time).
        .unwrap_or(0)
}

fn parse_ms(raw: Option<String>) -> Option<i64> {
    raw.and_then(|s| s.trim().parse::<i64>().ok())
}

impl LicenseManager {
    /// Open (or create) the license state under `dir`, with `store` as the
    /// corroborating second home for the trial timestamps.
    pub fn new(dir: &Path, store: Arc<dyn LicenseStore>) -> Self {
        Self::with_clock(dir, store, Box::new(wall_now_ms))
    }

    pub fn with_clock(dir: &Path, store: Arc<dyn LicenseStore>, now: NowFn) -> Self {
        Self::build(dir, store, now, Box::new(verify_license))
    }

    fn build(dir: &Path, store: Arc<dyn LicenseStore>, now: NowFn, verify: VerifyFn) -> Self {
        let path = dir.join(LICENSE_FILE);
        let file = read_license_file(&path);
        let anchor_wall_ms = now();
        let mgr = Self {
            path,
            store,
            file: Mutex::new(file),
            anchor_wall_ms,
            anchor: Instant::now(),
            now,
            verify,
            last_notice: Mutex::new(None),
        };
        // Starting the trial is a side effect of the first read, so a fresh
        // install is a running trial the moment the app is up — never a
        // "license required" screen before the user has said a word.
        let _ = mgr.status();
        mgr
    }

    fn monotonic_now_ms(&self) -> i64 {
        self.anchor_wall_ms
            .saturating_add(self.anchor.elapsed().as_millis() as i64)
    }

    /// Recompute everything from disk state. Cheap (one Ed25519 verify) and
    /// deliberately not cached.
    pub fn status(&self) -> LicenseStatus {
        let mut file = self.file.lock();

        let db_started_at_ms = parse_ms(self.store.get(DB_KEY_TRIAL_STARTED));
        let db_floor_ms = parse_ms(self.store.get(DB_KEY_CLOCK_FLOOR));
        let trial = evaluate_trial(TrialInputs {
            file_started_at_ms: file.trial_started_at_ms,
            db_started_at_ms,
            file_floor_ms: file.max_seen_wall_ms,
            db_floor_ms,
            wall_now_ms: (self.now)(),
            monotonic_now_ms: self.monotonic_now_ms(),
        });

        // Persist the trial's start and the new clock floor to BOTH stores —
        // but only when the value has genuinely moved.
        //
        // This matters because `status()` sits on the press→capture path: the
        // gate asks it on every dictation start, and YV35 measures that span in
        // milliseconds. The clock floor advances on literally every call, so
        // writing it every time would put a SQLite write and a file rewrite in
        // front of the user's first syllable, forever. `FLOOR_QUANTUM_MS`
        // (below) is the coarseness the floor is allowed to have; it is what
        // makes the steady state of this function pure reads.
        //
        // Each store is compared against the resolved value SEPARATELY, so a
        // store that lost its copy — a deleted license.json, a quarantined and
        // recreated database — is repaired from the survivor rather than left
        // half-populated. That repair is the whole reason there are two of them.
        let mut dirty = false;
        if file.trial_started_at_ms != Some(trial.started_at_ms) {
            file.trial_started_at_ms = Some(trial.started_at_ms);
            dirty = true;
        }
        if db_started_at_ms != Some(trial.started_at_ms) {
            self.store
                .set(DB_KEY_TRIAL_STARTED, &trial.started_at_ms.to_string());
        }
        if file
            .max_seen_wall_ms
            .unwrap_or(i64::MIN)
            .saturating_add(FLOOR_QUANTUM_MS)
            <= trial.floor_ms
        {
            file.max_seen_wall_ms = Some(trial.floor_ms);
            dirty = true;
        }
        if db_floor_ms
            .unwrap_or(i64::MIN)
            .saturating_add(FLOOR_QUANTUM_MS)
            <= trial.floor_ms
        {
            self.store
                .set(DB_KEY_CLOCK_FLOOR, &trial.floor_ms.to_string());
        }

        // Re-verify the stored key from scratch, every time. There is no cached
        // answer to go stale, and no boolean anywhere that says "licensed".
        let stored = file.license_key.clone();
        let has_stored_license = stored
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty());
        let (entitlement, problem) =
            decide_entitlement(stored.as_deref(), &file.revoked_kids, &trial, |k| {
                (self.verify)(k)
            });

        if dirty {
            write_license_file(&self.path, &file);
        }
        let revoked_count = file.revoked_kids.len();
        let revocation_checked_at_ms = file.revoked_checked_at_ms;
        drop(file);

        LicenseStatus {
            entitlement,
            license_problem: problem.as_ref().map(|(c, _)| c.clone()),
            license_problem_message: problem.as_ref().map(|(_, m)| m.clone()),
            has_stored_license,
            trial_days_left: trial.days_left,
            trial_expires_at_ms: trial.expires_at_ms,
            revocation_checked_at_ms,
            revoked_count,
        }
    }

    /// The gate itself.
    pub fn allows_new_dictation(&self) -> bool {
        self.status().allows_new_dictation()
    }

    /// True at most once per `GATE_NOTICE_INTERVAL_SECS` — the hotkey path asks
    /// this before showing a toast so a held key cannot spam Notification
    /// Center.
    pub fn should_announce_gate(&self) -> bool {
        let mut last = self.last_notice.lock();
        let now = Instant::now();
        let quiet = last
            .map(|t| now.duration_since(t).as_secs() < GATE_NOTICE_INTERVAL_SECS)
            .unwrap_or(false);
        if quiet {
            return false;
        }
        *last = Some(now);
        true
    }

    /// Store a license key — but only after it verifies. An unverifiable key is
    /// never written to disk, so a bad paste cannot leave the app in a state
    /// where it looks licensed to a later, sloppier reader.
    pub fn activate(&self, license_key: &str) -> Result<LicenseStatus, VerifyError> {
        let claims = (self.verify)(license_key)?;
        if claims.plan != SOLD_PLAN {
            return Err(VerifyError::WrongPlan);
        }
        {
            let mut file = self.file.lock();
            file.license_key = Some(license_key.trim().to_string());
            file.activated_at_ms = Some((self.now)());
            write_license_file(&self.path, &file);
        }
        Ok(self.status())
    }

    /// Remove the stored license (moving a seat to another Mac). The trial
    /// bookkeeping is untouched — this is not a way to get another fortnight.
    pub fn deactivate(&self) {
        let mut file = self.file.lock();
        file.license_key = None;
        file.activated_at_ms = None;
        write_license_file(&self.path, &file);
    }

    /// Fold a freshly fetched revocation list into the cache.
    pub fn apply_revocations(&self, kids: Vec<String>, checked_at_ms: i64) {
        let mut file = self.file.lock();
        file.revoked_kids = kids;
        file.revoked_checked_at_ms = Some(checked_at_ms);
        write_license_file(&self.path, &file);
    }

    /// THE decision point for the one network call this module makes: a fetch
    /// that failed for ANY reason — offline, DNS, TLS, 500, garbage JSON —
    /// leaves every byte of local state exactly as it was. Nothing is cleared,
    /// nothing expires, no "could not verify" state exists to brick anyone.
    /// Returns true when a list was actually applied.
    pub fn apply_fetch_result(&self, result: Result<Vec<String>, String>) -> bool {
        match result {
            Ok(kids) => {
                let now = (self.now)();
                self.apply_revocations(kids, now);
                true
            }
            Err(e) => {
                log::info!("license: revocation refresh skipped ({e}) — keeping the cached list");
                false
            }
        }
    }

    pub fn revoked_kids(&self) -> Vec<String> {
        self.file.lock().revoked_kids.clone()
    }

    pub fn now_ms(&self) -> i64 {
        (self.now)()
    }

    #[cfg(test)]
    fn file_snapshot(&self) -> LicenseFile {
        self.file.lock().clone()
    }
}

// ─── File I/O ────────────────────────────────────────────────────────

fn read_license_file(path: &Path) -> LicenseFile {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return LicenseFile {
            v: 1,
            ..Default::default()
        };
    };
    match serde_json::from_str::<LicenseFile>(&raw) {
        // A file from a future schema is not guessed at: it is ignored for
        // reading (so nothing is granted on a shape we do not understand) and
        // left ALONE on disk, because overwriting it would destroy a newer
        // Yap's state after a downgrade.
        Ok(f) if f.v == 1 => f,
        Ok(f) => {
            log::warn!(
                "license: license.json has unknown version {} — ignoring",
                f.v
            );
            LicenseFile {
                v: 1,
                ..Default::default()
            }
        }
        Err(e) => {
            // Corrupt/hand-edited file. Not fatal, and NOT a reason to block the
            // app: the trial's DB row survives this, and a real license can be
            // re-pasted in one action.
            log::warn!("license: license.json unreadable ({e}) — starting from empty state");
            LicenseFile {
                v: 1,
                ..Default::default()
            }
        }
    }
}

fn write_license_file(path: &Path, file: &LicenseFile) {
    let Ok(json) = serde_json::to_string_pretty(file) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Atomic-ish: write beside, then rename, so a crash mid-write cannot leave a
    // half-file that reads as "no license".
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_err() {
        return;
    }
    if std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

// ─── Revocation fetch (the only network call in this module) ─────────

#[derive(Debug, Deserialize)]
struct RevocationDocument {
    #[serde(default)]
    kids: Vec<String>,
}

/// Best-effort fetch of the public revocation list.
///
/// Every failure mode — offline, DNS down, 500, malformed JSON, TLS error —
/// returns `Err` and the caller does NOTHING with it beyond a log line. A Yap
/// that cannot reach the issuer keeps exactly the entitlement it had. That is a
/// deliberate trade: a refunded customer keeping the app while offline is a far
/// cheaper failure than a paying customer on a plane losing theirs.
pub async fn fetch_revocations(url: &str) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("http_{}", resp.status().as_u16()));
    }
    let body = resp.text().await.map_err(|e| e.to_string())?;
    let doc: RevocationDocument = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    Ok(doc.kids)
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    /// A license key signed by the REAL, deployed issuer key.
    ///
    /// This is the only proof that `ISSUER_PUBLIC_KEY_SPKI_B64` is the public
    /// half of the key on the box — a constant that "looks right" and a constant
    /// that verifies production signatures are different claims, and only this
    /// fixture can tell them apart.
    ///
    /// It is SAFE to ship: its `plan` is not the plan Yap sells, and
    /// `license_production_signed_fixture_grants_nothing` below is the standing
    /// guard that it can never become a free lifetime license. (An Ed25519
    /// signature over a chosen message reveals nothing about the private key.)
    const PRODUCTION_SIGNED_FIXTURE: &str = concat!(
        "eyJ2IjoxLCJwbGFuIjoieXAyLXB1YmtleS1wcm9vZi1ub3QtYS1saWNlbnNlIiwic2VhdHMiOjAsImVtYWlsX2hhc2gi",
        "OiIwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwIiwi",
        "aXNzdWVkX2F0IjoiMjAyNi0wOC0xMVQwMDowMDowMC4wMDBaIiwia2lkIjoieXAyZml4dHVyZTAwMDAwMDAwMDAwMCIs",
        "InNraWQiOiJmZWFkMTYzOWEzZjM3ZDg3In0",
        ".",
        "z9B7Ib52W20HqAW1DXbw-rg3e39mhEL-EsXgUlndpq98S4ieRqPsrP5oH3BgTXoFJXKafUHQw1lLpGMFzDCdDw",
    );

    // ── test signer (stands in for the box's private key) ──

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

    /// Mint a license the way `signClaims` in the issuer does.
    fn sign_with(k: &SigningKey, claims: serde_json::Value) -> String {
        let payload = b64url(serde_json::to_string(&claims).unwrap().as_bytes());
        let sig = k.sign(payload.as_bytes());
        format!("{payload}.{}", b64url(&sig.to_bytes()))
    }

    fn valid_claims() -> serde_json::Value {
        serde_json::json!({
            "v": 1,
            "plan": "lifetime",
            "seats": 3,
            "email_hash": "a".repeat(64),
            "issued_at": "2026-08-11T00:00:00.000Z",
            "kid": "0123456789abcdef01234567",
            "skid": skid_of_spki(&test_spki_b64(&test_key())).unwrap(),
        })
    }

    fn verify_test(key: &str, signer: &SigningKey) -> Result<Claims, VerifyError> {
        let skid = skid_of_spki(&test_spki_b64(signer)).unwrap();
        verify_license_with(key, &signer.verifying_key(), &skid)
    }

    // ── (1) the key format ──

    #[test]
    fn license_pinned_public_key_parses_and_matches_its_skid() {
        let key = parse_spki_ed25519(ISSUER_PUBLIC_KEY_SPKI_B64)
            .expect("pinned SPKI must parse as Ed25519");
        assert_eq!(key.as_bytes().len(), 32);
        assert_eq!(
            skid_of_spki(ISSUER_PUBLIC_KEY_SPKI_B64).as_deref(),
            Some(ISSUER_SKID),
            "ISSUER_SKID must be sha256(SPKI DER)[..16] of the pinned key"
        );
    }

    /// YP3 — the Buy button's destination, pinned.
    ///
    /// This is the one string in the app that decides where a customer's money
    /// goes, and a typo in it is silent: the button opens, a Stripe page loads
    /// (or 404s), and nothing in the build complains. So the value the issuer
    /// documented is asserted verbatim, and its shape is asserted separately so
    /// a future edit cannot quietly turn it into a non-Stripe or non-TLS URL.
    #[test]
    fn payment_link_is_the_live_link_the_issuer_documented() {
        assert_eq!(
            PAYMENT_LINK_URL, "https://buy.stripe.com/4gM00i88q3Fs5Dzbag1B602",
            "the Buy button must point at plink_1U2yNFBc7RJSrX28KrzOuXwk — see \
             drivia-forge docs/YAP-LICENSING.md § Live objects"
        );
        assert!(
            PAYMENT_LINK_URL.starts_with("https://buy.stripe.com/"),
            "checkout must be Stripe's own hosted page, over TLS"
        );
        assert!(
            !PAYMENT_LINK_URL.contains(char::is_whitespace),
            "a URL handed to open(1) must be a single argv element"
        );
    }

    /// The end-to-end proof: a blob signed by the private key on the Forge box
    /// verifies against the constant compiled into this binary.
    #[test]
    fn license_verifies_a_signature_from_the_deployed_issuer() {
        let claims = verify_license(PRODUCTION_SIGNED_FIXTURE)
            .expect("the deployed issuer's signature must verify against the pinned key");
        assert_eq!(claims.skid, ISSUER_SKID);
        assert_eq!(claims.v, CLAIMS_VERSION);
    }

    /// …and that proof must never turn into a free license.
    #[test]
    fn license_production_signed_fixture_grants_nothing() {
        let claims = verify_license(PRODUCTION_SIGNED_FIXTURE).unwrap();
        assert_ne!(
            claims.plan, SOLD_PLAN,
            "the pubkey-proof fixture must not be a sellable plan"
        );

        let (dir, store) = temp_env("fixture-grants-nothing");
        let mgr = LicenseManager::new(dir.path(), store);
        assert!(
            mgr.activate(PRODUCTION_SIGNED_FIXTURE).is_err(),
            "activation must refuse a non-lifetime plan"
        );
        assert!(matches!(
            mgr.status().entitlement,
            Entitlement::Trial { .. }
        ));
    }

    #[test]
    fn license_valid_key_verifies() {
        let k = test_key();
        let key = sign_with(&k, valid_claims());
        let claims = verify_test(&key, &k).expect("a well-formed signed key verifies");
        assert_eq!(claims.plan, "lifetime");
        assert_eq!(claims.seats, 3);
        assert_eq!(claims.kid, "0123456789abcdef01234567");
    }

    #[test]
    fn license_tampered_claims_rejected() {
        let k = test_key();
        let key = sign_with(&k, valid_claims());
        // Re-encode the claims with seats bumped, keeping the original signature.
        let mut claims = valid_claims();
        claims["seats"] = serde_json::json!(999);
        let forged_payload = b64url(serde_json::to_string(&claims).unwrap().as_bytes());
        let sig = key.split('.').nth(1).unwrap();
        assert_eq!(
            verify_test(&format!("{forged_payload}.{sig}"), &k),
            Err(VerifyError::BadSignature)
        );
    }

    #[test]
    fn license_tampered_signature_rejected() {
        let k = test_key();
        let key = sign_with(&k, valid_claims());
        let (payload, sig) = key.split_once('.').unwrap();
        let mut raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(sig)
            .unwrap();
        raw[0] ^= 0x01;
        assert_eq!(
            verify_test(&format!("{payload}.{}", b64url(&raw)), &k),
            Err(VerifyError::BadSignature)
        );
        // Truncated signature — a length oracle must not be reachable.
        assert_eq!(
            verify_test(&format!("{payload}.{}", b64url(&raw[..32])), &k),
            Err(VerifyError::BadSignature)
        );
    }

    #[test]
    fn license_signed_by_another_key_rejected() {
        let attacker = SigningKey::from_bytes(&[9u8; 32]);
        let key = sign_with(&attacker, valid_claims());
        assert_eq!(
            verify_test(&key, &test_key()),
            Err(VerifyError::BadSignature)
        );
        // …including against the real pinned key.
        assert_eq!(verify_license(&key), Err(VerifyError::BadSignature));
    }

    #[test]
    fn license_unknown_version_rejected() {
        let k = test_key();
        let mut claims = valid_claims();
        claims["v"] = serde_json::json!(2);
        let key = sign_with(&k, claims);
        assert_eq!(verify_test(&key, &k), Err(VerifyError::UnsupportedVersion));

        let mut claims0 = valid_claims();
        claims0["v"] = serde_json::json!(0);
        assert_eq!(
            verify_test(&sign_with(&k, claims0), &k),
            Err(VerifyError::UnsupportedVersion)
        );
    }

    #[test]
    fn license_wrong_signing_key_id_rejected() {
        let k = test_key();
        let mut claims = valid_claims();
        claims["skid"] = serde_json::json!("deadbeefdeadbeef");
        let key = sign_with(&k, claims);
        assert_eq!(verify_test(&key, &k), Err(VerifyError::WrongSigningKey));
    }

    #[test]
    fn license_malformed_input_rejected() {
        let k = test_key();
        for junk in ["", "  ", "nodot", "a.b.c", ".sig", "payload.", "🙂.🙂"] {
            assert!(verify_test(junk, &k).is_err(), "{junk:?} must not verify");
        }
        // Valid signature over a payload that is not JSON.
        let payload = b64url(b"not json at all");
        let sig = k.sign(payload.as_bytes());
        assert_eq!(
            verify_test(&format!("{payload}.{}", b64url(&sig.to_bytes())), &k),
            Err(VerifyError::MalformedClaims)
        );
    }

    // ── (2) storage ──

    fn temp_env(tag: &str) -> (tempdir::TempDir, Arc<dyn LicenseStore>) {
        (
            tempdir::TempDir::new(tag),
            Arc::new(MemoryStore::default()) as Arc<dyn LicenseStore>,
        )
    }

    /// A manager that trusts the TEST signing key instead of the pinned one, so
    /// the storage / revocation / gate paths can be driven end to end. Every
    /// other line of the manager is the production line.
    fn test_manager(dir: &Path, store: Arc<dyn LicenseStore>, now: i64) -> LicenseManager {
        let signer = test_key();
        let skid = skid_of_spki(&test_spki_b64(&signer)).unwrap();
        let vk = signer.verifying_key();
        LicenseManager::build(
            dir,
            store,
            Box::new(move || now),
            Box::new(move |k| verify_license_with(k, &vk, &skid)),
        )
    }

    /// A real license signed by the pinned issuer does not exist outside the
    /// box, so storage tests drive the manager through a file written by hand
    /// and assert on what the manager REFUSES.
    #[test]
    fn license_storage_is_reverified_on_every_read() {
        let (dir, store) = temp_env("storage-reverify");
        let mgr = LicenseManager::new(dir.path(), store.clone());
        assert!(matches!(
            mgr.status().entitlement,
            Entitlement::Trial { .. }
        ));

        // Drop a plausible-looking but unsigned key straight into the file, the
        // way a user following a "crack" would.
        let forged = sign_with(&test_key(), valid_claims());
        std::fs::write(
            dir.path().join(LICENSE_FILE),
            serde_json::to_string(&LicenseFile {
                v: 1,
                license_key: Some(forged),
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();

        let reopened = LicenseManager::new(dir.path(), store);
        let status = reopened.status();
        assert_eq!(status.license_problem.as_deref(), Some("bad_signature"));
        assert!(
            !matches!(status.entitlement, Entitlement::Licensed { .. }),
            "a forged stored key must never be licensed"
        );
    }

    #[test]
    fn license_file_has_no_plaintext_licensed_flag() {
        let (dir, store) = temp_env("no-plaintext-flag");
        let mgr = LicenseManager::new(dir.path(), store);
        let _ = mgr.status();
        let raw = std::fs::read_to_string(dir.path().join(LICENSE_FILE))
            .expect("the manager persists its state");
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let keys: Vec<&String> = parsed.as_object().unwrap().keys().collect();
        for k in &keys {
            let k = k.as_str();
            assert!(
                !(k.contains("licensed") || k.contains("is_valid") || k.contains("activated_ok")),
                "license.json must not carry a decided flag, found `{k}` in {keys:?}"
            );
        }
        // And the type itself has no boolean the reader could trust.
        let file: LicenseFile = serde_json::from_str(&raw).unwrap();
        assert!(file.license_key.is_none());
    }

    #[test]
    fn license_corrupt_file_does_not_block_the_app() {
        let (dir, store) = temp_env("corrupt-file");
        std::fs::write(dir.path().join(LICENSE_FILE), b"{ not json").unwrap();
        let mgr = LicenseManager::new(dir.path(), store);
        assert!(
            mgr.allows_new_dictation(),
            "a corrupt license file must fall back to the trial, never to a lockout"
        );
    }

    // ── (3) revocation ──

    #[test]
    fn license_revoked_kid_invalidates() {
        let (dir, store) = temp_env("revoked");
        let key = sign_with(&test_key(), valid_claims());
        let mgr = test_manager(dir.path(), store, 1_000_000);

        let activated = mgr
            .activate(&key)
            .expect("a well-signed lifetime key activates");
        assert!(matches!(
            activated.entitlement,
            Entitlement::Licensed { .. }
        ));

        // Refund lands: the customer's kid appears on the public list.
        mgr.apply_revocations(vec!["0123456789abcdef01234567".into()], 1_000_100);
        let status = mgr.status();
        assert_eq!(status.license_problem.as_deref(), Some("revoked"));
        assert!(
            status
                .license_problem_message
                .as_deref()
                .is_some_and(|m| m.contains("refunded")),
            "a revoked license must say WHY, not just fail"
        );
        // Still inside the trial, so the app is not bricked mid-sentence — the
        // gate closes when the trial does.
        assert!(status.allows_new_dictation());

        let expired = LicenseManager::build(
            dir.path(),
            Arc::new(MemoryStore::default()),
            Box::new(|| 1_000_000 + TRIAL_MS + DAY_MS),
            Box::new({
                let vk = test_key().verifying_key();
                let skid = skid_of_spki(&test_spki_b64(&test_key())).unwrap();
                move |k| verify_license_with(k, &vk, &skid)
            }),
        );
        match expired.status().entitlement {
            Entitlement::LicenseRequired { ref reason } => assert_eq!(reason, "revoked"),
            ref other => panic!("a revoked license past the trial must gate, got {other:?}"),
        }
    }

    /// Revoking somebody ELSE's license must not touch this machine.
    #[test]
    fn license_unrelated_revocation_does_not_invalidate() {
        let (dir, store) = temp_env("revoked-other");
        let key = sign_with(&test_key(), valid_claims());
        let mgr = test_manager(dir.path(), store, 1_000_000);
        mgr.activate(&key).unwrap();
        mgr.apply_revocations(vec!["somebody-elses-kid".into()], 1_000_100);
        assert!(matches!(
            mgr.status().entitlement,
            Entitlement::Licensed { .. }
        ));
    }

    #[test]
    fn license_revocation_url_is_https_on_the_issuer_host_only() {
        assert!(REVOCATION_URL.starts_with("https://"));
        let host = REVOCATION_URL
            .trim_start_matches("https://")
            .split('/')
            .next()
            .unwrap();
        assert_eq!(host, ISSUER_HOST);
        assert!(REVOCATION_URL.ends_with("/v1/yap/revoked.json"));
    }

    #[test]
    fn license_offline_revocation_failure_changes_nothing() {
        let (dir, store) = temp_env("offline");
        let key = sign_with(&test_key(), valid_claims());
        let mgr = test_manager(dir.path(), store, 1_000_000);
        mgr.activate(&key).unwrap();
        // A list that HAS been fetched before, so we can prove it survives.
        mgr.apply_revocations(vec!["someone-else".into()], 900);
        let before = mgr.status();
        let before_file = mgr.file_snapshot();

        // This is the production decision point, not a stand-in for it.
        for failure in ["dns error", "http_503", "expected value at line 1"] {
            assert!(!mgr.apply_fetch_result(Err(failure.into())));
        }

        let after = mgr.status();
        let after_file = mgr.file_snapshot();
        assert_eq!(before.entitlement, after.entitlement);
        assert_eq!(before.revoked_count, after.revoked_count);
        assert_eq!(before_file.revoked_kids, after_file.revoked_kids);
        assert_eq!(
            before_file.revoked_checked_at_ms,
            after_file.revoked_checked_at_ms
        );
        assert!(after.allows_new_dictation());
    }

    // ── (4) trial arithmetic ──

    fn inputs(
        start: Option<i64>,
        db_start: Option<i64>,
        floor: Option<i64>,
        now: i64,
    ) -> TrialInputs {
        TrialInputs {
            file_started_at_ms: start,
            db_started_at_ms: db_start,
            file_floor_ms: floor,
            db_floor_ms: None,
            wall_now_ms: now,
            monotonic_now_ms: now,
        }
    }

    #[test]
    fn trial_starts_on_first_run_and_lasts_fourteen_days() {
        let t = evaluate_trial(inputs(None, None, None, 1_000_000));
        assert_eq!(t.started_at_ms, 1_000_000);
        assert_eq!(t.expires_at_ms, 1_000_000 + TRIAL_MS);
        assert!(!t.expired);
        assert_eq!(t.days_left, 14);
    }

    #[test]
    fn trial_counts_down_and_expires_after_fourteen_days() {
        let start = 1_000_000;
        let day13 = evaluate_trial(inputs(Some(start), None, None, start + 13 * DAY_MS));
        assert!(!day13.expired);
        assert_eq!(day13.days_left, 1);

        let exactly = evaluate_trial(inputs(Some(start), None, None, start + TRIAL_MS));
        assert!(exactly.expired, "the trial is over the instant it is over");
        assert_eq!(exactly.days_left, 0);

        let after = evaluate_trial(inputs(Some(start), None, None, start + TRIAL_MS + 1));
        assert!(after.expired);
    }

    #[test]
    fn trial_clock_rollback_does_not_extend_it() {
        let start = 1_000_000;
        // Fourteen days later the app has recorded a floor…
        let expired = evaluate_trial(inputs(Some(start), None, None, start + TRIAL_MS + DAY_MS));
        assert!(expired.expired);
        assert_eq!(expired.floor_ms, start + TRIAL_MS + DAY_MS);

        // …so winding the Mac's clock back to day 1 changes nothing.
        let rolled = evaluate_trial(inputs(
            Some(start),
            None,
            Some(expired.floor_ms),
            start + DAY_MS,
        ));
        assert!(rolled.expired, "a clock rollback must not revive the trial");
        assert_eq!(rolled.effective_now_ms, expired.floor_ms);
    }

    #[test]
    fn trial_in_session_rollback_is_caught_by_the_monotonic_reading() {
        let start = 1_000_000;
        // Wall clock yanked back mid-session; the monotonic anchor still knows.
        let t = evaluate_trial(TrialInputs {
            file_started_at_ms: Some(start),
            db_started_at_ms: None,
            file_floor_ms: None,
            db_floor_ms: None,
            wall_now_ms: start - 10 * DAY_MS,
            monotonic_now_ms: start + TRIAL_MS + 1,
        });
        assert!(t.expired);
    }

    #[test]
    fn trial_earliest_start_wins_across_the_two_stores() {
        let early = 1_000_000;
        let late = early + 10 * DAY_MS;
        let t = evaluate_trial(inputs(Some(late), Some(early), None, late));
        assert_eq!(t.started_at_ms, early, "the earlier of the two stores wins");
    }

    #[test]
    fn trial_survives_deleting_the_license_file() {
        let start = 1_000_000;
        // File gone, DB row intact — the trial keeps its original start.
        let t = evaluate_trial(inputs(None, Some(start), None, start + TRIAL_MS + 1));
        assert_eq!(t.started_at_ms, start);
        assert!(
            t.expired,
            "deleting license.json must not restart the trial"
        );
    }

    #[test]
    fn trial_start_in_the_future_is_clamped() {
        let now = 1_000_000;
        let t = evaluate_trial(inputs(Some(now + 365 * DAY_MS), None, None, now));
        assert_eq!(t.started_at_ms, now);
        assert_eq!(t.expires_at_ms, now + TRIAL_MS);
    }

    #[test]
    fn trial_state_is_written_to_both_stores() {
        let (dir, store) = temp_env("both-stores");
        let mgr = LicenseManager::with_clock(dir.path(), store.clone(), Box::new(|| 5_000_000));
        let status = mgr.status();
        assert_eq!(status.trial_days_left, 14);

        let from_db = parse_ms(store.get(DB_KEY_TRIAL_STARTED)).expect("DB row written");
        let from_file = mgr
            .file_snapshot()
            .trial_started_at_ms
            .expect("file written");
        assert_eq!(from_db, from_file);

        // Delete the file entirely: the DB row still carries the start.
        std::fs::remove_file(dir.path().join(LICENSE_FILE)).unwrap();
        let reopened =
            LicenseManager::with_clock(dir.path(), store, Box::new(|| 5_000_000 + 13 * DAY_MS));
        assert_eq!(reopened.status().trial_days_left, 1);
    }

    /// The gate calls `status()` on every dictation start, on the
    /// press→capture span YV35 measures in milliseconds. Once the trial's
    /// bookkeeping has settled, asking it again must cost pure reads — no
    /// SQLite write, no file rewrite — or Yap pays disk I/O for the privilege
    /// of letting the user speak.
    #[test]
    fn trial_steady_state_costs_no_writes() {
        let dir = tempdir::TempDir::new("no-writes");
        let store = Arc::new(MemoryStore::default());
        let mgr = LicenseManager::with_clock(
            dir.path(),
            store.clone() as Arc<dyn LicenseStore>,
            Box::new(|| 5_000_000),
        );
        let settled = store.writes();
        let file_mtime = std::fs::metadata(dir.path().join(LICENSE_FILE))
            .unwrap()
            .modified()
            .unwrap();

        for _ in 0..50 {
            assert!(mgr.allows_new_dictation());
        }
        assert_eq!(
            store.writes(),
            settled,
            "a settled trial must not write to the DB on the dictation-start path"
        );
        assert_eq!(
            std::fs::metadata(dir.path().join(LICENSE_FILE))
                .unwrap()
                .modified()
                .unwrap(),
            file_mtime,
            "a settled trial must not rewrite license.json on the dictation-start path"
        );
    }

    /// …but the floor DOES still advance, or the rollback protection above is
    /// decoration. One quantum later, both stores move.
    #[test]
    fn trial_floor_advances_once_per_quantum() {
        let dir = tempdir::TempDir::new("floor-quantum");
        let store = Arc::new(MemoryStore::default());
        let handle = store.clone() as Arc<dyn LicenseStore>;
        let _first = LicenseManager::with_clock(dir.path(), handle.clone(), Box::new(|| 5_000_000));
        let later = LicenseManager::with_clock(
            dir.path(),
            handle,
            Box::new(|| 5_000_000 + FLOOR_QUANTUM_MS + 1),
        );
        let _ = later.status();
        assert!(
            parse_ms(store.get(DB_KEY_CLOCK_FLOOR)).unwrap() >= 5_000_000 + FLOOR_QUANTUM_MS,
            "the rollback floor must still advance across quanta"
        );
    }

    /// A store that lost its copy is repaired from the survivor — that is the
    /// entire reason the trial is written twice.
    #[test]
    fn trial_missing_db_row_is_rebuilt_from_the_file() {
        let dir = tempdir::TempDir::new("repair-db");
        let first = Arc::new(MemoryStore::default()) as Arc<dyn LicenseStore>;
        let mgr = LicenseManager::with_clock(dir.path(), first, Box::new(|| 5_000_000));
        let started = mgr.file_snapshot().trial_started_at_ms.unwrap();

        // Fresh (empty) DB, same license.json — as if the database had been
        // quarantined and recreated.
        let rebuilt = Arc::new(MemoryStore::default());
        let mgr2 = LicenseManager::with_clock(
            dir.path(),
            rebuilt.clone() as Arc<dyn LicenseStore>,
            Box::new(|| 5_000_000 + DAY_MS),
        );
        let _ = mgr2.status();
        assert_eq!(
            parse_ms(rebuilt.get(DB_KEY_TRIAL_STARTED)),
            Some(started),
            "the corroborating row must be rebuilt, not silently left missing"
        );
    }

    // ── (5) the gate ──

    #[test]
    fn license_expired_trial_requires_a_license_for_new_dictation() {
        let (dir, store) = temp_env("expired-gate");
        let mgr = LicenseManager::with_clock(dir.path(), store.clone(), Box::new(|| 1_000_000));
        assert!(mgr.allows_new_dictation());

        let later = LicenseManager::with_clock(
            dir.path(),
            store,
            Box::new(|| 1_000_000 + TRIAL_MS + DAY_MS),
        );
        let status = later.status();
        assert!(!status.allows_new_dictation());
        match status.entitlement {
            Entitlement::LicenseRequired { ref reason } => assert_eq!(reason, "trial_expired"),
            ref other => panic!("expected license_required, got {other:?}"),
        }
        // The typed code the command layer returns.
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["state"], "license_required");
        assert_eq!(LICENSE_REQUIRED_CODE, "license_required");
    }

    #[test]
    fn license_gate_notice_is_throttled() {
        let (dir, store) = temp_env("throttle");
        let mgr = LicenseManager::new(dir.path(), store);
        assert!(mgr.should_announce_gate());
        assert!(
            !mgr.should_announce_gate(),
            "a held hotkey must not become a notification storm"
        );
    }

    /// The whole point of "lifetime": a licensed machine is unaffected by the
    /// trial clock, a year after the trial ended.
    #[test]
    fn license_licensed_machine_dictates_normally_past_the_trial() {
        let (dir, store) = temp_env("licensed-past-trial");
        let key = sign_with(&test_key(), valid_claims());
        let mgr = test_manager(dir.path(), store.clone(), 1_000_000);
        mgr.activate(&key).unwrap();

        let signer = test_key();
        let skid = skid_of_spki(&test_spki_b64(&signer)).unwrap();
        let vk = signer.verifying_key();
        let year_later = LicenseManager::build(
            dir.path(),
            store,
            Box::new(|| 1_000_000 + 365 * DAY_MS),
            Box::new(move |k| verify_license_with(k, &vk, &skid)),
        );
        let status = year_later.status();
        assert!(status.trial_days_left == 0, "the trial itself is long gone");
        assert!(
            status.allows_new_dictation(),
            "a licensed machine keeps dictating"
        );
        match status.entitlement {
            Entitlement::Licensed {
                ref plan, seats, ..
            } => {
                assert_eq!(plan, SOLD_PLAN);
                assert_eq!(seats, 3);
            }
            ref other => panic!("expected licensed, got {other:?}"),
        }
        assert!(status.license_problem.is_none());
    }

    #[test]
    fn license_deactivate_does_not_hand_out_a_second_trial() {
        let (dir, store) = temp_env("deactivate");
        let key = sign_with(&test_key(), valid_claims());
        let mgr = test_manager(dir.path(), store, 1_000_000);
        mgr.activate(&key).unwrap();
        let started = mgr.file_snapshot().trial_started_at_ms;
        mgr.deactivate();
        let after = mgr.status();
        assert!(matches!(after.entitlement, Entitlement::Trial { .. }));
        assert_eq!(mgr.file_snapshot().trial_started_at_ms, started);
    }

    #[test]
    fn license_valid_license_allows_new_dictation_forever() {
        // Entitlement is decided by `Entitlement::allows_new_dictation`, which
        // is what the gate calls. A licensed state allows dictation whatever the
        // trial says; an expired one does not.
        let licensed = Entitlement::Licensed {
            plan: SOLD_PLAN.into(),
            seats: 3,
            kid: "abc".into(),
        };
        assert!(licensed.allows_new_dictation());
        assert!(Entitlement::Trial {
            days_left: 1,
            expires_at_ms: 0
        }
        .allows_new_dictation());
        assert!(!Entitlement::LicenseRequired {
            reason: "trial_expired".into()
        }
        .allows_new_dictation());
    }

    #[test]
    fn license_required_message_says_what_still_works() {
        // The wording is load-bearing: this is the moment a paying-or-not user
        // decides whether Yap held their words hostage.
        let m = LICENSE_REQUIRED_MESSAGE.to_lowercase();
        for word in ["history", "exports", "settings"] {
            assert!(m.contains(word), "the gate message must promise `{word}`");
        }
    }

    // ── a tiny temp-dir helper (no new dependency for one directory) ──

    mod tempdir {
        use std::path::{Path, PathBuf};

        pub struct TempDir(PathBuf);

        impl TempDir {
            pub fn new(tag: &str) -> Self {
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos();
                let p = std::env::temp_dir().join(format!("yap-license-{tag}-{nanos}"));
                std::fs::create_dir_all(&p).unwrap();
                TempDir(p)
            }
            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }
}
