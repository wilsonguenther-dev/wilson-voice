// yap-licensing-2026-10-19 — Yap becomes a client of the drivia[.]consulting licensing API (database #2). Filed 2026-10-19 by seat
// file-license-items from ~/Obsidian/Wilson-Brain/Projects/Yap/licensing-migration-plan-2026-10-19.md ("## Items", "## Wilson's
// decisions", "## Panel 2026-10-19"). The server half is drivia-consulting scripts/loop/items/99i-yap-licensing-2026-10-19.mjs
// (dc-yap-01..07). yap-lic-00-kill-forge-fetch is NOT here: it is filed in 07b-yap-walk-defects-2026-10-19.mjs (PR #230) and ships
// alone, first. yap-lic-03 (dual-read legacy) and yap-lic-06 (remove legacy) were REJECTED by the panel; the Forge removal is folded
// into yap-lic-02.
//
// HOST SPELLING: this loop's drift guard (build.mjs DRIFT) bans the sibling host string, so these items write the licensing host as
// drivia[.]consulting in prose, drivia\\.consulting in greps and drivia.$(printf consulting) in curls. The REAL host has a plain dot:
// code constants, URLs and site copy use https://drivia + .consulting exactly (decision ledger; the server is drivia-consulting DB #2).
//
// WILSON'S DECISIONS (2026-10-19T03:45Z, "Perfect, I agree on all five." — binding, never relitigated):
//   1. $29 one-time LIFETIME, 3 Macs.   2. 60 days offline for lifetime (server: token_ttl_days 30 + grace_days 30).
//   3. Live webhook + price now, checkout closed until launch.   4. Forge signing key LOST, Forge key format DROPPED.
//   5. Copy approved: buying takes one email-code sign-in; the "no account" wording changes.
// PANEL 2026-10-19 — the three plan-level changes, as encoded here:
//   (1) NO JWKS / KEY-LIST FETCH, EVER. Yap trusts only public keys compiled into the binary (EMBEDDED_KEYS in license_v2.rs).
//       Rotation ships in a minisign-signed app update that embeds the new kid at least one release before the server signs with it.
//       An unknown kid means "re-activate", never "fetch". (Yap's reqwest uses rustls-platform-verifier, which trusts user-installed
//       root CAs, so a fetched key set is an offline keygen.) yap-lic-01 tests it; yap-lic-02 greps for its absence.
//   (2) Checkout stays closed (dc-yap-01 purchase_enabled=false) — the server's job; the Buy button here just opens drivia[.]consulting/yap,
//       which renders its own closed state.
//   (3) ONE LAUNCH (phases 2 + 3 merged): the build carrying yap-lic-02/04 is HELD OFF THE UPDATER FEED (no release:, no latest.json
//       publish) until dc-yap-03 and dc-yap-04 are live on drivia[.]consulting and yap-lic-05's copy ships in the same launch.
//
// LAUNCH SEQUENCE (same as 07): every builder starts from origin/main and build mode never lands an unreviewed PR. Items 2-4 carry a
// DEPENDENCY preflight: when the previous item (or a drivia-consulting item) is not on main, it prints the dependency marker and exits
// 0, and the harness records "skipped: dependency not on main". The marker is ASSEMBLED AT RUN TIME ($(printf MAIN)), never written
// literally (build.mjs refuses a literal marker, PR #230). Alternate build -> review, one item per round, in this order:
//   yap-lic-01-jws-verifier -> yap-lic-02-activation-client -> yap-lic-04-ui-states-and-buy -> yap-lic-05-site-privacy-copy
// Review: Opus only (review-opus + review-opus2). yap-lic-01 and yap-lic-02 are SECURITY class (two Opus security seats; the panel also
// owes a round-2 verification seat on both specs). No Codex, no Gemini, no third-party reviewer.
// HAZARD (same as 07/08): no seat launches the GUI app, signals or quits Wilson's running /Applications/Yap.app, or writes TCC; no seat
// reads or writes the real Keychain item of the installed app (tests use a throwaway service name). No live Stripe, no sends.
// SYMBOLS ARE CONTRACT: the module, function, constant and test names below are what later preflights grep for. Do not rename them.

ITEMS.push({
  id: 'yap-lic-01-jws-verifier', prompt: 'yap-lic-01-jws-verifier', branch: 'loop/yap-lic-01-jws-verifier-embedded-keys', gated: null,
  title: 'Offline verifier for drivia[.]consulting license tokens: compact JWS, alg EdDSA + typ JWT pinned, kid looked up in EMBEDDED keys only (never a fetched key set), prd yap, ent.dictation, plan lifetime, exp + 30-day grace, rollback ratchet reused',
  preflight: `
    ( M=0; grep -q 'launch_license_init_makes_no_network_call' desktop/src-tauri/src/license.rs || M=1; git grep -qnE 'REVOCATION_URL|fetch_revocations|spawn_revocation_refresh' -- desktop/src-tauri/src && M=1; if [ "$M" = 1 ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-lic-00-kill-forge-fetch has not landed (07b, PR #230: the revocation fetch is still in license.rs) — land it, then the build pass again"; exit 0; fi; test -f desktop/src-tauri/src/license_v2.rs && grep -q 'pub const EMBEDDED_KEYS' desktop/src-tauri/src/license_v2.rs && grep -q 'pub fn verify_token' desktop/src-tauri/src/license_v2.rs && grep -q 'fn non_embedded_key_fails_even_when_cached' desktop/src-tauri/src/license_v2.rs )
  `,
  spec: `
    Panel 2026-10-19: APPROVED-WITH-CHANGES (BLOCKING) — embedded keys only; grace constant 30 d; reuse the trial ratchet.
    DEPENDS: yap-lic-00-kill-forge-fetch ON MAIN. Blocks: yap-lic-02 (its preflight greps verify_token / EMBEDDED_KEYS).
    SECURITY CLASS: two Opus security reviewers.

    DO (names are contract)
      1. NEW module desktop/src-tauri/src/license_v2.rs (declared in lib.rs). pub const EMBEDDED_KEYS: &[(&str, [u8; 32])] holding
         kid "dcl-2026-09-24" with x = NTXQamoM4o9K1MkQrZ_bWPRL7659nEDHaIssiCWodHs (base64url, decoded at compile time or in a
         const fn / test-checked literal). A test (embedded_key_matches_server_seed) asserts the constant equals the public_jwk of kid
         dcl-2026-09-24 seeded at drivia-consulting supabase/migrations/20260924002500_licensing_api.sql:258. Copy that JWK into
         desktop/src-tauri/tests/fixtures/dcl-signing-keys.json, with no network. Uses the ed25519-dalek already in Cargo.toml (no new crate).
      2. pub fn verify_token(token: &str, now_ms: i64) -> Result<TokenClaims, TokenError>: exactly 3 dot-separated parts; header alg ==
         "EdDSA" and typ == "JWT" (anything else refused, incl. "none" and "HS256"); kid looked up in EMBEDDED_KEYS ONLY — an unknown
         kid returns TokenError::UnknownKid, whose UI meaning is "re-activate"; the signature is checked over the exact
         "header.payload" bytes BEFORE the payload is parsed; then prd == "yap", ent.dictation == true, plan == "lifetime"; then
         now <= exp + GRACE_S with pub const GRACE_S = 30 days (decision 2: the TTL comes from the token's own exp, the grace is a
         constant here). Stable error codes: bad_format, bad_alg, bad_typ, unknown_kid, bad_signature, wrong_product, no_entitlement,
         wrong_plan, expired.
      3. NO KEY FETCH (Panel BLOCKING): there is no code path that adds a key to the trusted set at runtime: no reqwest, no JWKS URL,
         no cache of keys on disk. The public verifier signature takes no key-set argument.
      4. ROLLBACK: reuse the existing trial ratchet in license.rs (the max(wall, monotonic, floor) clock and DB_KEY_CLOCK_FLOOR
         "license.max_seen_wall_ms", ~:114-:183) for "now"; also record the highest token iat seen (maxIat) in license.json AND the
         SQLite corroborating row, and refuse a token with iat below it. Do not write a second ratchet.
      5. Wire into license.rs entitlement as the v2 path (the Forge path is removed in yap-lic-02, not here).
    TESTS live in #[cfg(test)] mod tests inside license_v2.rs. pub fn verify_token calls a pub(crate) fn verify_with(keys, token, now_ms)
    with EMBEDDED_KEYS (crate-private, never pub; yap-lic-02's client core is its only other caller), and only #[cfg(test)] code
    passes throwaway keys. Vectors: embedded_key_matches_server_seed
    (reads the fixture), valid, tampered payload, tampered signature, alg none, alg HS256,
    typ missing, unknown kid, wrong prd, no ent.dictation, plan not lifetime, expired-in-grace (accepted), past-grace (refused),
    clock rollback (refused), iat below maxIat (refused), and non_embedded_key_fails_even_when_cached: a token signed by a key that
    is present in a would-be cache file on disk is still refused because only EMBEDDED_KEYS are consulted.`,
  acceptance: `
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    mkdir -p desktop/target
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib -- --list) > desktop/target/lic01-list.txt 2>/dev/null
    grep -q '^license_v2::tests::non_embedded_key_fails_even_when_cached: test$' desktop/target/lic01-list.txt
    grep -q '^license_v2::tests::alg_none_refused: test$' desktop/target/lic01-list.txt
    grep -q '^license_v2::tests::alg_hs256_refused: test$' desktop/target/lic01-list.txt
    grep -q '^license_v2::tests::unknown_kid_means_reactivate: test$' desktop/target/lic01-list.txt
    grep -q '^license_v2::tests::expired_in_grace_accepted: test$' desktop/target/lic01-list.txt
    grep -q '^license_v2::tests::past_grace_refused: test$' desktop/target/lic01-list.txt
    grep -q '^license_v2::tests::clock_rollback_refused: test$' desktop/target/lic01-list.txt
    grep -q '^license_v2::tests::embedded_key_matches_server_seed: test$' desktop/target/lic01-list.txt
    test -f desktop/src-tauri/tests/fixtures/dcl-signing-keys.json
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib license_v2::)
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib license)
    ! git grep -qnE 'reqwest|https?://' -- desktop/src-tauri/src/license_v2.rs
    grep -q 'pub const GRACE_S' desktop/src-tauri/src/license_v2.rs
    (cd desktop && npm run loop:validate)
  `,
})

ITEMS.push({
  id: 'yap-lic-02-activation-client', prompt: 'yap-lic-02-activation-client', branch: 'loop/yap-lic-02-activation-client-forge-removed', gated: null,
  title: 'Activate / refresh / deactivate against https://drivia[.]consulting/api/v1/licensing with X-Drivia-App; token + device_id in the Keychain (security-framework) with device_id mirrored in license.json; refresh at most every 24 h; never downgrade on network/5xx/429/400/413/403 blocked; NO key-list fetch; the Forge format deleted',
  preflight: `
    ( M=0; test -f desktop/src-tauri/src/license_v2.rs || M=1; grep -q 'pub fn verify_token' desktop/src-tauri/src/license_v2.rs || M=1; grep -q 'pub const EMBEDDED_KEYS' desktop/src-tauri/src/license_v2.rs || M=1; GH_PAGER=cat GH_PROMPT_DISABLED=1 gh api repos/wilsonguenther-dev/$(printf drivia)-consulting/contents/docs/licensing/YAP-ROUNDTRIP.md --silent >/dev/null 2>&1 || M=1; if [ "$M" = 1 ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-lic-01-jws-verifier (license_v2.rs verify_token) and drivia-consulting dc-yap-05-test-roundtrip (docs/licensing/YAP-ROUNDTRIP.md) must both be on main — land them, then the build pass again"; exit 0; fi; test -f desktop/src-tauri/src/license_client.rs && grep -q 'pub const LICENSING_BASE' desktop/src-tauri/src/license_client.rs && ! git grep -qnE 'ISSUER_SKID|GRACE_PLAN|PRODUCTION_SIGNED_FIXTURE|ISSUER_PUBLIC_KEY_SPKI_B64' -- desktop/src-tauri/src desktop/src )
  `,
  spec: `
    Panel 2026-10-19: APPROVED-WITH-CHANGES (BLOCKING) — delete the daily JWKS fetch; mirror device_id; name the Keychain crate;
    handle 400/413/403 blocked; refresh at most once per 24 h; ALSO delete the Forge format (yap-lic-06's scope, rejected as a separate
    item); retarget license_gate.rs and tray_license.rs.
    DEPENDS: yap-lic-01 ON MAIN and drivia-consulting dc-yap-05 ON ITS MAIN (preflight). Blocks: yap-lic-04.
    SECURITY CLASS: two Opus security reviewers.
    HELD OFF THE UPDATER (Panel, one launch): merging this item is fine; no release: commit and no updates/latest.json publish may carry
    it until dc-yap-03 + dc-yap-04 are live and yap-lic-05 ships with it. Wilson's own comp DCL license (issued from /desk, plan lifetime)
    must exist before that release, because the Forge key path is gone after it.

    DO (names are contract)
      1. NEW desktop/src-tauri/src/license_client.rs: pub const LICENSING_BASE = "https://drivia[.]consulting/api/v1/licensing" (a test
         asserts no .vercel.app and no other host). activate / refresh / deactivate per drivia-consulting docs/licensing/CLIENT-INTEGRATION.md,
         header X-Drivia-App = the yap app key. Activation input accepts DCL- keys only.
      2. KEYCHAIN: crate security-framework (already in Cargo.lock through rustls-platform-verifier; add it as a direct dependency at the
         locked version, no new download). Token and device_id stored as generic passwords, accessibility AfterFirstUnlockThisDeviceOnly.
         device_id = random UUID created once. MIRROR device_id into license.json (it is not a secret) so a Keychain read failure after a
         signing-identity change re-activates the SAME seat instead of burning one. On any Keychain failure fall back to re-activation,
         never lock out.
      3. REFRESH: at most once every 24 h, and always when under 7 days remain to exp. No per-launch refresh beyond that rule.
      4. NO KEY-LIST FETCH (Panel BLOCKING): no JWKS URL, no key download, no merge into a cached set. Trust = license_v2::EMBEDDED_KEYS.
         An UnknownKid from verify_token triggers re-activation, not a fetch.
      5. RESPONSES: 200 stores the token; 404 invalid_key and 409 seat_limit are shown (409 lists the devices, links the portal);
         403 revoked deletes the token; 403 disputed KEEPS the token (state "on hold"); 403 blocked, 400 invalid_request and
         413 too_large never downgrade a valid stored token; 401 per CLIENT-INTEGRATION.md; 5xx / timeout / offline keep the token;
         429 honours Retry-After. Deactivate frees the seat server-side BEFORE clearing locally.
      6. lib.rs: activate_license / deactivate_license commands call license_client.
      7. FORGE REMOVAL (decision 4): delete ISSUER_PUBLIC_KEY_SPKI_B64, ISSUER_SKID, GRACE_PLAN and its grace path,
         PRODUCTION_SIGNED_FIXTURE and the test that verifies it, and the legacy branch of decide_entitlement in license.rs, and every
         drivia-forge citation (license.rs:14, :164, :1576; desktop/src/license/status.ts:61). KEEP the license tests: retarget
         desktop/src-tauri/tests/license_gate.rs and tests/tray_license.rs to the v2 entitlement path (license_gate.rs reads source
         through include_str and tray_license.rs builds LicenseStatus values; neither one signs tokens).
    TESTS live in #[cfg(test)] mod tests inside license_client.rs, against a local stub HTTP server. The client core takes (base, keys)
    privately, and release code has no runtime base or key override (the core reaches license_v2's verify_with crate-privately, never
    pub). No internet. Cases: 200, 404 invalid_key, 409 seat_limit,
    403 revoked deletes token, 403 disputed keeps it, 403 blocked keeps it, 400 keeps it, 413 keeps it, 503/timeout keeps it, 429 honours
    Retry-After, refresh skipped inside 24 h, refresh forced under 7 d, keychain_delete_reactivates_same_seat (deletes the throwaway
    Keychain item, re-activation sends the mirrored device_id).
    MANUAL PROOF (PR body): activate a test-mode key from dc-yap-05 in a clean YAP_DATA_DIR with a dev build (never the installed app),
    dictation completes, deactivate frees the seat.`,
  acceptance: `
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    mkdir -p desktop/target
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib -- --list) > desktop/target/lic02-list.txt 2>/dev/null
    grep -q '^license_client::tests::disputed_403_keeps_token: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::revoked_403_deletes_token: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::blocked_403_keeps_token: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::too_large_413_keeps_token: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::retry_after_429_honoured: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::refresh_skipped_inside_24h: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::keychain_delete_reactivates_same_seat: test$' desktop/target/lic02-list.txt
    (cd desktop/src-tauri && cargo test --features custom-protocol --test license_gate)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test tray_license)
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib license)
    ! git grep -qn 'vercel.app' -- desktop/src-tauri/src/license_client.rs
    ! git grep -qniE 'well-known|licensing-keys|jwks' -- desktop/src-tauri/src
    ! git grep -qnE 'ISSUER_SKID|GRACE_PLAN|PRODUCTION_SIGNED_FIXTURE|ISSUER_PUBLIC_KEY_SPKI_B64|drivia-forge' -- desktop/src-tauri/src desktop/src
    grep -q 'device_id' desktop/src-tauri/src/license_client.rs
    (cd desktop && npm run loop:validate)
  `,
})

ITEMS.push({
  id: 'yap-lic-04-ui-states-and-buy', prompt: 'yap-lic-04-ui-states-and-buy', branch: 'loop/yap-lic-04-license-panel-buy-drivia', gated: null,
  title: 'One license panel plus the existing tray state: key prefix, "Manage Macs" link to the portal, grace copy with the 60-day date, seat-limit lists devices; Buy opens https://drivia[.]consulting/yap (PAYMENT_LINK_URL -> PURCHASE_URL); no legacy state',
  preflight: `
    ( M=0; test -f desktop/src-tauri/src/license_client.rs || M=1; grep -q 'pub const LICENSING_BASE' desktop/src-tauri/src/license_client.rs || M=1; git grep -qn 'GRACE_PLAN' -- desktop/src-tauri/src && M=1; GH_PAGER=cat GH_PROMPT_DISABLED=1 gh api "repos/wilsonguenther-dev/$(printf drivia)-consulting/contents/src/app/%28site%29/yap/page.tsx" --silent >/dev/null 2>&1 || M=1; if [ "$M" = 1 ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-lic-02-activation-client (license_client.rs, Forge format gone) and drivia-consulting dc-yap-03-yap-buy-page (src/app/(site)/yap/page.tsx) must both be on main — land them, then the build pass again"; exit 0; fi; grep -q 'pub const PURCHASE_URL: &str = "https://drivia\\.consulting/yap"' desktop/src-tauri/src/license.rs && ! git grep -qn 'buy.stripe.com' -- desktop/src-tauri/src desktop/src )
  `,
  spec: `
    Panel 2026-10-19: APPROVED-WITH-CHANGES — no legacy state; ONE license panel plus the EXISTING tray state (the pill and tray copy
    matrix beyond that is cut); key prefix + "Manage Macs" portal link; grace copy shows the 60-day date.
    DEPENDS: yap-lic-02 ON MAIN and drivia-consulting dc-yap-03 ON ITS MAIN (preflight).
    HELD OFF THE UPDATER with yap-lic-02 (one launch, see header).

    DO (names are contract)
      1. license.rs: PAYMENT_LINK_URL (:176, buy.stripe.com) becomes pub const PURCHASE_URL: &str = "https://drivia[.]consulting/yap".
         Every caller (frontend PurchasePrompt.tsx and the tray) uses it. The page itself says when purchases are closed; Yap shows no
         separate closed state.
      2. desktop/src/license/LicensePanel.tsx + status.ts: one panel covering not activated / active (key prefix only, e.g. "DCL-ABCD…",
         never the full key) / offline in grace ("Works offline until <date>", the date = token exp + 30 d, i.e. up to 60 days) /
         on hold (403 disputed) / revoked / seat limit (lists the devices returned with 409, plus a "Manage Macs" link to
         https://drivia[.]consulting/portal/licenses). "Manage Macs" also shows in the active state.
      3. Tray: keep the existing tray license state; map the new states onto it, no new tray copy matrix.
      4. No "Legacy key" state anywhere (decision 4).
    TESTS: vitest for status.ts state mapping and LicensePanel rendering per state (src/license); cargo tray_license still green.
    SCREENSHOTS: each panel state, headless, from a dev build with a throwaway YAP_DATA_DIR (never the installed app), in
    docs/pr-screenshots/yap-lic-04-ui-states-and-buy/.`,
  acceptance: `
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    mkdir -p desktop/target
    ! git grep -qn 'buy.stripe.com' -- desktop/src-tauri/src desktop/src
    ! git grep -qni 'legacy key' -- desktop/src
    grep -q 'pub const PURCHASE_URL: &str = "https://drivia\\.consulting/yap"' desktop/src-tauri/src/license.rs
    grep -q 'Manage Macs' desktop/src/license/LicensePanel.tsx
    (cd desktop && npx vitest run src/license src/pill/license.test.ts)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test tray_license)
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib license)
    test -n "$(ls docs/pr-screenshots/yap-lic-04-ui-states-and-buy/*.png 2>/dev/null)"
    (cd desktop && npm run loop:validate)
  `,
})

ITEMS.push({
  id: 'yap-lic-05-site-privacy-copy', prompt: 'yap-lic-05-site-privacy-copy', branch: 'loop/yap-lic-05-site-privacy-terms-copy', gated: null,
  title: 'Site, privacy and terms tell the truth for the launch: buying takes one email-code sign-in, activation needs the network once and re-checks, 60 days offline, Buy -> drivia[.]consulting/yap, updater -> yapvoice.app first; ships WITH the yap-lic-02/04 build',
  preflight: `
    ( M=0; grep -q 'pub const PURCHASE_URL: &str = "https://drivia\\.consulting/yap"' desktop/src-tauri/src/license.rs || M=1; GH_PAGER=cat GH_PROMPT_DISABLED=1 gh api repos/wilsonguenther-dev/$(printf drivia)-consulting/contents/scripts/stripe/ensure-yap-live.mjs --silent >/dev/null 2>&1 || M=1; test "$(curl -s -o /dev/null -w '%{http_code}' https://drivia.$(printf consulting)/yap)" = 200 || M=1; if [ "$M" = 1 ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-lic-04-ui-states-and-buy (PURCHASE_URL) must be on main and drivia-consulting dc-yap-03 + dc-yap-04 must be live (/yap answers 200, ensure-yap-live.mjs on its main) — land them, then the build pass again"; exit 0; fi; ! git grep -qnE 'buy.stripe.com|refunded licenses|refunded-licenses|no login, no server|No sign-in, no email' -- site/dist PRIVACY.md TERMS.md && grep -q 'drivia\\.consulting/yap' site/dist/index.html && grep -q 'https://yapvoice.app/updates/latest.json' desktop/src-tauri/tauri.conf.json )
  `,
  spec: `
    Panel 2026-10-19: APPROVED-WITH-CHANGES — ships in the SAME launch as the yap-lic-02/04 build; scope adds site/dist/terms.html;
    carries the copy diffs below; proves yapvoice.app/updates/latest.json answers 200 before the endpoint swap. Decision 5 = approved.
    DEPENDS: yap-lic-04 ON MAIN, and drivia-consulting dc-yap-03 + dc-yap-04 LIVE (preflight).
    ONE LAUNCH (Panel, phases 2 + 3 merged): the new app build (yap-lic-02/04) is HELD OFF THE UPDATER until dc-yap-03 and dc-yap-04 are
    live on drivia[.]consulting AND this item's copy is merged; this item's site deploy and that build's updater publish go out together.
    Never publish the site copy before the build, nor the build before the copy. Checkout itself stays closed until Wilson's flip
    (dc-yap-01 / docs/runbooks/YAP-GO-LIVE.md); the copy is true either way.

    COPY DIFFS (sense fixed by the panel; final words are the builder's, in Wilson's voice, no bullets in prose)
      site/dist/index.html:72        "Account: None" -> "Account: email, only to buy"
      site/dist/index.html:180-181   "No account — No sign-in, no email, no profile." -> "No account to use it. Buying takes one email-code
                                     sign-in; that email is your account."
      site/dist/index.html:201       "no login, no server … the one call licensing makes is downloading a public list of refunded
                                     licenses" -> "Buy with an email code at drivia[.]consulting. Activate once with your key; Yap re-checks it
                                     when online and keeps working up to 60 days offline."   (if yap-lic-00 already removed the
                                     refund-list sentence, rewrite what is there to this sense)
      site/dist/index.html:204       network list ends "…a background download of the public refunded-licenses list" -> "…license
                                     activation and a periodic re-check with drivia[.]consulting"
      site/dist/index.html:229       href buy.stripe.com/… -> href="https://drivia[.]consulting/yap"
      site/dist/index.html:166       "no account" about transcription: UNCHANGED (still true for using the app)
      site/dist/privacy.html:51 and PRIVACY.md:34   revocation-check paragraph -> "License activation and re-check: sends your key, a random
                                     install id, your Mac's name and the app/macOS version to drivia[.]consulting. We keep your email, a
                                     salted device hash, the Mac name and a coarse IP prefix. Nothing from your dictation."
                                     (Panel Security: device_name is often the user's real name, and the IP prefix is stored — disclose both.)
      site/dist/terms.html and TERMS.md   Forge-era license terms -> lifetime license, 3 Macs, 60-day offline operation, a full refund or a
                                     lost dispute revokes the license.
      desktop/src-tauri/tauri.conf.json:67   updater endpoints: https://yapvoice.app/updates/latest.json FIRST, yap-lemon.vercel.app second.
    UPDATER PRECONDITION (Panel Infra): site/dist has no updates/ folder today. Before swapping the endpoint order, prove
    curl -s -o /dev/null -w '%{http_code}' https://yapvoice.app/updates/latest.json answers 200 (paste it in the PR body); if it does not,
    add the updates/latest.json route to the site deploy in this item and prove it on a preview, never swap to a 404.`,
  acceptance: `
    ! git grep -qnE 'buy.stripe.com|refunded licenses|refunded-licenses|no login, no server|No sign-in, no email' -- site/dist PRIVACY.md TERMS.md
    ! git grep -qn '<dt>Account</dt><dd>None</dd>' -- site/dist/index.html
    grep -q '<dt>Telemetry</dt><dd>None</dd>' site/dist/index.html
    grep -q 'drivia\\.consulting/yap' site/dist/index.html
    grep -q 'drivia\\.consulting' site/dist/privacy.html
    grep -qi 'Mac' PRIVACY.md
    grep -qi '60' site/dist/terms.html
    grep -qi '60' TERMS.md
    test "$(grep -c 'https://yapvoice.app/updates/latest.json' desktop/src-tauri/tauri.conf.json)" = 1
    test "$(curl -s -o /dev/null -w '%{http_code}' https://yapvoice.app/updates/latest.json)" = 200
    (cd desktop && npm run loop:validate)
  `,
})
