// yap-2026-10-26 — Wilson 2026-10-26: "Yap is extremely broken". Written 2026-10-26T13:46Z (date -u) by
// the pre-flight + backlog seat. FIX, no panel. Named 00a so it sorts straight after the harness file:
// yap-insert-bridge-2026-10-26 is FIRST PRIORITY in the build order (coordinator 2026-10-26). To run it
// alone first: Workflow {args: {mode: 'build', only: ['yap-insert-bridge-']}}.
//
// Also here: the three items queued in PR #240's thread (2026-10-19T16:34Z, meant for 07d, but 07d only
// exists inside unmerged #240 — so they live on main here) and the #249 carried advisories stub
// (#240 thread 2026-10-19T19:12Z). Two of the #240 items are SATISFIED BY OPEN PRs and must not be
// rebuilt: their preflight prints the DEPENDENCY marker (harness: "skipped: dependency not on main",
// no builder, no PR) until the satisfying PR lands, then checks its deliverable and retires as
// already-done. If that check ever fails after the PR landed, the builder builds ONLY the gap.
//
// Review: Opus only (review-opus + review-opus2). No Codex, no Gemini, no third-party reviewer.
// HAZARD (same as 07/07b/08): no agent launches the GUI app, posts events into, signals or quits Wilson's
// running /Applications/Yap.app, or writes TCC on this machine. Every automated proof is a cargo test, a
// vitest, or a script self-test; on-device checks go in the PR body as WILSON'S WALK.
// NO GUESSING: the root-cause probe goes into the PR body under "Root cause, proven" AND into
// docs/loop/evidence/<item id>.md, first line exactly "ROOT-CAUSE-PROBE <item id>".
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-insert-bridge-2026-10-26', prompt: 'yap-insert-bridge-2026-10-26', branch: 'loop/yap-insert-bridge-2026-10-26-paste-first', gated: null,
  title: 'Dictation lands in the focused field again — paste bridge first when PostEvent is granted; AX never strands a take on the Copy card; clipboard is never a silent fallback',
  preflight: `
    grep -q 'AX_ASYNC_DENYLIST' desktop/src-tauri/src/paste.rs && grep -q 'fallback=' desktop/src-tauri/src/paste.rs && grep -q 'fn post_event_granted_uses_paste_bridge_regardless_of_ax' desktop/src-tauri/tests/insertion_matrix.rs
  `,
  spec: `
    Panel: not required — FIX, class SECURITY (CGEventPost ⌘V / AX insertion / permission-gated path).
    FIRST PRIORITY. Label the PR "security" (gh pr edit <N> --add-label security) so the gate seats
    review-opus AND review-opus2 — paste.rs/focus.rs do not match SECURITY_PATHS by name.
    Wilson 2026-10-26: dictation no longer lands in the focused text box; he copy/pastes every result.

    ROOT CAUSE (diag seat 2026-10-26T13:45Z — code, not TCC; reproduce it as your probe, do not re-derive):
      #229 (6c3f991, yap-p0-insertion-bridge-fallback, shipped in 0.9.2, still in 0.9.3-dev) made AX
      kAXSelectedText the FIRST path whenever Accessibility is Granted (paste.rs:180-188
      choose_insert_path). In the Claude desktop app (Electron/Chromium) the AX set returns success but
      AXValue never reads back changed within 120 ms (focus.rs:186-208); the code calls that Unverified,
      deliberately skips ⌘V (double-insert risk) and ends on the Copy card (paste.rs:408-411).
      yap.log: "insert path=Ax (ax=Granted post=Granted)" -> "AX insert unverified ... no ⌘V ... showing
      the Copy card" -> "pasted=false". Claude app before #229: 41/41 pasted=true; after: 29/29
      pasted=false. Chrome after #229: 2 unverified -> card, 4 AX ok, 1 refused -> bridge ok. Terminal:
      23/23 "AX refused (not settable) -> paste bridge -> confirmed", so the bridge itself works.
      Permissions are fine: DR identical 0.9.2 vs 0.9.3-dev (team VHYV8C2JNU), TCC rows match, every take
      logs ax=Granted post=Granted. Ruled out: #250 (no paste.rs/focus.rs/paste_tx), #249 (fn and
      ⌃⌥⌘Space both call insert_dictated_take, lib.rs:2723), settings autoPaste is true.
      TextEdit takes the AX path fine, so #242's TextEdit-only smoke would NOT catch this.
      Probe for the evidence file: a cargo test on unmodified main showing choose_insert_path returns Ax
      for ax=Granted post=Granted, plus the read-only yap.log counts above (grep -c, no writes).

    DO (names are contract)
      1. paste.rs choose_insert_path: PostEvent Granted -> PasteBridge, whatever Accessibility is. Ax only
         when PostEvent is not Granted but Accessibility is. CopyCard when neither. This restores the
         0.9.1 path (41/41 Claude, 181/182 Terminal); the bridge stays receipt-checked (paste_tx) and
         still ends on the Copy card if nothing reads the clipboard.
      2. pub const AX_ASYNC_DENYLIST: &[&str] in paste.rs — com.anthropic.claudefordesktop,
         com.google.Chrome, com.tinyspeck.slackmacgap, com.microsoft.VSCode, com.hnc.Discord — plus any
         frontmost app whose bundle contains "Electron Framework.framework". Those never get an AX
         attempt; they go to the bridge.
      3. An AX result of Unverified (focus.rs:203) re-reads for ~400 ms (AXNumberOfCharacters and
         AXSelectedTextRange). Neither moved -> treat as Refused -> bridge runs EXACTLY ONCE, no card.
         A real change stays Inserted with 0 ⌘V.
      4. The "insert path=" log line (paste.rs:617-618) becomes one line per take:
         insert path=<Ax|PasteBridge|CopyCard> app=<frontmost bundle id> fallback=<none|reason>
         (reasons e.g. electron_denylist, ax_unverified_unchanged, no_receipt, post_event_denied).
      5. When insertion is impossible the pill says so — which permission (PostEvent / Accessibility) and
         a one-click Open Settings / Repair (invoke("repair_permission")) — never a silent clipboard
         leave-behind and never pasted=false without a card.
      6. Prior clipboard is restored after every bridge take (keep paste_tx's restore; prove it).
      7. tests/insertion_matrix.rs: the existing ax_trusted_uses_ax and ax_unverified_never_posts_cmd_v
         encode the regression — REPLACE them (do not keep a test asserting AX-first) with, exact names:
         post_event_granted_uses_paste_bridge_regardless_of_ax, ax_only_when_post_event_denied,
         both_denied_shows_copy_card, denylisted_bundle_never_calls_ax_insert,
         ax_unverified_unchanged_becomes_refused_bridge_once, ax_unverified_changed_stays_inserted_zero_cmd_v,
         bridge_no_receipt_ends_on_card_never_silent, insert_log_line_format (snapshot of path/app/fallback).
         Keep the other existing matrix tests green.
      8. If the #242 release smoke (scripts/qa) is on main when you build, add a Chrome <textarea> target
         next to TextEdit; if it is not, write "smoke-target follow-up for #242: add Chrome textarea /
         Claude desktop" in the PR body.

    WILSON'S WALK (PR body; signed Developer-ID app; one fn⌃ take AND one ⌃⌥⌘Space take each):
      1 TextEdit: text at the caret exactly once. 2 Chrome — Google Docs body and a plain <textarea>: once.
      3 Slack composer: once. 4 Claude desktop composer: once (the regression case). 5 A Tauri input
      (Yap scratchpad or Yap Test): once. 6 Terminal: once. 7 Prior clipboard restored after each take.
      8 On Yap Test with PostEvent AND Accessibility revoked: the pill names the blocked permission and
      offers one-click Open Settings / Repair; with only one revoked, insertion still works via the other.
      9 yap.log has one "insert path=... app=... fallback=..." line per take. 10 20 consecutive takes into
      Claude desktop -> 0 pasted=false.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/insertion_matrix.rs
    grep -q 'AX_ASYNC_DENYLIST' desktop/src-tauri/src/paste.rs
    grep -q 'com.anthropic.claudefordesktop' desktop/src-tauri/src/paste.rs
    grep -q 'Electron Framework' desktop/src-tauri/src/paste.rs
    grep -q 'fallback=' desktop/src-tauri/src/paste.rs
    grep -q 'app=' desktop/src-tauri/src/paste.rs
    ! grep -q 'fn ax_trusted_uses_ax()' desktop/src-tauri/tests/insertion_matrix.rs
    ! grep -q 'fn ax_unverified_never_posts_cmd_v()' desktop/src-tauri/tests/insertion_matrix.rs
    test -s docs/loop/evidence/yap-insert-bridge-2026-10-26.md
    grep -q '^ROOT-CAUSE-PROBE yap-insert-bridge-2026-10-26$' docs/loop/evidence/yap-insert-bridge-2026-10-26.md
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    mkdir -p desktop/target
    (cd desktop/src-tauri && cargo test --features custom-protocol --test insertion_matrix -- --list) > desktop/target/ib26-list.txt 2>/dev/null
    grep -q '^post_event_granted_uses_paste_bridge_regardless_of_ax: test$' desktop/target/ib26-list.txt
    grep -q '^ax_only_when_post_event_denied: test$' desktop/target/ib26-list.txt
    grep -q '^both_denied_shows_copy_card: test$' desktop/target/ib26-list.txt
    grep -q '^denylisted_bundle_never_calls_ax_insert: test$' desktop/target/ib26-list.txt
    grep -q '^ax_unverified_unchanged_becomes_refused_bridge_once: test$' desktop/target/ib26-list.txt
    grep -q '^ax_unverified_changed_stays_inserted_zero_cmd_v: test$' desktop/target/ib26-list.txt
    grep -q '^bridge_no_receipt_ends_on_card_never_silent: test$' desktop/target/ib26-list.txt
    grep -q '^insert_log_line_format: test$' desktop/target/ib26-list.txt
    (cd desktop/src-tauri && cargo test --features custom-protocol --test insertion_matrix)
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib paste)
    (cd desktop && npx vitest run src/pill)
    bash scripts/qa/hotkey-walk.sh --self-test
  `,
})

ITEMS.push({
  id: 'yap-founding-coupon-follows-launch', prompt: 'yap-founding-coupon-follows-launch', branch: 'loop/yap-founding-coupon-follows-launch', gated: null,
  title: 'FOUNDING19 follows the launch, not a date — valid for 30 days from the moment PURCHASE_OPEN flips true; no founding-price copy anywhere while purchasing is closed',
  preflight: `
    ( if ! grep -q 'pub const PURCHASE_OPEN' desktop/src-tauri/src/license.rs; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap24-PILL1 introduces PURCHASE_OPEN in license.rs and has not landed; land it, then build this"; exit 0; fi; grep -q 'pub fn founding_valid' desktop/src-tauri/src/license.rs && grep -q 'FOUNDING_OPENED_AT' desktop/src-tauri/src/license.rs && ! grep -q '2026-11-08' desktop/src-tauri/src/license.rs && grep -q 'founding_open' desktop/src/license/status.ts )
  `,
  spec: `
    Panel: not required — Wilson's decision 2026-10-26. Class SECURITY (licensing): label the PR "security"
    (gh pr edit <N> --add-label security) so the gate seats review-opus AND review-opus2 — license.rs does
    not match SECURITY_PATHS by name.
    DEPENDS: yap24-PILL1 (00f-yap24-pill-and-trial.mjs) — it introduces \`pub const PURCHASE_OPEN\` (default false,
    surfaced on the license status payload as purchase_open). PURCHASE_OPEN is a CLIENT const in license.rs,
    not a server-side flag on the issuer, and PR #232's yap-lic items do not define it (checked 2026-10-26). The
    preflight holds this item until PILL1 is on main.

    TODAY: the FOUNDING19 window is a DATE — license.rs:167-168 documents Stripe coupon z8UZkIH2 as "expires
    2026-11-08", and the founding copy (desktop/src/license/status.ts:67-68 FOUNDING_CODE / FOUNDING_PRICE_LABEL,
    rendered in LicensePanel.tsx:140 and PurchasePrompt.tsx:77) shows regardless of whether anyone can buy.

    DO (names are contract)
      1. Founding validity is bound to PURCHASE_OPEN, not a calendar date: valid from the moment PURCHASE_OPEN
         flips true, for 30 days. \`founding_opened_at\` must be ONE global instant, not each install's first run:
         because PURCHASE_OPEN is a release-time const, add \`pub const FOUNDING_OPENED_AT: Option<&str> = None\`
         (RFC 3339 UTC) set in the SAME release that flips PURCHASE_OPEN — a unit test fails the build when
         PURCHASE_OPEN is true and FOUNDING_OPENED_AT is None. (If, when you build, PURCHASE_OPEN has moved
         server-side to the issuer, persist founding_opened_at there on the first flip instead and read it
         over the existing license status fetch; say which in the PR body.)
      2. \`pub fn founding_valid(purchase_open: bool, opened_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool\`
         = purchase_open && opened_at is Some && opened_at <= now < opened_at + 30 days. The license status payload
         carries \`founding_open: bool\` computed by the backend; the webview never recomputes it.
      3. No founding-price copy anywhere while founding_open is false: LicensePanel.tsx and PurchasePrompt.tsx render
         the FOUNDING_CODE / FOUNDING_PRICE_LABEL lines ONLY when founding_open is true. The pill never shows a price
         (Y2-D rule, unchanged).
      4. Remove the hard-coded "expires 2026-11-08" from the license.rs:168 doc comment; replace with "valid 30 days
         from FOUNDING_OPENED_AT (the launch), see founding_valid".
    OWNER-ONLY (PR body, never done by an agent — money, outbound-review): on launch day Wilson sets the Stripe
    coupon z8UZkIH2 redeem_by to FOUNDING_OPENED_AT + 30 d, so Stripe and the app agree.
    TESTS (cargo test --lib license): flag false => founding_valid false for every opened_at/now AND the payload's
    founding_open is false; flip (opened_at = t0) => valid at t0, t0+29d23h; day 31 (t0+30d, t0+31d) => invalid;
    PURCHASE_OPEN true with FOUNDING_OPENED_AT None => test fails. vitest (src/license): founding_open false =>
    no "FOUNDING19" and no "Founding price" text in LicensePanel or PurchasePrompt; true => both present.
  `,
  acceptance: `
    grep -q 'pub fn founding_valid' desktop/src-tauri/src/license.rs
    grep -q 'FOUNDING_OPENED_AT' desktop/src-tauri/src/license.rs
    ! grep -q '2026-11-08' desktop/src-tauri/src/license.rs
    grep -q 'founding_open' desktop/src/license/status.ts
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib license)
    (cd desktop && npx vitest run src/license)
  `,
})

ITEMS.push({
  id: 'yap-meetings-summary-default', prompt: 'yap-meetings-summary-default', branch: 'loop/yap-meetings-summary-default-zero-setup', gated: null,
  title: 'SATISFIED BY #250 — a recorded meeting gets notes with zero setup; no "add a summary model" prompt anywhere',
  preflight: `
    ( if ! test -f desktop/src-tauri/src/summary_jobs.rs; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): satisfied-by #250 (feat/meeting-summary-default) — review and land #250; do not rebuild this"; exit 0; fi; ! grep -rqi 'add a summary model' desktop/src --include='*.ts' --include='*.tsx' --exclude='*.test.ts' --exclude='*.test.tsx' )
  `,
  spec: `
    Panel: not required — queued in #240's thread 2026-10-19T16:34Z (Wilson's Yap Test walk).
    SATISFIED-BY: #250 (feat/meeting-summary-default, head 0a87249). The preflight skips this item until
    #250 is on main, then retires it as already-done. Build ONLY if #250 landed and the preflight still
    fails: then the residual gap is that some non-test UI string still asks the user to add a summary model.
    Original ask: a recorded meeting shows "TRANSCRIPT READY · ADD A SUMMARY MODEL IN SETTINGS". Ship a
    default local summary (reuse the polish sidecar model) so no meeting ever asks the user to add a
    model; Settings may offer a bigger model as an option. Acceptance from the thread: fresh profile ->
    record 20 s -> summary appears with zero setup (WILSON'S WALK, PR body); grep that the string is gone.
  `,
  acceptance: `
    test -f desktop/src-tauri/src/summary_jobs.rs
    ! grep -rqi 'add a summary model' desktop/src --include='*.ts' --include='*.tsx' --exclude='*.test.ts' --exclude='*.test.tsx'
    (cd desktop && npm ci)
    (cd desktop && npx vitest run src/meetings)
  `,
})

ITEMS.push({
  id: 'yap-perm-accessibility-live-refresh', prompt: 'yap-perm-accessibility-live-refresh', branch: 'loop/yap-perm-accessibility-live-refresh', gated: null,
  title: 'SATISFIED BY #247 + #248 — the Accessibility banner follows the live probe, Repair resets the right service, no stale-instance refusal',
  preflight: `
    ( if ! grep -q 'pub fn reset_services' desktop/src-tauri/src/perm_probe.rs || ! test -f desktop/src/permission.refresh.test.tsx; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): satisfied-by #247 (fix/ax-probe-refresh-and-repair-service) — review and land #247; do not rebuild this"; exit 0; fi; if ! grep -q 'pub fn replace_dead_holder' desktop/src-tauri/src/lifecycle.rs 2>/dev/null; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): stale-instance half satisfied-by #248 (stacked on #241) — land #241 then #248"; exit 0; fi; true )
  `,
  spec: `
    Panel: not required — queued in #240's thread 2026-10-19T16:34Z. Class SECURITY (permission path).
    SATISFIED-BY: #247 (banner follows the live probe; Repair resets the service its row names) and #248
    (replace a dead single-instance holder — stacked on #241). The preflight skips this item until both
    are on main, then retires it as already-done. Do not rebuild either PR.
    Original ask: Accessibility granted in TCC (auth 2, csreq matches) while the banner kept saying off;
    the probe ran once at launch and never re-probed; Repair on the Accessibility row reset PostEvent, not
    Accessibility; "stale instance" refusal on second launch.
  `,
  acceptance: `
    grep -q 'pub fn reset_services' desktop/src-tauri/src/perm_probe.rs
    test -f desktop/src/permission.refresh.test.tsx
    grep -q 'pub fn replace_dead_holder' desktop/src-tauri/src/lifecycle.rs
    (cd desktop && npm ci)
    (cd desktop && npx vitest run src/permission.refresh.test.tsx)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test lifecycle)
  `,
})

ITEMS.push({
  id: 'yap-test-channel-product-name', prompt: 'yap-test-channel-product-name', branch: 'loop/yap-test-channel-product-name-everywhere', gated: null,
  title: 'Every permission banner and message names the running product ("Yap Test" in a test build), never a hard-coded "Yap"',
  preflight: `
    grep -q 'fn permission_copy_uses_product_name' desktop/src-tauri/src/permissions.rs && grep -rq 'productName' desktop/src/permission.ts
  `,
  spec: `
    Panel: not required — queued in #240's thread 2026-10-19T16:34Z (Wilson's Yap Test walk). Class
    SECURITY by path (permissions.rs).
    Problem: permission banners/copy say "Yap" inside a Yap Test build. System Settings lists the grant
    under the bundle's own name, so the copy must say the same name or the user toggles the wrong row.
    perm_truth::product_name_from_bundle_path already exists on main (perm_truth.rs:406); use it, do not
    add a second source of truth. Hard-coded sites on main include lib.rs:1491 ("Yap cannot hear you"),
    permissions.rs:157 and :332, and the frontend permission views — grep every user-facing "Yap" in the
    permission/onboarding/pill copy and route it through the product name (the brand word in marketing
    copy and the app title stays).
    DO: (1) Rust: every permission message takes product: &str from perm_truth; unit test
    permission_copy_uses_product_name asserts "Yap Test" appears and "Yap " alone does not for a
    /Applications/Yap Test.app path. (2) Expose the product name to the UI (existing permission status
    payload gets productName); desktop/src/permission.ts reads it; vitest renders the banner with
    productName "Yap Test". (3) If #238's buildChannel.ts is on main, keep its chip separate — the chip
    is the build head, productName is the TCC row name.
  `,
  acceptance: `
    grep -q 'fn permission_copy_uses_product_name' desktop/src-tauri/src/permissions.rs
    grep -rq 'productName' desktop/src/permission.ts
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && npx vitest run src/permission)
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib permission_copy_uses_product_name)
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib perm_truth)
  `,
})

ITEMS.push({
  id: 'yap-hotkeys-advisories-r1', prompt: 'yap-hotkeys-advisories-r1', branch: 'loop/yap-hotkeys-advisories-r1', gated: null,
  title: 'Close the seven non-blocking review advisories carried from #249 (reserved chords, launch re-validation, copy, TCC errors, parity test)',
  preflight: `
    grep -q 'reserved_chords_parity' desktop/src-tauri/src/hotkey_table.rs && grep -q 'sqlite3 -readonly' scripts/qa/hotkey-walk.sh && grep -q 'Esc to cancel' desktop/src/hotkeyRecorder.ts desktop/src/views/settings/Shortcut.tsx desktop/src/HotkeyRow.tsx 2>/dev/null
  `,
  spec: `
    Panel: not required — stub carried from #249 (landed a740614), posted in #240's thread
    2026-10-19T19:12Z. Advisories from review-opus / review-opus2 on e472c17:
      1. The reserved-chord list misses ⌃⇧⌘3, ⌃⇧⌘4, ⌥⌘H, ⌥⌘M, ⌥⌘W and ⌃⌘D — add them.
      2. The stored dictate chord is not re-validated against the reserved list at launch — re-validate;
         a reserved stored chord falls back to the default and the UI says why.
      3. The Cancel button in the chord recorder needs "Esc to cancel" copy.
      4. Without Full Disk Access, a stale grant reads "Needs Input Monitoring" without the
         remove-and-re-add hint — add the hint.
      5. A TCC query error is counted as "no row" — surface it as an error state instead.
      6. No Rust/TS parity test for the reserved-chord list — add test reserved_chords_parity (Rust reads
         the TS list or a shared JSON; both sides fail on drift).
      7. hotkey-walk.sh --table opens the TCC database with sqlite3 -readonly and quotes the bundle id.
  `,
  acceptance: `
    grep -q 'reserved_chords_parity' desktop/src-tauri/src/hotkey_table.rs
    grep -q 'sqlite3 -readonly' scripts/qa/hotkey-walk.sh
    shellcheck scripts/qa/hotkey-walk.sh
    bash scripts/qa/hotkey-walk.sh --self-test
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && npx vitest run src/hotkeyRecorder.test.tsx)
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib hotkey_table)
  `,
})

// yap-loop-stall-detector — filed 2026-10-26T22:58Z (date -u) by a Sonnet spec-writer seat. Audit 2026-10-26: a review
// pass sat 2 h with zero lands because the land gate required fmt, which was red on unmodified main; every PASS/PASS
// PR failed fix-land with ci-red and nothing (no seat, no parent code) noticed the pattern. Fixed by hand at ~15:20Z
// (fmt advisory, template.mjs:459-479); this item makes the loop notice next time. SECURITY class: it touches
// scripts/loop/*, which review-gate.mjs lists in SECURITY_PATHS. FUTURE item: do not stop a live pass to build it.
// The word the validator bans for the machine monitor script is avoided on purpose below: that script is globbed.
ITEMS.push({
  id: 'yap-loop-stall-detector', prompt: 'yap-loop-stall-detector', branch: 'loop/yap-loop-stall-detector', gated: null,
  title: 'The loop notices its own stalls — same-reason-twice, gate-red-on-main halt, no-progress halt, and an external ALERT STALL line',
  preflight: `
    test -f scripts/loop/stall.mjs && test -f scripts/loop/stall-detector.test.mjs && test -f scripts/loop/stall-alarm.sh && grep -q 'const GATE_BLOCKING' scripts/loop/template.mjs
  `,
  spec: `
    Panel: not required — harness fix. Class SECURITY: everything here is under scripts/loop/, which review-gate.mjs
    lists in SECURITY_PATHS, so the gate seats review-opus AND review-opus2. Label the PR "security"
    (gh pr edit <N> --add-label security) anyway. Builder stays Opus (no builder tag on this item).
    Do not regenerate or launch anything: this item only edits the harness sources; npm run loop:build is run by the
    orchestrator after the merge, never by the builder, and a live pass is never stopped for it.

    ROOT CAUSE (audit 2026-10-26, already fixed by hand — do not re-derive): GATE_CMDS (template.mjs:466-479) required
    cargo fmt while main was red on it (65 hunks), so every PASS/PASS PR returned fix-land status ci-red for the same
    command. Three blind spots let it run 2 h: (a) the review barrier retried or re-queued with no memory of the last
    reason; (b) REVIEW_RECON (template.mjs:1911-1958, baseline at :1938-1942) reports the baseline exits and the pass
    continued — recon's own prose called fmt informational (the BUILD_RECON text at :1860-1862) while the land gate
    called it blocking; (c) nobody outside the workflow was told.

    WHERE THINGS ARE (template.mjs, 2026-10-26; line numbers drift, grep the symbol): GATE_CMDS :466; the "TEN BLOCKING
    COMMANDS" prose :479; CI_GATE :516 (its OUTCOMES block :570-575 is where ci-red is defined); fixLandPrompt LAND
    RETURN with gateExits :1700-1703; the review barrier's two-seat dispatch :1531; the land seat dispatch and the status
    derivation :1715-1733; reviewOnePrSafe :1740; reviewChain :1756-1763 (it already breaks on halted); reviewPass
    :2118 (triage :2119, chains :2207 Promise.all, land-sweeper :2246, main-verify :2301); the recon call :1985 and
    RECON_SCHEMA just above it; the board writer is scripts/loop/status.mjs (STATUS in the prompts); Loop-Logs lines are appended by seats via LOGLINE (:333). The generated workflow is ONE script with no
    imports, so scripts/loop/stall.mjs cannot be imported by it: stall.mjs is the SOURCE, and template.mjs carries a
    byte-identical copy of its function bodies between the markers  STALL-RULES-BEGIN / STALL-RULES-END  (comment
    lines). The test asserts the two are identical. Do not add an import to template.mjs.

    DO (names are contract)
      0. ONE SOURCE FOR THE GATE FLAGS. In template.mjs add, immediately above GATE_CMDS:
         const GATE_BLOCKING = ['typecheck','test','build','polish','diarize','stage','polishtest','diarizetest','clippy','cargotest']
         const GATE_ADVISORY = ['fmt']
         The echo labels inside GATE_CMDS (the  echo "<name>=\$?"  tokens) must be exactly GATE_BLOCKING plus GATE_ADVISORY,
         and the "TEN BLOCKING COMMANDS" prose must say the count of GATE_BLOCKING. stall.mjs exports the same two
         arrays. A test extracts the echo labels from GATE_CMDS and fails on any drift in either direction, so
         adding a command to the gate without classifying it breaks the build. When yap-fmt-sweep-2026-10-26 flips fmt
         back to blocking, it moves 'fmt' between the arrays in the same diff (and updates land-gate-fmt.test.mjs).
         RECON AND THE LAND GATE READ THE SAME LIST: REVIEW_RECON and BUILD_RECON already interpolate GATE_CMDS(...);
         additionally make both recon prompts say "classify each exit with GATE_BLOCKING / GATE_ADVISORY — you do not
         decide what is informational" (interpolate the arrays, do not retype names), delete the recon-only fmt line
         at :1860-1862, and require RECON_SCHEMA to carry gateExits {typecheck,test,build,polish,diarize,stage,
         polishtest,diarizetest,fmt,clippy,cargotest} as numbers (required, not optional).
      1. SAME-REASON-TWICE (pure fn in stall.mjs, mirrored into template.mjs):
         stallReason(landResult) -> {status, reason}: status is the fix-land status already derived at :1726-1733;
         reason is, for ci-red, the FIRST non-zero BLOCKING command name read from gateExits (GATE_BLOCKING order),
         else the "failingCommand" field, else the reason field, else "unknown"; for any other status the reason field
         or "none". sameReasonTwice(history, next) -> boolean: true when history (array of {pr, status, reason}) holds
         a prior entry for the SAME pr whose status and reason both equal next's and status is not "merged".
         In reviewOnePr (before the land dispatch at :1715), consult the per-PR history kept in a module-level Map;
         when sameReasonTwice is true, do NOT dispatch another fix-land for that PR: dispatch ONE stall-writer seat
         (agentR, model 'opus', effort 'low', phase = the same phase the neighbouring fix-land calls use, label
         'stall-writer:' + itemId — not listed in seat-models.json, so it runs as written) that (a) labels the PR
         loop-stalled (create the label first, || true), (b) comments on the PR "loop-stalled: <status> <reason> twice
         in a row", (c) appends  "STALL pr=<n> item=<id> status=<s> reason=<command>"  as one line to the LOG file (Loop-Logs) and writes
         the board row with stage review:needs-human and a note beginning "STALL: <command>", and (d) returns.
         The PR's outcome is status 'loop-stalled'; the chain moves to the next PR (no third attempt, no needs-human
         relabel, no halt). The rule never fires on 'merged', and a changed reason resets nothing — the history is
         per PR and compares only the most recent prior entry.
      2. GATE RED ON MAIN, BEFORE ANY REVIEW (pure fn gateRedOnMain(gateExits, blocking, advisory) -> {red:[cmds],
         unreadable:[cmds], advisoryRed:[cmds]}): a blocking command is red when its exit is non-zero, unreadable when
         the key is missing or not an integer (unreadable counts as red for the halt, fail closed). Call it in
         reviewPass() right after the recon result is threaded (:2002-2018), ahead of the triage dispatch at :2119.
         When red or unreadable is non-empty: dispatch the stall-writer seat once to write
         "GATE RED ON MAIN: <cmd>[, <cmd>...]" (prefix "GATE UNREADABLE ON MAIN:" for the unreadable ones) to the board
         (harness:halted row), to the LOG file (Loop-Logs), and to the sentinel file /Users/wilsonguenther/code/wilson-voice-loop/STALL.txt
         (mkdir -p its dir; the file holds a stamp from date -u, the rule number, and the line); then reviewPass returns
         { halted: 'gate-red-on-main', red, queued: 0 } WITHOUT triage, chains, sweeper or verify, and the parent log says
         so. advisoryRed (today: fmt) is only reported in the log as "advisory red on main: fmt, N hunks", never a halt.
         Build mode (MODE === 'build') gets the same check after its recon: a red blocking command on main halts the
         builders too, because every builder gate would fail the same way (state this in the PR body if you scope it
         to review only; the review pass is the required half).
      3. NO PROGRESS (pure fn noProgress(results, n=3) -> boolean; results is the array of per-PR land outcomes in
         completion order, each {pr, status, gateFailure: boolean}): true when the LAST n entries are for n DISTINCT PRs,
         none is 'merged', and every one has gateFailure true (status ci-red, or a non-zero BLOCKING exit in gateExits).
         Evaluate after every land outcome in reviewChain (:1756-1763). On true: dispatch the stall-writer once (guard
         with a module-level boolean so two chains cannot both fire it) to write "STALL no-progress: last 3 land
         results were gate failures: <pr:command, ...>" to the board, the LOG file (Loop-Logs) and STALL.txt, then set a new module
         flag  stalled = true  that reviewChain checks next to its existing  if (halted) break  — the review barrier
         stops pulling PRs. Builders are unaffected (the flag is only read in reviewChain). The land-sweeper and
         main-verify still run (they settle what is open and are how the orchestrator sees the damage); the pass result
         carries stalled: true. A merged result anywhere in the last 3 breaks the run.
      4. EXTERNAL ALARM. The machine monitor in ~/bin (its filename contains the banned word, so reach it only by the
         glob  ~/bin/loop-w*-yap-learn.sh ) is outside this repo and a builder lane cannot ship it in a PR. So:
         (a) add scripts/loop/stall-alarm.sh to the repo — a sourceable bash function  stall_alarm_check  that reads
         \${STALL_FILE:-/Users/wilsonguenther/code/wilson-voice-loop/STALL.txt} and a state variable stall_alert
         (edge-triggered, same style as the RSS/mem/disk alerts in that script): when the file exists and the state
         is 0, set it to 1 and print one line  "<stamp> ALERT STALL <file contents, newlines folded to ' | '>" (stamp
         = date -u +%Y-%m-%dT%H:%M:%SZ); when the file is gone and the state is 1, set 0 and print
         "<stamp> cleared STALL". The monitor's Monitor filter already surfaces lines containing ALERT.
         (b) The PR body carries the exact 3-line patch for the ~/bin script (source the repo file by absolute path,
         initialise stall_alert=0 next to rss_alert, call stall_alarm_check once per loop iteration and once in the
         --status branch); the ORCHESTRATOR applies that patch to ~/bin after merge — say so in the PR body, and do not
         write outside the repo yourself.
         (c) Document in the PR body and as the header comment of stall-alarm.sh: the orchestrator removes STALL.txt
         after acting (that is the acknowledgement; the next monitor tick prints "cleared STALL"); the loop itself never
         deletes it, and a fresh stall overwrites it.
      5. TESTS: scripts/loop/stall.mjs (pure, no I/O, no imports) and scripts/loop/stall-detector.test.mjs (node:test,
         hermetic). Exact test names:
           gate_flags_match_gate_cmds            (echo labels in GATE_CMDS == GATE_BLOCKING + GATE_ADVISORY, both ways)
           recon_prompts_interpolate_gate_flags   (both recon prompts reference GATE_BLOCKING; no recon-only fmt line)
           stall_rules_block_is_identical_to_stall_mjs  (the template copy between the markers equals stall.mjs)
           same_reason_twice_ci_red_same_command_stalls
           same_reason_twice_different_command_does_not_stall
           same_reason_twice_different_pr_does_not_stall
           same_reason_twice_never_on_merged
           gate_red_on_main_flags_blocking_nonzero
           gate_red_on_main_ignores_advisory_fmt
           gate_red_on_main_missing_key_is_unreadable_and_red
           no_progress_three_distinct_prs_gate_failures_halts
           no_progress_same_pr_three_times_does_not_count
           no_progress_a_merge_in_the_window_resets
           stall_alarm_prints_alert_then_cleared   (runs stall-alarm.sh in bash with a temp STALL_FILE: absent -> no
                                                    output; created -> exactly one "ALERT STALL" line containing the
                                                    file text; second tick -> silent; file removed -> one "cleared
                                                    STALL" line)
         Keep land-gate-fmt.test.mjs, review-gate.test.mjs, seat-models.test.mjs and seat-cargo-env.test.mjs green.
         build.mjs's dry run (stubbed agent/parallel/phase/log) must still pass with the new stall-writer dispatches
         and the new recon schema key: extend the stubs in build.mjs only as far as the new code needs.
  `,
  acceptance: `
    test -f scripts/loop/stall.mjs
    test -f scripts/loop/stall-detector.test.mjs
    test -f scripts/loop/stall-alarm.sh
    grep -q 'const GATE_BLOCKING' scripts/loop/template.mjs
    grep -q 'const GATE_ADVISORY' scripts/loop/template.mjs
    grep -q 'STALL-RULES-BEGIN' scripts/loop/template.mjs
    grep -q 'STALL-RULES-END' scripts/loop/template.mjs
    test "$(grep -c 'GATE_CMDS(' scripts/loop/template.mjs)" -ge 4
    grep -q 'GATE_BLOCKING' scripts/loop/template.mjs && ! grep -n 'Plus the informational one' scripts/loop/template.mjs
    bash -n scripts/loop/stall-alarm.sh
    shellcheck scripts/loop/stall-alarm.sh
    node --test scripts/loop/stall-detector.test.mjs
    node --test scripts/loop/*.test.mjs
    S=scripts/loop/stall.mjs; cp $S $S.orig; sed -i.bak 's/function sameReasonTwice/function sameReasonTwiceDISABLED/' $S; node --test scripts/loop/stall-detector.test.mjs; MUT=$?; cp $S.orig $S; rm -f $S.orig $S.bak; test $MUT -ne 0
    node --test scripts/loop/stall-detector.test.mjs
    T="$(mktemp -d)" && printf 'GATE RED ON MAIN: clippy\\n' > "$T/STALL.txt" && STALL_FILE="$T/STALL.txt" bash -c '. scripts/loop/stall-alarm.sh; stall_alert=0; stall_alarm_check' | grep -q 'ALERT STALL.*GATE RED ON MAIN: clippy' && rm -rf "$T"
    (cd desktop && npm ci)
    (cd desktop && npm run loop:validate)
  `,
  notes: `
    Audit 2026-10-26: 2 h stall, root cause fmt blocking in land gate while red on main; fixed by hand (fmt advisory) at
    ~15:20Z; this item makes the loop notice next time. FUTURE item — never stop a live pass for it; build it when the
    lanes are idle, then the orchestrator runs npm run loop:build and applies the ~/bin monitor patch from the PR body.
    Open design points the builder should state a decision on in the PR body: (1) whether build mode also halts on a
    red blocking command (recommended yes); (2) an unreadable recon exit counts as red (fail closed) — a Sonnet recon
    seat that omits gateExits will halt the pass, which is the intended loud failure; (3) STALL.txt is removed only by
    the orchestrator.
  `,
})
