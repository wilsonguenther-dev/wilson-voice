// Y7 — TESTS + A REAL SMOKE. The gate in docs/loop/HARNESS.md is eight commands
// and none of them launches the app. Yap has 128 Rust integration test files and
// 25 vitest tests, and Wilson's report ("the app looks broken") was invisible to
// all of them — a green build is not a working app
// (feedback_loop_blind_to_visual_ux; feedback_real_browser_smoke_required).
//
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'Y7-A', prompt: 'Y7', branch: 'loop/y7-a-headless-smoke-against-the-built-binary', gated: null,
  title: 'A headless smoke that runs the real built binary end to end, not a unit test of its parts',
  preflight: `
    test -x scripts/smoke-headless.sh
    ./scripts/smoke-headless.sh
  `,
  spec: `
    Yap ALREADY has the hook this needs and nothing uses it as a gate:
    \`src-tauri/src/cli.rs\` (161 lines) plus "YV32 headless mode
    (\`--transcribe-file <wav>\`)" at lib.rs:3982, and a committed fixture
    \`tests/fixtures/quick-brown-fox-16k.wav\`. So the built binary can be driven
    with no window, no TCC and no microphone.

    Create \`scripts/smoke-headless.sh\` (\`set -euo pipefail\`, exit codes read
    bare), which:
      1. Builds the release binary the way the gate already does (stage
         \`src-tauri/binaries/yap-polish-<triple>\` first, then
         \`cargo build --release --features custom-protocol\`).
      2. Runs the weak-link check that already exists:
         \`./scripts/assert-weak-linked-14_4-symbols.sh
          desktop/target/release/wilson-voice\`. Its comment in ci.yml is the
         reason this whole item matters: "the build is green, the tests are
         green, and the binary is unlaunchable for a whole population of users."
      3. \`--transcribe-file tests/fixtures/quick-brown-fox-16k.wav\` against a
         TEMPORARY data dir (never the user's \`WilsonVoice\` dir) and asserts the
         transcript contains the expected words. If the ASR model is absent it
         must FAIL with "no model installed, run <the documented command>" —
         never skip silently, which is how a smoke test becomes decoration.
      4. Asserts the run wrote NOTHING into the real data dir.
      5. Runs the same file through the cleanup pipeline at the SHIPPED default
         level and prints the before/after, so Y4's "formatting is on" claim is
         visible in the smoke output rather than only in a fixture.
      6. Prints a single PASS/FAIL summary and the version from tauri.conf.json.

    Add \`"smoke:headless"\` to desktop/package.json scripts. This is the
    per-item smoke; Y7-B is the windowed one.

    What NOT to do:
      - Do NOT touch the user's data dir or their \`/Applications/Yap.app\`.
      - Do NOT skip when the model is missing.
      - Do NOT download a model inside the smoke script.
  `,
  acceptance: `
    test -x scripts/smoke-headless.sh
    grep -q 'set -euo pipefail' scripts/smoke-headless.sh
    grep -q 'assert-weak-linked-14_4-symbols.sh' scripts/smoke-headless.sh
    grep -q 'transcribe-file' scripts/smoke-headless.sh
    test 0 -eq "$(grep -c 'WilsonVoice' scripts/smoke-headless.sh)"
    node -e "process.exit(require('./desktop/package.json').scripts['smoke:headless']?0:1)"
    ./scripts/smoke-headless.sh ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y7-B', prompt: 'Y7', branch: 'loop/y7-b-windowed-smoke-that-screenshots-every-view', gated: null,
  title: 'A windowed smoke that launches Yap, walks all seven views and captures them at two sizes',
  preflight: `
    test -x scripts/smoke-windowed.sh
    test -d docs/smoke-shots
    ./scripts/smoke-windowed.sh --check-only
  `,
  spec: `
    Wilson's report was visual and nothing in the gate looks at pixels. This is
    the item that makes "looks broken" a detectable condition.

    \`scripts/smoke-windowed.sh\`:
      * Launches the built app against a TEMPORARY data dir, with a flag that
        seeds a deterministic fixture state (some history, a dictionary entry, a
        scratchpad note) and a SECOND run with an EMPTY state — the empty run is
        the one that catches Y5-B's missing empty states, and it is the more
        important of the two.
      * Walks all seven \`Nav\` views and all eight \`SettingsTab\`s, capturing
        each at 980x700 and at 720x520 (the configured default and the
        \`minWidth\`/\`minHeight\` floor from tauri.conf.json), into
        \`docs/smoke-shots/<run>/\`.
      * Captures the FLOAT PILL for every \`LivePhase\` at all three
        \`pill_position\` values, driven through a debug command that forces a
        phase (add one behind \`#[cfg(feature = "custom-protocol")]\` plus an env
        guard so it cannot be reached in a shipped build — and assert that).
      * FAILS, not warns, on: a view that renders no text at all; a view with an
        \`.animate\`/spinner element still present after 10 seconds; a horizontal
        scrollbar on the window at either size; any element whose bounding box
        extends past the window; and, in the empty run, a view with no
        \`data-empty-state\`. These are mechanical proxies for "looks broken" and
        each one corresponds to a defect this plan found.
      * \`--check-only\` runs the assertions against the last captured run
        without relaunching, so the preflight is cheap.

    Use the Playwright/CDP route only if the Tauri webview exposes a debug port
    in a \`custom-protocol\` build; if it does not, drive it with the OS
    screenshot tools plus the app's own debug commands and say so in the script
    header. Either way, do not add a browser automation dependency to
    desktop/package.json's runtime deps.

    Wire it in: \`npm run smoke:windowed\`, and make Y0-B's loop-smoke script
    call it when a display is available and skip it with a named message when
    there is none.

    Depends on Y5-B, Y5-C, Y5-G, Y7-A.

    What NOT to do:
      - Do NOT commit the screenshots into git history on every run. Commit ONE
        reference run and gitignore the rest, or the repo grows without bound.
      - Do NOT leave the forced-phase debug command reachable in a release build.
      - Do NOT assert pixel equality against a golden image. Assert the
        STRUCTURAL properties above; pixel goldens on a two-theme, two-size,
        animated UI are a permanent source of false red.
    PANEL 2026-09-12 — THIS ITEM IS NOW THE SECOND HALF. Y0-E ships
    scripts/smoke-windowed.sh with the five structural failure conditions BEFORE
    the Y5 lane, because an instrument that arrives ten items after the work it
    judges gates nothing, and build mode dispatches no reviewer. So the
    dependency line "Depends on Y5-B, Y5-C, Y5-G, Y7-A" now means: EXTEND Y0-E's
    script to the new pill phases and the dock positions those items added, and
    re-baseline docs/loop/SMOKE-BASELINE.md against the post-Y5 tree with the
    diff explained. It launches under \`YAP_DATA_DIR\` and \`--smoke\` (Y0-D) —
    never against the real data dir, and never with a global hotkey registered.
    \`--check-only\` remains a pre-flight convenience and is NOT acceptance:
    with no captured run it exits non-zero by Y0-E's contract.
    A display IS available on this run, so "no display" is a failure here, not
    a named skip.

  `,
  acceptance: `
    test -x scripts/smoke-windowed.sh
    grep -q '720' scripts/smoke-windowed.sh
    grep -q '980' scripts/smoke-windowed.sh
    grep -q 'data-empty-state' scripts/smoke-windowed.sh
    grep -q 'check-only' scripts/smoke-windowed.sh
    test -d docs/smoke-shots
    node -e "process.exit(require('./desktop/package.json').scripts['smoke:windowed']?0:1)"
    grep -q 'smoke-windowed' scripts/loop-smoke.sh
    # the forced-phase debug command cannot exist in a shipped build
    grep -q 'custom-protocol' desktop/src-tauri/src/lib.rs
    # PANEL: --check-only asserts against "the last captured run", so with no
    # run it is a no-op that exits 0. Acceptance runs a FULL capture, counts the
    # shots, and proves the detector can fail.
    YAP_DATA_DIR="$(mktemp -d)" ./scripts/smoke-windowed.sh ; test $? -eq 0
    test 28 -le "$(ls docs/smoke-shots/*/*.png | wc -l | tr -d ' ')"
    ./scripts/smoke-windowed.sh --check-only --self-test-must-fail ; test $? -ne 0
  `,
})

ITEMS.push({
  id: 'Y7-C', prompt: 'Y7', branch: 'loop/y7-c-non-vacuous-mutation-proof-for-the-new-tests', gated: null,
  title: 'Every test this loop added is proven non-vacuous by a mutation that makes it fail',
  preflight: `
    test -f docs/loop/MUTATIONS.md
    test -x scripts/assert-tests-are-non-vacuous.sh
    ./scripts/assert-tests-are-non-vacuous.sh
  `,
  spec: `
    The repo already practises this: \`docs/pr-screenshots/YV105/…/non-vacuous-mutations.txt\`,
    YV106, YV107, YV120, YV121 all carry one. Make it mechanical for the tests
    this loop adds, because the failure mode is specific and this plan is full of
    greps: a test that asserts a symbol exists, in a file that always contains
    it, proves nothing; and a grep proving absence with the wrong pattern or the
    wrong scope proves less than nothing (the "verification that verifies
    nothing" rule).

    Create \`scripts/assert-tests-are-non-vacuous.sh\` driven by a committed
    table \`docs/loop/MUTATIONS.md\`: one row per test file added by this loop,
    naming a SINGLE-LINE source mutation and the test that must then fail.
    The script applies each mutation to a scratch copy, runs that one test,
    asserts a NON-ZERO exit, and reverts. It fails if any mutation leaves the
    suite green.

    Seed the table with the mutations named in the earlier items, which are
    already written as acceptance steps there and should move here so they run
    together:
      shipped_defaults      flip \`auto_paste: true\` -> false
      formatting_fixtures   set \`cleanup_level\` back to "light"
      mic_auth_status       return \`Authorized\` unconditionally from
                            \`authorization_status()\`
      mic_gate              delete the microphone check from \`start_recording\`
      trial_state_machine   drop the max-seen-wall-clock floor
      dictation_chunked     remove the seam dedupe
      dictation_capture_memory  restore the unbounded \`raw\` append
      polish_long_form      restore \`MAX_POLISH_WORDS = 400\`
      paste_target_e2e      cache the paste target from hold time
      updater_endpoint      reduce the endpoint list to one
      no_outbound_on_the_dictation_path  add a URL literal to \`record.rs\`
      history_at_volume     remove the pagination tiebreaker
      lifecycle_e2e         skip the mute restore on one exit path

    Also assert the SHAPE of every grep-based acceptance this loop uses: a
    committed checker that scans the item files for \`grep\` invocations without
    a path scope, and fails. A scopeless grep in an acceptance gate is the
    single cheapest way to ship a false green.

    Run it as part of Y0-B's loop-smoke script, gated behind a flag so the
    per-item gate stays fast and the full mutation sweep runs once per part.

    Depends on every test-bearing item; sequence it last in its file.

    What NOT to do:
      - Do NOT mutate the test file to make it fail. Mutate the SOURCE.
      - Do NOT accept "the whole suite went red" as proof. The NAMED test must
        be the one that fails.
    PANEL 2026-09-12 — the item whose purpose is proving other tests non-vacuous
    was itself satisfiable by typing a 13-row markdown table, and it is
    sequenced last, so it can only audit tests that have already merged. Two
    changes: (1) the per-item mutation requirement now lives in the SHARED
    PREAMBLE and binds every item as it is built — this item COLLECTS the rows
    and re-runs them, it does not excuse anyone; (2) the ledger is one row per
    NEW TEST FILE, and scripts/assert-tests-are-non-vacuous.sh must EXECUTE each
    mutation (mutate, re-run, require red, restore, \`git diff --exit-code\`) and
    record the observed transition, not merely list it. A row whose mutation was
    never executed is a failed row.

  `,
  acceptance: `
    test -f docs/loop/MUTATIONS.md
    test -x scripts/assert-tests-are-non-vacuous.sh
    # PANEL: 13 rows is not a ledger for a loop that adds ~50 test files, and a
    # markdown table is not a proof. One row per new test FILE, and the script
    # must EXECUTE each mutation and record the observed red/green transition.
    test "$(ls desktop/src-tauri/tests/*.rs | wc -l | tr -d ' ')" -le "$(grep -c '^| ' docs/loop/MUTATIONS.md)"
    grep -q 'executed' docs/loop/MUTATIONS.md
    grep -q 'git diff --exit-code' scripts/assert-tests-are-non-vacuous.sh
    grep -q 'mic_auth_status' docs/loop/MUTATIONS.md
    grep -q 'no_outbound_on_the_dictation_path' docs/loop/MUTATIONS.md
    grep -q 'scopeless' scripts/assert-tests-are-non-vacuous.sh
    ./scripts/assert-tests-are-non-vacuous.sh ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y7-D', prompt: 'Y7', branch: 'loop/y7-d-frontend-coverage-for-the-pure-modules', gated: null,
  title: 'The pure frontend modules get real coverage, so the pill and the states are testable without a window',
  preflight: `
    test -f desktop/vitest.config.ts
    cd ${APP} && npm ci && npm test
  `,
  spec: `
    The frontend is 25 vitest tests across \`src/pill/live.test.ts\`,
    \`src/errors.test.ts\`, \`src/license/status.test.ts\`,
    \`src/meetings/*.test.ts\` and \`src/support/bundle.test.ts\`. The pattern is
    right — pure module, pure test, no rendering — and ci.yml explains why it is
    a gate: "a behavioural regression there shipped once because a type check was
    the only frontend gate."

    This loop adds a lot of pure modules (permission.ts, viewState.ts, toast.ts,
    diff.ts, onboarding.ts, pill/license.ts, pill/motion.ts, pill/hitbox.ts,
    pill/dock.ts, home/house.ts). Make the discipline enforceable:
      * \`desktop/vitest.config.ts\` with coverage thresholds that apply ONLY to
        the pure modules (an explicit include list — never a repo-wide number,
        which would either be trivially met or permanently red because App.tsx
        cannot be unit-tested).
      * A test that fails when a new file is added under \`src/pill/\` or a new
        \`*.ts\` pure module is added without a sibling \`*.test.ts\`. An explicit
        include list plus that check is what keeps the number honest.
      * Fix the reverse problem too: assert no pure module imports
        \`@tauri-apps/api\` — a pure module that invokes is not testable without a
        window, and that is how \`live.ts\` stays drivable. Components may import
        it; \`*.ts\` modules on the include list may not.

    Depends on Y5-*, Y6-A. Sequence after them.

    What NOT to do:
      - Do NOT set a global coverage threshold.
      - Do NOT add a DOM testing library to chase a number. The value here is
        the pure state machines, which need no DOM.
  `,
  acceptance: `
    test -f desktop/vitest.config.ts
    grep -q 'coverage' desktop/vitest.config.ts
    grep -q 'pill/live.ts' desktop/vitest.config.ts
    test -f desktop/src/purity.test.ts
    grep -q 'no_pure_module_imports_the_tauri_api' desktop/src/purity.test.ts
    grep -q 'every_pure_module_has_a_sibling_test' desktop/src/purity.test.ts
    cd ${APP} && npm ci
    npm test         ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y7-E', prompt: 'Y7', branch: 'loop/y7-e-release-dmg-smoke-on-a-clean-mac-path', gated: null,
  title: 'The shipped DMG is smoke-tested the way a first-time user meets it',
  preflight: `
    test -x scripts/smoke-dmg.sh
    grep -q 'smoke-dmg' docs/RELEASE.md
  `,
  spec: `
    The release path is real: \`.github/workflows/release.yml\` with six repo
    secrets set, a Developer ID certificate, notarization, and
    \`reference_yap_dmg_notarization\` recording a notarized v0.5.5 DMG plus the
    manual flow and the iCloud/keychain gotchas. v0.8.0 shipped with "the first
    working auto-update" (project_yap_build_state). None of that is smoke-tested
    from the user's side, and SEC-A + UPD-A both change things that only show up
    there.

    \`scripts/smoke-dmg.sh <path-to-dmg>\`, run manually and from docs/RELEASE.md:
      * \`spctl --assess --type exec -vv\` on the .app inside the mounted DMG ->
        accepted, source "Notarized Developer ID". An un-notarized build is a
        Gatekeeper wall for every user and nothing else in the pipeline sees it.
      * \`codesign -dv --verbose=4\` -> the signing identity is a Developer ID,
        not ad-hoc, and the team id matches what docs/RELEASE.md documents.
      * \`codesign -d --entitlements :-\` -> assert
        \`com.apple.security.app-sandbox\` is present and FALSE (the VALUE, not
        the key — SEC-A's rule), audio-input is true, and the two
        dylib-injection entitlements are absent.
      * The bundle identifier is \`com.wilsonguenther.wilson-voice\` — assert it
        has NOT changed. A rename silently resets every user's TCC grants.
      * Both sidecars are present inside the bundle's Resources and are
        themselves signed.
      * \`Info.plist\` carries all three usage strings (NSMicrophone,
        NSAudioCapture, NSAppleEvents). A missing one is a TCC failure with no
        dialog.
      * The updater manifest URL from UPD-A resolves and its signature verifies
        against the shipped pubkey — without installing anything.
      * \`xattr\` shows no quarantine-blocking detritus, and the resource-fork
        problem project_yap_build_state describes ("the tauri codesign flakes on
        resource-fork detritus") is checked for by name.
      * Prints nothing secret. No certificate serial, no key material, no
        app-specific password. The script must be safe to paste into a PR.

    Then wire it into docs/RELEASE.md as a required step before a release is
    announced, with the exact command.

    Depends on SEC-A, UPD-A.

    What NOT to do:
      - Do NOT run this in the per-item gate. It needs a built, signed,
        notarized DMG, which is minutes plus Apple's servers; it belongs to the
        release checklist and to \`mode: "review"\` with \`args: {dmg: true}\`.
      - Do NOT print any secret or certificate detail.
      - Do NOT install the DMG over the user's running /Applications/Yap.app.
    PANEL 2026-09-12 — \`bash -n\` is a syntax check, not a smoke test: the
    checklist this item writes (spctl assess, codesign verbose, entitlement
    values, bundle id, sidecar signatures, Info.plist usage strings, the updater
    manifest resolving) was never once executed. Run it for real against the
    DMG the review pass builds (\`args: {dmg: true}\`), or skip with a NAMED
    reason the PR body carries. And split the assertions into SEC-A's two
    profiles: a RELEASE profile (Developer ID + notarized + stapled) and a LOCAL
    profile (Apple Development, unnotarized, stable designated requirement) —
    asserting "Notarized Developer ID" unconditionally makes a correctly signed
    local DMG fail by construction. The updater assertions here are UPD-B's
    published triple (latest.json + .app.tar.gz + .sig), not the DMG alone.

  `,
  acceptance: `
    test -x scripts/smoke-dmg.sh
    grep -q 'spctl --assess' scripts/smoke-dmg.sh
    grep -q 'app-sandbox' scripts/smoke-dmg.sh
    grep -q 'com.wilsonguenther.wilson-voice' scripts/smoke-dmg.sh
    grep -q 'NSAudioCaptureUsageDescription' scripts/smoke-dmg.sh
    grep -q 'smoke-dmg' docs/RELEASE.md
    test 0 -eq "$(grep -c 'APPLE_PASSWORD' scripts/smoke-dmg.sh)"
    # PANEL: \`bash -n\` is a syntax check, not a smoke test. Run it against a
    # real artifact, or skip with a NAMED reason that the PR body carries.
    bash -n scripts/smoke-dmg.sh ; test $? -eq 0
    ./scripts/smoke-dmg.sh --require-artifact-or-name-the-reason ; test $? -eq 0
    grep -q 'codesign -dv' scripts/smoke-dmg.sh
    grep -q 'adhoc' scripts/smoke-dmg.sh
  `,
})
