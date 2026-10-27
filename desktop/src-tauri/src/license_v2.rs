//! yap-lic-01 — the offline verifier for drivia.consulting license tokens.
//!
//! A token is a compact JWS (`header.payload.signature`, each base64url) that
//! the consulting app's licensing API signs with Ed25519 (`alg: "EdDSA"`,
//! `typ: "JWT"`, `kid` in the header) and Yap verifies with NO network at all.
//! The claims are the issuer's `claimsSchema` in the consulting repo's
//! `src/lib/licensing/tokens.ts`: `sub` (license id), `act` (activation id),
//! `prd`, `plan`, `ent`, `dev`, `iat`, `exp` — the last two in SECONDS.
//!
//! The trust decision in one paragraph (Panel 2026-10-19, BLOCKING): the only
//! public keys this verifier will ever consult are [`EMBEDDED_KEYS`], compiled
//! into the binary. There is no key fetch, no key-set URL, no key cache on disk,
//! and the public entry point [`verify_token`] takes no key-set argument, so no
//! caller can widen the trusted set at runtime. A token whose `kid` is not
//! embedded is [`TokenError::UnknownKid`], which the UI reads as "re-activate":
//! a key rotation ships as a Yap release, never as a download.
//!
//! Order of checks is part of the contract: shape, header (`alg`, `typ`, `kid`),
//! then the signature over the exact `header.payload` bytes — and only THEN is
//! the payload decoded and parsed, so no attacker-chosen JSON is ever handed to
//! the parser. Entitlement (`prd`, `ent.dictation`, `plan`) and time (`exp` +
//! [`GRACE_S`]) are judged last. The caller supplies `now_ms` from the trial's
//! rollback ratchet in `license.rs` (`max(wall, monotonic, floor)`); this module
//! deliberately has no clock of its own and no second ratchet.

use base64::Engine as _;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

/// Offline grace past the token's own `exp`, in SECONDS (Panel decision 2: the
/// TTL comes from the token, the grace is a constant here). 30 days is the Yap
/// lifetime plan's grace and the issuer's `MAX_GRACE_S`.
pub const GRACE_S: i64 = 30 * 24 * 60 * 60;

/// The `prd` claim a Yap token must carry.
pub const PRODUCT: &str = "yap";

/// The `plan` claim a Yap token must carry (the plan Yap sells).
pub const PLAN: &str = "lifetime";

/// No honest token is anywhere near this long; the issuer refuses past it too.
pub const MAX_TOKEN_LEN: usize = 8192;

/// The ONLY public keys a license token may be verified against: `(kid, raw
/// 32-byte Ed25519 public key)`.
///
/// `dcl-2026-09-24` is the public half seeded by the consulting repo's
/// `supabase/migrations/20260924002500_licensing_api.sql:258`, JWK
/// `x = NTXQamoM4o9K1MkQrZ_bWPRL7659nEDHaIssiCWodHs` (base64url). The literal is
/// test-checked against a copy of that seed in
/// `tests/fixtures/dcl-signing-keys.json` (`embedded_key_matches_server_seed`).
pub const EMBEDDED_KEYS: &[(&str, [u8; 32])] = &[(
    "dcl-2026-09-24",
    [
        0x35, 0x35, 0xd0, 0x6a, 0x6a, 0x0c, 0xe2, 0x8f, 0x4a, 0xd4, 0xc9, 0x10, 0xad, 0x9f, 0xdb,
        0x58, 0xf4, 0x4b, 0xef, 0xae, 0x7d, 0x9c, 0x40, 0xc7, 0x68, 0x8b, 0x2c, 0x88, 0x25, 0xa8,
        0x74, 0x7b,
    ],
)];

/// Why a token was refused. Every variant has a STABLE code the UI branches on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenError {
    /// Not three non-empty base64url segments, an unparseable header or
    /// payload, a `crit` header, or longer than [`MAX_TOKEN_LEN`].
    BadFormat,
    /// Header `alg` is not exactly `EdDSA` (refuses `none`, `HS256`, ...).
    BadAlg,
    /// Header `typ` is not exactly `JWT` (including missing).
    BadTyp,
    /// Header `kid` is missing or not in [`EMBEDDED_KEYS`]. UI: "re-activate".
    UnknownKid,
    /// The signature is not 64 bytes or does not verify over `header.payload`.
    BadSignature,
    /// Signed, but `prd` is not [`PRODUCT`].
    WrongProduct,
    /// Signed, but `ent.dictation` is not `true`.
    NoEntitlement,
    /// Signed, but `plan` is not [`PLAN`].
    WrongPlan,
    /// `now > exp + GRACE_S`.
    Expired,
    /// Signed and in date, but its `iat` is below the highest `iat` this install
    /// has already accepted: an older token replayed (see `check_iat`).
    IatRollback,
}

impl TokenError {
    pub fn code(self) -> &'static str {
        match self {
            TokenError::BadFormat => "bad_format",
            TokenError::BadAlg => "bad_alg",
            TokenError::BadTyp => "bad_typ",
            TokenError::UnknownKid => "unknown_kid",
            TokenError::BadSignature => "bad_signature",
            TokenError::WrongProduct => "wrong_product",
            TokenError::NoEntitlement => "no_entitlement",
            TokenError::WrongPlan => "wrong_plan",
            TokenError::Expired => "expired",
            TokenError::IatRollback => "iat_rollback",
        }
    }

    /// What a human is told. Every refusal names a next step.
    pub fn message(self) -> &'static str {
        match self {
            TokenError::BadFormat | TokenError::BadAlg | TokenError::BadTyp => {
                "That does not look like a complete Yap license. Copy it again in full and paste it as one line."
            }
            TokenError::UnknownKid => {
                "This license was signed with a key this version of Yap does not know. Re-activate Yap to get a fresh license, or update Yap."
            }
            TokenError::BadSignature => {
                "That license did not check out. Copy it again in full; if it still fails, re-activate Yap."
            }
            TokenError::WrongProduct => "That license is for a different app, not Yap.",
            TokenError::NoEntitlement => {
                "That license does not include dictation. Reply to your purchase email and we will sort it out."
            }
            TokenError::WrongPlan => "That license is not a Yap lifetime license.",
            TokenError::Expired => {
                "Your Yap license has not been refreshed for too long. Re-activate Yap while online to renew it."
            }
            TokenError::IatRollback => {
                "That license is older than one this Mac already uses. Re-activate Yap to get the current one."
            }
        }
    }
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

/// The verified claims of a license token. `kid` is filled from the HEADER
/// after the signature verifies; it is never read from the payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenClaims {
    pub sub: String,
    pub act: String,
    pub prd: String,
    pub plan: String,
    pub ent: serde_json::Map<String, serde_json::Value>,
    pub dev: String,
    /// Issued-at, seconds since the epoch.
    pub iat: i64,
    /// Expiry, seconds since the epoch. Grace runs on top of this.
    pub exp: i64,
    #[serde(skip)]
    pub kid: String,
}

#[derive(Deserialize)]
struct Header {
    #[serde(default)]
    alg: Option<String>,
    #[serde(default)]
    typ: Option<String>,
    #[serde(default)]
    kid: Option<String>,
    #[serde(default)]
    crit: Option<serde_json::Value>,
}

/// Machine-to-machine base64url: unpadded, URL alphabet, nothing forgiven.
fn b64url(seg: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(seg.as_bytes())
        .ok()
}

/// Does this pasted string have the SHAPE of a compact JWS (three segments)?
/// A v1 Yap license key has two. Routing only; [`verify_token`] decides.
pub fn looks_like_token(s: &str) -> bool {
    s.trim().split('.').count() == 3
}

/// Verify a license token against [`EMBEDDED_KEYS`] and nothing else.
///
/// `now_ms` must come from the trial's rollback ratchet (`license.rs`), never a
/// bare wall-clock read. There is deliberately no key-set parameter.
pub fn verify_token(token: &str, now_ms: i64) -> Result<TokenClaims, TokenError> {
    verify_with(EMBEDDED_KEYS, token, now_ms)
}

/// The verifier with the trusted key set injected. Crate-private: production
/// passes [`EMBEDDED_KEYS`] (through [`verify_token`]); only `#[cfg(test)]`
/// code passes throwaway keys.
pub(crate) fn verify_with(
    keys: &[(&str, [u8; 32])],
    token: &str,
    now_ms: i64,
) -> Result<TokenClaims, TokenError> {
    let token = token.trim();
    if token.is_empty() || token.len() > MAX_TOKEN_LEN {
        return Err(TokenError::BadFormat);
    }
    let mut parts = token.split('.');
    let (Some(h64), Some(p64), Some(s64), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(TokenError::BadFormat);
    };
    if h64.is_empty() || p64.is_empty() || s64.is_empty() {
        return Err(TokenError::BadFormat);
    }

    // Header: the only bytes parsed before the signature, and only to learn
    // which embedded key to use and that the algorithm is the pinned one.
    let header: Header = b64url(h64)
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .ok_or(TokenError::BadFormat)?;
    if header.alg.as_deref() != Some("EdDSA") {
        return Err(TokenError::BadAlg);
    }
    if header.typ.as_deref() != Some("JWT") {
        return Err(TokenError::BadTyp);
    }
    // RFC 7515 §4.1.11: a `crit` this verifier does not implement must refuse.
    if header.crit.is_some() {
        return Err(TokenError::BadFormat);
    }
    let kid = header.kid.ok_or(TokenError::UnknownKid)?;
    let key_bytes = keys
        .iter()
        .find(|(k, _)| *k == kid)
        .map(|(_, bytes)| bytes)
        .ok_or(TokenError::UnknownKid)?;
    let key = VerifyingKey::from_bytes(key_bytes).map_err(|_| TokenError::UnknownKid)?;

    // Signature over the exact `header.payload` bytes, BEFORE the payload is
    // decoded. `verify_strict` refuses small-order / non-canonical inputs.
    let sig: [u8; 64] = b64url(s64)
        .and_then(|raw| raw.try_into().ok())
        .ok_or(TokenError::BadSignature)?;
    let signing_input = &token[..h64.len() + 1 + p64.len()];
    key.verify_strict(signing_input.as_bytes(), &Signature::from_bytes(&sig))
        .map_err(|_| TokenError::BadSignature)?;

    let mut claims: TokenClaims = b64url(p64)
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .ok_or(TokenError::BadFormat)?;
    claims.kid = kid;

    if claims.prd != PRODUCT {
        return Err(TokenError::WrongProduct);
    }
    if claims.ent.get("dictation") != Some(&serde_json::Value::Bool(true)) {
        return Err(TokenError::NoEntitlement);
    }
    if claims.plan != PLAN {
        return Err(TokenError::WrongPlan);
    }
    if now_ms.div_euclid(1000) > claims.exp.saturating_add(GRACE_S) {
        return Err(TokenError::Expired);
    }
    Ok(claims)
}

/// The `iat` half of the rollback ratchet: refuse a token issued before the
/// newest one this install has accepted (`max_iat`, the max of `license.json`
/// and the SQLite corroborating row, both kept by `license.rs`).
pub fn check_iat(claims: &TokenClaims, max_iat: Option<i64>) -> Result<(), TokenError> {
    match max_iat {
        Some(max) if claims.iat < max => Err(TokenError::IatRollback),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::license::{Entitlement, LicenseManager, LicenseStore, MemoryStore};
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::Arc;

    const TEST_KID: &str = "test-kid-throwaway";
    const DAY_S: i64 = 86_400;
    /// 2027-01-15T08:00:00Z, seconds — an arbitrary issue time.
    const IAT: i64 = 1_800_000_000;
    const EXP: i64 = IAT + 30 * DAY_S;

    fn test_key() -> SigningKey {
        SigningKey::from_bytes(&[9u8; 32])
    }

    fn test_keys() -> [(&'static str, [u8; 32]); 1] {
        [(TEST_KID, test_key().verifying_key().to_bytes())]
    }

    fn enc(v: &Value) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(v).unwrap())
    }

    fn header() -> Value {
        json!({ "alg": "EdDSA", "kid": TEST_KID, "typ": "JWT" })
    }

    fn claims_at(iat: i64) -> Value {
        json!({
            "sub": "7f1c2b9e-0000-4000-8000-000000000001",
            "act": "7f1c2b9e-0000-4000-8000-000000000002",
            "prd": "yap",
            "plan": "lifetime",
            "ent": { "dictation": true },
            "dev": "0".repeat(64),
            "iat": iat,
            "exp": iat + 30 * DAY_S,
        })
    }

    fn claims() -> Value {
        claims_at(IAT)
    }

    fn mint_with(sk: &SigningKey, header: &Value, claims: &Value) -> String {
        let input = format!("{}.{}", enc(header), enc(claims));
        let sig = sk.sign(input.as_bytes());
        format!(
            "{input}.{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sig.to_bytes())
        )
    }

    fn mint(header: &Value, claims: &Value) -> String {
        mint_with(&test_key(), header, claims)
    }

    fn at_s(s: i64) -> i64 {
        s * 1000
    }

    fn verify(token: &str, now_ms: i64) -> Result<TokenClaims, TokenError> {
        verify_with(&test_keys(), token, now_ms)
    }

    // ── tempdir + manager wiring (license.rs's own helpers are private) ──

    struct TempDir(std::path::PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let p = std::env::temp_dir().join(format!("yap-license-v2-{tag}-{nanos}"));
            std::fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The production manager with ONE substitution: tokens verify against the
    /// throwaway key. The clock is shared so a test can move it between calls.
    fn manager(
        dir: &std::path::Path,
        store: Arc<dyn LicenseStore>,
        clock: Arc<AtomicI64>,
    ) -> LicenseManager {
        let keys = test_keys();
        LicenseManager::with_token_verifier(
            dir,
            store,
            Box::new(move || clock.load(Ordering::SeqCst)),
            Box::new(move |t, now| verify_with(&keys, t, now)),
        )
    }

    // ── the embedded key ──

    #[test]
    fn embedded_key_matches_server_seed() {
        let raw = include_str!("../tests/fixtures/dcl-signing-keys.json");
        let doc: Value = serde_json::from_str(raw).unwrap();
        let jwk = doc["keys"]
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["kid"] == "dcl-2026-09-24")
            .expect("fixture carries kid dcl-2026-09-24");
        assert_eq!(jwk["kty"], "OKP");
        assert_eq!(jwk["crv"], "Ed25519");
        assert_eq!(jwk["alg"], "EdDSA");
        let x = b64url(jwk["x"].as_str().unwrap()).expect("x is base64url");
        let (kid, bytes) = EMBEDDED_KEYS
            .iter()
            .find(|(k, _)| *k == "dcl-2026-09-24")
            .expect("dcl-2026-09-24 is embedded");
        assert_eq!(*kid, "dcl-2026-09-24");
        assert_eq!(x.as_slice(), bytes.as_slice());
        assert_eq!(
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes),
            "NTXQamoM4o9K1MkQrZ_bWPRL7659nEDHaIssiCWodHs"
        );
        // And it is a usable Ed25519 point, not just 32 bytes.
        assert!(VerifyingKey::from_bytes(bytes).is_ok());
        // Exactly one trusted key today; a rotation is a deliberate edit here.
        assert_eq!(EMBEDDED_KEYS.len(), 1);
    }

    #[test]
    fn verifier_signature_takes_no_key_set() {
        // Compile-time pin: the public verifier is (token, now) -> result. A
        // key-set parameter added later fails to compile right here.
        let f: fn(&str, i64) -> Result<TokenClaims, TokenError> = verify_token;
        assert_eq!(f("a.b", 0), Err(TokenError::BadFormat));
    }

    // ── vectors ──

    #[test]
    fn valid_token_accepted() {
        let c = verify(&mint(&header(), &claims()), at_s(IAT + 60)).unwrap();
        assert_eq!(c.prd, "yap");
        assert_eq!(c.plan, "lifetime");
        assert_eq!(c.kid, TEST_KID);
        assert_eq!(c.iat, IAT);
        assert_eq!(c.exp, EXP);
    }

    #[test]
    fn tampered_payload_refused() {
        let tok = mint(&header(), &claims());
        let parts: Vec<&str> = tok.split('.').collect();
        let mut forged = claims();
        forged["exp"] = json!(EXP + 3650 * DAY_S);
        let t = format!("{}.{}.{}", parts[0], enc(&forged), parts[2]);
        assert_eq!(verify(&t, at_s(IAT)), Err(TokenError::BadSignature));
    }

    #[test]
    fn tampered_signature_refused() {
        let tok = mint(&header(), &claims());
        let (head, sig) = tok.rsplit_once('.').unwrap();
        let mut raw = b64url(sig).unwrap();
        raw[0] ^= 0x01;
        let t = format!(
            "{head}.{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&raw)
        );
        assert_eq!(verify(&t, at_s(IAT)), Err(TokenError::BadSignature));
        // A truncated signature is a bad signature too, never a panic.
        assert_eq!(
            verify(&format!("{head}.AAAA"), at_s(IAT)),
            Err(TokenError::BadSignature)
        );
    }

    #[test]
    fn signature_checked_before_payload_parsed() {
        // Unsigned garbage in the payload must be refused for its SIGNATURE,
        // proving the parser never saw it.
        let h = enc(&header());
        let t = format!("{h}.{}.{}", "not-json-at-all", "A".repeat(86));
        assert_eq!(verify(&t, at_s(IAT)), Err(TokenError::BadSignature));
    }

    #[test]
    fn alg_none_refused() {
        let mut h = header();
        h["alg"] = json!("none");
        let unsigned = format!("{}.{}.", enc(&h), enc(&claims()));
        // RFC 7519 unsecured shape (empty signature) and a signed variant.
        assert_eq!(verify(&unsigned, at_s(IAT)), Err(TokenError::BadFormat));
        assert_eq!(
            verify(&mint(&h, &claims()), at_s(IAT)),
            Err(TokenError::BadAlg)
        );
    }

    #[test]
    fn alg_hs256_refused() {
        let mut h = header();
        h["alg"] = json!("HS256");
        assert_eq!(
            verify(&mint(&h, &claims()), at_s(IAT)),
            Err(TokenError::BadAlg)
        );
        let mut h = header();
        h.as_object_mut().unwrap().remove("alg");
        assert_eq!(
            verify(&mint(&h, &claims()), at_s(IAT)),
            Err(TokenError::BadAlg)
        );
    }

    #[test]
    fn typ_missing_refused() {
        let mut h = header();
        h.as_object_mut().unwrap().remove("typ");
        assert_eq!(
            verify(&mint(&h, &claims()), at_s(IAT)),
            Err(TokenError::BadTyp)
        );
        h["typ"] = json!("at+jwt");
        assert_eq!(
            verify(&mint(&h, &claims()), at_s(IAT)),
            Err(TokenError::BadTyp)
        );
    }

    #[test]
    fn crit_header_refused() {
        let mut h = header();
        h["crit"] = json!(["exp"]);
        assert_eq!(
            verify(&mint(&h, &claims()), at_s(IAT)),
            Err(TokenError::BadFormat)
        );
    }

    #[test]
    fn unknown_kid_means_reactivate() {
        let mut h = header();
        h["kid"] = json!("dcl-2099-01-01");
        let e = verify(&mint(&h, &claims()), at_s(IAT)).unwrap_err();
        assert_eq!(e, TokenError::UnknownKid);
        assert_eq!(e.code(), "unknown_kid");
        assert!(e.message().contains("Re-activate"));
        h.as_object_mut().unwrap().remove("kid");
        assert_eq!(
            verify(&mint(&h, &claims()), at_s(IAT)),
            Err(TokenError::UnknownKid)
        );
    }

    #[test]
    fn wrong_product_refused() {
        let mut c = claims();
        c["prd"] = json!("learn");
        assert_eq!(
            verify(&mint(&header(), &c), at_s(IAT)),
            Err(TokenError::WrongProduct)
        );
    }

    #[test]
    fn no_dictation_entitlement_refused() {
        for ent in [
            json!({}),
            json!({ "dictation": false }),
            json!({ "dictation": "true" }),
        ] {
            let mut c = claims();
            c["ent"] = ent;
            assert_eq!(
                verify(&mint(&header(), &c), at_s(IAT)),
                Err(TokenError::NoEntitlement)
            );
        }
    }

    #[test]
    fn plan_not_lifetime_refused() {
        let mut c = claims();
        c["plan"] = json!("monthly");
        assert_eq!(
            verify(&mint(&header(), &c), at_s(IAT)),
            Err(TokenError::WrongPlan)
        );
    }

    #[test]
    fn expired_in_grace_accepted() {
        let tok = mint(&header(), &claims());
        assert!(verify(&tok, at_s(EXP + 1)).is_ok());
        assert!(verify(&tok, at_s(EXP + GRACE_S)).is_ok());
        assert!(verify(&tok, at_s(EXP + GRACE_S) + 999).is_ok());
    }

    #[test]
    fn past_grace_refused() {
        let tok = mint(&header(), &claims());
        assert_eq!(
            verify(&tok, at_s(EXP + GRACE_S + 1)),
            Err(TokenError::Expired)
        );
        assert_eq!(GRACE_S, 30 * DAY_S);
    }

    #[test]
    fn error_codes_are_stable() {
        let all = [
            (TokenError::BadFormat, "bad_format"),
            (TokenError::BadAlg, "bad_alg"),
            (TokenError::BadTyp, "bad_typ"),
            (TokenError::UnknownKid, "unknown_kid"),
            (TokenError::BadSignature, "bad_signature"),
            (TokenError::WrongProduct, "wrong_product"),
            (TokenError::NoEntitlement, "no_entitlement"),
            (TokenError::WrongPlan, "wrong_plan"),
            (TokenError::Expired, "expired"),
            (TokenError::IatRollback, "iat_rollback"),
        ];
        for (e, code) in all {
            assert_eq!(e.code(), code);
            assert_eq!(e.to_string(), code);
            assert!(!e.message().is_empty());
        }
    }

    #[test]
    fn malformed_shapes_refused() {
        for t in ["", "a", "a.b", "a.b.c.d", ".b.c", "a..c", "a.b."] {
            assert_eq!(verify(t, at_s(IAT)), Err(TokenError::BadFormat), "{t:?}");
        }
        let long = format!("{}.{}.{}", "a".repeat(MAX_TOKEN_LEN), "b", "c");
        assert_eq!(verify(&long, at_s(IAT)), Err(TokenError::BadFormat));
    }

    // ── the ratchet, through the real manager ──

    /// Licensed at T, time passes past grace, then the Mac's clock is wound
    /// back to a moment the token was valid — and a fresh launch still refuses
    /// it, because "now" is the trial ratchet's max(wall, monotonic, floor).
    #[test]
    fn clock_rollback_refused() {
        let dir = TempDir::new("rollback");
        let store: Arc<dyn LicenseStore> = Arc::new(MemoryStore::default());
        let tok = mint(&header(), &claims());
        let honest = at_s(IAT + 60);
        let late = at_s(EXP + GRACE_S + DAY_S);

        // Launch 1: honest clock, token installs, Yap is licensed.
        let mgr = manager(dir.path(), store.clone(), Arc::new(AtomicI64::new(honest)));
        let st = mgr.install_token(&tok).expect("valid token installs");
        assert!(matches!(st.entitlement, Entitlement::Licensed { .. }));
        drop(mgr);

        // Launch 2, weeks later, past exp + grace: refused, and the trial
        // ratchet records this launch's honest clock as the floor. (A forward
        // jump INSIDE one session is an excursion the floor deliberately
        // ignores — `CLOCK_EXCURSION_TOLERANCE_MS` — so time passes across a
        // relaunch here, as it does on a real Mac.)
        let mgr = manager(dir.path(), store.clone(), Arc::new(AtomicI64::new(late)));
        let st = mgr.status();
        assert!(!matches!(st.entitlement, Entitlement::Licensed { .. }));
        assert_eq!(st.license_problem.as_deref(), Some("expired"));
        drop(mgr);

        // Launch 3: the clock was wound back to when the token was valid.
        let rolled = Arc::new(AtomicI64::new(honest));
        let mgr = manager(dir.path(), store.clone(), rolled);
        let st = mgr.status();
        assert!(
            !matches!(st.entitlement, Entitlement::Licensed { .. }),
            "a rolled-back clock must not revive an expired token"
        );
        assert_eq!(st.license_problem.as_deref(), Some("expired"));
        assert_eq!(mgr.install_token(&tok).unwrap_err(), TokenError::Expired);

        // Positive control: the same token at the same wall time on a FRESH
        // install (no floor) is accepted — the floor is what refused it.
        let fresh = TempDir::new("rollback-control");
        let mgr = manager(
            fresh.path(),
            Arc::new(MemoryStore::default()),
            Arc::new(AtomicI64::new(honest)),
        );
        assert!(mgr.install_token(&tok).is_ok());
    }

    /// A newer token raises maxIat in license.json AND the SQLite row; an
    /// older one is then refused, even after license.json is deleted.
    #[test]
    fn iat_below_max_iat_refused() {
        let dir = TempDir::new("max-iat");
        let store: Arc<dyn LicenseStore> = Arc::new(MemoryStore::default());
        let clock = Arc::new(AtomicI64::new(at_s(IAT + 2 * DAY_S)));
        let older = mint(&header(), &claims_at(IAT));
        let newer = mint(&header(), &claims_at(IAT + DAY_S));

        let mgr = manager(dir.path(), store.clone(), clock.clone());
        mgr.install_token(&newer).expect("newer installs");
        assert_eq!(
            store.get(crate::license::DB_KEY_MAX_TOKEN_IAT).as_deref(),
            Some((IAT + DAY_S).to_string().as_str())
        );
        let on_disk: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join(crate::license::LICENSE_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(on_disk["max_token_iat"], json!(IAT + DAY_S));
        assert_eq!(
            mgr.install_token(&older).unwrap_err(),
            TokenError::IatRollback
        );
        drop(mgr);

        // Delete license.json: the SQLite row alone still holds the ratchet.
        std::fs::remove_file(dir.path().join(crate::license::LICENSE_FILE)).unwrap();
        let mgr = manager(dir.path(), store.clone(), clock.clone());
        assert_eq!(
            mgr.install_token(&older).unwrap_err(),
            TokenError::IatRollback
        );
        // Hand-write the older token into license.json: status refuses it too.
        drop(mgr);
        let mut file: Value = json!({ "v": 1, "license_token": older });
        file["max_token_iat"] = json!(null);
        std::fs::write(
            dir.path().join(crate::license::LICENSE_FILE),
            serde_json::to_string(&file).unwrap(),
        )
        .unwrap();
        let mgr = manager(dir.path(), store, clock);
        let st = mgr.status();
        assert!(!matches!(st.entitlement, Entitlement::Licensed { .. }));
        assert_eq!(st.license_problem.as_deref(), Some("iat_rollback"));
        // The pure half.
        let c = verify(&older, at_s(IAT)).unwrap();
        assert_eq!(check_iat(&c, Some(IAT + 1)), Err(TokenError::IatRollback));
        assert_eq!(check_iat(&c, Some(IAT)), Ok(()));
        assert_eq!(check_iat(&c, None), Ok(()));
    }

    /// A key sitting in a would-be key cache on disk — beside license.json, in
    /// the JWKS shape the issuer publishes — buys nothing: only EMBEDDED_KEYS
    /// are consulted, by the pure verifier and by the production manager.
    #[test]
    fn non_embedded_key_fails_even_when_cached() {
        let dir = TempDir::new("cached-key");
        let rogue = test_key();
        let cache = json!({ "keys": [{
            "kid": TEST_KID, "alg": "EdDSA", "kty": "OKP", "crv": "Ed25519", "use": "sig",
            "x": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rogue.verifying_key().to_bytes()),
        }]});
        for name in [
            "drivia-licensing-keys.json",
            "license-keys.json",
            "jwks.json",
        ] {
            std::fs::write(dir.path().join(name), cache.to_string()).unwrap();
        }
        let tok = mint_with(&rogue, &header(), &claims());
        // Sanity: the token IS valid under the cached key.
        assert!(verify(&tok, at_s(IAT)).is_ok());
        // The production verifier refuses it: its kid is not embedded.
        assert_eq!(verify_token(&tok, at_s(IAT)), Err(TokenError::UnknownKid));
        // Same rogue key claiming the embedded kid: refused on the signature.
        let mut h = header();
        h["kid"] = json!("dcl-2026-09-24");
        assert_eq!(
            verify_token(&mint_with(&rogue, &h, &claims()), at_s(IAT)),
            Err(TokenError::BadSignature)
        );
        // And the production manager over that very directory agrees.
        let store: Arc<dyn LicenseStore> = Arc::new(MemoryStore::default());
        let mgr = LicenseManager::with_clock(dir.path(), store, Box::new(move || at_s(IAT)));
        assert_eq!(mgr.install_token(&tok).unwrap_err(), TokenError::UnknownKid);
        assert!(!matches!(
            mgr.status().entitlement,
            Entitlement::Licensed { .. }
        ));
    }

    /// The pasted-key box routes a three-segment token to the v2 path, so the
    /// existing `activate_license` command reaches this verifier.
    #[test]
    fn activate_routes_tokens_to_v2() {
        let dir = TempDir::new("activate-route");
        let clock = Arc::new(AtomicI64::new(at_s(IAT + 60)));
        let mgr = manager(dir.path(), Arc::new(MemoryStore::default()), clock);
        let st = mgr
            .activate(&mint(&header(), &claims()))
            .expect("token activates");
        assert!(
            matches!(st.entitlement, Entitlement::Licensed { ref plan, .. } if plan == "lifetime")
        );
        assert!(st.has_stored_license);
        let mut c = claims();
        c["prd"] = json!("learn");
        let e = mgr.activate(&mint(&header(), &c)).unwrap_err();
        assert_eq!(e.code(), "wrong_product");
        mgr.deactivate();
        assert!(!matches!(
            mgr.status().entitlement,
            Entitlement::Licensed { .. }
        ));
    }

    /// No key fetch, no HTTP client, no key file read in the production half of
    /// this module. Needles are split so this test cannot match itself.
    #[test]
    fn license_v2_source_has_no_key_fetch() {
        const SRC: &str = include_str!("license_v2.rs");
        let prod = SRC
            .split(concat!("#[cfg(test)]\n", "mod tests"))
            .next()
            .unwrap();
        let needles = [
            concat!("req", "west"),
            concat!("u", "req"),
            concat!("http", "://"),
            concat!("https", "://"),
            concat!("std", "::fs"),
            concat!("File", "::open"),
            concat!("read_to", "_string"),
            concat!("tauri", "::"),
        ];
        for n in needles {
            assert!(!prod.contains(n), "{n:?} in license_v2.rs production code");
        }
    }
}
