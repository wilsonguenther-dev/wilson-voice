// Y2 — TRIAL + LIMITS IN THE PILL. Wilson, 2026-09-12, verbatim: "the pill does
// not tell people when they reach their limits or when the 14-day trial ends
// (Wispr Flow does)."
//
// AUDIT: the licensing BACKEND is good and the MAIN WINDOW is good. The pill —
// the only Yap surface a user looks at while working — knows nothing.
//
//   src-tauri/src/license.rs         1784 lines. TRIAL_DAYS = 14 (license.rs:107),
//                                    clock-rollback floor, two-store trial start,
//                                    Ed25519 offline verify, revocation list.
//   src-tauri/src/lib.rs:1111        license_allows_new_dictation — the one gate,
//                                    emits `license_required`, throttled notify.
//   src/license/status.ts            chipFor / statusCopy / trialWarningText /
//                                    TRIAL_WARN_DAYS = 3, all pure + unit-tested.
//   src/App.tsx:1194-1210            main window listens for license_status and
//                                    license_required and raises a sheet.
//
//   git grep -n "license\|trial" origin/main -- desktop/src/pill \
//        desktop/src/float-main.tsx desktop/src-tauri/src/float_pill.rs
//     -> ZERO MATCHES. The pill window never receives the license event, has no
//        state for it, and renders nothing.
//
// So a user on day 13 gets no warning where they are looking, and on day 15 the
// hotkey stops working with a throttled system notification as the only signal.
// status.ts:80-88 deliberately fires the trial warning ONCE at three days
// ("a countdown that reappears every launch is how a good app becomes
// nagware") — a good rule for a modal toast, and the wrong rule for the pill,
// which is ambient and can carry a quiet persistent numeral the way Wispr's
// Flow Bar does.
//
// USAGE LIMITS: there is no metering of any kind.
//   git grep -n "quota\|usage_limit\|daily_limit\|words_limit\|minutes_used" \
//        origin/main -- desktop  -> 0 functional matches.
// Wispr's free desktop tier is 2,000 words/week and it ships a named
// notification `WeeklyWordsLimitReached`
// (reference_wispr_parity_research §2.8 / §5.2, both [BUNDLE]/[OFFICIAL]).
// Yap's shipped model is $29 lifetime with a 14-day full-feature trial and NO
// subscription (project_yap_build_state, closed decision). Whether Yap gains a
// metered free tier at all is a pricing decision -> DB-A is gated:'panel'.
// Y2-A..E ship the mechanism and the surfaces for the trial, which is decided.
//
// SHARED PREAMBLE + STANDARD GATE: see 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'LIC-A', prompt: 'Y2', branch: 'loop/lic-a-purchase-to-working-dictation-proven-end-to-end', gated: null,
  title: 'Payment to working dictation, proven once end to end — the leg no item owned',
  preflight: `
    grep -q 'retrieve_license\\|retrieveLicense' desktop/src-tauri/src/license.rs
    grep -q 'ISSUANCE' docs/RELEASE.md
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test activation_e2e
  `,
  spec: `
    PANEL 2026-09-12, two seats independently, and it runs FIRST in this file
    because everything else here pushes a user toward paying. MEASURED at
    4e8c9adf: four new pressure surfaces are planned (Y2-B's numeral, Y2-C's
    gated state, Y2-D's upgrade sheet, Y2-E's tray line) and NOTHING in the
    plan owns the segment between the card being charged and a key existing.
      desktop/src/license/PurchasePrompt.tsx:15,72 — the sheet offers exactly
        two things: buy, or paste a key you already have.
      desktop/src-tauri/src/license.rs:37-41 — the signing key "lives
        root-owned, mode 0400, on the Forge box"; nothing in this repo mints.
      no Stripe webhook, issuer or fulfilment code anywhere in the tree.
      license.rs:117 REVOCATION_URL is an IP-bearing sslip.io hostname, so any
        box move breaks revocation silently.
    The worst outcome in the whole product is a $29 customer with a dead app,
    and it is currently untested.

    Do, and keep it small:
      * Prove the path ONCE with a staging key: issue from the Forge signer,
        activate it in a clean YAP_DATA_DIR (Y0-D), assert the entitlement flips
        and a dictation completes. That walk is the item's evidence.
      * Add "I already paid — retrieve my license" to the purchase sheet: it
        asks the issuer for a key by checkout email and activates on success.
        If issuance is MANUAL (Wilson's call, see docs/loop/PLAN.md §4), the
        button instead states the turnaround honestly and dictation keeps
        working under a short, signed grace claim rather than going dead.
      * Define what a FAILED activation says. Today that path has no copy and
        no surface. One sentence, one action, no raw Rust string.
      * Give REVOCATION_URL a real hostname so the box can move, and check
        issuer-host liveness in the release checklist, not at runtime.
      * Write the issuance runbook into docs/RELEASE.md under a heading
        containing ISSUANCE.

    What NOT to do:
      - Do NOT put the signing key, or any path to it, in this repo.
      - Do NOT make dictation depend on reaching the issuer. Offline verify
        stays the mechanism; retrieval is a convenience.
  `,
  acceptance: `
    grep -q 'ISSUANCE' docs/RELEASE.md
    grep -rq 'retrieve' desktop/src/license
    test 0 -eq "$(grep -c 'sslip.io' desktop/src-tauri/src/license.rs)"
    test -f desktop/src-tauri/tests/activation_e2e.rs
    grep -q 'a_signed_key_flips_the_entitlement_and_dictation_resumes' desktop/src-tauri/tests/activation_e2e.rs
    grep -q 'a_failed_activation_has_copy_and_an_action' desktop/src-tauri/tests/activation_e2e.rs
    grep -q 'retrieval_failure_never_blocks_offline_verification' desktop/src-tauri/tests/activation_e2e.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test activation_e2e ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y2-A', prompt: 'Y2', branch: 'loop/y2-a-license-status-reaches-the-pill-window', gated: null,
  title: 'The float window subscribes to license status — the wiring that does not exist',
  preflight: `
    grep -q 'license' desktop/src-tauri/src/float_pill.rs
    grep -q 'license_status\\|LicenseStatus' desktop/src/float-main.tsx
    cd ${APP} && npm ci && npm test -- pill
  `,
  spec: `
    Tauri events emitted with \`app.emit\` reach every window, but the float
    window has no listener and no state, so the payload lands nowhere.
    \`desktop/src/float-main.tsx\` is 53 lines and mounts a pill; it subscribes
    to nothing license-shaped.

    Do:
      * In float-main.tsx, on mount: \`invoke<LicenseStatus>("license_status")\`
        for the initial value (the same command App.tsx:1194 calls), then
        \`listen<LicenseStatus>("license", ...)\` and
        \`listen<LicenseStatus>("license_required", ...)\`. Hold it in one piece
        of state and pass it to whichever pill is mounted.
      * REUSE \`desktop/src/license/status.ts\` verbatim — \`chipFor\`,
        \`daysLeft\`, \`trialCountdown\`, \`statusCopy\`. Do not write a second
        copy of the trial arithmetic for the pill; that file exists precisely so
        the card, the prompt and the changelog "cannot drift apart" (its own
        doc comment) and the pill is now a fourth consumer.
      * Add a pure \`pillLicense(status)\` to a new
        \`desktop/src/pill/license.ts\` returning
        \`{ show: boolean, tone: "trial"|"urgent"|"ended", glyph: string,
           value: string|null, title: string }\`, with the display POLICY in one
        pure function so Y2-B/C/D render it and never re-decide it:
          - licensed                        -> show: false. Nothing. Ever.
          - trial, days_left > 7            -> show: false (ambient silence)
          - trial, 1..7 days                -> show: true, tone trial,  value "Nd"
          - trial, last day / 0             -> show: true, tone urgent, value "1d"
          - license_required                -> show: true, tone ended
        Seven days, not three: the pill is ambient and cheap to glance at, and
        the one-shot toast at TRIAL_WARN_DAYS = 3 stays exactly as it is. Say
        both numbers in the doc comment so the difference reads as deliberate.
      * The pill NEVER shows a price in this item. The money copy belongs to the
        purchase surface (Y2-D), and \`PRICE_LABEL\` must not be imported by any
        file under desktop/src/pill/.

    Tests in \`desktop/src/pill/license.test.ts\`, table-driven over the full
    fortnight (14 -> 0) plus licensed and license_required: assert the exact
    show/tone/value for each day, and assert \`show === false\` for every
    licensed status regardless of trial fields (a licensed user who once had a
    trial must see nothing).

    What NOT to do:
      - Do NOT poll \`license_status\` on an interval from the pill. It is
        event-driven; the backend already emits on every change.
      - Do NOT re-derive days-left from \`expires_at_ms\` and a second wall-clock read in the
        pill. The backend owns the clock, including the rollback floor
        (license.rs:473-520). A second clock is a second answer.
  `,
  acceptance: `
    test -f desktop/src/pill/license.ts
    test -f desktop/src/pill/license.test.ts
    grep -q 'pillLicense' desktop/src/pill/license.ts
    grep -q 'license_status' desktop/src/float-main.tsx
    grep -q 'license_required' desktop/src/float-main.tsx
    grep -q 'from "../license/status"' desktop/src/pill/license.ts
    # the pill never learns the price
    test 0 -eq "$(git grep -c 'PRICE_LABEL' -- desktop/src/pill | wc -l)"
    # no second clock in the pill
    test 0 -eq "$(grep -c 'now()' desktop/src/pill/license.ts)"
    cd ${APP} && npm ci
    npm test -- pill/license ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build            ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y2-B', prompt: 'Y2', branch: 'loop/y2-b-pill-trial-countdown-in-both-styles', gated: null,
  title: 'A quiet trial numeral on the pill in both pill styles, in the 30px side dock too',
  preflight: `
    grep -q 'pillLicense' desktop/src/pill/ClassicPill.tsx
    grep -q 'pillLicense' desktop/src/pill/YappyPill.tsx
    cd ${APP} && npm ci && npm test -- pill
  `,
  spec: `
    Render \`pillLicense(status)\` from Y2-A in both pills. \`ClassicPill.tsx\` is
    the DEFAULT (\`lib.rs:407 pill_style: "classic"\`) so it is not optional, and
    \`YappyPill.tsx\` is the character pill.

    ClassicPill: a small trailing chip on the capsule — the numeral in
    Departure Mono (the pixel face already bundled at
    src/assets/fonts/DepartureMono-Regular.woff2, and the face \`chipFor\` in
    status.ts:118 already says "the component sets it in Departure Mono"), the
    unit in the body face. \`urgent\` shifts hue and nothing else: no pulsing, no
    animation, no motion. A countdown that moves is a countdown that nags.

    YappyPill: Yappy holds it. Same numeral, same silence — a posture change at
    \`urgent\` (ears down, one blink slower) rather than a badge, because this
    pill's whole job is that state reads as character
    (feedback_companion_must_be_cute: pixel art, chunky, no angry eyebrows).

    THE SIDE DOCK IS THE HARD CASE AND IT IS IN SCOPE. Per
    reference_wispr_parity_research §4.2, Wispr's side-docked bar is a 30 px
    strip and their fixed-width label states "crush in a 30px column", which is
    why only the primary listening states rotate. A "3d" numeral fits a 30 px
    column; "3 days left" does not. So: the pill shows the VALUE only, and the
    full sentence lives in the \`title\` (tooltip) which \`pillLicense\` already
    returns. Verify at all three dock positions — \`pill_position\` is
    \`bottom|left|right\` (lib.rs:408 default "bottom").

    prefers-reduced-motion: ClassicPill.tsx:44-47 already paints one calm static
    frame under Reduce Motion. The trial chip must be present in that frame —
    it is information, not decoration, and must not be hidden with the animation.

    Tests: extend \`desktop/src/pill/license.test.ts\` for the geometry policy
    (\`value\` is never longer than 3 characters for any day 0..14) and add a
    vitest render assertion per pill that the chip is present for
    \`days_left: 5\`, absent for \`licensed\`, and present under a mocked
    \`matchMedia("(prefers-reduced-motion: reduce)") => matches: true\`.

    PR body owes: the pill captured at bottom, left and right docks, for
    days_left 10 (hidden), 5 (trial), 1 (urgent) and license_required.

    What NOT to do:
      - Do NOT animate, pulse, bounce or flash the countdown.
      - Do NOT put the word "upgrade" or a price on the capsule. That is Y2-D.
      - Do NOT let the chip widen the capsule enough to break the 30 px strip.
  `,
  acceptance: `
    grep -q 'pillLicense' desktop/src/pill/ClassicPill.tsx
    grep -q 'pillLicense' desktop/src/pill/YappyPill.tsx
    grep -q 'reduce' desktop/src/pill/ClassicPill.tsx
    grep -q 'value_is_never_wider_than_the_side_dock\\|valueFitsSideDock' desktop/src/pill/license.test.ts
    cd ${APP} && npm ci
    npm test -- pill ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y2-C', prompt: 'Y2', branch: 'loop/y2-c-trial-ended-pill-state-and-refused-press', gated: null,
  title: 'A refused hotkey press produces a pill state that explains itself, instead of a throttled notification',
  preflight: `
    grep -q '"gated"' desktop/src/pill/live.ts
    cd ${APP} && npm ci && npm test -- pill/live
  `,
  spec: `
    Today, past the trial: \`license_allows_new_dictation\` (lib.rs:1111-1126)
    emits \`license_required\`, calls \`notify()\` if \`should_announce_gate()\`
    permits, logs, returns false. The pill does not move. The user holds the key
    and nothing happens — which is the single worst possible reading of a
    paid-product boundary, because it is indistinguishable from a broken app.

    Add \`"gated"\` to \`LivePhase\` (live.ts:252, currently
    \`idle|listening|thinking|done|sleepy\`, plus \`"blocked"\` from PERM-C) and
    render it in both pills: the capsule takes the \`ended\` tone from
    \`pillLicense\`, shows the trial-ended glyph, and a click opens the purchase
    surface in the main window. It holds for ~2.5 s after a refused press and
    then settles back to the persistent \`ended\` chip, so leaning on the hotkey
    is answered every time without the state becoming permanent noise.

    Precedence, asserted in tests, because these will collide in real use:
      blocked (no mic)  >  gated (no license)  >  listening  >  thinking  >  done
    The microphone reason wins: telling a user to buy a license when Yap cannot
    hear them is the wrong sentence. This is the same order PERM-C enforces in
    \`start_recording\` and the pill must not disagree with the backend.

    Keep \`should_announce_gate\`'s throttle for the SYSTEM notification exactly
    as it is. The pill state is not throttled — it is the cheap in-place signal
    that makes the throttle safe.

    Copy, from the strings that already exist so nothing drifts:
    \`statusCopy(license_required)\` = "Dictation is paused" / "Everything you
    have already written is still here and still exportable." The pill shows the
    headline; the tooltip carries the body. Reuse, do not rewrite.

    Tests in \`desktop/src/pill/live.test.ts\` (271 lines already):
      * \`gated_holds_then_settles_to_the_ended_chip\`
      * \`blocked_outranks_gated\`
      * \`gated_never_suppresses_the_done_state_of_a_take_already_in_flight\` —
        the trial ending must not eat the result of a take that was allowed to
        start. license.rs's own doc (lib.rs:1100-1108) says the gate "never
        takes back the ones already spoken"; this is that promise in the UI.

    What NOT to do:
      - Do NOT make the gated state permanent-modal or focus-stealing. The pill
        is non-activating (\`macOSPrivateApi: true\`, NSPanel behaviour).
      - Do NOT disable the pill's other affordances. History, search, export and
        settings all keep working past the trial — that is the product's
        promise (KEEP_FOREVER_LINE in status.ts) and the pill must not imply
        otherwise.
  `,
  acceptance: `
    grep -q '"gated"' desktop/src/pill/live.ts
    grep -q 'blocked_outranks_gated' desktop/src/pill/live.test.ts
    grep -q 'gated_never_suppresses_the_done_state' desktop/src/pill/live.test.ts
    grep -q 'gated' desktop/src/pill/ClassicPill.tsx
    grep -q 'gated' desktop/src/pill/YappyPill.tsx
    grep -q 'statusCopy' desktop/src/pill/license.ts
    cd ${APP} && npm ci
    npm test -- pill ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y2-F', prompt: 'Y2', branch: 'loop/y2-f-a-stored-key-that-grants-nothing-is-not-a-lapsed-trial', gated: null,
  title: 'A paying customer whose key stops verifying is never shown a price',
  preflight: `
    grep -rq 'storedKeyProblem' desktop/src/pill
    cd ${APP} && npm ci && npm test -- license
  `,
  spec: `
    PANEL 2026-09-12, THREE seats independently — the most-converged gap in the
    trial lane. MEASURED at 4e8c9adf: the backend already distinguishes "a key
    is stored and granted nothing" and only the main window renders it.
      desktop/src/license/status.ts:47-48  license_problem_message,
                                           has_stored_license
      desktop/src/license/status.ts:213-217  storedKeyProblem(status)
      desktop/src/license/status.ts:70      DEFAULT_SEATS = 3
      desktop/src/license/status.test.ts:162  "This license was refunded or
                                           charged back."
      grep -rn 'storedKeyProblem|has_stored_license|license_problem_message'
        over all item files -> no functional hit
    Y2-A's pillLicense policy enumerates exactly three inputs — licensed, trial
    with days, license_required — so a revoked, unreadable, clock-broken or
    seat-capped key collapses into license_required, Y2-C paints the "ended"
    tone, and Y2-D offers the Payment Link to someone who has already paid.
    That is the most expensive sentence this app can say.

    Do:
      * pillLicense gains a SIXTH branch: \`has_stored_license && state !==
        "licensed"\` -> tone \`problem\`. Its copy comes from
        \`license_problem_message\`, never from the purchase copy, and it is
        distinct from the lapsed-trial tone at a glance in both pill styles.
      * Its click opens the License panel (re-activate / re-paste the key). NO
        purchase affordance is reachable from this tone — assert that.
      * The tray row says the same sentence, from the same source (Y2-E).
      * Table test over the whole matrix, including a revoked key, an unreadable
        store and a seat-capped key.

    Depends on Y2-A (the wiring) and pairs with Y2-D's "never show a price to a
    stored-key holder" rule, which this item is what makes checkable.
  `,
  acceptance: `
    grep -rq 'storedKeyProblem' desktop/src/pill
    grep -rq "problem" desktop/src/pill/license.ts
    test -f desktop/src/pill/license.test.ts
    grep -q 'a_revoked_key_never_shows_a_price' desktop/src/pill/license.test.ts
    grep -q 'a_seat_capped_key_reads_as_a_problem_not_a_lapsed_trial' desktop/src/pill/license.test.ts
    grep -q 'the_problem_tone_opens_the_license_panel' desktop/src/pill/license.test.ts
    cd ${APP} && npm ci
    npx tsc --noEmit ; test $? -eq 0
    npm test         ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y2-D', prompt: 'Y2', branch: 'loop/y2-d-one-upgrade-path-from-the-pill', gated: null,
  title: 'One click from the pill to purchase, reusing the existing Payment Link — no new money surface',
  preflight: `
    grep -q 'open_purchase_page\\|show_purchase' desktop/src-tauri/src/float_pill.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test purchase_from_pill
  `,
  spec: `
    The purchase machinery is built and proven: \`license::PAYMENT_LINK_URL\` is a
    compile-time constant, \`open_purchase_page\` is the only thing that can hand
    a URL to \`open(1)\` (status.ts:56-62 documents exactly this discipline and
    cites the Stripe objects), and \`src/license/PurchasePrompt.tsx\` is the
    sheet. Stripe was E2E-proven including the refund and dispute legs
    (project_yap_build_state, yap21).

    All this item does is connect the pill to it:
      * Clicking the pill in \`gated\` or \`urgent\` tone focuses the main window
        and raises the existing PurchasePrompt. It does NOT open a browser
        directly from the pill — the sheet is where the $29 / $19 founding copy,
        the seats line and KEEP_FOREVER_LINE live, and skipping it would put
        Wilson's user in Stripe with no context.
      * Add one Rust command \`reveal_purchase_prompt\` that unminimizes + focuses
        the main window and emits \`show_purchase\`. App.tsx listens and raises
        the sheet. The pill's job ends at "ask the main window".
      * The pill's own click target must not steal focus on hover or on the
        press that starts a dictation. \`ClassicPill.tsx\` already publishes a
        hitbox (\`watchPillHitbox\`, YV65) — the upgrade affordance shares it and
        is only live when \`pillLicense().show\` is true, so a licensed user's
        pill has no dead click region.

    Test \`tests/purchase_from_pill.rs\`:
      * \`reveal_purchase_prompt_never_opens_a_url\` — assert the command's body
        contains no call into the opener; the URL path stays behind
        \`open_purchase_page\`. This is a security property, not a style
        preference: one function is the only thing that may be handed to open(1).
      * \`pill_upgrade_is_inert_when_licensed\`.

    What NOT to do:
      - Do NOT add a second Payment Link, price string, or coupon code anywhere.
        FOUNDING_CODE and PRICE_LABEL live in status.ts and PAYMENT_LINK_URL
        lives in Rust; a third copy is a mispriced checkout waiting to happen.
      - Do NOT open a browser from the float window.
  `,
  acceptance: `
    grep -q 'fn reveal_purchase_prompt' desktop/src-tauri/src/lib.rs
    grep -rq 'show_purchase' desktop/src
    grep -q 'reveal_purchase_prompt' desktop/src/pill/license.ts
    test -f desktop/src-tauri/tests/purchase_from_pill.rs
    grep -q 'reveal_purchase_prompt_never_opens_a_url' desktop/src-tauri/tests/purchase_from_pill.rs
    # exactly one payment link and one price label in the tree
    test 1 -eq "$(git grep -c 'PAYMENT_LINK_URL: ' -- desktop/src-tauri/src/license.rs | cut -d: -f2)"
    test 1 -eq "$(git grep -l 'PRICE_LABEL =' -- desktop/src | wc -l | tr -d ' ')"
    cd ${APP} && npm ci && npm run build ; test $? -eq 0
    cd src-tauri && cargo test --features custom-protocol --test purchase_from_pill ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y2-E', prompt: 'Y2', branch: 'loop/y2-e-menu-bar-tray-carries-the-same-truth', gated: null,
  title: 'The menu-bar item says the same thing as the pill and the settings card, from one source',
  preflight: `
    grep -q 'pillLicense\\|license_tray_line' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test tray_license
  `,
  spec: `
    Yap has a tray menu (YV26; \`sync_tray\` referenced at lib.rs:4468) and it
    does not mention the trial. Wispr's status menu carries the state
    (reference_wispr_parity_research §2.2, \`hub_status_menu_*\`). A user who
    hides the pill — \`show_floating_pill\` is a setting (lib.rs:406) — currently
    has NO ambient signal at all.

    Add to the tray, from the SAME decision function so the three surfaces can
    never disagree:
      * A disabled header item carrying the state: "Trial — 5 days left" /
        "Licensed" / "Trial ended — dictation paused".
      * An "Upgrade Yap — $29 once" item, present only when
        \`pillLicense().show\` is true, firing \`reveal_purchase_prompt\`.
      * The tray ICON takes the urgent treatment on the last day and past the
        trial, and only then. A permanently decorated tray icon is noise.

    The decision must be shared, not duplicated: add
    \`license::tray_line(&LicenseStatus) -> (String, bool /*urgent*/)\` in Rust
    and assert in \`tests/tray_license.rs\` that its day boundaries match
    \`pillLicense\`'s exactly — 7 days to appear, urgent at <= 1 — by reading the
    thresholds from named constants that both sides import. Name them once:
    \`license::PILL_SHOW_DAYS = 7\` and \`license::PILL_URGENT_DAYS = 1\`, exported
    to TS through a generated constants module or asserted equal by a test that
    parses both files. Prefer the test-parses-both-files approach; it needs no
    build step and it fails loudly.

    \`sync_tray\` is already guarded and already the only place the tray is
    rebuilt — keep it that way. \`tests/tray_hotkey_no_collision.rs\` exists;
    do not disturb it.

    What NOT to do:
      - Do NOT add a badge count or a number on the tray icon.
      - Do NOT hardcode 7 and 1 in two languages. The whole point of this item
        is one source of truth for three surfaces.
  `,
  acceptance: `
    grep -q 'PILL_SHOW_DAYS' desktop/src-tauri/src/license.rs
    grep -q 'PILL_URGENT_DAYS' desktop/src-tauri/src/license.rs
    grep -q 'fn tray_line' desktop/src-tauri/src/license.rs
    test -f desktop/src-tauri/tests/tray_license.rs
    grep -q 'pill_and_tray_share_the_same_day_thresholds' desktop/src-tauri/tests/tray_license.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test tray_license            ; test $? -eq 0
    cargo test --features custom-protocol --test tray_hotkey_no_collision ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'DB-A', prompt: 'Y2', branch: 'loop/db-a-usage-metering-and-limit-surface', gated: 'panel',
  title: 'Usage metering and a "limit reached" surface — the numbers are Wilson\'s call',
  preflight: `
    grep -q 'usage_window\\|words_this_week' desktop/src-tauri/src/db.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test usage_meter
  `,
  spec: `
    GATED: 'panel'. Wilson said the pill must tell people "when they reach their
    limits". Yap has no limits today, by decision — $29 lifetime, 14-day
    full-feature trial, "no subscription v1", and
    reference_wispr_parity_research §3 lists "trial nag surfaces" under
    "Explicitly not wanted". Wispr's limit is 2,000 words/week on its free
    desktop tier with a \`WeeklyWordsLimitReached\` notification
    (§2.8 [LOCAL], §5.2 [OFFICIAL]). Yap's own marketing line is the opposite:
    "No account, no cloud, no subscription tier gating your words."

    So the MECHANISM is buildable now and the POLICY is not. What the panel and
    Wilson must decide before this item's numbers are written:
      1. Does Yap gain a metered free tier after the trial at all, or does the
         trial simply end (today's behaviour)?
      2. If metered: the unit (words / minutes / takes), the window (day /
         week / rolling 7d), and the number.
      3. Does a limit throttle NEW dictation only, matching the trial's
         boundary exactly (lib.rs:1100-1108), or degrade quality? (Degrading
         quality is almost certainly wrong — say so and let it be rejected.)
      4. Does the pill show consumption before the limit (a Wispr-style
         "1,847 / 2,000 words" readout) or only on arrival?

    BUILD REGARDLESS, because it is useful with or without a limit and it is
    the honest version of Insights:
      * \`usage\` rollup in SQLite: words and voiced-seconds per local day,
        written on take finalize, in the SAME transaction as the transcript row
        so the two can never disagree. Reuse the existing rollup shape —
        \`db.rs\` already carries an insights/day-series path
        (\`tests/meeting_stats_rollup.rs\`, \`get_insights\`, lib.rs:2420) —
        rather than adding a parallel aggregate.
      * \`db::usage_window(unit, window) -> UsageWindow { used, window_start }\`,
        pure over the rollup, with the LIMIT VALUE passed in by the caller and
        NOT stored in this function. That is the seam that lets the policy land
        later as one constant.
      * \`pillLicense\` (Y2-A) grows a \`limit\` branch behind a single
        \`LIMIT_ENABLED\` constant that is FALSE in this item. Wire the state,
        the copy and the tests; ship it dark.
      * Copy drafted, not shipped, for Wilson's review before any send-equivalent
        moment: the limit-reached sentence must name what still works
        (history, search, export, settings — KEEP_FOREVER_LINE) before it names
        what stopped.

    Tests \`tests/usage_meter.rs\`: rollup is written in the transcript's
    transaction (kill the process between and assert neither exists); a local-day
    boundary rolls at local midnight, not UTC; a rolling 7-day window excludes
    day 8 exactly; \`LIMIT_ENABLED == false\` means no gate is ever consulted.

    What NOT to do:
      - Do NOT pick a number. Do NOT ship \`LIMIT_ENABLED = true\`.
      - Do NOT meter by wall-clock recording time. Voiced seconds and words are
        the units a user recognises; a paused hotkey is not consumption.
      - Do NOT send any usage figure anywhere. Local only, forever.
  `,
  acceptance: `
    grep -q 'fn usage_window' desktop/src-tauri/src/db.rs
    grep -q 'LIMIT_ENABLED' desktop/src-tauri/src/license.rs
    grep -qE 'LIMIT_ENABLED: *bool *= *false' desktop/src-tauri/src/license.rs
    grep -q 'limit' desktop/src/pill/license.ts
    test -f desktop/src-tauri/tests/usage_meter.rs
    grep -q 'rollup_is_written_in_the_transcript_transaction' desktop/src-tauri/tests/usage_meter.rs
    grep -q 'local_day_boundary_is_local_not_utc' desktop/src-tauri/tests/usage_meter.rs
    # nothing leaves the machine
    test 0 -eq "$(git grep -cE 'reqwest|http://|https://' -- desktop/src-tauri/src/db.rs | wc -l)"
    cd ${APP} && npm ci && npm test ; test $? -eq 0
    cd src-tauri && cargo test --features custom-protocol --test usage_meter ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'SEC-B', prompt: 'Y2', branch: 'loop/sec-b-trial-state-machine-hardening', gated: null,
  title: 'The trial state machine gets the adversarial tests its own doc comment promises',
  preflight: `
    test -f desktop/src-tauri/tests/trial_state_machine.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test trial_state_machine
  `,
  spec: `
    \`license.rs\` documents its own threat model carefully (license.rs:43-53:
    "A local trial clock on a machine the user controls is deterrence, not
    security"; :473-483 the two-store earliest-wins rule and the
    max-seen-wall-clock floor). The implementation looks right. What it lacks is
    a test file that drives the fortnight and the attacks end to end — and Y2's
    whole UI layer is about to depend on \`days_left\` being correct on every one
    of those days.

    Create \`desktop/src-tauri/tests/trial_state_machine.rs\` over
    \`evaluate_trial\` / \`decide_entitlement\` with the in-memory store
    (\`MemoryStore\`, license.rs:382 — it already exists for this purpose and
    counts writes, so the tests need no disk and no app):
      * \`first_run_starts_the_trial_and_reports_fourteen\`
      * \`each_day_reports_one_fewer_and_zero_is_the_last_day\` — all 15 values.
      * \`clock_rolled_back_cannot_rewind_the_trial\` — the wall-clock floor.
      * \`deleting_the_license_file_does_not_restart_the_trial\` — the DB store
        still holds the start (license.rs:361-363 names this exact attack).
      * \`deleting_the_db_row_does_not_restart_the_trial\` — the mirror case.
      * \`earlier_of_the_two_stores_wins\`
      * \`a_valid_license_beats_an_expired_trial\`
      * \`a_revoked_license_does_not_cancel_a_running_trial\` (license.rs:568-569
        states this; assert it).
      * \`expired_trial_stops_only_new_dictation\` — assert \`allows_new_dictation\`
        is false while nothing else in the entitlement changes. Pair it with the
        existing \`tests/license_gate.rs\` call-site sweep rather than repeating it.
      * \`trial_days_left_never_goes_negative\`
      * \`should_announce_gate_throttles\` — N presses produce one announcement.

    Then close the loop to the UI: a test that for every day 14..0 the Rust
    \`days_left\` and the TS \`daysLeft\`/\`trialCountdown\` agree. Do it by
    generating a small JSON fixture from the Rust test
    (\`desktop/src-tauri/tests/fixtures/trial_days.json\`) and reading it from a
    vitest case, so the two languages are pinned to one table instead of two
    hand-written ladders.

    What NOT to do:
      - Do NOT make the trial cryptographic. license.rs:43 already closed that:
        deterrence, not security. Rewriting it as DRM is out of scope and a
        product change nobody asked for.
      - Do NOT test by sleeping. Inject the clock.
    PANEL 2026-09-12 — the attack ladder is missing the direction that actually
    fires in the field. \`evaluate_trial\` sets
    \`effective_now = max(wall, monotonic, recorded_floor)\` (license.rs:490-496)
    and writes \`floor_ms = effective_now\` back to both stores (:521). A trial
    START in the future IS clamped (\`stored_start.unwrap_or(effective_now)
    .min(effective_now)\`, :505); the FLOOR is not. So ONE forward clock
    excursion — a restored Time Machine image, a bad NTP jump, a user who set
    the date forward once — permanently poisons the floor, the trial reads
    expired on day two, and the design deliberately removes every ordinary
    recovery (deleting the file or the row buys nothing, by earliest-wins). This
    loop then builds a countdown numeral, a hard \`gated\` pill state, a refused
    press and a purchase sheet on top of that latch, so a quiet backend bug
    becomes a surface telling a user who never had a fair trial to pay.
      * Clamp the floor the way the start is clamped: refuse to advance the
        recorded floor more than a few hours beyond the current wall clock, and
        RECORD the excursion instead of absorbing it.
      * Add \`forward_clock_excursion_does_not_expire_the_trial\` and
        \`a_poisoned_floor_can_be_cleared_by_a_signed_grace_claim\`.
      * Give support one non-DRM lever: the signature path already exists, so a
        signed grace/extension claim costs nothing and turns an unrecoverable
        lockout into an email. LIC-A uses the same claim.

  `,
  acceptance: `
    test -f desktop/src-tauri/tests/trial_state_machine.rs
    test -f desktop/src-tauri/tests/fixtures/trial_days.json
    grep -q 'clock_rolled_back_cannot_rewind_the_trial' desktop/src-tauri/tests/trial_state_machine.rs
    grep -q 'deleting_the_license_file_does_not_restart_the_trial' desktop/src-tauri/tests/trial_state_machine.rs
    grep -q 'a_revoked_license_does_not_cancel_a_running_trial' desktop/src-tauri/tests/trial_state_machine.rs
    grep -q 'trial_days' desktop/src/license/status.test.ts
    test 0 -eq "$(grep -c 'thread::sleep' desktop/src-tauri/tests/trial_state_machine.rs)"
    cd ${APP} && npm ci
    npm test -- license ; test $? -eq 0
    cd src-tauri && cargo test --features custom-protocol --test trial_state_machine ; test $? -eq 0
    cargo test --features custom-protocol --test license_gate                        ; test $? -eq 0    grep -q 'forward_clock_excursion_does_not_expire_the_trial' desktop/src-tauri/tests/trial_state_machine.rs
    grep -q 'a_poisoned_floor_can_be_cleared_by_a_signed_grace_claim' desktop/src-tauri/tests/trial_state_machine.rs

  `,
})
