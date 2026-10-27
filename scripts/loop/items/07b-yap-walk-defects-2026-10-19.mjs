// yap-walk-defects-2026-10-19 — the defects the live walk of main 3569ce4 + installed 0.9.1 found
// (Projects/Loop-Logs/yap-live-walk-2026-10-19.md, seat yap-live-walk, 03:14Z-03:22Z). Sorted after
// 07-yap-p0-broken so this lane runs after the P0 lane. D1 (Carbon chord deadlock), D4 (capture tail)
// and D6 (RSS) are fixed or answered in PR fix/chord-deadlock-and-take-length, not here.
//
// HAZARD (same as 07): no seat launches the GUI app, posts events into, signals or quits Wilson's
// running /Applications/Yap.app, or writes TCC on this machine. Every automated proof is a cargo test,
// a vitest, or a script self-test. SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.
// NO GUESSING: the root-cause probe goes into the PR body under "Root cause, proven" AND into
// docs/loop/evidence/<item id>.md, first line exactly "ROOT-CAUSE-PROBE <item id>".

ITEMS.push({
  id: 'yap-qa-hotkey-walk-binding', prompt: 'yap-qa-hotkey-walk-binding', branch: 'loop/yap-qa-hotkey-walk-configured-chord', gated: null,
  title: 'hotkey-walk presses the CONFIGURED push-to-talk chord (fn, fn⌃, …) read from settings.json, has a dev-build flag, and the app shows a hint when fn is held without the binding\'s modifier',
  preflight: `
    grep -q 'pttBinding' scripts/qa/hotkey-walk.sh && grep -q -- '--dev-build' scripts/qa/hotkey-walk.sh && grep -q 'FN_WITHOUT_MODIFIER_HINT' desktop/src-tauri/src/ptt_macos.rs
  `,
  spec: `
    Panel: not required — FIX (walk D2, P1).
    Root cause (walk): scripts/qa/fn-post.swift "down" sets only maskSecondaryFn and HOLD_STEPS in
    scripts/qa/hotkey-walk.sh assumes binding "fn"; Wilson's binding (and the default) is
    pttBinding: "fn_control", so the walk's fn-only post never matches and the walk FAILS on a
    working app. Product side: the UI says "Hold fn⌃"; a user holding fn alone gets nothing and no
    reason.
    DO 1 — hotkey-walk.sh reads pttBinding from Yap's settings.json (same data dir the app uses;
    --settings <path> override for tests) and posts the matching chord through fn-post.swift (ctrl
    down, then fn down; release in reverse). Unknown binding -> refuse with a named error, post
    nothing. --self-test covers fn, fn_control and an unknown value against fixture settings files
    and prints one line per case "binding=<value> chord=<keys>".
    DO 2 — --dev-build flag for an unbundled dev binary (matches the process by its target path
    instead of the bundle), replacing the YAP_QA_FAKE_PIDS workaround for live dev walks
    (YAP_QA_FAKE_PIDS stays self-test-only).
    DO 3 — ptt_macos: when fn goes down and up (or is held > 400 ms) WITHOUT the binding's modifier,
    emit one hint event (constant FN_WITHOUT_MODIFIER_HINT, e.g. "Hold fn⌃ — fn alone does nothing
    with this binding"), throttled to once per 30 s, shown on the pill. Pure decision function
    fn_hint_due(binding, fn_down, ctrl_down, held_ms, last_hint_ms, now_ms) -> bool with unit tests.
  `,
  acceptance: `
    bash scripts/qa/hotkey-walk.sh --self-test > desktop/target/qa-walk-binding.txt 2>&1
    grep -q '^binding=fn_control chord=ctrl+fn$' desktop/target/qa-walk-binding.txt
    grep -q '^binding=fn chord=fn$' desktop/target/qa-walk-binding.txt
    grep -q 'unknown binding' desktop/target/qa-walk-binding.txt
    shellcheck scripts/qa/hotkey-walk.sh
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib fn_hint_due) > desktop/target/qa-fn-hint.txt 2>&1
    grep -Eq 'test result: ok. [1-9][0-9]* passed' desktop/target/qa-fn-hint.txt
    test -s docs/loop/evidence/yap-qa-hotkey-walk-binding.md
    head -1 docs/loop/evidence/yap-qa-hotkey-walk-binding.md | grep -qx 'ROOT-CAUSE-PROBE yap-qa-hotkey-walk-binding'
  `,
})

ITEMS.push({
  id: 'yap-lic-00-kill-forge-fetch', prompt: 'yap-lic-00-kill-forge-fetch', branch: 'loop/yap-lic-00-kill-forge-revocation-fetch', gated: null,
  title: 'SECURITY: delete the launch-time revocation GET to the dead Forge sslip.io host entirely; no licensing network call at all until the new client lands',
  preflight: `
    ! git grep -qnE 'sslip\\.io|forge\\.87|87-99-149-214' -- desktop/src-tauri/src desktop/src site docs && ! git grep -qnE 'REVOCATION_URL|ISSUER_HOST|fetch_revocations|spawn_revocation_refresh' -- desktop/src-tauri/src && grep -q 'launch_license_init_makes_no_network_call' desktop/src-tauri/src/license.rs && ! grep -qiE 'revocation (check|list|refresh|download)|refunded or charged back' PRIVACY.md
  `,
  spec: `
    Panel: not required — FIX, class SECURITY (walk D3; supersedes the earlier yap-license-revocation-dead-host
    draft). Spec source: Projects/Yap/licensing-migration-plan-2026-10-19.md "## Items (draft, not yet filed)"
    -> yap-lic-00.
    Root cause (walk yap.log 2026-10-19T03:19:57Z): "license: revocation refresh skipped (error sending request
    for url (https://forge.87-99-149-214.sslip.io/v1/yap/revoked.json))". Every launch (and every activation)
    sends a GET to the decommissioned Forge box: license.rs ISSUER_HOST / REVOCATION_URL (:151-156) and the
    fetch at :1467-1479; callers lib.rs spawn_revocation_refresh (:5672 in activate_license, :5724-5736 the fn,
    :6891-6895 at launch). A dead sslip.io name that anybody can later answer for is a licensing input the app
    trusts — security class, not cosmetics.
    DO — remove REVOCATION_URL, ISSUER_HOST, fetch_revocations and spawn_revocation_refresh and both call
    sites. NO replacement endpoint: licensing moves to the consulting app's existing /api/v1/licensing (sibling repo) in later
    items yap-lic-01..06 — do NOT implement any of those here. Keep READING any cached revoked_kids, so an
    install that has a cached list behaves exactly as before. Local licence verification (Ed25519 claims,
    trial, grace) is unchanged. Remove the host from every config and doc mention, and correct
    docs/ARCHITECTURE-AUDIT-2026-09-26.md §9 ("harmless no-op" was wrong: it was a live GET every launch).
    PRIVACY.md (repo root, outside the grep scope above, so it is named here): delete list item 3 "The
    revocation check" from "The only times Yap uses the network" — the list becomes the model download and
    the update check, TWO calls — and say plainly that the licensing path makes no network call: a licence
    is verified on the Mac against the compiled-in public key. The acceptance refuses PRIVACY.md text
    matching revocation (check|list|refresh|download) or "refunded or charged back", so do not phrase the
    removal as "no longer downloads a revocation list". PRIV-A (30-y6) pins the same two call sites.
    Legitimate unrelated "forge" words stay: forget/forged/forgeable, and the issuer-repo citations
    "drivia-forge" and the signing-key path.
    Tests: (1) launch_license_init_makes_no_network_call — the launch-time licence init takes an injected
    transport/client; a recording fake asserts zero requests across init + status + a cached revoked_kids load.
    (2) license_rs_has_no_http_client — reads license.rs via include_str! and fails on "sslip.io", "reqwest",
    "ureq" or "http://"/"https://" outside the PAYMENT_LINK_URL constant. (3) existing licence tests green.
    Can ship alone in the next release.
  `,
  acceptance: `
    ! git grep -nE 'sslip\\.io|forge\\.87|87-99-149-214' -- desktop/src-tauri/src desktop/src site docs
    ! git grep -nE 'REVOCATION_URL|ISSUER_HOST|fetch_revocations|spawn_revocation_refresh' -- desktop/src-tauri/src desktop/src
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib license) > desktop/target/qa-lic00.txt 2>&1
    grep -q 'launch_license_init_makes_no_network_call ... ok' desktop/target/qa-lic00.txt
    grep -q 'license_rs_has_no_http_client ... ok' desktop/target/qa-lic00.txt
    grep -Eq 'test result: ok. [1-9][0-9]* passed' desktop/target/qa-lic00.txt
    (cd desktop/src-tauri && cargo test --features custom-protocol --test license_gate)
    ! grep -niE 'revocation (check|list|refresh|download)|refunded or charged back' PRIVACY.md
    test -s docs/loop/evidence/yap-lic-00-kill-forge-fetch.md
    head -1 docs/loop/evidence/yap-lic-00-kill-forge-fetch.md | grep -qx 'ROOT-CAUSE-PROBE yap-lic-00-kill-forge-fetch'
    (cd desktop && npm run loop:validate)
  `,
})

ITEMS.push({
  id: 'yap-dev-perm-probe-attribution', prompt: 'yap-dev-perm-probe-attribution', branch: 'loop/yap-dev-perm-probe-host-terminal', gated: null,
  title: 'A dev build\'s permission probe says "dev build: permissions are the host terminal\'s" instead of showing the launching terminal\'s TCC grants as Yap\'s green rows',
  preflight: `
    grep -q 'pub fn running_as_bundle' desktop/src-tauri/src/perm_probe.rs && grep -q 'DevHostAttributed' desktop/src-tauri/src/perm_probe.rs
  `,
  spec: `
    Panel: not required — FIX (walk D5, P2).
    Root cause (walk step 2c): the dev binary is ad hoc signed and unbundled, so TCC attributes it to
    the RESPONSIBLE process (the launching terminal) and answered accessibility=true
    input_monitoring=true — the terminal's grants, shown as Yap's.
    DO — perm_probe gets pub fn running_as_bundle(exe_path, bundle_id_of_main_bundle,
    code_signature) -> bool (pure, unit-tested: /Applications/Yap.app/Contents/MacOS/wilson-voice with
    the Developer ID -> true; desktop/target/debug/wilson-voice ad hoc -> false). When false,
    probe_all marks every service ProbeState::DevHostAttributed (new variant) and the Permissions
    screen shows ONE line "dev build: permissions are the host terminal's" with no green rows and no
    Repair button (Repair would reset the wrong identity). The installed build is unchanged.
    Frontend: a vitest for the Permissions screen rendering DevHostAttributed.
  `,
  acceptance: `
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib running_as_bundle) > desktop/target/qa-dev-attrib.txt 2>&1
    grep -Eq 'test result: ok. [1-9][0-9]* passed' desktop/target/qa-dev-attrib.txt
    grep -rq "dev build: permissions are the host terminal's" desktop/src
    (cd desktop && npx vitest run --reporter=dot) 
    test -s docs/loop/evidence/yap-dev-perm-probe-attribution.md
    head -1 docs/loop/evidence/yap-dev-perm-probe-attribution.md | grep -qx 'ROOT-CAUSE-PROBE yap-dev-perm-probe-attribution'
  `,
})

ITEMS.push({
  id: 'loop-dependency-marker-stdout-only', prompt: 'loop-dependency-marker-stdout-only', branch: 'loop/harness-dependency-marker-executed-output-only', gated: null,
  title: 'The harness\'s dependency hold matches DEPENDENCY-NOT-ON-MAIN only as a line the EXECUTED preflight printed, never anywhere in the runner seat\'s JSON',
  preflight: `
    grep -q 'dependencyMarkerPrinted' scripts/loop/template.mjs && grep -q 'dependencyMarkerPrinted' scripts/loop/dependency-marker.test.mjs
  `,
  spec: `
    Panel: not required — FIX (walk D7, harness).
    Root cause: on main 3569ce4 the harness recorded yap-p0-perm-repair as "skipped: dependency not
    on main" although #228 is merged and its preflight passed. Every item preflight already assembles
    the marker at run time ("DEPENDENCY-NOT-ON-$(printf MAIN)"; build.mjs now REFUSES a literal one),
    but template.mjs runBuild tests /DEPENDENCY-NOT-ON-MAIN/ against asText(pre) — the WHOLE runner
    result JSON, including any free text the seat wrote (a note such as "no DEPENDENCY-NOT-ON-MAIN
    printed" trips it). Prove which field carried it from that run's log before changing code.
    DO — export a pure dependencyMarkerPrinted(pre) that looks only at the executed commands' captured
    stdout and matches /^DEPENDENCY-NOT-ON-MAIN:/m, and use it in runBuild. Tests (node --test, in
    review-gate.test.mjs or a sibling): marker line in stdout -> true; marker only in a notes/summary
    field -> false; marker text inside the echoed command string -> false.
  `,
  acceptance: `
    grep -q 'dependencyMarkerPrinted(pre)' scripts/loop/template.mjs
    ! grep -q '/DEPENDENCY-NOT-ON-MAIN/.test(asText(pre))' scripts/loop/template.mjs
    mkdir -p desktop/target && node --test --test-reporter=tap 'scripts/loop/*.test.mjs' > desktop/target/qa-dep-marker.txt 2>&1
    grep -q '^# fail 0$' desktop/target/qa-dep-marker.txt
    grep -Eq '^# pass [1-9][0-9]*$' desktop/target/qa-dep-marker.txt
    (cd desktop && npm run loop:build && npm run loop:validate)
  `,
})
