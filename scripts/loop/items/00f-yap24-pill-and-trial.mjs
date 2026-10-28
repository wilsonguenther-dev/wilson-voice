// yap24-PILL — THE PILL SAYS WHAT IS HAPPENING. Written 2026-09-26 by the architecture audit
// (docs/ARCHITECTURE-AUDIT-2026-09-26.md §3). Wilson, 2026-09-26: "the pill does not indicate
// when [state] … 14 days".
//
// What the pill fails to indicate on main today, each verified from code:
//   1. The 14-day trial for its first week: pillLicense() is SILENT while more than 7 days remain
//      (desktop/src/pill/license.ts:136-165, "trial, more than 7 days -> nothing"), and the numeral
//      itself is not drawn on main — Y2-B is still an open PR (#186).
//   2. Polishing and pasting: both are declared phases (pill/live.ts:411, :413) but no event
//      produces them — reduceTakePhase (live.ts:1152-1196) only preserves them. After decode the
//      pill shows "thinking" labelled "Transcribing" through the LLM polish and the paste.
//   3. A blind hotkey: Secure Input (secure_input.rs:48, 2 s poll; >100 "Secure Input ENABLED"
//      lines in Wilson's yap.log, from loginwindow and Chrome) and a disabled event tap (Y1-A tap
//      health) are on the status payload (lib.rs:893-922) but float-main.tsx:60-65 reads only
//      recording / busy / engine_loading / last_error. The user presses fn and nothing happens.
//   4. Hands-free vs hold: status carries hands_free (lib.rs:896); the pill draws the same
//      "listening" for both, so a double-tapped take (ptt_macos.rs:34, 450 ms) looks like a hold.
//
// This file is ONE lane. Pass 2 runs it beside 03-yap24-hotkeys-permissions-os.mjs:
//   args: {mode:'build', only:['yap24-PILL','yap24-OS'], panelApproved:[...]}
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs. Headless only.

ITEMS.push({
  id: 'yap24-PILL1', prompt: 'yap24-PILL', branch: 'loop/yap24-pill1-trial-visible-for-the-whole-trial', gated: 'panel',
  title: 'The trial is visible from day 14 to day 1: a quiet numeral for the first week, the Y2-B treatment for the last',
  preflight: `
    ( if ! ( test -f scripts/pill-matrix.mjs && grep -q 'pub const PURCHASE_URL' desktop/src-tauri/src/license.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap24-UI3 (scripts/pill-matrix.mjs) has not landed; build and land UI3 first"; exit 0; fi; grep -q "14d" desktop/src/pill/license.test.ts && grep -q 'purchase_open' desktop/src/pill/license.ts && grep -q 'PILL_QUIET_FROM_DAYS' desktop/src-tauri/src/license.rs )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26 (product copy/threshold — the panel may keep a quiet style, it may not keep silence)
    DEPENDS: [UPDATED 2026-10-26: #186 is CLOSED, not open; cherry-pick branch loop/y2-b-pill-trial-countdown-in-both-styles at 8d85cdf, see the last paragraph; the Buy path depends on yap-lic-04 (PR #232 lands that item) and the matrix on yap24-UI3] Y2-B (formerly open PR #186 — the numeral in both pill styles and the 30px side dock).
      Y2-B lives in 20-y2 on the SAME lane and runs AFTER this file, so do not wait for it: if #186
      is still open when this item starts, carry its commits onto this branch (rebase onto main,
      keep its tests), then close #186 with a "superseded by #<this PR>" comment naming the commits.
      Do not redraw the numeral from scratch. Y2-B's own pre-flight then retires it.

    EVIDENCE
      - pill/license.ts:136-165 display policy: "trial, more than 7 days -> nothing (ambient
        silence)". For half the trial the pill says nothing; Wilson's report is exactly this.

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, MEDIUM + HIGH, GROUNDED):
      - Introduce a SEPARATE shared constant for the new "quiet" threshold (e.g.
        PILL_QUIET_FROM_DAYS = 14) in BOTH license.rs and license.ts. Do NOT repurpose
        PILL_SHOW_DAYS = 7 — it is documented as shared with the Rust tray's own purchase-offer
        threshold (license.ts:52's comment, license.rs:680, tray_offers_purchase at :760) and an
        existing test pins it to 7 (license.test.ts:98); changing it would silently move when the
        tray offers to sell, not just when the pill goes quiet. Add a twin-constant equality test
        and cargo test --lib license to this item's acceptance (today it runs vitest only).
      - Do NOT let a working countdown promise a purchase that cannot happen: as of this panel,
        the Stripe payment link is inactive and the license issuer (Forge) does not answer, so
        this item's countdown/urgent copy must say plainly that purchasing is not open yet
        instead of showing a Buy action that dead-ends — coordinate with LIC-A's Panel revision
        in 20-y2-trial-and-limits.mjs, which is the item that actually restores a working
        purchase path (via the Drivia Consulting app, per the product ledger).
      - The trial is 14 days (license.rs; memory project_yap_build_state YP2 "14-day full trial").
      - Y2-B (#186) draws the numeral but was built against the 7-day policy and was never
        visually QA'd (STATUS-yap.md row Y2-B).

    DO
      1. Policy: trial 14..8 -> show, tone "trial-quiet" (the numeral at reduced contrast, no
         hourglass animation); 7..1 -> the existing "trial"; last day -> "urgent"; licensed ->
         nothing (unchanged); problem / ended unchanged.
      2. The numeral is the days remaining the backend computed (days_left, rollback floor
         included) — never recomputed in the webview.
      3. Hover/accessible title: "Free trial — N days left". Never a price on the pill (Y2-D rule).
      4. Both characters, all three docks, through the shell.

    Tests: license.test.ts — every day 14..0, licensed, problem, ended; a snapshot of the
    accessible title per tone.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - THE USER SEAT FOUND A LIE: on day 1 of an "urgent" trial the user taps Buy and gets an INACTIVE Stripe link on the retired account
        (license.rs:170-176 PAYMENT_LINK_URL). No item owned a working Buy path. The ledger fixes the checkout CLOSED until launch day.
      - CHANGE 1, the Buy chip is truthful: it renders ONLY when the license status reports purchase_open = true, sourced from one Rust
        const PURCHASE_OPEN (default false, surfaced on the license status payload and read in pill/license.ts). While false the pill and
        the countdown copy carry no purchase action at all (trial numeral, "Free trial - N days left", last-day "Trial ends today"). Wilson
        flips PURCHASE_OPEN in the same release that opens checkout. Depends on yap-lic-04 landing (PURCHASE_URL replaces PAYMENT_LINK_URL;
        PR #232 carries those items), so the preflight holds this item until license.rs has pub const PURCHASE_URL. Note yap-lic-04 shows
        no separate "closed" state in its LICENSE PANEL (the web page says when purchases are closed); this gate is for the PILL chip only and
        does not add a closed state anywhere else.
      - CHANGE 2, no redraw: carry the numeral work from branch loop/y2-b-pill-trial-countdown-in-both-styles at 8d85cdf (cherry-pick its numeral
        commits, keep its tests). The "if #186 is still open" path above is dead; #186 is closed.
      - CHANGE 3, the missing gate: the earlier revision promised cargo test --lib license and a twin-constant equality test; the acceptance now
        runs it and greps PILL_QUIET_FROM_DAYS in BOTH license.rs and license.ts. PILL_SHOW_DAYS (license.rs:867, license.ts:59) is NOT repurposed.
      - CHANGE 4, someone could notice it: the acceptance runs node scripts/pill-matrix.mjs --only trial-quiet (6 PNGs: 3 docks x 2 characters)
        once yap24-UI3 has landed; attach the PNGs to the PR, never commit them.
      - Founding-price copy: the FOUNDING19 coupon expires 2026-11-08 (license.rs:168). No founding-price string ships on the pill (Y2-D rule: never
        a price on the pill); the date question belongs to Wilson, not this item.
  `,
  acceptance: `
    grep -q 'purchase_open' desktop/src/pill/license.ts
    grep -q 'PILL_QUIET_FROM_DAYS' desktop/src/pill/license.ts
    grep -q 'PILL_QUIET_FROM_DAYS' desktop/src-tauri/src/license.rs
    grep -q 'pub const PURCHASE_OPEN' desktop/src-tauri/src/license.rs
    O="$(mktemp -d)" && node scripts/pill-matrix.mjs --only trial-quiet --out "$O" && test "$(ls "$O"/*.png | wc -l)" -ge 6
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill/license.test.ts
    npm test
    npm run build
    cd src-tauri && cargo test --features custom-protocol --lib license
  `,
})

ITEMS.push({
  id: 'yap24-PILL2', prompt: 'yap24-PILL', branch: 'loop/yap24-pill2-backend-stage-events-for-polish-and-paste', gated: 'panel',
  title: 'The backend says which stage a take is in, so "polishing" and "pasting" finally appear on the pill',
  preflight: `
    ( if ! ( test -f scripts/pill-matrix.mjs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap24-UI3 (scripts/pill-matrix.mjs) has not landed; build and land UI3 first"; exit 0; fi; grep -q "TAKE_STAGE_EVENT" desktop/src-tauri/src/lib.rs && grep -q "take_stage" desktop/src/float-main.tsx && grep -q 'insert-copy-card' desktop/src/pill/live.ts )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26.
    DEPENDS: Y5-C (merged #193 — the phase machine this feeds)

    EVIDENCE
      - pill/live.ts:411 "polishing" and :413 "pasting" are declared, owned by Y5-C / Y7-D, and
        never produced: reduceTakePhase (live.ts:1152-1196) has no event that yields either.
      - The backend emits recording, status, transcript, transcribe_progress, audio_level,
        license, license_required and a few UI events — no stage event (grep of .emit in
        src-tauri/src, 2026-09-26).
      - Result: the LLM polish (up to the Y4-E chunked deadline) and the paste receipt wait both
        read as "Transcribing".

    DO
      1. One event, TAKE_STAGE_EVENT = "take_stage", payload {stage, takeId, words?}, emitted at
         the real boundaries in the dictation pipeline: decode start, polish start (only when the
         polish stage actually runs), paste start, paste confirmed / not confirmed, done, empty,
         error. One emit helper; no stage is emitted from two places.
      2. reduceTakePhase gains {type:'stage'}; stage outranks the inferred status path, and the
         existing hold/timeout policy (PHASE_HOLD_MS) still bounds every working phase.
      3. The paste-not-confirmed receipt ("no app read the clipboard within 1500ms", seen in
         Wilson's log) becomes a visible one-line state, not only a log line.

    Tests: Rust — the dictation pipeline test harness asserts the stage sequence for
    polish-on and polish-off takes; TS — live.test.ts stage sequences incl. out-of-order status.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - HAND OFF, do not build a second surface: DO step 3 ("paste not confirmed becomes a visible one-line state") is VOID as a pill state. The ungated
        item yap-insert-bridge-2026-10-26 (00a) owns paste, the Accessibility probe and the Copy card (INSERT_COPY_CARD_EVENT "insert-copy-card"
        in paste.rs). A not-confirmed paste emits its stage and the pill hands off to that Copy card; the pill draws no extra sentence. The
        stage boundaries around paste live in paste.rs's single insertion chooser (choose_insert_path), so emit stage events from that one
        boundary and rebase after the insert-bridge PR; do not add a second emit site.
      - THE JOB THAT REMAINS: live.ts:411 "polishing" and :413 "pasting" are declared and never produced (reduceTakePhase, live.ts:1152-1196, has no
        event that yields them). Produce them from take_stage. The Rust test asserts the stage sequence for polish-on and polish-off takes and
        the acceptance now requires at least one test to RUN (grep for "test result: ok. N passed" with N >= 1); the old form could pass on
        an unmatched filter.
      - Someone could notice it: node scripts/pill-matrix.mjs --only polishing and --only pasting (6 PNGs each), PNGs attached to the PR.
  `,
  acceptance: `
    grep -q "TAKE_STAGE_EVENT" desktop/src-tauri/src/lib.rs
    grep -q 'insert-copy-card' desktop/src/pill/live.ts
    O="$(mktemp -d)" && node scripts/pill-matrix.mjs --only polishing --out "$O" && test "$(ls "$O"/*.png | wc -l)" -ge 6
    O="$(mktemp -d)" && node scripts/pill-matrix.mjs --only pasting --out "$O" && test "$(ls "$O"/*.png | wc -l)" -ge 6
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill
    npm run build
    cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib take_stage 2>&1 | grep -qE 'test result: ok\\. [1-9][0-9]* passed'
    cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-PILL3', prompt: 'yap24-PILL', branch: 'loop/yap24-pill3-blind-hotkey-and-blind-paste-states', gated: 'panel',
  title: 'When the hotkey cannot hear you, the pill says so: Secure Input, a disabled tap, and missing Accessibility are pill states',
  preflight: `
    ( if ! ( test -f scripts/pill-matrix.mjs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap24-UI3 (scripts/pill-matrix.mjs) has not landed; build and land UI3 first"; exit 0; fi; grep -q "secure_input" desktop/src/float-main.tsx && grep -q '"blind"' desktop/src/pill/live.ts && grep -q 'still works' desktop/src/pill/live.ts )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26.
    DEPENDS: PERM-C (merged #161), Y1-A (merged #162)

    EVIDENCE
      - lib.rs:893-922 build_status already computes secure.blocked, the tap health message and
        accessibility. float-main.tsx:60-65 BackendStatus declares only recording, busy,
        engine_loading, last_error.

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, HIGH, GROUNDED):
      - Do NOT move the Secure Input check to key-down + a backstop (see the REJECTED plan noted
        in yap24-OS5's Panel revision) — the tap is blind under Secure Input and never sees that
        key-down. Keep secure_input.rs's 2 s poll as the source for this pill state.
      - Add a FOURTH blind-hotkey cause this item's original three (Secure Input, tap disabled,
        Accessibility missing) do not cover: the hotkey tap was never created because Input
        Monitoring was missing at launch (ptt_macos.rs logs this and returns without ever
        retrying). Wire OS3's grant-flip detection to call ptt_macos::start again, and give this
        pill state a distinct cause/copy ("hotkey not installed") with the Privacy_ListenEvent
        deep link, not just "disabled."
      - Add a dwell rule before showing "blind": ignore loginwindow as an owner of Secure Input
        (Wilson's log shows it firing most often at the lock screen, when the pill is not visible
        anyway) and require >=1 s of continuous Secure Input before the state appears, clearing
        immediately once it is gone — otherwise a brief Chrome password-field focus flickers the
        pill. Acceptance gains a no-flicker case (secure input for 500 ms -> no blind state) and
        keeps the existing <=2.1 s appearance bound.
      - Wilson's logs: well over 100 WARN lines "Secure Input ENABLED by loginwindow / Google
        Chrome — the fn PTT event tap is blind" across yap.log, yap.log.1 and yap.log.2.
      - The live gate reducer (live.ts reduceGatePhase) handles mic permission, recording and
        cancel only.

    DO
      1. A gate phase "blind" with three causes, each one sentence and one action:
         Secure Input on (name the owning app, which secure_input.rs already resolves);
         event tap disabled by macOS (re-armed per Y1-A — say so while it is down);
         Accessibility missing (paste will copy to the clipboard only — say that, deep link).
      2. It outranks idle/sleepy but not a take in progress; it clears the moment status clears.
      3. The pill stays click-through except the action chip.

    Tests: live.test.ts — status payloads -> phase for each cause, precedence against a take.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - PREDATES #249. hotkeys.rs:14-17: the Carbon chord (default Control-Option-Command-Space) needs no Accessibility, no Input Monitoring and is delivered
        under Secure Input; attempt_tap re-arms on every permission edge (:19-20). So "blind" is now FN ONLY: only the fn key is dead under Secure Input.
      - COPY (exact): "fn is blocked by <app>'s secure field - <chord_label> still works", where <chord_label> is hotkeys.rs::chord_label (:496) for the
        user's CURRENT chord (so a rebound chord reads truthfully) and <app> is the owner secure_input.rs already resolves. The old claim that the
        hotkey is dead would be a lie for anyone with the chord on. The acceptance greps "still works" in live.ts, and live.test.ts gains a
        render case asserting the chord hint appears.
      - DROPPED: the "hotkey not installed" cause (done by #249's re-arm retry) and the Accessibility cause (belongs to yap-insert-bridge, which owns
        the Copy card). Keep the dwell rule (ignore loginwindow, at least 1 s continuous Secure Input, 500 ms never flickers, clears at once) and
        the 2 s poll (see OS5: exempt by name).
      - Someone could notice it: node scripts/pill-matrix.mjs --only blind, PNGs attached to the PR.
  `,
  acceptance: `
    grep -q 'still works' desktop/src/pill/live.ts
    O="$(mktemp -d)" && node scripts/pill-matrix.mjs --only blind --out "$O" && test "$(ls "$O"/*.png | wc -l)" -ge 6
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill
    npm test
    npm run build
  `,
})

ITEMS.push({
  id: 'yap24-PILL4', prompt: 'yap24-PILL', branch: 'loop/yap24-pill4-hands-free-looks-different-from-hold', gated: 'panel',
  title: 'Hands-free looks different from hold: a lock mark and a stop hint that names the key the user actually bound, in both characters and all docks',
  preflight: `
    ( if ! ( test -f scripts/pill-matrix.mjs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap24-UI3 (scripts/pill-matrix.mjs) has not landed; build and land UI3 first"; exit 0; fi; grep -q "hands_free" desktop/src/float-main.tsx && grep -q "hands-free" desktop/src/pill/live.test.ts && grep -qE 'chord_label|chordLabel' desktop/src/pill/live.ts )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26.
    DEPENDS: yap24-PILL2 (same reducer), Y5-K when merged (characters are data)

    EVIDENCE
      - lib.rs:896 status carries hands_free; the pill never reads it (float-main.tsx:60-65).
      - ptt_macos.rs:34 DOUBLE_TAP_MS = 450, :550 double-tap -> hands-free ON; Wilson's log shows
        dozens of hands-free takes, some several minutes long, with the same pill as a hold.

    DO: a listening variant "listening-locked" (phaseVisual label "Hands-free — tap fn⌃ to stop"),
    drawn by the shell as a small lock chip; the live commentary keeps running; Escape/cancel copy
    unchanged. Tests in live.test.ts.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - The hardcoded "Hands-free - tap fn-Control to stop" is WRONG now that chords are rebindable (#249). The label is built from the live binding:
        "Hands-free - " + chord_label (hotkeys.rs:496, carried on the status payload) + " to stop". A test in live.test.ts renders it with a REBOUND
        key and asserts the rebound label appears and the old literal does not; the acceptance fails if the string "tap fn" survives in non-test pill source.
      - Someone could notice it: node scripts/pill-matrix.mjs --only listening-locked, PNGs attached to the PR.
  `,
  acceptance: `
    grep -qE 'chord_label|chordLabel' desktop/src/pill/live.ts
    ! grep -rn --include='*.ts' --include='*.tsx' --exclude='*.test.ts' --exclude='*.test.tsx' 'tap fn' desktop/src/pill
    O="$(mktemp -d)" && node scripts/pill-matrix.mjs --only listening-locked --out "$O" && test "$(ls "$O"/*.png | wc -l)" -ge 6
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill
    npm run build
  `,
})
